//! Shengwang / Agora Conversational AI Engine (join / leave / interrupt).

use serde::Serialize;
use serde_json::{json, Value};

use super::agora_rtc_token::{build_rtc_token_now, AgoraTokenError};
use super::http_client::{apply_proxy, ProxyConfig};
use super::minimax_speech::{is_minimax_vendor, DEFAULT_MINIMAX_TTS_MODEL, DEFAULT_MINIMAX_VOICE};
use crate::config::DynamicConfig;
use crate::GLOBAL_DYNAMIC_CONFIG;

pub const DEFAULT_AGORA_API_BASE: &str = "https://api.agora.io/cn";
pub const USER_RTC_UID: u32 = 1;
pub const AGENT_RTC_UID: u32 = 8888;
const TOKEN_TTL_SECS: u32 = 3600;

#[derive(Debug)]
pub enum AgoraConvoError {
    NotConfigured(String),
    Token(AgoraTokenError),
    Network(String),
    Api { status: u16, message: String },
}

impl std::fmt::Display for AgoraConvoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotConfigured(msg) => write!(f, "{msg}"),
            Self::Token(e) => write!(f, "{e}"),
            Self::Network(msg) => write!(f, "Network error: {msg}"),
            Self::Api { status, message } => write!(f, "HTTP {status}: {message}"),
        }
    }
}

impl std::error::Error for AgoraConvoError {}

impl From<AgoraTokenError> for AgoraConvoError {
    fn from(value: AgoraTokenError) -> Self {
        Self::Token(value)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ConvoSession {
    pub app_id: String,
    pub channel: String,
    pub uid: u32,
    pub token: String,
    pub agent_id: String,
}

fn json_str(value: &Value, pointers: &[&str]) -> Option<String> {
    pointers.iter().find_map(|pointer| {
        value
            .pointer(pointer)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    })
}

fn parse_join_agent(bytes: &[u8], http_status: u16) -> Result<(String, String), AgoraConvoError> {
    let value: Value = serde_json::from_slice(bytes).map_err(|e| AgoraConvoError::Api {
        status: http_status,
        message: e.to_string(),
    })?;
    let agent_id = json_str(
        &value,
        &["/agent_id", "/agentId", "/data/agent_id", "/data/agentId"],
    )
    .unwrap_or_default();
    let status = json_str(&value, &["/status", "/data/status"]).unwrap_or_default();
    Ok((agent_id, status))
}

pub struct AgoraEndpoint {
    pub app_id: String,
    pub certificate: String,
    pub customer_id: String,
    pub customer_secret: String,
    pub api_base: String,
}

fn agora_source_complete(source: &crate::config::AiVendorSource) -> bool {
    nonempty_opt(source.app_id.as_ref())
        && nonempty_opt(source.api_key.as_ref())
        && nonempty_opt(source.secret_id.as_ref())
        && nonempty_opt(source.secret_key.as_ref())
}

pub fn resolve_agora_endpoint(config: &DynamicConfig) -> Result<AgoraEndpoint, AgoraConvoError> {
    let sources = config.effective_vendor_sources();
    if let Some(source) = sources
        .iter()
        .find(|item| item.enabled && item.is_agora() && agora_source_complete(item))
    {
        let api_base = if source.base_url.trim().is_empty() {
            DEFAULT_AGORA_API_BASE.to_string()
        } else {
            source.base_url.trim().to_string()
        };
        return Ok(AgoraEndpoint {
            app_id: source.app_id.as_deref().unwrap_or("").trim().to_string(),
            certificate: source.api_key.as_deref().unwrap_or("").trim().to_string(),
            customer_id: source.secret_id.as_deref().unwrap_or("").trim().to_string(),
            customer_secret: source
                .secret_key
                .as_deref()
                .unwrap_or("")
                .trim()
                .to_string(),
            api_base,
        });
    }
    if sources.iter().any(|item| item.is_agora()) {
        return Err(AgoraConvoError::NotConfigured(
            "Shengwang realtime talk needs App ID, certificate, Customer ID, and Customer Secret"
                .to_string(),
        ));
    }
    if config.agora_convo_enabled
        && nonempty(&config.agora_app_id)
        && nonempty(&config.agora_app_certificate)
        && nonempty(&config.agora_customer_id)
        && nonempty_opt(config.agora_customer_secret.as_ref())
    {
        let api_base = if config.agora_api_base.trim().is_empty() {
            DEFAULT_AGORA_API_BASE.to_string()
        } else {
            config.agora_api_base.trim().to_string()
        };
        return Ok(AgoraEndpoint {
            app_id: config.agora_app_id.trim().to_string(),
            certificate: config.agora_app_certificate.trim().to_string(),
            customer_id: config.agora_customer_id.trim().to_string(),
            customer_secret: config
                .agora_customer_secret
                .as_deref()
                .unwrap_or("")
                .trim()
                .to_string(),
            api_base,
        });
    }
    Err(AgoraConvoError::NotConfigured(
        "Shengwang conversational AI is not configured".to_string(),
    ))
}

pub fn convo_configured(config: &DynamicConfig) -> bool {
    resolve_agora_endpoint(config).is_ok()
}

fn nonempty(value: &str) -> bool {
    !value.trim().is_empty()
}

fn nonempty_opt(value: Option<&String>) -> bool {
    value.map(|s| !s.trim().is_empty()).unwrap_or(false)
}

pub fn join_url(api_base: &str, app_id: &str) -> String {
    format!(
        "{}/api/conversational-ai-agent/v2/projects/{}/join",
        api_base.trim().trim_end_matches('/'),
        app_id.trim()
    )
}

pub fn agent_action_url(api_base: &str, app_id: &str, agent_id: &str, action: &str) -> String {
    format!(
        "{}/api/conversational-ai-agent/v2/projects/{}/agents/{}/{}",
        api_base.trim().trim_end_matches('/'),
        app_id.trim(),
        agent_id.trim(),
        action
    )
}

pub fn build_join_body(
    channel: &str,
    agent_token: &str,
    user_uid: u32,
    llm: &LlmEndpoint,
    tts: &TtsEndpoint,
    language: &str,
    system_prompt: &str,
) -> Value {
    json!({
        "name": channel,
        "properties": {
            "channel": channel,
            "token": agent_token,
            "agent_rtc_uid": AGENT_RTC_UID.to_string(),
            "remote_rtc_uids": [user_uid.to_string()],
            "enable_string_uid": false,
            "idle_timeout": 30,
            "asr": { "language": language },
            "llm": {
                "url": llm.url,
                "api_key": llm.api_key,
                "system_messages": [{
                    "role": "system",
                    "content": system_prompt
                }],
                "greeting_message": "",
                "failure_message": "",
                "max_history": 10,
                "params": { "model": llm.model }
            },
            "tts": {
                "vendor": "minimax",
                "params": {
                    "key": tts.api_key,
                    "model": tts.model,
                    "voice_setting": {
                        "voice_id": tts.voice,
                        "speed": 1,
                        "vol": 1,
                        "pitch": 0
                    },
                    "audio_setting": { "sample_rate": 16000 }
                }
            }
        }
    })
}

#[derive(Debug, Clone)]
pub struct LlmEndpoint {
    pub url: String,
    pub api_key: String,
    pub model: String,
}

#[derive(Debug, Clone)]
pub struct TtsEndpoint {
    pub api_key: String,
    pub model: String,
    pub voice: String,
}

pub fn resolve_llm_endpoint(config: &DynamicConfig) -> Result<LlmEndpoint, AgoraConvoError> {
    let sources = config.effective_vendor_sources();
    let source = sources.iter().find(|item| {
        item.enabled
            && !is_minimax_vendor(item)
            && matches!(
                item.kind.trim().to_ascii_lowercase().as_str(),
                "openai" | "openrouter" | "openai_compatible"
            )
            && DynamicConfig::nonempty_opt(item.api_key.as_ref()).is_some()
    });
    if let Some(source) = source {
        let key = DynamicConfig::nonempty_opt(source.api_key.as_ref()).unwrap_or_default();
        let mut base = source.base_url.trim().to_string();
        if base.is_empty() {
            base = if source.kind.trim().eq_ignore_ascii_case("openrouter") {
                "https://openrouter.ai/api/v1".to_string()
            } else {
                config.shared_openai_base_url()
            };
        }
        let model = if source.kind.trim().eq_ignore_ascii_case("openrouter") {
            config
                .openai_model
                .trim()
                .to_string()
                .if_empty("openai/gpt-oss-20b:free")
        } else {
            config
                .openai_model
                .trim()
                .to_string()
                .if_empty("gpt-4o-mini")
        };
        return Ok(LlmEndpoint {
            url: chat_completions_url(&base),
            api_key: key,
            model,
        });
    }
    if let Some(key) = config.shared_openrouter_api_key() {
        return Ok(LlmEndpoint {
            url: chat_completions_url("https://openrouter.ai/api/v1"),
            api_key: key,
            model: config
                .openai_model
                .trim()
                .to_string()
                .if_empty("openai/gpt-oss-20b:free"),
        });
    }
    if let Some(key) = config.shared_openai_api_key() {
        return Ok(LlmEndpoint {
            url: chat_completions_url(&config.shared_openai_base_url()),
            api_key: key,
            model: config
                .openai_model
                .trim()
                .to_string()
                .if_empty("gpt-4o-mini"),
        });
    }
    Err(AgoraConvoError::NotConfigured(
        "Realtime talk needs a public OpenAI-compatible language model".to_string(),
    ))
}

pub fn resolve_minimax_tts(config: &DynamicConfig) -> Result<TtsEndpoint, AgoraConvoError> {
    let sources = config.effective_vendor_sources();
    let source = sources
        .iter()
        .find(|item| item.enabled && is_minimax_vendor(item))
        .ok_or_else(|| {
            AgoraConvoError::NotConfigured("Realtime talk needs a MiniMax provider".to_string())
        })?;
    let api_key = DynamicConfig::nonempty_opt(source.api_key.as_ref()).ok_or_else(|| {
        AgoraConvoError::NotConfigured("MiniMax API key is not configured".to_string())
    })?;
    let model = if config.speech_tts_model.trim().is_empty() {
        DEFAULT_MINIMAX_TTS_MODEL.to_string()
    } else {
        config.speech_tts_model.trim().to_string()
    };
    let voice = if config.speech_tts_voice.trim().is_empty() {
        DEFAULT_MINIMAX_VOICE.to_string()
    } else {
        config.speech_tts_voice.trim().to_string()
    };
    Ok(TtsEndpoint {
        api_key,
        model,
        voice,
    })
}

fn chat_completions_url(base: &str) -> String {
    let trimmed = base.trim().trim_end_matches('/');
    if trimmed.ends_with("/chat/completions") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/chat/completions")
    }
}

trait IfEmpty {
    fn if_empty(self, fallback: &str) -> String;
}

impl IfEmpty for String {
    fn if_empty(self, fallback: &str) -> String {
        if self.is_empty() {
            fallback.to_string()
        } else {
            self
        }
    }
}

pub async fn start_session(
    language: &str,
    system_prompt: &str,
) -> Result<ConvoSession, AgoraConvoError> {
    let config = GLOBAL_DYNAMIC_CONFIG.read().await;
    let agora = resolve_agora_endpoint(&config)?;
    let llm = resolve_llm_endpoint(&config)?;
    let tts = resolve_minimax_tts(&config)?;
    drop(config);
    let AgoraEndpoint {
        app_id,
        certificate,
        customer_id,
        customer_secret,
        api_base,
    } = agora;

    let channel = format!("merope-{}", uuid::Uuid::new_v4().simple());
    let user_token = build_rtc_token_now(
        &app_id,
        &certificate,
        &channel,
        USER_RTC_UID,
        TOKEN_TTL_SECS,
    )?;
    let agent_token = build_rtc_token_now(
        &app_id,
        &certificate,
        &channel,
        AGENT_RTC_UID,
        TOKEN_TTL_SECS,
    )?;
    let body = build_join_body(
        &channel,
        &agent_token,
        USER_RTC_UID,
        &llm,
        &tts,
        language,
        system_prompt,
    );

    let proxy = ProxyConfig::from_dynamic_config().await;
    let client = apply_proxy(
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent("Myriad/1.0"),
        &proxy,
    )
    .and_then(|b| b.build())
    .map_err(|e| AgoraConvoError::Network(e.to_string()))?;

    let url = join_url(&api_base, &app_id);
    let response = client
        .post(url)
        .basic_auth(&customer_id, Some(&customer_secret))
        .json(&body)
        .send()
        .await
        .map_err(|e| AgoraConvoError::Network(e.to_string()))?;
    let status = response.status();
    let bytes = response
        .bytes()
        .await
        .map_err(|e| AgoraConvoError::Network(e.to_string()))?;
    if !status.is_success() {
        return Err(AgoraConvoError::Api {
            status: status.as_u16(),
            message: api_error_message(&bytes),
        });
    }
    let http_status = status.as_u16();
    let (agent_id, agent_status) = parse_join_agent(&bytes, http_status)?;
    if agent_id.is_empty() {
        return Err(AgoraConvoError::Api {
            status: http_status,
            message: "Agent id missing".to_string(),
        });
    }
    tracing::info!(
        agent_id = %agent_id,
        channel = %channel,
        status = %agent_status,
        "Agora conversational agent joined"
    );
    Ok(ConvoSession {
        app_id,
        channel,
        uid: USER_RTC_UID,
        token: user_token,
        agent_id,
    })
}

pub async fn stop_session(agent_id: &str) -> Result<(), AgoraConvoError> {
    post_agent_action(agent_id, "leave").await
}

pub async fn interrupt_session(agent_id: &str) -> Result<(), AgoraConvoError> {
    post_agent_action(agent_id, "interrupt").await
}

async fn post_agent_action(agent_id: &str, action: &str) -> Result<(), AgoraConvoError> {
    let config = GLOBAL_DYNAMIC_CONFIG.read().await;
    let AgoraEndpoint {
        app_id,
        customer_id,
        customer_secret,
        api_base,
        ..
    } = resolve_agora_endpoint(&config)?;
    drop(config);

    let proxy = ProxyConfig::from_dynamic_config().await;
    let client = apply_proxy(
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .user_agent("Myriad/1.0"),
        &proxy,
    )
    .and_then(|b| b.build())
    .map_err(|e| AgoraConvoError::Network(e.to_string()))?;
    let url = agent_action_url(&api_base, &app_id, agent_id, action);
    let response = client
        .post(url)
        .basic_auth(&customer_id, Some(&customer_secret))
        .json(&json!({}))
        .send()
        .await
        .map_err(|e| AgoraConvoError::Network(e.to_string()))?;
    if !response.status().is_success() {
        let status = response.status().as_u16();
        let bytes = response.bytes().await.unwrap_or_default();
        return Err(AgoraConvoError::Api {
            status,
            message: api_error_message(&bytes),
        });
    }
    Ok(())
}

fn api_error_message(bytes: &[u8]) -> String {
    if let Ok(value) = serde_json::from_slice::<Value>(bytes) {
        if let Some(message) = value
            .pointer("/message")
            .or_else(|| value.pointer("/reason"))
            .or_else(|| value.pointer("/error/message"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        {
            return message.to_string();
        }
    }
    let text = String::from_utf8_lossy(bytes);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        "Speech service request failed".to_string()
    } else {
        trimmed.chars().take(400).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_and_leave_urls() {
        assert_eq!(
            join_url("https://api.agora.io/cn/", "abc"),
            "https://api.agora.io/cn/api/conversational-ai-agent/v2/projects/abc/join"
        );
        assert_eq!(
            agent_action_url("https://api.agora.io/cn", "abc", "ag1", "leave"),
            "https://api.agora.io/cn/api/conversational-ai-agent/v2/projects/abc/agents/ag1/leave"
        );
    }

    #[test]
    fn join_body_uses_minimax_and_chat_completions() {
        let llm = LlmEndpoint {
            url: "https://openrouter.ai/api/v1/chat/completions".to_string(),
            api_key: "sk-or".to_string(),
            model: "openai/gpt-oss-20b:free".to_string(),
        };
        let tts = TtsEndpoint {
            api_key: "mm-key".to_string(),
            model: "speech-2.8-turbo".to_string(),
            voice: "female-shaonv".to_string(),
        };
        let body = build_join_body(
            "room-1",
            "agent-token",
            1,
            &llm,
            &tts,
            "zh-CN",
            "you are merope",
        );
        assert_eq!(body["properties"]["tts"]["vendor"], "minimax");
        assert_eq!(
            body["properties"]["tts"]["params"]["voice_setting"]["voice_id"],
            "female-shaonv"
        );
        assert_eq!(body["properties"]["llm"]["url"], llm.url);
        assert_eq!(body["properties"]["remote_rtc_uids"][0], "1");
        assert_eq!(body["properties"]["asr"]["language"], "zh-CN");
    }

    #[test]
    fn chat_url_appends_completions() {
        assert_eq!(
            chat_completions_url("https://api.openai.com/v1"),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            chat_completions_url("https://api.openai.com/v1/chat/completions"),
            "https://api.openai.com/v1/chat/completions"
        );
    }

    #[test]
    fn default_config_keeps_realtime_talk_off() {
        assert!(!convo_configured(&crate::config::DynamicConfig::default()));
    }

    #[test]
    fn vendor_source_enables_realtime_talk() {
        let mut config = crate::config::DynamicConfig::default();
        config.ai_vendor_sources = vec![crate::config::AiVendorSource {
            slug: "agora".to_string(),
            kind: "agora".to_string(),
            display_name: "Shengwang / Agora".to_string(),
            enabled: true,
            preset: "agora".to_string(),
            api_key: Some("c".repeat(32)),
            secret_id: Some("cid".to_string()),
            secret_key: Some("csec".to_string()),
            app_id: Some("a".repeat(32)),
            base_url: "https://api.agora.io/cn".to_string(),
            ..crate::config::AiVendorSource::default()
        }];
        assert!(convo_configured(&config));
        config.ai_vendor_sources[0].enabled = false;
        assert!(!convo_configured(&config));
    }

    #[test]
    fn join_response_reads_nested_agent_id() {
        let (id, status) =
            parse_join_agent(br#"{"data":{"agentId":"ag-9","status":"RUNNING"}}"#, 200).unwrap();
        assert_eq!(id, "ag-9");
        assert_eq!(status, "RUNNING");
        let (id, _) = parse_join_agent(br#"{"agent_id":"ag-1"}"#, 200).unwrap();
        assert_eq!(id, "ag-1");
    }
}
