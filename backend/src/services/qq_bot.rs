//! QQ C2C Gateway worker.
//!
//! Unified-platform auth and Gateway loop follow EasyBot's QQ adapter:
//! `POST /app/getAppAccessToken`, `Authorization: QQBot <token>`,
//! `GET /gateway/bot`, Hello / Identify / Heartbeat.
//! Close-code permanence follows the official Node SDK table.
//! First cut reconnects with Identify; ingest into Work is a later ticket.

use std::time::{Duration, Instant};

use futures::{SinkExt, StreamExt};
use myriad_agent_rules::channel::{
    classify_gateway_close, parse_access_token_response, parse_gateway_url_response, worker_intent,
    ConnectFailureKind, InboundC2cText, WorkerIntent, GROUP_AND_C2C_EVENT,
};
use myriad_error::redact_secrets;
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::watch;
use tokio_tungstenite::tungstenite::Message;
use tracing::{info, warn};

use crate::config::DynamicConfig;
use crate::services::http_client;
use crate::GLOBAL_DYNAMIC_CONFIG;

const POLL: Duration = Duration::from_secs(2);
const AUTH_URL: &str = "https://bots.qq.com/app/getAppAccessToken";
const API_BASE: &str = "https://api.bot.qq.com";
const TOKEN_MARGIN: Duration = Duration::from_secs(60);
const TOKEN_REFRESH: Duration = Duration::from_secs(3500);
const HTTP_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone, PartialEq, Eq)]
struct CredentialFingerprint {
    enabled: bool,
    app_id: String,
    has_secret: bool,
    secret: Option<String>,
}

impl CredentialFingerprint {
    fn from_config(config: &DynamicConfig) -> Self {
        let app_id = config.qq_bot_app_id.trim().to_string();
        let secret = config
            .qq_bot_app_secret
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        Self {
            enabled: config.qq_bot_enabled,
            has_secret: secret.is_some(),
            app_id,
            secret,
        }
    }

    fn intent(&self) -> WorkerIntent {
        worker_intent(self.enabled, &self.app_id, self.has_secret)
    }
}

struct AccessToken {
    header: String,
    expires_at: Instant,
}

impl AccessToken {
    fn auth_header(&self) -> &str {
        &self.header
    }

    fn needs_refresh(&self) -> bool {
        Instant::now() + TOKEN_MARGIN >= self.expires_at
    }
}

pub fn spawn_worker() {
    tokio::spawn(async move {
        info!("QQ bot Gateway worker started");
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
            tokio::time::sleep(POLL).await;
            continue;
        }

        if last_permanent.as_ref() == Some(&fingerprint) {
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

        let result = run_gateway(&fingerprint, cancel_rx).await;
        watch_task.abort();
        match result {
            Ok(()) => {
                last_permanent = None;
            }
            Err(ConnectFailureKind::Permanent) => {
                warn!("QQ bot Gateway stopped: credentials rejected");
                last_permanent = Some(fingerprint);
            }
            Err(_) => {
                warn!("QQ bot Gateway transient failure; will reconnect");
                tokio::time::sleep(transient_backoff(1)).await;
            }
        }
    }
}

async fn run_gateway(
    fingerprint: &CredentialFingerprint,
    mut cancel: watch::Receiver<bool>,
) -> Result<(), ConnectFailureKind> {
    let secret = fingerprint
        .secret
        .as_deref()
        .ok_or(ConnectFailureKind::Permanent)?;
    let mut token = fetch_access_token(&fingerprint.app_id, secret).await?;
    let mut refreshed_once = false;
    let gw_url = loop {
        match fetch_gateway_url(token.auth_header()).await {
            Ok(url) => break url,
            Err(FetchGatewayError::TokenInvalid) if !refreshed_once => {
                token = fetch_access_token(&fingerprint.app_id, secret).await?;
                refreshed_once = true;
            }
            Err(FetchGatewayError::TokenInvalid) => return Err(ConnectFailureKind::Permanent),
            Err(FetchGatewayError::Failure(kind)) => return Err(kind),
        }
    };

    gateway_session(fingerprint, token, &gw_url, &mut cancel).await
}

async fn gateway_session(
    fingerprint: &CredentialFingerprint,
    mut token: AccessToken,
    gw_url: &str,
    cancel: &mut watch::Receiver<bool>,
) -> Result<(), ConnectFailureKind> {
    let (ws, _) = tokio::select! {
        _ = cancel.changed() => return Ok(()),
        result = tokio_tungstenite::connect_async(gw_url) => {
            result.map_err(|err| {
                log_transport("QQ Gateway connect failed", &err);
                ConnectFailureKind::Transient
            })?
        }
    };
    let (mut write, mut read) = ws.split();

    let Some(hello) = wait_hello(&mut read, cancel).await? else {
        let _ = write.close().await;
        return Ok(());
    };
    let hb_interval = Duration::from_millis((hello.heartbeat_interval as f64 * 0.75) as u64);
    let identify = serde_json::json!({
        "op": 2,
        "d": {
            "token": token.auth_header(),
            "intents": GROUP_AND_C2C_EVENT,
            "shard": [0, 1],
        }
    });
    write
        .send(Message::Text(identify.to_string().into()))
        .await
        .map_err(|err| {
            log_transport("QQ Identify send failed", &err);
            ConnectFailureKind::Transient
        })?;

    let mut seq: u64 = 0;
    let mut hb_timer = tokio::time::interval(hb_interval.max(Duration::from_millis(1)));
    hb_timer.tick().await;
    let mut token_timer = tokio::time::interval(TOKEN_REFRESH);
    token_timer.tick().await;

    loop {
        tokio::select! {
            _ = cancel.changed() => {
                let _ = write.close().await;
                return Ok(());
            }
            _ = hb_timer.tick() => {
                let hb = serde_json::json!({"op": 1, "d": seq});
                if write.send(Message::Text(hb.to_string().into())).await.is_err() {
                    return Err(ConnectFailureKind::Transient);
                }
            }
            _ = token_timer.tick() => {
                if token.needs_refresh() {
                    match fetch_access_token(&fingerprint.app_id, fingerprint.secret.as_deref().unwrap_or(""))
                        .await
                    {
                        Ok(next) => token = next,
                        Err(ConnectFailureKind::Permanent) => return Err(ConnectFailureKind::Permanent),
                        Err(_) => warn!("QQ token refresh failed; will retry"),
                    }
                }
            }
            msg = read.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Some(kind) = handle_payload(&text, &mut seq) {
                            return Err(kind);
                        }
                    }
                    Some(Ok(Message::Close(frame))) => {
                        let code = frame.map(|f| u16::from(f.code)).unwrap_or(1000);
                        return Err(classify_gateway_close(code));
                    }
                    Some(Ok(_)) => {}
                    Some(Err(err)) => {
                        log_transport("QQ Gateway stream error", &err);
                        return Err(ConnectFailureKind::Transient);
                    }
                    None => return Err(ConnectFailureKind::Transient),
                }
            }
        }
    }
}

#[derive(Deserialize)]
struct HelloData {
    heartbeat_interval: u64,
}

#[derive(Deserialize)]
struct GatewayPayload {
    op: u8,
    #[serde(default)]
    d: Option<Value>,
    #[serde(default)]
    s: Option<u64>,
    #[serde(default)]
    t: Option<String>,
}

async fn wait_hello<S>(
    read: &mut S,
    cancel: &mut watch::Receiver<bool>,
) -> Result<Option<HelloData>, ConnectFailureKind>
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
                        let payload: GatewayPayload = match serde_json::from_str(&text) {
                            Ok(p) => p,
                            Err(_) => continue,
                        };
                        if payload.op == 10 {
                            if let Some(d) = payload.d {
                                if let Ok(hello) = serde_json::from_value::<HelloData>(d) {
                                    return Ok(Some(hello));
                                }
                            }
                        }
                    }
                    Some(Ok(Message::Close(frame))) => {
                        let code = frame.map(|f| u16::from(f.code)).unwrap_or(1000);
                        return Err(classify_gateway_close(code));
                    }
                    Some(Err(_)) | None => return Err(ConnectFailureKind::Transient),
                    Some(Ok(_)) => {}
                }
            }
        }
    }
}

fn handle_payload(text: &str, seq: &mut u64) -> Option<ConnectFailureKind> {
    let payload: GatewayPayload = serde_json::from_str(text).ok()?;
    if let Some(s) = payload.s {
        *seq = s;
    }
    match payload.op {
        0 => {
            if payload.t.as_deref() == Some("C2C_MESSAGE_CREATE") {
                if let Some(event) = inbound_c2c_from_dispatch(payload.d.as_ref()) {
                    info!(msg_id = %event.msg_id, "QQ C2C text received");
                }
            }
            None
        }
        7 => Some(ConnectFailureKind::Transient),
        9 => Some(ConnectFailureKind::Transient),
        11 => None,
        _ => None,
    }
}

fn inbound_c2c_from_dispatch(data: Option<&Value>) -> Option<InboundC2cText> {
    let data = data?;
    let msg_id = data.get("id")?.as_str()?.trim();
    if msg_id.is_empty() {
        return None;
    }
    let user_openid = data
        .get("author")?
        .get("user_openid")?
        .as_str()?
        .trim();
    if user_openid.is_empty() {
        return None;
    }
    Some(InboundC2cText {
        msg_id: msg_id.to_string(),
        user_openid: user_openid.to_string(),
        content: data
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
    })
}

async fn fetch_access_token(
    app_id: &str,
    client_secret: &str,
) -> Result<AccessToken, ConnectFailureKind> {
    let client = http_client::get_global_client().await;
    let body = serde_json::json!({
        "appId": app_id,
        "clientSecret": client_secret,
    });
    let resp = client
        .post(AUTH_URL)
        .timeout(HTTP_TIMEOUT)
        .json(&body)
        .send()
        .await
        .map_err(|err| {
            log_transport("QQ getAppAccessToken request failed", &err);
            ConnectFailureKind::Transient
        })?;
    let status = resp.status().as_u16();
    let text = resp.text().await.map_err(|err| {
        log_transport("QQ getAppAccessToken body failed", &err);
        ConnectFailureKind::Transient
    })?;
    let (token, expires_in) = parse_access_token_response(status, &text)?;
    Ok(AccessToken {
        header: format!("QQBot {token}"),
        expires_at: Instant::now() + Duration::from_secs(expires_in.max(1)),
    })
}

enum FetchGatewayError {
    TokenInvalid,
    Failure(ConnectFailureKind),
}

async fn fetch_gateway_url(auth_header: &str) -> Result<String, FetchGatewayError> {
    let client = http_client::get_global_client().await;
    let url = format!("{API_BASE}/gateway/bot");
    let resp = client
        .get(&url)
        .timeout(HTTP_TIMEOUT)
        .header("Authorization", auth_header)
        .send()
        .await
        .map_err(|err| {
            log_transport("QQ /gateway/bot request failed", &err);
            FetchGatewayError::Failure(ConnectFailureKind::Transient)
        })?;
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    if myriad_agent_rules::channel::qq_token_needs_refresh(status, &text) {
        return Err(FetchGatewayError::TokenInvalid);
    }
    parse_gateway_url_response(status, &text).map_err(FetchGatewayError::Failure)
}

fn transient_backoff(attempt: u32) -> Duration {
    let secs = if attempt >= 6 { 30 } else { 1u64 << attempt.min(5) };
    Duration::from_secs(secs.min(30))
}

fn log_transport(context: &str, err: &impl std::fmt::Display) {
    let redacted = redact_secrets(&err.to_string());
    warn!(error = %redacted, "{context}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use myriad_agent_rules::channel::{classify_connect_failure, ConnectFailure};
    use serde_json::json;

    #[test]
    fn http_401_and_token_reject_are_permanent() {
        assert_eq!(
            classify_connect_failure(&ConnectFailure::HttpStatus {
                status: 401,
                body: "unauthorized",
            }),
            ConnectFailureKind::Permanent
        );
        assert_eq!(
            classify_connect_failure(&ConnectFailure::HttpStatus {
                status: 403,
                body: "forbidden",
            }),
            ConnectFailureKind::Permanent
        );
        assert_eq!(
            classify_connect_failure(&ConnectFailure::TokenRejected {
                body: "missing access_token",
            }),
            ConnectFailureKind::Permanent
        );
        assert_eq!(
            classify_gateway_close(4004),
            ConnectFailureKind::Permanent
        );
        assert_eq!(
            classify_gateway_close(4014),
            ConnectFailureKind::Permanent
        );
    }

    #[test]
    fn transport_and_5xx_are_transient() {
        assert_eq!(
            classify_connect_failure(&ConnectFailure::HttpStatus {
                status: 500,
                body: "oops",
            }),
            ConnectFailureKind::Transient
        );
        assert_eq!(classify_gateway_close(4009), ConnectFailureKind::Transient);
        assert_eq!(
            classify_connect_failure(&ConnectFailure::HttpStatus {
                status: 200,
                body: r#"{"code":100001}"#,
            }),
            ConnectFailureKind::Transient
        );
    }

    #[test]
    fn error_display_is_redacted_for_assignment_keys() {
        let raw = "qq_bot_app_secret=super-secret-qq-value clientSecret=also-secret-value";
        let redacted = redact_secrets(raw);
        assert!(!redacted.contains("super-secret-qq-value"));
        assert!(!redacted.contains("also-secret-value"));
    }

    #[test]
    fn fingerprint_intent_matches_rules() {
        let ready = CredentialFingerprint {
            enabled: true,
            app_id: "102123456".into(),
            has_secret: true,
            secret: Some("s".into()),
        };
        assert_eq!(ready.intent(), WorkerIntent::Run);

        let off = CredentialFingerprint {
            enabled: false,
            app_id: "102123456".into(),
            has_secret: true,
            secret: Some("s".into()),
        };
        assert_eq!(off.intent(), WorkerIntent::Stop);
    }

    #[test]
    fn c2c_dispatch_uses_user_openid() {
        let event = inbound_c2c_from_dispatch(Some(&json!({
            "id": "m1",
            "content": "帮我查天气",
            "author": { "user_openid": "openid-a" },
            "timestamp": "2026-09-06T12:00:00+08:00"
        })))
        .expect("c2c");
        assert_eq!(event.msg_id, "m1");
        assert_eq!(event.user_openid, "openid-a");
        assert_eq!(event.content, "帮我查天气");
        assert!(inbound_c2c_from_dispatch(Some(&json!({
            "id": "m2",
            "author": { "id": "guild-user" }
        })))
        .is_none());
    }

    #[test]
    fn identify_intents_are_c2c_only() {
        assert_eq!(GROUP_AND_C2C_EVENT, 1 << 25);
        assert_eq!(GROUP_AND_C2C_EVENT & (1 << 9), 0);
        assert_eq!(GROUP_AND_C2C_EVENT & (1 << 30), 0);
    }
}
