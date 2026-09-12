//! Per-user delivery queue stats, list, retry, cancel, dismiss, and purge.

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, DbErr, Statement};
use serde_json::json;

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

pub async fn list_delivery_for_user_filtered(
    db: &DatabaseConnection,
    user_id: i32,
    limit: i64,
    status_filter: Option<&str>,
) -> Result<serde_json::Value, String> {
    let limit = limit.clamp(1, 100);
    let status = status_filter.map(str::trim).filter(|s| {
        matches!(
            *s,
            "pending" | "delivering" | "delivered" | "dead" | "failed"
        )
    });

    let rows = if let Some(st) = status {
        // "dead" UI tab also includes status='failed' rows (no writer currently sets failed)
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
            // Host UI reads these; cancel prefix rules are still duplicated in frontend federationDeliveryUi.ts.
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
        // failed/cancelled Allow; any other unknown status also Allow.
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
            AppError::public_json("Invalid delivery id"),
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
            (StatusCode::INTERNAL_SERVER_ERROR, {
                tracing::error!("DB error: {e}");
                json!({"error": "Database error", "code": "database_error"})
            })
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                AppError::public_json("Delivery item not found"),
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
                AppError::public_json("Already delivered"),
            ));
        }
        RetryStatusDecision::InProgress => {
            return Err((
                StatusCode::CONFLICT,
                AppError::public_json("Delivery currently in progress"),
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
                   last_attempt_at = NULL,
                   lease_token = NULL,
                   lease_expires_at = NULL
               FROM federation_activities a
               WHERE dq.id = $1
                 AND a.id = dq.activity_id
                 AND a.user_id = $2
                 AND dq.status IN ('dead', 'pending', 'failed', 'cancelled')"#,
            [queue_id.into(), user_id.into()],
        ))
        .await
        .map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, {
                tracing::error!("DB error: {e}");
                json!({"error": "Database error", "code": "database_error"})
            })
        })?;

    if result.rows_affected() == 0 {
        return Err((
            StatusCode::CONFLICT,
            AppError::public_json("Could not retry delivery (status changed)"),
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
            AppError::public_json("Invalid delivery id"),
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
            (StatusCode::INTERNAL_SERVER_ERROR, {
                tracing::error!("DB error: {e}");
                json!({"error": "Database error", "code": "database_error"})
            })
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                AppError::public_json("Delivery item not found"),
            )
        })?;

    let status: String = row.try_get("", "status").unwrap_or_default();
    match classify_cancel_status(&status) {
        CancelStatusDecision::AlreadyDelivered => {
            return Err((
                StatusCode::BAD_REQUEST,
                AppError::public_json("Already delivered"),
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
                   next_retry_at = NULL,
                   lease_token = NULL,
                   lease_expires_at = NULL
               FROM federation_activities a
               WHERE dq.id = $1
                 AND a.id = dq.activity_id
                 AND a.user_id = $2
                 AND dq.status IN ('pending', 'delivering')"#,
            [queue_id.into(), user_id.into()],
        ))
        .await
        .map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, {
                tracing::error!("DB error: {e}");
                json!({"error": "Database error", "code": "database_error"})
            })
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
                (StatusCode::INTERNAL_SERVER_ERROR, {
                    tracing::error!("DB error: {e}");
                    json!({"error": "Database error", "code": "database_error"})
                })
            })?;
        let Some(again) = again else {
            return Err((
                StatusCode::NOT_FOUND,
                AppError::public_json("Delivery item not found"),
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
                    AppError::public_json("Already delivered"),
                ));
            }
            _ => {
                let mut body =
                    AppError::conflict("Could not cancel delivery (status changed)").to_json();
                body["status"] = json!(st);
                return Err((StatusCode::CONFLICT, body));
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
            (StatusCode::INTERNAL_SERVER_ERROR, {
                tracing::error!("DB error: {e}");
                json!({"error": "Database error", "code": "database_error"})
            })
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
                       last_attempt_at = NULL,
                       lease_token = NULL,
                       lease_expires_at = NULL
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
            (StatusCode::INTERNAL_SERVER_ERROR, {
                tracing::error!("DB error: {e}");
                json!({"error": "Database error", "code": "database_error"})
            })
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
                       next_retry_at = NULL,
                       lease_token = NULL,
                       lease_expires_at = NULL
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
    db: &impl ConnectionTrait,
    resource_id: &str,
    reason: &str,
) -> Result<u64, DbErr> {
    if resource_id.is_empty() {
        return Ok(0);
    }
    let needle = format!("%{}%", resource_id);
    // Exclude teardown activity types so dissolve/close fan-out rows survive.
    // Normalize `myriad:` prefix the same way as is_resource_teardown_activity_type.
    let result = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"UPDATE federation_delivery_queue dq
               SET status = 'dead',
                   error_message = $2,
                   last_attempt_at = NOW(),
                   next_retry_at = NULL,
                   lease_token = NULL,
                   lease_expires_at = NULL
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
        .await?;
    let cancelled = result.rows_affected();
    if cancelled > 0 {
        tracing::info!(
            resource_id = %resource_id,
            cancelled,
            "Cancelled pending federation deliveries for removed resource"
        );
    }
    Ok(cancelled)
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
            AppError::public_json("Invalid delivery id"),
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
            (StatusCode::INTERNAL_SERVER_ERROR, {
                tracing::error!("DB error: {e}");
                json!({"error": "Database error", "code": "database_error"})
            })
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                AppError::public_json("Delivery item not found"),
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
            (StatusCode::INTERNAL_SERVER_ERROR, {
                tracing::error!("DB error: {e}");
                json!({"error": "Database error", "code": "database_error"})
            })
        })?;

    if result.rows_affected() == 0 {
        return Err((
            StatusCode::CONFLICT,
            AppError::public_json("Could not dismiss delivery (status changed)"),
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
            (StatusCode::INTERNAL_SERVER_ERROR, {
                tracing::error!("DB error: {e}");
                json!({"error": "Database error", "code": "database_error"})
            })
        })?;

    Ok(json!({
        "success": true,
        "purged": result.rows_affected(),
        "limit": limit,
        "cancelled_only": cancelled_only,
    }))
}
use myriad_error::AppError;
