//! Same-instance inbox delivery and outbound delivery-queue enqueue.

use axum::{Json, http::StatusCode};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};

use crate::federation::actor::fetch_remote_actor;
use crate::federation::types::*;

use super::activities::{extract_activity_actor_id, move_preflight_error};
use super::inbox_err;
use super::receipt::receipt_key;
use super::receive::{
    execute_personal_activity, get_local_user, personal_inbox_scope, preflight_room_join_member,
};

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
               VALUES ($1, $2, $3, NULL, $4, true, $5::timestamptz)
               RETURNING id"#,
            [
                activity.id.clone().into(),
                user_id.into(),
                activity.activity_type.clone().into(),
                activity_json.clone().into(),
                crate::clock::now().into(),
            ],
        ))
        .await
        .map_err(db_err)?;

    let act_id = returning_id(act_row).map_err(|error| {
        db_err(sea_orm::DbErr::Custom(error))
    })?;

    // Same-instance inbox → handle directly (Follow / Accept / Undo / …).
    // Box::pin breaks the async recursion cycle:
    // deliver_activity_locally → handle_follow → PostCommit → enqueue_delivery → …
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
                           VALUES ($1, $2, $3, 'delivered', $4::timestamptz, $4::timestamptz)
                   ON CONFLICT (activity_id, target_inbox) DO NOTHING"#,
                        [
                            act_id.into(),
                            target_inbox.into(),
                            domain.into(),
                            crate::clock::now().into(),
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
           VALUES ($1, $2, $3, 'pending', $4::timestamptz)
                   ON CONFLICT (activity_id, target_inbox) DO NOTHING"#,
        [
            act_id.into(),
            target_inbox.into(),
            domain.into(),
            crate::clock::now().into(),
        ],
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
               VALUES ($1, $2, $3, NULL, $4, true, $5::timestamptz)
               RETURNING id"#,
            [
                activity.id.clone().into(),
                user_id.into(),
                activity.activity_type.clone().into(),
                activity_json.into(),
                crate::clock::now().into(),
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
           VALUES ($1, $2, $3, 'pending', $4::timestamptz)
           ON CONFLICT (activity_id, target_inbox) DO NOTHING"#,
        [
            act_id.into(),
            target_inbox.into(),
            domain.into(),
            crate::clock::now().into(),
        ],
    ))
    .await
    .map_err(db_err)?;
    Ok(())
}

fn validate_delivery_activity_id(activity_id: i32) -> Result<i32, String> {
    crate::federation::types::require_positive_id(Some(activity_id))
}

/// If inbox is `{base}/users/{username}/inbox`, return username.
pub(super) fn local_username_from_inbox_url(base_url: &str, inbox_url: &str) -> Option<String> {
    let trimmed = inbox_url.trim().trim_end_matches('/');
    let actor = trimmed.strip_suffix("/inbox")?;
    local_username_from_actor_url(base_url, actor)
}

/// Process an Activity for a local user exactly as if it had arrived at their
/// personal inbox, minus the HTTP transport.
///
/// Only the transport-level checks are skipped: the signer must be one of this
/// instance's own actors (it signed nothing because it never left the
/// process), so there is no HTTP Signature to verify and no trust policy to
/// apply to our own domain. Everything after that is the remote path —
/// the same actor / Move / RoomJoin preflight, the same durable receipt, the
/// same transaction and the same [`execute_personal_activity`] dispatch. A
/// same-instance follower therefore sees a Delete or Undo exactly as a remote
/// follower's instance would process it.
///
/// Used where the delivery worker cannot help (it refuses localhost / private
/// inboxes): same-instance Follow / Accept and follower fan-out.
pub async fn deliver_activity_locally(
    db: &DatabaseConnection,
    username: &str,
    activity: &serde_json::Value,
) -> Result<(), String> {
    let (user_id, _) = get_local_user(db, username)
        .await
        .map_err(|error| error_message(error, "user not found"))?;

    let activity_type = activity["type"].as_str().unwrap_or("");
    let signer = extract_activity_actor_id(activity);
    if activity_type.is_empty() || signer.is_empty() {
        return Err("Missing actor or type in activity".into());
    }
    let base_url = get_base_url().await;
    if local_username_from_actor_url(&base_url, &signer).is_none() {
        return Err(format!(
            "In-process delivery requires a local signer, got {signer}"
        ));
    }

    // Same preflight as `post_inbox`, resolved before the receipt transaction.
    // Local actors resolve from the users table, never over HTTP.
    let signer_actor = if matches!(
        activity_type,
        "Follow" | "Create" | "Update" | "Delete" | "Announce" | "Like"
    ) {
        Some(fetch_remote_actor(db, &signer).await?)
    } else {
        None
    };
    preflight_room_join_member(db, activity_type, &signer, activity)
        .await
        .map_err(|error| error_message(error, "RoomJoin preflight failed"))?;
    let move_verified = if activity_type == "Move" {
        let verified = crate::federation::move_actor::preflight_move(db, &signer, activity)
            .await
            .map_err(|error| error_message(move_preflight_error(error), "Move rejected"))?;
        Some(verified)
    } else {
        None
    };

    let body = serde_json::to_vec(activity).map_err(|e| format!("encode activity: {e}"))?;
    let key = receipt_key(
        &signer,
        activity["id"].as_str().unwrap_or(""),
        &personal_inbox_scope(user_id),
        &body,
    );
    execute_personal_activity(
        db,
        user_id,
        &key,
        &signer,
        activity_type,
        activity,
        signer_actor.as_ref(),
        signer_actor.as_ref(),
        move_verified.as_ref(),
    )
    .await
    .map(|_| ())
    .map_err(|error| error_message(error, "local delivery failed"))
}

fn error_message(error: (StatusCode, Json<serde_json::Value>), fallback: &str) -> String {
    error
        .1
        .0
        .get("error")
        .and_then(|v| v.as_str())
        .unwrap_or(fallback)
        .to_string()
}

#[cfg(test)]
mod tests {
    #[test]
    fn delivery_queue_rejects_non_positive_activity_ids() {
        assert!(super::validate_delivery_activity_id(0).is_err());
        assert!(super::validate_delivery_activity_id(-1).is_err());
        assert_eq!(super::validate_delivery_activity_id(1), Ok(1));
    }

    use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
    use serde_json::json;

    use super::deliver_activity_locally;
    use crate::federation::types::get_base_url;

    async fn count(db: &DatabaseConnection, sql: &str) -> i64 {
        db.query_one_raw(Statement::from_string(DatabaseBackend::Postgres, sql))
            .await
            .unwrap()
            .unwrap()
            .try_get_by_index::<i64>(0)
            .unwrap()
    }

    /// 同实例投递与远端收件箱同一套分发：Delete / Undo 撤掉时间线上的状态，
    /// 而不是各留一条空条目；回执去重；非本地签名者与未知 MFP 类型被拒。
    #[tokio::test]
    async fn same_instance_delivery_matches_the_remote_inbox() {
        let Some(fixture) = crate::federation::test_db::SchemaDb::new_or_media().await else {
            return;
        };
        let db = &fixture.db;
        let base = get_base_url().await;
        let alice = format!("{base}/users/alice");
        db.execute_unprepared("INSERT INTO users (id, username) VALUES (1, 'alice'), (2, 'bob')")
            .await
            .unwrap();
        let bob_rows = "SELECT COUNT(*) FROM federation_timeline WHERE user_id = 2";

        // Create 进时间线，Delete（对象是原活动 id）把它删掉，且不留 Delete 条目。
        let create = json!({
            "type": "Create",
            "id": format!("{base}/activities/c1"),
            "actor": &alice,
            "to": [crate::federation::types::AP_PUBLIC],
            "object": {
                "type": "Note",
                "id": format!("{base}/notes/n1"),
                "attributedTo": &alice,
                "content": "<p>hello &amp; bye</p>",
            },
        });
        deliver_activity_locally(db, "bob", &create).await.unwrap();
        assert_eq!(count(db, bob_rows).await, 1);
        let delete = json!({
            "type": "Delete",
            "id": format!("{base}/activities/d1"),
            "actor": &alice,
            "object": format!("{base}/activities/c1"),
        });
        deliver_activity_locally(db, "bob", &delete).await.unwrap();
        assert_eq!(count(db, bob_rows).await, 0);

        // Announce 进时间线，Undo(Announce) 撤掉它，不留 Undo 条目。
        let announce = json!({
            "type": "Announce",
            "id": format!("{base}/activities/a1"),
            "actor": &alice,
            "to": [crate::federation::types::AP_PUBLIC],
            "object": "https://remote.example/notes/9",
        });
        deliver_activity_locally(db, "bob", &announce).await.unwrap();
        assert_eq!(count(db, bob_rows).await, 1);
        let undo = json!({
            "type": "Undo",
            "id": format!("{base}/activities/u1"),
            "actor": &alice,
            "object": {
                "type": "Announce",
                "id": format!("{base}/activities/a1"),
                "actor": &alice,
                "object": "https://remote.example/notes/9",
            },
        });
        deliver_activity_locally(db, "bob", &undo).await.unwrap();
        assert_eq!(count(db, bob_rows).await, 0);

        // 同一活动再投一次：回执已接受，不再执行，撤掉的转发不会回来。
        deliver_activity_locally(db, "bob", &announce).await.unwrap();
        assert_eq!(count(db, bob_rows).await, 0);
        assert_eq!(
            count(
                db,
                "SELECT COUNT(*) FROM federation_inbox_receipts WHERE inbox_scope = 'user:2'"
            )
            .await,
            4
        );

        // 改资料的 Update 不是帖子。
        let update = json!({
            "type": "Update",
            "id": format!("{base}/activities/p1"),
            "actor": &alice,
            "object": {"type": "Person", "id": &alice, "name": "Alice"},
        });
        deliver_activity_locally(db, "bob", &update).await.unwrap();
        assert_eq!(count(db, bob_rows).await, 0);

        // 远端签名者只能走 HTTP 验签；未知 MFP 类型与远端路径一样被拒。
        let forged = json!({
            "type": "Create",
            "id": "https://evil.example/activities/x",
            "actor": "https://evil.example/users/x",
            "object": {"type": "Note", "id": "https://evil.example/notes/x"},
        });
        assert!(deliver_activity_locally(db, "bob", &forged).await.is_err());
        let bogus = json!({
            "type": "myriad:NotAThing",
            "id": format!("{base}/activities/m1"),
            "actor": &alice,
        });
        assert!(deliver_activity_locally(db, "bob", &bogus).await.is_err());
        assert_eq!(count(db, bob_rows).await, 0);

        fixture.close().await;
    }
}
