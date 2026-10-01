//! OneBot forward-WebSocket worker.
//!
//! NapCat is the server. This worker connects out, installs the live socket as
//! the outbound sender, and hands private messages to pairing. Reconnect,
//! credential rejection, and phase publishing stay in `bot_supervisor`.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use myriad_agent_rules::channel::{ConnectFailureKind, WorkerIntent};
use myriad_agent_rules::onebot::decode::{
    OneBotGroupLine, decode_group_ban, decode_group_inbound, decode_private_inbound,
    decode_replied_message, private_message_is_cq_string,
};
use myriad_agent_rules::onebot::encode::encode_get_msg;
use myriad_agent_rules::onebot::rules::{
    classify_onebot_handshake, onebot_frame_refuses_connection, onebot_group_allowed,
    onebot_worker_intent,
};
use myriad_agent_rules::onebot::wire::{Inbound, RespJson};
use serde::Serialize;
use tokio::sync::{RwLock, mpsc, watch};
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest, http::HeaderValue};
use tracing::{info, warn};

use crate::GLOBAL_DYNAMIC_CONFIG;
use crate::config::DynamicConfig;
use crate::services::bot_ingress;
use crate::services::bot_supervisor::{BotWorker, SessionResult, SupervisorPhase, supervise};
use crate::services::onebot_send;

const READ_IDLE: Duration = Duration::from_secs(90);

fn connect_failure(error: tokio_tungstenite::tungstenite::Error) -> ConnectFailureKind {
    match error {
        tokio_tungstenite::tungstenite::Error::Http(response) => {
            classify_onebot_handshake(Some(response.status().as_u16()), None)
        }
        _ => ConnectFailureKind::Transient,
    }
}

async fn groups_enabled() -> bool {
    GLOBAL_DYNAMIC_CONFIG.read().await.onebot_bot_groups_enabled
}

/// Groups outside the deployer's allowlist are never recorded and never
/// answered: she does not hear them at all.
async fn group_allowed(group_id: &str) -> bool {
    let config = GLOBAL_DYNAMIC_CONFIG.read().await;
    onebot_group_allowed(&config.onebot_bot_group_ids, group_id)
}

/// NapCat also accepts the token as `?access_token=` and reads it first, so a
/// deployer may put it in the URL. Logs and the status API show only the part
/// before the query.
fn redacted_url(url: &str) -> String {
    url.split(['?', '#']).next().unwrap_or_default().to_string()
}

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
        Some(redacted_url(&fingerprint.ws_url))
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
        .map_err(connect_failure)?;
    let (write, mut read) = socket.split();
    let (outbound_tx, mut outbound_rx) = mpsc::channel::<String>(32);
    onebot_send::install(std::sync::Arc::new(move |text| {
        outbound_tx
            .try_send(text)
            .map_err(|_| "onebot socket is not connected".to_string())
    }))
    .await;
    publish_status(OneBotPhase::Online, fingerprint).await;
    info!(url = %redacted_url(&fingerprint.ws_url), "OneBot socket connected");
    read_groups_back();

    let mut write = write;
    let outcome = loop {
        tokio::select! {
            _ = cancel.changed() => {
                if *cancel.borrow() {
                    break Ok(());
                }
            }
            outgoing = outbound_rx.recv() => {
                // The sender lives in the installed slot; losing it means this
                // socket is no longer the one delivery writes to.
                let Some(text) = outgoing else {
                    break Err(ConnectFailureKind::Transient);
                };
                if write.send(Message::Text(text.into())).await.is_err() {
                    break Err(ConnectFailureKind::Transient);
                }
            }
            frame = tokio::time::timeout(READ_IDLE, read.next()) => {
                match frame {
                    Err(_) => break Err(ConnectFailureKind::Transient),
                    Ok(None) => break Err(ConnectFailureKind::Transient),
                    Ok(Some(Err(_))) => break Err(ConnectFailureKind::Transient),
                    Ok(Some(Ok(Message::Close(_)))) => break Err(ConnectFailureKind::Transient),
                    Ok(Some(Ok(Message::Ping(payload)))) => {
                        if write.send(Message::Pong(payload)).await.is_err() {
                            break Err(ConnectFailureKind::Transient);
                        }
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
            // NapCat's token refusal arrives as a response with `echo: null`.
            // A matched echo only fails that one action.
            if onebot_frame_refuses_connection(&envelope.echo, &response.status, response.retcode) {
                warn!(retcode = response.retcode, "OneBot handshake refused");
                return Some(ConnectFailureKind::Permanent);
            }
            onebot_send::complete_echo(
                &envelope.echo,
                &response.status,
                response.retcode,
                response.data,
            )
            .await;
            None
        }
        Inbound::Event(event) => {
            // A heartbeat carries `status` as an object. Only a failed action
            // envelope with no echo field at all is a handshake refusal.
            if let Some(status) = event.status.as_ref().and_then(serde_json::Value::as_str)
                && let Some(retcode) = event
                    .extra
                    .get("retcode")
                    .and_then(serde_json::Value::as_i64)
                && onebot_frame_refuses_connection(&serde_json::Value::Null, status, retcode)
            {
                warn!(retcode, "OneBot handshake refused");
                return Some(ConnectFailureKind::Permanent);
            }
            let raw = serde_json::to_string(event.as_ref()).ok()?;
            if !groups_enabled().await {
                // Groups stay off until the deployer turns them on. A personal
                // QQ number is already in many groups; recording all of them
                // is not the private-chat default.
            } else if let Some(ban) = decode_group_ban(&raw, event.self_id) {
                if group_allowed(&ban.group_id).await {
                    crate::services::channel_group::muted(
                        &format!("onebot:{}", ban.group_id),
                        &ban.operator_id,
                        ban.muted,
                        ban.everyone,
                    )
                    .await;
                }
            } else if let Some(group) = decode_group_inbound(&raw, event.self_id) {
                if !group_allowed(&group.group_id).await {
                    return None;
                }
                mark_inbound().await;
                let self_id = event.self_id;
                tokio::spawn(async move {
                    // The lookup waits on this socket's read loop, so it runs here.
                    let group = resolve_reply(group, self_id).await;
                    let line = crate::services::channel_group::GroupLine::from(group);
                    crate::services::channel_group::record(&line).await;
                    if line.addressed {
                        let Some(permit) =
                            bot_ingress::try_acquire(bot_ingress::Channel::OneBot, line.text.len())
                        else {
                            return;
                        };
                        let _permit = permit;
                        crate::services::channel_group::handle(line, String::new()).await;
                    } else {
                        crate::services::channel_group::notice(line, String::new());
                    }
                });
                return None;
            }
            if private_message_is_cq_string(&raw) {
                static WARNED: AtomicBool = AtomicBool::new(false);
                if !WARNED.swap(true, Ordering::Relaxed) {
                    warn!(
                        "OneBot private message used CQ string format; set messagePostFormat to array"
                    );
                }
                return None;
            }
            let decoded = decode_private_inbound(&raw)?;
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

/// Start reading groups back; its calls wait on the socket's read loop, so
/// it runs on its own, once per connection.
fn read_groups_back() {
    tokio::spawn(catch_up_groups());
}

/// Lines of each group read back on connecting.
const CATCH_UP_LINES: u32 = 30;

/// On connecting, read back what each group said while she was away (a
/// restart, a re-login): the allowlisted groups, or with no allowlist, the
/// groups she was in lately. Only read, never answered.
async fn catch_up_groups() {
    if !groups_enabled().await {
        return;
    }
    let login = serde_json::json!({"action": "get_login_info", "params": {}});
    let Some(self_id) = onebot_send::call_action(login)
        .await
        .ok()
        .and_then(|info| info.get("user_id").and_then(serde_json::Value::as_i64))
    else {
        warn!("OneBot login info unavailable; not reading groups back");
        return;
    };
    let allowlist = {
        let config = GLOBAL_DYNAMIC_CONFIG.read().await;
        myriad_agent_rules::onebot::rules::normalize_onebot_group_allowlist(
            &config.onebot_bot_group_ids,
        )
        .unwrap_or_default()
    };
    let groups: Vec<String> = if allowlist.is_empty() {
        crate::services::channel_group::groups_lately(
            crate::services::channel_platform::ChannelPlatform::OneBot,
        )
        .await
    } else {
        allowlist.split(',').map(str::to_string).collect()
    };
    for group in groups {
        let Some(action) = myriad_agent_rules::onebot::encode::encode_get_group_msg_history(
            &group,
            CATCH_UP_LINES,
        ) else {
            continue;
        };
        let data = match onebot_send::call_action(action).await {
            Ok(data) => data,
            Err(error) => {
                warn!(%error, "OneBot group history unavailable");
                continue;
            }
        };
        let past: Vec<_> = myriad_agent_rules::onebot::decode::decode_group_history(&data, self_id)
            .into_iter()
            .filter_map(|past| {
                let at = chrono::DateTime::from_timestamp(past.at, 0)?;
                Some((
                    crate::services::channel_group::GroupLine::from(past.line),
                    at,
                    past.hers,
                ))
            })
            .collect();
        crate::services::channel_group::catch_up(&format!("onebot:{group}"), past).await;
    }
}

/// NapCat's reply segment names only a message id. Look it up so the quoted
/// line, and whether it was hers, reaches the group context. A failed lookup
/// leaves the line as it was.
async fn resolve_reply(mut group: OneBotGroupLine, self_id: i64) -> OneBotGroupLine {
    let Some(action) = group.reply_id.as_deref().and_then(encode_get_msg) else {
        return group;
    };
    match onebot_send::call_action(action).await {
        Ok(data) => {
            if let Some(quoted) = decode_replied_message(&data, self_id) {
                group.reply_to = Some(quoted);
            }
        }
        Err(error) => warn!(%error, "OneBot reply lookup failed"),
    }
    group
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

    #[test]
    fn token_in_the_url_query_never_reaches_logs_or_status() {
        assert_eq!(
            redacted_url("ws://127.0.0.1:3001/?access_token=secret"),
            "ws://127.0.0.1:3001/"
        );
        assert_eq!(redacted_url("ws://host:3001#frag"), "ws://host:3001");
        assert_eq!(redacted_url("ws://host:3001"), "ws://host:3001");
    }
}
