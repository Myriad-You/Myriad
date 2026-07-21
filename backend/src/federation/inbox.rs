//! Inbox 处理器（Layer 2）
//!
//! 接收远程实例发来的 Activity，验证 HTTP Signature，
//! 分发到对应处理器（Follow, Accept, Create, Announce, Undo 等）

use axum::{
    body::Bytes,
    extract::Path,
    http::{HeaderMap, StatusCode},
    Json,
};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::json;

use crate::federation::actor::fetch_remote_actor;
use crate::federation::errors::{is_permanent_federation_error, map_inbox_handler_error};
use crate::federation::signature::{
    parse_signature_header, require_covered_headers, verify_date_freshness, verify_digest,
    verify_signature,
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
    Path(username): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    let db = get_db()
        .await
        .map_err(|e| (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error": e}))))?;

    // 验证用户存在
    let (user_id, _) = get_local_user(&db, &username).await?;

    // 解析 Activity JSON
    let activity: serde_json::Value = serde_json::from_slice(&body).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Invalid JSON body"})),
        )
    })?;

    let activity_type = activity["type"].as_str().unwrap_or("").to_string();
    let actor_url_str = activity["actor"].as_str().unwrap_or("").to_string();

    if actor_url_str.is_empty() || activity_type.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Missing actor or type in activity"})),
        ));
    }

    // 验证 HTTP Signature
    let request_path = format!("/users/{}/inbox", username);
    verify_request_signature(&db, &headers, &body, &actor_url_str, &request_path).await?;

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
            ];
            if !ALLOWED_MFP_TYPES.contains(&ty) {
                tracing::warn!("Rejected unknown MFP activity type: {}", ty);
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": format!("Unknown MFP activity type: {}", ty)})),
                ));
            }
            handle_mfp_activity(&db, &actor_url_str, ty, &activity).await
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
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    let db = get_db()
        .await
        .map_err(|e| (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error": e}))))?;

    let activity: serde_json::Value = serde_json::from_slice(&body).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Invalid JSON body"})),
        )
    })?;

    let activity_type = activity["type"].as_str().unwrap_or("").to_string();
    let actor_url_str = activity["actor"].as_str().unwrap_or("").to_string();

    if actor_url_str.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Missing actor in activity"})),
        ));
    }

    // 验证签名
    verify_request_signature(&db, &headers, &body, &actor_url_str, "/inbox").await?;

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
        let remote = fetch_remote_actor(&db, &actor_url_str).await.map_err(|e| {
            tracing::warn!("Failed to fetch remote actor {}: {}", actor_url_str, e);
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Unknown actor"})),
            )
        })?;
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
        // Prefer first local user in `to` / `cc`, else first local user (single-tenant)
        let mut target_user_id: Option<i32> = None;
        for key in ["to", "cc"] {
            if let Some(arr) = activity.get(key).and_then(|v| v.as_array()) {
                for t in arr {
                    if let Some(url) = t.as_str() {
                        if let Some(uname) = url.rsplit('/').next() {
                            if let Ok(Some(row)) = db
                                .query_one(Statement::from_sql_and_values(
                                    DatabaseBackend::Postgres,
                                    "SELECT id FROM users WHERE username = $1",
                                    [uname.into()],
                                ))
                                .await
                            {
                                target_user_id = row.try_get::<i32>("", "id").ok();
                                if target_user_id.is_some() {
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            if target_user_id.is_some() {
                break;
            }
        }
        if target_user_id.is_none() {
            if let Ok(Some(row)) = db
                .query_one(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "SELECT id FROM users ORDER BY id LIMIT 1",
                    [],
                ))
                .await
            {
                target_user_id = row.try_get::<i32>("", "id").ok();
            }
        }
        if let Some(uid) = target_user_id {
            if activity_type.starts_with("myriad:") {
                return handle_mfp_activity(&db, &actor_url_str, &activity_type, &activity).await;
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

// ==================== Activity 处理器 ====================

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
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": e})),
        )
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
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": e})),
        )
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
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": e})),
        )
    })?;

    let migrated = migrate_follows_old_to_new(db, &old_actor, &new_actor)
        .await
        .map_err(|e| inbox_err("Move follow migration failed", e))?;

    // Record inbound Move for audit
    let activity_id = activity["id"].as_str().unwrap_or("").to_string();
    if !activity_id.is_empty() {
        let _ = db
            .execute(Statement::from_sql_and_values(
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
        .query_one(Statement::from_sql_and_values(
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
    let base_url = get_base_url().await;
    let local_username = get_username_by_id(db, local_user_id).await?;
    let local_actor_url = actor_url(&base_url, &local_username);

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
        tracing::debug!(
            "Ignoring Reject for unsupported object type={}",
            inner_type
        );
    }
    Ok(StatusCode::ACCEPTED)
}

/// Extract the accepted object id from an Accept activity.
///
/// Supports:
/// - `object`: string activity/object id
/// - `object`: nested `{ "id", "type", ... }` (Follow / ChannelOpen / …)
///
/// Does not walk arbitrary nesting beyond one object level (AP Accept.object).
pub fn extract_accept_object_id(activity: &serde_json::Value) -> String {
    let object = &activity["object"];
    if let Some(s) = object.as_str() {
        return s.trim().to_string();
    }
    if let Some(id) = object.get("id").and_then(|v| v.as_str()) {
        return id.trim().to_string();
    }
    String::new()
}

/// Nested object type for Accept routing (Channel vs Follow). Empty when object is a string id.
fn extract_accept_object_type(activity: &serde_json::Value) -> String {
    activity["object"]
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
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
    let accept_actor = activity["actor"].as_str().unwrap_or("");
    // Accept.object: string id OR nested Follow/ChannelOpen {id,type,…}
    let inner_type = extract_accept_object_type(activity);
    let follow_id = extract_accept_object_id(activity);

    if inner_type == "myriad:ChannelOpen" || inner_type == "myriad:Channel" {
        // 远程方接受了我们的 Channel 开启请求
        let channel_id = follow_id.as_str(); // object.id 就是 channel_id
        if !channel_id.is_empty() {
            // 先取 pending channel 的远程对端 URL，再在 Rust 侧用 same_actor_url 授权
            let pending = db
                .query_one(Statement::from_sql_and_values(
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
                .map(|remote_url| same_actor_url(accept_actor, &remote_url))
                .unwrap_or(false);

            if authorized {
                let result = db
                    .execute(Statement::from_sql_and_values(
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
        handle_follow_accept(db, local_user_id, accept_actor, &follow_id, activity).await?;
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
///    (path/alias drift only — never cross-user on the same host, never
///    cross-host username-only)
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
    //   query/host-case drift without scanning all history
    let rows = db
        .query_all(Statement::from_sql_and_values(
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
        .execute(Statement::from_sql_and_values(
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
            // 远程用户取消关注
            db.execute(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"DELETE FROM federation_follows
                   WHERE user_id = $1 AND direction = 'incoming'
                   AND remote_actor_id = (
                       SELECT id FROM federation_remote_actors WHERE actor_url = $2
                   )"#,
                [local_user_id.into(), actor_url_str.into()],
            ))
            .await
            .map_err(db_err)?;

            tracing::info!(
                "🔓 Follow removed: {} unfollowed user {}",
                actor_url_str,
                local_user_id
            );
        }
        "Like" | "Announce" => {
            crate::federation::interactions::handle_inbound_undo_interaction(
                db,
                local_user_id,
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

/// 处理内容类 Activity（Create/Update/Delete/Announce/Like）
async fn handle_content_activity(
    db: &DatabaseConnection,
    local_user_id: i32,
    actor_url_str: &str,
    activity_type: &str,
    activity: &serde_json::Value,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    let remote = fetch_remote_actor(db, actor_url_str).await.map_err(|e| {
        tracing::warn!("Failed to fetch actor: {}", e);
        (StatusCode::BAD_REQUEST, Json(json!({"error": e})))
    })?;

    let activity_id = activity["id"].as_str().unwrap_or("").to_string();
    let object_type = activity["object"]["type"].as_str().map(|s| s.to_string());

    // 记录 Activity
    db.execute(Statement::from_sql_and_values(
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
            let _ = db
                .execute(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    r#"DELETE FROM federation_timeline
                       WHERE user_id = $1
                         AND (
                           content_json->>'id' = $2
                           OR activity_id = $2
                           OR content_json #>> '{object,id}' = $2
                         )"#,
                    [local_user_id.into(), object_id.into()],
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

    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"INSERT INTO federation_timeline
               (user_id, activity_id, remote_actor_id, activity_type, object_type, content_preview, content_json, received_at)
           SELECT $1, $2, $3, $4, $5, $6, $7, NOW()
           WHERE NOT EXISTS (
               SELECT 1 FROM federation_timeline
               WHERE user_id = $1 AND activity_id = $2
           )"#,
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
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"INSERT INTO federation_timeline
                   (user_id, activity_id, remote_actor_id, activity_type, object_type, content_preview, content_json, received_at)
               SELECT f.user_id, $1, $2, $3, $4, $5, $6, NOW()
               FROM federation_follows f
               WHERE f.remote_actor_id = $2 AND f.direction = 'outgoing' AND f.status = 'accepted'
                 AND NOT EXISTS (
                     SELECT 1 FROM federation_timeline t
                     WHERE t.user_id = f.user_id AND t.activity_id = $1
                 )"#,
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

// ==================== HTTP Signature 验证 ====================

/// 验证请求的 HTTP Signature
async fn verify_request_signature(
    db: &DatabaseConnection,
    headers: &HeaderMap,
    body: &[u8],
    actor_url_str: &str,
    request_path: &str,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    // 获取 Signature header
    let sig_header = headers
        .get("Signature")
        .or_else(|| headers.get("signature"))
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
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

    let date = headers
        .get("Date")
        .or_else(|| headers.get("date"))
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Missing Date header"})),
            )
        })?;
    verify_date_freshness(date, chrono::Utc::now(), chrono::Duration::minutes(5)).map_err(
        |error| {
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": format!("Invalid request date: {}", error)})),
            )
        },
    )?;

    // Digest 验证（非空 body 必须携带 Digest header）
    if !body.is_empty() {
        let digest = headers
            .get("Digest")
            .or_else(|| headers.get("digest"))
            .ok_or_else(|| {
                (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({"error": "Missing Digest header for non-empty body"})),
                )
            })?;
        let digest_str = digest.to_str().map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid Digest header encoding"})),
            )
        })?;
        if !verify_digest(body, digest_str) {
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Digest verification failed"})),
            ));
        }
    }

    // 获取远程 Actor 的公钥
    let remote = fetch_remote_actor(db, actor_url_str).await.map_err(|e| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": format!("Cannot verify actor: {}", e)})),
        )
    })?;

    let public_key_pem = remote.public_key_pem.ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "Remote actor has no public key"})),
        )
    })?;

    // If we stored a public_key_id for this actor, Signature keyId must match
    // (normalized). Fail closed on mismatch. If no stored key id, PEM-only verify.
    if let Some(ref stored_kid) = remote.public_key_id {
        if !stored_kid.is_empty() && !same_key_id(stored_kid, &parsed.key_id) {
            tracing::warn!(
                "Signature keyId mismatch: stored={}, request={}",
                stored_kid,
                parsed.key_id
            );
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({
                    "error": "Signature keyId does not match actor public key id",
                    "stored_key_id": stored_kid,
                    "request_key_id": parsed.key_id,
                })),
            ));
        }
    }

    // 构建请求方法和路径
    let method = "POST"; // Inbox 总是 POST
    let path = request_path;

    // 将 HeaderMap 转换为简单 HashMap
    let header_map: std::collections::HashMap<String, String> = headers
        .iter()
        .filter_map(|(k, v)| {
            v.to_str()
                .ok()
                .map(|val| (k.as_str().to_lowercase(), val.to_string()))
        })
        .collect();

    let valid =
        verify_signature(&public_key_pem, &parsed, method, path, &header_map).map_err(|e| {
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": format!("Signature verification failed: {}", e)})),
            )
        })?;

    if !valid {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "Invalid signature"})),
        ));
    }

    Ok(())
}

// ==================== 投递入队 ====================

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
        .query_one(Statement::from_sql_and_values(
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
    //   deliver_activity_locally → handle_follow → enqueue_delivery → …
    if let Some(local_username) = local_username_from_inbox_url(&base_url, target_inbox) {
        match Box::pin(deliver_activity_locally(db, &local_username, &activity_json)).await {
            Ok(()) => {
                tracing::info!(
                    activity_type = %activity.activity_type,
                    target = %local_username,
                    "📬 Local inbox delivery (no HTTP)"
                );
                // Mark as delivered for observability (queue row optional)
                let _ = db
                    .execute(Statement::from_sql_and_values(
                        DatabaseBackend::Postgres,
                        r#"INSERT INTO federation_delivery_queue
                               (activity_id, target_inbox, target_domain, status, created_at, last_attempt_at)
                           VALUES ($1, $2, $3, 'delivered', NOW(), NOW())"#,
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
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"INSERT INTO federation_delivery_queue
               (activity_id, target_inbox, target_domain, status, created_at)
           VALUES ($1, $2, $3, 'pending', NOW())"#,
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
    let (user_id, _) = get_local_user(db, username)
        .await
        .map_err(|(_, j)| j.0.get("error").and_then(|v| v.as_str()).unwrap_or("user not found").to_string())?;

    let activity_type = activity["type"].as_str().unwrap_or("");
    let actor_url_str = activity["actor"].as_str().unwrap_or("");
    if activity_type.is_empty() || actor_url_str.is_empty() {
        return Err("Missing actor or type in activity".into());
    }

    let result = match activity_type {
        "Follow" => handle_follow(db, user_id, actor_url_str, activity).await,
        "Accept" => handle_accept(db, user_id, activity).await,
        "Reject" => handle_reject(db, user_id, actor_url_str, activity).await,
        "Undo" => handle_undo(db, user_id, actor_url_str, activity).await,
        "Move" => handle_move(db, actor_url_str, activity).await,
        "Create" | "Update" | "Delete" | "Announce" | "Like" => {
            handle_content_activity(db, user_id, actor_url_str, activity_type, activity).await
        }
        other => {
            tracing::debug!(activity_type = other, "Local delivery: unsupported type, ignoring");
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

// ==================== 辅助函数 ====================

/// Prefer shared async helper — never `blocking_read` inside the tokio runtime
/// (panics with "Cannot block the current thread from within a runtime").
async fn get_base_url() -> String {
    crate::federation::types::get_base_url().await
}

async fn get_db() -> Result<DatabaseConnection, String> {
    let db_opt = crate::DB_CONNECTION.read().await;
    db_opt
        .clone()
        .ok_or_else(|| "Database not connected".to_string())
}

async fn get_local_user(
    db: &DatabaseConnection,
    username: &str,
) -> Result<(i32, String), (StatusCode, Json<serde_json::Value>)> {
    let row = db
        .query_one(Statement::from_sql_and_values(
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
        .query_one(Statement::from_sql_and_values(
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

// ==================== MFP Activity 处理器 ====================

/// 处理 MFP 扩展 Activity（myriad:ChannelOpen, myriad:ChannelMessage, myriad:ChannelClose 等）
async fn handle_mfp_activity(
    db: &DatabaseConnection,
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
                "https://b.example/users/bob".into(), // same user, duplicate id rows
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
}
