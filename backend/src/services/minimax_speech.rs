//! MiniMax T2A speech synthesis (`/v1/t2a_v2`).
//!
//! Not OpenAI `/audio/speech`. ASR is intentionally out of scope here.

use reqwest::Client;
use serde_json::{json, Value};
use std::time::Duration;

use super::http_client::{apply_proxy, ProxyConfig};
use crate::config::AiVendorSource;

pub const DEFAULT_MINIMAX_HOST: &str = "https://api.minimaxi.com";
pub const DEFAULT_MINIMAX_TTS_MODEL: &str = "speech-2.8-turbo";
pub const DEFAULT_MINIMAX_VOICE: &str = "female-shaonv";

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug)]
pub enum MiniMaxSpeechError {
    ApiKeyNotConfigured,
    NetworkError(String),
    ApiError { message: String },
    InvalidAudioData(String),
    TextTooLong,
}

impl std::fmt::Display for MiniMaxSpeechError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ApiKeyNotConfigured => write!(f, "MiniMax speech API key is not configured"),
            Self::NetworkError(msg) => write!(f, "Network error: {msg}"),
            Self::ApiError { message } => write!(f, "{message}"),
            Self::InvalidAudioData(msg) => write!(f, "Invalid audio data: {msg}"),
            Self::TextTooLong => write!(f, "Text too long for TTS synthesis"),
        }
    }
}

impl std::error::Error for MiniMaxSpeechError {}

pub fn is_minimax_vendor(source: &AiVendorSource) -> bool {
    is_minimax_vendor_fields(&source.kind, &source.slug, &source.preset, &source.base_url)
}

pub fn is_minimax_vendor_fields(kind: &str, slug: &str, preset: &str, base_url: &str) -> bool {
    let kind = kind.trim().to_ascii_lowercase();
    if kind == "minimax" {
        return true;
    }
    let preset = preset.trim().to_ascii_lowercase();
    if preset == "minimax" {
        return true;
    }
    let slug = slug.trim().to_ascii_lowercase();
    if slug == "minimax" || slug.starts_with("minimax-") {
        return true;
    }
    let host = base_url.trim().to_ascii_lowercase();
    host.contains("minimax.io") || host.contains("minimaxi.com") || host.contains("minimax.chat")
}

/// Chat Base URL is `https://api.minimaxi.com/v1`; T2A lives at `/v1/t2a_v2`.
pub fn t2a_url(base_url: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return format!("{DEFAULT_MINIMAX_HOST}/v1/t2a_v2");
    }
    if trimmed.ends_with("/t2a_v2") {
        return trimmed.to_string();
    }
    if trimmed.ends_with("/v1") {
        format!("{trimmed}/t2a_v2")
    } else {
        format!("{trimmed}/v1/t2a_v2")
    }
}

/// Tencent-shaped speed `[-2, 6]` (0 = 1.0×) → MiniMax `[0.5, 2]`.
pub fn map_speed(tencent_speed: Option<f32>) -> f32 {
    let raw = tencent_speed.unwrap_or(0.0);
    (1.0 + raw * 0.25).clamp(0.5, 2.0)
}

/// Tencent-shaped volume `[-10, 10]` (0 = unity) → MiniMax `(0, 10]`.
pub fn map_volume(tencent_volume: Option<f32>) -> f32 {
    let raw = tencent_volume.unwrap_or(0.0);
    (1.0 + raw * 0.1).clamp(0.1, 10.0)
}

pub fn decode_hex_audio(hex: &str) -> Result<Vec<u8>, MiniMaxSpeechError> {
    let compact: String = hex.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    if compact.is_empty() {
        return Err(MiniMaxSpeechError::InvalidAudioData(
            "TTS response audio was empty".to_string(),
        ));
    }
    if compact.len() % 2 != 0 {
        return Err(MiniMaxSpeechError::InvalidAudioData(
            "TTS hex audio had an odd length".to_string(),
        ));
    }
    (0..compact.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&compact[i..i + 2], 16).map_err(|_| {
                MiniMaxSpeechError::InvalidAudioData("TTS hex audio was not hex".to_string())
            })
        })
        .collect()
}

pub struct MiniMaxSpeech {
    client: Client,
    api_key: String,
    t2a_url: String,
}

impl MiniMaxSpeech {
    pub fn new(
        api_key: String,
        base_url: String,
        proxy_config: &ProxyConfig,
    ) -> Result<Self, MiniMaxSpeechError> {
        let key = api_key.trim().to_string();
        if key.is_empty() {
            return Err(MiniMaxSpeechError::ApiKeyNotConfigured);
        }
        let builder = Client::builder()
            .timeout(DEFAULT_TIMEOUT)
            .connect_timeout(CONNECT_TIMEOUT)
            .user_agent("Myriad/1.0");
        let client = apply_proxy(builder, proxy_config)
            .and_then(|b| b.build())
            .map_err(|e| MiniMaxSpeechError::NetworkError(e.to_string()))?;
        Ok(Self {
            client,
            api_key: key,
            t2a_url: t2a_url(&base_url),
        })
    }

    pub async fn text_to_speech(
        &self,
        text: &str,
        model: &str,
        voice: &str,
        codec: &str,
        sample_rate: i32,
        speed: Option<f32>,
        volume: Option<f32>,
        emotion: Option<&str>,
    ) -> Result<Vec<u8>, MiniMaxSpeechError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(MiniMaxSpeechError::InvalidAudioData(
                "Speech text is empty".to_string(),
            ));
        }
        if text.chars().count() > 10_000 {
            return Err(MiniMaxSpeechError::TextTooLong);
        }

        let format = match codec.trim().to_ascii_lowercase().as_str() {
            "wav" => "wav",
            "pcm" => "pcm",
            "flac" => "flac",
            _ => "mp3",
        };
        let rate = match sample_rate {
            8000 | 16000 | 22050 | 24000 | 32000 | 44100 => sample_rate,
            _ => 16000,
        };

        let mut voice_setting = json!({
            "voice_id": voice,
            "speed": map_speed(speed),
            "vol": map_volume(volume),
            "pitch": 0,
        });
        if let Some(emotion) = emotion.map(str::trim).filter(|s| !s.is_empty()) {
            voice_setting["emotion"] = json!(emotion);
        }

        let body = json!({
            "model": model,
            "text": text,
            "stream": false,
            "voice_setting": voice_setting,
            "audio_setting": {
                "sample_rate": rate,
                "bitrate": 128000,
                "format": format,
                "channel": 1
            },
            "language_boost": "auto",
        });

        let response = self
            .client
            .post(&self.t2a_url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| MiniMaxSpeechError::NetworkError(e.to_string()))?;
        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .map_err(|e| MiniMaxSpeechError::NetworkError(e.to_string()))?;
        if !status.is_success() {
            return Err(MiniMaxSpeechError::ApiError {
                message: minimax_error_message(&bytes),
            });
        }
        match parse_t2a_audio(&bytes)? {
            T2aAudio::Bytes(audio) => Ok(audio),
            T2aAudio::Url(url) => self.fetch_audio_url(&url).await,
        }
    }

    async fn fetch_audio_url(&self, url: &str) -> Result<Vec<u8>, MiniMaxSpeechError> {
        if !url.starts_with("https://") {
            return Err(MiniMaxSpeechError::InvalidAudioData(
                "TTS audio URL must be https".to_string(),
            ));
        }
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| MiniMaxSpeechError::NetworkError(e.to_string()))?;
        if !response.status().is_success() {
            return Err(MiniMaxSpeechError::InvalidAudioData(
                "TTS audio URL could not be downloaded".to_string(),
            ));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|e| MiniMaxSpeechError::NetworkError(e.to_string()))?;
        if bytes.is_empty() {
            return Err(MiniMaxSpeechError::InvalidAudioData(
                "TTS audio URL was empty".to_string(),
            ));
        }
        Ok(bytes.to_vec())
    }
}

#[derive(Debug)]
enum T2aAudio {
    Bytes(Vec<u8>),
    Url(String),
}

fn parse_t2a_audio(bytes: &[u8]) -> Result<T2aAudio, MiniMaxSpeechError> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| {
        MiniMaxSpeechError::InvalidAudioData("TTS response was not JSON".to_string())
    })?;
    if let Some(code) = value
        .pointer("/base_resp/status_code")
        .and_then(Value::as_i64)
    {
        if code != 0 {
            let msg = value
                .pointer("/base_resp/status_msg")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .unwrap_or("Speech service request failed");
            return Err(MiniMaxSpeechError::ApiError {
                message: msg.to_string(),
            });
        }
    }
    if let Some(audio) = value
        .pointer("/data/audio")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if audio.starts_with("http://") || audio.starts_with("https://") {
            return Ok(T2aAudio::Url(audio.to_string()));
        }
        return decode_hex_audio(audio).map(T2aAudio::Bytes);
    }
    Err(MiniMaxSpeechError::InvalidAudioData(
        "TTS response missing audio".to_string(),
    ))
}

fn minimax_error_message(bytes: &[u8]) -> String {
    if let Ok(value) = serde_json::from_slice::<Value>(bytes) {
        if let Some(message) = value
            .pointer("/base_resp/status_msg")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        {
            return message.to_string();
        }
        if let Some(message) = value
            .pointer("/error/message")
            .and_then(Value::as_str)
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
    fn detects_minimax_hosts_and_slugs() {
        assert!(is_minimax_vendor_fields(
            "openai_compatible",
            "minimax",
            "minimax",
            "https://api.minimaxi.com/v1"
        ));
        assert!(is_minimax_vendor_fields(
            "openai_compatible",
            "minimax-2",
            "",
            ""
        ));
        assert!(is_minimax_vendor_fields(
            "openai_compatible",
            "custom",
            "",
            "https://api.minimax.io/v1"
        ));
        assert!(!is_minimax_vendor_fields(
            "openai_compatible",
            "openrouter",
            "openrouter",
            "https://openrouter.ai/api/v1"
        ));
    }

    #[test]
    fn builds_t2a_url_from_chat_base() {
        assert_eq!(
            t2a_url("https://api.minimaxi.com/v1"),
            "https://api.minimaxi.com/v1/t2a_v2"
        );
        assert_eq!(
            t2a_url("https://api.minimaxi.com"),
            "https://api.minimaxi.com/v1/t2a_v2"
        );
        assert_eq!(
            t2a_url("https://api.minimax.io/v1/"),
            "https://api.minimax.io/v1/t2a_v2"
        );
        assert_eq!(t2a_url(""), "https://api.minimaxi.com/v1/t2a_v2");
    }

    #[test]
    fn maps_tencent_speed_and_volume_to_minimax() {
        assert!((map_speed(None) - 1.0).abs() < f32::EPSILON);
        assert!((map_speed(Some(0.0)) - 1.0).abs() < f32::EPSILON);
        assert!((map_speed(Some(4.0)) - 2.0).abs() < f32::EPSILON);
        assert!((map_speed(Some(-2.0)) - 0.5).abs() < f32::EPSILON);
        assert!((map_volume(None) - 1.0).abs() < f32::EPSILON);
        assert!((map_volume(Some(10.0)) - 2.0).abs() < f32::EPSILON);
    }

    #[test]
    fn decodes_hex_audio() {
        assert_eq!(decode_hex_audio("4869").unwrap(), b"Hi");
        assert_eq!(decode_hex_audio(" 48 69 ").unwrap(), b"Hi");
        assert!(decode_hex_audio("zzz").is_err());
        assert!(decode_hex_audio("abc").is_err());
    }

    #[test]
    fn parses_successful_t2a_json() {
        let body = br#"{
            "data": { "audio": "4869", "status": 2 },
            "base_resp": { "status_code": 0, "status_msg": "success" }
        }"#;
        match parse_t2a_audio(body).unwrap() {
            T2aAudio::Bytes(audio) => assert_eq!(audio, b"Hi"),
            T2aAudio::Url(url) => panic!("expected bytes, got {url}"),
        }
    }

    #[test]
    fn rejects_t2a_business_error() {
        let body = br#"{
            "data": { "audio": "" },
            "base_resp": { "status_code": 2013, "status_msg": "invalid voice_id" }
        }"#;
        match parse_t2a_audio(body) {
            Err(MiniMaxSpeechError::ApiError { message }) => {
                assert_eq!(message, "invalid voice_id");
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}
