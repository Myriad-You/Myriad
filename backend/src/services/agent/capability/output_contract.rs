//! Capability output contract.
//!
//! `Capability::output_schema` used to be declared on every capability and read
//! by nothing. Two consequences:
//!
//! - The planner could not see what a step produces, so cross-step references
//!   (`"dataFrom": "search"`) were untyped guesses resolved by per-capability
//!   fallbacks at execution time.
//! - A handler returning the wrong shape ("hollow success") flowed straight into
//!   downstream steps, and only surfaced later as an escalation heuristic.
//!
//! This module makes the declaration load-bearing:
//!
//! - [`declared_output_fields`] feeds the compact capability index, so the
//!   planner can reference a concrete field (`"dataFrom": "search.results"`).
//! - [`check_output_contract`] runs after a step completes.
//!
//! **Runtime:** [`ContractViolation::Breach`] fails the step. [`ContractViolation::Drift`]
//! is still log-only. CI sample-output table asserts covered capabilities
//! produce no Breach.
//!
//! The two severities therefore describe *confidence*, not runtime behaviour:
//!
//! - [`ContractViolation::Breach`] — a declared field is present with the wrong
//!   JSON type, a `required` field is missing, or an enum/range bound is
//!   exceeded. Unambiguous: one side is definitely wrong, so CI rejects it.
//! - [`ContractViolation::Drift`] — the handler returned an object sharing no
//!   key at all with the declared properties. Logged only, not a step failure:
//!   remaining mismatches belong in the registry, not as a runtime abort.

use serde_json::Value;

/// Placeholder type used by a few schemas (`{"type": "any"}`) for genuinely
/// unconstrained payloads. The shared subset validator has no such type and
/// would reject every value, so these annotations are stripped before use.
const ANY_TYPE: &str = "any";

/// A mismatch between a step's output and its capability's declared contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContractViolation {
    /// The output contradicts the schema. Unambiguous — CI fails on it.
    Breach(String),
    /// The output is schema-clean but carries none of the declared fields —
    /// the declaration and the handler have drifted apart. Logged only.
    Drift(String),
}

impl ContractViolation {
    /// Whether the mismatch is unambiguous enough to gate on.
    ///
    /// Only selects the log message at runtime (see the module docs); the CI
    /// sample table is what actually rejects a `Breach`.
    pub fn is_fatal(&self) -> bool {
        matches!(self, ContractViolation::Breach(_))
    }

    pub fn message(&self) -> &str {
        match self {
            ContractViolation::Breach(message) | ContractViolation::Drift(message) => message,
        }
    }
}

/// Top-level output field names a capability declares.
///
/// Returned to the planner as the `"o"` entry of the compact index so it can
/// address a specific field instead of the whole step output.
pub fn declared_output_fields(schema: &Value) -> Vec<String> {
    schema
        .get("properties")
        .and_then(Value::as_object)
        .map(|properties| properties.keys().cloned().collect())
        .unwrap_or_default()
}

/// Remove `{"type": "any"}` annotations so unconstrained nodes validate as
/// unconstrained rather than as an unknown (and therefore failing) type.
fn strip_any_types(schema: &Value) -> Value {
    match schema {
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(key, value)| {
                    !(key.as_str() == "type" && value.as_str() == Some(ANY_TYPE))
                })
                .map(|(key, value)| (key.clone(), strip_any_types(value)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(strip_any_types).collect()),
        other => other.clone(),
    }
}

/// Check a step output against its capability's declared `output_schema`.
///
/// Returns `None` when the capability declares no usable contract (an empty
/// schema, which is the `Capability::default()` value).
pub fn check_output_contract(schema: &Value, output: &Value) -> Option<ContractViolation> {
    let object = schema.as_object()?;
    if object.is_empty() {
        return None;
    }

    if let Err(error) =
        myriad_json_schema::validate_inline_json_value(&strip_any_types(schema), output)
    {
        return Some(ContractViolation::Breach(format!(
            "输出不符合声明的 output_schema：{error}"
        )));
    }

    // Hollow success: the schema promises named fields and the handler returned
    // an object carrying none of them, so every downstream `xxxFrom` reference
    // into this step resolves to nothing.
    let declared = object.get("properties").and_then(Value::as_object)?;
    if declared.is_empty() {
        return None;
    }
    let actual = output.as_object()?;
    if actual.keys().any(|key| declared.contains_key(key)) {
        return None;
    }

    Some(ContractViolation::Drift(format!(
        "输出未包含任何声明字段（声明 [{}]，实际 [{}]）",
        join_keys(declared.keys()),
        join_keys(actual.keys())
    )))
}

fn join_keys<'a>(keys: impl Iterator<Item = &'a String>) -> String {
    keys.map(String::as_str).collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn summarize_schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "summary": { "type": "string" },
                "keyPoints": { "type": "array" }
            }
        })
    }

    #[test]
    fn declared_fields_feed_the_compact_index() {
        let mut fields = declared_output_fields(&summarize_schema());
        fields.sort();
        assert_eq!(fields, vec!["keyPoints".to_string(), "summary".to_string()]);
    }

    #[test]
    fn declared_fields_empty_without_properties() {
        assert!(declared_output_fields(&json!({})).is_empty());
        assert!(declared_output_fields(&json!({ "type": "string" })).is_empty());
    }

    #[test]
    fn conforming_output_passes() {
        let output = json!({ "summary": "内容摘要", "keyPoints": ["a", "b"] });
        assert!(check_output_contract(&summarize_schema(), &output).is_none());
    }

    #[test]
    fn extra_undeclared_fields_are_tolerated() {
        // Handlers routinely add envelope fields; only declared ones are checked.
        let output = json!({ "summary": "x", "tookMs": 12, "source": "cache" });
        assert!(check_output_contract(&summarize_schema(), &output).is_none());
    }

    #[test]
    fn wrong_type_on_declared_field_is_a_fatal_breach() {
        let output = json!({ "summary": 42 });
        let violation = check_output_contract(&summarize_schema(), &output)
            .expect("type mismatch must be reported");
        assert!(violation.is_fatal());
        assert!(violation.message().contains("output_schema"));
    }

    #[test]
    fn non_object_output_is_a_fatal_breach() {
        let violation = check_output_contract(&summarize_schema(), &json!("just text"))
            .expect("object schema must reject a bare string");
        assert!(violation.is_fatal());
    }

    #[test]
    fn missing_required_field_is_a_fatal_breach() {
        let schema = json!({
            "type": "object",
            "properties": { "imageUrl": { "type": "string" } },
            "required": ["imageUrl"]
        });
        let violation = check_output_contract(&schema, &json!({ "imageUrl2": "x" }))
            .expect("missing required field must be reported");
        assert!(violation.is_fatal());
    }

    #[test]
    fn hollow_output_is_reported_as_non_fatal_drift() {
        // The exact shape `steam.user` returns on its degraded path today.
        let output = json!({ "message": "未连接", "hint": "先绑定账号" });
        let violation = check_output_contract(&summarize_schema(), &output)
            .expect("zero declared fields must be reported");
        assert!(!violation.is_fatal());
        assert!(violation.message().contains("summary"));
    }

    #[test]
    fn empty_output_object_is_drift_not_success() {
        let violation = check_output_contract(&summarize_schema(), &json!({}))
            .expect("an empty object satisfies nothing");
        assert!(!violation.is_fatal());
    }

    #[test]
    fn empty_schema_is_unconstrained() {
        // Capability::default() leaves output_schema as `{}`.
        assert!(check_output_contract(&json!({}), &json!({ "anything": 1 })).is_none());
    }

    #[test]
    fn any_typed_fields_are_unconstrained() {
        // `http.fetch` declares `data: { "type": "any" }`.
        let schema = json!({
            "type": "object",
            "properties": {
                "status": { "type": "integer" },
                "data": { "type": ANY_TYPE }
            }
        });
        assert!(
            check_output_contract(&schema, &json!({ "status": 200, "data": [1, 2] })).is_none()
        );
        assert!(
            check_output_contract(&schema, &json!({ "status": 200, "data": "text" })).is_none()
        );
        // Stripping `any` must not weaken the sibling constraints.
        let violation = check_output_contract(&schema, &json!({ "status": "200" }))
            .expect("integer field must still be enforced");
        assert!(violation.is_fatal());
    }

    #[test]
    fn nested_any_is_stripped_at_depth() {
        let schema = json!({
            "type": "object",
            "properties": {
                "wrapper": {
                    "type": "object",
                    "properties": { "payload": { "type": ANY_TYPE } }
                }
            }
        });
        let output = json!({ "wrapper": { "payload": { "nested": true } } });
        assert!(check_output_contract(&schema, &output).is_none());
    }

    /// Sample outputs mirroring what each handler actually returns on success.
    ///
    /// Runtime now fails on Breach. This table keeps sampled handlers honest in CI.
    ///
    /// Add a row when you add a capability. Two rows for a handler that returns
    /// different shapes on different branches.
    fn ai_handler_samples() -> Vec<(&'static str, Value)> {
        vec![
            // execute_ai_summarize
            (
                "ai.summarize",
                json!({ "summary": "摘要正文", "style": "brief" }),
            ),
            // execute_ai_analyze — `analysis` is the model's prose, not a struct
            (
                "ai.analyze",
                json!({ "analysis": "分析正文", "type": "custom" }),
            ),
            // execute_ai_recommend — array when JSON extraction succeeds…
            (
                "ai.recommend",
                json!({ "recommendations": [{ "name": "x", "reason": "y" }], "count": 5 }),
            ),
            // …and the raw string when it does not
            (
                "ai.recommend",
                json!({ "recommendations": "推荐正文", "count": 5 }),
            ),
            // execute_ai_chat
            ("ai.chat", json!({ "reply": "回复正文" })),
            // execute_ai_image
            (
                "ai.image",
                json!({
                    "imageUrl": "https://example.invalid/a.png",
                    "width": 1024,
                    "height": 768,
                    "provider": "openrouter"
                }),
            ),
            // execute_gemini_grounding_search_wrapper (ai.webSearch / groundingSearch)
            (
                "ai.webSearch",
                json!({
                    "success": true,
                    "query": "q",
                    "searchType": "web",
                    "aiSummary": "s",
                    "results": [],
                    "totalResults": 0
                }),
            ),
            // groundingSearch shares the same wrapper, so the same shape
            (
                "ai.groundingSearch",
                json!({
                    "success": true,
                    "query": "q",
                    "searchType": "general",
                    "aiSummary": "s",
                    "results": [],
                    "totalResults": 0
                }),
            ),
            // execute_brewlia_annotate — annotations is always an array
            (
                "brewlia.annotate",
                json!({ "annotations": [], "fromCache": false, "itemId": 1 }),
            ),
            // execute_brewlia_podcast — duration is chars/200 as f64
            (
                "brewlia.podcast",
                json!({ "script": "台本", "duration": 3.5, "style": "dialogue", "itemId": 1 }),
            ),
            // execute_prompt_generate
            (
                "prompt.generate",
                json!({ "prompt": "p", "negativePrompt": "n", "title": "t" }),
            ),
            // execute_translate_text
            (
                "translate.text",
                json!({
                    "originalText": "a",
                    "translated": "b",
                    "targetLang": "en",
                    "sourceLang": "zh"
                }),
            ),
            // execute_code_explain
            (
                "code.explain",
                json!({
                    "code": "fn main() {}",
                    "language": "rust",
                    "explanation": "e",
                    "complexity": "simple"
                }),
            ),
            // execute_icon_recommend
            (
                "icon.recommend",
                json!({
                    "platformName": "github",
                    "iconType": "brand",
                    "iconName": "github",
                    "colorSuggestion": "#181717"
                }),
            ),
            // execute_speech_tts
            (
                "speech.tts",
                json!({
                    "success": true,
                    "audio": "AAAA",
                    "audioBase64": "AAAA",
                    "codec": "mp3",
                    "cached": false,
                    "duration": 1.25,
                    "voice": "v",
                    "textLength": 4
                }),
            ),
        ]
    }

    #[tokio::test]
    async fn declared_schemas_accept_real_handler_output() {
        let registry = crate::services::agent::capability::get_registry().await;
        let mut breaches = Vec::new();
        let mut drifts = Vec::new();

        for (capability_id, sample) in ai_handler_samples() {
            let capability = registry
                .get(capability_id)
                .unwrap_or_else(|| panic!("{capability_id} must be registered"));
            match check_output_contract(&capability.output_schema, &sample) {
                Some(violation) if violation.is_fatal() => {
                    breaches.push(format!("{capability_id}: {}", violation.message()));
                }
                Some(violation) => drifts.push(format!("{capability_id}: {}", violation.message())),
                None => {}
            }
        }

        assert!(
            breaches.is_empty(),
            "a declared output_schema contradicts what its handler returns; fix the declaration \
             (or the handler) rather than loosening this test:\n{}",
            breaches.join("\n")
        );
        assert!(
            drifts.is_empty(),
            "these declarations share no field with their handler's output:\n{}",
            drifts.join("\n")
        );
    }

    #[tokio::test]
    async fn sampled_capabilities_declare_the_fields_the_planner_will_reference() {
        // `declared_output_fields` feeds the planner's `o` index. A declared field
        // the handler never emits sends the planner after data that cannot exist —
        // which is how `ai.analyze` came to advertise `insights` and `confidence`.
        let registry = crate::services::agent::capability::get_registry().await;
        let mut phantom = Vec::new();

        for (capability_id, sample) in ai_handler_samples() {
            let capability = registry.get(capability_id).expect("registered");
            let actual = sample.as_object().expect("object sample");
            for field in declared_output_fields(&capability.output_schema) {
                if !actual.contains_key(&field) {
                    phantom.push(format!("{capability_id}.{field}"));
                }
            }
        }

        assert!(
            phantom.is_empty(),
            "declared output fields never produced by the handler: {phantom:?}"
        );
    }

    #[test]
    fn enum_and_range_bounds_are_enforced() {
        let schema = json!({
            "type": "object",
            "properties": {
                "pageType": { "type": "string", "enum": ["home", "brew"] },
                "width": { "type": "integer", "minimum": 256, "maximum": 2048 }
            }
        });
        assert!(
            check_output_contract(&schema, &json!({ "pageType": "brew", "width": 1024 })).is_none()
        );
        assert!(
            check_output_contract(&schema, &json!({ "pageType": "unknown" }))
                .is_some_and(|v| v.is_fatal())
        );
        assert!(
            check_output_contract(&schema, &json!({ "width": 4096 })).is_some_and(|v| v.is_fatal())
        );
    }
}
