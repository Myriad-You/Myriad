//! Shengwang / Agora Conversational AI Engine (join / leave / interrupt).

use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use super::agora_rtc_token::{build_rtc_rtm_token_now, AgoraTokenError};
use super::http_client::{apply_proxy, ProxyConfig};
use super::minimax_speech::{is_minimax_vendor, DEFAULT_MINIMAX_TTS_MODEL, DEFAULT_MINIMAX_VOICE};
use crate::config::DynamicConfig;
use crate::GLOBAL_DYNAMIC_CONFIG;

pub const DEFAULT_AGORA_API_BASE: &str = "https://api.agora.io/cn";
const TOKEN_TTL_SECS: u32 = 3600;

#[derive(Debug)]
pub enum AgoraConvoError {
    SessionUnavailable,
    NotConfigured(String),
    Token(AgoraTokenError),
    Network(String),
    Api { status: u16, message: String },
}

impl std::fmt::Display for AgoraConvoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SessionUnavailable => write!(f, "Realtime session is unavailable"),
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
    pub agent_uid: u32,
    pub token: String,
    pub agent_id: String,
}

#[derive(Clone)]
struct OwnedSession {
    user_id: i32,
    expires_at: Instant,
    // Bind controls to the credentials used to create the session, even when
    // the active vendor configuration changes. Never return these to a client.
    endpoint: Arc<AgoraEndpoint>,
    chat: Arc<super::agora_chat::ChatSession>,
}

impl OwnedSession {
    fn permits(&self, user_id: i32, now: Instant) -> bool {
        user_id > 0 && self.user_id == user_id && now < self.expires_at
    }
}

static SESSIONS: LazyLock<Mutex<HashMap<String, OwnedSession>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

async fn owned_session(user_id: i32, agent_id: &str) -> Result<OwnedSession, AgoraConvoError> {
    SESSIONS
        .lock()
        .await
        .get(agent_id)
        .filter(|session| session.permits(user_id, Instant::now()))
        .cloned()
        .ok_or(AgoraConvoError::SessionUnavailable)
}

pub fn conversation_language(language: &str) -> &'static str {
    let lower = language.trim().to_ascii_lowercase().replace('_', "-");
    if lower.starts_with("zh-tw")
        || lower.starts_with("zh-hk")
        || lower.starts_with("zh-mo")
        || lower.contains("hant")
    {
        "zh-TW"
    } else if lower.starts_with("zh") {
        "zh-CN"
    } else if lower.starts_with("ja") {
        "ja-JP"
    } else {
        "en-US"
    }
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
        && resolve_minimax_tts(config).is_ok()
        && config
            .resolve_strict_lite_ai_config()
            .and_then(|resolved| resolved.api_key)
            .is_some_and(|key| !key.trim().is_empty())
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
    agent_uid: u32,
    llm: &LlmEndpoint,
    tts: &TtsEndpoint,
    language: &str,
) -> Value {
    json!({
        "name": channel,
        "properties": {
            "channel": channel,
            "token": agent_token,
            "agent_rtc_uid": agent_uid.to_string(),
            "remote_rtc_uids": [user_uid.to_string()],
            "enable_string_uid": false,
            "idle_timeout": 30,
            "advanced_features": { "enable_rtm": true },
            "parameters": {
                "data_channel": "rtm",
                "enable_error_message": true
            },
            "asr": { "language": language },
            "llm": {
                "vendor": "custom",
                "url": llm.url,
                "api_key": llm.api_key,
                "system_messages": [],
                "greeting_message": "",
                "failure_message": "",
                "max_history": 0,
                "params": { "model": "myriad-chat", "stream": true }
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

#[derive(Clone)]
pub struct LlmEndpoint {
    pub url: String,
    pub api_key: String,
}

#[derive(Clone)]
pub struct TtsEndpoint {
    pub api_key: String,
    pub model: String,
    pub voice: String,
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

pub async fn start_session(
    chat: Arc<super::agora_chat::ChatSession>,
    language: &str,
    llm: &LlmEndpoint,
) -> Result<ConvoSession, AgoraConvoError> {
    let user_id = chat.claims.sub.parse().unwrap_or(0);
    if user_id <= 0 {
        return Err(AgoraConvoError::SessionUnavailable);
    }
    let config = GLOBAL_DYNAMIC_CONFIG.read().await;
    let agora = Arc::new(resolve_agora_endpoint(&config)?);
    let tts = resolve_minimax_tts(&config)?;
    drop(config);
    let AgoraEndpoint {
        app_id,
        certificate,
        customer_id,
        customer_secret,
        api_base,
    } = agora.as_ref();

    let expires_at = Instant::now() + Duration::from_secs(TOKEN_TTL_SECS.into());

    let channel = format!("merope-{}", uuid::Uuid::new_v4().simple());
    // RTM IDs are app-wide, not channel-local. Fixed IDs would kick users (and
    // agents) out of other conversations. Both participants join RTC and RTM.
    let (user_uid, agent_uid) = transport_uids(uuid::Uuid::new_v4());
    let user_token =
        build_rtc_rtm_token_now(app_id, certificate, &channel, user_uid, TOKEN_TTL_SECS)?;
    let agent_token =
        build_rtc_rtm_token_now(app_id, certificate, &channel, agent_uid, TOKEN_TTL_SECS)?;
    let body = build_join_body(
        &channel,
        &agent_token,
        user_uid,
        agent_uid,
        llm,
        &tts,
        language,
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

    let url = join_url(api_base, app_id);
    let response = client
        .post(url)
        .basic_auth(customer_id, Some(customer_secret))
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
    SESSIONS.lock().await.insert(
        agent_id.clone(),
        OwnedSession {
            user_id,
            expires_at,
            endpoint: agora.clone(),
            chat,
        },
    );
    let expiring_id = agent_id.clone();
    tokio::spawn(async move {
        tokio::time::sleep_until(expires_at.into()).await;
        let expired = {
            let mut sessions = SESSIONS.lock().await;
            if sessions
                .get(&expiring_id)
                .is_some_and(|session| session.expires_at == expires_at)
            {
                sessions.remove(&expiring_id)
            } else {
                None
            }
        };
        if let Some(session) = expired {
            session.chat.close().await;
            let _ = post_agent_action(&session.endpoint, &expiring_id, "leave").await;
        }
    });
    Ok(ConvoSession {
        app_id: app_id.clone(),
        channel,
        uid: user_uid,
        agent_uid,
        token: user_token,
        agent_id,
    })
}

fn transport_uids(seed: uuid::Uuid) -> (u32, u32) {
    let user_uid = ((seed.as_u128() as u32) & 0x7fff_fffe).max(2);
    (user_uid, user_uid + 1)
}

pub async fn stop_session(user_id: i32, agent_id: &str) -> Result<(), AgoraConvoError> {
    let session = owned_session(user_id, agent_id).await?;
    session.chat.close().await;
    // Keep the owner until a failed leave can be retried or the lease expires.
    post_agent_action(&session.endpoint, agent_id, "leave").await?;
    SESSIONS.lock().await.remove(agent_id);
    Ok(())
}

pub async fn interrupt_session(user_id: i32, agent_id: &str) -> Result<(), AgoraConvoError> {
    let session = owned_session(user_id, agent_id).await?;
    session.chat.interrupt().await;
    post_agent_action(&session.endpoint, agent_id, "interrupt").await
}

pub async fn chat_session(
    user_id: i32,
    agent_id: &str,
) -> Result<Arc<super::agora_chat::ChatSession>, AgoraConvoError> {
    Ok(owned_session(user_id, agent_id).await?.chat)
}

async fn post_agent_action(
    endpoint: &AgoraEndpoint,
    agent_id: &str,
    action: &str,
) -> Result<(), AgoraConvoError> {
    let AgoraEndpoint {
        app_id,
        customer_id,
        customer_secret,
        api_base,
        ..
    } = endpoint;

    let proxy = ProxyConfig::from_dynamic_config().await;
    let client = apply_proxy(
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .user_agent("Myriad/1.0"),
        &proxy,
    )
    .and_then(|b| b.build())
    .map_err(|e| AgoraConvoError::Network(e.to_string()))?;
    let url = agent_action_url(api_base, app_id, agent_id, action);
    let response = client
        .post(url)
        .basic_auth(customer_id, Some(customer_secret))
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

    #[tokio::test]
    async fn controls_require_the_live_owner_not_just_an_agent_id() {
        let now = Instant::now();
        let (chat, _) = super::super::agora_chat::ChatSession::register(
            crate::middleware::auth::mint_session_claims(7, "tester", false, false, 0),
            "chat-test".into(),
        )
        .await
        .unwrap();
        let session = OwnedSession {
            user_id: 7,
            expires_at: now + Duration::from_secs(10),
            endpoint: Arc::new(AgoraEndpoint {
                app_id: "app".into(),
                certificate: String::new(),
                customer_id: String::new(),
                customer_secret: String::new(),
                api_base: String::new(),
            }),
            chat: chat.clone(),
        };
        assert!(session.permits(7, now));
        assert!(!session.permits(8, now));
        assert!(!session.permits(0, now));
        assert!(!session.permits(-1, now));
        assert!(!session.permits(7, session.expires_at));
        chat.close().await;
    }

    #[tokio::test]
    async fn unknown_or_foreign_sessions_fail_before_provider_resolution() {
        assert!(matches!(
            interrupt_session(7, "unknown").await,
            Err(AgoraConvoError::SessionUnavailable)
        ));
        assert!(matches!(
            stop_session(8, "unknown").await,
            Err(AgoraConvoError::SessionUnavailable)
        ));
    }

    #[test]
    fn japanese_is_not_silently_transcribed_as_chinese() {
        assert_eq!(conversation_language("ja-JP"), "ja-JP");
        assert_eq!(conversation_language("JA_jp"), "ja-JP");
        assert_eq!(conversation_language("en-GB"), "en-US");
        assert_eq!(conversation_language("zh-CN"), "zh-CN");
        assert_eq!(conversation_language("zh-TW"), "zh-TW");
        assert_eq!(conversation_language("zh-HK"), "zh-TW");
        assert_eq!(conversation_language("fr-FR"), "en-US");
    }

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
            url: "https://myriad.example/api/speech/convo/chat/completions".to_string(),
            api_key: "ephemeral-callback-only".to_string(),
        };
        let tts = TtsEndpoint {
            api_key: "mm-key".to_string(),
            model: "speech-2.8-turbo".to_string(),
            voice: "female-shaonv".to_string(),
        };
        let body = build_join_body("room-1", "agent-token", 1, 8888, &llm, &tts, "zh-CN");
        assert_eq!(body["properties"]["tts"]["vendor"], "minimax");
        assert_eq!(
            body["properties"]["tts"]["params"]["voice_setting"]["voice_id"],
            "female-shaonv"
        );
        assert_eq!(body["properties"]["llm"]["url"], llm.url);
        assert_eq!(body["properties"]["llm"]["vendor"], "custom");
        assert_eq!(body["properties"]["llm"]["params"]["model"], "myriad-chat");
        assert_eq!(body["properties"]["llm"]["system_messages"], json!([]));
        assert_eq!(body["properties"]["remote_rtc_uids"][0], "1");
        assert_eq!(body["properties"]["agent_rtc_uid"], "8888");
        assert_eq!(body["properties"]["asr"]["language"], "zh-CN");
        assert_eq!(body["properties"]["advanced_features"]["enable_rtm"], true);
        assert_eq!(body["properties"]["parameters"]["data_channel"], "rtm");
    }

    #[test]
    fn default_config_keeps_realtime_talk_off() {
        assert!(!convo_configured(&crate::config::DynamicConfig::default()));
    }

    #[test]
    fn transport_uids_are_nonzero_distinct_and_session_scoped() {
        for seed in [0, u128::MAX, 42, 123456789] {
            let (user, agent) = transport_uids(uuid::Uuid::from_u128(seed));
            assert!(user > 0 && agent > 0);
            assert_ne!(user, agent);
        }
        assert_ne!(
            transport_uids(uuid::Uuid::from_u128(42)),
            transport_uids(uuid::Uuid::from_u128(44))
        );
    }

    #[test]
    fn realtime_talk_requires_transport_voice_and_strict_lite() {
        let mut config = crate::config::DynamicConfig::default();
        config.ai_vendor_sources = vec![
            crate::config::AiVendorSource {
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
            },
            crate::config::AiVendorSource {
                slug: "minimax".to_string(),
                kind: "minimax".to_string(),
                display_name: "MiniMax".to_string(),
                enabled: true,
                preset: "minimax".to_string(),
                api_key: Some("minimax-key".to_string()),
                ..crate::config::AiVendorSource::default()
            },
        ];
        config.lite_enabled = true;
        config.lite_ai_provider = "openai".to_string();
        config.lite_openai_model = "lite-model".to_string();
        config.lite_openai_api_key = Some("lite-key".to_string());
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
