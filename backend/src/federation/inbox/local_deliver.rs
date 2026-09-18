//! Same-instance inbox delivery and outbound delivery-queue enqueue.

use axum::{Json, http::StatusCode};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::json;

use crate::federation::actor::fetch_remote_actor;
use crate::federation::types::*;

use super::activities::{
    extract_activity_actor_id, handle_accept, handle_content_activity, handle_follow, handle_move,
    handle_reject, handle_undo,
};
use super::inbox_err;
use super::mfp::handle_mfp_activity;
use super::receive::get_local_user;

/// Receipt HTTP path uses `QueueOnly`. `InProcess` is the trusted local helper
/// (no nested receipt txn).
#[derive(Clone, Copy)]
pub(crate) enum DeliveryMode<'a> {
    QueueOnly,
    InProcess(&'a DatabaseConnection),
}

/// 将 Activity 入库；同实例 inbox 当场处理，否则入 pending 队列。
///
/// Same-instance inboxes are processed in-process (no HTTP). The delivery worker
/// refuses localhost/private targets, so without this shortcut Follow Accept
/// never lands and the initiator stays stuck on `pending` while the followee
/// already shows the follower as accepted.
pub(crate) async fn enqueue_delivery(
    db: &DatabaseConnection,
    user_id: i32,
    activity: &Activity,
    target_inbox: &str,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    // Full Activity JSON for `delivery/dispatch.rs` (`deliver_activity`).
    let activity_json = serde_json::to_value(activity).unwrap_or_default();
    let domain = extract_domain(target_inbox).unwrap_or_default();
    let base_url = get_base_url().await;

    // 存 Activity 记录
    let act_row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"INSERT INTO federation_activities
                   (activity_id, user_id, activity_type, object_type, object_json, is_local, published_at)
               VALUES ($1, $2, $3, NULL, $4, true, NOW())
               RETURNING id"#,
            [
                activity.id.clone().into(),
                user_id.into(),
                activity.activity_type.clone().into(),
                activity_json.clone().into(),
            ],
        ))
        .await
        .map_err(db_err)?;

    let act_id: i32 = act_row
        .map(|r| r.try_get("", "id").unwrap_or(0))
        .unwrap_or(0);

    // Same-instance inbox → handle directly (Follow / Accept / Undo / …).
    // Box::pin breaks the async recursion cycle:
    // deliver_activity_locally → handle_follow → enqueue_delivery → …
    if let Some(local_username) = local_username_from_inbox_url(&base_url, target_inbox) {
        match Box::pin(deliver_activity_locally(
            db,
            &local_username,
            &activity_json,
        ))
        .await
        {
            Ok(()) => {
                tracing::info!(
                    activity_type = %activity.activity_type,
                    target = %local_username,
                    "📬 Local inbox delivery (no HTTP)"
                );
                // Mark as delivered for observability (queue row optional)
                let _ = db
                    .execute_raw(Statement::from_sql_and_values(
                        DatabaseBackend::Postgres,
                        r#"INSERT INTO federation_delivery_queue
                               (activity_id, target_inbox, target_domain, status, created_at, last_attempt_at)
                           VALUES ($1, $2, $3, 'delivered', NOW(), NOW())
                   ON CONFLICT (activity_id, target_inbox) DO NOTHING"#,
                        [
                            act_id.into(),
                            target_inbox.into(),
                            domain.into(),
                        ],
                    ))
                    .await;
                return Ok(());
            }
            Err(e) => {
                tracing::warn!(
                    activity_type = %activity.activity_type,
                    target = %local_username,
                    error = %e,
                    "Local inbox delivery failed; falling back to HTTP queue"
                );
            }
        }
    }

    // 加入投递队列（远程 / local fallback）
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"INSERT INTO federation_delivery_queue
               (activity_id, target_inbox, target_domain, status, created_at)
           VALUES ($1, $2, $3, 'pending', NOW())
                   ON CONFLICT (activity_id, target_inbox) DO NOTHING"#,
        [act_id.into(), target_inbox.into(), domain.into()],
    ))
    .await
    .map_err(db_err)?;

    Ok(())
}

/// Queue-only variant used by receipt transactions.  It never performs local
/// recursive delivery or external I/O; the delivery queue is the durable
/// outbox for the resulting Accept.
pub(crate) async fn enqueue_delivery_queue(
    db: &impl ConnectionTrait,
    user_id: i32,
    activity: &Activity,
    target_inbox: &str,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    let activity_json = serde_json::to_value(activity).unwrap_or_default();
    let domain = extract_domain(target_inbox).unwrap_or_default();
    let act_row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"INSERT INTO federation_activities
                   (activity_id, user_id, activity_type, object_type, object_json, is_local, published_at)
               VALUES ($1, $2, $3, NULL, $4, true, NOW())
               RETURNING id"#,
            [
                activity.id.clone().into(),
                user_id.into(),
                activity.activity_type.clone().into(),
                activity_json.into(),
            ],
        ))
        .await
        .map_err(db_err)?;
    let act_row = act_row.ok_or_else(|| {
        inbox_err(
            "queue Accept activity failed",
            "INSERT RETURNING id produced no row".to_string(),
        )
    })?;
    let act_id = act_row
        .try_get::<i32>("", "id")
        .map_err(|e| inbox_err("read queued Accept activity id", e.to_string()))?;
    let act_id = validate_delivery_activity_id(act_id)
        .map_err(|e| inbox_err("validate queued Accept activity id", e))?;
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"INSERT INTO federation_delivery_queue
               (activity_id, target_inbox, target_domain, status, created_at)
           VALUES ($1, $2, $3, 'pending', NOW())
           ON CONFLICT (activity_id, target_inbox) DO NOTHING"#,
        [act_id.into(), target_inbox.into(), domain.into()],
    ))
    .await
    .map_err(db_err)?;
    Ok(())
}

fn validate_delivery_activity_id(activity_id: i32) -> Result<i32, String> {
    crate::federation::types::require_positive_id(Some(activity_id))
}

/// If inbox is `{base}/users/{username}/inbox`, return username.
fn local_username_from_inbox_url(base_url: &str, inbox_url: &str) -> Option<String> {
    let trimmed = inbox_url.trim().trim_end_matches('/');
    let actor = trimmed.strip_suffix("/inbox")?;
    local_username_from_actor_url(base_url, actor)
}

/// Process an Activity for a local user as if it arrived at their personal inbox
/// (skips HTTP Signature — caller is trusted in-process).
///
/// Used for same-instance Follow/Accept so initiator outgoing status flips to
/// `accepted` without waiting on the delivery worker (which refuses internal URLs).
pub async fn deliver_activity_locally(
    db: &DatabaseConnection,
    username: &str,
    activity: &serde_json::Value,
) -> Result<(), String> {
    let (user_id, _) = get_local_user(db, username).await.map_err(|(_, j)| {
        j.0.get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("user not found")
            .to_string()
    })?;

    let activity_type = activity["type"].as_str().unwrap_or("");
    let actor_url_owned = extract_activity_actor_id(activity);
    if activity_type.is_empty() || actor_url_owned.is_empty() {
        return Err("Missing actor or type in activity".into());
    }
    let actor_url_str = actor_url_owned.as_str();
    let follow_remote = if activity_type == "Follow" {
        Some(fetch_remote_actor(db, actor_url_str).await?)
    } else {
        None
    };
    let content_remote = if matches!(
        activity_type,
        "Create" | "Update" | "Delete" | "Announce" | "Like"
    ) {
        Some(fetch_remote_actor(db, actor_url_str).await?)
    } else {
        None
    };

    let result = match activity_type {
        "Follow" => {
            handle_follow(
                db,
                user_id,
                actor_url_str,
                activity,
                follow_remote.as_ref(),
                DeliveryMode::InProcess(db),
            )
            .await
        }
        "Accept" => handle_accept(db, user_id, activity).await,
        "Reject" => handle_reject(db, user_id, actor_url_str, activity).await,
        "Undo" => handle_undo(db, user_id, actor_url_str, activity).await,
        "Move" => handle_move(db, actor_url_str, activity).await,
        "Create" | "Update" | "Delete" | "Announce" | "Like" => {
            handle_content_activity(
                db,
                user_id,
                actor_url_str,
                activity_type,
                activity,
                content_remote.as_ref(),
            )
            .await
        }
        other if other.starts_with("myriad:") => {
            handle_mfp_activity(db, Some(user_id), actor_url_str, other, activity).await
        }
        other => {
            tracing::debug!(
                activity_type = other,
                "Local delivery: unsupported type, ignoring"
            );
            Ok(StatusCode::ACCEPTED)
        }
    };

    result.map(|_| ()).map_err(|(_, j)| {
        j.0.get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("local delivery failed")
            .to_string()
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn delivery_queue_rejects_non_positive_activity_ids() {
        assert!(super::validate_delivery_activity_id(0).is_err());
        assert!(super::validate_delivery_activity_id(-1).is_err());
        assert_eq!(super::validate_delivery_activity_id(1), Ok(1));
    }
}
