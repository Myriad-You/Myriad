//! Native tools need JSON Schema semantics, including MCP composition and local
//! references. This compiler never retrieves another file or network resource.
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex},
};

const MAX_SCHEMA_BYTES: usize = 64 * 1024;
const MAX_CACHED_SCHEMAS: usize = 128;
static CACHE: LazyLock<Mutex<HashMap<String, Arc<Prepared>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub(super) struct Prepared {
    pub schema: Value,
    validator: jsonschema::Validator,
}

impl Prepared {
    pub fn validate(&self, arguments: &Value) -> Result<(), String> {
        self.validator.validate(arguments).map_err(|error| {
            // The library's default display includes the rejected value.
            // Only report its location; credentials can be tool arguments.
            format!(
                "Tool arguments do not match the declared schema at {}",
                error.instance_path()
            )
        })
    }
}

pub(super) fn prepare(schema: &Value) -> Result<Arc<Prepared>, String> {
    let bytes = serde_json::to_vec(schema).map_err(|_| "Invalid tool schema")?;
    if bytes.len() > MAX_SCHEMA_BYTES {
        return Err("Tool schema exceeds 64 KiB".into());
    }
    if !schema.is_object() {
        return Err("Tool parameters must have an object schema".into());
    }
    check_depth(schema, 0)?;
    let key = hex::encode(Sha256::digest(&bytes));
    if let Some(prepared) = CACHE.lock().unwrap().get(&key).cloned() {
        return Ok(prepared);
    }
    let schema = normalize(schema);
    let validator = jsonschema::options()
        .offline()
        .with_pattern_options(jsonschema::PatternOptions::fancy_regex().backtrack_limit(10_000))
        .build(&schema)
        .map_err(|_| "Tool schema is invalid or requires an external reference")?;
    let prepared = Arc::new(Prepared { schema, validator });
    let mut cache = CACHE.lock().unwrap();
    if cache.len() >= MAX_CACHED_SCHEMAS {
        cache.clear();
    }
    cache.insert(key, prepared.clone());
    Ok(prepared)
}

fn check_depth(value: &Value, depth: usize) -> Result<(), String> {
    if depth > 32 {
        return Err("Tool schema is nested too deeply".into());
    }
    match value {
        Value::Object(map) => {
            for child in map.values() {
                check_depth(child, depth + 1)?;
            }
        }
        Value::Array(values) => {
            for child in values {
                check_depth(child, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Normalize the catalog's legacy `type: any` only at schema locations. Values
/// under const/default/enum/examples are data and must remain byte-for-byte equal.
fn normalize(value: &Value) -> Value {
    let Some(object) = value.as_object() else {
        return value.clone();
    };
    let mut object = object.clone();
    if object.get("type").is_some_and(|kind| kind == "any") {
        object.remove("type");
    }
    for (key, child) in &mut object {
        match key.as_str() {
            "properties" | "patternProperties" | "$defs" | "definitions" | "dependentSchemas"
            | "dependencies" => {
                if let Some(map) = child.as_object_mut() {
                    for schema in map.values_mut() {
                        *schema = normalize(schema);
                    }
                }
            }
            "allOf" | "anyOf" | "oneOf" | "prefixItems" => {
                if let Some(values) = child.as_array_mut() {
                    for schema in values {
                        *schema = normalize(schema);
                    }
                }
            }
            "items" if child.is_array() => {
                for schema in child.as_array_mut().unwrap() {
                    *schema = normalize(schema);
                }
            }
            "items"
            | "additionalProperties"
            | "unevaluatedProperties"
            | "additionalItems"
            | "unevaluatedItems"
            | "contains"
            | "not"
            | "if"
            | "then"
            | "else"
            | "propertyNames"
            | "contentSchema" => *child = normalize(child),
            _ => {}
        }
    }
    Value::Object(object)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn enforces_composition_local_references_and_conditional_constraints() {
        let prepared = prepare(&json!({"type":"object",
            "$defs":{"amount":{"type":"integer","minimum":1}},
            "properties":{"mode":{"enum":["read","write"]},"amount":{"$ref":"#/$defs/amount"},"target":{"type":"string","pattern":"^[a-z]+$"}},
            "required":["mode"],"additionalProperties":false,
            "if":{"properties":{"mode":{"const":"write"}}},"then":{"required":["target","amount"]}
        })).unwrap();
        assert!(prepared.validate(&json!({"mode":"read"})).is_ok());
        assert!(
            prepared
                .validate(&json!({"mode":"write","target":"item","amount":2}))
                .is_ok()
        );
        for invalid in [
            json!({"mode":"write"}),
            json!({"mode":"write","target":"item","amount":0}),
            json!({"mode":"write","target":"INVALID","amount":1}),
            json!({"mode":"read","extra":true}),
        ] {
            assert!(prepared.validate(&invalid).is_err(), "accepted {invalid}");
        }
        let choices =
            prepare(&json!({"type":"object","oneOf":[{"required":["id"]},{"required":["name"]}]}))
                .unwrap();
        assert!(choices.validate(&json!({"id":1})).is_ok());
        assert!(choices.validate(&json!({"id":1,"name":"both"})).is_err());
        assert!(choices.validate(&json!({})).is_err());
    }

    #[test]
    fn normalization_preserves_literal_objects_and_boolean_schemas() {
        let literal = json!({"type":"any","properties":{"type":"any"}});
        let source = json!({"type":"object","properties":{
            "payload":{"type":"any","const":literal,"default":literal,"examples":[literal]},
            "forbidden":false
        }});
        let prepared = prepare(&source).unwrap();
        assert_eq!(prepared.schema["properties"]["payload"]["const"], literal);
        assert_eq!(prepared.schema["properties"]["payload"]["default"], literal);
        assert_eq!(
            prepared.schema["properties"]["payload"]["examples"][0],
            literal
        );
        assert!(prepared.validate(&json!({"payload":literal})).is_ok());
        assert!(prepared.validate(&json!({"forbidden":null})).is_err());
    }

    #[test]
    fn external_references_fail_without_disclosing_the_uri() {
        for reference in [
            "https://example.invalid/secret-token/schema",
            "file:///private/secret-token",
        ] {
            let error = prepare(&json!({"type":"object","properties":{"x":{"$ref":reference}}}))
                .err()
                .unwrap();
            assert!(!error.contains("secret-token"));
        }
        let prepared =
            prepare(&json!({"type":"object","properties":{"secret":{"type":"integer"}}})).unwrap();
        let error = prepared
            .validate(&json!({"secret":"secret-token"}))
            .unwrap_err();
        assert!(!error.contains("secret-token"));
    }

    #[test]
    fn schema_cache_uses_content_and_rejects_unbounded_schemas() {
        let first = prepare(&json!({"type":"object","required":["first"]})).unwrap();
        let second = prepare(&json!({"type":"object","required":["second"]})).unwrap();
        assert!(first.validate(&json!({"first":true})).is_ok());
        assert!(second.validate(&json!({"first":true})).is_err());
        assert!(
            prepare(&json!({"type":"object","description":"x".repeat(MAX_SCHEMA_BYTES)})).is_err()
        );
    }

    #[tokio::test]
    async fn every_builtin_tool_has_a_valid_native_parameter_schema() {
        let registry = super::super::capability::get_registry();
        let capabilities = registry.get_all();
        assert!(!capabilities.is_empty());
        for capability in &capabilities {
            assert!(
                prepare(&capability.input_schema).is_ok(),
                "invalid schema for {}",
                capability.id
            );
        }
        for tool in super::super::tools::local_tools() {
            assert!(
                prepare(&tool.parameters).is_ok(),
                "invalid schema for {}",
                tool.name
            );
        }
        println!(
            "Validated {} built-in capability schemas",
            capabilities.len()
        );
    }
}
