//! 投递队列服务（Layer 2）
//!
//! 后台任务：从 federation_delivery_queue 取出待投递的 Activity，
//! 签名后发送到目标 inbox，支持指数退避重试。
//!
//! ## Lifecycle
//! - `spawn_delivery_worker` is started from `main` when the service boots in
//!   **full mode** (DB connected). It polls every 15s via `process_delivery_queue`.
//! - Publish / createNote enqueues rows in `fan_out_to_followers` (content module);
//!   this worker is what actually POSTs signed Activities to remote inboxes.
//!
//! ## Public media (Note attachments)
//! - Attachment URLs are served at `GET /media/federation/{userId}/{file}` with
//!   **no auth** (public `ServeDir`) so remote instances can fetch Image/Video
//!   objects embedded in AP Notes. Do not put this path behind session middleware.

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde::Serialize;
use serde_json::json;
use std::time::Duration;

use crate::federation::keys::KeyPair;
use crate::federation::signature::{sign_request, SignatureParams};
use crate::federation::types::*;

/// Immediate enqueue result (send path observability — before HTTP delivery).
#[derive(Debug, Clone, Serialize, Default)]
pub struct DeliveryEnqueueInfo {
    /// Rows inserted into federation_delivery_queue
    pub queued: u32,
    /// Remote peers we intended to reach (members / channel peer)
    pub remote_targets: u32,
    /// Members present but missing remote_actors / empty inbox
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub unresolved: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

fn is_zero_u32(v: &u32) -> bool {
    *v == 0
}



/// Fan-out enqueue stats for room activities.
#[derive(Debug, Clone, Default)]
pub struct FanoutResult {
    pub enqueued: u32,
    pub remote_with_inbox: u32,
    pub skipped_empty_inbox: u32,
    pub unresolved_members: u32,
}

impl FanoutResult {
    pub fn to_enqueue_info(&self) -> DeliveryEnqueueInfo {
        let mut info = DeliveryEnqueueInfo {
            queued: self.enqueued,
            remote_targets: self.remote_with_inbox + self.skipped_empty_inbox + self.unresolved_members,
            unresolved: self.unresolved_members + self.skipped_empty_inbox,
            warning: None,
        };
        if self.enqueued == 0 && info.remote_targets > 0 {
            info.warning = Some(
                "no_delivery_queued: remote members lack inbox or remote_actors row".into(),
            );
        } else if self.skipped_empty_inbox > 0 || self.unresolved_members > 0 {
            info.warning = Some(format!(
                "partial_enqueue: queued={} empty_inbox={} unresolved={}",
                self.enqueued, self.skipped_empty_inbox, self.unresolved_members
            ));
        }
        info
    }
}

/// Delivery queue batch outcome (worker metrics).
#[derive(Debug, Clone, Default)]
pub struct DeliveryBatchStats {
    pub delivered: u32,
    pub dead: u32,
    pub retried: u32,
}

/// 投递队列处理器 — 由后台任务驱动
///
/// 每次调用处理一批待投递的 Activity（最多 batch_size 个）
#[allow(dead_code)]
pub async fn process_delivery_queue(
    db: &DatabaseConnection,
    batch_size: u32,
) -> Result<u32, String> {
    let stats = process_delivery_queue_detailed(db, batch_size).await?;
    Ok(stats.delivered)
}

/// Same as process_delivery_queue but returns full batch counters.
pub async fn process_delivery_queue_detailed(
    db: &DatabaseConnection,
    batch_size: u32,
) -> Result<DeliveryBatchStats, String> {
    // Atomic claim with FOR UPDATE SKIP LOCKED so concurrent workers do not
    // double-deliver the same row. Also reclaims stuck `delivering` rows from
    // crashed workers (#97 behaviour kept).
    let pending = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            // MATERIALIZED keeps FOR UPDATE SKIP LOCKED from being inlined away (PG12+).
            r#"WITH selected AS MATERIALIZED (
                   SELECT id, status AS prev_status, attempts AS prev_attempts
                   FROM federation_delivery_queue
                   WHERE (status = 'pending'
                          AND (next_retry_at IS NULL OR next_retry_at <= NOW()))
                      -- 崩溃恢复：投递中途进程退出会把条目留在 delivering，超时后重新认领
                      OR (status = 'delivering'
                          AND last_attempt_at < NOW() - INTERVAL '10 minutes')
                   ORDER BY created_at ASC
                   LIMIT $1
                   FOR UPDATE SKIP LOCKED
               ),
               claimed AS (
                   UPDATE federation_delivery_queue dq
                   SET status = 'delivering',
                       last_attempt_at = NOW(),
                       attempts = CASE
                           WHEN s.prev_status = 'delivering' THEN s.prev_attempts + 1
                           ELSE s.prev_attempts
                       END
                   FROM selected s
                   WHERE dq.id = s.id
                   RETURNING dq.id, dq.activity_id, dq.target_inbox, dq.target_domain,
                             dq.attempts, dq.max_attempts, s.prev_status
               )
               SELECT c.id, c.activity_id, c.target_inbox, c.target_domain,
                      c.attempts, c.max_attempts, c.prev_status,
                      a.activity_id AS ap_activity_id, a.activity_type, a.object_json, a.user_id
               FROM claimed c
               JOIN federation_activities a ON a.id = c.activity_id"#,
            [(batch_size as i64).into()],
        ))
        .await
        .map_err(|e| format!("Queue claim failed: {}", e))?;

    let mut stats = DeliveryBatchStats::default();

    for row in pending {
        let queue_id: i32 = row.try_get("", "id").unwrap_or(0);
        let target_inbox: String = row.try_get("", "target_inbox").unwrap_or_default();
        let target_domain: String = row.try_get("", "target_domain").unwrap_or_default();
        let attempts: i32 = row.try_get("", "attempts").unwrap_or(0);
        let max_attempts: i32 = row.try_get("", "max_attempts").unwrap_or(12);
        let prev_status: String = row.try_get("", "prev_status").unwrap_or_default();
        let _ap_activity_id: String = row.try_get("", "ap_activity_id").unwrap_or_default();
        let activity_type: String = row.try_get("", "activity_type").unwrap_or_default();
        let object_json: serde_json::Value = row.try_get("", "object_json").unwrap_or_default();
        let user_id: i32 = row.try_get("", "user_id").unwrap_or(0);

        // Reclaimed stuck delivering: attempts already incremented in the claim UPDATE
        let reclaim = prev_status == "delivering";
        if reclaim && attempts >= max_attempts {
            let err = "Exceeded max attempts after reclaim";
            let _ = db
                .execute(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "UPDATE federation_delivery_queue SET status = 'dead', error_message = $1, last_attempt_at = NOW() WHERE id = $2",
                    [err.into(), queue_id.into()],
                ))
                .await;
            mark_delivery_dead(user_id, &activity_type, &target_domain, err).await;
            stats.dead += 1;
            continue;
        }

        // 投递前：目标实例信任策略检查（黑名单等）
        if let Err(reason) = crate::federation::trust::enforce_outbound(db, &target_domain).await {
            let _ = db
                .execute(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    r#"UPDATE federation_delivery_queue
                       SET status = 'dead', error_message = $1, last_attempt_at = NOW()
                       WHERE id = $2"#,
                    [reason.clone().into(), queue_id.into()],
                ))
                .await;
            tracing::warn!(
                "🛑 Delivery blocked by trust policy: target={}, reason={}",
                target_inbox,
                reason
            );
            mark_delivery_dead(user_id, &activity_type, &target_domain, &reason).await;
            stats.dead += 1;
            continue;
        }

        // object_json 已经是完整的 Activity JSON（含 @context/type/id/actor/object），直接发送
        let base_url = get_base_url().await;
        let username = get_username_by_id(db, user_id).await.unwrap_or_default();

        let body_bytes = serde_json::to_vec(&object_json).unwrap_or_default();

        // Move is signed as the **old** actor. Prefer activity.actor origin for
        // keyId so peers verifying against the old actor document succeed even
        // after local base_url has switched to the new domain.
        let (sign_base, sign_username) = signing_identity_for_activity(
            &activity_type,
            &object_json,
            &base_url,
            &username,
        );

        // 获取用户密钥对 — if missing, ensure once then reload so already-queued
        // deliveries recover without waiting for GET /users/{username}.
        match load_user_keypair_ensuring(db, user_id, &username).await {
            Ok(keypair) => {
                match deliver_activity(
                    &keypair,
                    &sign_base,
                    &sign_username,
                    &target_inbox,
                    &target_domain,
                    &body_bytes,
                )
                .await
                {
                    Ok(()) => {
                        // 投递成功
                        let _ = db
                            .execute(Statement::from_sql_and_values(
                                DatabaseBackend::Postgres,
                                "UPDATE federation_delivery_queue SET status = 'delivered', last_attempt_at = NOW() WHERE id = $1",
                                [queue_id.into()],
                            ))
                            .await;
                        stats.delivered += 1;

                        // 更新实例的 last_success_at，重置 failure_count
                        let _ = db
                            .execute(Statement::from_sql_and_values(
                                DatabaseBackend::Postgres,
                                "UPDATE federation_instances SET last_success_at = NOW(), failure_count = 0 WHERE domain = $1",
                                [target_domain.clone().into()],
                            ))
                            .await;

                        tracing::debug!("📤 Delivered {} to {}", activity_type, target_inbox);
                    }
                    Err(e) => {
                        let new_attempts = attempts + 1;
                        // Permanent: 4xx from peer, OR 5xx body that is really
                        // not_found / not_member (legacy peers still return 500).
                        let permanent = crate::federation::errors::is_permanent_delivery_error(&e);
                        if permanent || new_attempts >= max_attempts {
                            // 放弃
                            let _ = db
                                .execute(Statement::from_sql_and_values(
                                    DatabaseBackend::Postgres,
                                    "UPDATE federation_delivery_queue SET status = 'dead', attempts = $1, error_message = $2, last_attempt_at = NOW() WHERE id = $3",
                                    [new_attempts.into(), e.clone().into(), queue_id.into()],
                                ))
                                .await;
                            if permanent {
                                tracing::warn!(
                                    "💀 Delivery permanent failure to {}: {}",
                                    target_inbox,
                                    e
                                );
                            } else {
                                tracing::warn!(
                                    "💀 Delivery dead after {} attempts to {}: {}",
                                    new_attempts,
                                    target_inbox,
                                    e
                                );
                            }
                            mark_delivery_dead(user_id, &activity_type, &target_domain, &e).await;
                            stats.dead += 1;
                        } else {
                            // 指数退避：2^attempts 秒，最大 86400 秒 (24h)
                            let backoff_secs = std::cmp::min(2i64.pow(new_attempts as u32), 86400);
                            let _ = db
                                .execute(Statement::from_sql_and_values(
                                    DatabaseBackend::Postgres,
                                    "UPDATE federation_delivery_queue SET status = 'pending', attempts = $1, error_message = $2, last_attempt_at = NOW(), next_retry_at = NOW() + make_interval(secs => $4::double precision) WHERE id = $3",
                                    [new_attempts.into(), e.clone().into(), queue_id.into(), backoff_secs.into()],
                                ))
                                .await;

                            // 更新实例 failure_count
                            let _ = db
                                .execute(Statement::from_sql_and_values(
                                    DatabaseBackend::Postgres,
                                    "UPDATE federation_instances SET failure_count = failure_count + 1 WHERE domain = $1",
                                    [target_domain.clone().into()],
                                ))
                                .await;

                            tracing::warn!(
                                "⚠️ Delivery failed (attempt {}/{}), retrying in {}s: {}",
                                new_attempts,
                                max_attempts,
                                backoff_secs,
                                e
                            );
                            stats.retried += 1;
                        }
                    }
                }
            }
            Err(e) => {
                tracing::error!("Failed to load keypair for user {}: {}", user_id, e);
                // 密钥问题几乎不会自愈；按普通失败计数退避，避免 15s 热循环刷日志
                let new_attempts = attempts + 1;
                let err_msg = format!("Key load failed: {}", e);
                if new_attempts >= max_attempts {
                    let _ = db
                        .execute(Statement::from_sql_and_values(
                            DatabaseBackend::Postgres,
                            "UPDATE federation_delivery_queue SET status = 'dead', attempts = $1, error_message = $2, last_attempt_at = NOW() WHERE id = $3",
                            [
                                new_attempts.into(),
                                err_msg.clone().into(),
                                queue_id.into(),
                            ],
                        ))
                        .await;
                    mark_delivery_dead(user_id, &activity_type, &target_domain, &err_msg).await;
                    stats.dead += 1;
                } else {
                    let backoff_secs = std::cmp::min(2i64.pow(new_attempts as u32), 86400);
                    let _ = db
                        .execute(Statement::from_sql_and_values(
                            DatabaseBackend::Postgres,
                            "UPDATE federation_delivery_queue SET status = 'pending', attempts = $1, error_message = $2, last_attempt_at = NOW(), next_retry_at = NOW() + make_interval(secs => $4::double precision) WHERE id = $3",
                            [
                                new_attempts.into(),
                                err_msg.into(),
                                queue_id.into(),
                                backoff_secs.into(),
                            ],
                        ))
                        .await;
                    stats.retried += 1;
                }
            }
        }
    }

    Ok(stats)
}

async fn mark_delivery_dead(
    user_id: i32,
    activity_type: &str,
    target_domain: &str,
    error: &str,
) {
    crate::federation::notify::notify_delivery_failed(
        user_id,
        activity_type,
        target_domain,
        error,
    )
    .await;
    // Also log at error level for operator dashboards / log aggregators
    tracing::error!(
        user_id = user_id,
        activity_type = activity_type,
        target_domain = target_domain,
        error = %error,
        "federation delivery dead letter"
    );
}

/// 启动投递队列后台循环
pub fn spawn_delivery_worker(db: DatabaseConnection) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(15));
        loop {
            interval.tick().await;
            match process_delivery_queue_detailed(&db, 20).await {
                Ok(s) if s.delivered > 0 || s.dead > 0 || s.retried > 0 => {
                    tracing::info!(
                        "📤 Delivery worker: delivered={} dead={} retried={}",
                        s.delivered,
                        s.dead,
                        s.retried
                    );
                }
                Err(e) => {
                    tracing::error!("Delivery worker error: {}", e);
                }
                _ => {} // 无待投递项，静默
            }
        }
    });
}

// ==================== Query API (user observability) ====================

/// Per-user delivery queue summary.
pub async fn delivery_stats_for_user(
    db: &DatabaseConnection,
    user_id: i32,
) -> Result<serde_json::Value, String> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT dq.status, COUNT(*)::int AS cnt
               FROM federation_delivery_queue dq
               JOIN federation_activities a ON a.id = dq.activity_id
               WHERE a.user_id = $1
               GROUP BY dq.status"#,
            [user_id.into()],
        ))
        .await
        .map_err(|e| e.to_string())?;

    let mut pending = 0i32;
    let mut delivering = 0i32;
    let mut delivered = 0i32;
    let mut dead = 0i32;
    for r in rows {
        let status: String = r.try_get("", "status").unwrap_or_default();
        let cnt: i32 = r.try_get("", "cnt").unwrap_or(0);
        match status.as_str() {
            "pending" => pending = cnt,
            "delivering" => delivering = cnt,
            "delivered" => delivered = cnt,
            "dead" => dead = cnt,
            _ => {}
        }
    }
    Ok(json!({
        "pending": pending,
        "delivering": delivering,
        "delivered": delivered,
        "dead": dead,
        "active": pending + delivering,
        "failed": dead,
    }))
}

/// Recent delivery queue rows for the current user (failed first).
pub async fn list_delivery_for_user(
    db: &DatabaseConnection,
    user_id: i32,
    limit: i64,
) -> Result<serde_json::Value, String> {
    let limit = limit.clamp(1, 100);
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT dq.id, dq.status, dq.target_domain, dq.target_inbox,
                      dq.attempts, dq.max_attempts, dq.error_message,
                      dq.created_at, dq.last_attempt_at, dq.next_retry_at,
                      a.activity_type, a.activity_id AS ap_id
               FROM federation_delivery_queue dq
               JOIN federation_activities a ON a.id = dq.activity_id
               WHERE a.user_id = $1
               ORDER BY
                 CASE dq.status
                   WHEN 'dead' THEN 0
                   WHEN 'delivering' THEN 1
                   WHEN 'pending' THEN 2
                   ELSE 3
                 END,
                 dq.created_at DESC
               LIMIT $2"#,
            [user_id.into(), limit.into()],
        ))
        .await
        .map_err(|e| e.to_string())?;

    let mut items = Vec::with_capacity(rows.len());
    for r in rows {
        items.push(json!({
            "id": r.try_get::<i32>("", "id").unwrap_or(0),
            "status": r.try_get::<String>("", "status").unwrap_or_default(),
            "target_domain": r.try_get::<String>("", "target_domain").unwrap_or_default(),
            "target_inbox": r.try_get::<String>("", "target_inbox").unwrap_or_default(),
            "attempts": r.try_get::<i32>("", "attempts").unwrap_or(0),
            "max_attempts": r.try_get::<i32>("", "max_attempts").unwrap_or(12),
            "error_message": r.try_get::<Option<String>>("", "error_message").unwrap_or(None),
            "activity_type": r.try_get::<String>("", "activity_type").unwrap_or_default(),
            "activity_id": r.try_get::<String>("", "ap_id").unwrap_or_default(),
            "created_at": r.try_get::<chrono::DateTime<chrono::FixedOffset>>("", "created_at")
                .map(|t| t.to_rfc3339()).unwrap_or_default(),
            "last_attempt_at": r.try_get::<Option<chrono::DateTime<chrono::FixedOffset>>>("", "last_attempt_at")
                .ok().flatten().map(|t| t.to_rfc3339()),
            "next_retry_at": r.try_get::<Option<chrono::DateTime<chrono::FixedOffset>>>("", "next_retry_at")
                .ok().flatten().map(|t| t.to_rfc3339()),
        }));
    }
    Ok(json!({ "items": items, "total": items.len() }))
}

/// Re-queue a single dead (or stuck) delivery item owned by the user.
pub async fn retry_delivery_item(
    db: &DatabaseConnection,
    user_id: i32,
    queue_id: i32,
) -> Result<serde_json::Value, (axum::http::StatusCode, serde_json::Value)> {
    use axum::http::StatusCode;

    if queue_id <= 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            json!({"error": "Invalid delivery id"}),
        ));
    }

    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT dq.id, dq.status
               FROM federation_delivery_queue dq
               JOIN federation_activities a ON a.id = dq.activity_id
               WHERE dq.id = $1 AND a.user_id = $2"#,
            [queue_id.into(), user_id.into()],
        ))
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({"error": format!("DB error: {e}")}),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                json!({"error": "Delivery item not found"}),
            )
        })?;

    let status: String = row.try_get("", "status").unwrap_or_default();
    if status != "dead" && status != "pending" {
        // Allow re-queue of dead; pending is already waiting. Reject delivering/delivered.
        if status == "delivered" {
            return Err((
                StatusCode::BAD_REQUEST,
                json!({"error": "Already delivered"}),
            ));
        }
        if status == "delivering" {
            return Err((
                StatusCode::CONFLICT,
                json!({"error": "Delivery currently in progress"}),
            ));
        }
    }

    let result = db
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"UPDATE federation_delivery_queue
               SET status = 'pending',
                   attempts = 0,
                   error_message = NULL,
                   next_retry_at = NOW(),
                   last_attempt_at = NULL
               WHERE id = $1"#,
            [queue_id.into()],
        ))
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({"error": format!("DB error: {e}")}),
            )
        })?;

    if result.rows_affected() == 0 {
        return Err((
            StatusCode::NOT_FOUND,
            json!({"error": "Delivery item not found"}),
        ));
    }

    Ok(json!({
        "success": true,
        "id": queue_id,
        "status": "pending",
        "previous_status": status
    }))
}

/// Cancel a pending/delivering queue row owned by the user (marks `dead`).
pub async fn cancel_delivery_item(
    db: &DatabaseConnection,
    user_id: i32,
    queue_id: i32,
) -> Result<serde_json::Value, (axum::http::StatusCode, serde_json::Value)> {
    use axum::http::StatusCode;

    if queue_id <= 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            json!({"error": "Invalid delivery id"}),
        ));
    }

    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT dq.id, dq.status
               FROM federation_delivery_queue dq
               JOIN federation_activities a ON a.id = dq.activity_id
               WHERE dq.id = $1 AND a.user_id = $2"#,
            [queue_id.into(), user_id.into()],
        ))
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({"error": format!("DB error: {e}")}),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                json!({"error": "Delivery item not found"}),
            )
        })?;

    let status: String = row.try_get("", "status").unwrap_or_default();
    if status == "delivered" {
        return Err((
            StatusCode::BAD_REQUEST,
            json!({"error": "Already delivered"}),
        ));
    }
    if status == "dead" {
        return Ok(json!({
            "success": true,
            "id": queue_id,
            "status": "dead",
            "already": true,
        }));
    }
    // pending / delivering → dead (user cancelled)
    let result = db
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"UPDATE federation_delivery_queue
               SET status = 'dead',
                   error_message = 'cancelled: by user',
                   last_attempt_at = NOW(),
                   next_retry_at = NULL
               WHERE id = $1 AND status IN ('pending', 'delivering')"#,
            [queue_id.into()],
        ))
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({"error": format!("DB error: {e}")}),
            )
        })?;

    if result.rows_affected() == 0 {
        return Err((
            StatusCode::CONFLICT,
            json!({"error": "Could not cancel delivery (status changed)"}),
        ));
    }

    Ok(json!({
        "success": true,
        "id": queue_id,
        "status": "dead",
        "previous_status": status
    }))
}

/// Re-queue all dead delivery items for the user (capped).
pub async fn retry_all_dead_for_user(
    db: &DatabaseConnection,
    user_id: i32,
    limit: i64,
) -> Result<serde_json::Value, (axum::http::StatusCode, serde_json::Value)> {
    use axum::http::StatusCode;

    let limit = limit.clamp(1, 100);
    // Select ids first (ORDER BY + LIMIT), then update — clearer than nested UPDATE.
    let id_rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT dq.id
               FROM federation_delivery_queue dq
               JOIN federation_activities a ON a.id = dq.activity_id
               WHERE a.user_id = $1 AND dq.status = 'dead'
               ORDER BY dq.created_at DESC
               LIMIT $2"#,
            [user_id.into(), limit.into()],
        ))
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({"error": format!("DB error: {e}")}),
            )
        })?;

    let mut retried = 0u64;
    for r in id_rows {
        let Ok(id) = r.try_get::<i32>("", "id") else {
            continue;
        };
        match db
            .execute(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"UPDATE federation_delivery_queue
                   SET status = 'pending',
                       attempts = 0,
                       error_message = NULL,
                       next_retry_at = NOW(),
                       last_attempt_at = NULL
                   WHERE id = $1 AND status = 'dead'"#,
                [id.into()],
            ))
            .await
        {
            Ok(res) => retried += res.rows_affected(),
            Err(e) => {
                tracing::warn!("retry_all_dead item {} failed: {}", id, e);
            }
        }
    }

    Ok(json!({
        "success": true,
        "retried": retried,
        "limit": limit
    }))
}

/// Cancel all pending/delivering delivery items for the user (capped).
/// Marks matching rows `dead` with a user-cancelled error message.
pub async fn cancel_all_pending_for_user(
    db: &DatabaseConnection,
    user_id: i32,
    limit: i64,
) -> Result<serde_json::Value, (axum::http::StatusCode, serde_json::Value)> {
    use axum::http::StatusCode;

    let limit = limit.clamp(1, 200);
    let id_rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT dq.id
               FROM federation_delivery_queue dq
               JOIN federation_activities a ON a.id = dq.activity_id
               WHERE a.user_id = $1 AND dq.status IN ('pending', 'delivering')
               ORDER BY dq.created_at DESC
               LIMIT $2"#,
            [user_id.into(), limit.into()],
        ))
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({"error": format!("DB error: {e}")}),
            )
        })?;

    let mut cancelled = 0u64;
    for r in id_rows {
        let Ok(id) = r.try_get::<i32>("", "id") else {
            continue;
        };
        match db
            .execute(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"UPDATE federation_delivery_queue
                   SET status = 'dead',
                       error_message = 'cancelled: by user',
                       last_attempt_at = NOW(),
                       next_retry_at = NULL
                   WHERE id = $1 AND status IN ('pending', 'delivering')"#,
                [id.into()],
            ))
            .await
        {
            Ok(res) => cancelled += res.rows_affected(),
            Err(e) => {
                tracing::warn!("cancel_all_pending item {} failed: {}", id, e);
            }
        }
    }

    Ok(json!({
        "success": true,
        "cancelled": cancelled,
        "limit": limit
    }))
}

// ==================== 实际投递 ====================

/// 投递 Activity 到目标 inbox
async fn deliver_activity(
    keypair: &KeyPair,
    base_url: &str,
    username: &str,
    target_inbox: &str,
    target_domain: &str,
    body: &[u8],
) -> Result<(), String> {
    // 纵深防御：即使 inbox URL 已入库，投递前仍验证不指向内网
    if is_internal_url(target_inbox) {
        return Err(format!(
            "Refusing to deliver to internal URL: {}",
            target_inbox
        ));
    }

    let kid = key_id(base_url, username);

    // Host 头/签名的 host 必须与 URL 一致（含非默认端口），否则对端验签失败；
    // target_domain（不带端口）仅用于信任策略与实例统计。
    let (path, host_header) = match url::Url::parse(target_inbox) {
        Ok(u) => {
            let path = u.path().to_string();
            let host = match (u.host_str(), u.port()) {
                (Some(h), Some(p)) => format!("{}:{}", h, p),
                (Some(h), None) => h.to_string(),
                _ => target_domain.to_string(),
            };
            (path, host)
        }
        Err(_) => ("/inbox".to_string(), target_domain.to_string()),
    };

    let params = SignatureParams {
        key_id: &kid,
        host: &host_header,
        path: &path,
        method: "POST",
        body: Some(body),
    };

    let signed = sign_request(keypair, &params).map_err(|e| format!("Signing failed: {}", e))?;

    let user_agent = format!("Myriad/{} (+{})", env!("CARGO_PKG_VERSION"), base_url);
    let (target_url, client) = crate::services::outbound_security::build_public_http_client(
        target_inbox,
        Duration::from_secs(30),
        Some(&user_agent),
    )
    .await?;

    let resp = client
        .post(target_url)
        .header("Host", &host_header)
        .header("Date", &signed.date)
        .header("Digest", &signed.digest.unwrap_or_default())
        .header("Signature", &signed.signature)
        .header("Content-Type", AP_CONTENT_TYPE)
        .body(body.to_vec())
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    let status = resp.status();
    if status.is_success() || status.as_u16() == 202 {
        Ok(())
    } else {
        let body_text = resp.text().await.unwrap_or_default();
        let snippet = body_text.chars().take(200).collect::<String>();
        let code = status.as_u16();
        // Permanent client errors: do not burn max_attempts with useless retries.
        // Keep retrying 408/429 (timeout / rate limit) as transient.
        if status.is_client_error() && code != 408 && code != 429 {
            Err(format!("PERMANENT HTTP {}: {}", code, snippet))
        } else {
            Err(format!("HTTP {}: {}", code, snippet))
        }
    }
}

// ==================== 辅助函数 ====================

/// Choose base URL + username for HTTP Signature keyId.
///
/// For `Move`, use the activity's `actor` URL origin so the signature keyId
/// matches the departing actor document (`{old}/users/{u}#main-key`).
pub(crate) fn signing_identity_for_activity(
    activity_type: &str,
    object_json: &serde_json::Value,
    default_base: &str,
    default_username: &str,
) -> (String, String) {
    if activity_type != "Move" {
        return (
            default_base.trim_end_matches('/').to_string(),
            default_username.to_string(),
        );
    }
    let actor = object_json
        .get("actor")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if actor.is_empty() {
        return (
            default_base.trim_end_matches('/').to_string(),
            default_username.to_string(),
        );
    }
    // actor = `{base}/users/{username}`
    if let Some((base, uname)) = actor.rsplit_once("/users/") {
        let uname = uname.trim_end_matches('/');
        if !base.is_empty() && !uname.is_empty() && !uname.contains('/') {
            return (base.trim_end_matches('/').to_string(), uname.to_string());
        }
    }
    (
        default_base.trim_end_matches('/').to_string(),
        default_username.to_string(),
    )
}

/// Cancel pending/delivering queue rows whose activity body mentions `resource_id`
/// (room_id or channel_id). Call when the local resource is closed or deleted so
/// we stop fan-out KeyExchange / messages at a peer that will never accept them.
pub async fn cancel_pending_deliveries_for_resource(
    db: &DatabaseConnection,
    resource_id: &str,
    reason: &str,
) -> u64 {
    if resource_id.is_empty() {
        return 0;
    }
    let needle = format!("%{}%", resource_id);
    let res = db
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"UPDATE federation_delivery_queue dq
               SET status = 'dead',
                   error_message = $2,
                   last_attempt_at = NOW()
               FROM federation_activities a
               WHERE dq.activity_id = a.id
                 AND dq.status IN ('pending', 'delivering')
                 AND (
                   a.object_json::text LIKE $1
                   OR a.activity_id LIKE $1
                 )"#,
            [needle.into(), reason.into()],
        ))
        .await;
    match res {
        Ok(r) => {
            let n = r.rows_affected();
            if n > 0 {
                tracing::info!(
                    resource_id = %resource_id,
                    cancelled = n,
                    "Cancelled pending federation deliveries for removed resource"
                );
            }
            n
        }
        Err(e) => {
            tracing::warn!(
                resource_id = %resource_id,
                error = %e,
                "Failed to cancel pending deliveries"
            );
            0
        }
    }
}

/// Load keypair; if none (or empty), generate once via ensure then reload.
async fn load_user_keypair_ensuring(
    db: &DatabaseConnection,
    user_id: i32,
    username: &str,
) -> Result<KeyPair, String> {
    match load_user_keypair(db, user_id).await {
        Ok(kp) => Ok(kp),
        Err(e) if is_missing_federation_keys_error(&e) => {
            if username.trim().is_empty() {
                tracing::error!(
                    user_id = user_id,
                    error = %e,
                    "No federation keys and username lookup empty; cannot ensure keys"
                );
                return Err(e);
            }
            tracing::warn!(
                user_id = user_id,
                username = %username,
                error = %e,
                "Federation keys missing for outbound delivery; ensuring once"
            );
            match crate::federation::actor::ensure_user_federation_keys(db, user_id, username)
                .await
            {
                Ok(_) => match load_user_keypair(db, user_id).await {
                    Ok(kp) => {
                        tracing::info!(
                            user_id = user_id,
                            username = %username,
                            "Ensured federation keys; retrying delivery sign"
                        );
                        Ok(kp)
                    }
                    Err(reload_e) => {
                        tracing::error!(
                            user_id = user_id,
                            username = %username,
                            error = %reload_e,
                            "Federation keys still missing after ensure"
                        );
                        Err(reload_e)
                    }
                },
                Err(ensure_e) => {
                    tracing::error!(
                        user_id = user_id,
                        username = %username,
                        error = %ensure_e,
                        "Failed to ensure federation keys before delivery"
                    );
                    Err(format!("Key load failed (ensure): {}; original: {}", ensure_e, e))
                }
            }
        }
        Err(e) => Err(e),
    }
}

fn is_missing_federation_keys_error(err: &str) -> bool {
    // Only true absence/empty material — never re-generate on decrypt failure
    // (that would rotate keys if JWT secret or ciphertext is broken).
    err.contains("No federation keys found for user")
}

/// 加载用户的密钥对
async fn load_user_keypair(db: &DatabaseConnection, user_id: i32) -> Result<KeyPair, String> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT public_key_pem, private_key_encrypted FROM federation_keys WHERE user_id = $1",
            [user_id.into()],
        ))
        .await
        .map_err(|e| format!("DB error: {}", e))?
        .ok_or_else(|| "No federation keys found for user".to_string())?;

    let pub_pem: String = row.try_get("", "public_key_pem").unwrap_or_default();
    let encrypted: String = row.try_get("", "private_key_encrypted").unwrap_or_default();

    if pub_pem.trim().is_empty() || encrypted.trim().is_empty() {
        return Err("No federation keys found for user".to_string());
    }

    let jwt_secret = {
        let config = crate::GLOBAL_CONFIG.read().await;
        config.jwt_secret.clone()
    };

    KeyPair::from_encrypted(&pub_pem, &encrypted, &jwt_secret)
        .map_err(|e| format!("Key decryption failed: {}", e))
}

async fn get_username_by_id(db: &DatabaseConnection, user_id: i32) -> Result<String, String> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT username FROM users WHERE id = $1 LIMIT 1",
            [user_id.into()],
        ))
        .await
        .map_err(|e| format!("DB error: {}", e))?
        .ok_or_else(|| "User not found".to_string())?;

    Ok(row.try_get("", "username").unwrap_or_default())
}

async fn get_base_url() -> String {
    crate::federation::types::get_base_url().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn move_signing_uses_old_actor_base() {
        let act = json!({
            "type": "Move",
            "actor": "https://old.example/users/alice",
            "object": "https://old.example/users/alice",
            "target": "https://new.example/users/alice",
        });
        let (base, user) = signing_identity_for_activity(
            "Move",
            &act,
            "https://new.example",
            "alice",
        );
        assert_eq!(base, "https://old.example");
        assert_eq!(user, "alice");
    }

    #[test]
    fn missing_keys_error_is_detected() {
        assert!(is_missing_federation_keys_error(
            "No federation keys found for user"
        ));
        assert!(!is_missing_federation_keys_error(
            "Key decryption failed: AES-GCM decryption failed"
        ));
        assert!(!is_missing_federation_keys_error("DB error: connection refused"));
    }

    #[test]
    fn non_move_signing_uses_default_base() {
        let act = json!({
            "type": "Create",
            "actor": "https://new.example/users/alice",
        });
        let (base, user) =
            signing_identity_for_activity("Create", &act, "https://new.example", "alice");
        assert_eq!(base, "https://new.example");
        assert_eq!(user, "alice");
    }
}
