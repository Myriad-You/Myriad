use super::types::{GeminiGenerationConfig, OutputBudget};

/// How a JSON request is expressed to the provider.
#[derive(Clone, Copy)]
pub(super) enum JsonMode<'a> {
    /// Use the provider's native structured-output parameters.
    Structured(Option<&'a serde_json::Value>),
    /// Compatibility fallback for endpoints that reject those parameters: no
    /// structured-output fields, with the JSON contract restated in the prompt
    /// the way the pre-existing call sites did it.
    PromptOnly(Option<&'a serde_json::Value>),
}

impl JsonMode<'_> {
    pub(super) fn decorate_prompt(self, prompt: &str) -> String {
        let Self::PromptOnly(schema) = self else {
            return prompt.to_string();
        };
        let mut text = prompt.to_string();
        text.push_str("\n\nReturn one valid JSON value only, without Markdown fences.");
        if let Some(schema) = schema {
            text.push_str(" The JSON value must satisfy this schema:\n");
            text.push_str(&schema.to_string());
        }
        text
    }

    pub(super) fn gemini_generation_config(
        self,
        budget: Option<OutputBudget>,
    ) -> Option<GeminiGenerationConfig> {
        let structured = match self {
            Self::Structured(schema) => Some(schema.and_then(gemini_response_schema)),
            Self::PromptOnly(_) => None,
        };
        // 关思考不依赖结构化输出：退回 prompt-only 时预算还在。
        match (structured, budget) {
            (None, None) => None,
            (structured, budget) => Some(GeminiGenerationConfig {
                response_mime_type: if structured.is_some() {
                    "application/json".to_string()
                } else {
                    "text/plain".to_string()
                },
                response_schema: structured.flatten(),
                max_output_tokens: budget.map(|b| b.max_tokens),
            }),
        }
    }

    pub(super) fn openai_response_format(self, schema_name: &str) -> Option<serde_json::Value> {
        match self {
            // `strict` stays false: strict mode forbids free-form objects, and
            // planner step `params` is exactly that. Non-strict still guarantees
            // syntactically valid JSON and guides the shape.
            Self::Structured(Some(schema)) => Some(serde_json::json!({
                "type": "json_schema",
                "json_schema": {
                    "name": schema_name,
                    "schema": schema,
                    "strict": false,
                },
            })),
            Self::Structured(None) => Some(serde_json::json!({ "type": "json_object" })),
            Self::PromptOnly(_) => None,
        }
    }
}

/// Keys Gemini accepts inside `responseSchema` (an OpenAPI 3.0 subset).
/// Anything else — `additionalProperties`, `$schema`, `default`, `const` — is
/// rejected with a 400, so unknown keys are dropped rather than forwarded.
const GEMINI_SCHEMA_KEYS: &[&str] = &[
    "type",
    "format",
    "description",
    "nullable",
    "enum",
    "items",
    "properties",
    "required",
    "minItems",
    "maxItems",
];

/// Translate a JSON Schema into Gemini's `responseSchema` dialect.
///
/// Returns `None` when the schema cannot be expressed, in which case the caller
/// keeps `responseMimeType` (still guaranteeing valid JSON) and drops the
/// schema. The main inexpressible case is a free-form object: Gemini rejects an
/// `OBJECT` node without `properties`, which is how open maps like a planner
/// step's `params` are declared.
fn gemini_response_schema(schema: &serde_json::Value) -> Option<serde_json::Value> {
    let map = schema.as_object()?;

    let declared_type = map.get("type").and_then(serde_json::Value::as_str);
    if declared_type == Some("object")
        && map
            .get("properties")
            .and_then(serde_json::Value::as_object)
            .is_none_or(|properties| properties.is_empty())
    {
        return None;
    }

    let mut translated = serde_json::Map::new();
    for (key, value) in map {
        if !GEMINI_SCHEMA_KEYS.contains(&key.as_str()) {
            continue;
        }
        let translated_value = match key.as_str() {
            // Gemini's Type enum is upper case (STRING / OBJECT / ARRAY / ...).
            "type" => serde_json::Value::String(value.as_str()?.to_uppercase()),
            "items" => gemini_response_schema(value)?,
            "properties" => {
                let mut properties = serde_json::Map::new();
                for (name, property) in value.as_object()? {
                    properties.insert(name.clone(), gemini_response_schema(property)?);
                }
                serde_json::Value::Object(properties)
            }
            _ => value.clone(),
        };
        translated.insert(key.clone(), translated_value);
    }
    Some(serde_json::Value::Object(translated))
}

#[cfg(test)]
mod tests {
    use super::{gemini_response_schema, JsonMode};
    use crate::services::analyzer::types::{OpenAIMessage, OpenAIRequest, OutputBudget};
    use serde_json::json;

    fn openai_body(mode: JsonMode<'_>, budget: Option<OutputBudget>) -> serde_json::Value {
        serde_json::to_value(OpenAIRequest {
            model: "m".to_string(),
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                content: "hi".to_string(),
            }],
            response_format: mode.openai_response_format("s"),
            max_tokens: budget.map(|b| b.max_tokens),
        })
        .expect("serialize")
    }

    /// 没有预算的调用，报文必须和加这个功能之前一模一样。
    #[test]
    fn an_unbudgeted_call_sends_no_new_fields() {
        let body = openai_body(JsonMode::PromptOnly(None), None);
        let object = body.as_object().expect("object");
        assert_eq!(
            object.keys().map(String::as_str).collect::<Vec<_>>(),
            // serde_json 这里是 BTreeMap，键按字典序。
            vec!["messages", "model"],
            "普通调用的报文不该多出字段"
        );

        let config = JsonMode::PromptOnly(None).gemini_generation_config(None);
        assert!(
            config.is_none(),
            "没有结构化也没有预算时不该发 generationConfig"
        );
    }

    #[test]
    fn a_budgeted_call_caps_output_and_nothing_else() {
        let budget = OutputBudget { max_tokens: 2048 };
        let body = openai_body(JsonMode::Structured(None), Some(budget));
        assert_eq!(body["max_tokens"], json!(2048));
        assert_eq!(body["response_format"], json!({ "type": "json_object" }));
        // 关思考的参数各家不同，猜一个发出去会换来 4xx 和一次多余的重试。
        // 要加就得先按 base_url 认出网关。
        assert!(body.get("reasoning_effort").is_none());

        let config = JsonMode::Structured(None)
            .gemini_generation_config(Some(budget))
            .expect("generationConfig");
        let config = serde_json::to_value(config).expect("serialize");
        assert_eq!(config["maxOutputTokens"], json!(2048));
        assert_eq!(config["responseMimeType"], json!("application/json"));
        assert!(config.get("thinkingConfig").is_none());
    }

    /// 上限和结构化输出是两组参数，网关可能只认一组。退回 prompt-only 之后
    /// 上限还得在，否则退化路径上又变成没有上限。
    #[test]
    fn dropping_structured_output_keeps_the_output_cap() {
        let budget = OutputBudget { max_tokens: 2048 };
        let config = JsonMode::PromptOnly(None)
            .gemini_generation_config(Some(budget))
            .expect("generationConfig");
        let config = serde_json::to_value(config).expect("serialize");
        assert_eq!(config["maxOutputTokens"], json!(2048));
        assert_eq!(config["responseMimeType"], json!("text/plain"));
        assert!(config.get("responseSchema").is_none());
    }

    fn closed_schema() -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "summary": { "type": "string" },
                "score": { "type": "number", "minimum": 0.0 },
                "tags": { "type": "array", "items": { "type": "string" } },
                "kind": { "type": "string", "enum": ["a", "b"] }
            },
            "required": ["summary"],
            "additionalProperties": false
        })
    }

    // Structured output — Gemini schema dialect

    #[test]
    fn gemini_schema_uppercases_types_and_drops_unsupported_keys() {
        let translated = gemini_response_schema(&closed_schema()).expect("closed schema");
        assert_eq!(translated["type"], "OBJECT");
        assert_eq!(translated["properties"]["summary"]["type"], "STRING");
        assert_eq!(translated["properties"]["tags"]["type"], "ARRAY");
        assert_eq!(translated["properties"]["tags"]["items"]["type"], "STRING");
        assert_eq!(translated["required"], json!(["summary"]));
        // Gemini rejects these outright.
        assert!(translated.get("additionalProperties").is_none());
        assert!(translated["properties"]["score"].get("minimum").is_none());
        // enum is part of the accepted subset.
        assert_eq!(translated["properties"]["kind"]["enum"], json!(["a", "b"]));
    }

    #[test]
    fn gemini_schema_rejects_free_form_objects() {
        // Gemini has no way to express an OBJECT without properties, which is
        // how a planner step's open `params` map is declared. The whole schema
        // must be dropped rather than silently losing the field.
        assert!(gemini_response_schema(&json!({ "type": "object" })).is_none());
        assert!(gemini_response_schema(&json!({
            "type": "object",
            "properties": {}
        }))
        .is_none());
        assert!(gemini_response_schema(&json!({
            "type": "object",
            "properties": {
                "steps": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": { "params": { "type": "object" } }
                    }
                }
            }
        }))
        .is_none());
    }

    #[test]
    fn gemini_schema_rejects_non_object_roots() {
        assert!(gemini_response_schema(&json!("string")).is_none());
        assert!(gemini_response_schema(&json!([1, 2])).is_none());
    }

    // Structured output — request bodies

    #[test]
    fn structured_mode_sets_json_mime_and_schema_for_gemini() {
        let schema = closed_schema();
        let config = JsonMode::Structured(Some(&schema))
            .gemini_generation_config(None)
            .expect("structured mode configures generation");
        assert_eq!(config.response_mime_type, "application/json");
        assert_eq!(
            config.response_schema.expect("translatable schema")["type"],
            "OBJECT"
        );
    }

    #[test]
    fn untranslatable_schema_still_enforces_json_on_gemini() {
        let schema = json!({ "type": "object", "properties": { "p": { "type": "object" } } });
        let config = JsonMode::Structured(Some(&schema))
            .gemini_generation_config(None)
            .expect("json mime is kept");
        assert_eq!(config.response_mime_type, "application/json");
        assert!(config.response_schema.is_none());
    }

    #[test]
    fn openai_structured_mode_uses_json_schema_non_strict() {
        let schema = closed_schema();
        let format = JsonMode::Structured(Some(&schema))
            .openai_response_format("planner_output")
            .expect("response_format is set");
        assert_eq!(format["type"], "json_schema");
        assert_eq!(format["json_schema"]["name"], "planner_output");
        // strict mode forbids free-form objects, which planner params require.
        assert_eq!(format["json_schema"]["strict"], json!(false));
        assert_eq!(format["json_schema"]["schema"], schema);
    }

    #[test]
    fn openai_falls_back_to_json_object_without_a_schema() {
        let format = JsonMode::Structured(None)
            .openai_response_format("x")
            .expect("json mode is still requested");
        assert_eq!(format, json!({ "type": "json_object" }));
    }

    #[test]
    fn prompt_only_mode_sends_no_structured_parameters() {
        let schema = closed_schema();
        let mode = JsonMode::PromptOnly(Some(&schema));
        assert!(mode.gemini_generation_config(None).is_none());
        assert!(mode.openai_response_format("x").is_none());
    }

    #[test]
    fn prompt_only_mode_restates_the_contract_in_the_prompt() {
        let schema = closed_schema();
        let decorated = JsonMode::PromptOnly(Some(&schema)).decorate_prompt("do the thing");
        assert!(decorated.starts_with("do the thing"));
        assert!(decorated.contains("Return one valid JSON value only"));
        assert!(decorated.contains("\"summary\""));

        // Structured mode leaves the prompt untouched; the API enforces it.
        assert_eq!(
            JsonMode::Structured(Some(&schema)).decorate_prompt("do the thing"),
            "do the thing"
        );
    }
}
