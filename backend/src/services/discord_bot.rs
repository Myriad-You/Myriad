//! Discord DM worker: Gateway WebSocket, no Interactions Endpoint.
//!
//! Bot token stays on the outbound path. Errors never echo the token.
//! Identify is budgeted (1000/24h); reconnects Resume first.

use std::sync::OnceLock;
use std::time::Duration;

use chrono::Utc;
use futures::{SinkExt, StreamExt};
use myriad_agent_rules::channel::{
    classify_discord_rest, classify_gateway_close, discord_private_component_from_create,
    discord_private_text_from_create, discord_retry_after, discord_session_starts_remaining,
    discord_worker_intent, parse_discord_bot_identity, parse_discord_gateway_url,
    ConnectFailureKind, DiscordBotIdentity, WorkerIntent, DISCORD_DIRECT_MESSAGES,
    DISCORD_TEXT_LIMIT,
};
use myriad_error::redact_secrets;
use serde::Serialize;
use serde_json::Value;
use tokio::sync::{watch, RwLock};
use tokio_tungstenite::tungstenite::Message;
use tracing::{info, warn};

use crate::config::DynamicConfig;
use crate::services::http_client;
use crate::GLOBAL_DYNAMIC_CONFIG;

const POLL: Duration = Duration::from_secs(2);
const HTTP_TIMEOUT: Duration = Duration::from_secs(15);
const API_BASE: &str = "https://discord.com/api/v10";
const USER_AGENT: &str = "DiscordBot (https://github.com/myriad, 1.0)";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscordBotPhase {
    Offline,
    Connecting,
    Online,
    Rejected,
    Reconnecting,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiscordBotStatus {
    pub phase: DiscordBotPhase,
    pub enabled: bool,
    pub has_token: bool,
    pub bot_username: Option<String>,
    pub bot_name: Option<String>,
    pub bot_user_id: Option<String>,
    pub last_inbound_at: Option<String>,
}

#[derive(Clone)]
struct ResumeState {
    session_id: String,
    resume_url: String,
    seq: u64,
}

static SNAPSHOT: OnceLock<RwLock<DiscordBotStatus>> = OnceLock::new();

fn snapshot() -> &'static RwLock<DiscordBotStatus> {
    SNAPSHOT.get_or_init(|| {
        RwLock::new(DiscordBotStatus {
            phase: DiscordBotPhase::Offline,
            enabled: false,
            has_token: false,
            bot_username: None,
            bot_name: None,
            bot_user_id: None,
            last_inbound_at: None,
        })
    })
}

async fn publish_status(phase: DiscordBotPhase, fingerprint: &CredentialFingerprint) {
    let mut snap = snapshot().write().await;
    snap.phase = phase;
    snap.enabled = fingerprint.enabled;
    snap.has_token = fingerprint.has_token;
    if !fingerprint.enabled || !fingerprint.has_token {
        snap.bot_username = None;
        snap.bot_name = None;
        snap.bot_user_id = None;
        snap.last_inbound_at = None;
    }
}

async fn publish_phase(phase: DiscordBotPhase) {
    snapshot().write().await.phase = phase;
}

async fn publish_identity(identity: &DiscordBotIdentity) {
    let mut snap = snapshot().write().await;
    snap.bot_username = if identity.username.is_empty() {
        None
    } else {
        Some(identity.username.clone())
    };
    snap.bot_name = identity
        .global_name
        .clone()
        .or_else(|| Some(identity.username.clone()));
    snap.bot_user_id = Some(identity.id.clone());
}

async fn mark_inbound() {
    snapshot().write().await.last_inbound_at = Some(Utc::now().to_rfc3339());
}

pub async fn current_status() -> DiscordBotStatus {
    snapshot().read().await.clone()
}

pub async fn test_saved_credentials() -> Result<DiscordBotIdentity, ConnectFailureKind> {
    let token = {
        let config = GLOBAL_DYNAMIC_CONFIG.read().await;
        CredentialFingerprint::from_config(&config)
            .token
            .ok_or(ConnectFailureKind::Permanent)?
    };
    let identity = get_me(&token).await?;
    publish_identity(&identity).await;
    Ok(identity)
}

#[derive(Clone, PartialEq, Eq)]
struct CredentialFingerprint {
    enabled: bool,
    has_token: bool,
    token: Option<String>,
}

impl CredentialFingerprint {
    fn from_config(config: &DynamicConfig) -> Self {
        let token = config
            .discord_bot_token
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        Self {
            enabled: config.discord_bot_enabled,
            has_token: token.is_some(),
            token,
        }
    }

    fn intent(&self) -> WorkerIntent {
        discord_worker_intent(self.enabled, self.token.as_deref().unwrap_or(""))
    }
}

pub fn spawn_worker() {
    tokio::spawn(async move {
        info!("Discord bot worker started");
        run_loop().await;
    });
}

async fn run_loop() {
    let mut last_permanent: Option<CredentialFingerprint> = None;
    let mut resume: Option<ResumeState> = None;
    loop {
        let fingerprint = {
            let config = GLOBAL_DYNAMIC_CONFIG.read().await;
            CredentialFingerprint::from_config(&config)
        };

        if fingerprint.intent() != WorkerIntent::Run {
            last_permanent = None;
            resume = None;
            publish_status(DiscordBotPhase::Offline, &fingerprint).await;
            tokio::time::sleep(POLL).await;
            continue;
        }

        if last_permanent.as_ref() == Some(&fingerprint) {
            publish_status(DiscordBotPhase::Rejected, &fingerprint).await;
            tokio::time::sleep(POLL).await;
            continue;
        }

        let (cancel_tx, cancel_rx) = watch::channel(false);
        let watched = fingerprint.clone();
        let watch_task = tokio::spawn(async move {
            loop {
                tokio::time::sleep(POLL).await;
                let current = {
                    let config = GLOBAL_DYNAMIC_CONFIG.read().await;
                    CredentialFingerprint::from_config(&config)
                };
                if current != watched {
                    let _ = cancel_tx.send(true);
                    break;
                }
            }
        });

        publish_status(DiscordBotPhase::Connecting, &fingerprint).await;
        let result = run_session(&fingerprint, resume.clone(), cancel_rx).await;
        watch_task.abort();
        match result {
            Ok(next) => {
                last_permanent = None;
                resume = next;
                publish_status(DiscordBotPhase::Offline, &fingerprint).await;
            }
            Err((ConnectFailureKind::Permanent, _)) => {
                warn!("Discord bot stopped: credentials rejected");
                last_permanent = Some(fingerprint.clone());
                resume = None;
                publish_status(DiscordBotPhase::Rejected, &fingerprint).await;
            }
            Err((kind, next)) => {
                warn!(?kind, "Discord bot transient failure; will reconnect");
                resume = next;
                publish_status(DiscordBotPhase::Reconnecting, &fingerprint).await;
                tokio::time::sleep(transient_backoff(1)).await;
            }
        }
    }
}

async fn run_session(
    fingerprint: &CredentialFingerprint,
    resume: Option<ResumeState>,
    cancel: watch::Receiver<bool>,
) -> Result<Option<ResumeState>, (ConnectFailureKind, Option<ResumeState>)> {
    let token = fingerprint
        .token
        .as_deref()
        .ok_or((ConnectFailureKind::Permanent, None))?;
    let identity = get_me(token).await.map_err(|kind| (kind, resume.clone()))?;
    publish_identity(&identity).await;
    gateway_session(token, &identity.id, resume, cancel)
        .await
        .map_err(|(kind, next)| (kind, next))
}

async fn gateway_session(
    token: &str,
    bot_user_id: &str,
    resume: Option<ResumeState>,
    mut cancel: watch::Receiver<bool>,
) -> Result<Option<ResumeState>, (ConnectFailureKind, Option<ResumeState>)> {
    let (gw_url, remaining) = fetch_gateway(token)
        .await
        .map_err(|kind| (kind, resume.clone()))?;
    let will_identify = resume.is_none();
    if will_identify && remaining == Some(0) {
        warn!("Discord Identify budget exhausted; waiting instead of resetting token");
        return Err((ConnectFailureKind::Transient, resume));
    }

    let connect_url = resume
        .as_ref()
        .map(|state| with_gateway_query(&state.resume_url))
        .unwrap_or_else(|| with_gateway_query(&gw_url));

    let (ws, _) = tokio::select! {
        _ = cancel.changed() => return Ok(resume),
        result = tokio_tungstenite::connect_async(&connect_url) => {
            result.map_err(|err| {
                log_transport("Discord Gateway connect failed", &err, token);
                (ConnectFailureKind::Transient, resume.clone())
            })?
        }
    };
    let (mut write, mut read) = ws.split();

    let Some(hello_ms) = wait_hello(&mut read, &mut cancel, token)
        .await
        .map_err(|kind| (kind, resume.clone()))?
    else {
        let _ = write.close().await;
        return Ok(resume);
    };

    let jitter = (Utc::now().timestamp_subsec_nanos() as f64) / 1_000_000_000.0;
    let first_wait = Duration::from_millis(((hello_ms as f64) * jitter) as u64);
    let interval = Duration::from_millis(hello_ms.max(1000));

    let handshake = if let Some(state) = resume.as_ref() {
        serde_json::json!({
            "op": 6,
            "d": {
                "token": token,
                "session_id": state.session_id,
                "seq": state.seq,
            }
        })
    } else {
        serde_json::json!({
            "op": 2,
            "d": {
                "token": token,
                "intents": DISCORD_DIRECT_MESSAGES,
                "properties": {
                    "os": std::env::consts::OS,
                    "browser": "myriad",
                    "device": "myriad",
                }
            }
        })
    };
    write
        .send(Message::Text(handshake.to_string().into()))
        .await
        .map_err(|err| {
            log_transport("Discord handshake send failed", &err, token);
            (ConnectFailureKind::Transient, resume.clone())
        })?;

    let mut state = resume.clone();
    let mut seq = resume.as_ref().map(|row| row.seq).unwrap_or(0);
    let mut hb_acked = true;
    let mut first_beat = true;
    let mut hb_timer = tokio::time::interval(if first_wait.is_zero() {
        interval
    } else {
        first_wait
    });
    hb_timer.tick().await;

    loop {
        tokio::select! {
            _ = cancel.changed() => {
                let _ = write.close().await;
                return Ok(state);
            }
            _ = hb_timer.tick() => {
                if first_beat {
                    first_beat = false;
                    hb_timer = tokio::time::interval(interval);
                    hb_timer.tick().await;
                }
                if !hb_acked {
                    let _ = write.close().await;
                    return Err((ConnectFailureKind::Transient, state));
                }
                hb_acked = false;
                let hb = serde_json::json!({ "op": 1, "d": seq });
                if write.send(Message::Text(hb.to_string().into())).await.is_err() {
                    return Err((ConnectFailureKind::Transient, state));
                }
            }
            msg = read.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Some(kind) = handle_payload(
                            &text,
                            token,
                            bot_user_id,
                            &mut seq,
                            &mut state,
                            &mut hb_acked,
                            &mut write,
                        )
                        .await
                        {
                            return Err((kind, state));
                        }
                    }
                    Some(Ok(Message::Close(frame))) => {
                        let code = frame.map(|f| u16::from(f.code)).unwrap_or(1000);
                        let kind = classify_gateway_close(code);
                        let keep = matches!(kind, ConnectFailureKind::Transient)
                            && !matches!(code, 1000 | 1001);
                        return Err((kind, if keep { state } else { None }));
                    }
                    Some(Ok(_)) => {}
                    Some(Err(err)) => {
                        log_transport("Discord Gateway stream error", &err, token);
                        return Err((ConnectFailureKind::Transient, state));
                    }
                    None => return Err((ConnectFailureKind::Transient, state)),
                }
            }
        }
    }
}

async fn handle_payload<S>(
    text: &str,
    token: &str,
    bot_user_id: &str,
    seq: &mut u64,
    state: &mut Option<ResumeState>,
    hb_acked: &mut bool,
    write: &mut S,
) -> Option<ConnectFailureKind>
where
    S: SinkExt<Message> + Unpin,
{
    let payload: Value = serde_json::from_str(text).ok()?;
    if let Some(s) = payload.get("s").and_then(Value::as_u64) {
        *seq = s;
        if let Some(stored) = state.as_mut() {
            stored.seq = s;
        }
    }
    let op = payload
        .get("op")
        .and_then(Value::as_u64)
        .unwrap_or(u64::MAX);
    match op {
        0 => {
            let event = payload.get("t").and_then(Value::as_str).unwrap_or("");
            let data = payload.get("d");
            if event == "READY" {
                if let Some(data) = data {
                    if let Some(session_id) = data
                        .get("session_id")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|id| !id.is_empty())
                    {
                        let resume_url = data
                            .get("resume_gateway_url")
                            .and_then(Value::as_str)
                            .unwrap_or("wss://gateway.discord.gg")
                            .to_string();
                        *state = Some(ResumeState {
                            session_id: session_id.to_string(),
                            resume_url,
                            seq: *seq,
                        });
                    }
                    if let Some(user) = data.get("user") {
                        if let Some(identity) = parse_discord_bot_identity(user) {
                            let identity = identity;
                            tokio::spawn(async move {
                                publish_identity(&identity).await;
                                publish_phase(DiscordBotPhase::Online).await;
                            });
                        }
                    } else {
                        tokio::spawn(async { publish_phase(DiscordBotPhase::Online).await });
                    }
                }
            }
            if event == "RESUMED" {
                tokio::spawn(async { publish_phase(DiscordBotPhase::Online).await });
            }
            if event == "MESSAGE_CREATE" {
                if let Some(data) = data {
                    if let Some(inbound) = discord_private_text_from_create(data, bot_user_id) {
                        let token = token.to_string();
                        tokio::spawn(async move {
                            mark_inbound().await;
                            crate::services::discord_pairing::handle_inbound(inbound, &token).await;
                        });
                    }
                }
            }
            if event == "INTERACTION_CREATE" {
                if let Some(data) = data {
                    if let Some(inbound) = discord_private_component_from_create(data) {
                        let token = token.to_string();
                        tokio::spawn(async move {
                            mark_inbound().await;
                            crate::services::discord_pairing::handle_component(inbound, &token)
                                .await;
                        });
                    }
                }
            }
            None
        }
        1 => {
            let hb = serde_json::json!({ "op": 1, "d": *seq });
            let _ = write.send(Message::Text(hb.to_string().into())).await;
            None
        }
        7 => Some(ConnectFailureKind::Transient),
        9 => {
            let resumable = payload.get("d").and_then(Value::as_bool).unwrap_or(false);
            if !resumable {
                *state = None;
            }
            Some(ConnectFailureKind::Transient)
        }
        11 => {
            *hb_acked = true;
            None
        }
        _ => None,
    }
}

async fn wait_hello<S>(
    read: &mut S,
    cancel: &mut watch::Receiver<bool>,
    token: &str,
) -> Result<Option<u64>, ConnectFailureKind>
where
    S: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    let deadline = tokio::time::sleep(Duration::from_secs(15));
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            _ = cancel.changed() => return Ok(None),
            _ = &mut deadline => return Err(ConnectFailureKind::Transient),
            msg = read.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        let payload: Value = match serde_json::from_str(&text) {
                            Ok(value) => value,
                            Err(_) => continue,
                        };
                        if payload.get("op").and_then(Value::as_u64) == Some(10) {
                            if let Some(ms) = payload
                                .get("d")
                                .and_then(|d| d.get("heartbeat_interval"))
                                .and_then(Value::as_u64)
                            {
                                return Ok(Some(ms));
                            }
                        }
                    }
                    Some(Ok(Message::Close(frame))) => {
                        let code = frame.map(|f| u16::from(f.code)).unwrap_or(1000);
                        return Err(classify_gateway_close(code));
                    }
                    Some(Err(err)) => {
                        log_transport("Discord Hello failed", &err, token);
                        return Err(ConnectFailureKind::Transient);
                    }
                    None => return Err(ConnectFailureKind::Transient),
                    Some(Ok(_)) => {}
                }
            }
        }
    }
}

async fn fetch_gateway(token: &str) -> Result<(String, Option<u64>), ConnectFailureKind> {
    let (status, body) = discord_request(token, reqwest::Method::GET, "/gateway/bot", None).await?;
    let url = parse_discord_gateway_url(status, &body)?;
    Ok((url, discord_session_starts_remaining(&body)))
}

fn with_gateway_query(url: &str) -> String {
    if url.contains('?') {
        format!("{url}&v=10&encoding=json")
    } else {
        format!("{url}?v=10&encoding=json")
    }
}

async fn get_me(token: &str) -> Result<DiscordBotIdentity, ConnectFailureKind> {
    let (status, body) = discord_request(token, reqwest::Method::GET, "/users/@me", None).await?;
    if !(200..300).contains(&status) {
        return Err(classify_discord_rest(status, &body));
    }
    let data: Value = serde_json::from_str(&body).map_err(|_| ConnectFailureKind::Transient)?;
    parse_discord_bot_identity(&data).ok_or(ConnectFailureKind::Transient)
}

pub async fn send_message(
    token: &str,
    channel_id: &str,
    text: &str,
) -> Result<(), ConnectFailureKind> {
    send_outbound(token, channel_id, text, None).await
}

pub async fn send_outbound(
    token: &str,
    channel_id: &str,
    text: &str,
    components: Option<Value>,
) -> Result<(), ConnectFailureKind> {
    if channel_id.is_empty() || text.is_empty() {
        return Ok(());
    }
    if !bot_enabled().await {
        return Ok(());
    }
    let content: String = text.chars().take(DISCORD_TEXT_LIMIT).collect();
    let mut payload = serde_json::json!({
        "content": content,
        "allowed_mentions": { "parse": [] },
    });
    if let Some(components) = components {
        payload["components"] = components;
    }
    let path = format!("/channels/{channel_id}/messages");
    let (status, body) =
        discord_request(token, reqwest::Method::POST, &path, Some(payload)).await?;
    if status == 429 {
        let wait = discord_retry_after(&body).unwrap_or(1);
        warn!(retry_after = wait, "Discord sendMessage rate-limited");
        return Err(ConnectFailureKind::Transient);
    }
    if !(200..300).contains(&status) {
        return Err(classify_discord_rest(status, &body));
    }
    Ok(())
}

/// Multipart `files[0]` plus `payload_json`. Content stays empty so Work text is not duplicated.
pub async fn send_photo(
    token: &str,
    channel_id: &str,
    bytes: &[u8],
    mime: &str,
    components: Option<Value>,
) -> Result<(), ConnectFailureKind> {
    if channel_id.is_empty() || bytes.is_empty() || !bot_enabled().await {
        return Ok(());
    }
    let filename = match mime {
        "image/jpeg" | "image/jpg" => "photo.jpg",
        "image/gif" => "photo.gif",
        "image/webp" => "photo.webp",
        _ => "photo.png",
    };
    let mut payload = serde_json::json!({
        "allowed_mentions": { "parse": [] },
    });
    if let Some(components) = components {
        payload["components"] = components;
    }
    let form = reqwest::multipart::Form::new()
        .text("payload_json", payload.to_string())
        .part(
            "files[0]",
            reqwest::multipart::Part::bytes(bytes.to_vec())
                .file_name(filename)
                .mime_str(if mime.starts_with("image/") {
                    mime
                } else {
                    "image/png"
                })
                .unwrap_or_else(|_| reqwest::multipart::Part::bytes(bytes.to_vec())),
        );
    let path = format!("/channels/{channel_id}/messages");
    let (status, body) = discord_multipart(token, &path, form).await?;
    if status == 429 {
        let wait = discord_retry_after(&body).unwrap_or(1);
        warn!(retry_after = wait, "Discord sendPhoto rate-limited");
        return Err(ConnectFailureKind::Transient);
    }
    if !(200..300).contains(&status) {
        return Err(classify_discord_rest(status, &body));
    }
    Ok(())
}

pub async fn send_typing(token: &str, channel_id: &str) -> Result<(), ConnectFailureKind> {
    if channel_id.is_empty() || !bot_enabled().await {
        return Ok(());
    }
    let path = format!("/channels/{channel_id}/typing");
    let (status, body) = discord_request(token, reqwest::Method::POST, &path, None).await?;
    if status == 429 {
        return Err(ConnectFailureKind::Transient);
    }
    if status == 204 || (200..300).contains(&status) {
        return Ok(());
    }
    Err(classify_discord_rest(status, &body))
}

/// ACK a button press within 3 seconds (`DEFERRED_UPDATE_MESSAGE`).
pub async fn ack_component(
    token: &str,
    interaction_id: &str,
    interaction_token: &str,
) -> Result<(), ConnectFailureKind> {
    if interaction_id.is_empty() || interaction_token.is_empty() || !bot_enabled().await {
        return Ok(());
    }
    let path = format!("/interactions/{interaction_id}/{interaction_token}/callback");
    let payload = serde_json::json!({ "type": 6 });
    let (status, body) =
        discord_request(token, reqwest::Method::POST, &path, Some(payload)).await?;
    if status == 204 || (200..300).contains(&status) {
        return Ok(());
    }
    Err(classify_discord_rest(status, &body))
}

async fn bot_enabled() -> bool {
    GLOBAL_DYNAMIC_CONFIG.read().await.discord_bot_enabled
}

async fn discord_multipart(
    token: &str,
    path: &str,
    form: reqwest::multipart::Form,
) -> Result<(u16, String), ConnectFailureKind> {
    let client = http_client::get_global_client().await;
    let url = format!("{API_BASE}{path}");
    let resp = client
        .post(&url)
        .timeout(HTTP_TIMEOUT)
        .header("Authorization", format!("Bot {token}"))
        .header("User-Agent", USER_AGENT)
        .multipart(form)
        .send()
        .await
        .map_err(|err| {
            log_transport("Discord HTTP request failed", &err, token);
            ConnectFailureKind::Transient
        })?;
    let status = resp.status().as_u16();
    let text = resp.text().await.map_err(|err| {
        log_transport("Discord HTTP body failed", &err, token);
        ConnectFailureKind::Transient
    })?;
    if status == 401 {
        warn!(path, "Discord API rejected credentials");
        return Err(ConnectFailureKind::Permanent);
    }
    Ok((status, text))
}

async fn discord_request(
    token: &str,
    method: reqwest::Method,
    path: &str,
    body: Option<Value>,
) -> Result<(u16, String), ConnectFailureKind> {
    let client = http_client::get_global_client().await;
    let url = format!("{API_BASE}{path}");
    let mut request = client
        .request(method, &url)
        .timeout(HTTP_TIMEOUT)
        .header("Authorization", format!("Bot {token}"))
        .header("User-Agent", USER_AGENT);
    if let Some(body) = body {
        request = request.json(&body);
    }
    let resp = request.send().await.map_err(|err| {
        log_transport("Discord HTTP request failed", &err, token);
        ConnectFailureKind::Transient
    })?;
    let status = resp.status().as_u16();
    let text = resp.text().await.map_err(|err| {
        log_transport("Discord HTTP body failed", &err, token);
        ConnectFailureKind::Transient
    })?;
    if status == 401 {
        warn!(path, "Discord API rejected credentials");
        return Err(ConnectFailureKind::Permanent);
    }
    Ok((status, text))
}

fn transient_backoff(attempt: u32) -> Duration {
    let secs = if attempt >= 6 {
        30
    } else {
        1u64 << attempt.min(5)
    };
    Duration::from_secs(secs.min(30))
}

fn redact_token(input: &str, token: &str) -> String {
    let replaced = if token.is_empty() {
        input.to_string()
    } else {
        input.replace(token, "[DISCORD_BOT_TOKEN_REDACTED]")
    };
    redact_secrets(&replaced)
}

fn log_transport(context: &str, err: &impl std::fmt::Display, token: &str) {
    warn!(error = %redact_token(&err.to_string(), token), "{context}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use myriad_agent_rules::channel::{
        classify_discord_rest, discord_private_text_from_create, DISCORD_DIRECT_MESSAGES,
    };

    #[test]
    fn fingerprint_intent_matches_rules() {
        let ready = CredentialFingerprint {
            enabled: true,
            has_token: true,
            token: Some("MTk4.test".into()),
        };
        assert_eq!(ready.intent(), WorkerIntent::Run);
        let off = CredentialFingerprint {
            enabled: false,
            has_token: true,
            token: Some("MTk4.test".into()),
        };
        assert_eq!(off.intent(), WorkerIntent::Stop);
    }

    #[test]
    fn error_display_never_keeps_the_bot_token() {
        let token = "MTk4.super-secret-discord-token";
        let raw = format!("Authorization: Bot {token}");
        let redacted = redact_token(&raw, token);
        assert!(!redacted.contains("super-secret-discord-token"));
        assert!(!redacted.contains(token));
    }

    #[test]
    fn identify_intent_is_direct_messages_only() {
        assert_eq!(DISCORD_DIRECT_MESSAGES, 1 << 12);
    }

    #[test]
    fn blocked_dm_does_not_look_like_a_dead_token() {
        assert_eq!(
            classify_discord_rest(403, r#"{"code":50007,"message":"Cannot send messages"}"#),
            ConnectFailureKind::Transient
        );
    }

    #[test]
    fn dm_create_without_guild_is_kept() {
        let data = serde_json::json!({
            "id": "11",
            "channel_id": "22",
            "author": { "id": "33", "bot": false },
            "content": "AB1D-EFGH"
        });
        let inbound = discord_private_text_from_create(&data, "99").expect("dm");
        assert_eq!(inbound.author_id, "33");
        assert_eq!(inbound.channel_id, "22");
    }

    #[test]
    fn guild_message_is_dropped() {
        let data = serde_json::json!({
            "id": "11",
            "channel_id": "22",
            "guild_id": "44",
            "author": { "id": "33" },
            "content": "hi"
        });
        assert!(discord_private_text_from_create(&data, "99").is_none());
    }
}
