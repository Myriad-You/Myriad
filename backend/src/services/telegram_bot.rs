//! Telegram DM worker: `getUpdates` long poll, no webhook.
//!
//! Bot token stays on the outbound path. Errors never echo the token.

use std::sync::OnceLock;
use std::time::Duration;

use chrono::Utc;
use myriad_agent_rules::channel::{
    parse_telegram_bot_identity, parse_telegram_ok_payload, parse_telegram_private_inbounds,
    telegram_max_update_id, telegram_retry_after, telegram_worker_intent, ConnectFailureKind,
    TelegramBotIdentity, TelegramPrivateInbound, WorkerIntent,
};
use myriad_error::redact_secrets;
use serde::Serialize;
use tokio::sync::{watch, RwLock};
use tracing::{info, warn};

use crate::config::DynamicConfig;
use crate::services::http_client;
use crate::GLOBAL_DYNAMIC_CONFIG;

const POLL: Duration = Duration::from_secs(2);
const LONG_POLL_SECS: u64 = 25;
const HTTP_TIMEOUT: Duration = Duration::from_secs(15);
const LONG_POLL_HTTP_TIMEOUT: Duration = Duration::from_secs(35);
const API_HOST: &str = "https://api.telegram.org";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TelegramBotPhase {
    Offline,
    Connecting,
    Online,
    Rejected,
    Reconnecting,
}

#[derive(Debug, Clone, Serialize)]
pub struct TelegramBotStatus {
    pub phase: TelegramBotPhase,
    pub enabled: bool,
    pub has_token: bool,
    pub bot_username: Option<String>,
    pub bot_name: Option<String>,
    pub last_inbound_at: Option<String>,
}

static SNAPSHOT: OnceLock<RwLock<TelegramBotStatus>> = OnceLock::new();

fn snapshot() -> &'static RwLock<TelegramBotStatus> {
    SNAPSHOT.get_or_init(|| {
        RwLock::new(TelegramBotStatus {
            phase: TelegramBotPhase::Offline,
            enabled: false,
            has_token: false,
            bot_username: None,
            bot_name: None,
            last_inbound_at: None,
        })
    })
}

async fn publish_status(phase: TelegramBotPhase, fingerprint: &CredentialFingerprint) {
    let mut snap = snapshot().write().await;
    snap.phase = phase;
    snap.enabled = fingerprint.enabled;
    snap.has_token = fingerprint.has_token;
    if !fingerprint.enabled || !fingerprint.has_token {
        snap.bot_username = None;
        snap.bot_name = None;
        snap.last_inbound_at = None;
    }
}

async fn publish_phase(phase: TelegramBotPhase) {
    snapshot().write().await.phase = phase;
}

async fn publish_identity(identity: &TelegramBotIdentity) {
    let mut snap = snapshot().write().await;
    snap.bot_username = identity.username.clone();
    snap.bot_name = Some(identity.first_name.clone());
}

async fn mark_inbound() {
    snapshot().write().await.last_inbound_at = Some(Utc::now().to_rfc3339());
}

pub async fn current_status() -> TelegramBotStatus {
    snapshot().read().await.clone()
}

/// Probe the saved bot token with `getMe`. Secrets never appear in the error.
pub async fn test_saved_credentials() -> Result<TelegramBotIdentity, ConnectFailureKind> {
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
            .telegram_bot_token
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        Self {
            enabled: config.telegram_bot_enabled,
            has_token: token.is_some(),
            token,
        }
    }

    fn intent(&self) -> WorkerIntent {
        telegram_worker_intent(self.enabled, self.token.as_deref().unwrap_or(""))
    }
}

pub fn spawn_worker() {
    tokio::spawn(async move {
        info!("Telegram bot worker started");
        run_loop().await;
    });
}

async fn run_loop() {
    let mut last_permanent: Option<CredentialFingerprint> = None;
    loop {
        let fingerprint = {
            let config = GLOBAL_DYNAMIC_CONFIG.read().await;
            CredentialFingerprint::from_config(&config)
        };

        if fingerprint.intent() != WorkerIntent::Run {
            last_permanent = None;
            publish_status(TelegramBotPhase::Offline, &fingerprint).await;
            tokio::time::sleep(POLL).await;
            continue;
        }

        if last_permanent.as_ref() == Some(&fingerprint) {
            publish_status(TelegramBotPhase::Rejected, &fingerprint).await;
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

        publish_status(TelegramBotPhase::Connecting, &fingerprint).await;
        let result = run_session(&fingerprint, cancel_rx).await;
        watch_task.abort();
        match result {
            Ok(()) => {
                last_permanent = None;
                publish_status(TelegramBotPhase::Offline, &fingerprint).await;
            }
            Err(ConnectFailureKind::Permanent) => {
                warn!("Telegram bot stopped: credentials rejected");
                last_permanent = Some(fingerprint.clone());
                publish_status(TelegramBotPhase::Rejected, &fingerprint).await;
            }
            Err(_) => {
                warn!("Telegram bot transient failure; will reconnect");
                publish_status(TelegramBotPhase::Reconnecting, &fingerprint).await;
                tokio::time::sleep(transient_backoff(1)).await;
            }
        }
    }
}

async fn run_session(
    fingerprint: &CredentialFingerprint,
    mut cancel: watch::Receiver<bool>,
) -> Result<(), ConnectFailureKind> {
    let token = fingerprint
        .token
        .as_deref()
        .ok_or(ConnectFailureKind::Permanent)?;
    let identity = get_me(token).await?;
    publish_identity(&identity).await;
    publish_phase(TelegramBotPhase::Online).await;

    let mut offset: Option<i64> = None;
    loop {
        tokio::select! {
            _ = cancel.changed() => return Ok(()),
            result = get_updates(token, offset) => {
                match result {
                    Ok(body) => {
                        let inbounds = parse_telegram_private_inbounds(200, &body)?;
                        if let Some(max_id) = telegram_max_update_id(200, &body)? {
                            offset = Some(max_id + 1);
                        }
                        if !inbounds.is_empty() {
                            mark_inbound().await;
                        }
                        for event in inbounds {
                            let token = token.to_string();
                            tokio::spawn(async move {
                                match event {
                                    TelegramPrivateInbound::Text(text) => {
                                        crate::services::telegram_pairing::handle_inbound(
                                            text, &token,
                                        )
                                        .await;
                                    }
                                    TelegramPrivateInbound::Callback(callback) => {
                                        crate::services::telegram_pairing::handle_callback(
                                            callback, &token,
                                        )
                                        .await;
                                    }
                                }
                            });
                        }
                    }
                    Err(GetUpdatesError::RetryAfter(secs)) => {
                        warn!(retry_after = secs, "Telegram getUpdates rate-limited");
                        tokio::select! {
                            _ = cancel.changed() => return Ok(()),
                            _ = tokio::time::sleep(Duration::from_secs(secs.max(1))) => {}
                        }
                    }
                    Err(GetUpdatesError::Failure(ConnectFailureKind::Permanent)) => {
                        return Err(ConnectFailureKind::Permanent);
                    }
                    Err(GetUpdatesError::Failure(kind)) => return Err(kind),
                }
            }
        }
    }
}

enum GetUpdatesError {
    RetryAfter(u64),
    Failure(ConnectFailureKind),
}

async fn get_me(token: &str) -> Result<TelegramBotIdentity, ConnectFailureKind> {
    let (status, body) = telegram_request(token, "getMe", None, HTTP_TIMEOUT).await?;
    let result = parse_telegram_ok_payload(status, &body)?;
    parse_telegram_bot_identity(&result).ok_or(ConnectFailureKind::Transient)
}

async fn get_updates(token: &str, offset: Option<i64>) -> Result<String, GetUpdatesError> {
    let mut payload = serde_json::json!({
        "timeout": LONG_POLL_SECS,
        "allowed_updates": ["message", "callback_query"],
    });
    if let Some(offset) = offset {
        payload["offset"] = serde_json::Value::from(offset);
    }
    let (status, body) =
        telegram_request(token, "getUpdates", Some(payload), LONG_POLL_HTTP_TIMEOUT)
            .await
            .map_err(GetUpdatesError::Failure)?;
    if status == 429 {
        return Err(GetUpdatesError::RetryAfter(
            telegram_retry_after(&body).unwrap_or(1),
        ));
    }
    if let Err(kind) = parse_telegram_ok_payload(status, &body) {
        if let Some(secs) = telegram_retry_after(&body) {
            return Err(GetUpdatesError::RetryAfter(secs.max(1)));
        }
        return Err(GetUpdatesError::Failure(kind));
    }
    Ok(body)
}

pub async fn send_message(
    token: &str,
    chat_id: &str,
    text: &str,
) -> Result<(), ConnectFailureKind> {
    send_outbound(token, chat_id, text, None).await
}

pub async fn send_outbound(
    token: &str,
    chat_id: &str,
    text: &str,
    reply_markup: Option<serde_json::Value>,
) -> Result<(), ConnectFailureKind> {
    if chat_id.is_empty() || text.is_empty() {
        return Ok(());
    }
    let enabled = {
        let config = GLOBAL_DYNAMIC_CONFIG.read().await;
        config.telegram_bot_enabled
    };
    if !enabled {
        return Ok(());
    }
    let mut payload = serde_json::json!({
        "chat_id": chat_id,
        "text": myriad_agent_rules::channel::truncate_telegram_text(text),
    });
    if let Some(markup) = reply_markup {
        payload["reply_markup"] = markup;
    }
    let (status, body) =
        telegram_request(token, "sendMessage", Some(payload), HTTP_TIMEOUT).await?;
    if status == 429 {
        let wait = telegram_retry_after(&body).unwrap_or(1);
        warn!(retry_after = wait, "Telegram sendMessage rate-limited");
        return Err(ConnectFailureKind::Transient);
    }
    parse_telegram_ok_payload(status, &body).map(|_| ())
}

/// `sendPhoto` multipart. Caption stays empty so the Work text is not duplicated.
pub async fn send_photo(
    token: &str,
    chat_id: &str,
    bytes: &[u8],
    mime: &str,
    reply_markup: Option<serde_json::Value>,
) -> Result<(), ConnectFailureKind> {
    if chat_id.is_empty() || bytes.is_empty() {
        return Ok(());
    }
    let enabled = {
        let config = GLOBAL_DYNAMIC_CONFIG.read().await;
        config.telegram_bot_enabled
    };
    if !enabled {
        return Ok(());
    }
    let filename = photo_filename(mime);
    let mut form = reqwest::multipart::Form::new()
        .text("chat_id", chat_id.to_string())
        .part(
            "photo",
            reqwest::multipart::Part::bytes(bytes.to_vec())
                .file_name(filename)
                .mime_str(if mime.starts_with("image/") {
                    mime
                } else {
                    "image/png"
                })
                .unwrap_or_else(|_| reqwest::multipart::Part::bytes(bytes.to_vec())),
        );
    if let Some(markup) = reply_markup {
        form = form.text("reply_markup", markup.to_string());
    }
    let (status, body) = telegram_multipart(token, "sendPhoto", form).await?;
    if status == 429 {
        let wait = telegram_retry_after(&body).unwrap_or(1);
        warn!(retry_after = wait, "Telegram sendPhoto rate-limited");
        return Err(ConnectFailureKind::Transient);
    }
    parse_telegram_ok_payload(status, &body).map(|_| ())
}

fn photo_filename(mime: &str) -> &'static str {
    match mime {
        "image/jpeg" | "image/jpg" => "photo.jpg",
        "image/gif" => "photo.gif",
        "image/webp" => "photo.webp",
        _ => "photo.png",
    }
}

/// Must be called after every callback press or the client spinner never stops.
pub async fn answer_callback_query(
    token: &str,
    callback_query_id: &str,
) -> Result<(), ConnectFailureKind> {
    if callback_query_id.is_empty() {
        return Ok(());
    }
    let enabled = {
        let config = GLOBAL_DYNAMIC_CONFIG.read().await;
        config.telegram_bot_enabled
    };
    if !enabled {
        return Ok(());
    }
    let payload = serde_json::json!({ "callback_query_id": callback_query_id });
    let (status, body) =
        telegram_request(token, "answerCallbackQuery", Some(payload), HTTP_TIMEOUT).await?;
    if status == 429 {
        let wait = telegram_retry_after(&body).unwrap_or(1);
        warn!(
            retry_after = wait,
            "Telegram answerCallbackQuery rate-limited"
        );
        return Err(ConnectFailureKind::Transient);
    }
    parse_telegram_ok_payload(status, &body).map(|_| ())
}

/// Resolve `file_id` through `getFile` and download the bytes. Token never logs.
pub async fn download_file_bytes(token: &str, file_id: &str) -> Result<(Vec<u8>, String), String> {
    if token.is_empty() || file_id.is_empty() {
        return Err("telegram file_id is empty".to_string());
    }
    let payload = serde_json::json!({ "file_id": file_id });
    let (status, body) = telegram_request(token, "getFile", Some(payload), HTTP_TIMEOUT)
        .await
        .map_err(|error| format!("{error:?}"))?;
    let path = myriad_agent_rules::channel::parse_telegram_file_path(status, &body)
        .map_err(|error| format!("{error:?}"))?;
    let url = format!("{API_HOST}/file/bot{token}/{path}");
    let client = http_client::get_global_client().await;
    let resp = client
        .get(&url)
        .timeout(HTTP_TIMEOUT)
        .send()
        .await
        .map_err(|err| {
            log_transport("Telegram getFile download failed", &err, token);
            "telegram file download failed".to_string()
        })?;
    let mime = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("image/jpeg")
        .to_string();
    let bytes = resp.bytes().await.map_err(|err| {
        log_transport("Telegram getFile body failed", &err, token);
        "telegram file download failed".to_string()
    })?;
    if bytes.is_empty() {
        return Err("telegram file is empty".to_string());
    }
    Ok((bytes.to_vec(), mime))
}

/// `sendChatAction` typing. Official window is about 5 seconds or until a
/// bot message arrives; callers refresh while Work is still running.
pub async fn send_typing(token: &str, chat_id: &str) -> Result<(), ConnectFailureKind> {
    if chat_id.is_empty() {
        return Ok(());
    }
    let enabled = {
        let config = GLOBAL_DYNAMIC_CONFIG.read().await;
        config.telegram_bot_enabled
    };
    if !enabled {
        return Ok(());
    }
    let payload = serde_json::json!({
        "chat_id": chat_id,
        "action": "typing",
    });
    let (status, body) =
        telegram_request(token, "sendChatAction", Some(payload), HTTP_TIMEOUT).await?;
    if status == 429 {
        let wait = telegram_retry_after(&body).unwrap_or(1);
        warn!(retry_after = wait, "Telegram sendChatAction rate-limited");
        return Err(ConnectFailureKind::Transient);
    }
    parse_telegram_ok_payload(status, &body).map(|_| ())
}

async fn telegram_multipart(
    token: &str,
    method: &str,
    form: reqwest::multipart::Form,
) -> Result<(u16, String), ConnectFailureKind> {
    let client = http_client::get_global_client().await;
    let url = format!("{API_HOST}/bot{token}/{method}");
    let resp = client
        .post(&url)
        .timeout(HTTP_TIMEOUT)
        .multipart(form)
        .send()
        .await
        .map_err(|err| {
            log_transport(&format!("Telegram {method} request failed"), &err, token);
            ConnectFailureKind::Transient
        })?;
    let status = resp.status().as_u16();
    let text = resp.text().await.map_err(|err| {
        log_transport(&format!("Telegram {method} body failed"), &err, token);
        ConnectFailureKind::Transient
    })?;
    if !(200..300).contains(&status) && status != 429 {
        let kind = parse_telegram_ok_payload(status, &text)
            .err()
            .unwrap_or(ConnectFailureKind::Transient);
        if kind == ConnectFailureKind::Permanent {
            warn!(status, method, "Telegram API rejected credentials");
        } else {
            warn!(
                status,
                method,
                body = %redact_token(&text, token),
                "Telegram API error"
            );
        }
        return Err(kind);
    }
    Ok((status, text))
}

async fn telegram_request(
    token: &str,
    method: &str,
    body: Option<serde_json::Value>,
    timeout: Duration,
) -> Result<(u16, String), ConnectFailureKind> {
    let client = http_client::get_global_client().await;
    let url = format!("{API_HOST}/bot{token}/{method}");
    let request = if let Some(body) = body {
        client.post(&url).json(&body)
    } else {
        client.get(&url)
    };
    let resp = request.timeout(timeout).send().await.map_err(|err| {
        log_transport(&format!("Telegram {method} request failed"), &err, token);
        ConnectFailureKind::Transient
    })?;
    let status = resp.status().as_u16();
    let text = resp.text().await.map_err(|err| {
        log_transport(&format!("Telegram {method} body failed"), &err, token);
        ConnectFailureKind::Transient
    })?;
    if !(200..300).contains(&status) && status != 429 {
        let kind = parse_telegram_ok_payload(status, &text)
            .err()
            .unwrap_or(ConnectFailureKind::Transient);
        if kind == ConnectFailureKind::Permanent {
            warn!(status, method, "Telegram API rejected credentials");
        } else {
            warn!(
                status,
                method,
                body = %redact_token(&text, token),
                "Telegram API error"
            );
        }
        return Err(kind);
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
        input.replace(token, "[TELEGRAM_BOT_TOKEN_REDACTED]")
    };
    redact_secrets(&replaced)
}

fn log_transport(context: &str, err: &impl std::fmt::Display, token: &str) {
    warn!(error = %redact_token(&err.to_string(), token), "{context}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_intent_matches_rules() {
        let ready = CredentialFingerprint {
            enabled: true,
            has_token: true,
            token: Some("123456:ABC".into()),
        };
        assert_eq!(ready.intent(), WorkerIntent::Run);

        let off = CredentialFingerprint {
            enabled: false,
            has_token: true,
            token: Some("123456:ABC".into()),
        };
        assert_eq!(off.intent(), WorkerIntent::Stop);
    }

    #[test]
    fn error_display_never_keeps_the_bot_token() {
        let token = "123456:super-secret-telegram-token";
        let raw = format!(
            "error sending request for url (https://api.telegram.org/bot{token}/getUpdates)"
        );
        let redacted = redact_token(&raw, token);
        assert!(!redacted.contains("super-secret-telegram-token"));
        assert!(!redacted.contains(token));
    }

    #[tokio::test]
    async fn status_snapshot_starts_offline_and_records_phase() {
        let fingerprint = CredentialFingerprint {
            enabled: true,
            has_token: true,
            token: Some("t".into()),
        };
        publish_status(TelegramBotPhase::Connecting, &fingerprint).await;
        let status = current_status().await;
        assert_eq!(status.phase, TelegramBotPhase::Connecting);
        assert!(status.enabled);
        assert!(status.has_token);
        publish_phase(TelegramBotPhase::Online).await;
        assert_eq!(current_status().await.phase, TelegramBotPhase::Online);
    }
}
