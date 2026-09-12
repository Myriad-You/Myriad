//! Local Channel CRUD and message send/get.
use axum::{http::StatusCode, Json};
use myriad_error::AppError;
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::json;

use crate::federation::types::*;

use super::e2e::load_e2e_session;
use super::types::{
    ChannelDetail, ChannelSummary, CreateChannelRequest, MessageItem, SendMessageRequest,
    SendMessageResponse,
};

// Channel CRUD 功能

/// 创建（或打开）一个新 Channel
///
/// 如果与该远程 Actor 已有 pending/accepted/active 的同类型 Channel，直接返回已有通道
pub async fn create_channel(
    user_id: i32,
    username: &str,
    db: &DatabaseConnection,
    req: &CreateChannelRequest,
) -> Result<ChannelDetail, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;
    let channel_type = req.channel_type.as_deref().unwrap_or("text");
    let transport = req.transport.as_deref().unwrap_or("websocket");

    // 验证 channel_type 和 transport：取值表由 MFP 协议枚举自己拥有，
    // 手抄一份字符串白名单只会和 ChannelType/ChannelTransport 各自漂移。
    if serde_json::from_value::<crate::federation::types::ChannelType>(json!(channel_type)).is_err()
    {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json("Invalid channel_type")),
        ));
    }
    if serde_json::from_value::<crate::federation::types::ChannelTransport>(json!(transport))
        .is_err()
    {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json("Invalid transport")),
        ));
    }

    let remote_actor_url =
        crate::federation::follow::resolve_actor_reference(&req.remote_actor).await?;
    let local_actor = actor_url(&base_url, username);
    if same_actor_url(&remote_actor_url, &local_actor) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json(
                "Cannot create a channel with your own federation actor",
            )),
        ));
    }

    // 确保远程 Actor 已缓存
    let remote = crate::federation::actor::fetch_remote_actor(db, &remote_actor_url)
        .await
        .map_err(|e| {
            tracing::error!("[Channel] Failed to fetch remote actor: {}", e);
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "Cannot resolve remote actor",
                    "code": "remote_actor_unresolved",
                })),
            )
        })?;

    let remote_actor_id: i32 = remote.id;

    // 检查是否已有同类型的 pending/accepted/active Channel
    let existing = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT channel_id, status, transport, tapp_id, properties, initiated_by,
                      last_activity_at, created_at
               FROM federation_channels
               WHERE user_id = $1 AND remote_actor_id = $2 AND channel_type = $3
                     AND status IN ('pending', 'accepted', 'active')
               LIMIT 1"#,
            [user_id.into(), remote_actor_id.into(), channel_type.into()],
        ))
        .await
        .map_err(db_err)?;

    if let Some(row) = existing {
        return Ok(ChannelDetail {
            channel_id: row.try_get("", "channel_id").unwrap_or_default(),
            remote_actor_url: remote_actor_url.clone(),
            remote_actor_name: remote
                .display_name
                .clone()
                .or_else(|| remote.username.clone()),
            remote_actor_avatar: remote.avatar_url.clone(),
            channel_type: channel_type.to_string(),
            status: row.try_get::<String>("", "status").unwrap_or_default(),
            transport: row.try_get::<String>("", "transport").unwrap_or_default(),
            tapp_id: row.try_get::<Option<String>>("", "tapp_id").unwrap_or(None),
            properties: row
                .try_get::<Option<serde_json::Value>>("", "properties")
                .unwrap_or(None),
            initiated_by: row
                .try_get::<String>("", "initiated_by")
                .unwrap_or_default(),
            last_activity_at: row
                .try_get::<Option<chrono::DateTime<chrono::FixedOffset>>>("", "last_activity_at")
                .ok()
                .flatten()
                .map(|t| t.to_rfc3339()),
            created_at: row
                .try_get::<chrono::DateTime<chrono::FixedOffset>>("", "created_at")
                .map(|t| t.to_rfc3339())
                .unwrap_or_default(),
        });
    }

    // 创建新 Channel
    let channel_id = generate_channel_id();
    let properties = json!({
        // Align with live message_payload_limit (default 4 MiB; saver lower)
        "maxMessageSize": crate::federation::limits::message_payload_limit(),
        "supportedFormats": ["text/plain", "text/markdown", "application/json"]
    });

    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"INSERT INTO federation_channels
           (channel_id, user_id, remote_actor_id, channel_type, tapp_id, status, transport, properties, initiated_by, created_at)
           VALUES ($1, $2, $3, $4, $5, 'pending', $6, $7, 'local', NOW())"#,
        [
            channel_id.clone().into(),
            user_id.into(),
            remote_actor_id.into(),
            channel_type.into(),
            req.tapp_id.clone().into(),
            transport.into(),
            properties.clone().into(),
        ],
    ))
    .await
    .map_err(db_err)?;

    // 向远程 Actor 发送 ChannelOpen Activity
    let activity_id = generate_activity_id(&base_url);

    let channel_open = json!({
        "@context": build_context(),
        "type": "myriad:ChannelOpen",
        "id": &activity_id,
        "actor": &local_actor,
        "to": [&remote_actor_url],
        "object": {
            "type": "myriad:Channel",
            "id": &channel_id,
            "channelType": channel_type,
            "tappId": req.tapp_id,
            "protocol": "mfp/1.0",
            "transportPreference": [transport]
        }
    });

    // 记录 Activity 并投递
    let inbox = &remote.inbox_url;
    if !inbox.is_empty() {
        let domain = extract_domain(inbox).unwrap_or_default();
        let act_row = db
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"INSERT INTO federation_activities
                   (activity_id, user_id, activity_type, object_type, object_json, is_local, published_at)
                   VALUES ($1, $2, 'ChannelOpen', 'Channel', $3, true, NOW())
                   RETURNING id"#,
                [
                    activity_id.clone().into(),
                    user_id.into(),
                    channel_open.clone().into(),
                ],
            ))
            .await
            .map_err(db_err)?;

        if let Some(act_id) = act_row.and_then(|r| r.try_get::<i32>("", "id").ok()) {
            let _ = db
                .execute_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    r#"INSERT INTO federation_delivery_queue
                       (activity_id, target_inbox, target_domain, status, created_at)
                       VALUES ($1, $2, $3, 'pending', NOW())
                   ON CONFLICT (activity_id, target_inbox) DO NOTHING"#,
                    [act_id.into(), inbox.into(), domain.into()],
                ))
                .await;
        }
    }

    tracing::info!(
        "[Channel] Created channel {} with remote {}",
        channel_id,
        remote_actor_url
    );

    Ok(ChannelDetail {
        channel_id,
        remote_actor_url,
        remote_actor_name: remote
            .display_name
            .clone()
            .or_else(|| remote.username.clone()),
        remote_actor_avatar: remote.avatar_url.clone(),
        channel_type: channel_type.to_string(),
        status: "pending".to_string(),
        transport: transport.to_string(),
        tapp_id: req.tapp_id.clone(),
        properties: Some(properties),
        initiated_by: "local".to_string(),
        last_activity_at: None,
        created_at: chrono::Utc::now().to_rfc3339(),
    })
}

/// 获取用户的所有 Channel 列表
pub async fn list_channels(
    user_id: i32,
    username: &str,
    db: &DatabaseConnection,
) -> Result<Vec<ChannelSummary>, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;
    let local_actor = actor_url(&base_url, username);

    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT c.channel_id, c.channel_type, c.status, c.transport, c.initiated_by,
                      c.last_activity_at, c.created_at,
                      ra.actor_url,
                      COALESCE(NULLIF(ra.display_name, ''), ra.username) AS remote_actor_name,
                      ra.avatar_url,
                      COALESCE((SELECT COUNT(*) FROM federation_channel_messages m
                                WHERE m.channel_id = c.channel_id
                                  AND m.sender_actor != $2
                                  AND m.created_at > COALESCE(c.last_activity_at, c.created_at)), 0) AS unread_count
               FROM federation_channels c
               JOIN federation_remote_actors ra ON c.remote_actor_id = ra.id
               WHERE c.user_id = $1
               ORDER BY COALESCE(c.last_activity_at, c.created_at) DESC"#,
            [user_id.into(), local_actor.into()],
        ))
        .await
        .map_err(db_err)?;

    let mut channels = Vec::new();
    for row in rows {
        channels.push(ChannelSummary {
            channel_id: row.try_get("", "channel_id").unwrap_or_default(),
            remote_actor_url: row.try_get("", "actor_url").unwrap_or_default(),
            remote_actor_name: row
                .try_get::<Option<String>>("", "remote_actor_name")
                .unwrap_or(None),
            remote_actor_avatar: row
                .try_get::<Option<String>>("", "avatar_url")
                .unwrap_or(None),
            channel_type: row.try_get("", "channel_type").unwrap_or_default(),
            status: row.try_get("", "status").unwrap_or_default(),
            transport: row.try_get("", "transport").unwrap_or_default(),
            initiated_by: row.try_get("", "initiated_by").unwrap_or_default(),
            last_activity_at: row
                .try_get::<Option<chrono::DateTime<chrono::FixedOffset>>>("", "last_activity_at")
                .ok()
                .flatten()
                .map(|t| t.to_rfc3339()),
            created_at: row
                .try_get::<chrono::DateTime<chrono::FixedOffset>>("", "created_at")
                .map(|t| t.to_rfc3339())
                .unwrap_or_default(),
            unread_count: row.try_get::<i64>("", "unread_count").unwrap_or(0),
        });
    }

    Ok(channels)
}

/// 获取单个 Channel 详情
pub async fn get_channel(
    user_id: i32,
    channel_id: &str,
    db: &DatabaseConnection,
) -> Result<ChannelDetail, (StatusCode, Json<serde_json::Value>)> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT c.channel_id, c.channel_type, c.status, c.transport, c.tapp_id,
                      c.properties, c.initiated_by, c.last_activity_at, c.created_at,
                      ra.actor_url,
                      COALESCE(NULLIF(ra.display_name, ''), ra.username) AS remote_actor_name,
                      ra.avatar_url
               FROM federation_channels c
               JOIN federation_remote_actors ra ON c.remote_actor_id = ra.id
               WHERE c.user_id = $1 AND c.channel_id = $2"#,
            [user_id.into(), channel_id.into()],
        ))
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(AppError::public_json("Channel not found")),
            )
        })?;

    Ok(ChannelDetail {
        channel_id: row.try_get("", "channel_id").unwrap_or_default(),
        remote_actor_url: row.try_get("", "actor_url").unwrap_or_default(),
        remote_actor_name: row
            .try_get::<Option<String>>("", "remote_actor_name")
            .unwrap_or(None),
        remote_actor_avatar: row
            .try_get::<Option<String>>("", "avatar_url")
            .unwrap_or(None),
        channel_type: row.try_get("", "channel_type").unwrap_or_default(),
        status: row.try_get("", "status").unwrap_or_default(),
        transport: row.try_get("", "transport").unwrap_or_default(),
        tapp_id: row.try_get::<Option<String>>("", "tapp_id").unwrap_or(None),
        properties: row
            .try_get::<Option<serde_json::Value>>("", "properties")
            .unwrap_or(None),
        initiated_by: row.try_get("", "initiated_by").unwrap_or_default(),
        last_activity_at: row
            .try_get::<Option<chrono::DateTime<chrono::FixedOffset>>>("", "last_activity_at")
            .ok()
            .flatten()
            .map(|t| t.to_rfc3339()),
        created_at: row
            .try_get::<chrono::DateTime<chrono::FixedOffset>>("", "created_at")
            .map(|t| t.to_rfc3339())
            .unwrap_or_default(),
    })
}

/// 关闭 Channel
pub async fn close_channel(
    user_id: i32,
    username: &str,
    channel_id: &str,
    db: &DatabaseConnection,
) -> Result<serde_json::Value, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;

    // 验证通道归属 & 获取远程 actor 信息
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT c.status, ra.actor_url, ra.inbox_url
               FROM federation_channels c
               JOIN federation_remote_actors ra ON c.remote_actor_id = ra.id
               WHERE c.user_id = $1 AND c.channel_id = $2"#,
            [user_id.into(), channel_id.into()],
        ))
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(AppError::public_json("Channel not found")),
            )
        })?;

    let status: String = row.try_get("", "status").unwrap_or_default();
    if status == "closed" {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json("Channel already closed")),
        ));
    }

    let remote_actor_url: String = row.try_get("", "actor_url").unwrap_or_default();
    let remote_inbox: Option<String> = row
        .try_get::<Option<String>>("", "inbox_url")
        .unwrap_or(None);

    // 更新状态
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE federation_channels SET status = 'closed', closed_at = NOW() WHERE channel_id = $1",
        [channel_id.into()],
    ))
    .await
    .map_err(db_err)?;

    // Drop pending KeyExchange / messages for this channel (remote will not accept after close).
    // Order: cancel stale *before* enqueue ChannelClose. cancel_pending also excludes
    // ChannelClose/RoomDissolve activity types so a reverse order would still be safe.
    let _ = crate::federation::delivery::cancel_pending_deliveries_for_resource(
        db,
        channel_id,
        "cancelled: local channel closed",
    )
    .await;

    // Notify other local tabs/devices immediately (remote path already broadcasts in handle_channel_close)
    crate::federation::ws_gateway::broadcast_to_channel(
        channel_id,
        &json!({
            "type": "channel_closed",
            "channel_id": channel_id
        }),
    )
    .await;

    // 通知远程方
    let local_actor = actor_url(&base_url, username);
    let activity_id = generate_activity_id(&base_url);
    let close_activity = json!({
        "@context": build_context(),
        "type": "myriad:ChannelClose",
        "id": &activity_id,
        "actor": &local_actor,
        "to": [&remote_actor_url],
        "object": {
            "type": "myriad:Channel",
            "id": channel_id
        }
    });

    if let Some(inbox) = remote_inbox {
        let domain = extract_domain(&inbox).unwrap_or_default();
        let act_row = db
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"INSERT INTO federation_activities
                   (activity_id, user_id, activity_type, object_type, object_json, is_local, published_at)
                   VALUES ($1, $2, 'ChannelClose', 'Channel', $3, true, NOW())
                   RETURNING id"#,
                [
                    activity_id.clone().into(),
                    user_id.into(),
                    close_activity.clone().into(),
                ],
            ))
            .await
            .map_err(db_err)?;

        if let Some(act_id) = act_row.and_then(|r| r.try_get::<i32>("", "id").ok()) {
            let _ = db
                .execute_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    r#"INSERT INTO federation_delivery_queue
                       (activity_id, target_inbox, target_domain, status, created_at)
                       VALUES ($1, $2, $3, 'pending', NOW())
                   ON CONFLICT (activity_id, target_inbox) DO NOTHING"#,
                    [act_id.into(), inbox.into(), domain.into()],
                ))
                .await;
        }
    }

    tracing::info!("[Channel] Closed channel {}", channel_id);

    Ok(json!({
        "success": true,
        "channel_id": channel_id,
        "status": "closed"
    }))
}

/// Hard-delete a closed Channel (local row only; no remote notification).
///
/// Requires the channel to belong to the user and to already be `closed`.
/// Deletes messages, related file-transfer rows, cancels pending deliveries,
/// then removes the channel row.
pub async fn delete_channel(
    user_id: i32,
    channel_id: &str,
    db: &DatabaseConnection,
) -> Result<serde_json::Value, (StatusCode, Json<serde_json::Value>)> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT status FROM federation_channels WHERE user_id = $1 AND channel_id = $2",
            [user_id.into(), channel_id.into()],
        ))
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(AppError::public_json("Channel not found")),
            )
        })?;

    let status: String = row.try_get("", "status").unwrap_or_default();
    if status != "closed" {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json(
                "Channel must be closed before delete",
            )),
        ));
    }

    // Messages first
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM federation_channel_messages WHERE channel_id = $1",
        [channel_id.into()],
    ))
    .await
    .map_err(db_err)?;

    // Related transfer rows (channel_id is not always FK-enforced)
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM federation_file_transfers WHERE channel_id = $1",
        [channel_id.into()],
    ))
    .await
    .map_err(db_err)?;

    // Stop any leftover outbound deliveries for this channel id
    let _ = crate::federation::delivery::cancel_pending_deliveries_for_resource(
        db,
        channel_id,
        "cancelled: local channel deleted",
    )
    .await;

    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM federation_channels WHERE user_id = $1 AND channel_id = $2",
        [user_id.into(), channel_id.into()],
    ))
    .await
    .map_err(db_err)?;

    // Notify other local tabs/devices so they drop the conversation
    crate::federation::ws_gateway::broadcast_to_channel(
        channel_id,
        &json!({
            "type": "channel_deleted",
            "channel_id": channel_id
        }),
    )
    .await;

    tracing::info!("[Channel] Deleted channel {}", channel_id);

    Ok(json!({
        "success": true,
        "channel_id": channel_id
    }))
}

// 消息功能

/// 发送消息到 Channel
pub async fn send_message(
    user_id: i32,
    username: &str,
    channel_id: &str,
    db: &DatabaseConnection,
    req: &SendMessageRequest,
) -> Result<SendMessageResponse, (StatusCode, Json<serde_json::Value>)> {
    // 验证载荷大小（default 4 MiB；内存节约档略低）
    let max_payload = crate::federation::limits::message_payload_limit();
    let payload_size = req.payload.to_string().len();
    if payload_size > max_payload {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(
                AppError::from_status_u16(
                    413,
                    format!(
                        "Message payload too large: {} bytes (max {})",
                        payload_size, max_payload
                    ),
                )
                .with_code("payload_too_large")
                .to_json(),
            ),
        ));
    }

    let base_url = get_base_url().await;
    let message_type = req.message_type.as_deref().unwrap_or("text");
    let want_encrypt = req.encrypt.unwrap_or(false);

    // 验证通道存在且为 active 或 accepted
    let ch_row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT c.status, c.properties, ra.actor_url, ra.inbox_url
               FROM federation_channels c
               JOIN federation_remote_actors ra ON c.remote_actor_id = ra.id
               WHERE c.user_id = $1 AND c.channel_id = $2"#,
            [user_id.into(), channel_id.into()],
        ))
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(AppError::public_json("Channel not found")),
            )
        })?;

    let status: String = ch_row.try_get("", "status").unwrap_or_default();
    if !["active", "accepted"].contains(&status.as_str()) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Channel is not ready to send messages",
                "code": "channel_not_ready",
            })),
        ));
    }

    let remote_inbox: Option<String> = ch_row
        .try_get::<Option<String>>("", "inbox_url")
        .unwrap_or(None);
    let remote_actor_url: String = ch_row.try_get("", "actor_url").unwrap_or_default();
    let properties: Option<serde_json::Value> = ch_row
        .try_get::<Option<serde_json::Value>>("", "properties")
        .unwrap_or(None);

    // 可选：E2E 加密载荷。
    // 会话未就绪时降级明文（Aro 默认 encrypt=true，硬失败会导致「发消息失败」）。
    let (stored_payload, is_encrypted) = if want_encrypt {
        match load_e2e_session(channel_id, properties.as_ref()).await {
            Ok(session) if session.established => {
                match crate::federation::e2e::encrypt_json_payload(&session, &req.payload) {
                    Ok(encrypted) => (encrypted, true),
                    Err(e) => {
                        tracing::warn!(
                            channel_id = %channel_id,
                            error = %e,
                            "E2E encrypt failed; falling back to plaintext"
                        );
                        (req.payload.clone(), false)
                    }
                }
            }
            Ok(_) => {
                tracing::debug!(
                    channel_id = %channel_id,
                    "E2E not established yet; sending plaintext"
                );
                (req.payload.clone(), false)
            }
            Err(e) => {
                tracing::debug!(
                    channel_id = %channel_id,
                    error = %e,
                    "E2E session unavailable; sending plaintext"
                );
                (req.payload.clone(), false)
            }
        }
    } else {
        (req.payload.clone(), false)
    };

    // 存入消息
    let message_id = generate_message_id();
    let local_actor = actor_url(&base_url, username);

    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"INSERT INTO federation_channel_messages
           (channel_id, message_id, sender_actor, message_type, payload, reply_to, is_encrypted, created_at)
           VALUES ($1, $2, $3, $4, $5, $6, $7, NOW())"#,
        [
            channel_id.into(),
            message_id.clone().into(),
            local_actor.clone().into(),
            message_type.into(),
            stored_payload.clone().into(),
            req.reply_to.clone().into(),
            is_encrypted.into(),
        ],
    ))
    .await
    .map_err(db_err)?;

    // 更新通道最后活动时间；如果 accepted → active
    let new_status = if status == "accepted" {
        "active"
    } else {
        &status
    };
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE federation_channels SET last_activity_at = NOW(), status = $2 WHERE channel_id = $1",
        [channel_id.into(), new_status.into()],
    ))
    .await
    .map_err(db_err)?;

    // 通过 ActivityPub 投递消息给远程方
    let activity_id = generate_activity_id(&base_url);
    let msg_activity = json!({
        "@context": build_context(),
        "type": "myriad:ChannelMessage",
        "id": &activity_id,
        "actor": &local_actor,
        "to": [&remote_actor_url],
        "object": {
            "type": "myriad:ChannelMessage",
            "channel": channel_id,
            "messageId": &message_id,
            "messageType": message_type,
            "from": &local_actor,
            "payload": &stored_payload,
            "isEncrypted": is_encrypted,
            "replyTo": &req.reply_to,
            "timestamp": now_iso8601()
        }
    });

    let mut delivery = crate::federation::delivery::DeliveryEnqueueInfo {
        remote_targets: 1,
        ..Default::default()
    };
    if let Some(inbox) = remote_inbox.filter(|s| !s.is_empty()) {
        let domain = extract_domain(&inbox).unwrap_or_default();
        let act_row = db
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"INSERT INTO federation_activities
                   (activity_id, user_id, activity_type, object_type, object_json, is_local, published_at)
                   VALUES ($1, $2, 'ChannelMessage', 'ChannelMessage', $3, true, NOW())
                   RETURNING id"#,
                [
                    activity_id.clone().into(),
                    user_id.into(),
                    msg_activity.clone().into(),
                ],
            ))
            .await
            .map_err(db_err)?;

        if let Some(act_id) = act_row.and_then(|r| r.try_get::<i32>("", "id").ok()) {
            match db
                .execute_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    r#"INSERT INTO federation_delivery_queue
                       (activity_id, target_inbox, target_domain, status, created_at)
                       VALUES ($1, $2, $3, 'pending', NOW())
                   ON CONFLICT (activity_id, target_inbox) DO NOTHING"#,
                    [act_id.into(), inbox.into(), domain.into()],
                ))
                .await
            {
                Ok(_) => delivery.queued = 1,
                Err(e) => {
                    tracing::error!(
                        "[Channel] enqueue delivery failed channel={}: {}",
                        channel_id,
                        e
                    );
                    delivery.warning = Some(format!("enqueue_failed: {}", e));
                }
            }
        } else {
            delivery.warning = Some("activity_insert_failed".into());
        }
    } else {
        delivery.warning = Some("remote_inbox_missing".into());
        tracing::warn!(
            "[Channel] message {} has no remote inbox for channel {}",
            message_id,
            channel_id
        );
    }

    // 广播给该 Channel 的 WebSocket 连接。
    // 本地展示用：密文则尝试解密后广播明文，避免 Aro 先渲染 ciphertext 信封、
    // 等 poll/getMessages 才正常（WS 回声还可能覆盖已解密内容）。
    // 成功解密后 is_encrypted 也必须改 false，否则客户端会按 flag 再解一次。
    // DB / ActivityPub fan-out 仍用 stored_payload 密文；HTTP 响应的 is_encrypted 反映存储形态。
    let mut ws_payload = stored_payload.clone();
    let mut ws_is_encrypted = is_encrypted;
    if is_encrypted {
        // 解密不要求 established：信封自带发送方公钥，本地私钥在就能试。
        if let Ok(session) = load_e2e_session(channel_id, properties.as_ref()).await {
            if let Ok(plain) =
                crate::federation::e2e::decrypt_json_payload(&session, &stored_payload)
            {
                ws_payload = plain;
                ws_is_encrypted = false;
            }
        }
    }
    crate::federation::ws_gateway::broadcast_to_channel(
        channel_id,
        &json!({
            "type": "message",
            "channel_id": channel_id,
            "message": {
                "message_id": &message_id,
                "sender_actor": &local_actor,
                "message_type": message_type,
                "payload": &ws_payload,
                "is_encrypted": ws_is_encrypted,
                "reply_to": &req.reply_to,
                "created_at": now_iso8601()
            }
        }),
    )
    .await;

    Ok(SendMessageResponse {
        success: true,
        message_id,
        channel_id: channel_id.to_string(),
        is_encrypted,
        delivery: Some(delivery),
    })
}

/// 获取 Channel 消息历史
pub async fn get_messages(
    user_id: i32,
    channel_id: &str,
    db: &DatabaseConnection,
    before: Option<&str>,
    limit: Option<i64>,
) -> Result<Vec<MessageItem>, (StatusCode, Json<serde_json::Value>)> {
    // 验证通道归属，并读取 E2E 状态以便本地解密
    let ch_row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT properties FROM federation_channels WHERE user_id = $1 AND channel_id = $2",
            [user_id.into(), channel_id.into()],
        ))
        .await
        .map_err(db_err)?;

    let ch_row = match ch_row {
        Some(r) => r,
        None => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(AppError::public_json("Channel not found")),
            ))
        }
    };

    let properties = ch_row
        .try_get::<Option<serde_json::Value>>("", "properties")
        .ok()
        .flatten();
    let e2e_session = load_e2e_session(channel_id, properties.as_ref()).await.ok();

    let limit = limit.unwrap_or(50).min(200);

    let rows = if let Some(before_id) = before {
        db.query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT message_id, sender_actor, message_type, payload, reply_to, is_encrypted, created_at
               FROM federation_channel_messages
               WHERE channel_id = $1
                 AND created_at < (SELECT created_at FROM federation_channel_messages WHERE message_id = $2)
               ORDER BY created_at DESC
               LIMIT $3"#,
            [channel_id.into(), before_id.into(), limit.into()],
        ))
        .await
        .map_err(db_err)?
    } else {
        db.query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT message_id, sender_actor, message_type, payload, reply_to, is_encrypted, created_at
               FROM federation_channel_messages
               WHERE channel_id = $1
               ORDER BY created_at DESC
               LIMIT $2"#,
            [channel_id.into(), limit.into()],
        ))
        .await
        .map_err(db_err)?
    };

    let mut messages = Vec::new();
    for row in rows {
        let is_encrypted: bool = row.try_get("", "is_encrypted").unwrap_or(false);
        let mut payload: serde_json::Value = row.try_get("", "payload").unwrap_or(json!(null));
        // 本地持有会话时，把加密信封还原为明文 JSON（DB 仍保留密文）
        let mut display_encrypted = is_encrypted;
        if is_encrypted {
            if let Some(session) = e2e_session.as_ref() {
                if let Ok(plain) = crate::federation::e2e::decrypt_json_payload(session, &payload) {
                    payload = plain;
                    display_encrypted = false;
                }
            }
        }
        messages.push(MessageItem {
            message_id: row.try_get("", "message_id").unwrap_or_default(),
            sender_actor: row.try_get("", "sender_actor").unwrap_or_default(),
            message_type: row.try_get("", "message_type").unwrap_or_default(),
            payload,
            reply_to: row
                .try_get::<Option<String>>("", "reply_to")
                .unwrap_or(None),
            is_encrypted: display_encrypted,
            created_at: row
                .try_get::<chrono::DateTime<chrono::FixedOffset>>("", "created_at")
                .map(|t| t.to_rfc3339())
                .unwrap_or_default(),
        });
    }

    // 返回时按时间正序（最新在后）
    messages.reverse();

    Ok(messages)
}
