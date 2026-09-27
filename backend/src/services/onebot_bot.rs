//! OneBot forward-WebSocket worker.
//!
//! NapCat is the server. This worker connects out, installs the live socket as
//! the outbound sender, and hands private messages to pairing. Reconnect,
//! credential rejection, and phase publishing stay in `bot_supervisor`.

use std::sync::OnceLock;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use myriad_agent_rules::channel::{ConnectFailureKind, WorkerIntent};
use myriad_agent_rules::onebot::rules::{classify_onebot_handshake, onebot_worker_intent};
use myriad_agent_rules::onebot::wire::{Inbound, RespJson};
use serde::Serialize;
use tokio::sync::{RwLock, watch};
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest, http::HeaderValue};
use tracing::{info, warn};

use crate::GLOBAL_DYNAMIC_CONFIG;
use crate::config::DynamicConfig;
use crate::services::bot_ingress;
use crate::services::bot_supervisor::{BotWorker, SessionResult, SupervisorPhase, supervise};
use crate::services::onebot_send;

const READ_IDLE: Duration = Duration::from_secs(90);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OneBotPhase {
    Offline,
    Connecting,
    Online,
    Rejected,
    Reconnecting,
}

#[derive(Debug, Clone, Serialize)]
pub struct OneBotStatus {
    pub phase: OneBotPhase,
    pub enabled: bool,
    pub has_url: bool,
    pub has_token: bool,
    pub ws_url: Option<String>,
    pub last_inbound_at: Option<String>,
}

static SNAPSHOT: OnceLock<RwLock<OneBotStatus>> = OnceLock::new();

fn snapshot() -> &'static RwLock<OneBotStatus> {
    SNAPSHOT.get_or_init(|| {
        RwLock::new(OneBotStatus {
            phase: OneBotPhase::Offline,
            enabled: false,
            has_url: false,
            has_token: false,
            ws_url: None,
            last_inbound_at: None,
        })
    })
}

pub async fn current_status() -> OneBotStatus {
    snapshot().read().await.clone()
}

async fn publish_status(phase: OneBotPhase, fingerprint: &CredentialFingerprint) {
    let mut snap = snapshot().write().await;
    snap.phase = phase;
    snap.enabled = fingerprint.enabled;
    snap.has_url = !fingerprint.ws_url.is_empty();
    snap.has_token = fingerprint.has_token;
    snap.ws_url = if fingerprint.ws_url.is_empty() {
        None
    } else {
        Some(fingerprint.ws_url.clone())
    };
    if !fingerprint.enabled || fingerprint.ws_url.is_empty() || !fingerprint.has_token {
        snap.last_inbound_at = None;
    }
}

async fn mark_inbound() {
    let mut snap = snapshot().write().await;
    snap.last_inbound_at = Some(chrono::Utc::now().to_rfc3339());
}

#[derive(Clone, PartialEq, Eq)]
struct CredentialFingerprint {
    enabled: bool,
    ws_url: String,
    has_token: bool,
    token: Option<String>,
}

impl CredentialFingerprint {
    fn from_config(config: &DynamicConfig) -> Self {
        let ws_url = config.onebot_bot_ws_url.trim().to_string();
        let token = config
            .onebot_bot_access_token
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        Self {
            enabled: config.onebot_bot_enabled,
            has_token: token.is_some(),
            ws_url,
            token,
        }
    }

    fn intent(&self) -> WorkerIntent {
        onebot_worker_intent(self.enabled, &self.ws_url, self.has_token)
    }
}

/// Owned by the persona supervisor; dropping this future stops the socket.
pub(crate) async fn run_worker() {
    supervise::<OneBotWorker>().await;
}

struct OneBotWorker;

impl BotWorker for OneBotWorker {
    const NAME: &'static str = "OneBot";
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
            SupervisorPhase::Offline => OneBotPhase::Offline,
            SupervisorPhase::Rejected => OneBotPhase::Rejected,
            SupervisorPhase::Connecting => OneBotPhase::Connecting,
            SupervisorPhase::Reconnecting => OneBotPhase::Reconnecting,
        };
        publish_status(phase, fingerprint).await;
    }

    async fn run_session(
        fingerprint: &CredentialFingerprint,
        _resume: Option<()>,
        cancel: watch::Receiver<bool>,
    ) -> SessionResult<()> {
        run_socket(fingerprint, cancel)
            .await
            .map(|()| None)
            .map_err(|kind| (kind, None))
    }
}

async fn run_socket(
    fingerprint: &CredentialFingerprint,
    mut cancel: watch::Receiver<bool>,
) -> Result<(), ConnectFailureKind> {
    let token = fingerprint
        .token
        .as_deref()
        .filter(|value| !value.is_empty())
        .ok_or(ConnectFailureKind::Permanent)?;
    if fingerprint.ws_url.is_empty() {
        return Err(ConnectFailureKind::Permanent);
    }
    let mut request = fingerprint
        .ws_url
        .as_str()
        .into_client_request()
        .map_err(|_| ConnectFailureKind::Permanent)?;
    if let Ok(value) = HeaderValue::from_str(&format!("Bearer {token}")) {
        request.headers_mut().insert("Authorization", value);
    } else {
        return Err(ConnectFailureKind::Permanent);
    }

    let (socket, _response) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(|_| ConnectFailureKind::Transient)?;
    let (write, mut read) = socket.split();
    let writer = std::sync::Arc::new(tokio::sync::Mutex::new(write));
    let outbound = writer.clone();
    onebot_send::install(std::sync::Arc::new(move |text| {
        let outbound = outbound.clone();
        tokio::spawn(async move {
            let _ = outbound.lock().await.send(Message::Text(text.into())).await;
        });
        Ok(())
    }))
    .await;
    publish_status(OneBotPhase::Online, fingerprint).await;
    info!(url = %fingerprint.ws_url, "OneBot socket connected");

    let outcome = loop {
        tokio::select! {
            _ = cancel.changed() => {
                if *cancel.borrow() {
                    break Ok(());
                }
            }
            frame = tokio::time::timeout(READ_IDLE, read.next()) => {
                match frame {
                    Err(_) => break Err(ConnectFailureKind::Transient),
                    Ok(None) => break Err(ConnectFailureKind::Transient),
                    Ok(Some(Err(_))) => break Err(ConnectFailureKind::Transient),
                    Ok(Some(Ok(Message::Close(_)))) => break Err(ConnectFailureKind::Transient),
                    Ok(Some(Ok(Message::Ping(payload)))) => {
                        let _ = writer.lock().await.send(Message::Pong(payload)).await;
                    }
                    Ok(Some(Ok(Message::Text(text)))) => {
                        if let Some(kind) = handle_text(text.as_str()).await {
                            break Err(kind);
                        }
                    }
                    Ok(Some(Ok(_))) => {}
                }
            }
        }
    };
    onebot_send::clear().await;
    outcome
}

/// `Some` means the socket must close with that failure kind.
async fn handle_text(text: &str) -> Option<ConnectFailureKind> {
    let inbound: Inbound = serde_json::from_str(text).ok()?;
    match inbound {
        Inbound::Resp(envelope) => {
            let response: RespJson = serde_json::from_str(text).ok()?;
            // A matched echo only fails that one action. Closing the socket is
            // reserved for a refusal that names no request.
            onebot_send::complete_echo(&envelope.echo, response.retcode).await;
            None
        }
        Inbound::Event(event) => {
            if let Some(retcode) = event
                .extra
                .get("retcode")
                .and_then(serde_json::Value::as_i64)
            {
                if classify_onebot_handshake(None, Some(retcode)) == ConnectFailureKind::Permanent {
                    warn!(retcode, "OneBot handshake refused");
                    return Some(ConnectFailureKind::Permanent);
                }
            }
            let raw = serde_json::to_string(event.as_ref()).ok()?;
            let Some(decoded) = myriad_agent_rules::onebot::decode::decode_private_inbound(&raw)
            else {
                return None;
            };
            let permit = bot_ingress::try_acquire(bot_ingress::Channel::OneBot, raw.len())?;
            mark_inbound().await;
            tokio::spawn(async move {
                let _permit = permit;
                crate::services::onebot_pairing::handle_inbound(decoded).await;
            });
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_intent_matches_rules() {
        let ready = CredentialFingerprint {
            enabled: true,
            ws_url: "ws://127.0.0.1:3001".into(),
            has_token: true,
            token: Some("secret".into()),
        };
        assert_eq!(ready.intent(), WorkerIntent::Run);
        let off = CredentialFingerprint {
            enabled: false,
            ..ready
        };
        assert_eq!(off.intent(), WorkerIntent::Stop);
    }
}
