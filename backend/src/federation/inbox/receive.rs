//! Personal and shared ActivityPub inbox HTTP receive paths.

use axum::{
    Json,
    extract::{Path, State},
    http::{Request, StatusCode},
};
use myriad_error::AppError;
use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, DatabaseTransaction, Statement,
    TransactionTrait,
};
use serde_json::json;

use crate::federation::actor::{RemoteActorInfo, fetch_remote_actor};
use crate::federation::errors::public_inbox_error;
use crate::federation::limits::{
    buffer_inbox_body, try_acquire_inbox_parse, validate_inbox_json_budget,
};
use crate::federation::types::*;

use super::activities::{
    distribute_to_followers, extract_accept_object_id, extract_activity_actor_id, handle_accept,
    handle_content_activity, handle_follow, handle_reject, handle_undo, record_room_peer_activity,
};
use super::inbox_err;
use super::local_deliver::DeliveryMode;
use super::mfp::{ensure_allowed_mfp_type, handle_mfp_activity};
use super::receipt::{
    ReceiptClaim, ReceiptKey, ReceiptOutcome, claim_receipt, finish_receipt, receipt_key,
};
use super::signature::{promote_verified_actor, verify_preparse_gate, verify_request_signature};

/// A receipt conflict is a protocol error, not a replay success.  Returning
/// 409 makes a peer/operator aware that one activity id was reused for
/// different signed bytes and, importantly, never runs either handler.
fn receipt_conflict(
    key: &ReceiptKey,
    stored_digest: &str,
) -> (StatusCode, Json<serde_json::Value>) {
    tracing::warn!(
        signer = %key.signer,
        activity_id = %key.activity_id,
        expected_digest = %stored_digest,
        received_digest = %key.body_digest,
        "Federation activity id reused with a different body digest"
    );
    (
        StatusCode::CONFLICT,
        Json(json!({
            "error": "Activity id was already received with different bytes",
            "activity_id": key.activity_id,
        })),
    )
}

fn receipt_rejected(status: u16, message: Option<&str>) -> (StatusCode, Json<serde_json::Value>) {
    let status = StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_REQUEST);
    let public = message
        .map(str::trim)
        .filter(|message| !message.is_empty())
        .map(public_inbox_error)
        .unwrap_or("Activity was permanently rejected");
    (
        status,
        Json(json!({
            "error": public,
        })),
    )
}

fn room_join_member_actor(
    activity_type: &str,
    actor_url: &str,
    activity: &serde_json::Value,
) -> Option<String> {
    if activity_type != "myriad:RoomJoin" || activity.get("object").is_none() {
        return None;
    }
    Some(
        activity
            .get("object")
            .and_then(|object| object.get("member"))
            .and_then(|member| member.as_str())
            .filter(|member| !member.trim().is_empty())
            .unwrap_or(actor_url)
            .to_string(),
    )
}

/// Resolve a remote RoomJoin member before opening the receipt transaction.
/// KeyExchange fanout then uses only DB state and can roll back atomically with
/// the membership write. Local-member spoof attempts are left to the handler's
/// permanent authorization rejection and must not trigger an HTTP self-fetch.
async fn preflight_room_join_member(
    db: &DatabaseConnection,
    activity_type: &str,
    actor_url: &str,
    activity: &serde_json::Value,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    let Some(joining) = room_join_member_actor(activity_type, actor_url, activity) else {
        return Ok(());
    };
    let base_url = get_base_url().await;
    if local_username_from_actor_url(&base_url, &joining).is_some() {
        return Ok(());
    }
    fetch_remote_actor(db, &joining).await.map_err(|e| {
        tracing::warn!(actor = %joining, error = %e, "Failed to preflight RoomJoin member");
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "Cannot resolve RoomJoin member actor",
                "retry": true
            })),
        )
    })?;
    Ok(())
}

/// Actor that verified the signature, with its real `federation_remote_actors`
/// id, for preflight branches that write it as a foreign key.
///
/// Promotion only fails when persisting an already-verified actor fails (a
/// local DB fault): retryable 503, never a permanent 4xx rejection and never an
/// id of 0. The cause is logged by `promote_verified_actor`, not returned.
fn require_verified_actor(
    verified_actor: &Result<RemoteActorInfo, String>,
) -> Result<&RemoteActorInfo, (StatusCode, Json<serde_json::Value>)> {
    verified_actor.as_ref().map_err(|_| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "Remote actor temporarily unavailable",
                "retry": true
            })),
        )
    })
}

/// 4xx handler results are deterministic peer/state failures and can be
/// durably rejected.  429 and all 5xx results stay retryable.
fn receipt_result_is_permanent(status: StatusCode) -> bool {
    status != StatusCode::TOO_MANY_REQUESTS && status.is_client_error()
}

const RECEIPT_HANDLER_SAVEPOINT: &str = "myriad_inbox_handler";

/// Isolate handler effects from the already-claimed receipt. A permanent
/// rejection rolls back to this point, then records only the rejected receipt.
pub(crate) async fn begin_receipt_handler_effects(
    txn: &DatabaseTransaction,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    txn.execute_unprepared(&format!("SAVEPOINT {RECEIPT_HANDLER_SAVEPOINT}"))
        .await
        .map_err(|e| inbox_err("create inbox handler savepoint", e.to_string()))?;
    Ok(())
}

pub(crate) async fn rollback_receipt_handler_effects(
    txn: &DatabaseTransaction,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    txn.execute_unprepared(&format!(
        "ROLLBACK TO SAVEPOINT {RECEIPT_HANDLER_SAVEPOINT}"
    ))
    .await
    .map_err(|e| inbox_err("rollback rejected inbox handler effects", e.to_string()))?;
    Ok(())
}

async fn rollback_receipt_transaction(txn: DatabaseTransaction) {
    if let Err(e) = txn.rollback().await {
        tracing::error!(error = %e, "Failed to rollback federation inbox receipt transaction");
    }
}

/// Claim a receipt and map non-execution states to the HTTP result expected by
/// ActivityPub peers.  The transaction remains open only for `Execute`.
async fn claim_or_respond(
    txn: &DatabaseTransaction,
    key: &ReceiptKey,
) -> Result<Option<StatusCode>, (StatusCode, Json<serde_json::Value>)> {
    match claim_receipt(txn, key)
        .await
        .map_err(|e| inbox_err("inbox receipt claim failed", e))?
    {
        ReceiptClaim::Execute(_) => Ok(None),
        ReceiptClaim::AlreadyAccepted => {
            tracing::info!(
                signer = %key.signer,
                activity_id = %key.activity_id,
                inbox_scope = %key.inbox_scope,
                "suppressed duplicate federation activity using durable inbox receipt"
            );
            Ok(Some(StatusCode::ACCEPTED))
        }
        ReceiptClaim::Conflict { stored_digest } => Err(receipt_conflict(key, &stored_digest)),
        ReceiptClaim::Rejected { status, message } => {
            Err(receipt_rejected(status, message.as_deref()))
        }
    }
}

/// Complete the receipt and commit the same transaction that contains the
/// handler's DB effects. Retryable failures roll back the whole transaction,
/// including the uncommitted claim, so a fresh request can execute without
/// retaining partial effects or a lease-recovery state machine.
async fn finish_and_commit(
    txn: DatabaseTransaction,
    key: &ReceiptKey,
    outcome: ReceiptOutcome,
    status: StatusCode,
    message: Option<&str>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    finish_receipt(&txn, key, outcome, status.as_u16(), message)
        .await
        .map_err(|e| inbox_err("inbox receipt completion failed", e))?;
    txn.commit()
        .await
        .map_err(|e| inbox_err("inbox receipt commit failed", e.to_string()))?;
    Ok(status)
}

/// POST /users/{username}/inbox
///
/// 接收远程实例发来的 Activity
/// 必须携带有效的 HTTP Signature
pub async fn post_inbox(
    State(db): State<DatabaseConnection>,
    Path(username): Path<String>,
    request: Request<axum::body::Body>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    // Cheap existence check before reserving concurrent memory budget.
    let (user_id, _) = get_local_user(&db, &username).await?;

    let headers = request.headers().clone();
    // reserve concurrent raw-body budget *before* buffering; release on drop.
    // Exhausted raw-body budget → 429 before allocating the request body.
    let (body, _inflight) = buffer_inbox_body(request).await?;

    // 先做只依赖 header/原始字节的检查，再解析 body（inbox 上限见 federation::limits::INBOX_BODY_LIMIT）
    let signature = verify_preparse_gate(&headers, &body)?;

    // Do not queue already-buffered remote bodies behind admitted deliveries,
    // and reject pathological JSON before allocating a complete Value tree.
    // The permit intentionally lives with `activity` through verification and
    // dispatch so this cap bounds complete parsed trees, not only parse CPU.
    let _parse_permit = try_acquire_inbox_parse().ok_or_else(|| {
        (
            StatusCode::TOO_MANY_REQUESTS,
            Json(AppError::public_json(
                "Inbox delivery budget exhausted; retry later",
            )),
        )
    })?;
    validate_inbox_json_budget(&body)
        .map_err(|error| (StatusCode::BAD_REQUEST, Json(AppError::public_json(error))))?;

    // 解析 Activity JSON
    let activity: serde_json::Value = serde_json::from_slice(&body).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json("Invalid JSON body")),
        )
    })?;

    let activity_type = activity["type"].as_str().unwrap_or("").to_string();
    // Actor may be a string IRI or expanded object `{ "id": "...", "type": "Person" }`.
    let actor_url_str = extract_activity_actor_id(&activity);

    if actor_url_str.is_empty() || activity_type.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json("Missing actor or type in activity")),
        ));
    }

    // 验证 HTTP Signature（actor fetch is ephemeral until verified）
    let request_path = format!("/users/{}/inbox", username);
    let verified =
        verify_request_signature(&db, &headers, &signature, &actor_url_str, &request_path).await?;
    // Same actor facts that verified the signature; no second fetch below.
    let verified_actor = promote_verified_actor(&db, &actor_url_str, verified).await;

    let activity_id = activity["id"].as_str().unwrap_or("");

    // 信任策略：黑名单 / 速率 / 内容过滤
    let actor_domain = extract_domain(&actor_url_str).unwrap_or_default();
    if let Err(reason) =
        crate::federation::trust::enforce_inbound(&db, &actor_domain, &activity).await
    {
        tracing::warn!(
            "🛑 Inbox rejected by trust policy: domain={}, reason={}",
            actor_domain,
            reason
        );
        return Err((
            StatusCode::FORBIDDEN,
            Json(AppError::public_json("Rejected by trust policy")),
        ));
    }

    // Remote actor resolution is a preflight step.  It must complete before
    // opening the receipt transaction so a failed/unknown actor cannot leave
    // a claimed receipt or partial handler effects behind.
    let follow_remote = if activity_type == "Follow" {
        Some(require_verified_actor(&verified_actor)?)
    } else {
        None
    };
    let content_remote = if matches!(
        activity_type.as_str(),
        "Create" | "Update" | "Delete" | "Announce" | "Like"
    ) {
        Some(require_verified_actor(&verified_actor)?)
    } else {
        None
    };
    preflight_room_join_member(&db, &activity_type, &actor_url_str, &activity).await?;

    tracing::info!(
        "📬 Inbox received: type={}, actor={}, target_user={}",
        activity_type,
        actor_url_str,
        username
    );

    let inbox_scope = format!("user:{user_id}");
    let key = receipt_key(&actor_url_str, activity_id, &inbox_scope, &body);
    let txn = db
        .begin()
        .await
        .map_err(|e| inbox_err("begin inbox receipt transaction", e.to_string()))?;
    match claim_or_respond(&txn, &key).await {
        Ok(Some(status)) => {
            rollback_receipt_transaction(txn).await;
            return Ok(status);
        }
        Ok(None) => {}
        Err(error) => {
            rollback_receipt_transaction(txn).await;
            return Err(error);
        }
    }
    if let Err(error) = begin_receipt_handler_effects(&txn).await {
        rollback_receipt_transaction(txn).await;
        return Err(error);
    }

    let result = dispatch_personal_activity(
        &txn,
        user_id,
        &actor_url_str,
        &activity_type,
        &activity,
        follow_remote,
        content_remote,
        DeliveryMode::QueueOnly,
    )
    .await;
    match result {
        Ok(status) => finish_and_commit(txn, &key, ReceiptOutcome::Accepted, status, None).await,
        Err((status, body)) if receipt_result_is_permanent(status) => {
            let message = body
                .0
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("permanent federation inbox rejection")
                .to_string();
            if let Err(error) = rollback_receipt_handler_effects(&txn).await {
                rollback_receipt_transaction(txn).await;
                return Err(error);
            }
            finish_and_commit(txn, &key, ReceiptOutcome::Rejected, status, Some(&message))
                .await
                .and(Err((status, Json(body.0))))
        }
        Err((status, body)) => {
            rollback_receipt_transaction(txn).await;
            Err((status, body))
        }
    }
}

async fn dispatch_personal_activity<C: ConnectionTrait>(
    db: &C,
    local_user_id: i32,
    actor_url_str: &str,
    activity_type: &str,
    activity: &serde_json::Value,
    follow_remote: Option<&RemoteActorInfo>,
    content_remote: Option<&RemoteActorInfo>,
    delivery_mode: DeliveryMode<'_>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    match activity_type {
        "Follow" => {
            handle_follow(
                db,
                local_user_id,
                actor_url_str,
                activity,
                follow_remote,
                delivery_mode,
            )
            .await
        }
        "Accept" => handle_accept(db, local_user_id, activity).await,
        "Reject" => handle_reject(db, local_user_id, actor_url_str, activity).await,
        "Undo" => handle_undo(db, local_user_id, actor_url_str, activity).await,
        // Move verification performs remote HTTP fetches.  It must be split
        // into preflight + transactional migration before being accepted here;
        // never claim a receipt and then execute crash-partial DB effects.
        "Move" => Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(AppError::public_json(
                "Move handling temporarily unavailable while transactional verification is pending",
            )),
        )),
        "Create" | "Update" | "Delete" | "Announce" | "Like" => {
            handle_content_activity(
                db,
                local_user_id,
                actor_url_str,
                activity_type,
                activity,
                content_remote,
            )
            .await
        }
        ty if ty.starts_with("myriad:") => {
            ensure_allowed_mfp_type(ty)?;
            handle_mfp_activity(db, Some(local_user_id), actor_url_str, ty, activity).await
        }
        _ => Ok(StatusCode::ACCEPTED),
    }
}

/// POST /inbox  (Shared Inbox)
///
/// 共享收件箱 — 面向所有本地用户的 Activity
pub async fn post_shared_inbox(
    State(db): State<DatabaseConnection>,
    request: Request<axum::body::Body>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    let headers = request.headers().clone();
    // reserve concurrent raw-body budget *before* buffering; release on drop.
    // Exhausted raw-body budget → 429 before allocating the request body.
    let (body, _inflight) = buffer_inbox_body(request).await?;

    // 先做只依赖 header/原始字节的检查，再解析 body（inbox 上限见 federation::limits::INBOX_BODY_LIMIT）
    let signature = verify_preparse_gate(&headers, &body)?;

    // Held through verification and dispatch while the complete JSON tree is
    // alive; see FEDERATION.md "Public inbox resource boundary".
    let _parse_permit = try_acquire_inbox_parse().ok_or_else(|| {
        (
            StatusCode::TOO_MANY_REQUESTS,
            Json(AppError::public_json(
                "Inbox delivery budget exhausted; retry later",
            )),
        )
    })?;
    validate_inbox_json_budget(&body)
        .map_err(|error| (StatusCode::BAD_REQUEST, Json(AppError::public_json(error))))?;

    let activity: serde_json::Value = serde_json::from_slice(&body).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json("Invalid JSON body")),
        )
    })?;

    let activity_type = activity["type"].as_str().unwrap_or("").to_string();
    let actor_url_str = extract_activity_actor_id(&activity);

    if actor_url_str.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json("Missing actor in activity")),
        ));
    }

    // 验证签名（actor fetch is ephemeral until verified）
    let verified =
        verify_request_signature(&db, &headers, &signature, &actor_url_str, "/inbox").await?;
    // Same actor facts that verified the signature; no second fetch below.
    let verified_actor = promote_verified_actor(&db, &actor_url_str, verified).await;

    let activity_id = activity["id"].as_str().unwrap_or("");

    // 信任策略
    let actor_domain = extract_domain(&actor_url_str).unwrap_or_default();
    if let Err(reason) =
        crate::federation::trust::enforce_inbound(&db, &actor_domain, &activity).await
    {
        tracing::warn!(
            "🛑 Shared inbox rejected by trust policy: domain={}, reason={}",
            actor_domain,
            reason
        );
        return Err((
            StatusCode::FORBIDDEN,
            Json(AppError::public_json("Rejected by trust policy")),
        ));
    }

    tracing::info!(
        "📬 Shared inbox received: type={}, actor={}",
        activity_type,
        actor_url_str
    );

    // Remote actor resolution and object ownership are preflight checks.  No
    // receipt is created until they pass, so policy/identity rejection cannot
    // be mistaken for an accepted delivery.
    let public_remote_id = if matches!(activity_type.as_str(), "Create" | "Announce")
        && crate::federation::audience::may_distribute_to_followers(&activity, &actor_url_str)
    {
        let remote = require_verified_actor(&verified_actor)?;
        if activity_type == "Create" {
            crate::federation::audience::verify_object_ownership(
                &actor_url_str,
                &activity["object"],
            )
            .map_err(|e| {
                tracing::warn!(error = %e, "Shared inbox Create rejected by ownership check");
                (
                    StatusCode::FORBIDDEN,
                    Json(AppError::public_json("Object ownership check failed")),
                )
            })?;
        }
        Some(remote.id)
    } else {
        None
    };
    let follow_remote = if activity_type == "Follow" {
        Some(require_verified_actor(&verified_actor)?)
    } else {
        None
    };
    let content_remote = if matches!(activity_type.as_str(), "Delete" | "Update" | "Like") {
        Some(require_verified_actor(&verified_actor)?)
    } else {
        None
    };
    preflight_room_join_member(&db, &activity_type, &actor_url_str, &activity).await?;

    let key = receipt_key(&actor_url_str, activity_id, "shared", &body);
    let txn = db
        .begin()
        .await
        .map_err(|e| inbox_err("begin shared inbox receipt transaction", e.to_string()))?;
    match claim_or_respond(&txn, &key).await {
        Ok(Some(status)) => {
            rollback_receipt_transaction(txn).await;
            return Ok(status);
        }
        Ok(None) => {}
        Err(error) => {
            rollback_receipt_transaction(txn).await;
            return Err(error);
        }
    }
    if let Err(error) = begin_receipt_handler_effects(&txn).await {
        rollback_receipt_transaction(txn).await;
        return Err(error);
    }

    let result = dispatch_shared_activity(
        &txn,
        &activity_type,
        &actor_url_str,
        &activity,
        public_remote_id,
        follow_remote,
        content_remote,
    )
    .await;
    match result {
        Ok(status) => finish_and_commit(txn, &key, ReceiptOutcome::Accepted, status, None).await,
        Err((status, body)) if receipt_result_is_permanent(status) => {
            let message = body
                .0
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("permanent federation inbox rejection")
                .to_string();
            if let Err(error) = rollback_receipt_handler_effects(&txn).await {
                rollback_receipt_transaction(txn).await;
                return Err(error);
            }
            finish_and_commit(txn, &key, ReceiptOutcome::Rejected, status, Some(&message))
                .await
                .and(Err((status, Json(body.0))))
        }
        Err((status, body)) => {
            rollback_receipt_transaction(txn).await;
            Err((status, body))
        }
    }
}

async fn dispatch_shared_activity<C: ConnectionTrait>(
    db: &C,
    activity_type: &str,
    actor_url_str: &str,
    activity: &serde_json::Value,
    public_remote_id: Option<i32>,
    follow_remote: Option<&RemoteActorInfo>,
    content_remote: Option<&RemoteActorInfo>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    if matches!(activity_type, "Create" | "Announce") {
        if let Some(remote_id) = public_remote_id {
            distribute_to_followers(db, remote_id, activity_type, activity).await?;
            record_room_peer_activity(db, remote_id, actor_url_str, activity_type, activity)
                .await?;
        }
        return Ok(StatusCode::ACCEPTED);
    }

    // Move verification currently requires remote HTTP actor documents.  Do
    // not execute its DB migration outside the receipt transaction.
    if activity_type == "Move" {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(AppError::public_json(
                "Move handling temporarily unavailable while transactional verification is pending",
            )),
        ));
    }

    if activity_type.starts_with("myriad:")
        || matches!(
            activity_type,
            "Follow" | "Accept" | "Undo" | "Delete" | "Update" | "Like"
        )
    {
        if let Some(uid) = resolve_shared_inbox_local_user(db, activity_type, activity).await {
            if activity_type.starts_with("myriad:") {
                ensure_allowed_mfp_type(activity_type)?;
                return handle_mfp_activity(db, Some(uid), actor_url_str, activity_type, activity)
                    .await;
            }
            return match activity_type {
                "Follow" => {
                    handle_follow(
                        db,
                        uid,
                        actor_url_str,
                        activity,
                        follow_remote,
                        DeliveryMode::QueueOnly,
                    )
                    .await
                }
                "Accept" => handle_accept(db, uid, activity).await,
                "Undo" => handle_undo(db, uid, actor_url_str, activity).await,
                "Create" | "Update" | "Delete" | "Announce" | "Like" => {
                    handle_content_activity(
                        db,
                        uid,
                        actor_url_str,
                        activity_type,
                        activity,
                        content_remote,
                    )
                    .await
                }
                _ => Ok(StatusCode::ACCEPTED),
            };
        }
    }
    Ok(StatusCode::ACCEPTED)
}

/// Resolve which local user a shared-inbox activity targets.
///
/// Order:
/// 1. Local usernames in `to` / `cc` (string or first array element; also arrays)
/// 2. For **Accept**: owner of the outgoing Follow cited by object id (never guess)
/// 3. Single local user instance fallback only when not Accept (avoids wrong-user
/// Accept on multi-user hosts)
async fn resolve_shared_inbox_local_user(
    db: &impl ConnectionTrait,
    activity_type: &str,
    activity: &serde_json::Value,
) -> Option<i32> {
    // 1) to / cc — support string or array (AP allows both)
    for key in ["to", "cc"] {
        let Some(val) = activity.get(key) else {
            continue;
        };
        let urls: Vec<&str> = if let Some(s) = val.as_str() {
            vec![s]
        } else if let Some(arr) = val.as_array() {
            arr.iter().filter_map(|v| v.as_str()).collect()
        } else {
            continue;
        };
        for url in urls {
            if let Some(uid) = local_user_id_from_actorish_url(db, url).await {
                return Some(uid);
            }
        }
    }

    // 2) Accept: bind to the local user who owns the outgoing Follow activity_id
    if activity_type == "Accept" {
        let follow_id = extract_accept_object_id(activity);
        if !follow_id.is_empty() {
            if let Ok(Some(row)) = db
                .query_one_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    r#"SELECT f.user_id
                       FROM federation_follows f
                       WHERE f.direction = 'outgoing'
                         AND (
                           f.activity_id = $1
                           OR rtrim(f.activity_id, '/') = rtrim($1::text, '/')
                           OR lower(f.activity_id) = lower($1)
                           OR rtrim(split_part(split_part(f.activity_id, '?', 1), '#', 1), '/')
                              = rtrim(split_part(split_part($1::text, '?', 1), '#', 1), '/')
                         )
                       ORDER BY CASE WHEN f.status = 'pending' THEN 0 ELSE 1 END
                       LIMIT 1"#,
                    [follow_id.into()],
                ))
                .await
            {
                if let Ok(uid) = row.try_get::<i32>("", "user_id") {
                    if uid > 0 {
                        return Some(uid);
                    }
                }
            }
        }
        // No unambiguous Follow owner — do not fall through to first user.
        return None;
    }

    // 3) Single-tenant convenience fallback (not for Accept).
    // Only when the instance genuinely has exactly one user. The previous
    // `ORDER BY id LIMIT 1` silently handed unaddressed activities to the
    // oldest account on multi-user hosts.
    if let Ok(rows) = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id FROM users ORDER BY id LIMIT 2",
            [],
        ))
        .await
    {
        if rows.len() == 1 {
            return rows[0].try_get::<i32>("", "id").ok();
        }
        tracing::warn!(
            activity_type,
            "Shared inbox activity has no resolvable local recipient and the instance \
             has more than one user; refusing first-user fallback"
        );
    }
    None
}

/// Resolve a local user id from an actor-ish URL.
///
/// 只认 `{base_url}/users/{username}`。远端 `https://evil.example/users/alice`
/// 不得落到本地 `alice`。
async fn local_user_id_from_actorish_url(db: &impl ConnectionTrait, url: &str) -> Option<i32> {
    let base = get_base_url().await;
    let local = local_username_from_actor_url(&base, url)?;
    if let Ok(Some(row)) = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id FROM users WHERE username = $1",
            [local.into()],
        ))
        .await
    {
        return row.try_get::<i32>("", "id").ok();
    }
    None
}

pub(crate) async fn get_local_user(
    db: &impl ConnectionTrait,
    username: &str,
) -> Result<(i32, String), (StatusCode, Json<serde_json::Value>)> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id, username FROM users WHERE username = $1 LIMIT 1",
            [username.into()],
        ))
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(AppError::public_json("User not found")),
            )
        })?;

    let id = row_positive_id(&row, "id").map_err(|error| {
        db_err(sea_orm::DbErr::Custom(error))
    })?;
    let username: String = row.try_get("", "username").map_err(db_err)?;
    if username.is_empty() {
        return Err(db_err(sea_orm::DbErr::Custom(
            "user username is empty".into(),
        )));
    }
    Ok((id, username))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    #[test]
    fn verified_actor_promotion_failure_is_retryable_and_opaque() {
        let failed: Result<RemoteActorInfo, String> =
            Err("duplicate key value violates unique constraint \"secret_idx\"".into());
        let (status, body) = require_verified_actor(&failed).unwrap_err();
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert!(!receipt_result_is_permanent(status), "must not be durably rejected");
        assert_eq!(body.0["retry"], true);
        assert!(!body.0.to_string().contains("secret_idx"), "no internal detail");

        let ok: Result<RemoteActorInfo, String> = Ok(RemoteActorInfo {
            id: 42,
            actor_url: "https://a.example/users/alice".into(),
            username: Some("alice".into()),
            domain: "a.example".into(),
            display_name: None,
            avatar_url: None,
            inbox_url: "https://a.example/users/alice/inbox".into(),
            public_key_pem: None,
            public_key_id: None,
            mfp_version: None,
        });
        assert_eq!(require_verified_actor(&ok).unwrap().id, 42);
    }

    #[test]
    fn room_join_preflight_targets_the_joining_member() {
        let activity = serde_json::json!({
            "type": "myriad:RoomJoin",
            "actor": "https://owner.example/users/alice",
            "object": {"member": "https://member.example/users/bob"}
        });
        assert_eq!(
            super::room_join_member_actor(
                "myriad:RoomJoin",
                "https://owner.example/users/alice",
                &activity,
            )
            .as_deref(),
            Some("https://member.example/users/bob")
        );
        assert_eq!(
            super::room_join_member_actor("Create", "https://owner.example/users/alice", &activity),
            None
        );
    }

    #[test]
    fn receipt_replay_does_not_echo_sql() {
        let err = receipt_rejected(
            403,
            Some(
                "claim inbound receipt insert: relation \"federation_inbox_receipts\" does not exist",
            ),
        );
        assert_eq!(err.0, StatusCode::FORBIDDEN);
        assert_eq!(
            err.1.0.get("error").and_then(|v| v.as_str()),
            Some("Inbox processing failed")
        );
        let ownership = receipt_rejected(403, Some("Object ownership check failed"));
        assert_eq!(
            ownership.1.0.get("error").and_then(|v| v.as_str()),
            Some("Access denied")
        );
    }
}
