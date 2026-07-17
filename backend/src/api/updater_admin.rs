//! Admin-only proxy to the Myriad updater. All routes require `admin_middleware`.
//!
//! Production: backend calls `updater-gateway` with `UPDATER_GATEWAY_SECRET` (no
//! `UPDATE_TOKEN` in the fat process); the gateway injects the update token. The browser
//! never sees either secret. See docs/updater-spec.md §13 for the upstream contract.
//!
//! The `UpdaterClient` is stashed in a process-global `OnceLock` so handlers don't need to
//! thread axum `State` through — this matches the style of the rest of `backend/src/main.rs`,
//! which constructs the `Router` without a generic state parameter.

use std::sync::OnceLock;
use std::time::Duration;

use axum::{
    extract::{Path, Query},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json, Response},
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::services::updater_client::{UpdaterClient, UpdaterClientError};

fn authenticated_user_id(headers: &HeaderMap) -> Option<i32> {
    crate::middleware::auth::verify_jwt_token(headers)
        .ok()
        .and_then(|claims| claims.sub.parse::<i32>().ok())
}

/// Build `admin:<id>:<username>` for updater audit (`X-Update-Actor`).
/// Only derived from verified JWT on the backend; never trusted from the browser as a substitute for UPDATE_TOKEN.
fn actor_from_headers(headers: &HeaderMap) -> Option<String> {
    let claims = crate::middleware::auth::verify_jwt_token(headers).ok()?;
    let id = claims.sub.parse::<i32>().ok()?;
    let user = claims
        .username
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '@'))
        .take(64)
        .collect::<String>();
    if user.is_empty() {
        Some(format!("admin:{id}"))
    } else {
        Some(format!("admin:{id}:{user}"))
    }
}

/// Backend-side audit line (independent of updater audit.log).
fn log_admin_actor(action: &str, headers: &HeaderMap) {
    match actor_from_headers(headers) {
        Some(actor) => tracing::info!(%actor, %action, "admin updater action"),
        None => tracing::info!(%action, actor = "unknown", "admin updater action"),
    }
}

async fn track_updater_job(
    updater: UpdaterClient,
    response: &Value,
    headers: &HeaderMap,
    kind: &'static str,
) {
    let Some(job_id) = response.get("job_id").and_then(Value::as_str) else {
        return;
    };
    let Some(user_id) = authenticated_user_id(headers) else {
        return;
    };
    if let Some(manager) = crate::services::agent::notifications::get_notification_manager() {
        manager
            .notify_updater_job(user_id, job_id, kind, "pending", "任务已进入更新队列")
            .await;
    }

    spawn_updater_job_tracker(updater, user_id, job_id.to_string(), kind.to_string());
}

fn spawn_updater_job_tracker(updater: UpdaterClient, user_id: i32, job_id: String, kind: String) {
    tokio::spawn(async move {
        let mut last_status = "pending".to_string();
        for attempt in 0..450 {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
            let Ok(job) = updater.get_json(&format!("/jobs/{}", job_id)).await else {
                continue;
            };
            let status = job
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            if status != last_status {
                let detail = job
                    .get("steps")
                    .and_then(Value::as_array)
                    .and_then(|steps| steps.last())
                    .and_then(|step| step.get("error").and_then(Value::as_str))
                    .map(str::to_string)
                    .or_else(|| {
                        job.get("to_version")
                            .and_then(Value::as_str)
                            .map(|version| format!("目标版本: {}", version))
                    })
                    .unwrap_or_else(|| format!("任务状态: {}", status));
                if let Some(manager) =
                    crate::services::agent::notifications::get_notification_manager()
                {
                    manager
                        .notify_updater_job(user_id, &job_id, &kind, status, &detail)
                        .await;
                }
                last_status = status.to_string();
            }
            if matches!(status, "succeeded" | "failed" | "needs_manual") {
                return;
            }
        }

        if let Some(manager) = crate::services::agent::notifications::get_notification_manager() {
            manager
                .notify_updater_job(
                    user_id,
                    &job_id,
                    &kind,
                    "unknown",
                    "状态监控超时，请在系统更新面板确认任务结果",
                )
                .await;
        }
    });
}

/// backend 本身可能在更新中被替换。启动后从持久化通知恢复未完成 job 的轮询，
/// 避免通知永久停留在“已提交”。
pub async fn resume_pending_job_notifications() {
    let Some(updater) = client().cloned() else {
        return;
    };
    let Some(manager) = crate::services::agent::notifications::get_notification_manager() else {
        return;
    };
    let pending = manager.pending_updater_jobs().await;
    if !pending.is_empty() {
        tracing::info!(
            count = pending.len(),
            "restoring updater notification trackers"
        );
    }
    for (user_id, job_id, kind) in pending {
        spawn_updater_job_tracker(updater.clone(), user_id, job_id, kind);
    }
}

/// Holds the optional client. `None` means "no updater configured" — we still want the routes
/// to exist (so admins get a predictable 503 instead of a 404) but mutating calls will refuse.
static UPDATER: OnceLock<Option<UpdaterClient>> = OnceLock::new();

/// Initialise the client. Call once during backend startup.
pub fn init(client: Option<UpdaterClient>) {
    if UPDATER.set(client).is_err() {
        tracing::warn!("updater_admin::init called twice; ignoring later call");
    }
}

fn client() -> Option<&'static UpdaterClient> {
    UPDATER.get().and_then(|c| c.as_ref())
}

fn err_to_response(e: UpdaterClientError) -> Response {
    let status = e.status();
    // Display already redacts secrets; double-check for JSON bodies.
    let msg = crate::util::redact::redact_secrets(&e.to_string());
    let body = Json(json!({ "error": msg }));
    (status, body).into_response()
}

fn require() -> Result<&'static UpdaterClient, Box<Response>> {
    client().ok_or_else(|| {
        Box::new(
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "error": "updater service is not configured on this backend",
                    "hint": "set MYRIAD_UPDATER_URL and UPDATER_GATEWAY_SECRET (production gateway hop)"
                })),
            )
                .into_response(),
        )
    })
}

fn require_mutate() -> Result<&'static UpdaterClient, Box<Response>> {
    let c = require()?;
    if !c.can_mutate() {
        return Err(Box::new(auth_missing()));
    }
    Ok(c)
}

pub async fn status() -> Response {
    let c = match require() {
        Ok(c) => c,
        Err(r) => return *r,
    };
    match c.get_json("/status").await {
        Ok(v) => Json(v).into_response(),
        Err(e) => err_to_response(e),
    }
}

#[derive(Deserialize)]
pub struct AvailableQuery {
    #[serde(default)]
    pub channel: Option<String>,
    /// Ephemeral override: `release` | `commit`. Must be forwarded to updater —
    /// UI channel checks rely on this when draft mode differs from saved prefs.
    #[serde(default)]
    pub mode: Option<String>,
}

pub async fn available(Query(q): Query<AvailableQuery>) -> Response {
    let c = match require() {
        Ok(c) => c,
        Err(r) => return *r,
    };
    let mut parts = Vec::new();
    if let Some(ch) = q.channel.as_deref().filter(|s| !s.is_empty()) {
        parts.push(format!("channel={}", urlencoding_simple(ch)));
    }
    if let Some(m) = q.mode.as_deref().filter(|s| !s.is_empty()) {
        parts.push(format!("mode={}", urlencoding_simple(m)));
    }
    let path = if parts.is_empty() {
        "/available".to_string()
    } else {
        format!("/available?{}", parts.join("&"))
    };
    match c.get_json(&path).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => err_to_response(e),
    }
}

pub async fn jobs() -> Response {
    let c = match require() {
        Ok(c) => c,
        Err(r) => return *r,
    };
    match c.get_json("/jobs").await {
        Ok(v) => Json(v).into_response(),
        Err(e) => err_to_response(e),
    }
}

pub async fn job(Path(id): Path<String>) -> Response {
    let c = match require() {
        Ok(c) => c,
        Err(r) => return *r,
    };
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "invalid job id" })),
        )
            .into_response();
    }
    match c.get_json(&format!("/jobs/{id}")).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => err_to_response(e),
    }
}

pub async fn snapshots() -> Response {
    let c = match require() {
        Ok(c) => c,
        Err(r) => return *r,
    };
    match c.get_json("/snapshots").await {
        Ok(v) => Json(v).into_response(),
        Err(e) => err_to_response(e),
    }
}

#[derive(Deserialize)]
pub struct CommitsQuery {
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub limit: Option<u32>,
}

pub async fn commits(Query(q): Query<CommitsQuery>) -> Response {
    let c = match require() {
        Ok(c) => c,
        Err(r) => return *r,
    };
    let mut path = "/commits?".to_string();
    let mut parts = Vec::new();
    if let Some(b) = q.branch.as_deref().filter(|s| !s.is_empty()) {
        parts.push(format!("branch={b}"));
    }
    if let Some(n) = q.limit {
        parts.push(format!("limit={n}"));
    }
    path.push_str(&parts.join("&"));
    match c.get_json(&path).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => err_to_response(e),
    }
}

#[derive(Deserialize)]
pub struct BuildsQuery {
    #[serde(default)]
    pub limit: Option<u32>,
}

/// List immutable commit builds that exist in both Docker Hub image repositories.
pub async fn builds(Query(q): Query<BuildsQuery>) -> Response {
    let c = match require() {
        Ok(c) => c,
        Err(r) => return *r,
    };
    let path = q
        .limit
        .map(|limit| format!("/builds?limit={limit}"))
        .unwrap_or_else(|| "/builds".to_string());
    match c.get_json(&path).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => err_to_response(e),
    }
}

#[derive(Deserialize)]
pub struct ReleasesQuery {
    #[serde(default)]
    pub channel: Option<String>,
    #[serde(default)]
    pub limit: Option<u32>,
}

pub async fn releases(Query(q): Query<ReleasesQuery>) -> Response {
    let c = match require() {
        Ok(c) => c,
        Err(r) => return *r,
    };
    let mut parts = Vec::new();
    if let Some(ch) = q.channel.as_deref().filter(|s| !s.is_empty()) {
        parts.push(format!("channel={ch}"));
    }
    if let Some(n) = q.limit {
        parts.push(format!("limit={n}"));
    }
    let path = if parts.is_empty() {
        "/releases".to_string()
    } else {
        format!("/releases?{}", parts.join("&"))
    };
    match c.get_json(&path).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => err_to_response(e),
    }
}

#[derive(Deserialize)]
pub struct CompareQuery {
    #[serde(default)]
    pub from: Option<String>,
    pub to: String,
}

pub async fn compare(Query(q): Query<CompareQuery>) -> Response {
    let c = match require() {
        Ok(c) => c,
        Err(r) => return *r,
    };
    let mut parts = vec![format!("to={}", urlencoding_simple(&q.to))];
    if let Some(f) = q.from.as_deref().filter(|s| !s.is_empty()) {
        parts.push(format!("from={}", urlencoding_simple(f)));
    }
    let path = format!("/compare?{}", parts.join("&"));
    match c.get_json(&path).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => err_to_response(e),
    }
}

fn urlencoding_simple(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[derive(Deserialize)]
pub struct UpdateBody {
    #[serde(default)]
    pub target_version: Option<String>,
    #[serde(default)]
    pub target_commit: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub allow_downgrade: bool,
    #[serde(default)]
    pub allow_risk: bool,
    #[serde(default)]
    pub allow_diverged: Option<bool>,
    #[serde(default)]
    pub allow_unknown: Option<bool>,
    #[serde(default)]
    pub allow_irreversible: Option<bool>,
    #[serde(default)]
    pub allow_skip_versions: bool,
    /// Explicit confirm when any risk / downgrade flags are set. Required by updater
    /// soft gate; normal upgrades leave this false/omitted (no extra UX click).
    #[serde(default)]
    pub confirm_risk: bool,
}

fn update_requests_risk(body: &UpdateBody) -> bool {
    body.allow_risk
        || body.allow_downgrade
        || body.allow_diverged == Some(true)
        || body.allow_unknown == Some(true)
        || body.allow_irreversible == Some(true)
}

pub async fn trigger_update(headers: HeaderMap, Json(body): Json<UpdateBody>) -> Response {
    let c = match require_mutate() {
        Ok(c) => c,
        Err(r) => return *r,
    };
    // Soft gate: when risk flags are set, require explicit confirm_risk (body) or header.
    // Happy-path upgrades (no risk flags) are unchanged.
    let confirm_header = headers
        .get("X-Myriad-Confirm-Risk")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|s| s.eq_ignore_ascii_case("true") || s == "1");
    if update_requests_risk(&body) && !(body.confirm_risk || confirm_header) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "high-risk update requires confirm_risk=true (or X-Myriad-Confirm-Risk: true)",
                "hint": "re-submit with the same risk flags plus confirm_risk after operator confirmation"
            })),
        )
            .into_response();
    }
    log_admin_actor("update", &headers);
    let idem = headers
        .get("Idempotency-Key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let actor = actor_from_headers(&headers);
    let mut payload = json!({
        "allow_skip_versions": body.allow_skip_versions,
        "allow_downgrade": body.allow_downgrade,
        "allow_risk": body.allow_risk,
        "confirm_risk": body.confirm_risk || confirm_header,
    });
    if let Some(v) = body.target_version {
        payload["target_version"] = json!(v);
    }
    if let Some(v) = body.target_commit {
        payload["target_commit"] = json!(v);
    }
    if let Some(v) = body.mode {
        payload["mode"] = json!(v);
    }
    if let Some(v) = body.allow_diverged {
        payload["allow_diverged"] = json!(v);
    }
    if let Some(v) = body.allow_unknown {
        payload["allow_unknown"] = json!(v);
    }
    if let Some(v) = body.allow_irreversible {
        payload["allow_irreversible"] = json!(v);
    }
    match c
        .post_json_with_actor(
            "/update",
            Some(&payload),
            idem.as_deref(),
            actor.as_deref(),
        )
        .await
    {
        Ok(v) => {
            track_updater_job(c.clone(), &v, &headers, "update").await;
            Json(v).into_response()
        }
        Err(e) => err_to_response(e),
    }
}

#[derive(Deserialize)]
pub struct PrefsBody {
    #[serde(default)]
    pub channel: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
}

pub async fn set_prefs(Json(body): Json<PrefsBody>) -> Response {
    let c = match require_mutate() {
        Ok(c) => c,
        Err(r) => return *r,
    };
    let payload = json!({
        "channel": body.channel,
        "mode": body.mode,
    });
    match c.post_json("/prefs", Some(&payload), None).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => err_to_response(e),
    }
}

#[derive(Deserialize)]
pub struct RollbackBody {
    pub snapshot_id: String,
}

pub async fn rollback(headers: HeaderMap, Json(body): Json<RollbackBody>) -> Response {
    let c = match require_mutate() {
        Ok(c) => c,
        Err(r) => return *r,
    };
    log_admin_actor("rollback", &headers);
    let actor = actor_from_headers(&headers);
    let payload = json!({ "snapshot_id": body.snapshot_id });
    match c
        .post_json_with_actor("/rollback", Some(&payload), None, actor.as_deref())
        .await
    {
        Ok(v) => {
            track_updater_job(c.clone(), &v, &headers, "rollback").await;
            Json(v).into_response()
        }
        Err(e) => err_to_response(e),
    }
}

pub async fn diagnostics() -> Response {
    let c = match require_mutate() {
        Ok(c) => c,
        Err(r) => return *r,
    };
    match c.get_json("/diagnostics").await {
        Ok(v) => Json(v).into_response(),
        Err(e) => err_to_response(e),
    }
}

pub async fn exit_maintenance(headers: HeaderMap) -> Response {
    let c = match require_mutate() {
        Ok(c) => c,
        Err(r) => return *r,
    };
    log_admin_actor("rescue/exit-maintenance", &headers);
    let actor = actor_from_headers(&headers);
    match c
        .post_json_with_actor::<Value>("/rescue/exit-maintenance", None, None, actor.as_deref())
        .await
    {
        Ok(v) => Json(v).into_response(),
        Err(e) => err_to_response(e),
    }
}

pub async fn forget_current(headers: HeaderMap) -> Response {
    let c = match require_mutate() {
        Ok(c) => c,
        Err(r) => return *r,
    };
    log_admin_actor("rescue/forget-current", &headers);
    let actor = actor_from_headers(&headers);
    match c
        .post_json_with_actor::<Value>("/rescue/forget-current", None, None, actor.as_deref())
        .await
    {
        Ok(v) => Json(v).into_response(),
        Err(e) => err_to_response(e),
    }
}

/// One-click recovery: roll back to the snapshot on the stuck needs_manual job.
pub async fn rescue_continue(headers: HeaderMap) -> Response {
    let c = match require_mutate() {
        Ok(c) => c,
        Err(r) => return *r,
    };
    log_admin_actor("rescue/continue", &headers);
    let actor = actor_from_headers(&headers);
    match c
        .post_json_with_actor::<Value>("/rescue/continue", None, None, actor.as_deref())
        .await
    {
        Ok(v) => {
            track_updater_job(c.clone(), &v, &headers, "rollback").await;
            Json(v).into_response()
        }
        Err(e) => err_to_response(e),
    }
}

/// Trigger the updater's self-update flow. Spawns a helper container that replaces
/// the running updater after a short delay. See docs/updater-spec.md §14.
pub async fn self_update(headers: HeaderMap) -> Response {
    let c = match require_mutate() {
        Ok(c) => c,
        Err(r) => return *r,
    };
    log_admin_actor("self-update", &headers);
    let actor = actor_from_headers(&headers);
    match c
        .post_json_with_actor::<Value>("/admin/self-update", None, None, actor.as_deref())
        .await
    {
        Ok(v) => Json(v).into_response(),
        Err(e) => err_to_response(e),
    }
}

/// Last TCB self-update helper outcome (`state/self-update-last.json`). Also on GET /status.
pub async fn self_update_last() -> Response {
    let c = match require() {
        Ok(c) => c,
        Err(r) => return *r,
    };
    match c.get_json("/self-update/last").await {
        Ok(v) => Json(v).into_response(),
        Err(e) => err_to_response(e),
    }
}

fn auth_missing() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({
            "error": "backend cannot authenticate to updater-gateway; mutating requests disabled",
            "hint": "set UPDATER_GATEWAY_SECRET on backend (and matching gateway). Legacy direct hops may set UPDATE_TOKEN instead."
        })),
    )
        .into_response()
}
