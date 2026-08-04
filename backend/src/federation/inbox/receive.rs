
use axum::{
    extract::{Path, State},
    http::{HeaderMap, Request, StatusCode},
    Json,
};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::json;

use crate::federation::actor::{
    fetch_remote_actor, fetch_remote_actor_for_verify, persist_verified_remote_actor,
    ResolvedRemoteActor,
};
use crate::federation::errors::{is_permanent_federation_error, map_inbox_handler_error};
use crate::federation::limits::buffer_inbox_body;
use crate::federation::replay::{is_replay_or_record, replay_dedup_keys};
use crate::federation::signature::{
    parse_signature_header, require_covered_headers, verify_date_freshness, verify_digest,
    verify_signature, HTTP_DATE_MAX_SKEW,
};
use crate::federation::types::*;

/// Map handler errors to HTTP status; permanent peer-state mismatches → 4xx.
fn inbox_err(context: &str, e: String) -> (StatusCode, Json<serde_json::Value>) {
    if is_permanent_federation_error(&e) {
        tracing::warn!("{}: {}", context, e);
    } else {
        tracing::error!("{}: {}", context, e);
    }
    let (status, body) = map_inbox_handler_error(e);
    (status, body)
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
    // MYR-002: reserve concurrent raw-body budget *before* buffering; release on drop.
    // Exhausted budget → 429 (does not lower INBOX_BODY_LIMIT).
    let (body, _inflight) = buffer_inbox_body(request).await?;

    // 先做只依赖 header/原始字节的检查，再解析 body（inbox 上限见 federation::limits::INBOX_BODY_LIMIT）
    verify_preparse_gate(&headers, &body)?;

    // 解析 Activity JSON
    let activity: serde_json::Value = serde_json::from_slice(&body).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Invalid JSON body"})),
        )
    })?;

    let activity_type = activity["type"].as_str().unwrap_or("").to_string();
    // Actor may be a string IRI or expanded object `{ "id": "...", "type": "Person" }`.
    let actor_url_str = extract_activity_actor_id(&activity);

    if actor_url_str.is_empty() || activity_type.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Missing actor or type in activity"})),
        ));
    }

    // 验证 HTTP Signature（MYR-022: actor fetch is ephemeral until verified）
    let request_path = format!("/users/{}/inbox", username);
    verify_request_signature(&db, &headers, &body, &actor_url_str, &request_path).await?;

    // MYR-023: short-lived activity id / digest dedup after successful auth.
    // Legitimate peer retries get 202 without re-running side-effect handlers.
    let activity_id = activity["id"].as_str().unwrap_or("");
    if is_replay_or_record(&replay_dedup_keys(activity_id, &body)) {
        tracing::info!(
            activity_id = %activity_id,
            activity_type = %activity_type,
            actor = %actor_url_str,
            "📬 Inbox replay suppressed (activity id / body digest seen recently)"
        );
        return Ok(StatusCode::ACCEPTED);
    }

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
            Json(json!({"error": "Rejected by trust policy", "reason": reason})),
        ));
    }

    tracing::info!(
        "📬 Inbox received: type={}, actor={}, target_user={}",
        activity_type,
        actor_url_str,
        username
    );

    // 分发处理
    match activity_type.as_str() {
        "Follow" => handle_follow(&db, user_id, &actor_url_str, &activity).await,
        "Accept" => handle_accept(&db, user_id, &activity).await,
        "Reject" => handle_reject(&db, user_id, &actor_url_str, &activity).await,
        "Undo" => handle_undo(&db, user_id, &actor_url_str, &activity).await,
        "Move" => handle_move(&db, &actor_url_str, &activity).await,
        "Create" | "Update" | "Delete" | "Announce" | "Like" => {
            handle_content_activity(&db, user_id, &actor_url_str, &activity_type, &activity).await
        }
        // MFP 扩展类型（白名单验证）
        ty if ty.starts_with("myriad:") => {
            const ALLOWED_MFP_TYPES: &[&str] = &[
                "myriad:ChannelOpen",
                "myriad:ChannelClose",
                "myriad:ChannelAccept",
                "myriad:ChannelMessage",
                "myriad:RoomInvite",
                "myriad:RoomJoin",
                "myriad:RoomLeave",
                "myriad:RoomDissolve",
                "myriad:RoomMessage",
                "myriad:RoomPin",
                "myriad:RoomGovernance",
                "myriad:RingJoin",
                "myriad:RingSync",
                "myriad:RingLeave",
                "myriad:FileTransfer",
                "myriad:KeyExchange",
                // Digital Life 角色互访（myriad:CharacterVisit*）随该功能一并移出。
                //
                // 白名单和分派必须同进同退：只加白名单会让这些活动通过验签、
                // 拿到 202 Accepted，然后因为没有处理器被静默丢弃 —— 远端据此
                // 认为投递成功、不再重试。现在不在白名单里，inbox 返回 400
                // "Unknown MFP activity type"，行为是诚实的。
                //
                // 功能回归时，这 6 个类型与 handle_mfp_activity 里的分派分支
                // 必须在同一次改动里一起加回来。
            ];
            if !ALLOWED_MFP_TYPES.contains(&ty) {
                tracing::warn!("Rejected unknown MFP activity type: {}", ty);
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": format!("Unknown MFP activity type: {}", ty)})),
                ));
            }
            handle_mfp_activity(&db, Some(user_id), &actor_url_str, ty, &activity).await
        }
        _ => {
            tracing::warn!("Unsupported activity type: {}", activity_type);
            Ok(StatusCode::ACCEPTED) // AP 规范建议静默接受未知类型
        }
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
    // MYR-002: reserve concurrent raw-body budget *before* buffering; release on drop.
    // Exhausted budget → 429 (does not lower INBOX_BODY_LIMIT).
    let (body, _inflight) = buffer_inbox_body(request).await?;

    // 先做只依赖 header/原始字节的检查，再解析 body（inbox 上限见 federation::limits::INBOX_BODY_LIMIT）
    verify_preparse_gate(&headers, &body)?;

    let activity: serde_json::Value = serde_json::from_slice(&body).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Invalid JSON body"})),
        )
    })?;

    let activity_type = activity["type"].as_str().unwrap_or("").to_string();
    let actor_url_str = extract_activity_actor_id(&activity);

    if actor_url_str.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Missing actor in activity"})),
        ));
    }

    // 验证签名（MYR-022: actor fetch is ephemeral until verified）
    verify_request_signature(&db, &headers, &body, &actor_url_str, "/inbox").await?;

    // MYR-023: short-lived activity id / digest dedup after successful auth.
    let activity_id = activity["id"].as_str().unwrap_or("");
    if is_replay_or_record(&replay_dedup_keys(activity_id, &body)) {
        tracing::info!(
            activity_id = %activity_id,
            activity_type = %activity_type,
            actor = %actor_url_str,
            "📬 Shared inbox replay suppressed (activity id / body digest seen recently)"
        );
        return Ok(StatusCode::ACCEPTED);
    }

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
            Json(json!({"error": "Rejected by trust policy", "reason": reason})),
        ));
    }

    tracing::info!(
        "📬 Shared inbox received: type={}, actor={}",
        activity_type,
        actor_url_str
    );

    // 公开内容 → 粉丝时间线
    if matches!(activity_type.as_str(), "Create" | "Announce") {
        // 只有寻址到 Public 或该 Actor 自己 followers collection 的活动才能进入
        // 粉丝首页。定向给具体个人（甚至完全未寻址）的活动过去也会被广播给
        // 该 Actor 的全部本地粉丝。
        if !crate::federation::audience::may_distribute_to_followers(&activity, &actor_url_str) {
            tracing::warn!(
                actor = %actor_url_str,
                activity_type = %activity_type,
                "Shared inbox activity is not addressed to Public or the actor's followers; not distributing"
            );
            // 静默接受：投递方无需知道我们的分发决策，重投也无意义。
            return Ok(StatusCode::ACCEPTED);
        }

        let remote = fetch_remote_actor(&db, &actor_url_str).await.map_err(|e| {
            tracing::warn!("Failed to fetch remote actor {}: {}", actor_url_str, e);
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Unknown actor"})),
            )
        })?;

        // Create 还需要证明内嵌对象确实属于签名 Actor（Announce 的对象本来
        // 就是别人的，不能套用同一条规则）。
        if activity_type == "Create" {
            if let Err(e) = crate::federation::audience::verify_object_ownership(
                &actor_url_str,
                &activity["object"],
            ) {
                tracing::warn!(actor = %actor_url_str, "Shared inbox Create rejected: {}", e);
                return Err((
                    StatusCode::FORBIDDEN,
                    Json(
                        json!({"error": "Object ownership check failed", "reason": e.to_string()}),
                    ),
                ));
            }
        }

        distribute_to_followers(&db, remote.id, &activity_type, &activity).await?;
        return Ok(StatusCode::ACCEPTED);
    }

    // Move is not user-targeted: re-point local follow graph for the migrating remote.
    if activity_type == "Move" {
        return handle_move(&db, &actor_url_str, &activity).await;
    }

    // MFP / social: route through same handlers as personal inbox when addressed
    // to a local user (to/cc) or when object has room/channel ids.
    if activity_type.starts_with("myriad:")
        || matches!(
            activity_type.as_str(),
            "Follow" | "Accept" | "Undo" | "Delete" | "Update" | "Like"
        )
    {
        let target_user_id = resolve_shared_inbox_local_user(&db, &activity_type, &activity).await;
        if let Some(uid) = target_user_id {
            if activity_type.starts_with("myriad:") {
                return handle_mfp_activity(
                    &db,
                    Some(uid),
                    &actor_url_str,
                    &activity_type,
                    &activity,
                )
                .await;
            }
            return match activity_type.as_str() {
                "Follow" => handle_follow(&db, uid, &actor_url_str, &activity).await,
                "Accept" => handle_accept(&db, uid, &activity).await,
                "Undo" => handle_undo(&db, uid, &actor_url_str, &activity).await,
                "Create" | "Update" | "Delete" | "Announce" | "Like" => {
                    handle_content_activity(&db, uid, &actor_url_str, &activity_type, &activity)
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
    db: &DatabaseConnection,
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
/// Exact `{base_url}/users/{username}` form only. The previous implementation
/// fell back to "last path segment" for any URL, so a remote
/// `https://evil.example/users/alice` resolved to the **local** `alice`.
async fn local_user_id_from_actorish_url(db: &DatabaseConnection, url: &str) -> Option<i32> {
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

// Activity 处理器

/// Handle ActivityPub Move (domain / account migration).
///
/// Fail-closed unless **all** pass:
/// 1. HTTP Signature already verified (caller)
/// 2. `actor` == signed actor == `object` (old id); `target` present and distinct
/// 3. Fresh fetch of old actor has `movedTo` == target
/// 4. Fresh fetch of new actor has `alsoKnownAs` containing old id
///
/// On accept: re-point local `federation_follows` from old remote actor → new.
async fn handle_move(
    db: &DatabaseConnection,
    signed_actor: &str,
    activity: &serde_json::Value,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    use crate::federation::move_actor::{
        fetch_actor_document, migrate_follows_old_to_new, verify_move_structure,
        verify_new_actor_also_known_as, verify_old_actor_moved_to,
    };

    let (old_actor, new_actor) = verify_move_structure(activity, signed_actor).map_err(|e| {
        tracing::warn!("Move rejected (structure): {}", e);
        (StatusCode::BAD_REQUEST, Json(json!({"error": e})))
    })?;

    let old_doc = fetch_actor_document(db, &old_actor).await.map_err(|e| {
        tracing::warn!("Move rejected (old actor fetch): {}", e);
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": format!("Cannot fetch old actor for Move verify: {}", e)})),
        )
    })?;

    verify_old_actor_moved_to(&old_doc, &old_actor, &new_actor).map_err(|e| {
        tracing::warn!("Move rejected (movedTo): {}", e);
        (StatusCode::BAD_REQUEST, Json(json!({"error": e})))
    })?;

    let new_doc = fetch_actor_document(db, &new_actor).await.map_err(|e| {
        tracing::warn!("Move rejected (new actor fetch): {}", e);
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": format!("Cannot fetch new actor for Move verify: {}", e)})),
        )
    })?;

    verify_new_actor_also_known_as(&new_doc, &new_actor, &old_actor).map_err(|e| {
        tracing::warn!("Move rejected (alsoKnownAs): {}", e);
        (StatusCode::BAD_REQUEST, Json(json!({"error": e})))
    })?;

    let migrated = migrate_follows_old_to_new(db, &old_actor, &new_actor)
        .await
        .map_err(|e| inbox_err("Move follow migration failed", e))?;

    // Record inbound Move for audit
    let activity_id = activity["id"].as_str().unwrap_or("").to_string();
    if !activity_id.is_empty() {
        let _ = db
            .execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"INSERT INTO federation_activities
                       (activity_id, activity_type, object_type, object_json, is_local, received_at, published_at)
                   VALUES ($1, 'Move', 'Person', $2, false, NOW(), NOW())
                   ON CONFLICT (activity_id) DO NOTHING"#,
                [activity_id.clone().into(), activity.clone().into()],
            ))
            .await;
    }

    tracing::info!(
        old_actor = %old_actor,
        new_actor = %new_actor,
        migrated_follows = migrated,
        "Accepted ActivityPub Move"
    );

    Ok(StatusCode::ACCEPTED)
}

/// 处理 Follow 请求
async fn handle_follow(
    db: &DatabaseConnection,
    local_user_id: i32,
    actor_url_str: &str,
    activity: &serde_json::Value,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    // Follow.object 必须就是本次要记录的本地 Actor。
    //
    // 过去只用投递路径（/users/{name}/inbox 或 sharedInbox 的收件人解析）决定
    // local_user_id，从不看 activity.object，于是向 bob 的 inbox POST 一条
    // `Follow{object: ".../users/alice"}` 会给 **bob** 记上一个粉丝，随后
    // 发出的 Accept 里 object 还被重写成 bob 的 Actor。
    let base_url = get_base_url().await;
    let local_username = get_username_by_id(db, local_user_id).await?;
    let local_actor_url = actor_url(&base_url, &local_username);

    let follow_target = crate::federation::audience::object_id(&activity["object"]);
    match follow_target.as_deref() {
        Some(target) if same_actor_url(target, &local_actor_url) => {}
        Some(target) => {
            tracing::warn!(
                actor = %actor_url_str,
                target,
                expected = %local_actor_url,
                "Follow rejected: object does not match the local actor being followed"
            );
            return Err((
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(json!({
                    "error": "Follow.object does not match this actor",
                    "expected": local_actor_url,
                    "found": target,
                })),
            ));
        }
        None => {
            tracing::warn!(actor = %actor_url_str, "Follow rejected: missing object");
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Follow.object is required"})),
            ));
        }
    }

    // 获取或缓存远程 Actor
    let remote = fetch_remote_actor(db, actor_url_str).await.map_err(|e| {
        tracing::warn!("Failed to fetch actor for Follow: {}", e);
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Cannot resolve remote actor"})),
        )
    })?;

    let activity_id = activity["id"].as_str().unwrap_or("").to_string();

    // Record incoming follow. On Postgres, ON CONFLICT DO UPDATE always reports
    // rows_affected >= 1 even when the row was already accepted — so the old
    // `rows_affected == 0` idempotency check never fired and re-enqueued Accept.
    // Pattern: conditional UPDATE + RETURNING; empty result means already accepted.
    let upserted = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"INSERT INTO federation_follows (user_id, remote_actor_id, direction, status, activity_id, created_at)
               VALUES ($1, $2, 'incoming', 'accepted', $3, NOW())
               ON CONFLICT (user_id, remote_actor_id, direction) DO UPDATE SET
                   status = 'accepted',
                   activity_id = EXCLUDED.activity_id
               WHERE federation_follows.status IS DISTINCT FROM 'accepted'
               RETURNING id"#,
            [
                local_user_id.into(),
                remote.id.into(),
                activity_id.clone().into(),
            ],
        ))
        .await
        .map_err(db_err)?;

    if upserted.is_none() {
        // Already accepted — idempotent 202, no second Accept / enqueue / notify
        tracing::debug!(
            activity_id,
            "Ignoring already-accepted Follow (no second Accept)"
        );
        return Ok(StatusCode::ACCEPTED);
    }

    // 自动发送 Accept（Myriad 个人实例默认自动接受）
    // base_url / local_username / local_actor_url 已在入口处校验 Follow.object 时取得。

    // Embed a minimal Follow object (type/id/actor/object). Full inbound JSON
    // may omit id or carry extra @context noise that confuses remote Accept matching.
    let follow_activity_id = activity["id"].as_str().unwrap_or("").to_string();
    let accept_object = json!({
        "type": "Follow",
        "id": &follow_activity_id,
        "actor": actor_url_str,
        "object": &local_actor_url,
    });

    let accept = Activity {
        context: build_ap_context(),
        activity_type: "Accept".to_string(),
        id: generate_activity_id(&base_url),
        actor: local_actor_url.clone(),
        to: Some(vec![actor_url_str.to_string()]),
        cc: None,
        published: Some(now_iso8601()),
        object: accept_object,
        target: None,
    };

    // 入队投递
    enqueue_delivery(db, local_user_id, &accept, &remote.inbox_url).await?;

    // 新粉丝通知
    let follower_label = crate::federation::notify::actor_label(db, actor_url_str).await;
    crate::federation::notify::notify_new_follower(local_user_id, actor_url_str, &follower_label)
        .await;

    tracing::info!("✅ Follow accepted: {} → {}", actor_url_str, local_username);

    Ok(StatusCode::ACCEPTED)
}

/// 处理 Reject（Room 邀请被拒绝等）
async fn handle_reject(
    db: &DatabaseConnection,
    _local_user_id: i32,
    actor_url_str: &str,
    activity: &serde_json::Value,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    let object = &activity["object"];
    let inner_type = object.get("type").and_then(|v| v.as_str()).unwrap_or("");
    if inner_type == "myriad:RoomInvite"
        || inner_type == "myriad:Room"
        || object.get("room").is_some()
    {
        crate::federation::room::handle_room_invite_reject(db, actor_url_str, activity)
            .await
            .map_err(|e| inbox_err("Room invite Reject handling failed", e))?;
    } else {
        tracing::debug!("Ignoring Reject for unsupported object type={}", inner_type);
    }
    Ok(StatusCode::ACCEPTED)
}

/// Extract actor id from an ActivityPub activity.
///
/// Supports string IRI and expanded object `{ "id": "…", "type": "Person" }`.
/// Some peers embed the actor document; treating only strings rejects valid
/// Accept/Follow payloads even when the HTTP Signature keyId is correct.
pub fn extract_activity_actor_id(activity: &serde_json::Value) -> String {
    extract_iri_or_object_id(&activity["actor"])
}

/// Extract an IRI from a JSON value that may be a string, object with `id`/`href`,
/// or a single-level array of either (ActivityStreams multi-value).
fn extract_iri_or_object_id(value: &serde_json::Value) -> String {
    if let Some(s) = value.as_str() {
        return s.trim().to_string();
    }
    if let Some(id) = value.get("id").and_then(|v| v.as_str()) {
        return id.trim().to_string();
    }
    // AS2 Link objects use `href` rather than `id`.
    if let Some(href) = value.get("href").and_then(|v| v.as_str()) {
        return href.trim().to_string();
    }
    if let Some(arr) = value.as_array() {
        for item in arr {
            if let Some(s) = item.as_str() {
                let t = s.trim();
                if !t.is_empty() {
                    return t.to_string();
                }
            }
            if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                let t = id.trim();
                if !t.is_empty() {
                    return t.to_string();
                }
            }
            if let Some(href) = item.get("href").and_then(|v| v.as_str()) {
                let t = href.trim();
                if !t.is_empty() {
                    return t.to_string();
                }
            }
        }
    }
    String::new()
}

/// Extract the accepted object id from an Accept activity.
///
/// Supports:
/// - `object`: string activity/object id
/// - `object`: nested `{ "id", "type", ... }` (Follow / ChannelOpen / …)
/// - `object`: AS2 Link `{ "href": "…" }`
/// - `object`: array of the above (first non-empty id)
///
/// Does not walk arbitrary nesting beyond one object / array level.
pub fn extract_accept_object_id(activity: &serde_json::Value) -> String {
    extract_iri_or_object_id(&activity["object"])
}

/// Nested object type for Accept routing (Channel vs Follow).
/// Empty when object is a string id or Link-only. Arrays: first typed element.
fn extract_accept_object_type(activity: &serde_json::Value) -> String {
    let object = &activity["object"];
    if let Some(t) = object.get("type").and_then(|v| v.as_str()) {
        return t.to_string();
    }
    if let Some(arr) = object.as_array() {
        for item in arr {
            if let Some(t) = item.get("type").and_then(|v| v.as_str()) {
                if !t.is_empty() {
                    return t.to_string();
                }
            }
        }
    }
    String::new()
}

/// 处理 Accept（我们发出的 Follow 被接受）
///
/// 授权绑定：状态变更仅在 Accept 的签名 actor 正是该 Channel/Follow 的
/// 远程对端时生效，防止第三方实例伪造他人的 Accept。
/// Actor 比对走 `same_actor_url`（host 大小写 / trailing slash），不依赖 SQL 字节级相等。
async fn handle_accept(
    db: &DatabaseConnection,
    local_user_id: i32,
    activity: &serde_json::Value,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    // 签名校验保证了 activity.actor 就是本次请求的签名者
    // (string IRI or expanded object `{id}`; see extract_activity_actor_id)
    let accept_actor = extract_activity_actor_id(activity);
    // Accept.object: string id OR nested Follow/ChannelOpen {id,type,…}
    let inner_type = extract_accept_object_type(activity);
    let follow_id = extract_accept_object_id(activity);

    if inner_type == "myriad:ChannelOpen" || inner_type == "myriad:Channel" {
        // 远程方接受了我们的 Channel 开启请求
        let channel_id = follow_id.as_str(); // object.id 就是 channel_id
        if !channel_id.is_empty() {
            // 先取 pending channel 的远程对端 URL，再在 Rust 侧用 same_actor_url 授权
            let pending = db
                .query_one_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    r#"SELECT ra.actor_url
                       FROM federation_channels c
                       JOIN federation_remote_actors ra ON c.remote_actor_id = ra.id
                       WHERE c.channel_id = $1 AND c.user_id = $2 AND c.status = 'pending'"#,
                    [channel_id.into(), local_user_id.into()],
                ))
                .await
                .map_err(db_err)?;

            let authorized = pending
                .as_ref()
                .and_then(|row| row.try_get::<String>("", "actor_url").ok())
                // Align with Follow Accept: host+username case / path form drift.
                .map(|remote_url| same_actor_or_user(&accept_actor, &remote_url))
                .unwrap_or(false);

            if authorized {
                let result = db
                    .execute_raw(Statement::from_sql_and_values(
                        DatabaseBackend::Postgres,
                        r#"UPDATE federation_channels
                           SET status = 'accepted', last_activity_at = NOW()
                           WHERE channel_id = $1 AND user_id = $2 AND status = 'pending'"#,
                        [channel_id.into(), local_user_id.into()],
                    ))
                    .await
                    .map_err(db_err)?;

                if result.rows_affected() > 0 {
                    let remote_url = pending
                        .and_then(|row| row.try_get::<String>("", "actor_url").ok())
                        .unwrap_or_default();
                    let remote_label =
                        crate::federation::notify::actor_label(db, &remote_url).await;
                    crate::federation::notify::notify_channel_accepted(
                        local_user_id,
                        channel_id,
                        &remote_label,
                    )
                    .await;
                    // Unlock initiator's live Aro client (composer was pending-locked).
                    // Mirrors handle_channel_accept WS path for myriad:ChannelAccept.
                    crate::federation::ws_gateway::broadcast_to_channel(
                        channel_id,
                        &serde_json::json!({
                            "type": "channel_accepted",
                            "channel_id": channel_id
                        }),
                    )
                    .await;
                    tracing::info!("✅ Channel accepted: {}", channel_id);
                }
            } else {
                tracing::debug!(
                    "Channel accept no-op (not pending, not owner, or actor mismatch): channel={} accept_actor={}",
                    channel_id,
                    accept_actor
                );
            }
        }
    } else {
        // Standard Follow Accept — match Follow object.id, then fallback to a single
        // pending outgoing toward Accept.actor (same_actor_url). Idempotent notify.
        handle_follow_accept(db, local_user_id, &accept_actor, &follow_id, activity).await?;
    }

    Ok(StatusCode::ACCEPTED)
}

/// Loose actor match: exact `same_actor_url`, or same host+port + `/users/{name}`.
///
/// Handles path-capitalization / alias drift between WebFinger-resolved URLs and
/// Accept.actor built from the remote's base_url + preferredUsername.
fn same_actor_or_user(left: &str, right: &str) -> bool {
    if same_actor_url(left, right) {
        return true;
    }
    let l = normalize_actor_url(left);
    let r = normalize_actor_url(right);
    let (l_host, l_user) = split_actor_host_user(&l);
    let (r_host, r_user) = split_actor_host_user(&r);
    !l_host.is_empty()
        && l_host == r_host
        && !l_user.is_empty()
        && l_user.eq_ignore_ascii_case(&r_user)
}

fn split_actor_host_user(normalized: &str) -> (String, String) {
    // normalized form: scheme://host[:port]/users/name
    let Ok(url) = url::Url::parse(normalized) else {
        return (String::new(), String::new());
    };
    let host = match (url.host_str(), url.port()) {
        (Some(h), Some(p)) => format!("{}:{}", h.to_ascii_lowercase(), p),
        (Some(h), None) => h.to_ascii_lowercase(),
        _ => String::new(),
    };
    let path = url.path().trim_end_matches('/');
    let user = path
        .strip_prefix("/users/")
        .filter(|rest| !rest.is_empty() && !rest.contains('/'))
        .unwrap_or("")
        .to_string();
    (host, user)
}

/// Same host + username-compatible for Accept authorization under path drift.
///
/// Used when `same_actor_or_user` fails (e.g. Accept.actor is `/@bob` while the
/// stored remote is `/users/bob`) but we must **never** let a different
/// `/users/{name}` on the same host accept someone else's Follow.
fn same_host_username_compatible(accept_actor: &str, remote: &str) -> bool {
    let an = normalize_actor_url(accept_actor);
    let rn = normalize_actor_url(remote);
    let (ah, a_user) = split_actor_host_user(&an);
    let (rh, r_user) = split_actor_host_user(&rn);
    if ah.is_empty() || ah != rh {
        return false;
    }
    // Both standard `/users/{name}`: require same username (case-insensitive).
    // (Normally covered by `same_actor_or_user`; kept for defense-in-depth.)
    if !a_user.is_empty() && !r_user.is_empty() {
        return a_user.eq_ignore_ascii_case(&r_user);
    }
    // Accept uses a non-`/users/` path form (e.g. `/@bob`, `/ap/users/bob`):
    // last path segment (strip leading `@`) must match the stored remote user.
    if a_user.is_empty() && !r_user.is_empty() {
        if let Ok(url) = url::Url::parse(&an) {
            let last = url
                .path()
                .trim_end_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or("")
                .trim_start_matches('@');
            return !last.is_empty() && last.eq_ignore_ascii_case(&r_user);
        }
    }
    false
}

/// Resolve which outgoing Follow an Accept refers to.
///
/// Matching order (safe, no silent multi-pick):
/// 1. Normalized `activity_id` + Accept.actor is remote peer (`same_actor_or_user`)
/// 2. Unique normalized `activity_id` + **same host + username-compatible**
/// (path/alias drift only — never cross-user on the same host, never
/// cross-host username-only)
/// 3. Exactly one **pending** outgoing to Accept.actor via `same_actor_or_user`
///
/// Already-`accepted` rows only yield idempotent success when matched by id
/// (or unique id+host+user). Ambiguous multi-pending → no match.
///
/// Returns `(activity_id, remote_actor_url, already_accepted)`.
pub fn resolve_follow_accept_target(
    follow_id: &str,
    accept_actor: &str,
    candidates: &[(String, String, String)], // activity_id, remote_actor_url, status
) -> Option<(String, String, bool)> {
    let follow_norm = normalize_activity_id(follow_id);
    if !follow_norm.is_empty() {
        // Prefer actor-authorized id match (host+user or same_actor_url)
        for (aid, remote, status) in candidates {
            if same_activity_id(aid, &follow_norm) && same_actor_or_user(accept_actor, remote) {
                return Some((aid.clone(), remote.clone(), status == "accepted"));
            }
        }
        // Unique id match with same host **and** username-compatible path drift.
        // Host-only was insufficient: multi-user instances share a host, and
        // activity ids can leak; a different /users/{name} must not Accept.
        let id_hits: Vec<_> = candidates
            .iter()
            .filter(|(aid, remote, _)| {
                same_activity_id(aid, &follow_norm)
                    && same_host_username_compatible(accept_actor, remote)
            })
            .collect();
        if id_hits.len() == 1 {
            let (aid, remote, status) = id_hits[0];
            tracing::info!(
                follow_id = follow_id,
                activity_id = %aid,
                accept_actor = accept_actor,
                remote = %remote,
                "Follow Accept: unique activity_id + host+user match (actor path drift)"
            );
            return Some((aid.clone(), remote.clone(), status == "accepted"));
        }
        if id_hits.len() > 1 {
            tracing::warn!(
                follow_id = follow_id,
                accept_actor = accept_actor,
                hits = id_hits.len(),
                "Follow Accept ambiguous: multiple activity_id matches on same host+user"
            );
        }
    }

    // Fallback: only pending + same_actor_or_user (same host+user). Never
    // username-only across hosts (same_actor_or_user enforces host).
    let pending_to_actor: Vec<_> = candidates
        .iter()
        .filter(|(_, remote, status)| {
            *status == "pending" && same_actor_or_user(accept_actor, remote)
        })
        .collect();
    if pending_to_actor.len() == 1 {
        let (aid, remote, _) = pending_to_actor[0];
        tracing::info!(
            follow_id = follow_id,
            activity_id = %aid,
            accept_actor = accept_actor,
            "Follow Accept fallback: matched single pending outgoing to Accept actor"
        );
        return Some((aid.clone(), remote.clone(), false));
    }

    if pending_to_actor.len() > 1 {
        tracing::warn!(
            follow_id = follow_id,
            accept_actor = accept_actor,
            candidates = pending_to_actor.len(),
            "Follow Accept fallback ambiguous: multiple pending outgoing to Accept actor"
        );
    }
    None
}

async fn handle_follow_accept(
    db: &DatabaseConnection,
    local_user_id: i32,
    accept_actor: &str,
    follow_id: &str,
    activity: &serde_json::Value,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if accept_actor.is_empty() {
        tracing::warn!(
            activity_id = %activity["id"].as_str().unwrap_or(""),
            "Follow Accept missing actor; ignoring"
        );
        return Ok(());
    }

    // Candidate window (not full-table scan):
    // - all pending outgoing for this user (usually few)
    // - accepted rows matching activity_id variants (idempotent re-Accept)
    // - recent accepted (last 40) so normalized id compare can still hit after
    // query/host-case drift without scanning all history
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT f.activity_id, f.status, ra.actor_url
               FROM federation_follows f
               JOIN federation_remote_actors ra ON f.remote_actor_id = ra.id
               WHERE f.user_id = $1 AND f.direction = 'outgoing'
                 AND (
                   f.status = 'pending'
                   OR (
                     f.status = 'accepted'
                     AND (
                       ($2 <> '' AND f.activity_id = $2)
                       OR ($2 <> '' AND rtrim(f.activity_id, '/') = rtrim($2::text, '/'))
                       OR ($2 <> '' AND lower(f.activity_id) = lower($2))
                       OR ($2 <> '' AND rtrim(split_part(split_part(f.activity_id, '?', 1), '#', 1), '/')
                           = rtrim(split_part(split_part($2::text, '?', 1), '#', 1), '/'))
                       OR f.id IN (
                         SELECT f2.id FROM federation_follows f2
                         WHERE f2.user_id = $1 AND f2.direction = 'outgoing'
                           AND f2.status = 'accepted'
                         ORDER BY COALESCE(f2.accepted_at, f2.created_at) DESC NULLS LAST
                         LIMIT 40
                       )
                     )
                   )
                 )
               LIMIT 120"#,
            [local_user_id.into(), follow_id.into()],
        ))
        .await
        .map_err(db_err)?;

    let candidates: Vec<(String, String, String)> = rows
        .iter()
        .map(|r| {
            (
                r.try_get::<String>("", "activity_id").unwrap_or_default(),
                r.try_get::<String>("", "actor_url").unwrap_or_default(),
                r.try_get::<String>("", "status").unwrap_or_default(),
            )
        })
        .filter(|(aid, remote, _)| !aid.is_empty() && !remote.is_empty())
        .collect();

    let Some((matched_id, remote_url, already_accepted)) =
        resolve_follow_accept_target(follow_id, accept_actor, &candidates)
    else {
        tracing::warn!(
            follow_id = follow_id,
            accept_actor = accept_actor,
            candidates = candidates.len(),
            "Follow Accept no match (activity_id + fallback failed)"
        );
        return Ok(());
    };

    if already_accepted {
        tracing::debug!(
            activity_id = %matched_id,
            accept_actor = accept_actor,
            "Follow Accept idempotent: already accepted"
        );
        return Ok(());
    }

    let result = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"UPDATE federation_follows
               SET status = 'accepted', accepted_at = NOW()
               WHERE user_id = $1 AND direction = 'outgoing' AND activity_id = $2
                 AND status IS DISTINCT FROM 'accepted'"#,
            [local_user_id.into(), matched_id.clone().into()],
        ))
        .await
        .map_err(db_err)?;

    if result.rows_affected() == 0 {
        tracing::debug!(
            activity_id = %matched_id,
            "Follow Accept race: row already accepted"
        );
        return Ok(());
    }

    let label = crate::federation::notify::actor_label(db, &remote_url).await;
    crate::federation::notify::notify_follow_accepted(local_user_id, accept_actor, &label).await;
    tracing::info!(
        activity_id = %matched_id,
        accept_actor = accept_actor,
        follow_object_id = follow_id,
        "✅ Our follow accepted"
    );
    Ok(())
}

/// 处理 Undo（包括 Undo Follow / Like / Announce）
async fn handle_undo(
    db: &DatabaseConnection,
    local_user_id: i32,
    actor_url_str: &str,
    activity: &serde_json::Value,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    let inner_type = activity["object"]["type"].as_str().unwrap_or("");

    match inner_type {
        "Follow" => {
            // 远程用户取消关注。
            //
            // 字节级相等和本文件其余地方的 same_actor_url 语义不一致：末尾斜杠或
            // host 大小写一变，取关就静默成功（202）而粉丝关系还留着 —— 对面以为
            // 已经取关，我们这边还在往它的 inbox 投递。快路径保留精确匹配
            // （actor_url 有唯一约束，命中即唯一），未命中时再走规范化兜底。
            let removed = db
                .execute_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    r#"DELETE FROM federation_follows
                       WHERE user_id = $1 AND direction = 'incoming'
                       AND remote_actor_id IN (
                           SELECT id FROM federation_remote_actors WHERE actor_url = $2
                       )"#,
                    [local_user_id.into(), actor_url_str.into()],
                ))
                .await
                .map_err(db_err)?;

            let mut removed = removed.rows_affected();
            if removed == 0 {
                removed = delete_incoming_follow_normalized(db, local_user_id, actor_url_str)
                    .await
                    .map_err(db_err)?;
            }

            if removed > 0 {
                tracing::info!(
                    "🔓 Follow removed: {} unfollowed user {}",
                    actor_url_str,
                    local_user_id
                );
            } else {
                tracing::debug!(
                    actor = %actor_url_str,
                    local_user_id,
                    "Undo Follow matched no incoming follow row"
                );
            }
        }
        "Like" | "Announce" => {
            // 被撤销的互动必须由本次签名 Actor 创建 —— 否则任意远端都能
            // 按 activity id 撤销别人的 Like/Announce。
            let inner_actor = extract_activity_actor_id(&activity["object"]);
            if !inner_actor.is_empty() && !same_actor_url(&inner_actor, actor_url_str) {
                tracing::warn!(
                    actor = %actor_url_str,
                    inner_actor = %inner_actor,
                    "Undo rejected: inner activity belongs to a different actor"
                );
                return Err((
                    StatusCode::FORBIDDEN,
                    Json(json!({
                        "error": "Undo actor does not own the undone activity",
                        "actor": actor_url_str,
                        "inner_actor": inner_actor,
                    })),
                ));
            }
            crate::federation::interactions::handle_inbound_undo_interaction(
                db,
                local_user_id,
                actor_url_str,
                activity,
            )
            .await;
        }
        _ => {
            tracing::debug!("Undo for unsupported type: {}", inner_type);
        }
    }

    Ok(StatusCode::ACCEPTED)
}

/// Delete this user's incoming follow from `actor_url_str`, comparing actor URLs
/// with [`same_actor_url`] instead of byte equality.
///
/// Fallback for the exact-match DELETE: the stored `actor_url` can differ from
/// the Undo's `actor` by trailing slash or host case, and an unfollow that
/// silently keeps the follower is worse than a slightly wider scan (bounded to
/// this user's incoming follows).
async fn delete_incoming_follow_normalized(
    db: &DatabaseConnection,
    local_user_id: i32,
    actor_url_str: &str,
) -> Result<u64, sea_orm::DbErr> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT f.id, ra.actor_url
               FROM federation_follows f
               JOIN federation_remote_actors ra ON f.remote_actor_id = ra.id
               WHERE f.user_id = $1 AND f.direction = 'incoming'"#,
            [local_user_id.into()],
        ))
        .await?;

    let mut removed = 0u64;
    for row in rows {
        let (Ok(id), Ok(stored)) = (
            row.try_get::<i32>("", "id"),
            row.try_get::<String>("", "actor_url"),
        ) else {
            continue;
        };
        if !same_actor_url(&stored, actor_url_str) {
            continue;
        }
        removed += db
            .execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "DELETE FROM federation_follows WHERE id = $1",
                [id.into()],
            ))
            .await?
            .rows_affected();
    }
    Ok(removed)
}

/// 处理内容类 Activity（Create/Update/Delete/Announce/Like）
async fn handle_content_activity(
    db: &DatabaseConnection,
    local_user_id: i32,
    actor_url_str: &str,
    activity_type: &str,
    activity: &serde_json::Value,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    // 签名只证明"请求由 activity.actor 的公钥签出"。要动对象，还得证明这个
    // Actor 有权动它 —— 否则任意联邦实例都能伪造他人内容或删除他人条目。
    //
    // Create/Update：attributedTo 必须是签名 Actor，对象 id 必须同源。
    // Delete：对象通常已压缩成裸 IRI，只能做同源判断，真正的所有权在下面的
    // SQL 里用 remote_actor_id 再收一次。
    // Announce/Like：对象本来就是别人的，不适用。
    let ownership = match activity_type {
        "Create" | "Update" => Some(crate::federation::audience::verify_object_ownership(
            actor_url_str,
            &activity["object"],
        )),
        "Delete" => Some(crate::federation::audience::verify_object_same_origin(
            actor_url_str,
            &activity["object"],
        )),
        _ => None,
    };
    if let Some(Err(e)) = ownership {
        tracing::warn!(
            actor = %actor_url_str,
            activity_type,
            "Content activity rejected by ownership check: {}",
            e
        );
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": "Object ownership check failed", "reason": e.to_string()})),
        ));
    }

    let remote = fetch_remote_actor(db, actor_url_str).await.map_err(|e| {
        tracing::warn!("Failed to fetch actor: {}", e);
        (StatusCode::BAD_REQUEST, Json(json!({"error": e})))
    })?;

    let activity_id = activity["id"].as_str().unwrap_or("").to_string();
    let object_type = activity["object"]["type"].as_str().map(|s| s.to_string());

    // 记录 Activity
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"INSERT INTO federation_activities
               (activity_id, remote_actor_id, activity_type, object_type, object_json, is_local, received_at, published_at)
           VALUES ($1, $2, $3, $4, $5, false, NOW(), NOW())
           ON CONFLICT (activity_id) DO NOTHING"#,
        [
            activity_id.into(),
            remote.id.into(),
            activity_type.into(),
            object_type.clone().into(),
            activity["object"].clone().into(),
        ],
    ))
    .await
    .map_err(db_err)?;

    // Delete: soft-remove prior Create of the same object from this user's timeline
    if activity_type == "Delete" {
        let object_id = activity["object"]["id"]
            .as_str()
            .or_else(|| activity["object"].as_str())
            .unwrap_or("");
        if !object_id.is_empty() {
            // `remote_actor_id` 约束是关键：没有它，任何持有效签名的远端都能
            // 用任意 object id 删掉目标用户时间线里**别人**的条目。
            let _ = db
                .execute_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    r#"DELETE FROM federation_timeline
                       WHERE user_id = $1
                         AND remote_actor_id = $3
                         AND (
                           content_json->>'id' = $2
                           OR activity_id = $2
                           OR content_json #>> '{object,id}' = $2
                         )"#,
                    [local_user_id.into(), object_id.into(), remote.id.into()],
                ))
                .await;
        }
        return Ok(StatusCode::ACCEPTED);
    }

    // Like: record activity only — do not pollute home feed (counts via activities).
    if activity_type == "Like" {
        crate::federation::interactions::handle_inbound_like(
            db,
            local_user_id,
            actor_url_str,
            activity,
        )
        .await;
        return Ok(StatusCode::ACCEPTED);
    }

    // 添加到 Timeline — prefer plain source.content for Note objects
    // Announce / Create / Update land on the feed.
    let preview = timeline_preview_from_object(&activity["object"]);

    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"INSERT INTO federation_timeline
               (user_id, activity_id, remote_actor_id, activity_type, object_type, content_preview, content_json, received_at)
           VALUES ($1, $2, $3, $4, $5, $6, $7, NOW())
           ON CONFLICT (user_id, activity_id) DO NOTHING"#,
        [
            local_user_id.into(),
            activity["id"].as_str().unwrap_or("").into(),
            remote.id.into(),
            activity_type.into(),
            object_type.into(),
            preview.into(),
            activity["object"].clone().into(),
        ],
    ))
    .await
    .map_err(db_err)?;

    Ok(StatusCode::ACCEPTED)
}

/// Plain timeline preview from an AP object (Note prefers source.content).
fn timeline_preview_from_object(object: &serde_json::Value) -> Option<String> {
    object
        .pointer("/source/content")
        .and_then(|v| v.as_str())
        .or_else(|| object.get("content").and_then(|v| v.as_str()))
        .or_else(|| object.get("summary").and_then(|v| v.as_str()))
        .or_else(|| object.get("content_preview").and_then(|v| v.as_str()))
        .or_else(|| object.get("mfp:contentPreview").and_then(|v| v.as_str()))
        .or_else(|| object.get("name").and_then(|v| v.as_str()))
        .map(|s| {
            let plain = s
                .replace("<p>", "")
                .replace("</p>", "")
                .replace("<br>", " ")
                .replace("<br/>", " ")
                .replace("<br />", " ")
                .replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&amp;", "&")
                .replace("&quot;", "\"");
            plain.chars().take(200).collect::<String>()
        })
        .filter(|s| !s.trim().is_empty())
}

/// 将共享收件箱的活动分发给所有关注该 Actor 的本地用户
async fn distribute_to_followers(
    db: &DatabaseConnection,
    remote_actor_id: i32,
    activity_type: &str,
    activity: &serde_json::Value,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    // Likes are recorded in federation_activities only — not home feed.
    if activity_type == "Like" {
        return Ok(());
    }

    let activity_id_str = activity["id"].as_str().unwrap_or("");
    let object_type = activity["object"]["type"].as_str().map(|s| s.to_string());
    let preview = timeline_preview_from_object(&activity["object"]);

    // 批量 INSERT — 一次 SQL 分发到所有关注者的时间线，避免 N+1
    let _ = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"INSERT INTO federation_timeline
                   (user_id, activity_id, remote_actor_id, activity_type, object_type, content_preview, content_json, received_at)
               SELECT f.user_id, $1, $2, $3, $4, $5, $6, NOW()
               FROM federation_follows f
               WHERE f.remote_actor_id = $2 AND f.direction = 'outgoing' AND f.status = 'accepted'
               -- 去重由 (user_id, activity_id) 唯一索引保证。原先的 NOT EXISTS
               -- 是先查后插：同一条活动并发送达时，两次扇出可以同时通过检查。
               ON CONFLICT (user_id, activity_id) DO NOTHING"#,
            [
                activity_id_str.into(),
                remote_actor_id.into(),
                activity_type.into(),
                object_type.into(),
                preview.into(),
                activity["object"].clone().into(),
            ],
        ))
        .await;

    Ok(())
}

// HTTP Signature 验证

/// 读取一个**只允许出现一次**的请求头。
///
/// HTTP 允许同名 header 重复，而这里的两条读取路径对重复的处理正好相反：
/// [`HeaderMap::get`] 取**第一个**值，`headers.iter().collect::<HashMap<_,_>>()`
/// 留下**最后一个**。于是一个带两个 `Date`（或两个 `Digest`）的请求可以用第一个
/// 值通过新鲜度 / 摘要闸门，再用第二个值去重建签名字符串 —— 攻击者只要重放一条
/// 抓到的签名，配上自己构造的 body 和一对重复头，就能让任意内容被认成对方签发。
///
/// 这种歧义从来不是正常流量，一律 401。注意重复检查只作用于签名真正覆盖的
/// header：`Via` / `X-Forwarded-For` 之类的重复是合法且无害的。
fn unique_header<'a>(
    headers: &'a HeaderMap,
    name: &str,
) -> Result<Option<&'a str>, (StatusCode, Json<serde_json::Value>)> {
    let mut values = headers.get_all(name).iter();
    let Some(first) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some() {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": format!("Ambiguous request: header `{name}` appears more than once"),
            })),
        ));
    }
    first.to_str().map(Some).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": format!("Invalid `{name}` header encoding")})),
        )
    })
}

/// 为重建签名字符串收集 header 值。
///
/// 只收集签名 `headers` 参数里列出的名字，且每个都走 [`unique_header`]。
/// 缺失的 header 不放进 map —— 由 `verify_signature` 报
/// "Missing header for signature"，保持原有错误语义。
fn signing_header_map(
    headers: &HeaderMap,
    covered: &[String],
) -> Result<std::collections::HashMap<String, String>, (StatusCode, Json<serde_json::Value>)> {
    let mut map = std::collections::HashMap::with_capacity(covered.len());
    for name in covered {
        if name == "(request-target)" {
            continue;
        }
        if let Some(value) = unique_header(headers, name.as_str())? {
            map.insert(name.clone(), value.to_string());
        }
    }
    Ok(map)
}

/// 解析请求体**之前**必须通过的检查。
///
/// inbox 允许 INBOX_BODY_LIMIT 的请求体（房间/频道消息与文件分块确实需要），而过去的顺序是
/// 「先 `serde_json::from_slice` 整个 body，再验签」—— 于是任何未认证客户端都能
/// 用一坨满额 inbox body 的 JSON 逼服务端做一次完整解析，代价完全不对等。
///
/// 这里把只依赖 header 和原始字节、不需要网络往返的检查提到解析之前：
/// Signature 头存在且可解析、签名覆盖的 header 集合合规、Date 新鲜、
/// Digest 与原始 body 逐字节相符。攻击者要让我们开始解析 JSON，至少得先算出
/// 这段 body 正确的 SHA-256 并附上格式合法的签名头。
///
/// 注意这**不是**认证 —— 真正的签名验证仍然在 [`verify_request_signature`] 里
/// 完成（需要先解析出 actor 才能取公钥）。这只是把最廉价的拒绝点前移。
fn verify_preparse_gate(
    headers: &HeaderMap,
    body: &[u8],
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    let sig_header = unique_header(headers, "signature")?.ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "Missing Signature header"})),
        )
    })?;

    let parsed = parse_signature_header(sig_header).map_err(|e| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": format!("Invalid Signature header: {}", e)})),
        )
    })?;
    require_covered_headers(&parsed, !body.is_empty()).map_err(|e| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": format!("Invalid signed-header set: {}", e)})),
        )
    })?;

    let date = unique_header(headers, "date")?.ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "Missing Date header"})),
        )
    })?;
    verify_date_freshness(date, chrono::Utc::now(), HTTP_DATE_MAX_SKEW).map_err(|error| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": format!("Invalid request date: {}", error)})),
        )
    })?;

    if !body.is_empty() {
        let digest_str = unique_header(headers, "digest")?.ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Missing Digest header for request with body"})),
            )
        })?;
        if !verify_digest(body, digest_str) {
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Digest verification failed"})),
            ));
        }
    }

    Ok(())
}

// merged from handlers.rs

/// 验证请求的 HTTP Signature
///
/// MYR-022: remote Actor material used for the public key is resolved via
/// [`fetch_remote_actor_for_verify`] (DB cache hit or **ephemeral** HTTP fetch).
/// Failed signatures never write an unauthenticated remote document into
/// `federation_remote_actors`. Successful verification may persist via
/// [`persist_verified_remote_actor`].
async fn verify_request_signature(
    db: &DatabaseConnection,
    headers: &HeaderMap,
    body: &[u8],
    actor_url_str: &str,
    request_path: &str,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    // 获取 Signature header
    let sig_header = unique_header(headers, "signature")?.ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "Missing Signature header"})),
        )
    })?;

    let parsed = parse_signature_header(sig_header).map_err(|e| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": format!("Invalid Signature header: {}", e)})),
        )
    })?;
    require_covered_headers(&parsed, !body.is_empty()).map_err(|e| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": format!("Invalid signed-header set: {}", e)})),
        )
    })?;

    let date = unique_header(headers, "date")?.ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "Missing Date header"})),
        )
    })?;
    verify_date_freshness(date, chrono::Utc::now(), HTTP_DATE_MAX_SKEW).map_err(|error| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": format!("Invalid request date: {}", error)})),
        )
    })?;

    // Digest 验证（非空 body 必须携带 Digest header）
    if !body.is_empty() {
        let digest_str = unique_header(headers, "digest")?.ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Missing Digest header for non-empty body"})),
            )
        })?;
        if !verify_digest(body, digest_str) {
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Digest verification failed"})),
            ));
        }
    }

    // MYR-022: trusted cache or ephemeral remote fetch — never poison DB on 401.
    let mut resolved: ResolvedRemoteActor =
        fetch_remote_actor_for_verify(db, actor_url_str, false)
            .await
            .map_err(|e| {
                (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({"error": format!("Cannot verify actor: {}", e)})),
                )
            })?;

    // If we stored a public_key_id for this actor, Signature keyId must match
    // (normalized). On mismatch, force ephemeral re-fetch once — stale cache
    // after key rotation / domain path change was a common permanent 401 source.
    // If no stored key id, PEM-only verify.
    if let Some(ref stored_kid) = resolved.info.public_key_id {
        if !stored_kid.is_empty() && !same_key_id(stored_kid, &parsed.key_id) {
            tracing::warn!(
                "Signature keyId mismatch (will refresh actor ephemerally): stored={}, request={}",
                stored_kid,
                parsed.key_id
            );
            match fetch_remote_actor_for_verify(db, actor_url_str, true).await {
                Ok(fresh) => resolved = fresh,
                Err(e) => {
                    tracing::warn!("Actor refresh after keyId mismatch failed: {}", e);
                }
            }
            if let Some(ref fresh_kid) = resolved.info.public_key_id {
                if !fresh_kid.is_empty() && !same_key_id(fresh_kid, &parsed.key_id) {
                    // Last chance: request keyId may still be a valid id for the
                    // same actor path even if publicKey.id differs slightly —
                    // only accept when the request keyId is clearly under this
                    // actor URL (same origin + /users/{name}).
                    let actor_ok = key_id_belongs_to_actor(&parsed.key_id, actor_url_str);
                    if !actor_ok {
                        tracing::warn!(
                            "Signature keyId mismatch after refresh: stored={}, request={}",
                            fresh_kid,
                            parsed.key_id
                        );
                        return Err((
                            StatusCode::UNAUTHORIZED,
                            Json(json!({
                                "error": "Signature keyId does not match actor public key id",
                                "stored_key_id": fresh_kid,
                                "request_key_id": parsed.key_id,
                            })),
                        ));
                    }
                    tracing::info!(
                        "Accepting Signature keyId under actor URL after publicKey.id drift: request={} actor={}",
                        parsed.key_id,
                        actor_url_str
                    );
                }
            }
        }
    }

    let public_key_pem = resolved.info.public_key_pem.as_deref().ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "Remote actor has no public key"})),
        )
    })?;

    // 构建请求方法和路径
    let method = "POST"; // Inbox 总是 POST
    let path = request_path;

    // 只取签名覆盖的 header，且每个都必须唯一（见 unique_header）。
    let header_map = signing_header_map(headers, &parsed.headers)?;

    let valid =
        verify_signature(public_key_pem, &parsed, method, path, &header_map).map_err(|e| {
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": format!("Signature verification failed: {}", e)})),
            )
        })?;

    if !valid {
        // Ephemeral document is dropped here — never written to DB (MYR-022).
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "Invalid signature"})),
        ));
    }

    // Signature OK: promote ephemeral actor material into the trusted cache.
    if resolved.needs_persist {
        if let Err(e) = persist_verified_remote_actor(db, &resolved).await {
            // Handlers re-fetch via fetch_remote_actor; log and continue.
            tracing::warn!(
                actor = %actor_url_str,
                error = %e,
                "Failed to persist verified remote actor; handlers may re-fetch"
            );
        }
    }

    Ok(())
}

// 投递入队

/// 将 Activity 入库并加入投递队列。
///
/// Same-instance inboxes are processed in-process (no HTTP). The delivery worker
/// refuses localhost/private targets, so without this shortcut Follow Accept
/// never lands and the initiator stays stuck on `pending` while the followee
/// already shows the follower as accepted.
async fn enqueue_delivery(
    db: &DatabaseConnection,
    user_id: i32,
    activity: &Activity,
    target_inbox: &str,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    // 序列化完整 Activity（含 @context/type/id/actor/object），供 delivery.rs 直接发送
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

    let result = match activity_type {
        "Follow" => handle_follow(db, user_id, actor_url_str, activity).await,
        "Accept" => handle_accept(db, user_id, activity).await,
        "Reject" => handle_reject(db, user_id, actor_url_str, activity).await,
        "Undo" => handle_undo(db, user_id, actor_url_str, activity).await,
        "Move" => handle_move(db, actor_url_str, activity).await,
        "Create" | "Update" | "Delete" | "Announce" | "Like" => {
            handle_content_activity(db, user_id, actor_url_str, activity_type, activity).await
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

// 辅助函数

/// Prefer shared async helper — never `blocking_read` inside the tokio runtime
/// (panics with "Cannot block the current thread from within a runtime").
async fn get_base_url() -> String {
    crate::federation::types::get_base_url().await
}

async fn get_local_user(
    db: &DatabaseConnection,
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
                Json(json!({"error": "User not found"})),
            )
        })?;

    Ok((
        row.try_get("", "id").unwrap_or(0),
        row.try_get("", "username").unwrap_or_default(),
    ))
}

async fn get_username_by_id(
    db: &DatabaseConnection,
    user_id: i32,
) -> Result<String, (StatusCode, Json<serde_json::Value>)> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT username FROM users WHERE id = $1 LIMIT 1",
            [user_id.into()],
        ))
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "User not found"})),
            )
        })?;

    Ok(row.try_get("", "username").unwrap_or_default())
}

// MFP Activity 处理器

/// 处理 MFP 扩展 Activity（myriad:ChannelOpen, myriad:ChannelMessage, myriad:ChannelClose 等）
async fn handle_mfp_activity(
    db: &DatabaseConnection,
    _local_user_id: Option<i32>,
    actor_url_str: &str,
    activity_type: &str,
    activity: &serde_json::Value,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    tracing::info!(
        "📬 MFP activity received: type={}, actor={}",
        activity_type,
        actor_url_str
    );

    match activity_type {
        "myriad:ChannelOpen" => {
            crate::federation::channel::handle_channel_open(db, actor_url_str, activity)
                .await
                .map_err(|e| inbox_err("ChannelOpen handling failed", e))?;
            Ok(StatusCode::ACCEPTED)
        }
        "myriad:ChannelMessage" => {
            crate::federation::channel::handle_channel_message(db, actor_url_str, activity)
                .await
                .map_err(|e| inbox_err("ChannelMessage handling failed", e))?;
            Ok(StatusCode::ACCEPTED)
        }
        "myriad:ChannelClose" => {
            crate::federation::channel::handle_channel_close(db, actor_url_str, activity)
                .await
                .map_err(|e| inbox_err("ChannelClose handling failed", e))?;
            Ok(StatusCode::ACCEPTED)
        }
        // Phase 4: Room
        "myriad:RoomInvite" => {
            crate::federation::room::handle_room_invite(db, actor_url_str, activity)
                .await
                .map_err(|e| inbox_err("RoomInvite handling failed", e))?;
            Ok(StatusCode::ACCEPTED)
        }
        "myriad:RoomMessage" => {
            crate::federation::room::handle_room_message(db, actor_url_str, activity)
                .await
                .map_err(|e| inbox_err("RoomMessage handling failed", e))?;
            Ok(StatusCode::ACCEPTED)
        }
        "myriad:RoomLeave" => {
            crate::federation::room::handle_room_leave(db, actor_url_str, activity)
                .await
                .map_err(|e| inbox_err("RoomLeave handling failed", e))?;
            Ok(StatusCode::ACCEPTED)
        }
        "myriad:RoomDissolve" => {
            crate::federation::room::handle_room_dissolve(db, actor_url_str, activity)
                .await
                .map_err(|e| inbox_err("RoomDissolve handling failed", e))?;
            Ok(StatusCode::ACCEPTED)
        }
        // Phase 5: Ring
        "myriad:RingJoin" => {
            crate::federation::ring::handle_ring_join(db, actor_url_str, activity)
                .await
                .map_err(|e| inbox_err("RingJoin handling failed", e))?;
            Ok(StatusCode::ACCEPTED)
        }
        "myriad:RingSync" => {
            crate::federation::ring::handle_ring_sync(db, actor_url_str, activity)
                .await
                .map_err(|e| inbox_err("RingSync handling failed", e))?;
            Ok(StatusCode::ACCEPTED)
        }
        "myriad:RingLeave" => {
            crate::federation::ring::handle_ring_leave(db, actor_url_str, activity)
                .await
                .map_err(|e| inbox_err("RingLeave handling failed", e))?;
            Ok(StatusCode::ACCEPTED)
        }
        "myriad:FileTransfer" => {
            crate::federation::file_transfer::handle_file_transfer(db, actor_url_str, activity)
                .await
                .map_err(|e| inbox_err("FileTransfer handling failed", e))?;
            Ok(StatusCode::ACCEPTED)
        }
        "myriad:ChannelAccept" => {
            crate::federation::channel::handle_channel_accept(db, actor_url_str, activity)
                .await
                .map_err(|e| inbox_err("ChannelAccept handling failed", e))?;
            Ok(StatusCode::ACCEPTED)
        }
        "myriad:KeyExchange" => {
            // Channel vs Room：object.room 优先，否则走 Channel
            let is_room = activity
                .get("object")
                .and_then(|o| o.get("room"))
                .and_then(|v| v.as_str())
                .is_some();
            if is_room {
                crate::federation::room::handle_key_exchange(db, actor_url_str, activity)
                    .await
                    .map_err(|e| inbox_err("Room KeyExchange handling failed", e))?;
            } else {
                crate::federation::channel::handle_key_exchange(db, actor_url_str, activity)
                    .await
                    .map_err(|e| inbox_err("Channel KeyExchange handling failed", e))?;
            }
            Ok(StatusCode::ACCEPTED)
        }
        "myriad:RoomJoin" => {
            crate::federation::room::handle_room_join(db, actor_url_str, activity)
                .await
                .map_err(|e| inbox_err("RoomJoin handling failed", e))?;
            Ok(StatusCode::ACCEPTED)
        }
        "myriad:RoomPin" => {
            crate::federation::room::handle_room_pin(db, actor_url_str, activity)
                .await
                .map_err(|e| inbox_err("RoomPin handling failed", e))?;
            Ok(StatusCode::ACCEPTED)
        }
        "myriad:RoomGovernance" => {
            crate::federation::room::handle_room_governance(db, actor_url_str, activity)
                .await
                .map_err(|e| inbox_err("RoomGovernance handling failed", e))?;
            Ok(StatusCode::ACCEPTED)
        }
        _ => {
            tracing::info!("Unhandled MFP activity type: {}", activity_type);
            Ok(StatusCode::ACCEPTED)
        }
    }
}

#[cfg(test)]
mod tests {

    /// 白名单与分派必须一一对应。
    ///
    /// 这个不变量已经出过两次问题：`myriad:CharacterVisit*` 先是只进了白名单、
    /// 没有分派分支（活动验签通过、返 202、然后被静默丢弃 —— 对远端撒谎，
    /// 它以为投递成功不会重试）；随后功能被移除时白名单又没跟着摘。
    ///
    /// 跨两个 match 的约束类型系统表达不了，所以对源码断言。
    #[test]
    fn every_allowed_mfp_type_has_a_dispatch_arm() {
        // receive.rs is the sole inbox impl module (handlers merged in).
        let src = include_str!("receive.rs");

        let allowlist = src
            .split("const ALLOWED_MFP_TYPES: &[&str] = &[")
            .nth(1)
            .expect("ALLOWED_MFP_TYPES literal moved")
            .split("];")
            .next()
            .unwrap();
        let allowed: Vec<&str> = allowlist
            .lines()
            .map(str::trim)
            .filter(|l| !l.starts_with("//"))
            .filter_map(|l| l.strip_prefix('"'))
            .filter_map(|l| l.split('"').next())
            .collect();
        assert!(
            allowed.len() >= 15,
            "expected the full MFP allowlist, got {allowed:?}"
        );

        let dispatch = src
            .split("async fn handle_mfp_activity(")
            .nth(1)
            .expect("handle_mfp_activity moved");

        let undispatched: Vec<&&str> = allowed
            .iter()
            .filter(|ty| !dispatch.contains(&format!("\"{ty}\"")))
            .collect();

        assert!(
            undispatched.is_empty(),
            "these MFP types are accepted by the inbox allowlist but have no dispatch arm, \n\
             so they would be signature-verified, answered 202, then silently dropped: {undispatched:?}"
        );
    }
    use super::*;

    #[test]
    fn accept_matches_follow_activity_id() {
        let candidates = vec![(
            "https://a.example/activities/1".into(),
            "https://b.example/users/bob".into(),
            "pending".into(),
        )];
        let got = resolve_follow_accept_target(
            "https://a.example/activities/1",
            "https://b.example/users/bob",
            &candidates,
        );
        assert_eq!(
            got,
            Some((
                "https://a.example/activities/1".into(),
                "https://b.example/users/bob".into(),
                false
            ))
        );
    }

    #[test]
    fn accept_matches_activity_id_trailing_slash() {
        let candidates = vec![(
            "https://a.example/activities/1".into(),
            "https://b.example/users/bob".into(),
            "pending".into(),
        )];
        let got = resolve_follow_accept_target(
            "https://a.example/activities/1/",
            "https://B.example/users/bob",
            &candidates,
        );
        assert!(got.is_some());
    }

    #[test]
    fn accept_fallback_single_pending_to_actor() {
        let candidates = vec![(
            "https://a.example/activities/missing".into(),
            "https://b.example/users/bob".into(),
            "pending".into(),
        )];
        let got = resolve_follow_accept_target(
            "https://other/activities/x",
            "https://b.example/users/bob/",
            &candidates,
        );
        assert_eq!(
            got.map(|(id, _, already)| (id, already)),
            Some(("https://a.example/activities/missing".into(), false))
        );
    }

    #[test]
    fn accept_fallback_ambiguous_does_not_match() {
        let candidates = vec![
            (
                "https://a.example/activities/1".into(),
                "https://b.example/users/bob".into(),
                "pending".into(),
            ),
            (
                "https://a.example/activities/2".into(),
                "https://b.example/users/bob".into(),
                "pending".into(),
            ),
        ];
        let got = resolve_follow_accept_target(
            "https://unknown/activities/z",
            "https://b.example/users/bob",
            &candidates,
        );
        assert!(got.is_none());
    }

    #[test]
    fn accept_rejects_actor_mismatch_even_with_id() {
        let candidates = vec![(
            "https://a.example/activities/1".into(),
            "https://b.example/users/bob".into(),
            "pending".into(),
        )];
        let got = resolve_follow_accept_target(
            "https://a.example/activities/1",
            "https://evil.example/users/mallory",
            &candidates,
        );
        assert!(got.is_none());
    }

    #[test]
    fn accept_idempotent_already_accepted_by_id() {
        let candidates = vec![(
            "https://a.example/activities/1".into(),
            "https://b.example/users/bob".into(),
            "accepted".into(),
        )];
        let got = resolve_follow_accept_target(
            "https://a.example/activities/1",
            "https://b.example/users/bob",
            &candidates,
        );
        assert_eq!(got.map(|(_, _, already)| already), Some(true));
    }

    #[test]
    fn accept_matches_username_case_drift_on_same_host() {
        let candidates = vec![(
            "https://a.example/activities/1".into(),
            "https://b.example/users/Bob".into(),
            "pending".into(),
        )];
        let got = resolve_follow_accept_target(
            "https://a.example/activities/1",
            "https://b.example/users/bob",
            &candidates,
        );
        assert!(got.is_some());
    }

    #[test]
    fn accept_rejects_same_host_different_user_even_with_id() {
        // Multi-user instances share a host. activity_id is not a capability:
        // Carol must not Accept Alice→Bob by citing Bob's Follow id.
        let candidates = vec![(
            "https://a.example/activities/1".into(),
            "https://b.example/users/bob".into(),
            "pending".into(),
        )];
        let got = resolve_follow_accept_target(
            "https://a.example/activities/1",
            "https://b.example/users/carol",
            &candidates,
        );
        assert!(got.is_none());
        // Substring username tricks (/users/bob.extra) must also fail.
        let got2 = resolve_follow_accept_target(
            "https://a.example/activities/1",
            "https://b.example/users/bob.extra",
            &candidates,
        );
        assert!(got2.is_none());
    }

    #[test]
    fn accept_unique_id_same_host_path_drift_alias() {
        // Same host+user under non-standard Accept.actor path form.
        let candidates = vec![(
            "https://a.example/activities/1".into(),
            "https://b.example/users/bob".into(),
            "pending".into(),
        )];
        let got = resolve_follow_accept_target(
            "https://a.example/activities/1",
            "https://b.example/@bob",
            &candidates,
        );
        assert!(got.is_some());
    }

    #[test]
    fn accept_matches_nested_follow_object_id() {
        let activity = serde_json::json!({
            "type": "Accept",
            "actor": "https://b.example/users/bob",
            "object": {
                "type": "Follow",
                "id": "https://a.example/activities/1",
                "actor": "https://a.example/users/alice",
                "object": "https://b.example/users/bob"
            }
        });
        assert_eq!(
            extract_accept_object_id(&activity),
            "https://a.example/activities/1"
        );
        assert_eq!(extract_accept_object_type(&activity), "Follow");
        let candidates = vec![(
            "https://a.example/activities/1".into(),
            "https://b.example/users/bob".into(),
            "pending".into(),
        )];
        let follow_id = extract_accept_object_id(&activity);
        let got = resolve_follow_accept_target(
            &follow_id,
            activity["actor"].as_str().unwrap(),
            &candidates,
        );
        assert!(got.is_some());
    }

    #[test]
    fn accept_matches_string_object_with_query_and_fragment() {
        let candidates = vec![(
            "https://a.example/activities/1".into(),
            "https://b.example/users/bob".into(),
            "pending".into(),
        )];
        let got = resolve_follow_accept_target(
            "https://A.example/activities/1/?utm=1#section",
            "https://b.example/users/bob",
            &candidates,
        );
        assert!(got.is_some());
    }

    #[test]
    fn accept_idempotent_already_accepted_with_query_drift() {
        let candidates = vec![(
            "https://a.example/activities/1".into(),
            "https://b.example/users/bob".into(),
            "accepted".into(),
        )];
        let got = resolve_follow_accept_target(
            "https://a.example/activities/1?retry=1",
            "https://b.example/users/bob",
            &candidates,
        );
        assert_eq!(got.map(|(_, _, already)| already), Some(true));
    }

    #[test]
    fn accept_ambiguous_id_hits_on_same_host_no_silent_pick() {
        let candidates = vec![
            (
                "https://a.example/activities/1".into(),
                "https://b.example/users/bob".into(),
                "pending".into(),
            ),
            (
                "https://a.example/activities/1/?x=1".into(), // normalizes to same id
                "https://b.example/users/bob".into(),         // same user, duplicate id rows
                "pending".into(),
            ),
        ];
        // Accept.actor is neither authorized nor username-compatible uniquely
        // across ambiguous id rows → must not silent-pick.
        let got = resolve_follow_accept_target(
            "https://a.example/activities/1",
            "https://b.example/users/other",
            &candidates,
        );
        assert!(got.is_none());
        // Same user but two candidate rows with same normalized id → ambiguous.
        let got2 = resolve_follow_accept_target(
            "https://a.example/activities/1",
            "https://b.example/users/bob",
            &candidates,
        );
        // Step 1 iterates and returns the first actor-auth hit (non-ambiguous by design).
        assert!(got2.is_some());
    }

    #[test]
    fn accept_rejects_evil_host_same_username() {
        // Cross-host username-only must never match.
        let candidates = vec![(
            "https://a.example/activities/1".into(),
            "https://b.example/users/bob".into(),
            "pending".into(),
        )];
        let got = resolve_follow_accept_target(
            "https://a.example/activities/1",
            "https://evil.example/users/bob",
            &candidates,
        );
        assert!(got.is_none());
        // Fallback with wrong host id also fails
        let got2 = resolve_follow_accept_target(
            "https://unknown/activities/z",
            "https://evil.example/users/bob",
            &candidates,
        );
        assert!(got2.is_none());
    }

    #[test]
    fn extract_accept_object_id_from_string() {
        let activity = serde_json::json!({
            "type": "Accept",
            "actor": "https://b.example/users/bob",
            "object": "https://a.example/activities/9"
        });
        assert_eq!(
            extract_accept_object_id(&activity),
            "https://a.example/activities/9"
        );
        assert_eq!(extract_accept_object_type(&activity), "");
    }

    #[test]
    fn extract_accept_object_id_from_array_and_link() {
        // AS2 multi-value object array (first non-empty id wins).
        let arr = serde_json::json!({
            "type": "Accept",
            "actor": "https://b.example/users/bob",
            "object": [
                {"type": "Follow", "id": "https://a.example/activities/arr-1"}
            ]
        });
        assert_eq!(
            extract_accept_object_id(&arr),
            "https://a.example/activities/arr-1"
        );
        assert_eq!(extract_accept_object_type(&arr), "Follow");

        // Link object uses href.
        let link = serde_json::json!({
            "type": "Accept",
            "actor": "https://b.example/users/bob",
            "object": {"type": "Link", "href": "https://a.example/activities/link-1"}
        });
        assert_eq!(
            extract_accept_object_id(&link),
            "https://a.example/activities/link-1"
        );
    }

    #[test]
    fn extract_activity_actor_id_string_and_expanded() {
        let plain = serde_json::json!({
            "actor": "https://b.example/users/bob"
        });
        assert_eq!(
            extract_activity_actor_id(&plain),
            "https://b.example/users/bob"
        );
        let expanded = serde_json::json!({
            "actor": {
                "type": "Person",
                "id": "https://b.example/users/bob",
                "preferredUsername": "bob"
            }
        });
        assert_eq!(
            extract_activity_actor_id(&expanded),
            "https://b.example/users/bob"
        );
        let empty = serde_json::json!({ "actor": {} });
        assert!(extract_activity_actor_id(&empty).is_empty());
    }

    #[test]
    fn extract_activity_actor_id_link_href_and_string_array() {
        let link = serde_json::json!({
            "actor": {"type": "Link", "href": "https://b.example/users/bob"}
        });
        assert_eq!(
            extract_activity_actor_id(&link),
            "https://b.example/users/bob"
        );
        let arr = serde_json::json!({
            "actor": ["", "  https://b.example/users/bob  "]
        });
        assert_eq!(
            extract_activity_actor_id(&arr),
            "https://b.example/users/bob"
        );
        let arr_obj = serde_json::json!({
            "actor": [
                {"type": "Person"},
                {"type": "Person", "id": "https://b.example/users/carol"}
            ]
        });
        assert_eq!(
            extract_activity_actor_id(&arr_obj),
            "https://b.example/users/carol"
        );
    }

    #[test]
    fn extract_accept_object_skips_empty_array_entries() {
        let activity = serde_json::json!({
            "type": "Accept",
            "actor": "https://b.example/users/bob",
            "object": ["", "   ", "https://a.example/activities/keep"]
        });
        assert_eq!(
            extract_accept_object_id(&activity),
            "https://a.example/activities/keep"
        );
    }

    #[test]
    fn extract_accept_object_type_from_array_first_typed() {
        let activity = serde_json::json!({
            "type": "Accept",
            "actor": "https://b.example/users/bob",
            "object": [
                {"id": "https://a.example/activities/x"},
                {"type": "Follow", "id": "https://a.example/activities/y"}
            ]
        });
        assert_eq!(extract_accept_object_type(&activity), "Follow");
        assert_eq!(
            extract_accept_object_id(&activity),
            "https://a.example/activities/x"
        );
        let link_only = serde_json::json!({
            "object": {"type": "Link", "href": "https://a.example/activities/z"}
        });
        // Link has a type but Accept routing treats pure Link object type as reported.
        assert_eq!(extract_accept_object_type(&link_only), "Link");
    }

    #[test]
    fn accept_matches_with_expanded_actor_object() {
        // Peers that embed actor document must still authorize by id.
        let candidates = vec![(
            "https://a.example/activities/1".into(),
            "https://b.example/users/bob".into(),
            "pending".into(),
        )];
        let activity = serde_json::json!({
            "type": "Accept",
            "actor": {
                "type": "Person",
                "id": "https://b.example/users/bob"
            },
            "object": {
                "type": "Follow",
                "id": "https://a.example/activities/1"
            }
        });
        let actor = extract_activity_actor_id(&activity);
        let follow_id = extract_accept_object_id(&activity);
        let got = resolve_follow_accept_target(&follow_id, &actor, &candidates);
        assert!(got.is_some());
    }

    #[test]
    fn accept_fallback_skips_already_accepted_when_id_unknown() {
        // Fallback only considers pending; an accepted-only set must not re-match.
        let candidates = vec![(
            "https://a.example/activities/old".into(),
            "https://b.example/users/bob".into(),
            "accepted".into(),
        )];
        let got = resolve_follow_accept_target(
            "https://unknown/activities/z",
            "https://b.example/users/bob",
            &candidates,
        );
        assert!(got.is_none());
    }

    #[test]
    fn extract_accept_object_id_trims_whitespace() {
        let activity = serde_json::json!({
            "type": "Accept",
            "actor": "https://b.example/users/bob",
            "object": "  https://a.example/activities/ws  "
        });
        assert_eq!(
            extract_accept_object_id(&activity),
            "https://a.example/activities/ws"
        );
    }

    #[test]
    fn extract_accept_object_id_missing_or_empty() {
        assert_eq!(extract_accept_object_id(&serde_json::json!({})), "");
        assert_eq!(
            extract_accept_object_id(&serde_json::json!({"object": ""})),
            ""
        );
        assert_eq!(
            extract_accept_object_id(&serde_json::json!({"object": {}})),
            ""
        );
        assert_eq!(
            extract_accept_object_id(&serde_json::json!({"object": []})),
            ""
        );
    }

    #[test]
    fn same_actor_or_user_matches_host_user_case() {
        assert!(same_actor_or_user(
            "https://b.example/users/Bob",
            "https://B.EXAMPLE/users/bob"
        ));
        assert!(!same_actor_or_user(
            "https://b.example/users/bob",
            "https://evil.example/users/bob"
        ));
        assert!(!same_actor_or_user(
            "https://b.example/users/bob",
            "https://b.example/users/carol"
        ));
    }

    #[test]
    fn extract_accept_object_type_from_nested_follow() {
        let activity = serde_json::json!({
            "type": "Accept",
            "object": {"type": "Follow", "id": "https://a.example/activities/1"}
        });
        assert_eq!(extract_accept_object_type(&activity), "Follow");
        assert_eq!(
            extract_accept_object_id(&activity),
            "https://a.example/activities/1"
        );
    }

    #[test]
    fn extract_activity_actor_id_from_link_href() {
        let activity = serde_json::json!({
            "actor": {"type": "Link", "href": "https://b.example/users/bob"}
        });
        assert_eq!(
            extract_activity_actor_id(&activity),
            "https://b.example/users/bob"
        );
    }

    #[test]
    fn extract_iri_or_object_id_from_string_array() {
        let activity = serde_json::json!({
            "object": ["", "  https://a.example/activities/arr  "]
        });
        assert_eq!(
            extract_accept_object_id(&activity),
            "https://a.example/activities/arr"
        );
    }

    #[test]
    fn extract_accept_object_id_trims_string_and_nested() {
        assert_eq!(
            extract_accept_object_id(&serde_json::json!({
                "object": "  https://a.example/activities/1  "
            })),
            "https://a.example/activities/1"
        );
        assert_eq!(
            extract_accept_object_id(&serde_json::json!({
                "object": {"id": "  https://a.example/activities/2  ", "type": "Follow"}
            })),
            "https://a.example/activities/2"
        );
        assert_eq!(
            extract_accept_object_type(&serde_json::json!({
                "object": {"id": "x", "type": "myriad:ChannelOpen"}
            })),
            "myriad:ChannelOpen"
        );
    }

    #[test]
    fn same_host_username_compatible_at_handle_and_reject() {
        assert!(same_host_username_compatible(
            "https://b.example/@bob",
            "https://b.example/users/bob"
        ));
        assert!(same_host_username_compatible(
            "https://b.example/@Bob",
            "https://b.example/users/bob"
        ));
        // Different user on same host
        assert!(!same_host_username_compatible(
            "https://b.example/@carol",
            "https://b.example/users/bob"
        ));
        // Cross host
        assert!(!same_host_username_compatible(
            "https://evil.example/@bob",
            "https://b.example/users/bob"
        ));
    }

    #[test]
    fn resolve_follow_accept_empty_candidates_and_empty_id() {
        assert!(resolve_follow_accept_target(
            "https://a.example/activities/1",
            "https://b.example/users/bob",
            &[],
        )
        .is_none());
        // Empty follow_id skips id match; only unique pending-to-actor fallback.
        let candidates = vec![(
            "https://a.example/activities/9".into(),
            "https://b.example/users/bob".into(),
            "pending".into(),
        )];
        let got = resolve_follow_accept_target("", "https://b.example/users/bob", &candidates);
        assert_eq!(
            got.map(|(id, _, already)| (id, already)),
            Some(("https://a.example/activities/9".into(), false))
        );
        // accepted-only with empty id → no pending fallback
        let accepted_only = vec![(
            "https://a.example/activities/9".into(),
            "https://b.example/users/bob".into(),
            "accepted".into(),
        )];
        assert!(
            resolve_follow_accept_target("", "https://b.example/users/bob", &accepted_only)
                .is_none()
        );
    }

    #[test]
    fn resolve_follow_accept_fallback_ignores_non_pending() {
        // Wrong id + only accepted/rejected rows to actor → no match.
        let candidates = vec![
            (
                "https://a.example/activities/1".into(),
                "https://b.example/users/bob".into(),
                "accepted".into(),
            ),
            (
                "https://a.example/activities/2".into(),
                "https://b.example/users/bob".into(),
                "rejected".into(),
            ),
        ];
        assert!(resolve_follow_accept_target(
            "https://unknown/activities/z",
            "https://b.example/users/bob",
            &candidates,
        )
        .is_none());
    }

    #[test]
    fn resolve_follow_accept_port_sensitive_actor_auth() {
        let candidates = vec![(
            "https://a.example/activities/1".into(),
            "https://b.example:8443/users/bob".into(),
            "pending".into(),
        )];
        // Port mismatch → not same actor
        assert!(resolve_follow_accept_target(
            "https://a.example/activities/1",
            "https://b.example/users/bob",
            &candidates,
        )
        .is_none());
        assert!(resolve_follow_accept_target(
            "https://a.example/activities/1",
            "https://b.example:8443/users/bob",
            &candidates,
        )
        .is_some());
    }

    #[test]
    fn split_actor_host_user_shapes() {
        let (h, u) = split_actor_host_user("https://b.example/users/bob");
        assert_eq!(h, "b.example");
        assert_eq!(u, "bob");
        let (h2, u2) = split_actor_host_user("https://b.example:8443/users/Bob");
        assert_eq!(h2, "b.example:8443");
        assert_eq!(u2, "Bob");
        let (h3, u3) = split_actor_host_user("https://b.example/@bob");
        assert_eq!(h3, "b.example");
        assert_eq!(u3, ""); // non-/users/ path → empty user
        let (h4, u4) = split_actor_host_user("not-a-url");
        assert_eq!(h4, "");
        assert_eq!(u4, "");
    }

    #[test]
    fn resolve_follow_accept_accepted_id_path_drift_idempotent() {
        // Already accepted + @handle Accept.actor still yields already=true.
        let candidates = vec![(
            "https://a.example/activities/1".into(),
            "https://b.example/users/bob".into(),
            "accepted".into(),
        )];
        let got = resolve_follow_accept_target(
            "https://a.example/activities/1/",
            "https://b.example/@bob",
            &candidates,
        );
        assert_eq!(
            got.map(|(_, remote, already)| (remote, already)),
            Some(("https://b.example/users/bob".into(), true))
        );
    }

    #[test]
    fn resolve_follow_accept_prefers_actor_auth_over_host_only() {
        // Two remotes same host different users; id matches bob only.
        let candidates = vec![
            (
                "https://a.example/activities/1".into(),
                "https://b.example/users/bob".into(),
                "pending".into(),
            ),
            (
                "https://a.example/activities/other".into(),
                "https://b.example/users/carol".into(),
                "pending".into(),
            ),
        ];
        let got = resolve_follow_accept_target(
            "https://a.example/activities/1",
            "https://b.example/users/bob",
            &candidates,
        );
        assert_eq!(
            got.map(|(id, remote, _)| (id, remote)),
            Some((
                "https://a.example/activities/1".into(),
                "https://b.example/users/bob".into()
            ))
        );
        // Carol citing bob's id must fail when she has no own pending row
        // (if she also has a pending, empty-id fallback could match her).
        let bob_only = vec![candidates[0].clone()];
        assert!(resolve_follow_accept_target(
            "https://a.example/activities/1",
            "https://b.example/users/carol",
            &bob_only,
        )
        .is_none());
    }

    #[test]
    fn r45_same_host_username_compatible_users_path_case() {
        assert!(same_host_username_compatible(
            "https://b.example/users/Bob",
            "https://b.example/users/bob"
        ));
        assert!(!same_host_username_compatible(
            "https://b.example/users/bob",
            "https://b.example/users/carol"
        ));
    }

    #[test]
    fn r46_same_host_username_compatible_at_handle_last_segment() {
        assert!(same_host_username_compatible(
            "https://b.example/@alice",
            "https://b.example/users/alice"
        ));
        assert!(!same_host_username_compatible(
            "https://evil.example/@alice",
            "https://b.example/users/alice"
        ));
    }

    #[test]
    fn r47_resolve_follow_accept_target_no_candidates() {
        assert!(resolve_follow_accept_target(
            "https://a.example/activities/1",
            "https://b.example/users/bob",
            &[],
        )
        .is_none());
    }

    #[test]
    fn r48_resolve_follow_accept_target_id_match_trailing_slash() {
        let candidates = vec![(
            "https://a.example/activities/1".into(),
            "https://b.example/users/bob".into(),
            "pending".into(),
        )];
        assert!(resolve_follow_accept_target(
            "https://a.example/activities/1/",
            "https://b.example/users/bob",
            &candidates,
        )
        .is_some());
    }

    #[test]
    fn r49_resolve_follow_accept_target_rejects_cross_user_same_host() {
        let candidates = vec![(
            "https://a.example/activities/1".into(),
            "https://b.example/users/bob".into(),
            "pending".into(),
        )];
        assert!(resolve_follow_accept_target(
            "https://a.example/activities/1",
            "https://b.example/users/carol",
            &candidates,
        )
        .is_none());
    }

    #[test]
    fn r50_resolve_follow_accept_target_idempotent_accepted() {
        let candidates = vec![(
            "https://a.example/activities/1".into(),
            "https://b.example/users/bob".into(),
            "accepted".into(),
        )];
        let got = resolve_follow_accept_target(
            "https://a.example/activities/1",
            "https://b.example/users/bob",
            &candidates,
        );
        assert_eq!(got.map(|(_, _, already)| already), Some(true));
    }

    #[test]
    fn r51_extract_accept_object_id_string_vs_nested() {
        assert_eq!(
            extract_accept_object_id(&serde_json::json!({
                "object": "https://a.example/activities/z"
            })),
            "https://a.example/activities/z"
        );
        assert_eq!(
            extract_accept_object_id(&serde_json::json!({
                "object": {"id": "https://a.example/activities/n", "type": "Follow"}
            })),
            "https://a.example/activities/n"
        );
    }

    #[test]
    fn r52_same_actor_or_user_rejects_different_ports() {
        assert!(!same_actor_or_user(
            "https://b.example:8443/users/bob",
            "https://b.example/users/bob"
        ));
        assert!(same_actor_or_user(
            "https://b.example:8443/users/bob",
            "https://b.example:8443/users/BOB/"
        ));
    }

    #[test]
    fn extract_activity_actor_id_from_string_array() {
        // AS2 multi-value actor (rare for Accept, valid for some peers).
        let activity = serde_json::json!({
            "actor": ["https://b.example/users/bob", "https://b.example/users/other"]
        });
        assert_eq!(
            extract_activity_actor_id(&activity),
            "https://b.example/users/bob"
        );
        let empty_arr = serde_json::json!({ "actor": [] });
        assert!(extract_activity_actor_id(&empty_arr).is_empty());
        let blank_then_id = serde_json::json!({
            "actor": ["  ", {"id": "https://b.example/users/carol"}]
        });
        assert_eq!(
            extract_activity_actor_id(&blank_then_id),
            "https://b.example/users/carol"
        );
    }

    #[test]
    fn extract_accept_object_id_prefers_id_over_href_on_same_object() {
        // When both id and href exist, id wins (canonical activity id).
        let activity = serde_json::json!({
            "type": "Accept",
            "object": {
                "type": "Follow",
                "id": "https://a.example/activities/id-wins",
                "href": "https://a.example/activities/href-ignored"
            }
        });
        assert_eq!(
            extract_accept_object_id(&activity),
            "https://a.example/activities/id-wins"
        );
    }

    #[test]
    fn accept_path_alias_ap_users_form_matches_stored_users_url() {
        // Some peers emit Accept.actor as /ap/users/{name} while we store /users/{name}.
        let candidates = vec![(
            "https://a.example/activities/1".into(),
            "https://b.example/users/bob".into(),
            "pending".into(),
        )];
        let got = resolve_follow_accept_target(
            "https://a.example/activities/1",
            "https://b.example/ap/users/bob",
            &candidates,
        );
        assert!(got.is_some());
        // Different user under /ap/users must still fail.
        let got_bad = resolve_follow_accept_target(
            "https://a.example/activities/1",
            "https://b.example/ap/users/carol",
            &candidates,
        );
        assert!(got_bad.is_none());
    }

    /// 重复 header 必须 401，而不是「闸门看第一个、验签看最后一个」。
    ///
    /// 回归的是一条完整的伪造链：抓一条合法签名 → 重发时把 `Date` / `Digest`
    /// 各写两遍（第一个是新鲜时间 / 自造 body 的摘要，第二个是原签名覆盖的值）
    /// → 新鲜度和摘要闸门用第一个值放行，签名用第二个值验过 → 任意内容被认成
    /// 对方签发。`.get()` 是 first-wins、`.iter().collect()` 是 last-wins，
    /// 这个差值就是漏洞本身。
    #[test]
    fn duplicate_signed_header_is_rejected() {
        let mut headers = HeaderMap::new();
        headers.append("date", "Mon, 04 Aug 2025 10:00:00 GMT".parse().unwrap());
        headers.append("date", "Mon, 04 Aug 2025 09:00:00 GMT".parse().unwrap());

        // 底层差值仍然存在（这正是必须显式拒绝的原因）
        assert_ne!(
            headers.get("date").unwrap().to_str().unwrap(),
            headers.get_all("date").iter().last().unwrap().to_str().unwrap(),
        );

        let err = unique_header(&headers, "date").unwrap_err();
        assert_eq!(err.0, StatusCode::UNAUTHORIZED);

        let covered = vec!["(request-target)".to_string(), "date".to_string()];
        assert!(signing_header_map(&headers, &covered).is_err());
    }

    #[test]
    fn unique_header_reads_single_values_case_insensitively() {
        let mut headers = HeaderMap::new();
        headers.insert("Date", "Mon, 04 Aug 2025 10:00:00 GMT".parse().unwrap());
        headers.insert("Digest", "SHA-256=abc".parse().unwrap());
        assert_eq!(
            unique_header(&headers, "date").unwrap(),
            Some("Mon, 04 Aug 2025 10:00:00 GMT")
        );
        assert_eq!(unique_header(&headers, "digest").unwrap(), Some("SHA-256=abc"));
        assert_eq!(unique_header(&headers, "signature").unwrap(), None);
    }

    /// 重复只在签名覆盖的 header 上是致命的。代理常常重复 `Via` /
    /// `X-Forwarded-For`，把它们一起拒掉会误伤正常联邦流量。
    #[test]
    fn duplicate_uncovered_header_does_not_block_verification() {
        let mut headers = HeaderMap::new();
        headers.insert("host", "b.example".parse().unwrap());
        headers.insert("date", "Mon, 04 Aug 2025 10:00:00 GMT".parse().unwrap());
        headers.append("via", "1.1 alpha".parse().unwrap());
        headers.append("via", "1.1 beta".parse().unwrap());

        let covered = vec![
            "(request-target)".to_string(),
            "host".to_string(),
            "date".to_string(),
        ];
        let map = signing_header_map(&headers, &covered).unwrap();
        assert_eq!(map.get("host").map(String::as_str), Some("b.example"));
        assert_eq!(map.len(), 2, "(request-target) is rebuilt, not read");
    }

    /// 签名字符串只由签名自己列出的 header 组成 —— 未覆盖的 header 不得混入。
    #[test]
    fn signing_header_map_only_includes_covered_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("host", "b.example".parse().unwrap());
        headers.insert("date", "Mon, 04 Aug 2025 10:00:00 GMT".parse().unwrap());
        headers.insert("x-extra", "ignored".parse().unwrap());

        let covered = vec!["host".to_string(), "date".to_string()];
        let map = signing_header_map(&headers, &covered).unwrap();
        assert!(!map.contains_key("x-extra"));
    }
}

