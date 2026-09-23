//! Inbound ChannelOpen / ChannelMessage / ChannelClose.
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use serde_json::json;

use crate::federation::types::*;

use super::e2e::load_e2e_session;

// Inbox 处理（远程 Channel 事件）

/// 处理收到的 ChannelOpen Activity
pub async fn handle_channel_open(
    db: &impl ConnectionTrait,
    actor_url_str: &str,
    activity: &serde_json::Value,
) -> Result<(), String> {
    let object = activity.get("object").ok_or("Missing object")?;
    let channel_id = object
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or("Missing channel id")?;
    let channel_type = object
        .get("channelType")
        .and_then(|v| v.as_str())
        .unwrap_or("text");
    let tapp_id = object.get("tappId").and_then(|v| v.as_str());
    let transport = object
        .get("transportPreference")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .and_then(|v| v.as_str())
        .unwrap_or("websocket");

    // 查找本地对应的远程 actor 记录
    let actor_row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id FROM federation_remote_actors WHERE actor_url = $1",
            [actor_url_str.into()],
        ))
        .await
        .map_err(|e| e.to_string())?
        .ok_or("Remote actor not found")?;

    let remote_actor_id = crate::federation::types::row_positive_id(&actor_row, "id")?;

    // 路由目标用户：优先按 Activity 的 to 字段解析本地 actor（发起方在
    // create_channel 里总会把目标 actor URL 写进 to）；解析不出时按关注
    // 关系回退（兼容不带 to 或 to 里只有集合地址的异构发送方）。都找不到
    // 就丢弃——绝不回退给任意本地用户，避免陌生通道被塞进错误的收件箱。
    let base_url = get_base_url().await;
    let to_entries: Vec<&str> = match activity.get("to") {
        Some(serde_json::Value::Array(arr)) => arr.iter().filter_map(|v| v.as_str()).collect(),
        Some(serde_json::Value::String(s)) => vec![s.as_str()],
        _ => Vec::new(),
    };

    // 防 DB 放大：`to` 数组可被塞入海量条目，只看前 `MAX_TO_LOOKUPS` 个。
    const MAX_TO_LOOKUPS: usize = 16;
    let mut target_user_id = first_local_recipient(
        db,
        &base_url,
        to_entries.iter().take(MAX_TO_LOOKUPS).copied(),
    )
    .await
    .map_err(|e| e.to_string())?
    .map(|(uid, _)| uid);

    if target_user_id.is_none() {
        // 关注关系回退：to 缺失/无法解析（如 BASE_URL 迁移后远端持有旧 URL）
        target_user_id = db
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"SELECT user_id FROM federation_follows
                   WHERE remote_actor_id = $1 AND status = 'accepted'
                   LIMIT 1"#,
                [remote_actor_id.into()],
            ))
            .await
            .map_err(|e| e.to_string())?
            .and_then(|r| r.try_get("", "user_id").ok());
    }

    let Some(target_user_id) = target_user_id else {
        // Unroutable: log and Ok(()) → 202 so the peer does not retry.
        tracing::warn!(
            "[Channel] Dropping unroutable ChannelOpen from {} (to={:?})",
            actor_url_str,
            to_entries.iter().take(MAX_TO_LOOKUPS).collect::<Vec<_>>()
        );
        return Ok(());
    };

    // 创建本地 Channel 记录
    let inserted = match db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"INSERT INTO federation_channels
           (channel_id, user_id, remote_actor_id, channel_type, tapp_id, status, transport, initiated_by, created_at)
           VALUES ($1, $2, $3, $4, $5, 'pending', $6, 'remote', NOW())
           ON CONFLICT (channel_id) DO NOTHING"#,
            [
                channel_id.into(),
                target_user_id.into(),
                remote_actor_id.into(),
                channel_type.into(),
                tapp_id.into(),
                transport.into(),
            ],
        ))
        .await
    {
        Ok(result) => result,
        Err(e) if crate::federation::types::is_unique_violation(&e) => {
            tracing::info!(
                "[Channel] ChannelOpen relationship already active for user {} type {}",
                target_user_id,
                channel_type
            );
            return Ok(());
        }
        Err(e) => return Err(e.to_string()),
    };

    if inserted.rows_affected() > 0 {
        let label = crate::federation::notify::actor_label(db, actor_url_str).await;
        crate::federation::notify::notify_channel_invite(
            target_user_id,
            channel_id,
            actor_url_str,
            &label,
        )
        .await;
    }

    tracing::info!(
        "[Channel] Received ChannelOpen {} from {}",
        channel_id,
        actor_url_str
    );

    Ok(())
}

/// 处理收到的 ChannelMessage Activity
pub async fn handle_channel_message(
    db: &impl ConnectionTrait,
    actor_url_str: &str,
    activity: &serde_json::Value,
) -> Result<(), String> {
    let object = activity.get("object").ok_or("Missing object")?;
    let channel_id = object
        .get("channel")
        .and_then(|v| v.as_str())
        .ok_or("Missing channel")?;

    // 验证通道存在且发送方是该通道的远程方
    let ch_check = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT c.status, c.user_id FROM federation_channels c
               JOIN federation_remote_actors ra ON c.remote_actor_id = ra.id
               WHERE c.channel_id = $1 AND ra.actor_url = $2
               FOR UPDATE OF c"#,
            [channel_id.into(), actor_url_str.into()],
        ))
        .await
        .map_err(|e| e.to_string())?;

    if ch_check.is_none() {
        // Distinguish "channel not yet created" (race with ChannelOpen) from
        // "wrong remote actor" (permanent). Buffer only the race case.
        let channel_exists = db
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT 1 FROM federation_channels WHERE channel_id = $1",
                [channel_id.into()],
            ))
            .await
            .map_err(|e| e.to_string())?
            .is_some();

        if !channel_exists {
            // ChannelOpen may still be in flight. Fail (no 202) so the remote
            // retries; duplicates are idempotent via message_id.
            tracing::info!(
                channel_id,
                actor = actor_url_str,
                "[Channel] Channel not yet present; signaling retry"
            );
            return Err(format!(
                "Channel {} not yet present; retry after ChannelOpen",
                channel_id
            ));
        }

        return Err(format!(
            "Channel {} not found or actor {} is not the remote party",
            channel_id, actor_url_str
        ));
    }

    let ch_status: String = ch_check
        .as_ref()
        .and_then(|r| r.try_get::<String>("", "status").ok())
        .unwrap_or_default();
    if ch_status == "closed" {
        return Err(format!("Channel {} is closed", channel_id));
    }
    let owner_user_id: Option<i32> = ch_check
        .as_ref()
        .and_then(|r| r.try_get::<i32>("", "user_id").ok());

    let fallback_msg_id = generate_message_id();
    let message_id = object
        .get("messageId")
        .and_then(|v| v.as_str())
        .unwrap_or(&fallback_msg_id);
    // object.from 必须与签名 actor 一致，并且落库一律存签名 actor。
    if let Some(claimed) = object.get("from").and_then(|v| v.as_str()) {
        if !same_actor_url(claimed, actor_url_str) {
            return Err(format!(
                "ChannelMessage from {claimed} does not match signed actor {actor_url_str}"
            ));
        }
    }
    let sender = actor_url_str;
    let message_type = object
        .get("messageType")
        .and_then(|v| v.as_str())
        .unwrap_or("text");
    let payload = object.get("payload").cloned().unwrap_or(json!(null));
    let reply_to = object.get("replyTo").and_then(|v| v.as_str());
    let is_encrypted = object
        .get("isEncrypted")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // 存入消息
    let inserted = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"INSERT INTO federation_channel_messages
           (channel_id, message_id, sender_actor, message_type, payload, reply_to, is_encrypted, created_at)
           VALUES ($1, $2, $3, $4, $5, $6, $7, NOW())
           ON CONFLICT (message_id) DO NOTHING"#,
            [
                channel_id.into(),
                message_id.into(),
                sender.into(),
                message_type.into(),
                payload.clone().into(),
                reply_to.into(),
                is_encrypted.into(),
            ],
        ))
        .await
        .map_err(|e| e.to_string())?;

    // 更新通道活动时间
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE federation_channels SET last_activity_at = NOW(), status = 'active' WHERE channel_id = $1",
        [channel_id.into()],
    ))
    .await
    .map_err(|e| e.to_string())?;

    // 广播到 WebSocket。E2E 时优先明文，避免收件人先闪 ciphertext。
    let mut ws_payload = payload.clone();
    let mut ws_is_encrypted = is_encrypted;
    if is_encrypted {
        if let Ok(Some(prop_row)) = db
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT properties FROM federation_channels WHERE channel_id = $1",
                [channel_id.into()],
            ))
            .await
        {
            let properties = prop_row
                .try_get::<Option<serde_json::Value>>("", "properties")
                .ok()
                .flatten();
            if let Ok(session) = load_e2e_session(channel_id, properties.as_ref()).await {
                if let Ok(plain) = crate::federation::e2e::decrypt_json_payload(&session, &payload)
                {
                    ws_payload = plain;
                    ws_is_encrypted = false;
                }
            }
        }
    }
    crate::federation::ws_gateway::broadcast_to_channel(
        channel_id,
        &json!({
            "type": "message",
            "channel_id": channel_id,
            "message": {
                "message_id": message_id,
                "sender_actor": sender,
                "message_type": message_type,
                "payload": ws_payload,
                "is_encrypted": ws_is_encrypted,
                "reply_to": reply_to,
                "created_at": now_iso8601()
            }
        }),
    )
    .await;

    // 新消息才推通知中心（重放/去重不通知）。
    // 跳过自己发的内容（本机 actor 回环 / 同实例双端）。
    // 预览用已解密的 ws_payload，避免通知栏出现 ciphertext JSON。
    if inserted.rows_affected() > 0 {
        if let Some(user_id) = owner_user_id {
            let base_url = get_base_url().await;
            let is_self = db
                .query_one_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "SELECT username FROM users WHERE id = $1 LIMIT 1",
                    [user_id.into()],
                ))
                .await
                .ok()
                .flatten()
                .and_then(|r| r.try_get::<String>("", "username").ok())
                .map(|uname| {
                    let mine = actor_url(&base_url, &uname);
                    same_actor_url(sender, &mine)
                })
                .unwrap_or(false);
            if !is_self {
                let label = crate::federation::notify::actor_label(db, sender).await;
                crate::federation::notify::notify_channel_message(
                    user_id,
                    channel_id,
                    sender,
                    &label,
                    message_type,
                    &ws_payload,
                )
                .await;
            }
        }
    }

    tracing::info!(
        "[Channel] Received message {} in channel {}",
        message_id,
        channel_id
    );

    Ok(())
}

/// 处理收到的 ChannelClose Activity
pub async fn handle_channel_close(
    db: &impl ConnectionTrait,
    actor_url_str: &str,
    activity: &serde_json::Value,
) -> Result<(), String> {
    let object = activity.get("object").ok_or("Missing object")?;
    let channel_id = object
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or("Missing channel id")?;

    // 验证发送方是该通道的远程方
    let ch_check = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT 1 FROM federation_channels c
               JOIN federation_remote_actors ra ON c.remote_actor_id = ra.id
               WHERE c.channel_id = $1 AND ra.actor_url = $2"#,
            [channel_id.into(), actor_url_str.into()],
        ))
        .await
        .map_err(|e| e.to_string())?;

    if ch_check.is_none() {
        return Err(format!(
            "Channel {} not found or actor {} is not the remote party",
            channel_id, actor_url_str
        ));
    }

    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE federation_channels SET status = 'closed', closed_at = NOW() WHERE channel_id = $1",
        [channel_id.into()],
    ))
    .await
    .map_err(|e| e.to_string())?;

    // 通知 WebSocket 连接
    crate::federation::ws_gateway::broadcast_to_channel(
        channel_id,
        &json!({
            "type": "channel_closed",
            "channel_id": channel_id
        }),
    )
    .await;

    tracing::info!("[Channel] Channel {} closed by remote", channel_id);

    Ok(())
}
