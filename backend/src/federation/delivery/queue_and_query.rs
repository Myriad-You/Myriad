
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde::Serialize;
use serde_json::json;
use std::time::Duration;

use crate::federation::keys::KeyPair;
use crate::federation::signature::{sign_request, SignatureParams};
use crate::federation::types::*;

pub(crate) async fn get_username_by_id(db: &DatabaseConnection, user_id: i32) -> Result<String, String> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT username FROM users WHERE id = $1 LIMIT 1",
            [user_id.into()],
        ))
        .await
        .map_err(|e| { tracing::error!("DB error: {}", e); "Database error".to_string() })?
        .ok_or_else(|| "User not found".to_string())?;

    Ok(row.try_get("", "username").unwrap_or_default())
}

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
            remote_targets: self.remote_with_inbox
                + self.skipped_empty_inbox
                + self.unresolved_members,
            unresolved: self.unresolved_members + self.skipped_empty_inbox,
            warning: None,
        };
        if self.enqueued == 0 && info.remote_targets > 0 {
            info.warning =
                Some("no_delivery_queued: remote members lack inbox or remote_actors row".into());
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
        .query_all_raw(Statement::from_sql_and_values(
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
            let mark = db
                .execute_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "UPDATE federation_delivery_queue SET status = 'dead', error_message = $1, last_attempt_at = NOW() WHERE id = $2 AND status = 'delivering'",
                    [err.into(), queue_id.into()],
                ))
                .await;
            if mark.as_ref().map(|r| r.rows_affected()).unwrap_or(0) == 0 {
                continue;
            }
            mark_delivery_dead(user_id, &activity_type, &target_domain, err).await;
            stats.dead += 1;
            continue;
        }

        // 投递前：目标实例信任策略检查（黑名单等）
        if let Err(reason) = crate::federation::trust::enforce_outbound(db, &target_domain).await {
            let mark = db
                .execute_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    r#"UPDATE federation_delivery_queue
                       SET status = 'dead', error_message = $1, last_attempt_at = NOW()
                       WHERE id = $2 AND status = 'delivering'"#,
                    [reason.clone().into(), queue_id.into()],
                ))
                .await;
            if mark.as_ref().map(|r| r.rows_affected()).unwrap_or(0) == 0 {
                continue;
            }
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
        let (sign_base, sign_username) =
            signing_identity_for_activity(&activity_type, &object_json, &base_url, &username);

        // 获取用户密钥对 — if missing, ensure once then reload so already-queued
        // deliveries recover without waiting for GET /users/{username}.
        // Prefer stored key_id from federation_keys (domain-move G retargets it);
        // recompute from sign_base only when missing. Move always signs with the
        // activity actor origin so old peers can verify against the departing doc.
        match load_user_keypair_ensuring(db, user_id, &username).await {
            Ok(loaded) => {
                match deliver_activity(
                    &loaded.keypair,
                    &sign_base,
                    &sign_username,
                    &target_inbox,
                    &target_domain,
                    &body_bytes,
                    loaded.stored_key_id.as_deref(),
                    &activity_type,
                )
                .await
                {
                    Ok(()) => {
                        // 投递成功 — only if still `delivering` (user cancel may
                        // have marked dead mid-flight; do not resurrect).
                        let mark = db
                            .execute_raw(Statement::from_sql_and_values(
                                DatabaseBackend::Postgres,
                                "UPDATE federation_delivery_queue SET status = 'delivered', last_attempt_at = NOW() WHERE id = $1 AND status = 'delivering'",
                                [queue_id.into()],
                            ))
                            .await;
                        if mark.as_ref().map(|r| r.rows_affected()).unwrap_or(0) == 0 {
                            tracing::info!(
                                queue_id = queue_id,
                                target = %target_inbox,
                                "Delivery HTTP ok but queue row no longer delivering (cancelled?); not marking delivered"
                            );
                            continue;
                        }
                        stats.delivered += 1;

                        // 更新实例的 last_success_at，重置 failure_count
                        let _ = db
                            .execute_raw(Statement::from_sql_and_values(
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
                            // 放弃 — only if still delivering (preserve user cancel).
                            let mark = db
                                .execute_raw(Statement::from_sql_and_values(
                                    DatabaseBackend::Postgres,
                                    "UPDATE federation_delivery_queue SET status = 'dead', attempts = $1, error_message = $2, last_attempt_at = NOW() WHERE id = $3 AND status = 'delivering'",
                                    [new_attempts.into(), e.clone().into(), queue_id.into()],
                                ))
                                .await;
                            if mark.as_ref().map(|r| r.rows_affected()).unwrap_or(0) == 0 {
                                continue;
                            }
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
                            let backoff_secs = retry_backoff_secs(new_attempts);
                            let mark = db
                                .execute_raw(Statement::from_sql_and_values(
                                    DatabaseBackend::Postgres,
                                    "UPDATE federation_delivery_queue SET status = 'pending', attempts = $1, error_message = $2, last_attempt_at = NOW(), next_retry_at = NOW() + make_interval(secs => $4::double precision) WHERE id = $3 AND status = 'delivering'",
                                    [new_attempts.into(), e.clone().into(), queue_id.into(), backoff_secs.into()],
                                ))
                                .await;
                            if mark.as_ref().map(|r| r.rows_affected()).unwrap_or(0) == 0 {
                                continue;
                            }

                            // 更新实例 failure_count
                            let _ = db
                                .execute_raw(Statement::from_sql_and_values(
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
                let new_attempts = attempts + 1;
                let err_msg = format!("Key load failed: {}", e);
                // Empty username cannot ensure — mark dead instead of spinning
                // retries. Decrypt failures back off (no re-generate) until max.
                let permanent = is_unrecoverable_key_load_error(&e);
                if permanent || new_attempts >= max_attempts {
                    let mark = db
                        .execute_raw(Statement::from_sql_and_values(
                            DatabaseBackend::Postgres,
                            "UPDATE federation_delivery_queue SET status = 'dead', attempts = $1, error_message = $2, last_attempt_at = NOW() WHERE id = $3 AND status = 'delivering'",
                            [
                                new_attempts.into(),
                                err_msg.clone().into(),
                                queue_id.into(),
                            ],
                        ))
                        .await;
                    if mark.as_ref().map(|r| r.rows_affected()).unwrap_or(0) == 0 {
                        continue;
                    }
                    if permanent {
                        tracing::error!(
                            user_id = user_id,
                            queue_id = queue_id,
                            error = %err_msg,
                            "federation key load permanent failure; marking dead"
                        );
                    }
                    mark_delivery_dead(user_id, &activity_type, &target_domain, &err_msg).await;
                    stats.dead += 1;
                } else {
                    // 密钥问题几乎不会自愈；按普通失败计数退避，避免 15s 热循环刷日志
                    let backoff_secs = retry_backoff_secs(new_attempts);
                    let mark = db
                        .execute_raw(Statement::from_sql_and_values(
                            DatabaseBackend::Postgres,
                            "UPDATE federation_delivery_queue SET status = 'pending', attempts = $1, error_message = $2, last_attempt_at = NOW(), next_retry_at = NOW() + make_interval(secs => $4::double precision) WHERE id = $3 AND status = 'delivering'",
                            [
                                new_attempts.into(),
                                err_msg.into(),
                                queue_id.into(),
                                backoff_secs.into(),
                            ],
                        ))
                        .await;
                    if mark.as_ref().map(|r| r.rows_affected()).unwrap_or(0) == 0 {
                        continue;
                    }
                    stats.retried += 1;
                }
            }
        }
    }

    Ok(stats)
}

async fn mark_delivery_dead(user_id: i32, activity_type: &str, target_domain: &str, error: &str) {
    crate::federation::notify::notify_delivery_failed(user_id, activity_type, target_domain, error)
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
        // Lab dual-instance: `MYRIAD_FEDERATION_DELIVERY_INTERVAL_SECS` (e.g. 2)
        // speeds full-chain harness without changing production default (15s).
        let secs = std::env::var("MYRIAD_FEDERATION_DELIVERY_INTERVAL_SECS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|n| *n >= 1 && *n <= 3600)
            .unwrap_or(15);
        let mut interval = tokio::time::interval(Duration::from_secs(secs));
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

// Query API (user observability)

/// Per-user delivery queue summary.
pub async fn delivery_stats_for_user(
    db: &DatabaseConnection,
    user_id: i32,
) -> Result<serde_json::Value, String> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
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
///
/// Optional `status_filter` (pending|delivering|delivered|dead) scopes the
/// query so the "delivered" tab is not empty when the default priority order
/// fills the page with dead/pending only.
pub async fn list_delivery_for_user(
    db: &DatabaseConnection,
    user_id: i32,
    limit: i64,
) -> Result<serde_json::Value, String> {
    list_delivery_for_user_filtered(db, user_id, limit, None).await
}

pub async fn list_delivery_for_user_filtered(
    db: &DatabaseConnection,
    user_id: i32,
    limit: i64,
    status_filter: Option<&str>,
) -> Result<serde_json::Value, String> {
    let limit = limit.clamp(1, 100);
    let status = status_filter
        .map(str::trim)
        .filter(|s| {
            matches!(
                *s,
                "pending" | "delivering" | "delivered" | "dead" | "failed"
            )
        });

    let rows = if let Some(st) = status {
        // "dead" UI tab also includes soft-failed rows
        if st == "dead" {
            db.query_all_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"SELECT dq.id, dq.status, dq.target_domain, dq.target_inbox,
                          dq.attempts, dq.max_attempts, dq.error_message,
                          dq.created_at, dq.last_attempt_at, dq.next_retry_at,
                          a.activity_type, a.activity_id AS ap_id
                   FROM federation_delivery_queue dq
                   JOIN federation_activities a ON a.id = dq.activity_id
                   WHERE a.user_id = $1
                     AND dq.status IN ('dead', 'failed')
                   ORDER BY dq.created_at DESC
                   LIMIT $2"#,
                [user_id.into(), limit.into()],
            ))
            .await
            .map_err(|e| e.to_string())?
        } else {
            db.query_all_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"SELECT dq.id, dq.status, dq.target_domain, dq.target_inbox,
                          dq.attempts, dq.max_attempts, dq.error_message,
                          dq.created_at, dq.last_attempt_at, dq.next_retry_at,
                          a.activity_type, a.activity_id AS ap_id
                   FROM federation_delivery_queue dq
                   JOIN federation_activities a ON a.id = dq.activity_id
                   WHERE a.user_id = $1 AND dq.status = $2
                   ORDER BY dq.created_at DESC
                   LIMIT $3"#,
                [user_id.into(), st.into(), limit.into()],
            ))
            .await
            .map_err(|e| e.to_string())?
        }
    } else {
        db.query_all_raw(Statement::from_sql_and_values(
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
        .map_err(|e| e.to_string())?
    };

    let mut items = Vec::with_capacity(rows.len());
    for r in rows {
        let status = r.try_get::<String>("", "status").unwrap_or_default();
        let error_message = r
            .try_get::<Option<String>>("", "error_message")
            .unwrap_or(None);
        let activity_type = r.try_get::<String>("", "activity_type").unwrap_or_default();
        let intentional_cancel = is_intentional_cancel_delivery_error(error_message.as_deref());
        // pending/failed always offer retry; dead only when not intentional cancel
        let retryable = match status.as_str() {
            "pending" | "failed" => true,
            "dead" => should_offer_retry_for_dead_error(error_message.as_deref()),
            _ => false,
        };
        items.push(json!({
            "id": r.try_get::<i32>("", "id").unwrap_or(0),
            "status": status,
            "target_domain": r.try_get::<String>("", "target_domain").unwrap_or_default(),
            "target_inbox": r.try_get::<String>("", "target_inbox").unwrap_or_default(),
            "attempts": r.try_get::<i32>("", "attempts").unwrap_or(0),
            "max_attempts": r.try_get::<i32>("", "max_attempts").unwrap_or(12),
            "error_message": error_message,
            "activity_type": activity_type.clone(),
            "activity_id": r.try_get::<String>("", "ap_id").unwrap_or_default(),
            "created_at": r.try_get::<chrono::DateTime<chrono::FixedOffset>>("", "created_at")
                .map(|t| t.to_rfc3339()).unwrap_or_default(),
            "last_attempt_at": r.try_get::<Option<chrono::DateTime<chrono::FixedOffset>>>("", "last_attempt_at")
                .ok().flatten().map(|t| t.to_rfc3339()),
            "next_retry_at": r.try_get::<Option<chrono::DateTime<chrono::FixedOffset>>>("", "next_retry_at")
                .ok().flatten().map(|t| t.to_rfc3339()),
            // UI helpers (host + Aro) — avoid re-implementing cancel prefix rules
            "intentional_cancel": intentional_cancel,
            "retryable": retryable,
            "is_teardown_activity": is_resource_teardown_activity_type(&activity_type),
        }));
    }
    Ok(json!({ "items": items, "total": items.len() }))
}

/// Status gate for retry (ownership is enforced separately via user_id JOIN).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RetryStatusDecision {
    Allow,
    AlreadyDelivered,
    InProgress,
}

/// Pure decision: which queue statuses may be re-queued.
pub(crate) fn classify_retry_status(status: &str) -> RetryStatusDecision {
    match status {
        "dead" | "pending" => RetryStatusDecision::Allow,
        "delivered" => RetryStatusDecision::AlreadyDelivered,
        "delivering" => RetryStatusDecision::InProgress,
        // Unknown / cancelled variants: allow re-queue only if previously dead-like
        other if other == "failed" || other == "cancelled" => RetryStatusDecision::Allow,
        _ => RetryStatusDecision::Allow,
    }
}

/// Status gate for cancel (ownership via user_id JOIN).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CancelStatusDecision {
    /// pending/delivering → mark dead
    Cancel,
    /// already dead → idempotent success
    AlreadyDead,
    /// delivered → reject
    AlreadyDelivered,
}

pub(crate) fn classify_cancel_status(status: &str) -> CancelStatusDecision {
    match status {
        "delivered" => CancelStatusDecision::AlreadyDelivered,
        "dead" => CancelStatusDecision::AlreadyDead,
        "pending" | "delivering" => CancelStatusDecision::Cancel,
        _ => CancelStatusDecision::Cancel,
    }
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

    // Ownership: only rows whose activity belongs to this user.
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT dq.id, dq.status, dq.error_message
               FROM federation_delivery_queue dq
               JOIN federation_activities a ON a.id = dq.activity_id
               WHERE dq.id = $1 AND a.user_id = $2"#,
            [queue_id.into(), user_id.into()],
        ))
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                { tracing::error!("DB error: {e}"); json!({"error": "Database error"}) },
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                json!({"error": "Delivery item not found"}),
            )
        })?;

    let status: String = row.try_get("", "status").unwrap_or_default();
    let prev_error: Option<String> = row.try_get("", "error_message").ok().flatten();
    // Explicit single-id retry may revive a user-cancelled row (unlike bulk
    // retry-all-dead). Surface that so clients can warn.
    let revived_cancelled = is_user_cancelled_delivery_error(prev_error.as_deref());
    match classify_retry_status(&status) {
        RetryStatusDecision::AlreadyDelivered => {
            return Err((
                StatusCode::BAD_REQUEST,
                json!({"error": "Already delivered"}),
            ));
        }
        RetryStatusDecision::InProgress => {
            return Err((
                StatusCode::CONFLICT,
                json!({"error": "Delivery currently in progress"}),
            ));
        }
        RetryStatusDecision::Allow => {}
    }

    // Re-check ownership + retriable status atomically. Without this, a race
    // with the delivery worker completing can flip `delivered` back to pending
    // and re-send already-accepted activities.
    let result = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"UPDATE federation_delivery_queue dq
               SET status = 'pending',
                   attempts = 0,
                   error_message = NULL,
                   next_retry_at = NOW(),
                   last_attempt_at = NULL
               FROM federation_activities a
               WHERE dq.id = $1
                 AND a.id = dq.activity_id
                 AND a.user_id = $2
                 AND dq.status IN ('dead', 'pending', 'failed', 'cancelled')"#,
            [queue_id.into(), user_id.into()],
        ))
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                { tracing::error!("DB error: {e}"); json!({"error": "Database error"}) },
            )
        })?;

    if result.rows_affected() == 0 {
        return Err((
            StatusCode::CONFLICT,
            json!({"error": "Could not retry delivery (status changed)"}),
        ));
    }

    Ok(json!({
        "success": true,
        "id": queue_id,
        "status": "pending",
        "previous_status": status,
        "revived_cancelled": revived_cancelled,
    }))
}

/// Cancel a pending/delivering queue row owned by the user (marks `dead`).
///
/// Idempotent for already-dead rows (`already: true`). Ownership enforced by
/// joining `federation_activities.user_id` — other users get 404.
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
        .query_one_raw(Statement::from_sql_and_values(
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
                { tracing::error!("DB error: {e}"); json!({"error": "Database error"}) },
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                json!({"error": "Delivery item not found"}),
            )
        })?;

    let status: String = row.try_get("", "status").unwrap_or_default();
    match classify_cancel_status(&status) {
        CancelStatusDecision::AlreadyDelivered => {
            return Err((
                StatusCode::BAD_REQUEST,
                json!({"error": "Already delivered"}),
            ));
        }
        CancelStatusDecision::AlreadyDead => {
            return Ok(json!({
                "success": true,
                "id": queue_id,
                "status": "dead",
                "already": true,
            }));
        }
        CancelStatusDecision::Cancel => {}
    }
    // pending / delivering → dead (user cancelled). Re-assert ownership so a
    // concurrent ownership edge cannot cancel another user's row by id alone.
    let result = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"UPDATE federation_delivery_queue dq
               SET status = 'dead',
                   error_message = 'cancelled: by user',
                   last_attempt_at = NOW(),
                   next_retry_at = NULL
               FROM federation_activities a
               WHERE dq.id = $1
                 AND a.id = dq.activity_id
                 AND a.user_id = $2
                 AND dq.status IN ('pending', 'delivering')"#,
            [queue_id.into(), user_id.into()],
        ))
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                { tracing::error!("DB error: {e}"); json!({"error": "Database error"}) },
            )
        })?;

    if result.rows_affected() == 0 {
        // Race: worker may have finished (delivered/dead) between SELECT and UPDATE.
        // Re-read so we never leave the client with a bare 409 when cancel "won"
        // as a terminal dead row (including non-cancelled permanent fail).
        let again = db
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"SELECT dq.status, dq.error_message
                   FROM federation_delivery_queue dq
                   JOIN federation_activities a ON a.id = dq.activity_id
                   WHERE dq.id = $1 AND a.user_id = $2"#,
                [queue_id.into(), user_id.into()],
            ))
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    { tracing::error!("DB error: {e}"); json!({"error": "Database error"}) },
                )
            })?;
        let Some(again) = again else {
            return Err((
                StatusCode::NOT_FOUND,
                json!({"error": "Delivery item not found"}),
            ));
        };
        let st: String = again.try_get("", "status").unwrap_or_default();
        let err_msg: Option<String> = again.try_get("", "error_message").ok().flatten();
        match st.as_str() {
            "dead" => {
                return Ok(json!({
                    "success": true,
                    "id": queue_id,
                    "status": "dead",
                    "already": true,
                    "user_cancelled": is_user_cancelled_delivery_error(err_msg.as_deref()),
                    "previous_status": status,
                }));
            }
            "delivered" => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    json!({"error": "Already delivered"}),
                ));
            }
            _ => {
                return Err((
                    StatusCode::CONFLICT,
                    json!({"error": "Could not cancel delivery (status changed)", "status": st}),
                ));
            }
        }
    }

    Ok(json!({
        "success": true,
        "id": queue_id,
        "status": "dead",
        "previous_status": status
    }))
}

/// True when a dead-letter row was intentionally cancelled by the user/API.
///
/// Bulk `retry-dead` must not requeue these (would surprise the user and
/// re-fire deliberately stopped outbound). Per-item retry still allows them
/// so an explicit click on a cancelled row can recover it.
///
/// Matches production cancel markers: exact `cancelled: by user` and any
/// `cancelled:…` prefix used by resource teardown (room dissolve, channel close).
pub(crate) fn is_user_cancelled_delivery_error(error_message: Option<&str>) -> bool {
    error_message
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .is_some_and(|s| {
            s.eq_ignore_ascii_case("cancelled: by user")
                || s.to_ascii_lowercase().starts_with("cancelled:")
        })
}

/// Re-queue all dead delivery items for the user (capped).
///
/// Skips rows whose `error_message` indicates user cancel (`cancelled:…`).
/// Transient / peer failures and suite-seeded dead letters still requeue.
pub async fn retry_all_dead_for_user(
    db: &DatabaseConnection,
    user_id: i32,
    limit: i64,
) -> Result<serde_json::Value, (axum::http::StatusCode, serde_json::Value)> {
    use axum::http::StatusCode;

    let limit = limit.clamp(1, 100);
    // Over-fetch so skipping cancelled rows still fills the limit.
    let select_cap = (limit * 3).clamp(1, 300);
    let id_rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT dq.id, dq.error_message
               FROM federation_delivery_queue dq
               JOIN federation_activities a ON a.id = dq.activity_id
               WHERE a.user_id = $1 AND dq.status = 'dead'
               ORDER BY dq.created_at DESC
               LIMIT $2"#,
            [user_id.into(), select_cap.into()],
        ))
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                { tracing::error!("DB error: {e}"); json!({"error": "Database error"}) },
            )
        })?;

    let mut retried = 0u64;
    let mut skipped_cancelled = 0u64;
    for r in id_rows {
        if retried >= limit as u64 {
            break;
        }
        let Ok(id) = r.try_get::<i32>("", "id") else {
            continue;
        };
        // Nullable column: prefer Option, fall back to String (driver variance).
        let err_msg = r
            .try_get::<Option<String>>("", "error_message")
            .ok()
            .flatten()
            .or_else(|| r.try_get::<String>("", "error_message").ok());
        if is_user_cancelled_delivery_error(err_msg.as_deref()) {
            skipped_cancelled += 1;
            continue;
        }
        // Defense-in-depth: SQL re-asserts no `cancelled:` prefix so a race that
        // wrote user-cancel after SELECT still cannot be bulk-retried.
        match db
            .execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"UPDATE federation_delivery_queue
                   SET status = 'pending',
                       attempts = 0,
                       error_message = NULL,
                       next_retry_at = NOW(),
                       last_attempt_at = NULL
                   WHERE id = $1
                     AND status = 'dead'
                     AND (
                       error_message IS NULL
                       OR TRIM(error_message) = ''
                       OR error_message NOT ILIKE 'cancelled:%'
                     )"#,
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
        "skipped_cancelled": skipped_cancelled,
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
        .query_all_raw(Statement::from_sql_and_values(
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
                { tracing::error!("DB error: {e}"); json!({"error": "Database error"}) },
            )
        })?;

    let mut cancelled = 0u64;
    for r in id_rows {
        let Ok(id) = r.try_get::<i32>("", "id") else {
            continue;
        };
        match db
            .execute_raw(Statement::from_sql_and_values(
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

// 实际投递

/// 投递 Activity 到目标 inbox
///
/// `stored_key_id`: canonical HTTP Signature keyId from `federation_keys` when
/// present (survives domain-move G retarget). Fallback: recompute from base_url.
/// For `Move`, always recompute from `base_url` (the **old** actor origin) so
/// peers verifying against the departing actor document succeed.
#[allow(clippy::too_many_arguments)]
async fn deliver_activity(
    keypair: &KeyPair,
    base_url: &str,
    username: &str,
    target_inbox: &str,
    target_domain: &str,
    body: &[u8],
    stored_key_id: Option<&str>,
    activity_type: &str,
) -> Result<(), String> {
    // 纵深防御：即使 inbox URL 已入库，投递前仍验证不指向内网
    if is_internal_url(target_inbox) {
        return Err(format!(
            "Refusing to deliver to internal URL: {}",
            target_inbox
        ));
    }

    let kid = resolve_signing_key_id(activity_type, base_url, username, stored_key_id);

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
        // 只读错误正文的前若干字节。`resp.text()` 会把整个响应缓冲进内存，
        // 而这条路径上的对端是任意联邦实例 —— 一个恶意或故障的远端可以对每次
        // 投递失败回一个巨大的 body，把内存打爆。我们只需要够写日志的一小段。
        const MAX_ERROR_BODY: usize = 8 * 1024;
        let body_text = crate::services::outbound_security::read_limited_body(resp, MAX_ERROR_BODY)
            .await
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_default();
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

/// 重试退避：指数增长 + 抖动，上限 24 小时。
///
/// 没有抖动时，同一个远端实例宕机期间积压的**所有**投递会算出完全相同的
/// `next_retry_at`，于是每一轮都整齐地同时打过去 —— 对方刚恢复就被我们自己
/// 制造的尖峰再打一次。抖动把它们摊开。
///
/// 抖动取 ±25%：足以打散同批，又不会让退避语义走形。
pub(crate) fn retry_backoff_secs(attempts: i32) -> i64 {
    let base = 2i64
        .saturating_pow(attempts.clamp(0, 32) as u32)
        .min(86_400);
    // 低位退避（1~2 秒）加抖动没有意义，反而可能算出 0
    if base <= 2 {
        return base;
    }
    let spread = base / 4;
    let jitter = (rand::random::<u64>() % (2 * spread as u64 + 1)) as i64 - spread;
    (base + jitter).clamp(1, 86_400)
}

// 辅助函数

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

/// Activity types that intentionally fan out resource teardown.
///
/// These must **not** be cancelled by [`cancel_pending_deliveries_for_resource`]:
/// cancel-after-enqueue of RoomDissolve / ChannelClose would dead-letter the
/// dissolve/close itself and remotes would never learn the resource is gone.
///
/// Accepts DB-stored short names (`RoomDissolve`, `ChannelClose`) and JSON
/// `type` forms (`myriad:RoomDissolve`, `myriad:ChannelClose`).
pub(crate) fn is_resource_teardown_activity_type(activity_type: &str) -> bool {
    let t = activity_type.trim();
    if t.is_empty() {
        return false;
    }
    let bare = t
        .strip_prefix("myriad:")
        .or_else(|| t.strip_prefix("Myriad:"))
        .unwrap_or(t);
    bare.eq_ignore_ascii_case("RoomDissolve") || bare.eq_ignore_ascii_case("ChannelClose")
}

/// Whether a dead-letter error is an intentional local/API cancel (not a peer fail).
///
/// Used by UI classification (no Retry / show Cancelled) and bulk skip paths.
/// Same prefix rules as [`is_user_cancelled_delivery_error`].
pub(crate) fn is_intentional_cancel_delivery_error(error_message: Option<&str>) -> bool {
    is_user_cancelled_delivery_error(error_message)
}

/// Whether the host/UI should offer Retry for a dead row with this error.
/// Intentional cancels (`cancelled:…`) must not look retriable.
pub(crate) fn should_offer_retry_for_dead_error(error_message: Option<&str>) -> bool {
    !is_intentional_cancel_delivery_error(error_message)
}

/// Cancel pending/delivering queue rows whose activity body mentions `resource_id`
/// (room_id or channel_id). Call when the local resource is closed or deleted so
/// we stop fan-out KeyExchange / messages at a peer that will never accept them.
///
/// **Does not cancel** resource-teardown fan-outs ([`is_resource_teardown_activity_type`]):
/// `RoomDissolve` / `ChannelClose` (and `myriad:` variants). Callers should prefer
/// cancel-stale-**then** enqueue teardown so order is safe even without this filter.
pub async fn cancel_pending_deliveries_for_resource(
    db: &DatabaseConnection,
    resource_id: &str,
    reason: &str,
) -> u64 {
    if resource_id.is_empty() {
        return 0;
    }
    let needle = format!("%{}%", resource_id);
    // Exclude teardown activity types so dissolve/close fan-out rows survive.
    // Normalize `myriad:` prefix the same way as is_resource_teardown_activity_type.
    let res = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"UPDATE federation_delivery_queue dq
               SET status = 'dead',
                   error_message = $2,
                   last_attempt_at = NOW(),
                   next_retry_at = NULL
               FROM federation_activities a
               WHERE dq.activity_id = a.id
                 AND dq.status IN ('pending', 'delivering')
                 AND (
                   a.object_json::text LIKE $1
                   OR a.activity_id LIKE $1
                 )
                 AND LOWER(
                   CASE
                     WHEN LOWER(a.activity_type) LIKE 'myriad:%'
                       THEN SUBSTRING(a.activity_type FROM 8)
                     ELSE a.activity_type
                   END
                 ) NOT IN ('roomdissolve', 'channelclose')"#,
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

/// Delete a single **dead** delivery queue row owned by the user.
///
/// Used to dismiss intentional cancels and other dead clutter from the UI.
/// Pending/delivering/delivered rows are rejected — cancel or wait first.
pub async fn dismiss_delivery_item(
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
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT dq.id, dq.status, dq.error_message
               FROM federation_delivery_queue dq
               JOIN federation_activities a ON a.id = dq.activity_id
               WHERE dq.id = $1 AND a.user_id = $2"#,
            [queue_id.into(), user_id.into()],
        ))
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                { tracing::error!("DB error: {e}"); json!({"error": "Database error"}) },
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                json!({"error": "Delivery item not found"}),
            )
        })?;

    let status: String = row.try_get("", "status").unwrap_or_default();
    let prev_error: Option<String> = row.try_get("", "error_message").ok().flatten();
    if status != "dead" {
        return Err((
            StatusCode::BAD_REQUEST,
            json!({
                "error": "Only dead delivery items can be dismissed",
                "status": status,
            }),
        ));
    }

    let result = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"DELETE FROM federation_delivery_queue dq
               USING federation_activities a
               WHERE dq.id = $1
                 AND a.id = dq.activity_id
                 AND a.user_id = $2
                 AND dq.status = 'dead'"#,
            [queue_id.into(), user_id.into()],
        ))
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                { tracing::error!("DB error: {e}"); json!({"error": "Database error"}) },
            )
        })?;

    if result.rows_affected() == 0 {
        return Err((
            StatusCode::CONFLICT,
            json!({"error": "Could not dismiss delivery (status changed)"}),
        ));
    }

    Ok(json!({
        "success": true,
        "id": queue_id,
        "dismissed": true,
        "previous_status": status,
        "was_cancelled": is_user_cancelled_delivery_error(prev_error.as_deref()),
    }))
}

/// Bulk-delete dead delivery rows for the user (capped).
///
/// When `cancelled_only` is true, only rows whose `error_message` matches
/// `cancelled:%` are removed (intentional cancels / resource teardown leftovers).
pub async fn purge_dead_for_user(
    db: &DatabaseConnection,
    user_id: i32,
    limit: i64,
    cancelled_only: bool,
) -> Result<serde_json::Value, (axum::http::StatusCode, serde_json::Value)> {
    use axum::http::StatusCode;

    let limit = limit.clamp(1, 200);
    let sql = if cancelled_only {
        r#"DELETE FROM federation_delivery_queue dq
           USING federation_activities a
           WHERE a.id = dq.activity_id
             AND a.user_id = $1
             AND dq.status = 'dead'
             AND dq.error_message IS NOT NULL
             AND TRIM(dq.error_message) <> ''
             AND dq.error_message ILIKE 'cancelled:%'
             AND dq.id IN (
               SELECT dq2.id
               FROM federation_delivery_queue dq2
               JOIN federation_activities a2 ON a2.id = dq2.activity_id
               WHERE a2.user_id = $1
                 AND dq2.status = 'dead'
                 AND dq2.error_message IS NOT NULL
                 AND TRIM(dq2.error_message) <> ''
                 AND dq2.error_message ILIKE 'cancelled:%'
               ORDER BY dq2.created_at DESC
               LIMIT $2
             )"#
    } else {
        r#"DELETE FROM federation_delivery_queue dq
           USING federation_activities a
           WHERE a.id = dq.activity_id
             AND a.user_id = $1
             AND dq.status = 'dead'
             AND dq.id IN (
               SELECT dq2.id
               FROM federation_delivery_queue dq2
               JOIN federation_activities a2 ON a2.id = dq2.activity_id
               WHERE a2.user_id = $1 AND dq2.status = 'dead'
               ORDER BY dq2.created_at DESC
               LIMIT $2
             )"#
    };

    let result = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            sql,
            [user_id.into(), limit.into()],
        ))
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                { tracing::error!("DB error: {e}"); json!({"error": "Database error"}) },
            )
        })?;

    Ok(json!({
        "success": true,
        "purged": result.rows_affected(),
        "limit": limit,
        "cancelled_only": cancelled_only,
    }))
}

/// Loaded signing material + optional canonical keyId from storage.
struct LoadedSigningKey {
    keypair: KeyPair,
    /// From `federation_keys.key_id` when non-empty (post domain-move G).
    stored_key_id: Option<String>,
}

/// Pick HTTP Signature keyId for outbound delivery.
///
/// - **Move**: always `key_id(sign_base, username)` (old actor origin).
/// - **Else**: prefer non-empty stored keyId; else recompute from sign_base.
pub(crate) fn resolve_signing_key_id(
    activity_type: &str,
    sign_base: &str,
    username: &str,
    stored_key_id: Option<&str>,
) -> String {
    if activity_type == "Move" {
        return key_id(sign_base, username);
    }
    if let Some(kid) = stored_key_id.map(str::trim).filter(|s| !s.is_empty()) {
        return kid.to_string();
    }
    key_id(sign_base, username)
}

/// Load keypair; if none (or empty), generate once via ensure then reload.
///
/// Universal choke point for all `federation_delivery_queue` producers.
async fn load_user_keypair_ensuring(
    db: &DatabaseConnection,
    user_id: i32,
    username: &str,
) -> Result<LoadedSigningKey, String> {
    match load_user_keypair(db, user_id).await {
        Ok(loaded) => Ok(loaded),
        Err(e) if is_missing_federation_keys_error(&e) => {
            if username.trim().is_empty() {
                tracing::error!(
                    user_id = user_id,
                    error = %e,
                    "No federation keys and username lookup empty; cannot ensure keys"
                );
                return Err(
                    "No federation keys found for user and username empty (cannot ensure)"
                        .to_string(),
                );
            }
            tracing::warn!(
                user_id = user_id,
                username = %username,
                error = %e,
                "Federation keys missing for outbound delivery; ensuring once"
            );
            match crate::federation::actor::ensure_user_federation_keys(db, user_id, username).await
            {
                Ok(_) => match load_user_keypair(db, user_id).await {
                    Ok(loaded) => {
                        tracing::info!(
                            user_id = user_id,
                            username = %username,
                            "Ensured federation keys; retrying delivery sign"
                        );
                        Ok(loaded)
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
                    Err(format!(
                        "Key load failed (ensure): {}; original: {}",
                        ensure_e, e
                    ))
                }
            }
        }
        // Decrypt / other load errors: return as-is. Never ensure/regenerate.
        Err(e) => Err(e),
    }
}

/// True only for missing/empty key material (exact SELECT-no-row path).
/// Decrypt failures must not match — regenerating would rotate the public key.
pub(crate) fn is_missing_federation_keys_error(err: &str) -> bool {
    err.contains("No federation keys found for user")
}

/// Errors that cannot self-heal via ensure (mark dead immediately).
/// Decrypt failures are *not* included: they must not re-generate, but may
/// still back off until max_attempts if ops fix JWT/storage.
pub(crate) fn is_unrecoverable_key_load_error(err: &str) -> bool {
    err.contains("username empty (cannot ensure)")
}

/// 加载用户的密钥对 + 库内 key_id（domain-move 后的规范 keyId）
async fn load_user_keypair(
    db: &DatabaseConnection,
    user_id: i32,
) -> Result<LoadedSigningKey, String> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT public_key_pem, private_key_encrypted, key_id FROM federation_keys WHERE user_id = $1",
            [user_id.into()],
        ))
        .await
        .map_err(|e| { tracing::error!("DB error: {}", e); "Database error".to_string() })?
        .ok_or_else(|| "No federation keys found for user".to_string())?;

    let pub_pem: String = row.try_get("", "public_key_pem").unwrap_or_default();
    let encrypted: String = row.try_get("", "private_key_encrypted").unwrap_or_default();
    let stored_kid: String = row.try_get("", "key_id").unwrap_or_default();

    if pub_pem.trim().is_empty() || encrypted.trim().is_empty() {
        return Err("No federation keys found for user".to_string());
    }

    let jwt_secret = {
        let config = crate::GLOBAL_CONFIG.read().await;
        config.jwt_secret.clone()
    };

    let keypair = KeyPair::from_encrypted(&pub_pem, &encrypted, &jwt_secret)
        .map_err(|e| format!("Key decryption failed: {}", e))?;
    Ok(LoadedSigningKey {
        keypair,
        stored_key_id: if stored_kid.trim().is_empty() {
            None
        } else {
            Some(stored_kid)
        },
    })
}

