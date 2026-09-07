//! OpenAI-compatible file STT / TTS (`/audio/transcriptions`, `/audio/speech`).
//!
//! Used for official OpenAI and OpenRouter. Not the chat-completions audio path.

use reqwest::multipart;
use reqwest::Client;
use serde_json::json;
use std::time::Duration;

use super::http_client::{apply_proxy, ProxyConfig};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug)]
pub enum OpenAiSpeechError {
    ApiKeyNotConfigured,
    NetworkError(String),
    ApiError {
        status: u16,
        message: String,
    },
    InvalidAudioData(String),
    /// 目前不构造：speech_runtime 走自己的可用性判定后才调这里。
    /// Display 臂保留，接入新供应商时直接可用。
    #[allow(dead_code)]
    TtsNotAvailable(String),
}

impl std::fmt::Display for OpenAiSpeechError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ApiKeyNotConfigured => {
                write!(f, "OpenAI-compatible speech API key not configured")
            }
            Self::NetworkError(msg) => write!(f, "Network error: {msg}"),
            Self::ApiError { status, message } => write!(f, "API error [{status}]: {message}"),
            Self::InvalidAudioData(msg) => write!(f, "Invalid audio data: {msg}"),
            Self::TtsNotAvailable(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for OpenAiSpeechError {}

#[derive(Debug, Clone)]
pub struct OpenAiCompatibleSpeech {
    client: Client,
    api_key: String,
    base_url: String,
    referer: Option<String>,
}

impl OpenAiCompatibleSpeech {
    pub fn new(
        api_key: String,
        base_url: String,
        proxy_config: &ProxyConfig,
        referer: Option<String>,
    ) -> Result<Self, OpenAiSpeechError> {
        let key = api_key.trim().to_string();
        if key.is_empty() {
            return Err(OpenAiSpeechError::ApiKeyNotConfigured);
        }
        let builder = Client::builder()
            .timeout(DEFAULT_TIMEOUT)
            .connect_timeout(CONNECT_TIMEOUT)
            .user_agent("Myriad/1.0");
        let client = apply_proxy(builder, proxy_config)
            .and_then(|b| b.build())
            .map_err(|e| OpenAiSpeechError::NetworkError(e.to_string()))?;
        Ok(Self {
            client,
            api_key: key,
            base_url: normalize_openai_base_url(&base_url),
            referer,
        })
    }

    fn apply_auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let req = req.bearer_auth(&self.api_key);
        match &self.referer {
            Some(referer) if !referer.is_empty() => req
                .header("HTTP-Referer", referer)
                .header("X-Title", "Myriad"),
            _ => req,
        }
    }

    pub async fn text_to_speech(
        &self,
        text: &str,
        model: &str,
        voice: &str,
        format: &str,
        instructions: Option<&str>,
    ) -> Result<Vec<u8>, OpenAiSpeechError> {
        let url = format!("{}/audio/speech", self.base_url);
        let mut body = json!({
            "model": model,
            "input": text,
            "voice": voice,
            "response_format": format,
        });
        if let Some(instructions) = instructions.filter(|s| !s.trim().is_empty()) {
            if tts_model_accepts_instructions(model) {
                body["instructions"] = json!(instructions);
            }
        }

        let response = self
            .apply_auth(self.client.post(url).json(&body))
            .send()
            .await
            .map_err(|e| OpenAiSpeechError::NetworkError(e.to_string()))?;
        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .map_err(|e| OpenAiSpeechError::NetworkError(e.to_string()))?;
        if !status.is_success() {
            return Err(OpenAiSpeechError::ApiError {
                status: status.as_u16(),
                message: openai_error_message(&bytes),
            });
        }
        if looks_like_json_error(&bytes) {
            return Err(OpenAiSpeechError::ApiError {
                status: status.as_u16(),
                message: openai_error_message(&bytes),
            });
        }
        if bytes.is_empty() {
            return Err(OpenAiSpeechError::InvalidAudioData(
                "TTS response was empty".to_string(),
            ));
        }
        Ok(bytes.to_vec())
    }

    pub async fn speech_to_text(
        &self,
        audio: Vec<u8>,
        filename: &str,
        mime: &str,
        model: &str,
        language: Option<&str>,
    ) -> Result<String, OpenAiSpeechError> {
        if audio.is_empty() {
            return Err(OpenAiSpeechError::InvalidAudioData(
                "audio payload is empty".to_string(),
            ));
        }
        let url = format!("{}/audio/transcriptions", self.base_url);
        let mut form = multipart::Form::new()
            .text("model", model.to_string())
            .part(
                "file",
                multipart::Part::bytes(audio)
                    .file_name(filename.to_string())
                    .mime_str(mime)
                    .map_err(|e| OpenAiSpeechError::InvalidAudioData(e.to_string()))?,
            );
        if let Some(language) = language.filter(|s| !s.trim().is_empty()) {
            if !stt_model_is_gpt_transcribe(model) {
                form = form.text("language", language.to_string());
            }
        }

        let response = self
            .apply_auth(self.client.post(url).multipart(form))
            .send()
            .await
            .map_err(|e| OpenAiSpeechError::NetworkError(e.to_string()))?;
        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .map_err(|e| OpenAiSpeechError::NetworkError(e.to_string()))?;
        if !status.is_success() {
            return Err(OpenAiSpeechError::ApiError {
                status: status.as_u16(),
                message: openai_error_message(&bytes),
            });
        }
        let parsed: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|e| OpenAiSpeechError::ApiError {
                status: status.as_u16(),
                message: {
                    tracing::error!(%e, "invalid transcription JSON");
                    "invalid transcription JSON".to_string()
                },
            })?;
        parsed
            .get("text")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| OpenAiSpeechError::ApiError {
                status: status.as_u16(),
                message: "transcription response missing text".to_string(),
            })
    }
}

pub fn normalize_openai_base_url(raw: &str) -> String {
    raw.trim().trim_end_matches('/').to_string()
}

pub fn tts_model_accepts_instructions(model: &str) -> bool {
    let model = model.to_ascii_lowercase();
    model.contains("gpt-4o-mini-tts")
}

pub fn stt_model_is_gpt_transcribe(model: &str) -> bool {
    model.to_ascii_lowercase().contains("gpt-transcribe")
}

/// OpenRouter's catalog does not currently list official OpenAI TTS slugs.
pub fn openrouter_official_tts_unavailable(model: &str) -> bool {
    let model = model.trim().to_ascii_lowercase();
    model.is_empty()
        || model.contains("gpt-4o-mini-tts")
        || model == "tts-1"
        || model == "tts-1-hd"
        || model.ends_with("/tts-1")
        || model.ends_with("/tts-1-hd")
}

fn looks_like_json_error(bytes: &[u8]) -> bool {
    let trimmed = bytes.trim_ascii_start();
    trimmed.first() == Some(&b'{')
        && serde_json::from_slice::<serde_json::Value>(bytes)
            .ok()
            .is_some_and(|v| v.get("error").is_some())
}

fn openai_error_message(bytes: &[u8]) -> String {
    if let Ok(value) = serde_json::from_slice::<serde_json::Value>(bytes) {
        if let Some(message) = value
            .pointer("/error/message")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        {
            return message.to_string();
        }
        if let Some(message) = value
            .get("message")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        {
            return message.to_string();
        }
    }
    String::from_utf8_lossy(bytes).chars().take(400).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_trailing_slash_from_base_url() {
        assert_eq!(
            normalize_openai_base_url("https://api.openai.com/v1/"),
            "https://api.openai.com/v1"
        );
    }

    #[test]
    fn detects_openrouter_official_tts_gap() {
        assert!(openrouter_official_tts_unavailable(""));
        assert!(openrouter_official_tts_unavailable(
            "openai/gpt-4o-mini-tts-2025-12-15"
        ));
        assert!(openrouter_official_tts_unavailable("tts-1"));
        assert!(!openrouter_official_tts_unavailable(
            "mistralai/voxtral-mini-tts-2603"
        ));
    }

    #[test]
    fn instructions_only_on_mini_tts() {
        assert!(tts_model_accepts_instructions("gpt-4o-mini-tts"));
        assert!(!tts_model_accepts_instructions("tts-1"));
    }
}
