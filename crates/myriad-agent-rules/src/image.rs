//! Image dimension and prompt mapping. No I/O.

use crate::IMAGE_PROMPT_MAX_CHARS;
use serde_json::Value;
use std::collections::HashMap;

/// Parse image width/height: integers, whole floats, or numeric strings (`"768"` / `"768px"`).
pub fn parse_image_dim(value: &Value) -> Option<u32> {
    if let Some(n) = value.as_u64() {
        return u32::try_from(n).ok().filter(|&n| n > 0);
    }
    if let Some(n) = value.as_i64() {
        return u32::try_from(n).ok().filter(|&n| n > 0);
    }
    if let Some(n) = value.as_f64() {
        if n.is_finite() && n > 0.0 && n.fract() == 0.0 && n <= f64::from(u32::MAX) {
            return Some(n as u32);
        }
        return None;
    }
    if let Some(s) = value.as_str() {
        let s = s.trim();
        let s = s
            .strip_suffix("px")
            .or_else(|| s.strip_suffix("PX"))
            .unwrap_or(s)
            .trim();
        return s.parse::<u32>().ok().filter(|&n| n > 0);
    }
    None
}

/// Default image size when caller omits width/height.
pub const DEFAULT_IMAGE_WIDTH: u32 = 1024;
pub const DEFAULT_IMAGE_HEIGHT: u32 = 1024;
pub const IMAGE_DIM_MIN: u32 = 256;
pub const IMAGE_DIM_MAX: u32 = 2048;

/// Clamp a parsed image dimension into the supported range.
pub fn clamp_image_dim(dim: u32) -> u32 {
    dim.clamp(IMAGE_DIM_MIN, IMAGE_DIM_MAX)
}

/// Resolve width/height from optional JSON values (256–2048, default 1024).
pub fn resolve_image_size(width: Option<&Value>, height: Option<&Value>) -> (u32, u32) {
    (
        width
            .and_then(parse_image_dim)
            .map(clamp_image_dim)
            .unwrap_or(DEFAULT_IMAGE_WIDTH),
        height
            .and_then(parse_image_dim)
            .map(clamp_image_dim)
            .unwrap_or(DEFAULT_IMAGE_HEIGHT),
    )
}

/// Resolve width/height from params with defaults and clamps.
pub fn resolve_image_dimensions(params: &HashMap<String, Value>) -> (u32, u32) {
    resolve_image_size(params.get("width"), params.get("height"))
}

/// Extract image generation prompt from string or nested prompt.generate object.
pub fn resolve_image_prompt(params: &HashMap<String, Value>) -> Result<String, String> {
    let prompt_val = params.get("prompt");
    let prompt = prompt_val
        .and_then(|v| {
            v.as_str().map(|s| s.to_string()).or_else(|| {
                v.get("prompt")
                    .and_then(|inner| inner.as_str())
                    .map(|s| s.to_string())
            })
        })
        .ok_or_else(|| "Missing prompt parameter".to_string())?;
    if prompt.chars().count() > IMAGE_PROMPT_MAX_CHARS {
        return Err(format!(
            "Prompt too long (max {IMAGE_PROMPT_MAX_CHARS} characters)"
        ));
    }
    Ok(prompt)
}

/// Extract negativePrompt from params or nested prompt object.
pub fn resolve_negative_prompt(params: &HashMap<String, Value>) -> Option<String> {
    params
        .get("negativePrompt")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            params
                .get("prompt")
                .and_then(|v| v.get("negativePrompt"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_image_dim_accepts_number_and_string() {
        assert_eq!(parse_image_dim(&json!(768)), Some(768));
        assert_eq!(parse_image_dim(&json!(768.0)), Some(768));
        assert_eq!(parse_image_dim(&json!("1024")), Some(1024));
        assert_eq!(parse_image_dim(&json!(" 768px ")), Some(768));
        assert_eq!(parse_image_dim(&json!(0)), None);
        assert_eq!(parse_image_dim(&json!("nope")), None);
        assert_eq!(clamp_image_dim(10), IMAGE_DIM_MIN);
        assert_eq!(clamp_image_dim(9999), IMAGE_DIM_MAX);
        assert_eq!(
            resolve_image_size(None, None),
            (DEFAULT_IMAGE_WIDTH, DEFAULT_IMAGE_HEIGHT)
        );
    }

    #[test]
    fn resolve_image_prompt_and_dims() {
        let mut params = HashMap::from([("prompt".into(), json!("a cat"))]);
        assert_eq!(resolve_image_prompt(&params).unwrap(), "a cat");
        params.insert(
            "prompt".into(),
            json!({ "prompt": "nested", "negativePrompt": "blur" }),
        );
        assert_eq!(resolve_image_prompt(&params).unwrap(), "nested");
        assert_eq!(resolve_negative_prompt(&params).as_deref(), Some("blur"));
        let cjk = "画".repeat(400);
        assert!(cjk.len() > 1000, "regression: CJK is 3 bytes per char");
        params.insert("prompt".into(), json!(cjk));
        assert!(resolve_image_prompt(&params).is_ok());

        let too_long = "x".repeat(IMAGE_PROMPT_MAX_CHARS + 1);
        params.insert("prompt".into(), json!(too_long));
        assert!(resolve_image_prompt(&params)
            .unwrap_err()
            .contains("too long"));

        let dims = resolve_image_dimensions(&HashMap::from([
            ("width".into(), json!("512px")),
            ("height".into(), json!(768)),
        ]));
        assert_eq!(dims, (512, 768));
        assert_eq!(
            resolve_image_size(Some(&json!("768")), Some(&json!("1024px"))),
            (768, 1024)
        );
        assert_eq!(resolve_image_size(None, None), (1024, 1024));
        assert_eq!(
            resolve_image_size(Some(&json!(100)), Some(&json!(5000))),
            (IMAGE_DIM_MIN, IMAGE_DIM_MAX)
        );
    }
}
