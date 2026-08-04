
use axum::{http::StatusCode, Json};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::federation::types::*;

// Early channel activity buffer
//
// ChannelOpen can race with ChannelMessage / KeyExchange: the latter may arrive
// before the channel row exists. Buffer briefly; on ChannelOpen flush in order.
// If the buffer is full or the entry expires, return an error so the peer retries.

struct BufferedChannelActivity {
    actor_url: String,
    activity: serde_json::Value,
    buffered_at: Instant,
}

const EARLY_MSG_TTL: Duration = Duration::from_secs(120);
pub(crate) const EARLY_MSG_MAX_PER_CHANNEL: usize = 64;
const EARLY_MSG_MAX_TOTAL: usize = 256;

fn early_activity_buffer() -> &'static Mutex<HashMap<String, Vec<BufferedChannelActivity>>> {
    static BUF: OnceLock<Mutex<HashMap<String, Vec<BufferedChannelActivity>>>> = OnceLock::new();
    BUF.get_or_init(|| Mutex::new(HashMap::new()))
}

fn purge_expired_early_activities(map: &mut HashMap<String, Vec<BufferedChannelActivity>>) {
    let now = Instant::now();
    map.retain(|_, msgs| {
        msgs.retain(|m| now.duration_since(m.buffered_at) < EARLY_MSG_TTL);
        !msgs.is_empty()
    });
}

/// Buffer ChannelMessage or KeyExchange that arrived before the channel row exists.
pub(crate) fn buffer_early_channel_activity(
    channel_id: &str,
    actor_url: &str,
    activity: &serde_json::Value,
) -> bool {
    let Ok(mut map) = early_activity_buffer().lock() else {
        return false;
    };
    purge_expired_early_activities(&mut map);

    let total: usize = map.values().map(|v| v.len()).sum();
    if total >= EARLY_MSG_MAX_TOTAL {
        return false;
    }

    let entry = map.entry(channel_id.to_string()).or_default();
    if entry.len() >= EARLY_MSG_MAX_PER_CHANNEL {
        return false;
    }

    // Deduplicate by AP activity id or messageId
    let dedupe_key = activity.get("id").and_then(|v| v.as_str()).or_else(|| {
        activity
            .get("object")
            .and_then(|o| o.get("messageId"))
            .and_then(|v| v.as_str())
    });
    if let Some(key) = dedupe_key {
        let already = entry.iter().any(|m| {
            m.activity.get("id").and_then(|v| v.as_str()) == Some(key)
                || m.activity
                    .get("object")
                    .and_then(|o| o.get("messageId"))
                    .and_then(|v| v.as_str())
                    == Some(key)
        });
        if already {
            return true;
        }
    }

    entry.push(BufferedChannelActivity {
        actor_url: actor_url.to_string(),
        activity: activity.clone(),
        buffered_at: Instant::now(),
    });
    true
}

/// Backward-compatible name used by ChannelMessage path.
fn buffer_early_channel_message(
    channel_id: &str,
    actor_url: &str,
    activity: &serde_json::Value,
) -> bool {
    buffer_early_channel_activity(channel_id, actor_url, activity)
}

fn take_early_channel_activities(channel_id: &str) -> Vec<BufferedChannelActivity> {
    let Ok(mut map) = early_activity_buffer().lock() else {
        return Vec::new();
    };
    purge_expired_early_activities(&mut map);
    map.remove(channel_id).unwrap_or_default()
}

async fn flush_early_channel_messages(db: &DatabaseConnection, channel_id: &str) {
    let items = take_early_channel_activities(channel_id);
    if items.is_empty() {
        return;
    }
    tracing::info!(
        "[Channel] Flushing {} buffered activit(y/ies) for {}",
        items.len(),
        channel_id
    );
    for m in items {
        let ty = m
            .activity
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let result = if ty == "myriad:KeyExchange" || ty == "KeyExchange" {
            handle_key_exchange(db, &m.actor_url, &m.activity).await
        } else {
            handle_channel_message(db, &m.actor_url, &m.activity).await
        };
        if let Err(e) = result {
            tracing::warn!(
                "[Channel] Failed to apply buffered {} for {}: {}",
                ty,
                channel_id,
                e
            );
        }
    }
}

// 请求/响应类型

/// 创建 Channel 请求
#[derive(Debug, Deserialize)]
pub struct CreateChannelRequest {
    /// 远程 Actor URL 或 acct:user@domain / @user@domain / user@domain
    pub remote_actor: String,
    /// 通道类型: text, file-transfer, rpc, data-exchange, stream
    pub channel_type: Option<String>,
    /// 关联 Tapp ID
    pub tapp_id: Option<String>,
    /// 传输方式: http, websocket
    pub transport: Option<String>,
}

/// 发送消息请求
#[derive(Debug, Deserialize)]
pub struct SendMessageRequest {
    /// 消息类型: text, file-meta, rpc-request, rpc-response, system
    pub message_type: Option<String>,
    /// 消息载荷
    pub payload: serde_json::Value,
    /// 回复的消息 ID
    pub reply_to: Option<String>,
    /// 是否使用 Channel E2E 加密载荷（需先完成密钥交换）
    #[serde(default)]
    pub encrypt: Option<bool>,
}

/// Channel 概要
#[derive(Debug, Serialize)]
pub struct ChannelSummary {
    pub channel_id: String,
    pub remote_actor_url: String,
    pub remote_actor_name: Option<String>,
    pub remote_actor_avatar: Option<String>,
    pub channel_type: String,
    pub status: String,
    pub transport: String,
    pub initiated_by: String,
    pub last_activity_at: Option<String>,
    pub created_at: String,
    pub unread_count: i64,
}

/// Channel 详情
#[derive(Debug, Serialize)]
pub struct ChannelDetail {
    pub channel_id: String,
    pub remote_actor_url: String,
    pub remote_actor_name: Option<String>,
    pub remote_actor_avatar: Option<String>,
    pub channel_type: String,
    pub status: String,
    pub transport: String,
    pub tapp_id: Option<String>,
    pub properties: Option<serde_json::Value>,
    pub initiated_by: String,
    pub last_activity_at: Option<String>,
    pub created_at: String,
}

/// 消息条目
#[derive(Debug, Clone, Serialize)]
pub struct MessageItem {
    pub message_id: String,
    pub sender_actor: String,
    pub message_type: String,
    pub payload: serde_json::Value,
    pub reply_to: Option<String>,
    pub is_encrypted: bool,
    pub created_at: String,
}

/// 发送消息响应
#[derive(Debug, Serialize)]
pub struct SendMessageResponse {
    pub success: bool,
    pub message_id: String,
    pub channel_id: String,
    /// 是否已对 payload 做 E2E 加密
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub is_encrypted: bool,
    /// Outbound delivery enqueue observability (remote peer)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery: Option<crate::federation::delivery::DeliveryEnqueueInfo>,
}

/// 发起 E2E 密钥交换响应
#[derive(Debug, Serialize)]
pub struct E2eKeyExchangeResponse {
    pub success: bool,
    pub channel_id: String,
    pub public_key: String,
    pub algorithm: String,
    pub established: bool,
}

// Channel CRUD 功能

/// 创建（或打开）一个新 Channel
///
/// 如果与该远程 Actor 已有 active/pending 的同类型 Channel，直接返回已有通道
pub async fn create_channel(
    user_id: i32,
    username: &str,
    db: &DatabaseConnection,
    req: &CreateChannelRequest,
) -> Result<ChannelDetail, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;
    let channel_type = req.channel_type.as_deref().unwrap_or("text");
    let transport = req.transport.as_deref().unwrap_or("websocket");

    // 验证 channel_type 和 transport
    if !["text", "file-transfer", "rpc", "data-exchange", "stream"].contains(&channel_type) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Invalid channel_type"})),
        ));
    }
    if !["http", "websocket"].contains(&transport) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Invalid transport"})),
        ));
    }

    let remote_actor_url =
        crate::federation::follow::resolve_actor_reference(&req.remote_actor).await?;
    let local_actor = actor_url(&base_url, username);
    if same_actor_url(&remote_actor_url, &local_actor) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Cannot create a channel with your own federation actor"})),
        ));
    }

    // 确保远程 Actor 已缓存
    let remote = crate::federation::actor::fetch_remote_actor(db, &remote_actor_url)
        .await
        .map_err(|e| {
            tracing::error!("[Channel] Failed to fetch remote actor: {}", e);
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": format!("Cannot resolve remote actor: {}", e)})),
            )
        })?;

    let remote_actor_id: i32 = remote.id;

    // 检查是否已有同类型的 active/pending Channel
    let existing = db
        .query_one(Statement::from_sql_and_values(
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
        // Align with MESSAGE_PAYLOAD_LIMIT (36 MiB — Tapp package share; MYR-002)
        "maxMessageSize": MAX_MESSAGE_PAYLOAD,
        "supportedFormats": ["text/plain", "text/markdown", "application/json"]
    });

    db.execute(Statement::from_sql_and_values(
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
            .query_one(Statement::from_sql_and_values(
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
                .execute(Statement::from_sql_and_values(
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
        .query_all(Statement::from_sql_and_values(
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
        .query_one(Statement::from_sql_and_values(
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
                Json(json!({"error": "Channel not found"})),
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
        .query_one(Statement::from_sql_and_values(
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
                Json(json!({"error": "Channel not found"})),
            )
        })?;

    let status: String = row.try_get("", "status").unwrap_or_default();
    if status == "closed" {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Channel already closed"})),
        ));
    }

    let remote_actor_url: String = row.try_get("", "actor_url").unwrap_or_default();
    let remote_inbox: Option<String> = row
        .try_get::<Option<String>>("", "inbox_url")
        .unwrap_or(None);

    // 更新状态
    db.execute(Statement::from_sql_and_values(
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
            .query_one(Statement::from_sql_and_values(
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
                .execute(Statement::from_sql_and_values(
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
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT status FROM federation_channels WHERE user_id = $1 AND channel_id = $2",
            [user_id.into(), channel_id.into()],
        ))
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "Channel not found"})),
            )
        })?;

    let status: String = row.try_get("", "status").unwrap_or_default();
    if status != "closed" {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Channel must be closed before delete"})),
        ));
    }

    // Messages first
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM federation_channel_messages WHERE channel_id = $1",
        [channel_id.into()],
    ))
    .await
    .map_err(db_err)?;

    // Related transfer rows (channel_id is not always FK-enforced)
    db.execute(Statement::from_sql_and_values(
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

    db.execute(Statement::from_sql_and_values(
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

/// 最大消息载荷大小（JSON 序列化后字符串长度）。
/// 取值与上限链的单一事实源见 [`crate::federation::limits`]。
use crate::federation::limits::MESSAGE_PAYLOAD_LIMIT as MAX_MESSAGE_PAYLOAD;

/// 发送消息到 Channel
pub async fn send_message(
    user_id: i32,
    username: &str,
    channel_id: &str,
    db: &DatabaseConnection,
    req: &SendMessageRequest,
) -> Result<SendMessageResponse, (StatusCode, Json<serde_json::Value>)> {
    // 验证载荷大小
    let payload_size = req.payload.to_string().len();
    if payload_size > MAX_MESSAGE_PAYLOAD {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(
                json!({"error": format!("Message payload too large: {} bytes (max {})", payload_size, MAX_MESSAGE_PAYLOAD)}),
            ),
        ));
    }

    let base_url = get_base_url().await;
    let message_type = req.message_type.as_deref().unwrap_or("text");
    let want_encrypt = req.encrypt.unwrap_or(false);

    // 验证通道存在且为 active 或 accepted
    let ch_row = db
        .query_one(Statement::from_sql_and_values(
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
                Json(json!({"error": "Channel not found"})),
            )
        })?;

    let status: String = ch_row.try_get("", "status").unwrap_or_default();
    if !["active", "accepted"].contains(&status.as_str()) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(
                json!({"error": format!("Channel is {}, cannot send messages (must be accepted first)", status)}),
            ),
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

    db.execute(Statement::from_sql_and_values(
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
    db.execute(Statement::from_sql_and_values(
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
            .query_one(Statement::from_sql_and_values(
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
                .execute(Statement::from_sql_and_values(
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
    // 本地展示用：若会话已建立，优先广播解密后的明文，避免 Aro 先渲染 ciphertext 信封、
    // 等 poll/getMessages 才正常（WS 回声还可能覆盖已解密内容）。
    // 成功解密后 is_encrypted 也必须改 false，否则客户端会按 flag 再解一次。
    // DB / ActivityPub fan-out 仍用 stored_payload 密文；HTTP 响应的 is_encrypted 反映存储形态。
    let mut ws_payload = stored_payload.clone();
    let mut ws_is_encrypted = is_encrypted;
    if is_encrypted {
        if let Ok(session) = load_e2e_session(channel_id, properties.as_ref()).await {
            if session.established {
                if let Ok(plain) =
                    crate::federation::e2e::decrypt_json_payload(&session, &stored_payload)
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
        .query_one(Statement::from_sql_and_values(
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
                Json(json!({"error": "Channel not found"})),
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
        db.query_all(Statement::from_sql_and_values(
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
        db.query_all(Statement::from_sql_and_values(
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
                if session.established {
                    if let Ok(plain) =
                        crate::federation::e2e::decrypt_json_payload(session, &payload)
                    {
                        payload = plain;
                        display_encrypted = false;
                    }
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

// Inbox 处理（远程 Channel 事件）

/// 处理收到的 ChannelOpen Activity
pub async fn handle_channel_open(
    db: &DatabaseConnection,
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
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id FROM federation_remote_actors WHERE actor_url = $1",
            [actor_url_str.into()],
        ))
        .await
        .map_err(|e| e.to_string())?
        .ok_or("Remote actor not found")?;

    let remote_actor_id: i32 = actor_row.try_get("", "id").unwrap_or(0);

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

    // 防 DB 放大：请求体上限 50MB，to 数组可被塞入海量条目，只看前几个
    const MAX_TO_LOOKUPS: usize = 16;
    let mut target_user_id: Option<i32> = None;
    for entry in to_entries.iter().take(MAX_TO_LOOKUPS) {
        let Some(username) = local_username_from_actor_url(&base_url, entry) else {
            continue;
        };
        let row = db
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT id FROM users WHERE username = $1 LIMIT 1",
                [username.into()],
            ))
            .await
            .map_err(|e| e.to_string())?;
        if let Some(uid) = row.and_then(|r| r.try_get::<i32>("", "id").ok()) {
            target_user_id = Some(uid);
            break;
        }
    }

    if target_user_id.is_none() {
        // 关注关系回退：to 缺失/无法解析（如 BASE_URL 迁移后远端持有旧 URL）
        target_user_id = db
            .query_one(Statement::from_sql_and_values(
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
        // 无法路由属于对这台实例而言的永久性条件：记日志后按 AP 惯例
        // 静默丢弃（上层返回 202），返回 Err 会变成 5xx 引发远端重试风暴。
        tracing::warn!(
            "[Channel] Dropping unroutable ChannelOpen from {} (to={:?})",
            actor_url_str,
            to_entries.iter().take(MAX_TO_LOOKUPS).collect::<Vec<_>>()
        );
        return Ok(());
    };

    // 创建本地 Channel 记录
    let inserted = db
        .execute(Statement::from_sql_and_values(
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
        .map_err(|e| e.to_string())?;

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

    // ChannelMessage may have raced ahead of ChannelOpen — apply buffered ones now.
    flush_early_channel_messages(db, channel_id).await;

    Ok(())
}

/// 处理收到的 ChannelMessage Activity
pub async fn handle_channel_message(
    db: &DatabaseConnection,
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
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT c.status, c.user_id FROM federation_channels c
               JOIN federation_remote_actors ra ON c.remote_actor_id = ra.id
               WHERE c.channel_id = $1 AND ra.actor_url = $2"#,
            [channel_id.into(), actor_url_str.into()],
        ))
        .await
        .map_err(|e| e.to_string())?;

    if ch_check.is_none() {
        // Distinguish "channel not yet created" (race with ChannelOpen) from
        // "wrong remote actor" (permanent). Buffer only the race case.
        let channel_exists = db
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT 1 FROM federation_channels WHERE channel_id = $1",
                [channel_id.into()],
            ))
            .await
            .map_err(|e| e.to_string())?
            .is_some();

        if !channel_exists {
            // Fast path: buffer for flush on ChannelOpen. Always fail (no 202) so
            // the remote retries — covers process restart before open, and buffer
            // full / mutex poison. Duplicates are idempotent via message_id.
            let buffered = buffer_early_channel_message(channel_id, actor_url_str, activity);
            if buffered {
                tracing::info!(
                    "[Channel] Buffered early message for {} from {} (channel not yet present); signaling retry",
                    channel_id,
                    actor_url_str
                );
            } else {
                tracing::warn!(
                    "[Channel] Early-message buffer full for {}; signaling retry",
                    channel_id
                );
            }
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
    let sender = object
        .get("from")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
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
        .execute(Statement::from_sql_and_values(
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
    db.execute(Statement::from_sql_and_values(
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
            .query_one(Statement::from_sql_and_values(
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
                if session.established {
                    if let Ok(plain) =
                        crate::federation::e2e::decrypt_json_payload(&session, &payload)
                    {
                        ws_payload = plain;
                        ws_is_encrypted = false;
                    }
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
                .query_one(Statement::from_sql_and_values(
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
    db: &DatabaseConnection,
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
        .query_one(Statement::from_sql_and_values(
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

    db.execute(Statement::from_sql_and_values(
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

// E2E helpers (from accept_e2e)

// E2E 会话辅助

pub(crate) async fn jwt_secret_for_channel_e2e() -> String {
    let config = crate::GLOBAL_CONFIG.read().await;
    config.jwt_secret.clone()
}

/// 从 channel.properties.e2e 加载会话（private key may be sealed at rest）
pub async fn load_e2e_session(
    channel_id: &str,
    properties: Option<&serde_json::Value>,
) -> Result<crate::federation::e2e::EncryptionSession, String> {
    let e2e = properties
        .and_then(|p| p.get("e2e"))
        .ok_or("No e2e state on channel; call key-exchange first")?;
    let local_pk = e2e
        .get("local_public_key")
        .and_then(|v| v.as_str())
        .ok_or("Missing local_public_key in e2e state")?;
    let local_sk_stored = e2e
        .get("local_private_key")
        .and_then(|v| v.as_str())
        .ok_or("Missing local_private_key in e2e state")?;
    let jwt_secret = jwt_secret_for_channel_e2e().await;
    let local_sk = crate::federation::e2e::unseal_private_key(local_sk_stored, &jwt_secret)?;
    let remote_pk = e2e.get("remote_public_key").and_then(|v| v.as_str());
    crate::federation::e2e::session_from_stored(channel_id, local_pk, &local_sk, remote_pk)
}

/// 处理 myriad:KeyExchange Activity
///
/// 1. 校验公钥材料（`e2e` 模块）
/// 2. 写入 channel.properties.e2e.remote_public_key，标记会话 established
/// 3. 作为特殊 message 存入历史，并 WebSocket 广播
pub async fn handle_key_exchange(
    db: &DatabaseConnection,
    actor_url_str: &str,
    activity: &serde_json::Value,
) -> Result<(), String> {
    let object = activity.get("object").ok_or("Missing object")?;
    let channel_id = object
        .get("channel")
        .and_then(|v| v.as_str())
        .ok_or("Missing channel")?;
    let public_key = object
        .get("publicKey")
        .and_then(|v| v.as_str())
        .ok_or("Missing publicKey")?;
    let algorithm = object
        .get("algorithm")
        .and_then(|v| v.as_str())
        .unwrap_or(crate::federation::e2e::E2E_ALGORITHM);

    crate::federation::e2e::validate_public_key_b64(public_key)
        .map_err(|e| format!("Invalid remote E2E public key: {e}"))?;

    // 验证发送方是该 Channel 的远程方，并读取 properties
    let ch_row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT c.properties, c.status FROM federation_channels c
               JOIN federation_remote_actors ra ON c.remote_actor_id = ra.id
               WHERE c.channel_id = $1 AND ra.actor_url = $2"#,
            [channel_id.into(), actor_url_str.into()],
        ))
        .await
        .map_err(|e| e.to_string())?;

    let ch_row = match ch_row {
        Some(r) => r,
        None => {
            // Align with ChannelMessage: Open may still be in flight.
            let channel_exists = db
                .query_one(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "SELECT 1 FROM federation_channels WHERE channel_id = $1",
                    [channel_id.into()],
                ))
                .await
                .map_err(|e| e.to_string())?
                .is_some();
            if !channel_exists {
                let buffered = buffer_early_channel_activity(channel_id, actor_url_str, activity);
                if buffered {
                    tracing::info!(
                        "[Channel] Buffered early KeyExchange for {} from {} (channel not yet present)",
                        channel_id,
                        actor_url_str
                    );
                }
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
    };

    let ch_status: String = ch_row
        .try_get("", "status")
        .unwrap_or_else(|_| "pending".into());
    if ch_status == "closed" {
        return Err(format!("Channel {channel_id} is closed"));
    }

    let mut properties = ch_row
        .try_get::<Option<serde_json::Value>>("", "properties")
        .ok()
        .flatten()
        .unwrap_or_else(|| json!({}));

    // 合并 e2e 状态：写入 remote_public_key；若已有本地密钥则 established=true
    let mut e2e_obj = properties.get("e2e").cloned().unwrap_or_else(|| json!({}));
    e2e_obj["remote_public_key"] = json!(public_key);
    e2e_obj["algorithm"] = json!(algorithm);
    let has_local = e2e_obj
        .get("local_private_key")
        .and_then(|v| v.as_str())
        .map(|s| !s.is_empty())
        .unwrap_or(false);
    e2e_obj["established"] = json!(has_local);
    properties["e2e"] = e2e_obj;

    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE federation_channels SET properties = $2, last_activity_at = NOW() WHERE channel_id = $1",
        [channel_id.into(), properties.into()],
    ))
    .await
    .map_err(|e| e.to_string())?;

    let message_id = activity
        .get("id")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(generate_message_id);

    let payload = json!({
        "publicKey": public_key,
        "algorithm": algorithm,
    });

    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"INSERT INTO federation_channel_messages
           (channel_id, message_id, sender_actor, message_type, payload, is_encrypted, created_at)
           VALUES ($1, $2, $3, 'myriad:KeyExchange', $4, false, NOW())
           ON CONFLICT (message_id) DO NOTHING"#,
        [
            channel_id.into(),
            message_id.clone().into(),
            actor_url_str.into(),
            payload.clone().into(),
        ],
    ))
    .await
    .map_err(|e| e.to_string())?;

    crate::federation::ws_gateway::broadcast_to_channel(
        channel_id,
        &json!({
            "type": "key_exchange",
            "channel_id": channel_id,
            "from": actor_url_str,
            "publicKey": public_key,
            "algorithm": algorithm,
            "established": has_local
        }),
    )
    .await;

    tracing::info!(
        "[Channel] KeyExchange received in channel {} from {} (local_keys={})",
        channel_id,
        actor_url_str,
        has_local
    );
    Ok(())
}

// accept / initiate e2e (merged)

/// 接受 Channel（本地用户确认）
pub async fn accept_channel(
    user_id: i32,
    username: &str,
    channel_id: &str,
    db: &DatabaseConnection,
) -> Result<serde_json::Value, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;

    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT c.status, c.initiated_by, ra.actor_url, ra.inbox_url
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
                Json(json!({"error": "Channel not found"})),
            )
        })?;

    let status: String = row.try_get("", "status").unwrap_or_default();
    if status != "pending" {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": format!("Channel is {}, cannot accept", status)})),
        ));
    }

    let remote_actor_url: String = row.try_get("", "actor_url").unwrap_or_default();
    let remote_inbox: Option<String> = row
        .try_get::<Option<String>>("", "inbox_url")
        .unwrap_or(None);

    // 更新状态
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE federation_channels SET status = 'accepted' WHERE channel_id = $1",
        [channel_id.into()],
    ))
    .await
    .map_err(db_err)?;

    // 发送 Accept Activity
    let local_actor = actor_url(&base_url, username);
    let activity_id = generate_activity_id(&base_url);
    let accept = json!({
        "@context": build_context(),
        "type": "Accept",
        "id": &activity_id,
        "actor": &local_actor,
        "to": [&remote_actor_url],
        "object": {
            "type": "myriad:ChannelOpen",
            "id": channel_id
        }
    });

    if let Some(inbox) = remote_inbox {
        let domain = extract_domain(&inbox).unwrap_or_default();
        let act_row = db
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"INSERT INTO federation_activities
                   (activity_id, user_id, activity_type, object_type, object_json, is_local, published_at)
                   VALUES ($1, $2, 'Accept', 'ChannelOpen', $3, true, NOW())
                   RETURNING id"#,
                [
                    activity_id.clone().into(),
                    user_id.into(),
                    accept.clone().into(),
                ],
            ))
            .await
            .map_err(db_err)?;

        if let Some(act_id) = act_row.and_then(|r| r.try_get::<i32>("", "id").ok()) {
            let _ = db
                .execute(Statement::from_sql_and_values(
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

    let notif_id = crate::federation::notify::channel_invite_notification_id(channel_id, user_id);
    crate::federation::notify::mark_invite_notification_read(user_id, &notif_id).await;

    Ok(json!({
        "success": true,
        "channel_id": channel_id,
        "status": "accepted"
    }))
}

/// 处理收到的 ChannelAccept Activity（myriad:ChannelAccept）
///
/// 远程方接受了我方发起的 Channel：把 status 置为 'accepted'。
/// 与外部 `Accept`（object=myriad:ChannelOpen）等价的快捷形式，
/// 来自仅实现 MFP 扩展的对端实例。
pub async fn handle_channel_accept(
    db: &DatabaseConnection,
    actor_url_str: &str,
    activity: &serde_json::Value,
) -> Result<(), String> {
    let object = activity.get("object").ok_or("Missing object")?;
    let channel_id = object
        .get("id")
        .and_then(|v| v.as_str())
        .or_else(|| object.as_str())
        .ok_or("Missing channel id")?;

    // 验证发送方确为该 Channel 的远程方
    let ch_check = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT c.status FROM federation_channels c
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

    let result = db
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"UPDATE federation_channels
               SET status = 'accepted', last_activity_at = NOW()
               WHERE channel_id = $1 AND status = 'pending'"#,
            [channel_id.into()],
        ))
        .await
        .map_err(|e| e.to_string())?;

    if result.rows_affected() > 0 {
        crate::federation::ws_gateway::broadcast_to_channel(
            channel_id,
            &json!({
                "type": "channel_accepted",
                "channel_id": channel_id
            }),
        )
        .await;
        tracing::info!(
            "[Channel] {} accepted by remote {}",
            channel_id,
            actor_url_str
        );
    }

    Ok(())
}







/// 发起 Channel E2E 密钥交换：生成 X25519 密钥对、写入 properties、投递 myriad:KeyExchange
pub async fn initiate_e2e_key_exchange(
    user_id: i32,
    username: &str,
    channel_id: &str,
    db: &DatabaseConnection,
) -> Result<E2eKeyExchangeResponse, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;

    let ch_row = db
        .query_one(Statement::from_sql_and_values(
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
                Json(json!({"error": "Channel not found"})),
            )
        })?;

    let status: String = ch_row.try_get("", "status").unwrap_or_default();
    // Alignment: do not fan-out KeyExchange while local channel is still pending
    // (remote may not have applied ChannelOpen yet → not_found storm).
    if !["active", "accepted"].contains(&status.as_str()) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": format!(
                    "Channel is {status}; wait until active/accepted before E2E key exchange"
                )
            })),
        ));
    }

    let remote_inbox: Option<String> = ch_row
        .try_get::<Option<String>>("", "inbox_url")
        .unwrap_or(None);
    let remote_actor_url: String = ch_row.try_get("", "actor_url").unwrap_or_default();
    let mut properties = ch_row
        .try_get::<Option<serde_json::Value>>("", "properties")
        .ok()
        .flatten()
        .unwrap_or_else(|| json!({}));

    // 保留已有 remote key（若对端先发起）
    let existing_remote = properties
        .get("e2e")
        .and_then(|e| e.get("remote_public_key"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // 已有本地密钥则复用：打开会话时自动 key-exchange 若每次轮换密钥，
    // 对端仍握旧公钥 → 解密失败，且历史里堆满 outbound KeyExchange。
    let existing_local_pk = properties
        .get("e2e")
        .and_then(|e| e.get("local_public_key"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let existing_local_sk = properties
        .get("e2e")
        .and_then(|e| e.get("local_private_key"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let jwt_secret = jwt_secret_for_channel_e2e().await;
    // new_keypair: only write a history KeyExchange bubble when the local key is mint-new
    let (public_key, sealed_sk, established, new_keypair) = if let (Some(pk), Some(sk_stored)) =
        (existing_local_pk, existing_local_sk)
    {
        // Validate we can still unseal; if seal secret rotated, mint a new pair.
        match crate::federation::e2e::unseal_private_key(&sk_stored, &jwt_secret) {
            Ok(_sk) => {
                let established = existing_remote
                    .as_ref()
                    .map(|r| crate::federation::e2e::validate_public_key_b64(r).is_ok())
                    .unwrap_or(false);
                (pk, sk_stored, established, false)
            }
            Err(_) => {
                let mut session = crate::federation::e2e::create_session(channel_id);
                let mut established = false;
                if let Some(ref remote_pk) = existing_remote {
                    if crate::federation::e2e::accept_key_exchange(&mut session, remote_pk).is_ok()
                    {
                        established = true;
                    }
                }
                let sealed = crate::federation::e2e::seal_private_key(
                    &session.local_keypair.private_key,
                    &jwt_secret,
                )
                .map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({"error": format!("Failed to seal E2E key: {}", e)})),
                    )
                })?;
                (session.local_keypair.public_key, sealed, established, true)
            }
        }
    } else {
        let mut session = crate::federation::e2e::create_session(channel_id);
        let mut established = false;
        if let Some(ref remote_pk) = existing_remote {
            if crate::federation::e2e::accept_key_exchange(&mut session, remote_pk).is_ok() {
                established = true;
            }
        }
        let sealed = crate::federation::e2e::seal_private_key(
            &session.local_keypair.private_key,
            &jwt_secret,
        )
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("Failed to seal E2E key: {}", e)})),
            )
        })?;
        (session.local_keypair.public_key, sealed, established, true)
    };

    let e2e_state = json!({
        "local_public_key": public_key,
        "local_private_key": sealed_sk,
        "remote_public_key": existing_remote,
        "established": established,
        "sealed": true,
        "algorithm": crate::federation::e2e::E2E_ALGORITHM,
    });
    properties["e2e"] = e2e_state;

    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE federation_channels SET properties = $2, last_activity_at = NOW() WHERE channel_id = $1 AND user_id = $3",
        [channel_id.into(), properties.into(), user_id.into()],
    ))
    .await
    .map_err(db_err)?;

    let local_actor = actor_url(&base_url, username);
    let activity_id = generate_activity_id(&base_url);
    let kx_object = crate::federation::e2e::KeyExchangePayload::for_channel(
        channel_id,
        &public_key,
        Some(now_iso8601()),
    )
    .to_json();
    let kx_activity = json!({
        "@context": build_context(),
        "type": "myriad:KeyExchange",
        "id": &activity_id,
        "actor": &local_actor,
        "to": [&remote_actor_url],
        "object": kx_object
    });

    // 本地历史：仅在新生成密钥时记一条（复用密钥时重复写入会刷屏且误导）
    if new_keypair {
        let message_id = generate_message_id();
        let _ = db
            .execute(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"INSERT INTO federation_channel_messages
               (channel_id, message_id, sender_actor, message_type, payload, is_encrypted, created_at)
               VALUES ($1, $2, $3, 'myriad:KeyExchange', $4, false, NOW())"#,
                [
                    channel_id.into(),
                    message_id.into(),
                    local_actor.clone().into(),
                    json!({
                        "publicKey": &public_key,
                        "algorithm": crate::federation::e2e::E2E_ALGORITHM,
                        "direction": "outbound"
                    })
                    .into(),
                ],
            ))
            .await;
    }

    if let Some(inbox) = remote_inbox {
        let domain = extract_domain(&inbox).unwrap_or_default();
        let act_row = db
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"INSERT INTO federation_activities
                   (activity_id, user_id, activity_type, object_type, object_json, is_local, published_at)
                   VALUES ($1, $2, 'KeyExchange', 'KeyExchange', $3, true, NOW())
                   RETURNING id"#,
                [
                    activity_id.clone().into(),
                    user_id.into(),
                    kx_activity.clone().into(),
                ],
            ))
            .await
            .map_err(db_err)?;

        if let Some(act_id) = act_row.and_then(|r| r.try_get::<i32>("", "id").ok()) {
            let _ = db
                .execute(Statement::from_sql_and_values(
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

    crate::federation::ws_gateway::broadcast_to_channel(
        channel_id,
        &json!({
            "type": "key_exchange",
            "channel_id": channel_id,
            "from": local_actor,
            "publicKey": &public_key,
            "algorithm": crate::federation::e2e::E2E_ALGORITHM,
            "established": established,
            "direction": "outbound"
        }),
    )
    .await;

    tracing::info!(
        "[Channel] E2E key exchange initiated for {} (established={}, new_keypair={})",
        channel_id,
        established,
        new_keypair
    );

    Ok(E2eKeyExchangeResponse {
        success: true,
        channel_id: channel_id.to_string(),
        public_key,
        algorithm: crate::federation::e2e::E2E_ALGORITHM.to_string(),
        established,
    })
}


#[cfg(test)]
mod channel_early_buffer_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn early_buffer_accepts_and_dedupes_by_activity_id() {
        let id = "ch_test_buffer_unit";
        // clear any prior via purge by using unique id
        let act = json!({"id": "https://example.com/act/1", "type": "Create"});
        assert!(buffer_early_channel_activity(id, "https://a.example/actor", &act));
        // second with same id is treated as success (dedupe)
        assert!(buffer_early_channel_activity(id, "https://a.example/actor", &act));
    }

    #[test]
    fn early_buffer_rejects_when_per_channel_cap_reached() {
        let id = "ch_test_buffer_cap";
        for i in 0..EARLY_MSG_MAX_PER_CHANNEL {
            let act = json!({"id": format!("https://example.com/act/{i}"), "type": "Create"});
            assert!(buffer_early_channel_activity(id, "https://a.example/actor", &act), "i={i}");
        }
        let overflow = json!({"id": "https://example.com/act/overflow", "type": "Create"});
        assert!(!buffer_early_channel_activity(id, "https://a.example/actor", &overflow));
    }
}
