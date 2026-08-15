//! Standalone TTS synthesis (cache + Tencent) shared by HTTP `/api/speech/tts`
//! and agent `speech.tts`.
//!
//! Lives in the services layer so agent handlers do not depend on `api::speech`.

use crate::services::data_paths::paths;
use crate::services::tencent_speech_service::{
    TencentSpeechError, TencentSpeechService, TtsRequest,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use tokio::fs;

/// Independent TTS cache under `data/brew/standalone_tts/{text_hash}/`.
const STANDALONE_TTS_SUBDIR: &str = "standalone_tts";

/// TTS request DTO (HTTP body + agent capability params).
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct TtsApiRequest {
    /// 要转换的文本（中文最大150字，英文最大500字母）
    pub text: String,
    /// 音色ID（可选，默认10510000-晓晓）
    #[serde(default)]
    pub voice_type: Option<i32>,
    /// 语速 [-2, 6]，默认0
    #[serde(default)]
    pub speed: Option<f32>,
    /// 音量 [-10, 10]，默认0
    #[serde(default)]
    pub volume: Option<f32>,
    /// 返回格式: wav, mp3, pcm，默认mp3
    #[serde(default)]
    pub codec: Option<String>,
    /// 采样率: 8000, 16000, 24000，默认16000
    #[serde(default)]
    pub sample_rate: Option<i32>,
    /// 情感类别（仅多情感音色支持）
    #[serde(default)]
    pub emotion: Option<String>,
    /// 强制重新合成（跳过 exact + any-voice 缓存；与 batch TTS 语义对齐）
    #[serde(default)]
    pub force_regenerate: bool,
}

/// TTS response DTO (HTTP body + agent capability result).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TtsApiResponse {
    /// 是否成功
    pub success: bool,
    /// Base64编码的音频数据
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio: Option<String>,
    /// 会话ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// 是否来自缓存
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached: Option<bool>,
    /// 错误信息
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Generate stable text hash used as standalone cache directory name.
pub fn generate_text_hash(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    hex::encode(hasher.finalize())
}

/// Audio filename for a voice/speed/sample_rate/codec combination.
pub fn generate_audio_filename(voice_type: i32, speed: f32, sample_rate: i32, codec: &str) -> String {
    format!("{}_{}_{}.{}", voice_type, speed as i32, sample_rate, codec)
}

fn get_standalone_tts_dir(text_hash: &str) -> PathBuf {
    paths().brew.join(STANDALONE_TTS_SUBDIR).join(text_hash)
}

fn get_standalone_tts_file_path(
    text_hash: &str,
    voice_type: i32,
    speed: f32,
    sample_rate: i32,
    codec: &str,
) -> PathBuf {
    get_standalone_tts_dir(text_hash).join(generate_audio_filename(
        voice_type,
        speed,
        sample_rate,
        codec,
    ))
}

async fn read_tts_file(path: &PathBuf) -> Option<String> {
    match fs::read(path).await {
        Ok(data) => {
            tracing::debug!("TTS file read: {}", path.display());
            Some(BASE64.encode(&data))
        }
        Err(_) => None,
    }
}

async fn find_exact_tts(
    text_hash: &str,
    voice_type: i32,
    speed: f32,
    sample_rate: i32,
    codec: &str,
) -> Option<String> {
    let path = get_standalone_tts_file_path(text_hash, voice_type, speed, sample_rate, codec);
    if let Some(audio) = read_tts_file(&path).await {
        tracing::info!("Standalone TTS exact match: {}", path.display());
        return Some(audio);
    }
    None
}

async fn find_any_tts(text_hash: &str, codec: &str) -> Option<String> {
    let tts_dir = get_standalone_tts_dir(text_hash);

    let mut entries = match fs::read_dir(&tts_dir).await {
        Ok(entries) => entries,
        Err(_) => return None,
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let file_name = entry.file_name();
        let file_name_str = file_name.to_string_lossy();

        if file_name_str.ends_with(&format!(".{codec}")) {
            if let Some(audio) = read_tts_file(&entry.path()).await {
                tracing::info!("Standalone TTS any-voice match: {}", entry.path().display());
                return Some(audio);
            }
        }
    }

    None
}

async fn write_tts_file(
    text_hash: &str,
    voice_type: i32,
    speed: f32,
    sample_rate: i32,
    codec: &str,
    audio_base64: &str,
) -> Result<(), std::io::Error> {
    let tts_dir = get_standalone_tts_dir(text_hash);
    fs::create_dir_all(&tts_dir).await?;

    let audio_data = BASE64
        .decode(audio_base64)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    let file_path = get_standalone_tts_file_path(text_hash, voice_type, speed, sample_rate, codec);
    fs::write(&file_path, &audio_data).await?;

    tracing::info!(
        "Standalone TTS file written: {} ({} bytes)",
        file_path.display(),
        audio_data.len()
    );
    Ok(())
}

/// Map Tencent speech errors to operator-facing messages (no HTTP status).
pub fn tencent_speech_error_message(error: &TencentSpeechError) -> String {
    match error {
        TencentSpeechError::ApiKeyNotConfigured => {
            // Same guidance string used by agent error catalog.
            crate::services::agent::response_agent::tts_not_configured()
        }
        TencentSpeechError::NetworkError(msg) => msg.clone(),
        TencentSpeechError::ApiError { code, message } => {
            format!("[{code}] {message}")
        }
        TencentSpeechError::ParseError(msg) => msg.clone(),
        TencentSpeechError::InvalidAudioData(msg) => msg.clone(),
        TencentSpeechError::TextTooLong => {
            "文本过长，中文最大150字，英文最大500字母".to_string()
        }
    }
}

/// Shared standalone TTS synthesis used by HTTP `/api/speech/tts` and agent `speech.tts`.
///
/// Returns the same cache + Tencent path as the product API (base64 audio when successful).
pub async fn synthesize_standalone_tts(request: &TtsApiRequest) -> Result<TtsApiResponse, String> {
    if request.text.trim().is_empty() {
        return Err("文本不能为空".to_string());
    }

    let codec = request.codec.as_deref().unwrap_or("mp3");
    let voice_type = request.voice_type.unwrap_or(10510000);
    let speed = request.speed.unwrap_or(0.0);
    let sample_rate = request.sample_rate.unwrap_or(16000);
    let text_hash = generate_text_hash(&request.text);

    // force_regenerate：跳过 exact + any-voice 缓存，强制按指定音色重新合成
    if !request.force_regenerate {
        if let Some(cached_audio) =
            find_exact_tts(&text_hash, voice_type, speed, sample_rate, codec).await
        {
            return Ok(TtsApiResponse {
                success: true,
                audio: Some(cached_audio),
                session_id: Some(format!("cached-{}", &text_hash[..8])),
                cached: Some(true),
                error: None,
            });
        }

        if let Some(cached_audio) = find_any_tts(&text_hash, codec).await {
            return Ok(TtsApiResponse {
                success: true,
                audio: Some(cached_audio),
                session_id: Some(format!("cached-any-{}", &text_hash[..8])),
                cached: Some(true),
                error: None,
            });
        }
    }

    let service = TencentSpeechService::new()
        .await
        .map_err(|e| tencent_speech_error_message(&e))?;

    let tts_request = TtsRequest {
        text: request.text.clone(),
        voice_type: request.voice_type,
        speed: request.speed,
        volume: request.volume,
        codec: Some(codec.to_string()),
        sample_rate: request.sample_rate,
        emotion_category: request.emotion.clone(),
        ..Default::default()
    };

    match service.text_to_speech(tts_request).await {
        Ok(response) => {
            if let Some(ref audio) = response.audio {
                if let Err(e) =
                    write_tts_file(&text_hash, voice_type, speed, sample_rate, codec, audio).await
                {
                    tracing::warn!("Failed to write TTS file: {}", e);
                }
            }
            Ok(TtsApiResponse {
                success: true,
                audio: response.audio,
                session_id: response.session_id,
                cached: Some(false),
                error: None,
            })
        }
        Err(e) => Err(tencent_speech_error_message(&e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_hash_is_stable_sha256_hex() {
        let h1 = generate_text_hash("hello");
        let h2 = generate_text_hash("hello");
        let h3 = generate_text_hash("hello!");
        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
        assert_eq!(h1.len(), 64);
        assert!(h1.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn audio_filename_embeds_voice_speed_rate_codec() {
        let name = generate_audio_filename(10510000, 0.0, 16000, "mp3");
        assert_eq!(name, "10510000_0_16000.mp3");
        let name2 = generate_audio_filename(1, 1.5, 24000, "wav");
        assert_eq!(name2, "1_1_24000.wav"); // speed cast to i32
    }

    #[test]
    fn empty_text_is_rejected_without_network() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let req = TtsApiRequest {
            text: "   ".into(),
            voice_type: None,
            speed: None,
            volume: None,
            codec: None,
            sample_rate: None,
            emotion: None,
            force_regenerate: false,
        };
        let err = rt
            .block_on(synthesize_standalone_tts(&req))
            .expect_err("empty text");
        assert!(err.contains("不能为空"), "{err}");
    }

    #[test]
    fn tencent_error_messages_cover_key_variants() {
        let msg = tencent_speech_error_message(&TencentSpeechError::ApiKeyNotConfigured);
        assert!(msg.contains("TTS") || msg.contains("腾讯云") || msg.contains("未配置"), "{msg}");
        assert_eq!(
            tencent_speech_error_message(&TencentSpeechError::TextTooLong),
            "文本过长，中文最大150字，英文最大500字母"
        );
        assert!(tencent_speech_error_message(&TencentSpeechError::NetworkError(
            "boom".into()
        ))
        .contains("boom"));
    }
}
