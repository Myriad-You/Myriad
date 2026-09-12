//! Gemini native image + speech via `generateContent`.
//!
//! Same host / `x-goog-api-key` / proxy as the text analyzer. Not the
//! OpenAI `/images` or `/audio` routes.

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde_json::{json, Value};

use crate::services::http_client::get_long_running_client;
use crate::services::image_generation::{
    GeneratedImage, ImageGenerationConfig, ImageGenerationError, ImageReference,
};

const DEFAULT_GEMINI_BASE: &str = "https://generativelanguage.googleapis.com";
const PCM_SAMPLE_RATE: u32 = 24_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GeminiMediaError {
    NotConfigured(String),
    Provider(String),
    InvalidResponse(String),
}

impl std::fmt::Display for GeminiMediaError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotConfigured(message)
            | Self::Provider(message)
            | Self::InvalidResponse(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for GeminiMediaError {}

impl From<GeminiMediaError> for ImageGenerationError {
    fn from(error: GeminiMediaError) -> Self {
        match error {
            GeminiMediaError::NotConfigured(message) => {
                ImageGenerationError::NotConfigured(message)
            }
            GeminiMediaError::Provider(message) => ImageGenerationError::Provider(message),
            GeminiMediaError::InvalidResponse(message) => {
                ImageGenerationError::InvalidResponse(message)
            }
        }
    }
}

pub fn generate_content_url(base_url: &str, model: &str) -> String {
    let mut base = base_url.trim().trim_end_matches('/').to_string();
    if base.is_empty() {
        base = DEFAULT_GEMINI_BASE.to_string();
    }
    if let Some(stripped) = base.strip_suffix("/v1beta") {
        base = stripped.trim_end_matches('/').to_string();
    }
    let model = model.trim().trim_start_matches("models/");
    format!("{base}/v1beta/models/{model}:generateContent")
}

pub fn gemini_aspect_ratio(width: u32, height: u32) -> &'static str {
    if width == height {
        "1:1"
    } else if width.saturating_mul(2) == height.saturating_mul(3) {
        "3:2"
    } else if width.saturating_mul(3) == height.saturating_mul(2) {
        "2:3"
    } else if width.saturating_mul(3) == height.saturating_mul(4) {
        "4:3"
    } else if width.saturating_mul(4) == height.saturating_mul(3) {
        "3:4"
    } else if width > height {
        "16:9"
    } else {
        "9:16"
    }
}

pub fn image_request_body(
    prompt: &str,
    width: u32,
    height: u32,
    references: &[ImageReference],
) -> Value {
    let mut parts = Vec::new();
    for reference in references {
        parts.push(json!({
            "inlineData": {
                "mimeType": reference.media_type,
                "data": BASE64.encode(&reference.bytes),
            }
        }));
    }
    parts.push(json!({ "text": prompt }));
    json!({
        "contents": [{ "parts": parts }],
        "generationConfig": {
            "responseModalities": ["TEXT", "IMAGE"],
            "imageConfig": {
                "aspectRatio": gemini_aspect_ratio(width, height)
            }
        }
    })
}

pub fn tts_request_body(text: &str, voice: &str) -> Value {
    json!({
        "contents": [{
            "parts": [{ "text": text }]
        }],
        "generationConfig": {
            "responseModalities": ["AUDIO"],
            "speechConfig": {
                "voiceConfig": {
                    "prebuiltVoiceConfig": {
                        "voiceName": voice
                    }
                }
            }
        }
    })
}

pub fn stt_request_body(audio: &[u8], mime: &str, language: Option<&str>) -> Value {
    let hint = match language.map(str::trim).filter(|value| !value.is_empty()) {
        Some(code) => {
            format!("Transcribe this audio (language: {code}). Return only the transcript.")
        }
        None => "Transcribe this audio. Return only the transcript.".to_string(),
    };
    json!({
        "contents": [{
            "parts": [
                { "text": hint },
                {
                    "inlineData": {
                        "mimeType": mime,
                        "data": BASE64.encode(audio)
                    }
                }
            ]
        }]
    })
}

pub async fn generate_image(
    config: &ImageGenerationConfig,
    prompt: &str,
    width: u32,
    height: u32,
    references: &[ImageReference],
) -> Result<GeneratedImage, ImageGenerationError> {
    let value = post_generate_content(
        &config.base_url,
        &config.api_key,
        &config.model,
        &image_request_body(prompt, width, height, references),
    )
    .await
    .map_err(ImageGenerationError::from)?;
    let (bytes, media_type) = first_inline_bytes(&value, "image")
        .ok_or_else(|| ImageGenerationError::InvalidResponse(gemini_empty_message(&value)))?;
    let media_type = normalize_image_media_type(&media_type, &bytes);
    Ok(GeneratedImage {
        source: format!("data:{media_type};base64,{}", BASE64.encode(&bytes)),
        media_type,
        width,
        height,
    })
}

pub async fn text_to_speech(
    base_url: &str,
    api_key: &str,
    model: &str,
    text: &str,
    voice: &str,
) -> Result<Vec<u8>, GeminiMediaError> {
    let value =
        post_generate_content(base_url, api_key, model, &tts_request_body(text, voice)).await?;
    let (bytes, mime) = first_inline_bytes(&value, "audio")
        .ok_or_else(|| GeminiMediaError::InvalidResponse(gemini_empty_message(&value)))?;
    Ok(normalize_tts_bytes(&bytes, &mime))
}

pub async fn speech_to_text(
    base_url: &str,
    api_key: &str,
    model: &str,
    audio: Vec<u8>,
    mime: &str,
    language: Option<&str>,
) -> Result<String, GeminiMediaError> {
    let value = post_generate_content(
        base_url,
        api_key,
        model,
        &stt_request_body(&audio, mime, language),
    )
    .await?;
    let text = first_text(&value)
        .ok_or_else(|| GeminiMediaError::InvalidResponse(gemini_empty_message(&value)))?;
    if text.trim().is_empty() {
        return Err(GeminiMediaError::InvalidResponse(
            "Gemini returned an empty transcript".to_string(),
        ));
    }
    Ok(text)
}

async fn post_generate_content(
    base_url: &str,
    api_key: &str,
    model: &str,
    body: &Value,
) -> Result<Value, GeminiMediaError> {
    if api_key.trim().is_empty() {
        return Err(GeminiMediaError::NotConfigured(
            "Gemini API key is not configured".to_string(),
        ));
    }
    if model.trim().is_empty() {
        return Err(GeminiMediaError::NotConfigured(
            "Gemini model is not configured".to_string(),
        ));
    }
    let client = get_long_running_client().await;
    let url = generate_content_url(base_url, model);
    let response = client
        .post(url)
        .header("x-goog-api-key", api_key.trim())
        .json(body)
        .send()
        .await
        .map_err(|error| GeminiMediaError::Provider(error.to_string()))?;
    let status = response.status();
    let bytes = response
        .bytes()
        .await
        .map_err(|error| GeminiMediaError::Provider(error.to_string()))?;
    if !status.is_success() {
        let body: String = String::from_utf8_lossy(&bytes).chars().take(600).collect();
        tracing::error!(%status, body = %body, "Gemini API request failed");
        return Err(GeminiMediaError::Provider(
            "Gemini API request failed".to_string(),
        ));
    }
    serde_json::from_slice(&bytes).map_err(|error| {
        tracing::error!(%error, "invalid Gemini JSON");
        GeminiMediaError::InvalidResponse("invalid Gemini JSON".to_string())
    })
}

fn first_inline_bytes(value: &Value, kind_prefix: &str) -> Option<(Vec<u8>, String)> {
    if let Some(part) = value
        .pointer("/candidates/0/content/parts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find_map(|part| inline_part(part, kind_prefix))
    {
        return Some(part);
    }
    let interaction_key = if kind_prefix == "image" {
        "/output_image"
    } else {
        "/output_audio"
    };
    let blob = value.pointer(interaction_key)?;
    let data = blob.get("data").and_then(Value::as_str)?;
    let mime = blob
        .get("mime_type")
        .or_else(|| blob.get("mimeType"))
        .and_then(Value::as_str)
        .unwrap_or(if kind_prefix == "image" {
            "image/png"
        } else {
            "audio/L16"
        });
    Some((BASE64.decode(data).ok()?, mime.to_string()))
}

fn inline_part(part: &Value, kind_prefix: &str) -> Option<(Vec<u8>, String)> {
    let blob = part.get("inlineData").or_else(|| part.get("inline_data"))?;
    let mime = blob
        .get("mimeType")
        .or_else(|| blob.get("mime_type"))
        .and_then(Value::as_str)
        .unwrap_or("");
    if !mime.starts_with(kind_prefix) && !mime.is_empty() {
        return None;
    }
    let data = blob.get("data").and_then(Value::as_str)?;
    Some((
        BASE64.decode(data).ok()?,
        if mime.is_empty() {
            if kind_prefix == "image" {
                "image/png".to_string()
            } else {
                "audio/L16".to_string()
            }
        } else {
            mime.to_string()
        },
    ))
}

fn first_text(value: &Value) -> Option<String> {
    value
        .pointer("/candidates/0/content/parts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find_map(|part| part.get("text").and_then(Value::as_str))
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn gemini_empty_message(value: &Value) -> String {
    if let Some(reason) = value
        .pointer("/promptFeedback/blockReason")
        .or_else(|| value.pointer("/prompt_feedback/block_reason"))
        .or_else(|| value.pointer("/candidates/0/finishReason"))
        .or_else(|| value.pointer("/candidates/0/finish_reason"))
        .and_then(Value::as_str)
    {
        return format!("Gemini returned no media (finishReason: {reason})");
    }
    "Gemini returned no media".to_string()
}

fn normalize_image_media_type(mime: &str, bytes: &[u8]) -> String {
    let mime = mime.split(';').next().unwrap_or(mime).trim();
    if matches!(mime, "image/png" | "image/jpeg" | "image/webp") {
        return mime.to_string();
    }
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png".to_string()
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        "image/jpeg".to_string()
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        "image/webp".to_string()
    } else {
        "image/png".to_string()
    }
}

pub fn normalize_tts_bytes(bytes: &[u8], mime: &str) -> Vec<u8> {
    let mime = mime.to_ascii_lowercase();
    if mime.contains("wav") || mime.contains("mpeg") || mime.contains("mp3") || mime.contains("ogg")
    {
        return bytes.to_vec();
    }
    if bytes.starts_with(b"RIFF") || bytes.starts_with(b"ID3") || bytes.starts_with(&[0xff, 0xfb]) {
        return bytes.to_vec();
    }
    let rate = mime
        .split(';')
        .find_map(|part| part.trim().strip_prefix("rate="))
        .and_then(|value| value.parse().ok())
        .unwrap_or(PCM_SAMPLE_RATE);
    pcm16_mono_to_wav(bytes, rate)
}

pub fn pcm16_mono_to_wav(pcm: &[u8], sample_rate: u32) -> Vec<u8> {
    let data_len = pcm.len() as u32;
    let byte_rate = sample_rate.saturating_mul(2);
    let mut out = Vec::with_capacity(44 + pcm.len());
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    out.extend_from_slice(pcm);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_generate_content_url() {
        assert_eq!(
            generate_content_url(
                "https://generativelanguage.googleapis.com",
                "gemini-3.1-flash-image"
            ),
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-3.1-flash-image:generateContent"
        );
        assert_eq!(
            generate_content_url(
                "https://example.com/v1beta/",
                "models/gemini-2.5-flash-preview-tts"
            ),
            "https://example.com/v1beta/models/gemini-2.5-flash-preview-tts:generateContent"
        );
    }

    #[test]
    fn image_body_includes_reference_and_aspect() {
        let reference = ImageReference {
            bytes: b"\x89PNG\r\n\x1a\n".to_vec(),
            media_type: "image/png".to_string(),
        };
        let body = image_request_body("portrait", 1024, 1536, &[reference]);
        assert_eq!(
            body["generationConfig"]["imageConfig"]["aspectRatio"],
            "2:3"
        );
        assert_eq!(
            body["generationConfig"]["responseModalities"],
            json!(["TEXT", "IMAGE"])
        );
        assert!(body["contents"][0]["parts"][0]["inlineData"]["data"].is_string());
        assert_eq!(body["contents"][0]["parts"][1]["text"], "portrait");
    }

    #[test]
    fn image_body_preserves_multiple_reference_order_and_text_only() {
        let references = [
            ImageReference {
                bytes: vec![1],
                media_type: "image/png".into(),
            },
            ImageReference {
                bytes: vec![2],
                media_type: "image/jpeg".into(),
            },
        ];
        let body = image_request_body("combine", 1024, 1024, &references);
        let parts = body["contents"][0]["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0]["inlineData"]["data"], BASE64.encode([1]));
        assert_eq!(parts[1]["inlineData"]["data"], BASE64.encode([2]));
        assert_eq!(parts[2]["text"], "combine");
        let body = image_request_body("draw", 1024, 1024, &[]);
        assert_eq!(body["contents"][0]["parts"], json!([{ "text": "draw" }]));
    }

    #[test]
    fn reads_inline_image_and_pcm_audio() {
        let image = json!({
            "candidates": [{
                "content": {
                    "parts": [
                        { "text": "ok" },
                        { "inlineData": { "mimeType": "image/png", "data": BASE64.encode(b"\x89PNG\r\n\x1a\n") } }
                    ]
                }
            }]
        });
        let (bytes, mime) = first_inline_bytes(&image, "image").expect("image");
        assert_eq!(mime, "image/png");
        assert!(bytes.starts_with(b"\x89PNG"));

        let audio = json!({
            "candidates": [{
                "content": {
                    "parts": [{
                        "inlineData": {
                            "mimeType": "audio/L16;codec=pcm;rate=24000",
                            "data": BASE64.encode(&[0u8; 8])
                        }
                    }]
                }
            }]
        });
        let (pcm, mime) = first_inline_bytes(&audio, "audio").expect("audio");
        let wav = normalize_tts_bytes(&pcm, &mime);
        assert!(wav.starts_with(b"RIFF"));
        assert_eq!(&wav[8..12], b"WAVE");
    }

    #[test]
    fn wav_header_is_little_endian_pcm() {
        let wav = pcm16_mono_to_wav(&[1, 2, 3, 4], 24_000);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 24_000);
        assert_eq!(&wav[44..], &[1, 2, 3, 4]);
    }

    #[test]
    fn stt_hint_is_english() {
        let none = stt_request_body(b"abc", "audio/wav", None);
        let ja = stt_request_body(b"abc", "audio/wav", Some("ja-JP"));
        let none_hint = none["contents"][0]["parts"][0]["text"].as_str().unwrap();
        let ja_hint = ja["contents"][0]["parts"][0]["text"].as_str().unwrap();
        assert_eq!(
            none_hint,
            "Transcribe this audio. Return only the transcript."
        );
        assert_eq!(
            ja_hint,
            "Transcribe this audio (language: ja-JP). Return only the transcript."
        );
        assert!(!none_hint.chars().any(is_cjk));
        assert!(!ja_hint.chars().any(is_cjk));
    }

    fn is_cjk(ch: char) -> bool {
        ('\u{4e00}'..='\u{9fff}').contains(&ch)
    }
}
