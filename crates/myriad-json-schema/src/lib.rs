//! Deterministic JSON Schema subset validator for Myriad AI Task structured
//! output, Data Exchange, and agent interaction.
//!
//! Workspace crate so domain services share one implementation without
//! depending on the HTTP API layer. Unsupported keywords are ignored; `$ref`
//! is rejected at install time so validation never performs file or network I/O.

use serde_json::{Map, Value};

fn schema_type_matches(expected: &str, value: &Value) -> bool {
    match expected {
        "null" => value.is_null(),
        "boolean" => value.is_boolean(),
        "object" => value.is_object(),
        "array" => value.is_array(),
        "number" => value.is_number(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        "string" => value.is_string(),
        _ => false,
    }
}

fn validate_number_bounds(
    schema: &Map<String, Value>,
    value: &Value,
    path: &str,
) -> Result<(), String> {
    let Some(number) = value.as_f64() else {
        return Ok(());
    };
    if let Some(minimum) = schema.get("minimum").and_then(Value::as_f64) {
        if number < minimum {
            return Err(format!("{path} is below minimum {minimum}"));
        }
    }
    if let Some(maximum) = schema.get("maximum").and_then(Value::as_f64) {
        if number > maximum {
            return Err(format!("{path} is above maximum {maximum}"));
        }
    }
    Ok(())
}

fn validate_schema(schema: &Value, value: &Value, path: &str, depth: usize) -> Result<(), String> {
    if depth > 32 {
        return Err(format!("{path} exceeds schema nesting limit"));
    }
    let object = schema
        .as_object()
        .ok_or_else(|| format!("{path} has an invalid schema"))?;

    if let Some(constant) = object.get("const") {
        if value != constant {
            return Err(format!("{path} does not match const"));
        }
    }
    if let Some(allowed) = object.get("enum").and_then(Value::as_array) {
        if !allowed.iter().any(|candidate| candidate == value) {
            return Err(format!("{path} is not an allowed enum value"));
        }
    }
    if let Some(expected) = object.get("type") {
        let matches = match expected {
            Value::String(kind) => schema_type_matches(kind, value),
            Value::Array(kinds) => kinds
                .iter()
                .filter_map(Value::as_str)
                .any(|kind| schema_type_matches(kind, value)),
            _ => false,
        };
        if !matches {
            return Err(format!("{path} has the wrong JSON type"));
        }
    }

    if let Some(text) = value.as_str() {
        let length = text.chars().count() as u64;
        if object
            .get("minLength")
            .and_then(Value::as_u64)
            .is_some_and(|minimum| length < minimum)
        {
            return Err(format!("{path} is shorter than minLength"));
        }
        if object
            .get("maxLength")
            .and_then(Value::as_u64)
            .is_some_and(|maximum| length > maximum)
        {
            return Err(format!("{path} is longer than maxLength"));
        }
    }
    validate_number_bounds(object, value, path)?;

    if let Some(items) = value.as_array() {
        if object
            .get("minItems")
            .and_then(Value::as_u64)
            .is_some_and(|minimum| items.len() < minimum as usize)
        {
            return Err(format!("{path} has fewer than minItems"));
        }
        if object
            .get("maxItems")
            .and_then(Value::as_u64)
            .is_some_and(|maximum| items.len() > maximum as usize)
        {
            return Err(format!("{path} has more than maxItems"));
        }
        if let Some(item_schema) = object.get("items") {
            for (index, item) in items.iter().enumerate() {
                validate_schema(item_schema, item, &format!("{path}[{index}]"), depth + 1)?;
            }
        }
    }

    if let Some(map) = value.as_object() {
        if let Some(required) = object.get("required").and_then(Value::as_array) {
            for key in required.iter().filter_map(Value::as_str) {
                if !map.contains_key(key) {
                    return Err(format!("{path}.{key} is required"));
                }
            }
        }
        let properties = object.get("properties").and_then(Value::as_object);
        if let Some(properties) = properties {
            for (key, property_schema) in properties {
                if let Some(property) = map.get(key) {
                    validate_schema(
                        property_schema,
                        property,
                        &format!("{path}.{key}"),
                        depth + 1,
                    )?;
                }
            }
        }
        if object.get("additionalProperties") == Some(&Value::Bool(false)) {
            let empty = Map::new();
            let declared = properties.unwrap_or(&empty);
            if let Some(key) = map.keys().find(|key| !declared.contains_key(*key)) {
                return Err(format!("{path}.{key} is not declared"));
            }
        }
    }

    Ok(())
}

/// Validate `value` against the install-time-approved schema subset.
pub fn validate_inline_json_value(schema: &Value, value: &Value) -> Result<(), String> {
    validate_schema(schema, value, "$", 0)
}

/// Max serialized size for inline schemas accepted by AI Task / Data Exchange.
pub const MAX_INLINE_SCHEMA_BYTES: usize = 16 * 1024;

/// Validate that an inline schema object is well-formed enough to accept
/// (size bound, no `$ref`, has a discriminant). Does not validate a value.
pub fn validate_inline_data_schema(schema: &Value) -> Result<(), String> {
    let object = schema
        .as_object()
        .ok_or_else(|| "Schema must be an inline JSON object".to_string())?;
    let encoded =
        serde_json::to_vec(schema).map_err(|_| "Schema cannot be serialized".to_string())?;
    if encoded.len() > MAX_INLINE_SCHEMA_BYTES {
        return Err(format!(
            "Schema is too large (max {MAX_INLINE_SCHEMA_BYTES} bytes)"
        ));
    }

    fn reject_refs(value: &Value, depth: usize) -> Result<(), String> {
        if depth > 32 {
            return Err("Schema nesting is too deep".to_string());
        }
        match value {
            Value::Object(map) => {
                if map.contains_key("$ref") {
                    return Err("Schema does not support $ref".to_string());
                }
                for child in map.values() {
                    reject_refs(child, depth + 1)?;
                }
            }
            Value::Array(values) => {
                for child in values {
                    reject_refs(child, depth + 1)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    reject_refs(schema, 0)?;
    if !object.contains_key("type")
        && !object.contains_key("properties")
        && !object.contains_key("enum")
        && !object.contains_key("const")
    {
        return Err("Schema must declare type, properties, enum, or const".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_inline_json_value;
    use serde_json::json;

    #[test]
    fn requires_declared_properties_when_additional_false() {
        let schema = json!({
            "type": "object",
            "properties": { "name": { "type": "string" } },
            "required": ["name"],
            "additionalProperties": false
        });
        assert!(validate_inline_json_value(&schema, &json!({ "name": "ok" })).is_ok());
        assert!(validate_inline_json_value(&schema, &json!({ "name": "ok", "x": 1 })).is_err());
        assert!(validate_inline_json_value(&schema, &json!({})).is_err());
    }

    #[test]
    fn enforces_string_and_number_bounds() {
        let schema = json!({
            "type": "object",
            "properties": {
                "title": { "type": "string", "minLength": 2, "maxLength": 4 },
                "n": { "type": "integer", "minimum": 1, "maximum": 3 }
            },
            "required": ["title", "n"]
        });
        assert!(validate_inline_json_value(&schema, &json!({ "title": "ab", "n": 2 })).is_ok());
        assert!(validate_inline_json_value(&schema, &json!({ "title": "a", "n": 2 })).is_err());
        assert!(validate_inline_json_value(&schema, &json!({ "title": "abcde", "n": 2 })).is_err());
        assert!(validate_inline_json_value(&schema, &json!({ "title": "ab", "n": 0 })).is_err());
    }
}
