//! Pure AI Task prompt construction and structured-output normalization.
//!
//! HTTP handlers map [`AiTaskLogicError`] to Axum responses; provider execution
//! and registry state stay elsewhere.

use serde_json::{json, Value};

use crate::services::json_schema_subset::validate_inline_json_value;
use crate::services::permission_service::TappPermission;
use myriad_tapp_contract::manifest::{TappAiOperation, TappAiOutputFormat};

pub const MAX_INPUT_BYTES: usize = 128 * 1024;
pub const MAX_CONTEXT_BYTES: usize = 128 * 1024;
pub const MAX_IDEMPOTENCY_KEY_BYTES: usize = 128;

/// Domain error with stable API codes for prepare / normalize paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiTaskLogicError {
    pub code: String,
    pub message: String,
}

impl AiTaskLogicError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn into_pair(self) -> (String, String) {
        (self.code, self.message)
    }
}

impl std::fmt::Display for AiTaskLogicError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for AiTaskLogicError {}

pub fn permission_for_operation(operation: TappAiOperation) -> TappPermission {
    match operation {
        TappAiOperation::Generate => TappPermission::AiGenerate,
        TappAiOperation::Analyze => TappPermission::AiAnalyze,
        TappAiOperation::Chat => TappPermission::AiChat,
        TappAiOperation::Image => TappPermission::AiImage,
    }
}

pub fn default_output_format(operation: TappAiOperation) -> TappAiOutputFormat {
    if operation == TappAiOperation::Image {
        TappAiOutputFormat::Image
    } else {
        TappAiOutputFormat::Text
    }
}

pub fn validate_idempotency_key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_IDEMPOTENCY_KEY_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
}

pub fn extract_text_input(input: &Value, key: &str) -> Option<String> {
    input
        .as_str()
        .or_else(|| input.get(key).and_then(Value::as_str))
        .map(str::to_owned)
}

/// Build the model-facing prompt for a declared AI operation.
pub fn build_operation_prompt(
    operation: TappAiOperation,
    input: &Value,
) -> Result<String, AiTaskLogicError> {
    match operation {
        TappAiOperation::Generate => extract_text_input(input, "prompt")
            .filter(|prompt| !prompt.trim().is_empty())
            .ok_or_else(|| {
                AiTaskLogicError::new(
                    "INVALID_AI_TASK_INPUT",
                    "generate input must be a non-empty string or contain prompt",
                )
            }),
        TappAiOperation::Image => extract_text_input(input, "prompt")
            .filter(|prompt| !prompt.trim().is_empty())
            .ok_or_else(|| {
                AiTaskLogicError::new(
                    "INVALID_AI_TASK_INPUT",
                    "image input must be a non-empty string or contain prompt",
                )
            }),
        TappAiOperation::Analyze => {
            let object = input.as_object().ok_or_else(|| {
                AiTaskLogicError::new(
                    "INVALID_AI_TASK_INPUT",
                    "analyze input must be an object",
                )
            })?;
            let data = object.get("data").ok_or_else(|| {
                AiTaskLogicError::new("INVALID_AI_TASK_INPUT", "analyze input requires data")
            })?;
            let instruction = object
                .get("instruction")
                .and_then(Value::as_str)
                .unwrap_or("Analyze the supplied data and return the most useful findings.");
            if instruction.len() > 4_000
                || myriad_prompt_security::validate_prompt_security(instruction).is_some()
            {
                return Err(AiTaskLogicError::new(
                    "UNSAFE_AI_TASK_INPUT",
                    "Analyze instruction is invalid or unsafe",
                ));
            }
            Ok(format!("{instruction}\n\nData:\n{data}"))
        }
        TappAiOperation::Chat => {
            let messages = input
                .get("messages")
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    AiTaskLogicError::new(
                        "INVALID_AI_TASK_INPUT",
                        "chat input requires a messages array",
                    )
                })?;
            if messages.is_empty() || messages.len() > 100 {
                return Err(AiTaskLogicError::new(
                    "INVALID_AI_TASK_INPUT",
                    "chat messages must contain 1-100 entries",
                ));
            }
            let mut transcript = Vec::with_capacity(messages.len());
            for message in messages {
                let role = message.get("role").and_then(Value::as_str).unwrap_or("");
                let content = message.get("content").and_then(Value::as_str).unwrap_or("");
                if !matches!(role, "system" | "user" | "assistant")
                    || content.is_empty()
                    || content.len() > 10_000
                    || myriad_prompt_security::validate_prompt_security(content).is_some()
                {
                    return Err(AiTaskLogicError::new(
                        "INVALID_AI_TASK_INPUT",
                        "chat contains an invalid or unsafe message",
                    ));
                }
                transcript.push(format!("[{}]\n{}", role.to_uppercase(), content));
            }
            Ok(transcript.join("\n\n"))
        }
    }
}

/// Assemble the full prompt after context resolution (pure; security checks included).
pub fn assemble_task_prompt(
    operation: TappAiOperation,
    input: &Value,
    context: &str,
    output_format: TappAiOutputFormat,
    output_schema: Option<&Value>,
) -> Result<String, AiTaskLogicError> {
    let mut prompt = build_operation_prompt(operation, input)?;
    if operation == TappAiOperation::Image {
        if prompt.len() > 1_000
            || myriad_prompt_security::validate_image_prompt_security(&prompt).is_some()
        {
            return Err(AiTaskLogicError::new(
                "UNSAFE_AI_TASK_INPUT",
                "Image prompt is invalid or unsafe",
            ));
        }
    } else {
        if prompt.len() > MAX_INPUT_BYTES
            || myriad_prompt_security::validate_prompt_security(&prompt).is_some()
        {
            return Err(AiTaskLogicError::new(
                "UNSAFE_AI_TASK_INPUT",
                "AI task prompt is invalid or unsafe",
            ));
        }
        prompt.push_str(context);
        if output_format == TappAiOutputFormat::Json {
            prompt.push_str("\n\nReturn one valid JSON value only, without Markdown fences.");
            if let Some(schema) = output_schema {
                prompt.push_str(" The JSON value must satisfy this schema:\n");
                prompt.push_str(&schema.to_string());
            }
        }
    }
    if prompt.len() > MAX_INPUT_BYTES + MAX_CONTEXT_BYTES {
        return Err(AiTaskLogicError::new(
            "AI_TASK_PROMPT_LIMIT",
            "Resolved AI task prompt is too large",
        ));
    }
    Ok(prompt)
}

/// Normalize a text-model completion into the host result envelope.
pub fn normalize_text_result(
    format: TappAiOutputFormat,
    schema: Option<&Value>,
    provenance: &[Value],
    raw: String,
) -> Result<Value, AiTaskLogicError> {
    let value = match format {
        TappAiOutputFormat::Text => Value::String(raw),
        TappAiOutputFormat::Json => serde_json::from_str::<Value>(&raw).map_err(|_| {
            AiTaskLogicError::new(
                "AI_INVALID_STRUCTURED_OUTPUT",
                "Model response was not valid JSON",
            )
        })?,
        TappAiOutputFormat::Image => {
            return Err(AiTaskLogicError::new(
                "AI_INVALID_STRUCTURED_OUTPUT",
                "text task cannot use image output",
            ))
        }
    };
    if let Some(schema) = schema {
        validate_inline_json_value(schema, &value).map_err(|error| {
            AiTaskLogicError::new(
                "AI_OUTPUT_SCHEMA_MISMATCH",
                format!("Model response failed output schema validation: {error}"),
            )
        })?;
    }
    Ok(json!({
        "format": format,
        "value": value,
        "contextProvenance": provenance,
    }))
}

#[cfg(test)]
mod tests {
    use super::{
        assemble_task_prompt, build_operation_prompt, normalize_text_result,
        validate_idempotency_key,
    };
    use myriad_tapp_contract::manifest::{TappAiOperation, TappAiOutputFormat};
    use serde_json::json;

    #[test]
    fn validates_idempotency_key_shape() {
        assert!(validate_idempotency_key("refresh:day-2026_07_15"));
        assert!(!validate_idempotency_key(""));
        assert!(!validate_idempotency_key("contains whitespace"));
    }

    #[test]
    fn rejects_invalid_chat_roles() {
        let result = build_operation_prompt(
            TappAiOperation::Chat,
            &json!({ "messages": [{ "role": "tool", "content": "secret" }] }),
        );
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, "INVALID_AI_TASK_INPUT");
    }

    #[test]
    fn generate_prompt_from_string_or_object() {
        assert_eq!(
            build_operation_prompt(TappAiOperation::Generate, &json!("hello")).unwrap(),
            "hello"
        );
        assert_eq!(
            build_operation_prompt(
                TappAiOperation::Generate,
                &json!({ "prompt": "world" })
            )
            .unwrap(),
            "world"
        );
    }

    #[test]
    fn normalize_json_output_validates_schema() {
        let schema = json!({
            "type": "object",
            "properties": { "ok": { "type": "boolean" } },
            "required": ["ok"],
            "additionalProperties": false
        });
        let value = normalize_text_result(
            TappAiOutputFormat::Json,
            Some(&schema),
            &[],
            r#"{"ok":true}"#.into(),
        )
        .unwrap();
        assert_eq!(value["value"]["ok"], true);

        let err = normalize_text_result(
            TappAiOutputFormat::Json,
            Some(&schema),
            &[],
            r#"{"ok":1}"#.into(),
        )
        .unwrap_err();
        assert_eq!(err.code, "AI_OUTPUT_SCHEMA_MISMATCH");
    }

    #[test]
    fn assemble_json_prompt_appends_schema_instruction() {
        let schema = json!({ "type": "object" });
        let prompt = assemble_task_prompt(
            TappAiOperation::Generate,
            &json!("write a card"),
            "\n\nctx",
            TappAiOutputFormat::Json,
            Some(&schema),
        )
        .unwrap();
        assert!(prompt.contains("write a card"));
        assert!(prompt.contains("ctx"));
        assert!(prompt.contains("Return one valid JSON value only"));
        assert!(prompt.contains("\"type\":\"object\"") || prompt.contains("\"type\": \"object\""));
    }
}
