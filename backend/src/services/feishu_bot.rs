//! Feishu p2p long-connection worker.
//!
//! Tenant token + `/callback/ws/endpoint` then WSS. AppSecret stays on the
//! outbound path. Errors never echo the secret.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use chrono::Utc;
use myriad_agent_rules::channel::{
    ConnectFailureKind, WorkerIntent, feishu_worker_intent, parse_feishu_tenant_token,
    parse_feishu_ws_endpoint,
};
use myriad_error::redact_secrets;
use serde::Serialize;
use tokio::sync::{RwLock, watch};
use tracing::warn;

use crate::GLOBAL_DYNAMIC_CONFIG;
use crate::config::DynamicConfig;
use crate::services::bot_supervisor::{BotWorker, SessionResult, SupervisorPhase, supervise};
use crate::services::http_client;

const HTTP_TIMEOUT: Duration = Duration::from_secs(15);
const API_BASE: &str = "https://open.feishu.cn";
const TOKEN_MARGIN: Duration = Duration::from_secs(60);

struct CachedToken {
    app_id: String,
    header: String,
    expires_at: Instant,
}

static TOKEN_CACHE: OnceLock<RwLock<Option<CachedToken>>> = OnceLock::new();

fn token_cache() -> &'static RwLock<Option<CachedToken>> {
    TOKEN_CACHE.get_or_init(|| RwLock::new(None))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FeishuBotPhase {
    Offline,
    Connecting,
    Online,
    Rejected,
    Reconnecting,
}

#[derive(Debug, Clone, Serialize)]
pub struct FeishuBotStatus {
    pub phase: FeishuBotPhase,
    pub enabled: bool,
    pub has_app_id: bool,
    pub has_secret: bool,
    pub app_id: Option<String>,
    pub last_inbound_at: Option<String>,
}

static SNAPSHOT: OnceLock<RwLock<FeishuBotStatus>> = OnceLock::new();

fn snapshot() -> &'static RwLock<FeishuBotStatus> {
    SNAPSHOT.get_or_init(|| {
        RwLock::new(FeishuBotStatus {
            phase: FeishuBotPhase::Offline,
            enabled: false,
            has_app_id: false,
            has_secret: false,
            app_id: None,
            last_inbound_at: None,
        })
    })
}

async fn publish_status(phase: FeishuBotPhase, fingerprint: &CredentialFingerprint) {
    let mut snap = snapshot().write().await;
    snap.phase = phase;
    snap.enabled = fingerprint.enabled;
    snap.has_app_id = !fingerprint.app_id.is_empty();
    snap.has_secret = fingerprint.has_secret;
    snap.app_id = if fingerprint.app_id.is_empty() {
        None
    } else {
        Some(fingerprint.app_id.clone())
    };
    if !fingerprint.enabled || fingerprint.app_id.is_empty() || !fingerprint.has_secret {
        snap.last_inbound_at = None;
    }
}

pub async fn publish_phase_online() {
    snapshot().write().await.phase = FeishuBotPhase::Online;
}

pub async fn mark_inbound() {
    snapshot().write().await.last_inbound_at = Some(Utc::now().to_rfc3339());
}

pub async fn current_status() -> FeishuBotStatus {
    snapshot().read().await.clone()
}

pub async fn test_saved_credentials() -> Result<(), ConnectFailureKind> {
    let fingerprint = {
        let config = GLOBAL_DYNAMIC_CONFIG.read().await;
        CredentialFingerprint::from_config(&config)
    };
    let secret = fingerprint
        .secret
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or(ConnectFailureKind::Permanent)?;
    if fingerprint.app_id.is_empty() {
        return Err(ConnectFailureKind::Permanent);
    }
    let _token = fetch_access_token(&fingerprint.app_id, secret).await?;
    Ok(())
}

#[derive(Clone, PartialEq, Eq)]
struct CredentialFingerprint {
    enabled: bool,
    app_id: String,
    has_secret: bool,
    secret: Option<String>,
}

impl CredentialFingerprint {
    fn from_config(config: &DynamicConfig) -> Self {
        let app_id = config.feishu_bot_app_id.trim().to_string();
        let secret = config
            .feishu_bot_app_secret
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        Self {
            enabled: config.feishu_bot_enabled,
            has_secret: secret.is_some(),
            app_id,
            secret,
        }
    }

    fn intent(&self) -> WorkerIntent {
        feishu_worker_intent(self.enabled, &self.app_id, self.has_secret)
    }
}

struct AccessToken {
    header: String,
    expires_at: Instant,
}

/// Fresh `Bearer` header for OpenAPI send/download. Refreshes before expiry.
pub async fn cached_auth_header() -> Result<String, ConnectFailureKind> {
    cached_auth_header_inner(false).await
}

/// Force a new tenant token. Used after OpenAPI reports token expiry.
pub async fn refresh_auth_header() -> Result<String, ConnectFailureKind> {
    cached_auth_header_inner(true).await
}

async fn cached_auth_header_inner(force: bool) -> Result<String, ConnectFailureKind> {
    let fingerprint = {
        let config = GLOBAL_DYNAMIC_CONFIG.read().await;
        CredentialFingerprint::from_config(&config)
    };
    let secret = fingerprint
        .secret
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or(ConnectFailureKind::Permanent)?;
    if fingerprint.app_id.is_empty() {
        return Err(ConnectFailureKind::Permanent);
    }
    if !force {
        let cache = token_cache().read().await;
        if let Some(cached) = cache.as_ref() {
            if cached.app_id == fingerprint.app_id
                && Instant::now() + TOKEN_MARGIN < cached.expires_at
            {
                return Ok(cached.header.clone());
            }
        }
    }
    let token = fetch_access_token(&fingerprint.app_id, secret).await?;
    {
        let mut cache = token_cache().write().await;
        *cache = Some(CachedToken {
            app_id: fingerprint.app_id.clone(),
            header: token.header.clone(),
            expires_at: token.expires_at,
        });
    }
    Ok(token.header)
}

/// Owned by the persona supervisor; dropping this future stops channel admission.
pub(crate) async fn run_worker() {
    supervise::<FeishuWorker>().await;
}

struct FeishuWorker;

impl BotWorker for FeishuWorker {
    const NAME: &'static str = "Feishu bot";
    type Fingerprint = CredentialFingerprint;
    type Resume = ();

    fn fingerprint(config: &DynamicConfig) -> CredentialFingerprint {
        CredentialFingerprint::from_config(config)
    }

    fn intent(fingerprint: &CredentialFingerprint) -> WorkerIntent {
        fingerprint.intent()
    }

    async fn publish(phase: SupervisorPhase, fingerprint: &CredentialFingerprint) {
        let phase = match phase {
            SupervisorPhase::Offline => FeishuBotPhase::Offline,
            SupervisorPhase::Rejected => FeishuBotPhase::Rejected,
            SupervisorPhase::Connecting => FeishuBotPhase::Connecting,
            SupervisorPhase::Reconnecting => FeishuBotPhase::Reconnecting,
        };
        publish_status(phase, fingerprint).await;
    }

    async fn run_session(
        fingerprint: &CredentialFingerprint,
        _resume: Option<()>,
        cancel: watch::Receiver<bool>,
    ) -> SessionResult<()> {
        run_gateway(fingerprint, cancel)
            .await
            .map(|()| None)
            .map_err(|kind| (kind, None))
    }
}

async fn run_gateway(
    fingerprint: &CredentialFingerprint,
    cancel: watch::Receiver<bool>,
) -> Result<(), ConnectFailureKind> {
    let secret = fingerprint
        .secret
        .as_deref()
        .ok_or(ConnectFailureKind::Permanent)?;
    let token = fetch_access_token(&fingerprint.app_id, secret).await?;
    {
        let mut cache = token_cache().write().await;
        *cache = Some(CachedToken {
            app_id: fingerprint.app_id.clone(),
            header: token.header.clone(),
            expires_at: token.expires_at,
        });
    }
    let (ws_url, service_id, ping_secs) = fetch_ws_endpoint(&fingerprint.app_id, secret).await?;
    crate::services::feishu_ws::run_session(
        &ws_url,
        service_id,
        Duration::from_secs(ping_secs.max(1)),
        cancel,
    )
    .await
}

pub async fn fetch_access_token(
    app_id: &str,
    app_secret: &str,
) -> Result<AccessToken, ConnectFailureKind> {
    let client = feishu_http_client().await?;
    let url = format!("{API_BASE}/open-apis/auth/v3/tenant_access_token/internal");
    let body = serde_json::json!({
        "app_id": app_id,
        "app_secret": app_secret,
    });
    let resp = client
        .post(&url)
        .timeout(HTTP_TIMEOUT)
        .json(&body)
        .send()
        .await
        .map_err(|err| {
            log_transport("Feishu tenant_access_token request failed", &err);
            ConnectFailureKind::Transient
        })?;
    let status = resp.status().as_u16();
    let text = resp.text().await.map_err(|err| {
        log_transport("Feishu tenant_access_token body failed", &err);
        ConnectFailureKind::Transient
    })?;
    let (token, expires_in) = parse_feishu_tenant_token(status, &text)?;
    Ok(AccessToken {
        header: format!("Bearer {token}"),
        expires_at: Instant::now() + Duration::from_secs(expires_in.max(1)),
    })
}

async fn fetch_ws_endpoint(
    app_id: &str,
    app_secret: &str,
) -> Result<(String, i32, u64), ConnectFailureKind> {
    let client = feishu_http_client().await?;
    let url = format!("{API_BASE}/callback/ws/endpoint");
    let body = serde_json::json!({
        "AppID": app_id,
        "AppSecret": app_secret,
    });
    let resp = client
        .post(&url)
        .timeout(HTTP_TIMEOUT)
        .header("locale", "zh")
        .json(&body)
        .send()
        .await
        .map_err(|err| {
            log_transport("Feishu ws endpoint request failed", &err);
            ConnectFailureKind::Transient
        })?;
    let status = resp.status().as_u16();
    let text = resp.text().await.map_err(|err| {
        log_transport("Feishu ws endpoint body failed", &err);
        ConnectFailureKind::Transient
    })?;
    parse_feishu_ws_endpoint(status, &text)
}

async fn feishu_http_client() -> Result<reqwest::Client, ConnectFailureKind> {
    let proxy = http_client::ProxyConfig::from_dynamic_config().await;
    let builder = reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .connect_timeout(Duration::from_secs(10))
        .user_agent("Myriad/1.0");
    let builder = if proxy.should_use_proxy() {
        http_client::apply_proxy(builder, &proxy).map_err(|err| {
            log_transport("Feishu HTTP client proxy build failed", &err);
            ConnectFailureKind::Transient
        })?
    } else {
        builder.no_proxy()
    };
    builder.build().map_err(|err| {
        log_transport("Feishu HTTP client build failed", &err);
        ConnectFailureKind::Transient
    })
}

fn log_transport(context: &str, err: &impl std::fmt::Display) {
    let redacted = redact_secrets(&err.to_string());
    warn!(error = %redacted, "{context}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_intent_matches_rules() {
        let ready = CredentialFingerprint {
            enabled: true,
            app_id: "cli_a".into(),
            has_secret: true,
            secret: Some("s".into()),
        };
        assert_eq!(ready.intent(), WorkerIntent::Run);

        let off = CredentialFingerprint {
            enabled: false,
            app_id: "cli_a".into(),
            has_secret: true,
            secret: Some("s".into()),
        };
        assert_eq!(off.intent(), WorkerIntent::Stop);
    }

    #[tokio::test]
    async fn status_snapshot_starts_offline_and_records_phase() {
        let fingerprint = CredentialFingerprint {
            enabled: true,
            app_id: "cli_a".into(),
            has_secret: true,
            secret: Some("s".into()),
        };
        publish_status(FeishuBotPhase::Connecting, &fingerprint).await;
        let status = current_status().await;
        assert_eq!(status.phase, FeishuBotPhase::Connecting);
        assert!(status.enabled);
        assert!(status.has_app_id);
        assert!(status.has_secret);
        publish_phase_online().await;
        assert_eq!(current_status().await.phase, FeishuBotPhase::Online);
    }
}
