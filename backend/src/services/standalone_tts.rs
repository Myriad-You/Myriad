//! Standalone TTS synthesis (cache + provider) shared by HTTP `/api/speech/tts`
//! and agent `speech.tts`.
//!
//! Lives in the services layer so agent handlers do not depend on `api::speech`.

use crate::services::data_paths::paths;
use crate::services::speech_runtime::{
    SpeechProviderKind, configured_provider, synthesize_openai_tts,
};
use crate::services::tencent_speech_service::{
    TencentSpeechError, TencentSpeechService, TtsRequest,
};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use tokio::fs;

/// Independent TTS cache under `data/phantasi/standalone_tts/{text_hash}/`.
const STANDALONE_TTS_SUBDIR: &str = "standalone_tts";

/// TTS request DTO (HTTP body + agent capability params).
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct TtsApiRequest {
    /// 要转换的文本。Tencent 路径最多 150 个 Unicode scalar。
    pub text: String,
    /// 音色ID（可选，默认爱小溪 AI_XIAO_XI）
    #[serde(default)]
    pub voice_type: Option<i32>,
    /// 语速（缺省 0.0；本层不校验区间）
    #[serde(default)]
    pub speed: Option<f32>,
    /// 音量（Option；本层不填缺省、不校验区间）
    #[serde(default)]
    pub volume: Option<f32>,
    /// 音频格式（缺省 mp3；本层不校验枚举）
    #[serde(default)]
    pub codec: Option<String>,
    /// 采样率（缺省 16000；本层不校验取值）
    #[serde(default)]
    pub sample_rate: Option<i32>,
    /// 情感类别（透传 `emotion_category`；本层不校验音色是否支持）
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
pub fn generate_audio_filename(
    voice_type: i32,
    speed: f32,
    sample_rate: i32,
    codec: &str,
) -> String {
    format!("{}_{}_{}.{}", voice_type, speed as i32, sample_rate, codec)
}

/// Rejection message for a codec that cannot be used as a cache file extension.
pub const INVALID_TTS_CODEC_MESSAGE: &str = "Unsupported audio codec";

/// The codec becomes the cache file extension (here and in the podcast batch
/// cache), so it must stay a single safe path component: a `/` or `..` in it
/// would point the cache read/write outside the cache directory.
pub fn validate_tts_codec(codec: &str) -> Result<(), String> {
    if crate::services::tapp_validation::is_safe_path_component(codec) {
        Ok(())
    } else {
        Err(INVALID_TTS_CODEC_MESSAGE.to_string())
    }
}

fn get_standalone_tts_dir(text_hash: &str) -> PathBuf {
    paths().phantasi.join(STANDALONE_TTS_SUBDIR).join(text_hash)
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
            let path = entry.path();
            if let Some(audio) = read_tts_file(&path).await {
                tracing::info!("Standalone TTS any-voice match: {}", path.display());
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
        TencentSpeechError::ApiKeyNotConfigured => "Speech service is not configured".to_string(),
        TencentSpeechError::NetworkError(_) => "Speech service is unreachable".to_string(),
        TencentSpeechError::ApiError { .. } => "Speech service request failed".to_string(),
        TencentSpeechError::ParseError(_) => "Speech service request failed".to_string(),
        TencentSpeechError::InvalidAudioData(_) => "Invalid audio data".to_string(),
        TencentSpeechError::TextTooLong => "Speech text is too long".to_string(),
    }
}

/// Shared standalone TTS synthesis used by HTTP `/api/speech/tts` and agent `speech.tts`.
///
/// Match configured_provider(): OpenAI|OpenRouter, Gemini, MiniMax, else Tencent (no fallback).
/// Tencent reads+writes cache; OpenAI/MiniMax write-only; Gemini no cache.
pub async fn synthesize_standalone_tts(request: &TtsApiRequest) -> Result<TtsApiResponse, String> {
    if request.text.trim().is_empty() {
        return Err("Speech text is empty".to_string());
    }

    let codec = request.codec.as_deref().unwrap_or("mp3");
    validate_tts_codec(codec)?;
    let provider = configured_provider().await;
    if matches!(
        provider,
        SpeechProviderKind::OpenAi | SpeechProviderKind::OpenRouter
    ) {
        return synthesize_openai_standalone(request, codec).await;
    }
    if provider == SpeechProviderKind::Gemini {
        return synthesize_gemini_standalone(request).await;
    }
    if provider == SpeechProviderKind::MiniMax {
        return synthesize_minimax_standalone(request, codec).await;
    }

    let voice_type = request
        .voice_type
        .unwrap_or(crate::services::tencent_speech_service::voice_types::AI_XIAO_XI);
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
            crate::services::speech_runtime::note_tts(
                "tencent",
                "tts",
                &request.text,
                response.audio.is_some(),
            )
            .await;
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
        Err(e) => {
            crate::services::speech_runtime::note_tts("tencent", "tts", &request.text, false).await;
            Err(tencent_speech_error_message(&e))
        }
    }
}

async fn synthesize_gemini_standalone(request: &TtsApiRequest) -> Result<TtsApiResponse, String> {
    let audio = crate::services::speech_runtime::synthesize_gemini_tts(&request.text).await?;
    let text_hash = generate_text_hash(&request.text);
    let audio_b64 = BASE64.encode(&audio);
    Ok(TtsApiResponse {
        success: true,
        audio: Some(audio_b64),
        session_id: Some(format!("gemini-{}", &text_hash[..8])),
        cached: Some(false),
        error: None,
    })
}

async fn synthesize_minimax_standalone(
    request: &TtsApiRequest,
    codec: &str,
) -> Result<TtsApiResponse, String> {
    let sample_rate = request.sample_rate.unwrap_or(16000);
    let (audio, voice) = crate::services::speech_runtime::synthesize_minimax_tts(
        &request.text,
        codec,
        sample_rate,
        request.speed,
        request.volume,
        request.emotion.as_deref(),
    )
    .await?;
    let text_hash = generate_text_hash(&request.text);
    let speed = request.speed.unwrap_or(0.0);
    let cache_voice = cache_tag_from_voice(&voice);
    let audio_b64 = BASE64.encode(&audio);
    if let Err(e) = write_tts_file(
        &text_hash,
        cache_voice,
        speed,
        sample_rate,
        codec,
        &audio_b64,
    )
    .await
    {
        tracing::warn!("Failed to write MiniMax TTS file: {}", e);
    }
    Ok(TtsApiResponse {
        success: true,
        audio: Some(audio_b64),
        session_id: Some(format!("minimax-{}", &text_hash[..8])),
        cached: Some(false),
        error: None,
    })
}

async fn synthesize_openai_standalone(
    request: &TtsApiRequest,
    codec: &str,
) -> Result<TtsApiResponse, String> {
    let (audio, voice) = synthesize_openai_tts(&request.text, codec).await?;
    let text_hash = generate_text_hash(&request.text);
    let speed = request.speed.unwrap_or(0.0);
    let sample_rate = request.sample_rate.unwrap_or(16000);
    let cache_voice = cache_tag_from_voice(&voice);
    let audio_b64 = BASE64.encode(&audio);
    if let Err(e) = write_tts_file(
        &text_hash,
        cache_voice,
        speed,
        sample_rate,
        codec,
        &audio_b64,
    )
    .await
    {
        tracing::warn!("Failed to write OpenAI TTS file: {}", e);
    }
    Ok(TtsApiResponse {
        success: true,
        audio: Some(audio_b64),
        session_id: Some(format!("openai-{}", &text_hash[..8])),
        cached: Some(false),
        error: None,
    })
}

/// Pack a string voice into the existing i32 cache filename slot.
fn cache_tag_from_voice(voice: &str) -> i32 {
    let mut hash: u32 = 2166136261;
    for byte in voice.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(16777619);
    }
    (hash & 0x7fff_ffff) as i32
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
        assert!(err.contains("empty"), "{err}");
    }

    #[test]
    fn codec_must_be_a_single_path_component() {
        for good in ["mp3", "wav", "pcm", "opus", "flac"] {
            assert!(validate_tts_codec(good).is_ok(), "{good}");
        }
        for bad in [
            "",
            ".",
            "..",
            "mp3/../../x",
            "../etc",
            "a\\b",
            "/abs",
            ".hidden",
        ] {
            assert_eq!(
                validate_tts_codec(bad),
                Err(INVALID_TTS_CODEC_MESSAGE.to_string()),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn traversal_codec_is_rejected_before_cache_lookup() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let req = TtsApiRequest {
            text: "hello".into(),
            voice_type: None,
            speed: None,
            volume: None,
            codec: Some("mp3/../../../../etc/passwd".into()),
            sample_rate: None,
            emotion: None,
            force_regenerate: false,
        };
        let err = rt
            .block_on(synthesize_standalone_tts(&req))
            .expect_err("traversal codec");
        assert_eq!(err, INVALID_TTS_CODEC_MESSAGE);
    }

    #[test]
    fn tencent_error_messages_cover_key_variants() {
        let msg = tencent_speech_error_message(&TencentSpeechError::ApiKeyNotConfigured);
        assert_eq!(msg, "Speech service is not configured");
        assert_eq!(
            tencent_speech_error_message(&TencentSpeechError::TextTooLong),
            "Speech text is too long"
        );
        assert_eq!(
            tencent_speech_error_message(&TencentSpeechError::NetworkError("boom".into())),
            "Speech service is unreachable"
        );
    }
}
