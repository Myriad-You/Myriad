//! Bounded reference-image input for Tapp AI Tasks. Never fetch caller URLs.

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use myriad_tapp_contract::manifest::TappAiOperation;
use serde_json::Value;

use super::{
    ai_task_prepare::{AiTaskLogicError, MAX_INPUT_BYTES},
    image_cache::ImageCacheService,
    image_generation::{load_local_reference, ImageReference},
};

pub const MAX_REFERENCE_IMAGES: usize = 4;
pub const MAX_REFERENCE_BYTES: usize = 10 * 1024 * 1024;
// Base64 expansion, data-URL headers, and the existing text-input allowance.
pub const MAX_IMAGE_INPUT_BYTES: usize =
    MAX_REFERENCE_BYTES.div_ceil(3) * 4 + MAX_REFERENCE_IMAGES * 128 + MAX_INPUT_BYTES;

fn invalid_reference(message: impl Into<String>) -> AiTaskLogicError {
    AiTaskLogicError::new("INVALID_AI_IMAGE_REFERENCE", message)
}

fn reference_limit() -> AiTaskLogicError {
    AiTaskLogicError::new(
        "AI_IMAGE_REFERENCE_LIMIT",
        "Reference images must total at most 10 MiB of decoded image data",
    )
}

/// Keep text/unknown fields under the existing limit; only image references get
/// the larger allowance. Do this before hashing or cloning a request.
pub fn validate_task_input(
    operation: TappAiOperation,
    input: &Value,
) -> Result<(), AiTaskLogicError> {
    let encoded_size = serde_json::to_vec(input)
        .map_err(|_| {
            AiTaskLogicError::new(
                "INVALID_AI_TASK_INPUT",
                "AI task input cannot be serialized",
            )
        })?
        .len();
    let is_image = operation == TappAiOperation::Image;
    let limit = if is_image {
        MAX_IMAGE_INPUT_BYTES
    } else {
        MAX_INPUT_BYTES
    };
    if encoded_size > limit {
        return Err(AiTaskLogicError::new(
            "AI_TASK_INPUT_LIMIT",
            format!("AI task input exceeds {limit} bytes"),
        ));
    }
    let Some(references) = input.get("referenceImages") else {
        if encoded_size > MAX_INPUT_BYTES {
            return Err(AiTaskLogicError::new(
                "AI_TASK_INPUT_LIMIT",
                "AI task text input exceeds 256 KiB",
            ));
        }
        return Ok(());
    };
    if !is_image {
        return Err(invalid_reference(
            "referenceImages is only supported for image tasks",
        ));
    }
    let references = references.as_array().ok_or_else(|| {
        invalid_reference("referenceImages must be an array of image source strings")
    })?;
    if references.len() > MAX_REFERENCE_IMAGES {
        return Err(AiTaskLogicError::new(
            "AI_IMAGE_REFERENCE_LIMIT",
            "referenceImages accepts at most 4 images",
        ));
    }
    let text_input = input
        .as_object()
        .expect("referenceImages belongs to an object")
        .iter()
        .filter(|(key, _)| key.as_str() != "referenceImages")
        .collect::<std::collections::BTreeMap<_, _>>();
    // Serialize borrowed values: do not clone multi-megabyte reference strings.
    let text_size = serde_json::to_vec(&text_input)
        .expect("JSON values serialize")
        .len();
    if text_size > MAX_INPUT_BYTES {
        return Err(AiTaskLogicError::new(
            "AI_TASK_INPUT_LIMIT",
            "AI task text input exceeds 256 KiB",
        ));
    }
    for reference in references {
        let source = reference.as_str().ok_or_else(|| {
            invalid_reference("Each reference image must be a data URL or local image-cache path")
        })?;
        if !source.starts_with("data:")
            && !(source.starts_with("/api/brew/image-cache/")
                && ImageCacheService::new()
                    .local_path_for_public_url(source)
                    .is_some())
        {
            return Err(invalid_reference(
                "Reference images must use base64 data URLs or local /api/brew/image-cache paths",
            ));
        }
    }
    Ok(())
}

fn decode_reference(source: &str) -> Result<ImageReference, AiTaskLogicError> {
    let (metadata, encoded) = source
        .strip_prefix("data:")
        .and_then(|data| data.split_once(','))
        .ok_or_else(|| invalid_reference("Invalid reference image data URL"))?;
    let media_type = metadata
        .strip_suffix(";base64")
        .filter(|mime| matches!(*mime, "image/png" | "image/jpeg" | "image/webp"))
        .ok_or_else(|| invalid_reference("Reference images must be base64 PNG, JPEG, or WebP"))?;
    if encoded.len() > MAX_REFERENCE_BYTES.div_ceil(3) * 4 {
        return Err(reference_limit());
    }
    let bytes = BASE64
        .decode(encoded)
        .map_err(|_| invalid_reference("Invalid reference image base64"))?;
    if bytes.len() > MAX_REFERENCE_BYTES {
        return Err(reference_limit());
    }
    ImageReference::new(bytes, media_type)
        .map_err(|_| invalid_reference("Reference image content does not match its media type"))
}

/// Resolve references before quota reservation or task registration, preserving
/// caller order. Only validated local cache paths ever reach the filesystem.
pub async fn load_image_references(input: &Value) -> Result<Vec<ImageReference>, AiTaskLogicError> {
    validate_task_input(TappAiOperation::Image, input)?;
    let Some(sources) = input.get("referenceImages").and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    let mut references = Vec::with_capacity(sources.len());
    let mut total_bytes = 0;
    for source in sources {
        let source = source.as_str().expect("validated reference source");
        let reference = if source.starts_with("data:") {
            decode_reference(source)?
        } else {
            load_local_reference(source)
                .await
                .map_err(|_| invalid_reference("Local reference image is missing or invalid"))?
        };
        total_bytes += reference.bytes.len();
        if total_bytes > MAX_REFERENCE_BYTES {
            return Err(reference_limit());
        }
        references.push(reference);
    }
    Ok(references)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn png_url(size: usize) -> String {
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.resize(size.max(bytes.len()), 0);
        format!("data:image/png;base64,{}", BASE64.encode(bytes))
    }

    #[tokio::test]
    async fn accepts_ordered_inline_references_and_text_only_inputs() {
        let references = load_image_references(&json!({
            "prompt": "combine", "referenceImages": [png_url(8),
                format!("data:image/jpeg;base64,{}", BASE64.encode([0xff, 0xd8, 0xff]))]
        }))
        .await
        .unwrap();
        assert_eq!(references[0].media_type, "image/png");
        assert_eq!(references[1].media_type, "image/jpeg");
        assert!(load_image_references(&json!("draw"))
            .await
            .unwrap()
            .is_empty());
        assert!(
            load_image_references(&json!({"prompt": "draw", "referenceImages": []}))
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn resolves_local_cache_images_and_reports_missing_ones() {
        let service = ImageCacheService::new();
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
        let stored = service
            .store_bytes_with_status(&bytes, "image/png")
            .await
            .unwrap();
        let input = json!({"referenceImages": [stored.url]});
        let result = load_image_references(&input).await;
        if stored.created {
            service.remove_stored_url(&stored.url).await.unwrap();
        }
        let references = result.unwrap();
        assert_eq!(references.len(), 1);
        assert_eq!(references[0].bytes, bytes);
        assert_eq!(references[0].media_type, "image/png");
        if stored.created {
            assert_eq!(
                load_image_references(&input).await.unwrap_err().code,
                "INVALID_AI_IMAGE_REFERENCE"
            );
        }
    }

    #[tokio::test]
    async fn rejects_malformed_spoofed_and_external_references() {
        for source in [
            "data:image/png;base64,!!!",
            "data:image/png;base64,YmFk",
            "data:image/png,abc",
            "data:image/svg+xml;base64,AA==",
            "",
            "https://example.com/a.png",
            "http://127.0.0.1/a.png",
            "file:///etc/passwd",
            "blob:example",
            "/api/brew/image-cache/aa/../secret.png",
            "/api/brew/image-cache/aa/not-a-hash.png",
        ] {
            let error = load_image_references(&json!({"referenceImages": [source]}))
                .await
                .unwrap_err();
            assert_eq!(error.code, "INVALID_AI_IMAGE_REFERENCE", "{source}");
        }
        for references in [json!(null), json!("a.png"), json!([{}]), json!([1])] {
            assert!(
                load_image_references(&json!({"referenceImages": references}))
                    .await
                    .is_err()
            );
        }
        assert!(
            validate_task_input(TappAiOperation::Generate, &json!({"referenceImages": []}))
                .is_err()
        );
    }

    #[tokio::test]
    async fn enforces_count_total_bytes_and_separate_text_limit() {
        let too_many = json!({"referenceImages": vec![png_url(8); MAX_REFERENCE_IMAGES + 1]});
        assert_eq!(
            load_image_references(&too_many).await.unwrap_err().code,
            "AI_IMAGE_REFERENCE_LIMIT"
        );
        let oversized = json!({"referenceImages": [png_url(MAX_REFERENCE_BYTES / 2 + 1), png_url(MAX_REFERENCE_BYTES / 2 + 1)]});
        assert_eq!(
            load_image_references(&oversized).await.unwrap_err().code,
            "AI_IMAGE_REFERENCE_LIMIT"
        );
        let valid = json!({"prompt": "draw", "referenceImages": [png_url(MAX_INPUT_BYTES)]});
        assert!(load_image_references(&valid).await.is_ok());
        let text = json!({"prompt": "x".repeat(MAX_INPUT_BYTES), "referenceImages": []});
        assert_eq!(
            validate_task_input(TappAiOperation::Image, &text)
                .unwrap_err()
                .code,
            "AI_TASK_INPUT_LIMIT"
        );
        assert_eq!(
            validate_task_input(TappAiOperation::Image, &json!("x".repeat(MAX_INPUT_BYTES)))
                .unwrap_err()
                .code,
            "AI_TASK_INPUT_LIMIT"
        );
    }
}
