//! Resolve the configured speech provider and run file STT / TTS.

use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use serde::Serialize;

use crate::GLOBAL_DYNAMIC_CONFIG;

use super::gemini_media::{self, GeminiMediaError};
use super::http_client::ProxyConfig;
use super::minimax_speech::{
    DEFAULT_MINIMAX_HOST, DEFAULT_MINIMAX_TTS_MODEL, DEFAULT_MINIMAX_VOICE, MiniMaxSpeech,
    MiniMaxSpeechError, is_minimax_vendor,
};
use super::openai_compatible_speech::{
    OpenAiCompatibleSpeech, OpenAiSpeechError, openrouter_official_tts_unavailable,
};
use super::tencent_speech_service::{
    AsrRequest, TencentSpeechError, TencentSpeechService, TtsRequest,
};

pub const OPENROUTER_SPEECH_BASE_URL: &str = "https://openrouter.ai/api/v1";
pub const DEFAULT_OPENAI_STT_MODEL: &str = "gpt-transcribe";
pub const DEFAULT_OPENAI_TTS_MODEL: &str = "gpt-4o-mini-tts";
pub const DEFAULT_OPENAI_TTS_VOICE: &str = "marin";
pub const DEFAULT_OPENROUTER_STT_MODEL: &str = "openai/gpt-transcribe";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeechProviderKind {
    Tencent,
    OpenAi,
    OpenRouter,
    Gemini,
    MiniMax,
}

impl SpeechProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tencent => "tencent",
            Self::OpenAi => "openai",
            Self::OpenRouter => "openrouter",
            Self::Gemini => "gemini",
            Self::MiniMax => "minimax",
        }
    }

    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "openai" | "openai_compatible" => Self::OpenAi,
            "openrouter" => Self::OpenRouter,
            "gemini" => Self::Gemini,
            "minimax" => Self::MiniMax,
            _ => Self::Tencent,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SpeechProbe {
    pub provider: String,
    pub available: bool,
    pub tts_enabled: bool,
    pub asr_enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SpeechTestResult {
    pub success: bool,
    pub provider: String,
    pub tts_ok: bool,
    pub asr_ok: bool,
    pub tts_skipped: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcript: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub async fn configured_provider() -> SpeechProviderKind {
    let config = GLOBAL_DYNAMIC_CONFIG.read().await;
    if let Some(source) = config.find_vendor_source(&config.speech_source) {
        if is_minimax_vendor(&source) {
            return SpeechProviderKind::MiniMax;
        }
        return SpeechProviderKind::parse(&source.kind);
    }
    SpeechProviderKind::parse(&config.speech_provider)
}

pub async fn speech_probe() -> SpeechProbe {
    let provider = configured_provider().await;
    match provider {
        SpeechProviderKind::Tencent => match TencentSpeechService::new().await {
            Ok(_) => SpeechProbe {
                provider: provider.as_str().to_string(),
                available: true,
                tts_enabled: true,
                asr_enabled: true,
                error: None,
            },
            Err(e) => SpeechProbe {
                provider: provider.as_str().to_string(),
                available: false,
                tts_enabled: false,
                asr_enabled: false,
                error: Some(tencent_message(&e)),
            },
        },
        SpeechProviderKind::OpenAi | SpeechProviderKind::OpenRouter => {
            match resolve_openai_speech().await {
                Ok(resolved) => SpeechProbe {
                    provider: provider.as_str().to_string(),
                    available: true,
                    tts_enabled: resolved.tts_available,
                    asr_enabled: true,
                    error: if resolved.tts_available {
                        None
                    } else {
                        Some(
                            resolved
                                .tts_skip_reason
                                .unwrap_or_else(|| "Official speech requires OpenAI".to_string()),
                        )
                    },
                },
                Err(e) => SpeechProbe {
                    provider: provider.as_str().to_string(),
                    available: false,
                    tts_enabled: false,
                    asr_enabled: false,
                    error: Some(openai_message(&e)),
                },
            }
        }
        SpeechProviderKind::Gemini => match resolve_gemini_speech().await {
            Ok(_) => SpeechProbe {
                provider: provider.as_str().to_string(),
                available: true,
                tts_enabled: true,
                asr_enabled: true,
                error: None,
            },
            Err(e) => SpeechProbe {
                provider: provider.as_str().to_string(),
                available: false,
                tts_enabled: false,
                asr_enabled: false,
                error: Some(gemini_message(&e)),
            },
        },
        SpeechProviderKind::MiniMax => match resolve_minimax_speech().await {
            Ok(_) => {
                let asr_enabled = fallback_asr_available().await;
                SpeechProbe {
                    provider: provider.as_str().to_string(),
                    available: true,
                    tts_enabled: true,
                    asr_enabled,
                    error: if asr_enabled {
                        None
                    } else {
                        Some("Listening needs Tencent Cloud, OpenAI, or Gemini".to_string())
                    },
                }
            }
            Err(e) => SpeechProbe {
                provider: provider.as_str().to_string(),
                available: false,
                tts_enabled: false,
                asr_enabled: false,
                error: Some(minimax_message(&e)),
            },
        },
    }
}

pub async fn synthesize_gemini_tts(text: &str) -> Result<Vec<u8>, String> {
    let resolved = resolve_gemini_speech()
        .await
        .map_err(|e| gemini_message(&e))?;
    let result = gemini_media::text_to_speech(
        &resolved.base_url,
        &resolved.api_key,
        &resolved.tts_model,
        text,
        &resolved.voice,
    )
    .await
    .map_err(|e| gemini_message(&e));
    note_tts("gemini", &resolved.tts_model, text, result.is_ok()).await;
    result
}

pub async fn synthesize_openai_tts(text: &str, codec: &str) -> Result<(Vec<u8>, String), String> {
    let resolved = resolve_openai_speech()
        .await
        .map_err(|e| openai_message(&e))?;
    if !resolved.tts_available {
        return Err(resolved
            .tts_skip_reason
            .unwrap_or_else(|| "Official speech requires OpenAI".to_string()));
    }
    let audio = resolved
        .client
        .text_to_speech(
            text,
            &resolved.tts_model,
            &resolved.voice,
            codec,
            Some("Speak in natural, clear short sentences."),
        )
        .await
        .map_err(|e| openai_message(&e));
    let provider = configured_provider().await;
    note_tts(provider.as_str(), &resolved.tts_model, text, audio.is_ok()).await;
    Ok((audio?, resolved.voice))
}

pub async fn synthesize_minimax_tts(
    text: &str,
    codec: &str,
    sample_rate: i32,
    speed: Option<f32>,
    volume: Option<f32>,
    emotion: Option<&str>,
) -> Result<(Vec<u8>, String), String> {
    let resolved = resolve_minimax_speech()
        .await
        .map_err(|e| minimax_message(&e))?;
    let audio = resolved
        .client
        .text_to_speech(
            text,
            &resolved.tts_model,
            &resolved.voice,
            codec,
            sample_rate,
            speed,
            volume,
            emotion,
        )
        .await
        .map_err(|e| minimax_message(&e));
    note_tts("minimax", &resolved.tts_model, text, audio.is_ok()).await;
    Ok((audio?, resolved.voice))
}

pub async fn transcribe_bytes(
    audio: Vec<u8>,
    format: &str,
    language: Option<&str>,
) -> Result<String, String> {
    let provider = configured_provider().await;
    let audio_len = audio.len();
    let (model, result) = match provider {
        SpeechProviderKind::Tencent => {
            let service = TencentSpeechService::new()
                .await
                .map_err(|e| tencent_message(&e))?;
            let engine = match language.unwrap_or("zh") {
                code if code.starts_with("en") => "16k_en",
                code if code.starts_with("ja") => "16k_ja",
                code if code.starts_with("ko") => "16k_ko",
                code if code.starts_with("fr") || code.starts_with("de") => "16k_en",
                _ => "16k_zh",
            };
            let request = AsrRequest {
                eng_ser_vice_type: engine.to_string(),
                source_type: 1,
                voice_format: format.to_string(),
                data: Some(BASE64.encode(&audio)),
                data_len: Some(audio.len() as i32),
                ..Default::default()
            };
            let result = service
                .speech_to_text(request)
                .await
                .map(|response| response.result.unwrap_or_default())
                .map_err(|e| tencent_message(&e));
            (engine.to_string(), result)
        }
        SpeechProviderKind::OpenAi | SpeechProviderKind::OpenRouter => {
            let resolved = resolve_openai_speech()
                .await
                .map_err(|e| openai_message(&e))?;
            let filename = format!("speech.{format}");
            let mime = audio_mime(format);
            let model = resolved.stt_model.clone();
            let result = resolved
                .client
                .speech_to_text(audio, &filename, mime, &resolved.stt_model, language)
                .await
                .map_err(|e| openai_message(&e));
            (model, result)
        }
        SpeechProviderKind::Gemini => {
            let resolved = resolve_gemini_speech()
                .await
                .map_err(|e| gemini_message(&e))?;
            let model = resolved.stt_model.clone();
            let result = gemini_media::speech_to_text(
                &resolved.base_url,
                &resolved.api_key,
                &resolved.stt_model,
                audio,
                audio_mime(format),
                language,
            )
            .await
            .map_err(|e| gemini_message(&e));
            (model, result)
        }
        SpeechProviderKind::MiniMax => {
            let (executed, model, result) =
                fallback_transcribe_bytes(audio, format, language).await;
            let ledger_provider = stt_ledger_provider(provider, executed);
            note_stt(
                ledger_provider,
                &model,
                audio_len,
                result.as_deref().unwrap_or(""),
                result.is_ok(),
            )
            .await;
            return result;
        }
    };
    note_stt(
        provider.as_str(),
        &model,
        audio_len,
        result.as_deref().unwrap_or(""),
        result.is_ok(),
    )
    .await;
    result
}

/// MiniMax ASR is TTS-only; cost must follow the provider that actually ran.
pub(crate) fn stt_ledger_provider(
    configured: SpeechProviderKind,
    executed: Option<SpeechProviderKind>,
) -> &'static str {
    match configured {
        SpeechProviderKind::MiniMax => executed
            .map(SpeechProviderKind::as_str)
            .unwrap_or("none"),
        other => other.as_str(),
    }
}

pub(crate) async fn note_tts(provider: &str, model: &str, text: &str, ok: bool) {
    let (input_tokens, output_tokens) = crate::services::ai_cost_ledger::estimate_tts_tokens(text);
    crate::services::ai_cost_ledger::record_ai_tokens_from_attribution(
        provider,
        model,
        input_tokens,
        output_tokens,
        if ok { "completed" } else { "failed" },
        if ok { None } else { Some("AI_PROVIDER_ERROR") },
    )
    .await;
}

pub(crate) async fn note_stt(
    provider: &str,
    model: &str,
    audio_bytes: usize,
    transcript: &str,
    ok: bool,
) {
    let (input_tokens, output_tokens) =
        crate::services::ai_cost_ledger::estimate_stt_tokens(audio_bytes, transcript);
    crate::services::ai_cost_ledger::record_ai_tokens_from_attribution(
        provider,
        model,
        input_tokens,
        output_tokens,
        if ok { "completed" } else { "failed" },
        if ok { None } else { Some("AI_PROVIDER_ERROR") },
    )
    .await;
}

pub async fn test_speech_roundtrip() -> SpeechTestResult {
    let provider = configured_provider().await;
    let probe = speech_probe().await;
    if !probe.available {
        return SpeechTestResult {
            success: false,
            provider: provider.as_str().to_string(),
            tts_ok: false,
            asr_ok: false,
            tts_skipped: false,
            audio: None,
            transcript: None,
            error: probe.error,
        };
    }

    if !probe.tts_enabled {
        return SpeechTestResult {
            success: true,
            provider: provider.as_str().to_string(),
            tts_ok: false,
            asr_ok: true,
            tts_skipped: true,
            audio: None,
            transcript: None,
            error: probe.error.or_else(|| {
                Some("Transcription is ready; official speech requires OpenAI".to_string())
            }),
        };
    }

    let phrase = "Speech test succeeded";
    let tts = match provider {
        SpeechProviderKind::Tencent => {
            let service = match TencentSpeechService::new().await {
                Ok(s) => s,
                Err(e) => {
                    return fail(provider, tencent_message(&e));
                }
            };
            match service
                .text_to_speech(TtsRequest {
                    text: phrase.to_string(),
                    codec: Some("mp3".to_string()),
                    ..Default::default()
                })
                .await
            {
                Ok(response) => match response.audio {
                    Some(audio) => {
                        note_tts(provider.as_str(), "tts", phrase, true).await;
                        audio
                    }
                    None => {
                        note_tts(provider.as_str(), "tts", phrase, false).await;
                        return fail(provider, "Speech service returned no audio".to_string());
                    }
                },
                Err(e) => {
                    note_tts(provider.as_str(), "tts", phrase, false).await;
                    return fail(provider, tencent_message(&e));
                }
            }
        }
        SpeechProviderKind::OpenAi | SpeechProviderKind::OpenRouter => {
            match synthesize_openai_tts(phrase, "mp3").await {
                Ok((bytes, _)) => BASE64.encode(bytes),
                Err(e) => return fail(provider, e),
            }
        }
        SpeechProviderKind::Gemini => match synthesize_gemini_tts(phrase).await {
            Ok(bytes) => BASE64.encode(bytes),
            Err(e) => return fail(provider, e),
        },
        SpeechProviderKind::MiniMax => {
            match synthesize_minimax_tts(phrase, "mp3", 16000, None, None, None).await {
                Ok((bytes, _)) => BASE64.encode(bytes),
                Err(e) => return fail(provider, e),
            }
        }
    };

    let decoded = match BASE64.decode(&tts) {
        Ok(bytes) => bytes,
        Err(e) => {
            tracing::warn!("Speech test audio decode failed: {e}");
            return fail(provider, "Invalid audio data".to_string());
        }
    };
    let asr_format = if provider == SpeechProviderKind::Gemini {
        "wav"
    } else {
        "mp3"
    };
    let transcript = transcribe_bytes(decoded, asr_format, Some("zh")).await.ok();
    SpeechTestResult {
        success: true,
        provider: provider.as_str().to_string(),
        tts_ok: true,
        asr_ok: transcript.as_deref().is_some_and(|s| !s.trim().is_empty()),
        tts_skipped: false,
        audio: Some(tts),
        transcript,
        error: None,
    }
}

struct ResolvedGeminiSpeech {
    api_key: String,
    base_url: String,
    stt_model: String,
    tts_model: String,
    voice: String,
}

async fn resolve_gemini_speech() -> Result<ResolvedGeminiSpeech, GeminiMediaError> {
    let config = GLOBAL_DYNAMIC_CONFIG.read().await;
    let source = if config.speech_source.trim().is_empty() {
        None
    } else {
        config.find_vendor_source(&config.speech_source)
    };
    let api_key = source
        .as_ref()
        .map(|item| config.resolve_source_credential(item).api_key)
        .unwrap_or_else(|| config.shared_gemini_api_key())
        .ok_or_else(|| {
            GeminiMediaError::NotConfigured("Gemini API key is not configured".to_string())
        })?;
    let base_url = source
        .as_ref()
        .map(|item| item.base_url.trim().to_string())
        .filter(|item| !item.is_empty())
        .or_else(|| {
            config
                .gemini_base_url
                .as_deref()
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "https://generativelanguage.googleapis.com".to_string());
    let stt_model = if config.speech_stt_model.trim().is_empty() {
        "gemini-3.6-flash".to_string()
    } else {
        config.speech_stt_model.trim().to_string()
    };
    let tts_model = if config.speech_tts_model.trim().is_empty() {
        "gemini-2.5-flash-preview-tts".to_string()
    } else {
        config.speech_tts_model.trim().to_string()
    };
    let voice = if config.speech_tts_voice.trim().is_empty() {
        "Kore".to_string()
    } else {
        config.speech_tts_voice.trim().to_string()
    };
    Ok(ResolvedGeminiSpeech {
        api_key,
        base_url,
        stt_model,
        tts_model,
        voice,
    })
}

struct ResolvedMiniMaxSpeech {
    client: MiniMaxSpeech,
    tts_model: String,
    voice: String,
}

async fn resolve_minimax_speech() -> Result<ResolvedMiniMaxSpeech, MiniMaxSpeechError> {
    let config = GLOBAL_DYNAMIC_CONFIG.read().await;
    let source = if config.speech_source.trim().is_empty() {
        None
    } else {
        config.find_vendor_source(&config.speech_source)
    };
    let source = source.filter(|item| is_minimax_vendor(item)).or_else(|| {
        config
            .ai_vendor_sources
            .iter()
            .find(|item| item.enabled && is_minimax_vendor(item))
            .cloned()
    });
    let api_key = source
        .as_ref()
        .and_then(|item| config.resolve_source_credential(item).api_key)
        .ok_or(MiniMaxSpeechError::ApiKeyNotConfigured)?;
    let base_url = source
        .as_ref()
        .map(|item| item.base_url.trim().to_string())
        .filter(|item| !item.is_empty())
        .unwrap_or_else(|| DEFAULT_MINIMAX_HOST.to_string());
    let tts_model = if config.speech_tts_model.trim().is_empty() {
        DEFAULT_MINIMAX_TTS_MODEL.to_string()
    } else {
        config.speech_tts_model.trim().to_string()
    };
    let voice = if config.speech_tts_voice.trim().is_empty() {
        DEFAULT_MINIMAX_VOICE.to_string()
    } else {
        config.speech_tts_voice.trim().to_string()
    };
    drop(config);
    let proxy = ProxyConfig::from_dynamic_config().await;
    let client = MiniMaxSpeech::new(api_key, base_url, &proxy)?;
    Ok(ResolvedMiniMaxSpeech {
        client,
        tts_model,
        voice,
    })
}

async fn fallback_asr_available() -> bool {
    TencentSpeechService::from_any_configured().await.is_ok()
        || fallback_openai_stt().await.is_ok()
        || fallback_gemini_stt().await.is_ok()
}

async fn fallback_transcribe_bytes(
    audio: Vec<u8>,
    format: &str,
    language: Option<&str>,
) -> (Option<SpeechProviderKind>, String, Result<String, String>) {
    if let Ok(service) = TencentSpeechService::from_any_configured().await {
        let engine = match language.unwrap_or("zh") {
            code if code.starts_with("en") => "16k_en",
            code if code.starts_with("ja") => "16k_ja",
            code if code.starts_with("ko") => "16k_ko",
            code if code.starts_with("fr") || code.starts_with("de") => "16k_en",
            _ => "16k_zh",
        };
        let request = AsrRequest {
            eng_ser_vice_type: engine.to_string(),
            source_type: 1,
            voice_format: format.to_string(),
            data: Some(BASE64.encode(&audio)),
            data_len: Some(audio.len() as i32),
            ..Default::default()
        };
        let result = service
            .speech_to_text(request)
            .await
            .map(|response| response.result.unwrap_or_default())
            .map_err(|e| tencent_message(&e));
        return (Some(SpeechProviderKind::Tencent), engine.to_string(), result);
    }
    if let Ok(resolved) = fallback_openai_stt().await {
        let filename = format!("speech.{format}");
        let mime = audio_mime(format);
        let model = resolved.stt_model.clone();
        let result = resolved
            .client
            .speech_to_text(audio, &filename, mime, &resolved.stt_model, language)
            .await
            .map_err(|e| openai_message(&e));
        return (Some(SpeechProviderKind::OpenAi), model, result);
    }
    if let Ok(resolved) = fallback_gemini_stt().await {
        let model = resolved.stt_model.clone();
        let result = gemini_media::speech_to_text(
            &resolved.base_url,
            &resolved.api_key,
            &resolved.stt_model,
            audio,
            audio_mime(format),
            language,
        )
        .await
        .map_err(|e| gemini_message(&e));
        return (Some(SpeechProviderKind::Gemini), model, result);
    }
    (
        None,
        "none".to_string(),
        Err("Listening needs Tencent Cloud, OpenAI, or Gemini".to_string()),
    )
}

struct FallbackOpenAiStt {
    client: OpenAiCompatibleSpeech,
    stt_model: String,
}

async fn fallback_openai_stt() -> Result<FallbackOpenAiStt, OpenAiSpeechError> {
    let config = GLOBAL_DYNAMIC_CONFIG.read().await;
    let source = config.ai_vendor_sources.iter().find(|item| {
        item.enabled
            && !is_minimax_vendor(item)
            && matches!(
                item.kind.trim().to_ascii_lowercase().as_str(),
                "openai" | "openrouter" | "openai_compatible"
            )
            && config.resolve_source_credential(item).api_key.is_some()
    });
    let (api_key, base_url, stt_model, referer) = if let Some(source) = source {
        let kind = SpeechProviderKind::parse(&source.kind);
        let key = config
            .resolve_source_credential(source)
            .api_key
            .unwrap_or_default();
        let base = source.base_url.trim().to_string();
        if kind == SpeechProviderKind::OpenRouter {
            (
                key,
                if base.is_empty() {
                    OPENROUTER_SPEECH_BASE_URL.to_string()
                } else {
                    base
                },
                DEFAULT_OPENROUTER_STT_MODEL.to_string(),
                config.base_url.clone(),
            )
        } else {
            (
                key,
                if base.is_empty() {
                    config.shared_openai_base_url()
                } else {
                    base
                },
                DEFAULT_OPENAI_STT_MODEL.to_string(),
                None,
            )
        }
    } else if let Some(key) = config.shared_openai_api_key() {
        (
            key,
            config.shared_openai_base_url(),
            DEFAULT_OPENAI_STT_MODEL.to_string(),
            None,
        )
    } else if let Some(key) = config.shared_openrouter_api_key() {
        (
            key,
            OPENROUTER_SPEECH_BASE_URL.to_string(),
            DEFAULT_OPENROUTER_STT_MODEL.to_string(),
            config.base_url.clone(),
        )
    } else {
        return Err(OpenAiSpeechError::ApiKeyNotConfigured);
    };
    drop(config);
    let proxy = ProxyConfig::from_dynamic_config().await;
    let client = OpenAiCompatibleSpeech::new(api_key, base_url, &proxy, referer)?;
    Ok(FallbackOpenAiStt { client, stt_model })
}

async fn fallback_gemini_stt() -> Result<ResolvedGeminiSpeech, GeminiMediaError> {
    let config = GLOBAL_DYNAMIC_CONFIG.read().await;
    let source = config
        .ai_vendor_sources
        .iter()
        .find(|item| item.enabled && item.kind.trim().eq_ignore_ascii_case("gemini"));
    let api_key = source
        .map(|item| config.resolve_source_credential(item).api_key)
        .unwrap_or_else(|| config.shared_gemini_api_key())
        .ok_or_else(|| {
            GeminiMediaError::NotConfigured("Gemini API key is not configured".to_string())
        })?;
    let base_url = config
        .ai_vendor_sources
        .iter()
        .find(|item| item.enabled && item.kind.trim().eq_ignore_ascii_case("gemini"))
        .map(|item| item.base_url.trim().to_string())
        .filter(|item| !item.is_empty())
        .or_else(|| {
            config
                .gemini_base_url
                .as_deref()
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "https://generativelanguage.googleapis.com".to_string());
    Ok(ResolvedGeminiSpeech {
        api_key,
        base_url,
        stt_model: "gemini-3.6-flash".to_string(),
        tts_model: String::new(),
        voice: String::new(),
    })
}

struct ResolvedOpenAiSpeech {
    client: OpenAiCompatibleSpeech,
    stt_model: String,
    tts_model: String,
    voice: String,
    tts_available: bool,
    tts_skip_reason: Option<String>,
}

async fn resolve_openai_speech() -> Result<ResolvedOpenAiSpeech, OpenAiSpeechError> {
    let config = GLOBAL_DYNAMIC_CONFIG.read().await;
    let source = if config.speech_source.trim().is_empty() {
        None
    } else {
        config.find_vendor_source(&config.speech_source)
    };
    let provider = if let Some(source) = source.as_ref() {
        SpeechProviderKind::parse(&source.kind)
    } else {
        SpeechProviderKind::parse(&config.speech_provider)
    };
    let stt_override = config.speech_stt_model.trim().to_string();
    let tts_override = config.speech_tts_model.trim().to_string();
    let voice_override = config.speech_tts_voice.trim().to_string();
    let site_url = config.base_url.clone();
    let source_key = source
        .as_ref()
        .map(|item| config.resolve_source_credential(item).api_key);
    let source_base = source
        .as_ref()
        .map(|item| item.base_url.trim().to_string())
        .filter(|item| !item.is_empty());
    let openrouter_key = source_key
        .clone()
        .unwrap_or_else(|| config.shared_openrouter_api_key())
        .unwrap_or_default();
    let openai_key = source_key
        .unwrap_or_else(|| config.shared_openai_api_key())
        .unwrap_or_default();
    let openai_base = source_base.unwrap_or_else(|| config.shared_openai_base_url());
    drop(config);

    let (api_key, base_url, default_stt, default_tts, default_voice, referer) = match provider {
        SpeechProviderKind::OpenRouter => (
            openrouter_key,
            OPENROUTER_SPEECH_BASE_URL.to_string(),
            DEFAULT_OPENROUTER_STT_MODEL,
            "",
            DEFAULT_OPENAI_TTS_VOICE,
            site_url,
        ),
        SpeechProviderKind::OpenAi => (
            openai_key,
            openai_base,
            DEFAULT_OPENAI_STT_MODEL,
            DEFAULT_OPENAI_TTS_MODEL,
            DEFAULT_OPENAI_TTS_VOICE,
            None,
        ),
        SpeechProviderKind::Tencent | SpeechProviderKind::Gemini | SpeechProviderKind::MiniMax => {
            return Err(OpenAiSpeechError::ApiKeyNotConfigured);
        }
    };

    let stt_model = if stt_override.is_empty() {
        default_stt.to_string()
    } else {
        normalize_model_slug(provider, &stt_override)
    };
    let tts_model = if tts_override.is_empty() {
        default_tts.to_string()
    } else {
        normalize_model_slug(provider, &tts_override)
    };
    let voice = if voice_override.is_empty() {
        default_voice.to_string()
    } else {
        voice_override
    };

    let (tts_available, tts_skip_reason) = if provider == SpeechProviderKind::OpenRouter
        && openrouter_official_tts_unavailable(&tts_model)
    {
        (false, Some("Official speech requires OpenAI".to_string()))
    } else {
        (true, None)
    };

    let proxy = ProxyConfig::from_dynamic_config().await;
    let client = OpenAiCompatibleSpeech::new(api_key, base_url, &proxy, referer)?;
    Ok(ResolvedOpenAiSpeech {
        client,
        stt_model,
        tts_model,
        voice,
        tts_available,
        tts_skip_reason,
    })
}

fn normalize_model_slug(provider: SpeechProviderKind, model: &str) -> String {
    let trimmed = model.trim();
    if provider != SpeechProviderKind::OpenRouter || trimmed.contains('/') {
        return trimmed.to_string();
    }
    format!("openai/{trimmed}")
}

fn audio_mime(format: &str) -> &'static str {
    match format.to_ascii_lowercase().as_str() {
        "mp3" | "mpeg" | "mpga" => "audio/mpeg",
        "wav" => "audio/wav",
        "m4a" => "audio/mp4",
        "webm" => "audio/webm",
        "ogg" => "audio/ogg",
        "flac" => "audio/flac",
        _ => "application/octet-stream",
    }
}

fn tencent_message(error: &TencentSpeechError) -> String {
    super::standalone_tts::tencent_speech_error_message(error)
}

fn gemini_message(error: &GeminiMediaError) -> String {
    match error {
        GeminiMediaError::NotConfigured(_) => "Speech service is not configured".to_string(),
        GeminiMediaError::Provider(_) | GeminiMediaError::InvalidResponse(_) => {
            "Speech service request failed".to_string()
        }
    }
}

fn openai_message(error: &OpenAiSpeechError) -> String {
    match error {
        OpenAiSpeechError::ApiKeyNotConfigured => "Speech service is not configured".to_string(),
        OpenAiSpeechError::NetworkError(_) => "Speech service is unreachable".to_string(),
        OpenAiSpeechError::ApiError { .. } => "Speech service request failed".to_string(),
        OpenAiSpeechError::InvalidAudioData(_) => "Invalid audio data".to_string(),
    }
}

fn minimax_message(error: &MiniMaxSpeechError) -> String {
    match error {
        MiniMaxSpeechError::ApiKeyNotConfigured => "Speech service is not configured".to_string(),
        MiniMaxSpeechError::NetworkError(_) => "Speech service is unreachable".to_string(),
        MiniMaxSpeechError::ApiError { message } => {
            if message.trim().is_empty() {
                "Speech service request failed".to_string()
            } else {
                message.clone()
            }
        }
        MiniMaxSpeechError::InvalidAudioData(_) => "Invalid audio data".to_string(),
        MiniMaxSpeechError::TextTooLong => "Speech text is too long".to_string(),
    }
}

fn fail(provider: SpeechProviderKind, error: String) -> SpeechTestResult {
    SpeechTestResult {
        success: false,
        provider: provider.as_str().to_string(),
        tts_ok: false,
        asr_ok: false,
        tts_skipped: false,
        audio: None,
        transcript: None,
        error: Some(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_provider_kinds() {
        assert_eq!(
            SpeechProviderKind::parse("openai"),
            SpeechProviderKind::OpenAi
        );
        assert_eq!(
            SpeechProviderKind::parse("OpenRouter"),
            SpeechProviderKind::OpenRouter
        );
        assert_eq!(SpeechProviderKind::parse(""), SpeechProviderKind::Tencent);
        assert_eq!(
            SpeechProviderKind::parse("gemini"),
            SpeechProviderKind::Gemini
        );
        assert_eq!(
            SpeechProviderKind::parse("minimax"),
            SpeechProviderKind::MiniMax
        );
    }

    #[test]
    fn minimax_asr_ledger_follows_the_executed_provider() {
        assert_eq!(
            stt_ledger_provider(
                SpeechProviderKind::MiniMax,
                Some(SpeechProviderKind::Tencent)
            ),
            "tencent"
        );
        assert_eq!(
            stt_ledger_provider(
                SpeechProviderKind::MiniMax,
                Some(SpeechProviderKind::OpenAi)
            ),
            "openai"
        );
        assert_eq!(
            stt_ledger_provider(
                SpeechProviderKind::MiniMax,
                Some(SpeechProviderKind::Gemini)
            ),
            "gemini"
        );
        assert_eq!(
            stt_ledger_provider(SpeechProviderKind::MiniMax, None),
            "none"
        );
        assert_ne!(
            stt_ledger_provider(
                SpeechProviderKind::MiniMax,
                Some(SpeechProviderKind::Tencent)
            ),
            "minimax"
        );
        assert_eq!(
            stt_ledger_provider(SpeechProviderKind::Tencent, None),
            "tencent"
        );
    }

    #[test]
    fn prefixes_openrouter_bare_model_ids() {
        assert_eq!(
            normalize_model_slug(SpeechProviderKind::OpenRouter, "gpt-transcribe"),
            "openai/gpt-transcribe"
        );
        assert_eq!(
            normalize_model_slug(SpeechProviderKind::OpenAi, "gpt-transcribe"),
            "gpt-transcribe"
        );
    }
}
