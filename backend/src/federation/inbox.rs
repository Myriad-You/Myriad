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
use crate::federation::signature::{
    parse_signature_header, require_covered_headers, verify_date_freshness, verify_digest,
    verify_signature,
};
use crate::federation::types::*;

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
        "Undo" => handle_undo(&db, user_id, &actor_url_str, &activity).await,
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
                "myriad:RoomMessage",
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

    // 对于公开活动，尝试添加到所有关注该 actor 的本地用户的 Timeline
    if matches!(activity_type.as_str(), "Create" | "Announce") {
        let remote = fetch_remote_actor(&db, &actor_url_str).await.map_err(|e| {
            tracing::warn!("Failed to fetch remote actor {}: {}", actor_url_str, e);
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Unknown actor"})),
            )
        })?;
        distribute_to_followers(&db, remote.id, &activity_type, &activity).await?;
    }

    Ok(StatusCode::ACCEPTED)
}

// ==================== Activity 处理器 ====================

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
    let base_url = get_base_url();
    let local_username = get_username_by_id(db, local_user_id).await?;
    let local_actor_url = actor_url(&base_url, &local_username);

    let accept = Activity {
        context: build_ap_context(),
        activity_type: "Accept".to_string(),
        id: generate_activity_id(&base_url),
        actor: local_actor_url.clone(),
        to: Some(vec![actor_url_str.to_string()]),
        cc: None,
        published: Some(now_iso8601()),
        object: activity.clone(),
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
    // Accept 的 object 可能是 Follow 或 ChannelOpen
    let object = &activity["object"];
    let inner_type = object.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let follow_id = object
        .get("id")
        .and_then(|v| v.as_str())
        .or_else(|| activity["object"].as_str())
        .unwrap_or("");

    if inner_type == "myriad:ChannelOpen" || inner_type == "myriad:Channel" {
        // 远程方接受了我们的 Channel 开启请求
        let channel_id = follow_id; // object.id 就是 channel_id
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
        // 标准 Follow Accept
        if !follow_id.is_empty() {
            let pending = db
                .query_one(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    r#"SELECT ra.actor_url
                       FROM federation_follows f
                       JOIN federation_remote_actors ra ON f.remote_actor_id = ra.id
                       WHERE f.user_id = $1 AND f.direction = 'outgoing' AND f.activity_id = $2"#,
                    [local_user_id.into(), follow_id.into()],
                ))
                .await
                .map_err(db_err)?;

            let remote_url = pending
                .and_then(|row| row.try_get::<String>("", "actor_url").ok())
                .unwrap_or_default();
            let authorized =
                !remote_url.is_empty() && same_actor_url(accept_actor, &remote_url);

            if authorized {
                let result = db
                    .execute(Statement::from_sql_and_values(
                        DatabaseBackend::Postgres,
                        r#"UPDATE federation_follows
                           SET status = 'accepted', accepted_at = NOW()
                           WHERE user_id = $1 AND direction = 'outgoing' AND activity_id = $2"#,
                        [local_user_id.into(), follow_id.into()],
                    ))
                    .await
                    .map_err(db_err)?;

                if result.rows_affected() > 0 {
                    let label = crate::federation::notify::actor_label(db, &remote_url).await;
                    crate::federation::notify::notify_follow_accepted(
                        local_user_id,
                        if accept_actor.is_empty() {
                            follow_id
                        } else {
                            accept_actor
                        },
                        &label,
                    )
                    .await;
                    tracing::info!("✅ Our follow accepted: {}", follow_id);
                }
            } else {
                tracing::debug!(
                    "Follow accept no-op (missing row or actor mismatch): follow={} accept_actor={}",
                    follow_id,
                    accept_actor
                );
            }
        }
    }

    Ok(StatusCode::ACCEPTED)
}

/// 处理 Undo（包括 Undo Follow）
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

    // 添加到 Timeline
    let preview = activity["object"]["content"]
        .as_str()
        .or_else(|| activity["object"]["summary"].as_str())
        .map(|s| s.chars().take(200).collect::<String>());

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

/// 将共享收件箱的活动分发给所有关注该 Actor 的本地用户
async fn distribute_to_followers(
    db: &DatabaseConnection,
    remote_actor_id: i32,
    activity_type: &str,
    activity: &serde_json::Value,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    let activity_id_str = activity["id"].as_str().unwrap_or("");
    let object_type = activity["object"]["type"].as_str().map(|s| s.to_string());
    let preview = activity["object"]["content"]
        .as_str()
        .or_else(|| activity["object"]["summary"].as_str())
        .map(|s| s.chars().take(200).collect::<String>());

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

/// 将 Activity 入库并加入投递队列
async fn enqueue_delivery(
    db: &DatabaseConnection,
    user_id: i32,
    activity: &Activity,
    target_inbox: &str,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    // 序列化完整 Activity（含 @context/type/id/actor/object），供 delivery.rs 直接发送
    let activity_json = serde_json::to_value(activity).unwrap_or_default();
    let domain = extract_domain(target_inbox).unwrap_or_default();

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
                activity_json.into(),
            ],
        ))
        .await
        .map_err(db_err)?;

    let act_id: i32 = act_row
        .map(|r| r.try_get("", "id").unwrap_or(0))
        .unwrap_or(0);

    // 加入投递队列
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

// ==================== 辅助函数 ====================

fn get_base_url() -> String {
    let config = crate::GLOBAL_CONFIG.blocking_read();
    let base_url = config
        .base_url
        .clone()
        .or_else(|| config.frontend_url.clone())
        .unwrap_or_else(|| format!("http://{}:{}", config.server_host, config.server_port));
    base_url.trim_end_matches('/').to_string()
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
                .map_err(|e| {
                    tracing::error!("ChannelOpen handling failed: {}", e);
                    (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e})))
                })?;
            Ok(StatusCode::ACCEPTED)
        }
        "myriad:ChannelMessage" => {
            crate::federation::channel::handle_channel_message(db, actor_url_str, activity)
                .await
                .map_err(|e| {
                    tracing::error!("ChannelMessage handling failed: {}", e);
                    (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e})))
                })?;
            Ok(StatusCode::ACCEPTED)
        }
        "myriad:ChannelClose" => {
            crate::federation::channel::handle_channel_close(db, actor_url_str, activity)
                .await
                .map_err(|e| {
                    tracing::error!("ChannelClose handling failed: {}", e);
                    (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e})))
                })?;
            Ok(StatusCode::ACCEPTED)
        }
        // Phase 4: Room
        "myriad:RoomInvite" => {
            crate::federation::room::handle_room_invite(db, actor_url_str, activity)
                .await
                .map_err(|e| {
                    tracing::error!("RoomInvite handling failed: {}", e);
                    (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e})))
                })?;
            Ok(StatusCode::ACCEPTED)
        }
        "myriad:RoomMessage" => {
            crate::federation::room::handle_room_message(db, actor_url_str, activity)
                .await
                .map_err(|e| {
                    tracing::error!("RoomMessage handling failed: {}", e);
                    (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e})))
                })?;
            Ok(StatusCode::ACCEPTED)
        }
        "myriad:RoomLeave" => {
            crate::federation::room::handle_room_leave(db, actor_url_str, activity)
                .await
                .map_err(|e| {
                    tracing::error!("RoomLeave handling failed: {}", e);
                    (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e})))
                })?;
            Ok(StatusCode::ACCEPTED)
        }
        // Phase 5: Ring
        "myriad:RingJoin" => {
            crate::federation::ring::handle_ring_join(db, actor_url_str, activity)
                .await
                .map_err(|e| {
                    tracing::error!("RingJoin handling failed: {}", e);
                    (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e})))
                })?;
            Ok(StatusCode::ACCEPTED)
        }
        "myriad:RingSync" => {
            crate::federation::ring::handle_ring_sync(db, actor_url_str, activity)
                .await
                .map_err(|e| {
                    tracing::error!("RingSync handling failed: {}", e);
                    (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e})))
                })?;
            Ok(StatusCode::ACCEPTED)
        }
        "myriad:RingLeave" => {
            crate::federation::ring::handle_ring_leave(db, actor_url_str, activity)
                .await
                .map_err(|e| {
                    tracing::error!("RingLeave handling failed: {}", e);
                    (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e})))
                })?;
            Ok(StatusCode::ACCEPTED)
        }
        "myriad:FileTransfer" => {
            crate::federation::file_transfer::handle_file_transfer(db, actor_url_str, activity)
                .await
                .map_err(|e| {
                    tracing::error!("FileTransfer handling failed: {}", e);
                    (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e})))
                })?;
            Ok(StatusCode::ACCEPTED)
        }
        "myriad:ChannelAccept" => {
            crate::federation::channel::handle_channel_accept(db, actor_url_str, activity)
                .await
                .map_err(|e| {
                    tracing::error!("ChannelAccept handling failed: {}", e);
                    (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e})))
                })?;
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
                    .map_err(|e| {
                        tracing::error!("Room KeyExchange handling failed: {}", e);
                        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e})))
                    })?;
            } else {
                crate::federation::channel::handle_key_exchange(db, actor_url_str, activity)
                    .await
                    .map_err(|e| {
                        tracing::error!("Channel KeyExchange handling failed: {}", e);
                        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e})))
                    })?;
            }
            Ok(StatusCode::ACCEPTED)
        }
        "myriad:RoomJoin" => {
            crate::federation::room::handle_room_join(db, actor_url_str, activity)
                .await
                .map_err(|e| {
                    tracing::error!("RoomJoin handling failed: {}", e);
                    (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e})))
                })?;
            Ok(StatusCode::ACCEPTED)
        }
        "myriad:RoomGovernance" => {
            crate::federation::room::handle_room_governance(db, actor_url_str, activity)
                .await
                .map_err(|e| {
                    tracing::error!("RoomGovernance handling failed: {}", e);
                    (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e})))
                })?;
            Ok(StatusCode::ACCEPTED)
        }
        _ => {
            tracing::info!("Unhandled MFP activity type: {}", activity_type);
            Ok(StatusCode::ACCEPTED)
        }
    }
}
