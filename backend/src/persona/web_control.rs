//! Authenticated first-party calls to state owned by the web process.
//! No model-supplied URL, category, permission grant or executable is accepted.
use crate::services::agent::{self, executor::handlers::HandlerContext};
use axum::{
    body::Bytes,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use hmac::{Hmac, KeyInit, Mac};
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::Sha256;
use std::{
    collections::{HashMap, HashSet},
    sync::{LazyLock, Mutex},
    time::Duration,
};

pub const PATH: &str = "/internal/persona/web-capability";
const HEADER: &str = "x-myriad-persona-auth";
pub const MAX_REQUEST: usize = 256 * 1024;
const MAX_RESPONSE: usize = 4 * 1024 * 1024;
const MAX_NONCES: usize = 1024;
const WINDOW: i64 = 30;
const CONTEXT: &[u8] =
    b"myriad.persona.web-capability.v1\0POST\0/internal/persona/web-capability\0";
static NONCES: LazyLock<Mutex<HashMap<String, i64>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static BUDGET: LazyLock<std::sync::Arc<tokio::sync::Semaphore>> =
    LazyLock::new(|| std::sync::Arc::new(tokio::sync::Semaphore::new(8)));

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Call {
    capability: String,
    user_id: i32,
    autonomy_permission_cap: Option<Vec<String>>,
    params: HashMap<String, Value>,
}

pub(crate) fn is_web_owned(capability: &str) -> bool {
    matches!(
        capability,
        "scheduler.create"
            | "scheduler.trigger"
            | "scheduler.list"
            | "phantasi.schedule"
            | "task.submit"
            | "platform.refresh"
            | "system.metrics"
    )
}

fn signature(secret: &str, time: &str, nonce: &str, body: &[u8]) -> Option<Vec<u8>> {
    if secret.len() < 32 {
        return None;
    }
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).ok()?;
    mac.update(CONTEXT);
    mac.update(time.as_bytes());
    mac.update(b"\0");
    mac.update(nonce.as_bytes());
    mac.update(b"\0");
    mac.update(body);
    Some(mac.finalize().into_bytes().to_vec())
}

fn verify(secret: &str, header: &str, body: &[u8], now: i64) -> Option<String> {
    use subtle::ConstantTimeEq;
    if body.len() > MAX_REQUEST || header.len() > 180 {
        return None;
    }
    let mut fields = header.split(':');
    if fields.next()? != "v1" {
        return None;
    }
    let time = fields.next()?;
    let nonce = fields.next()?;
    let signed = fields.next()?;
    if fields.next().is_some() || nonce.len() != 32 || !nonce.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return None;
    }
    let issued: i64 = time.parse().ok()?;
    if issued.abs_diff(now) > WINDOW as u64 {
        return None;
    }
    let expected = signature(secret, time, nonce, body)?;
    let supplied = hex::decode(signed).ok()?;
    if supplied.len() != expected.len() || !bool::from(expected.ct_eq(&supplied)) {
        return None;
    }
    Some(nonce.into())
}

fn consume_nonce(cache: &mut HashMap<String, i64>, nonce: String, now: i64) -> bool {
    // Retain through the entire acceptance window, including allowed future skew.
    cache.retain(|_, seen| now.saturating_sub(*seen) <= WINDOW * 2);
    if cache.contains_key(&nonce) || cache.len() >= MAX_NONCES {
        return false;
    }
    cache.insert(nonce, now);
    true
}

pub async fn admission(request: axum::extract::Request, next: axum::middleware::Next) -> Response {
    let Ok(_permit) = BUDGET.clone().try_acquire_owned() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    match tokio::time::timeout(Duration::from_secs(120), next.run(request)).await {
        Ok(response) => response,
        Err(_) => StatusCode::GATEWAY_TIMEOUT.into_response(),
    }
}

pub async fn handle(
    crate::extract::Db(db): crate::extract::Db,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(secret) = crate::middleware::auth::session_secret() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let now = chrono::Utc::now().timestamp();
    let Some(nonce) = headers
        .get(HEADER)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| verify(&secret, h, &body, now))
    else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let consumed = NONCES
        .lock()
        .map(|mut cache| consume_nonce(&mut cache, nonce, now))
        .unwrap_or(false);
    if !consumed {
        return StatusCode::CONFLICT.into_response();
    }
    let Ok(call) = serde_json::from_slice::<Call>(&body) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if !is_web_owned(&call.capability) || call.user_id < 0 {
        return StatusCode::FORBIDDEN.into_response();
    }
    // The body authenticates a trusted process, never an everlasting user grant.
    // Use the current account and permission state again at the execution owner.
    let result = execute(&db, call).await;
    let Ok(encoded) = serde_json::to_vec(&result) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    if encoded.len() > MAX_RESPONSE {
        return StatusCode::BAD_GATEWAY.into_response();
    }
    (
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        encoded,
    )
        .into_response()
}

async fn execute(db: &sea_orm::DatabaseConnection, call: Call) -> Result<Value, String> {
    if call.user_id > 0 {
        let exists = db
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT id FROM users WHERE id = $1",
                [call.user_id.into()],
            ))
            .await
            .map_err(|_| "Could not verify current account")?;
        if exists.is_none() {
            return Err("Account no longer exists".into());
        }
    }
    agent::ensure_agent_usage_allowed(db, call.user_id).await?;
    let capability = agent::capability::get_capability_by_id(&call.capability)
        .await
        .ok_or("Unknown web capability")?;
    // Same authority as the worker's steps: re-reads the autonomy grant, so a
    // revocation between the worker's check and this call still stops it.
    let granted: HashSet<String> = agent::consciousness::authorize_capability(
        db,
        call.user_id,
        call.autonomy_permission_cap.as_deref(),
        &call.capability,
        &capability.required_permissions,
    )
    .await
    .map_err(|_| "Current permission denied".to_string())?
    .into_iter()
    .collect();
    if call.capability == "scheduler.create" {
        agent::scheduler_create_actions_within_grants(&call.params, &granted)?;
    }
    let context = HandlerContext {
        db,
        ai_analyzer: None,
        user_id: call.user_id,
        task_id: None,
        step_id: None,
        execution_context: None,
        autonomy_permission_cap: call.autonomy_permission_cap,
    };
    // Deliberately bypass the remote-dispatch wrapper: this is the state owner.
    agent::executor::handlers::execute_local_capability(
        &call.capability,
        "",
        &capability.category,
        &call.params,
        &context,
    )
    .await
}

pub async fn call(
    capability: &str,
    params: &HashMap<String, Value>,
    context: &HandlerContext<'_>,
) -> Result<Value, String> {
    if !is_web_owned(capability) {
        return Err("Capability is not owned by web".into());
    }
    let body = serde_json::to_vec(&Call {
        capability: capability.into(),
        user_id: context.user_id,
        autonomy_permission_cap: context.autonomy_permission_cap.clone(),
        params: params.clone(),
    })
    .map_err(|_| "Could not encode web capability")?;
    if body.len() > MAX_REQUEST {
        return Err("Web capability request is too large".into());
    }
    let secret = crate::middleware::auth::session_secret()
        .ok_or("Web capability authentication unavailable")?;
    let time = chrono::Utc::now().timestamp().to_string();
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let signed = signature(&secret, &time, &nonce, &body)
        .ok_or("Web capability authentication unavailable")?;
    let base =
        std::env::var("PERSONA_WEB_UPSTREAM").unwrap_or_else(|_| "http://backend:1103".into());
    if crate::config::AppConfig::is_production_environment() && base != "http://backend:1103" {
        return Err("Production persona web upstream must be http://backend:1103".into());
    }
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|_| "Could not create web capability client")?;
    // No retry: a disconnect after an accepted mutation has an unknown outcome.
    let mut response = client
        .post(format!("{base}{PATH}"))
        .header(HEADER, format!("v1:{time}:{nonce}:{}", hex::encode(signed)))
        .header("content-type", "application/json")
        .body(body)
        .send()
        .await
        .map_err(|_| outcome_unknown("web capability request failed"))?;
    if response.status().is_server_error() || response.status().as_u16() == 409 {
        return Err(outcome_unknown("web capability did not return a result"));
    }
    if !response.status().is_success() {
        return Err(format!(
            "Web capability returned HTTP {}",
            response.status().as_u16()
        ));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| outcome_unknown("web capability response interrupted"))?
    {
        if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE {
            return Err(outcome_unknown("web capability response is too large"));
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice::<Result<Value, String>>(&bytes)
        .map_err(|_| outcome_unknown("invalid web capability response"))?
}

fn outcome_unknown(detail: &str) -> String {
    format!("{} {detail}", myriad_agent_rules::OUTCOME_UNKNOWN_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;
    const SECRET: &str = "test-secret-at-least-thirty-two-bytes";
    #[test]
    fn authentication_binds_complete_body_and_expires() {
        let nonce = "0123456789abcdef0123456789abcdef";
        let body = br#"{"user_id":7}"#;
        let header = format!(
            "v1:100:{nonce}:{}",
            hex::encode(signature(SECRET, "100", nonce, body).unwrap())
        );
        assert_eq!(verify(SECRET, &header, body, 100), Some(nonce.into()));
        assert!(verify(SECRET, &header, br#"{"user_id":0}"#, 100).is_none());
        assert!(verify(SECRET, &header, body, 131).is_none());
        assert!(verify(SECRET, &header, body, 69).is_none());
        assert!(
            verify(
                "different-secret-at-least-thirty-two-bytes",
                &header,
                body,
                100
            )
            .is_none()
        );
    }
    #[test]
    fn replay_cache_does_not_evict_accepted_calls_under_pressure() {
        let mut cache = HashMap::new();
        assert!(consume_nonce(&mut cache, "first".into(), 100));
        assert!(!consume_nonce(&mut cache, "first".into(), 101));
        for i in 1..MAX_NONCES {
            assert!(consume_nonce(&mut cache, i.to_string(), 101));
        }
        assert!(!consume_nonce(&mut cache, "overflow".into(), 101));
        assert!(!consume_nonce(&mut cache, "first".into(), 160));
        assert!(consume_nonce(&mut cache, "new".into(), 162));
    }
    #[test]
    fn remote_surface_excludes_model_and_mcp_execution() {
        for capability in [
            "mcp.any.tool",
            "heartbeat.create",
            "ai.generate",
            "config.set",
            "seo.apply",
            "seo.generate",
            "",
            "scheduler.delete",
        ] {
            assert!(!is_web_owned(capability));
        }
        for capability in [
            "scheduler.create",
            "scheduler.trigger",
            "scheduler.list",
            "phantasi.schedule",
            "task.submit",
            "platform.refresh",
            "system.metrics",
        ] {
            assert!(is_web_owned(capability));
        }
    }
}
