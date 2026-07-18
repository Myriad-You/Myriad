//! Route definitions.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    middleware,
    response::{IntoResponse, Json},
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::api::{auth, ApiState};
use crate::error::UpdaterError;
use crate::state::Phase;
use crate::version::{DeployTag, UpdateMode};
use crate::worker::{AvailableInfo, Command as WorkerCmd};

pub fn build(state: ApiState) -> Router {
    // Keep only the liveness probe unauthenticated. Even read-only updater endpoints expose
    // deployment metadata or trigger outbound release/build discovery, and this service can ask
    // docker-guard to mutate the stack. The backend attaches UPDATE_TOKEN to every upstream call.
    let public = Router::new().route("/healthz", get(healthz));

    let token_only = Router::new()
        .route("/status", get(status))
        .route("/available", get(available))
        .route("/commits", get(list_commits))
        .route("/builds", get(list_builds))
        .route("/releases", get(list_releases))
        .route("/compare", get(compare_refs))
        .route("/jobs", get(list_jobs))
        .route("/jobs/{id}", get(get_job))
        .route("/snapshots", get(list_snapshots))
        .route("/update", post(update))
        .route("/prefs", post(set_prefs))
        .route("/rollback", post(rollback))
        // One-click recovery for needs_manual / stuck post-swap jobs: same privilege as
        // `/rollback` (admin + token via backend). Does not require host manual-override
        // because it only rolls back to the snapshot already associated with the stuck job.
        .route("/rescue/continue", post(rescue_continue))
        .route("/admin/self-update", post(self_update))
        // Durable last self-update outcome (also embedded in GET /status as self_update_last).
        .route("/self-update/last", get(self_update_last))
        .route("/diagnostics", get(diagnostics))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::token_required,
        ));

    let manual = Router::new()
        .route("/rescue/exit-maintenance", post(rescue_exit))
        .route("/rescue/forget-current", post(rescue_forget))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::token_and_manual_required,
        ));

    Router::new()
        .merge(public)
        .merge(token_only)
        .merge(manual)
        .with_state(state)
}

#[derive(Serialize)]
struct StatusResp {
    schema_version: u32,
    updater_version: String,
    current_version: Option<DeployTag>,
    current_commit_sha: Option<String>,
    channel: String,
    /// release | commit
    update_mode: UpdateMode,
    /// Effective check interval (prefs or CHECK_INTERVAL_SECS fallback). 0 = off.
    check_interval_secs: u64,
    /// Raw prefs value when set; omitted when using env fallback.
    #[serde(skip_serializing_if = "Option::is_none")]
    check_interval_secs_pref: Option<u64>,
    /// Auto-install clear upgrades on the current channel. Default false.
    auto_install: bool,
    maintenance_active: bool,
    maintenance_phase: Phase,
    job_in_flight: Option<String>,
    latest_available: Option<crate::state::LatestAvailable>,
    update_available: bool,
    /// Target is older than current; UI should confirm before calling update with allow_downgrade.
    downgrade_available: bool,
    requires_self_update: bool,
    last_checked_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rescue_snapshot_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rescue_source_version: Option<DeployTag>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rollback_version: Option<DeployTag>,
    /// Channels valid for the current mode (for UI selectors).
    available_channels: Vec<&'static str>,
    /// Last TCB self-update helper outcome from `state/self-update-last.json` (if any).
    #[serde(skip_serializing_if = "Option::is_none")]
    self_update_last: Option<crate::docker::self_update_helper::SelfUpdateLastStatus>,
}

/// Public liveness probe. Intentionally minimal: no versions, token status, or secrets.
/// Compose / orchestrator healthchecks only need HTTP 200 + `{"ok":true}`.
async fn healthz() -> Json<Value> {
    Json(json!({"ok": true}))
}

async fn status(State(st): State<ApiState>) -> Result<Json<StatusResp>, ApiError> {
    let u = st.state.read_updater()?;
    let m = st.state.read_maintenance()?;
    let job = st.state.read_current_job()?;
    let channel = st.worker.effective_channel();
    let update_mode = st.worker.effective_mode();

    // Trust explicit tri-state only — no fuzzy tag-string fallbacks.
    let update_available = u
        .latest_available
        .as_ref()
        .is_some_and(|la| la.is_upgrade == Some(true));
    let downgrade_available = u
        .latest_available
        .as_ref()
        .is_some_and(|la| la.is_downgrade == Some(true));
    let requires_self_update = u
        .latest_available
        .as_ref()
        .is_some_and(|la| la.requires_self_update);

    let (rescue_snapshot_id, rescue_source_version) = resolve_rescue_hint(&st, &m, job.as_deref())?;

    // Product tracks. Commit mode is only valid when channel == preview (enforced
    // in validate_channel_for_mode); the channel list itself does not change.
    let available_channels = vec!["stable", "preview"];
    let self_update_last = read_self_update_last(st.state.root());

    Ok(Json(StatusResp {
        schema_version: 1,
        updater_version: crate::self_version().to_string(),
        current_version: u.current_version,
        current_commit_sha: u.current_commit_sha,
        channel,
        update_mode,
        check_interval_secs: st.worker.effective_check_interval_secs(),
        check_interval_secs_pref: u.check_interval_secs,
        auto_install: u.auto_install,
        maintenance_active: m.active,
        maintenance_phase: m.phase,
        job_in_flight: job,
        latest_available: u.latest_available,
        update_available,
        downgrade_available,
        requires_self_update,
        last_checked_at: u.last_checked_at,
        rescue_snapshot_id,
        rescue_source_version,
        rollback_version: u.rollback_version,
        available_channels,
        self_update_last,
    }))
}

fn read_self_update_last(
    state_root: &std::path::Path,
) -> Option<crate::docker::self_update_helper::SelfUpdateLastStatus> {
    let path = state_root.join("self-update-last.json");
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Token-authenticated view of `state/self-update-last.json` (null when never run).
async fn self_update_last(State(st): State<ApiState>) -> Result<Json<Value>, ApiError> {
    match read_self_update_last(st.state.root()) {
        Some(s) => Ok(Json(serde_json::to_value(s)?)),
        None => Ok(Json(json!(null))),
    }
}

/// Find the snapshot (and source version) to offer for one-click continue when stuck.
fn resolve_rescue_hint(
    st: &ApiState,
    maint: &crate::state::MaintenanceFile,
    current_job: Option<&str>,
) -> Result<(Option<String>, Option<DeployTag>), ApiError> {
    let stuck = matches!(maint.phase, Phase::NeedsManual)
        || (maint.active && (maint.phase.is_post_swap() || maint.phase.is_rollback()));
    if !stuck {
        return Ok((None, None));
    }

    let job_id = maint
        .job_id
        .clone()
        .or_else(|| current_job.map(|s| s.to_string()));
    let Some(job_id) = job_id else {
        return Ok((None, None));
    };
    let job = match st.state.read_job(&job_id) {
        Ok(j) => j,
        Err(_) => return Ok((None, None)),
    };
    let Some(snapshot_id) = job.snapshot_id else {
        return Ok((None, None));
    };

    let source = st
        .state
        .read_snapshots()
        .ok()
        .and_then(|sf| {
            sf.items
                .into_iter()
                .find(|m| m.id == snapshot_id)
                .and_then(|m| m.source_version)
        })
        .or(job.from_version);

    Ok((Some(snapshot_id), source))
}

#[derive(Deserialize)]
struct AvailableQuery {
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    mode: Option<String>,
}

async fn available(
    State(st): State<ApiState>,
    Query(q): Query<AvailableQuery>,
) -> Result<Json<Value>, ApiError> {
    // Query params are ephemeral overrides for this check only — they do NOT persist.
    // Use POST /prefs to change saved channel/mode.
    let mode = q
        .mode
        .as_deref()
        .map(|s| s.parse::<UpdateMode>())
        .transpose()
        .map_err(ApiError::from)?;

    let (tx, rx) = tokio::sync::oneshot::channel();
    st.worker
        .sender()
        .send(WorkerCmd::CheckUpdates {
            channel: q.channel.clone(),
            mode,
            reply: tx,
        })
        .await
        .map_err(|_| ApiError(StatusCode::SERVICE_UNAVAILABLE, "worker unavailable".into()))?;
    let info = rx
        .await
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "worker dropped".into()))??;
    Ok(Json(available_to_json(info)))
}

fn available_to_json(info: Option<AvailableInfo>) -> Value {
    match info {
        None => Value::Null,
        Some(AvailableInfo::Release(m)) => serde_json::to_value(m).unwrap_or(Value::Null),
        Some(AvailableInfo::Commit {
            tag,
            full_sha,
            message,
            branch,
            notes_url,
            source,
            freshness,
            is_upgrade,
            is_downgrade,
            relation,
        }) => {
            // Prefer precomputed direction (push-time / ancestry). Fall back to freshness.
            let (rel, ahead_by, behind_by, current_sha, is_up, is_down) =
                match (relation.as_deref(), is_upgrade, is_downgrade, freshness.as_ref()) {
                    (Some(r), Some(up), Some(down), f) => (
                        Some(r),
                        f.map(|x| x.ahead_by),
                        f.map(|x| x.behind_by),
                        f.and_then(|x| x.current_sha.clone()),
                        Some(up),
                        Some(down),
                    ),
                    (_, _, _, Some(f)) => (
                        Some(f.relation.as_str()),
                        Some(f.ahead_by),
                        Some(f.behind_by),
                        f.current_sha.clone(),
                        Some(f.is_upgrade()),
                        Some(f.is_downgrade()),
                    ),
                    _ => (Some("unknown"), None, None, None, Some(true), Some(false)),
                };
            json!({
                "schema_version": 1,
                "mode": "commit",
                "source": source,
                "version": tag.as_str(),
                "channel": branch,
                "commit_sha": full_sha,
                "current_commit_sha": current_sha,
                "relation": rel,
                "ahead_by": ahead_by,
                "behind_by": behind_by,
                "is_upgrade": is_up,
                "is_downgrade": is_down,
                "message": message,
                "notes_url": notes_url,
                "released_at": chrono::Utc::now().to_rfc3339(),
                "images": {},
                "env": { "required": [], "new": [], "removed": [] },
                "migrations": {
                    "irreversible": false,
                    "estimated_seconds": 60,
                    "requires_full_backup": true
                },
                "updater": {
                    "min_updater_version": crate::self_version(),
                    "self_update_required": false
                },
                "postgres": { "min_pg_version": "15", "max_pg_version": "unbounded" },
                "signature": null
            })
        }
    }
}

async fn list_jobs(State(st): State<ApiState>) -> Result<Json<Vec<String>>, ApiError> {
    let mut ids = st.state.list_jobs()?;
    ids.sort();
    Ok(Json(ids))
}

async fn get_job(
    State(st): State<ApiState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let job = st.state.read_job(&id)?;
    Ok(Json(serde_json::to_value(job)?))
}

async fn list_snapshots(State(st): State<ApiState>) -> Result<Json<Value>, ApiError> {
    let s = st.state.read_snapshots()?;
    Ok(Json(serde_json::to_value(s)?))
}

#[derive(Deserialize)]
struct UpdateBody {
    /// Release tag (`v1.2.3`) or commit/branch tag (`dev-abc1234`, bare sha, `main`).
    /// Alias: `target` also accepted via flatten-like dual field below.
    #[serde(default)]
    target_version: Option<String>,
    /// Explicit commit/sha/branch for commit mode (preferred over target_version when set).
    #[serde(default)]
    target_commit: Option<String>,
    /// release | commit — defaults to current prefs.
    #[serde(default)]
    mode: Option<String>,
    /// Must be true when the target is older than the currently running deploy.
    #[serde(default)]
    allow_downgrade: bool,
    /// Umbrella: enables diverged + unknown + irreversible (and downgrade).
    #[serde(default)]
    allow_risk: bool,
    #[serde(default)]
    allow_diverged: Option<bool>,
    #[serde(default)]
    allow_unknown: Option<bool>,
    #[serde(default)]
    allow_irreversible: Option<bool>,
    #[serde(default)]
    allow_skip_versions: bool,
    /// Required when any risk/downgrade flag is true. Normal upgrades omit this.
    #[serde(default)]
    confirm_risk: bool,
}

/// Optional `X-Update-Actor` from the backend hop (after UPDATE_TOKEN auth).
/// Direct callers with a stolen token can forge this — still better than no actor.
fn extract_actor(headers: &axum::http::HeaderMap) -> Option<String> {
    headers
        .get("X-Update-Actor")
        .and_then(|v| v.to_str().ok())
        .and_then(crate::redact::sanitize_actor)
}

fn risk_flags_set(body: &UpdateBody) -> bool {
    body.allow_risk
        || body.allow_downgrade
        || body.allow_diverged == Some(true)
        || body.allow_unknown == Some(true)
        || body.allow_irreversible == Some(true)
}

fn confirm_risk_present(body: &UpdateBody, headers: &axum::http::HeaderMap) -> bool {
    if body.confirm_risk {
        return true;
    }
    headers
        .get("X-Myriad-Confirm-Risk")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|s| s.eq_ignore_ascii_case("true") || s == "1")
}

async fn update(
    State(st): State<ApiState>,
    headers: axum::http::HeaderMap,
    Json(body): Json<UpdateBody>,
) -> Result<Json<Value>, ApiError> {
    let _ = body.allow_skip_versions;
    // Soft gate: high-risk flags need an explicit confirm. Happy-path upgrades unchanged.
    let confirmed = confirm_risk_present(&body, &headers);
    if risk_flags_set(&body) && !confirmed {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "high-risk update requires confirm_risk=true (or X-Myriad-Confirm-Risk: true) \
             alongside allow_risk / allow_downgrade / related flags"
                .into(),
        ));
    }
    let mode = match body.mode.as_deref() {
        Some(s) => s.parse::<UpdateMode>().map_err(ApiError::from)?,
        None => st.worker.effective_mode(),
    };
    let raw = body.target_commit.or(body.target_version).ok_or_else(|| {
        ApiError(
            StatusCode::BAD_REQUEST,
            "target_version or target_commit is required".into(),
        )
    })?;
    let target = DeployTag::parse(&raw).map_err(ApiError::from)?;
    // Resolve mode from the target when possible so dev-channel installs of formal
    // releases (vX.Y.Z) use the release path, and commit tags never go through release.
    let mode = if target.is_release() {
        UpdateMode::Release
    } else if mode == UpdateMode::Release {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            format!(
                "release mode requires a vX.Y.Z target, got {} (use mode=commit for CI tags)",
                target.as_str()
            ),
        ));
    } else {
        UpdateMode::Commit
    };

    let idem = headers
        .get("Idempotency-Key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let actor = extract_actor(&headers);

    let (tx, rx) = tokio::sync::oneshot::channel();
    st.worker
        .sender()
        .send(WorkerCmd::Update {
            target,
            mode,
            allow_downgrade: body.allow_downgrade,
            allow_risk: body.allow_risk,
            allow_diverged: body.allow_diverged,
            allow_unknown: body.allow_unknown,
            allow_irreversible: body.allow_irreversible,
            idempotency_key: idem,
            actor,
            reply: tx,
        })
        .await
        .map_err(|_| ApiError(StatusCode::SERVICE_UNAVAILABLE, "worker unavailable".into()))?;
    let job_id = rx
        .await
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "worker dropped".into()))??;
    Ok(Json(json!({
        "job_id": job_id,
        "mode": mode.as_str(),
        "allow_downgrade": body.allow_downgrade,
        "allow_risk": body.allow_risk,
        "allow_diverged": body.allow_diverged,
        "allow_unknown": body.allow_unknown,
        "allow_irreversible": body.allow_irreversible,
        "confirm_risk": confirmed,
    })))
}

#[derive(Deserialize)]
struct CommitsQuery {
    #[serde(default)]
    branch: Option<String>,
    #[serde(default = "default_commit_limit")]
    limit: u32,
}

fn default_commit_limit() -> u32 {
    20
}

async fn list_commits(
    State(st): State<ApiState>,
    Query(q): Query<CommitsQuery>,
) -> Result<Json<Value>, ApiError> {
    let branch = q.branch.unwrap_or_else(|| st.worker.effective_channel());
    let (tx, rx) = tokio::sync::oneshot::channel();
    st.worker
        .sender()
        .send(WorkerCmd::ListCommits {
            branch: branch.clone(),
            limit: q.limit,
            reply: tx,
        })
        .await
        .map_err(|_| ApiError(StatusCode::SERVICE_UNAVAILABLE, "worker unavailable".into()))?;
    let items = rx
        .await
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "worker dropped".into()))??;
    Ok(Json(json!({
        "schema_version": 1,
        "branch": crate::version::commit_branch_for_channel(&branch),
        "items": items.iter().map(|c| json!({
            "sha": c.sha,
            "short_sha": c.short_sha,
            "message": c.message,
            "html_url": c.html_url,
            "committed_at": c.committed_at,
            "tag": format!("dev-{}", c.short_sha),
        })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
struct BuildsQuery {
    #[serde(default = "default_commit_limit")]
    limit: u32,
}

async fn list_builds(
    State(st): State<ApiState>,
    Query(q): Query<BuildsQuery>,
) -> Result<Json<Value>, ApiError> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    st.worker
        .sender()
        .send(WorkerCmd::ListBuilds {
            limit: q.limit,
            reply: tx,
        })
        .await
        .map_err(|_| ApiError(StatusCode::SERVICE_UNAVAILABLE, "worker unavailable".into()))?;
    let items = rx
        .await
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "worker dropped".into()))??;
    Ok(Json(json!({
        "schema_version": 1,
        "source": "dockerhub",
        "items": items,
    })))
}

#[derive(Deserialize)]
struct ReleasesQuery {
    #[serde(default)]
    channel: Option<String>,
    #[serde(default = "default_commit_limit")]
    limit: u32,
}

async fn list_releases(
    State(st): State<ApiState>,
    Query(q): Query<ReleasesQuery>,
) -> Result<Json<Value>, ApiError> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    st.worker
        .sender()
        .send(WorkerCmd::ListReleases {
            channel: q.channel.clone(),
            limit: q.limit,
            reply: tx,
        })
        .await
        .map_err(|_| ApiError(StatusCode::SERVICE_UNAVAILABLE, "worker unavailable".into()))?;
    let items = rx
        .await
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "worker dropped".into()))??;
    Ok(Json(json!({
        "schema_version": 1,
        "channel": q.channel.unwrap_or_else(|| st.worker.effective_channel()),
        "items": items.iter().map(|r| json!({
            "tag_name": r.tag_name,
            "name": r.name,
            "prerelease": r.prerelease,
            "version": r.tag_name,
        })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
struct CompareQuery {
    /// Optional base; defaults to currently recorded deploy tag.
    #[serde(default)]
    from: Option<String>,
    /// Required target (vX.Y.Z, dev-sha, branch, bare sha).
    to: String,
}

async fn compare_refs(
    State(st): State<ApiState>,
    Query(q): Query<CompareQuery>,
) -> Result<Json<Value>, ApiError> {
    if q.to.trim().is_empty() {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "query param `to` is required".into(),
        ));
    }
    let (tx, rx) = tokio::sync::oneshot::channel();
    st.worker
        .sender()
        .send(WorkerCmd::Compare {
            from: q.from.clone(),
            to: q.to.clone(),
            reply: tx,
        })
        .await
        .map_err(|_| ApiError(StatusCode::SERVICE_UNAVAILABLE, "worker unavailable".into()))?;
    let f = rx
        .await
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "worker dropped".into()))??;
    Ok(Json(json!({
        "schema_version": 1,
        "relation": f.relation.as_str(),
        "ahead_by": f.ahead_by,
        "behind_by": f.behind_by,
        "current_sha": f.current_sha,
        "target_sha": f.target_sha,
        "current_ref": f.current_ref,
        "target_ref": f.target_ref,
        "is_upgrade": f.is_upgrade(),
        "is_downgrade": f.is_downgrade(),
    })))
}

#[derive(Deserialize)]
struct PrefsBody {
    #[serde(default)]
    channel: Option<String>,
    /// release | commit
    #[serde(default)]
    mode: Option<String>,
    /// Check interval seconds: 0 | 3600 | 21600 | 43200 | 86400.
    /// Send JSON `null` explicitly to clear the pref (fall back to CHECK_INTERVAL_SECS).
    #[serde(default, deserialize_with = "deserialize_opt_opt_u64")]
    check_interval_secs: Option<Option<u64>>,
    #[serde(default)]
    auto_install: Option<bool>,
}

/// Distinguishes "field omitted" (None) from "field set to null" (Some(None)).
fn deserialize_opt_opt_u64<'de, D>(deserializer: D) -> std::result::Result<Option<Option<u64>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::<u64>::deserialize(deserializer)?))
}

async fn set_prefs(
    State(st): State<ApiState>,
    Json(body): Json<PrefsBody>,
) -> Result<Json<Value>, ApiError> {
    if body.channel.is_none()
        && body.mode.is_none()
        && body.check_interval_secs.is_none()
        && body.auto_install.is_none()
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "provide channel, mode, check_interval_secs, and/or auto_install".into(),
        ));
    }
    let mode = body
        .mode
        .as_deref()
        .map(|s| s.parse::<UpdateMode>())
        .transpose()
        .map_err(ApiError::from)?;
    let (tx, rx) = tokio::sync::oneshot::channel();
    st.worker
        .sender()
        .send(WorkerCmd::SetPrefs {
            channel: body.channel,
            mode,
            check_interval_secs: body.check_interval_secs,
            auto_install: body.auto_install,
            reply: tx,
        })
        .await
        .map_err(|_| ApiError(StatusCode::SERVICE_UNAVAILABLE, "worker unavailable".into()))?;
    let prefs = rx
        .await
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "worker dropped".into()))??;
    Ok(Json(json!({
        "ok": true,
        "channel": prefs.channel,
        "mode": prefs.mode.as_str(),
        "check_interval_secs": prefs.check_interval_secs,
        "check_interval_secs_pref": prefs.check_interval_secs_pref,
        "auto_install": prefs.auto_install,
    })))
}

#[derive(Deserialize)]
struct RollbackBody {
    snapshot_id: String,
}

async fn self_update(
    State(st): State<ApiState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let actor = extract_actor(&headers);
    let (tx, rx) = tokio::sync::oneshot::channel();
    st.worker
        .sender()
        .send(WorkerCmd::SelfUpdate {
            actor,
            reply: tx,
        })
        .await
        .map_err(|_| ApiError(StatusCode::SERVICE_UNAVAILABLE, "worker unavailable".into()))?;
    let report = rx
        .await
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "worker dropped".into()))??;
    Ok(Json(json!({
        "ok": true,
        "helper_container_id": report.helper_container_id,
        "new_updater_tag": report.new_updater_tag,
        "previous_updater_tag": report.previous_updater_tag,
        "scheduled": report.scheduled,
    })))
}

async fn rollback(
    State(st): State<ApiState>,
    headers: axum::http::HeaderMap,
    Json(body): Json<RollbackBody>,
) -> Result<Json<Value>, ApiError> {
    let actor = extract_actor(&headers);
    let (tx, rx) = tokio::sync::oneshot::channel();
    st.worker
        .sender()
        .send(WorkerCmd::Rollback {
            snapshot_id: body.snapshot_id,
            actor,
            reply: tx,
        })
        .await
        .map_err(|_| ApiError(StatusCode::SERVICE_UNAVAILABLE, "worker unavailable".into()))?;
    let job_id = rx
        .await
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "worker dropped".into()))??;
    Ok(Json(json!({"job_id": job_id})))
}

async fn diagnostics(State(st): State<ApiState>) -> Result<Json<Value>, ApiError> {
    let updater = st.state.read_updater()?;
    let maintenance = st.state.read_maintenance()?;
    let snapshots = st.state.read_snapshots()?;
    let env_probe_path = st.state.root().join("env-probe.json");
    let env_probe = std::fs::read_to_string(&env_probe_path)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok());
    let history =
        crate::state::history::tail(&st.state.root().join("history.log"), 100).unwrap_or_default();
    Ok(Json(json!({
        "updater_version": crate::self_version(),
        "config": {
            "channel": st.config.channel.to_string(),
            "registry_mirror": st.config.registry_mirror,
            "check_interval_secs": st.config.check_interval_secs,
        },
        "state": {
            "updater": updater,
            "maintenance": maintenance,
            "snapshots": snapshots,
            "env_probe": env_probe,
            "history_tail": history,
        }
    })))
}

async fn rescue_exit(
    State(st): State<ApiState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Value>, ApiError> {
    st.state.clear_maintenance()?;
    st.state.set_current_job(None)?;
    let actor = extract_actor(&headers);
    let line = match actor.as_deref() {
        Some(a) => format!("audit: rescue_exit_maintenance via=API actor={a}"),
        None => "audit: rescue_exit_maintenance via=API".to_string(),
    };
    st.state.append_history(&line)?;
    let _ = st.state.append_audit(&line);
    Ok(Json(json!({"ok": true})))
}

/// One-click recovery: roll back to the snapshot recorded on the stuck job.
///
/// Clears a stuck `job.current` / needs_manual marker enough for a new rollback job
/// to start, then enqueues the same path as `POST /rollback`.
async fn rescue_continue(
    State(st): State<ApiState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let m = st.state.read_maintenance()?;
    let current = st.state.read_current_job()?;
    let (snapshot_id, source_version) = resolve_rescue_hint(&st, &m, current.as_deref())?;
    let Some(snapshot_id) = snapshot_id else {
        return Err(ApiError(
            StatusCode::PRECONDITION_FAILED,
            "no stuck job with a snapshot to continue from; pick a snapshot and POST /rollback"
                .into(),
        ));
    };

    // Verify snapshot still exists on disk metadata.
    let snaps = st.state.read_snapshots()?;
    if !snaps.items.iter().any(|s| s.id == snapshot_id) {
        return Err(ApiError(
            StatusCode::NOT_FOUND,
            format!("rescue snapshot {snapshot_id} is missing from snapshots.json"),
        ));
    }

    // Free the single-slot worker if the failed job is still marked current.
    if current.is_some() {
        st.state.set_current_job(None)?;
    }
    let actor = extract_actor(&headers);
    let actor_suffix = actor
        .as_deref()
        .map(|a| format!(" actor={a}"))
        .unwrap_or_default();
    let hist = format!(
        "rescue/continue: rolling back to {snapshot_id} (source={}){actor_suffix}",
        source_version
            .as_ref()
            .map(|v| v.to_string())
            .unwrap_or_else(|| "?".into())
    );
    let audit = format!(
        "audit: rescue_continue snapshot={snapshot_id} source={}{actor_suffix}",
        source_version
            .as_ref()
            .map(|v| v.to_string())
            .unwrap_or_else(|| "?".into())
    );
    st.state.append_history(&hist)?;
    let _ = st.state.append_audit(&audit);

    let (tx, rx) = tokio::sync::oneshot::channel();
    st.worker
        .sender()
        .send(WorkerCmd::Rollback {
            snapshot_id: snapshot_id.clone(),
            actor,
            reply: tx,
        })
        .await
        .map_err(|_| ApiError(StatusCode::SERVICE_UNAVAILABLE, "worker unavailable".into()))?;
    let job_id = rx
        .await
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "worker dropped".into()))??;

    Ok(Json(json!({
        "ok": true,
        "job_id": job_id,
        "snapshot_id": snapshot_id,
        "source_version": source_version,
    })))
}

async fn rescue_forget(
    State(st): State<ApiState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Value>, ApiError> {
    st.state.set_current_job(None)?;
    let actor = extract_actor(&headers);
    let line = match actor.as_deref() {
        Some(a) => format!("audit: rescue_forget_job via=API actor={a}"),
        None => "audit: rescue_forget_job via=API".to_string(),
    };
    st.state.append_history(&line)?;
    let _ = st.state.append_audit(&line);
    Ok(Json(json!({"ok": true})))
}

// ----- error mapping -----

pub struct ApiError(pub StatusCode, pub String);

impl<E: Into<UpdaterError>> From<E> for ApiError {
    fn from(e: E) -> Self {
        let e = e.into();
        let status = match &e {
            UpdaterError::Unauthorized => StatusCode::UNAUTHORIZED,
            UpdaterError::Conflict => StatusCode::CONFLICT,
            UpdaterError::NotFound(_) => StatusCode::NOT_FOUND,
            UpdaterError::InvalidInput(_) => StatusCode::BAD_REQUEST,
            UpdaterError::Precondition(_) => StatusCode::PRECONDITION_FAILED,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        ApiError(status, e.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        // Redact known secrets if an error string ever echoed env/header values.
        let msg = crate::redact::redact_secrets(&self.1);
        let body = Json(json!({"error": msg}));
        (self.0, body).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn healthz_is_minimal() {
        let Json(v) = healthz().await;
        assert_eq!(v, json!({"ok": true}));
        // No version / token / config leakage on the public probe.
        assert!(v.as_object().map(|o| o.len() == 1).unwrap_or(false));
    }

    #[test]
    fn dockerhub_available_payload_uses_precomputed_direction() {
        let payload = available_to_json(Some(AvailableInfo::Commit {
            tag: DeployTag::parse("dev-5a4527a").unwrap(),
            full_sha: "5a4527a".to_string(),
            message: "Docker Hub common frontend/backend build".to_string(),
            branch: "preview".to_string(),
            notes_url: "https://hub.docker.com/r/example/backend/tags?name=dev-5a4527a".to_string(),
            source: "dockerhub".to_string(),
            freshness: None,
            // Push-time newer → ahead, even without git ancestry.
            is_upgrade: Some(true),
            is_downgrade: Some(false),
            relation: Some("ahead".to_string()),
        }));

        assert_eq!(payload["source"], "dockerhub");
        assert_eq!(payload["relation"], "ahead");
        assert_eq!(payload["is_upgrade"], true);
        assert_eq!(payload["is_downgrade"], false);
    }

    fn sample_update(allow_risk: bool, allow_downgrade: bool, confirm_risk: bool) -> UpdateBody {
        UpdateBody {
            target_version: Some("v1.0.0".into()),
            target_commit: None,
            mode: None,
            allow_downgrade,
            allow_risk,
            allow_diverged: None,
            allow_unknown: None,
            allow_irreversible: None,
            allow_skip_versions: false,
            confirm_risk,
        }
    }

    #[test]
    fn risk_soft_gate_only_when_flags_set() {
        let clean = sample_update(false, false, false);
        assert!(!risk_flags_set(&clean));

        let risky = sample_update(true, false, false);
        assert!(risk_flags_set(&risky));
        assert!(!confirm_risk_present(&risky, &axum::http::HeaderMap::new()));

        let confirmed = sample_update(false, true, true);
        assert!(risk_flags_set(&confirmed));
        assert!(confirm_risk_present(
            &confirmed,
            &axum::http::HeaderMap::new()
        ));

        let via_header = sample_update(false, true, false);
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            "X-Myriad-Confirm-Risk",
            axum::http::HeaderValue::from_static("true"),
        );
        assert!(confirm_risk_present(&via_header, &headers));
    }
}
