//! WebSocket 网关（Phase 3 — Layer 3）
//!
//! 为 Channel 提供实时双向通信能力。
//! 每个 Channel 可以有多个 WebSocket 连接（同一用户多设备）。
//!
//! # Tapp attribution
//!
//! Browser WebSockets cannot send `X-Tapp-Runtime-Grant`. Tapp runtimes mint a
//! one-time ticket via REST (`POST .../ws-ticket` with the grant header) and
//! present it as `?tapp_ws_ticket=` on upgrade. A present-but-invalid ticket
//! is rejected (fail closed); host UI omits the query param and uses Claims only.

use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    extract::{Extension, Query, State},
    http::HeaderMap,
    response::IntoResponse,
    response::Response,
};
use sea_orm::DatabaseConnection;
use futures::{SinkExt, StreamExt};
use once_cell::sync::Lazy;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

use crate::middleware::auth::Claims;
use crate::middleware::ws_origin::{
    allowed_origins_from_global_config, assert_ws_origin_for_cookie_session,
};
use crate::services::tapp_ws_ticket::{
    self, ConsumedWsTicket, WsTicketError, WsTicketKind, TAPP_WS_TICKET_QUERY,
};

use crate::federation::types::get_base_url;

/// Optional one-time Tapp WS ticket query param name:
/// [`TAPP_WS_TICKET_QUERY`] (`tapp_ws_ticket`).
#[derive(Debug, Default, Deserialize)]
pub struct FederationWsQuery {
    #[serde(default)]
    pub tapp_ws_ticket: Option<String>,
}

const _: &str = TAPP_WS_TICKET_QUERY;

fn ws_ticket_http_error(
    err: WsTicketError,
) -> (axum::http::StatusCode, axum::Json<serde_json::Value>) {
    let status = axum::http::StatusCode::from_u16(err.status_hint())
        .unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR);
    (
        status,
        axum::Json(json!({
            "error": err.message(),
            "code": err.code(),
        })),
    )
}

// 连接管理

/// 每个 Channel 的广播通道
struct ChannelBroadcast {
    tx: broadcast::Sender<String>,
}

/// 全局 Channel 广播注册表
static CHANNEL_REGISTRY: Lazy<Arc<RwLock<HashMap<String, ChannelBroadcast>>>> =
    Lazy::new(|| Arc::new(RwLock::new(HashMap::new())));

/// 全局 Room 广播注册表（复用同结构）
static ROOM_REGISTRY: Lazy<Arc<RwLock<HashMap<String, ChannelBroadcast>>>> =
    Lazy::new(|| Arc::new(RwLock::new(HashMap::new())));

/// 获取或创建 Channel 的广播通道
async fn get_or_create_channel_tx(channel_id: &str) -> broadcast::Sender<String> {
    {
        let registry = CHANNEL_REGISTRY.read().await;
        if let Some(cb) = registry.get(channel_id) {
            return cb.tx.clone();
        }
    }

    let mut registry = CHANNEL_REGISTRY.write().await;
    // 双重检查
    if let Some(cb) = registry.get(channel_id) {
        return cb.tx.clone();
    }

    let (tx, _) = broadcast::channel(256);
    registry.insert(channel_id.to_string(), ChannelBroadcast { tx: tx.clone() });
    tx
}

/// 广播消息到指定 Channel 的所有 WebSocket 连接
pub async fn broadcast_to_channel(channel_id: &str, message: &serde_json::Value) {
    let registry = CHANNEL_REGISTRY.read().await;
    if let Some(cb) = registry.get(channel_id) {
        let msg = serde_json::to_string(message).unwrap_or_default();
        let _ = cb.tx.send(msg);
    }
}

/// 清理空的 Channel 广播通道
async fn cleanup_channel(channel_id: &str) {
    let mut registry = CHANNEL_REGISTRY.write().await;
    if let Some(cb) = registry.get(channel_id) {
        if cb.tx.receiver_count() == 0 {
            registry.remove(channel_id);
        }
    }
}

// Room 广播

/// 获取或创建 Room 的广播通道
async fn get_or_create_room_tx(room_id: &str) -> broadcast::Sender<String> {
    {
        let registry = ROOM_REGISTRY.read().await;
        if let Some(cb) = registry.get(room_id) {
            return cb.tx.clone();
        }
    }
    let mut registry = ROOM_REGISTRY.write().await;
    if let Some(cb) = registry.get(room_id) {
        return cb.tx.clone();
    }
    let (tx, _) = broadcast::channel(512);
    registry.insert(room_id.to_string(), ChannelBroadcast { tx: tx.clone() });
    tx
}

/// 广播消息到指定 Room 的所有 WebSocket 连接
pub async fn broadcast_to_room(room_id: &str, message: &serde_json::Value) {
    let registry = ROOM_REGISTRY.read().await;
    if let Some(cb) = registry.get(room_id) {
        let msg = serde_json::to_string(message).unwrap_or_default();
        let _ = cb.tx.send(msg);
    }
}

/// 清理空的 Room 广播通道
async fn cleanup_room(room_id: &str) {
    let mut registry = ROOM_REGISTRY.write().await;
    if let Some(cb) = registry.get(room_id) {
        if cb.tx.receiver_count() == 0 {
            registry.remove(room_id);
        }
    }
}

// WebSocket 处理器

/// WebSocket 升级端点
///
/// GET /api/federation/channels/{channel_id}/ws
/// GET /api/federation/channels/{channel_id}/ws?tapp_ws_ticket=...
///
/// 需要认证（通过 auth_middleware 注入 Claims）。
/// 连接后自动加入该 Channel 的广播组。
///
/// - Ticket present: validate+consume; attribute as Tapp runtime; reject bad tickets.
/// - Ticket absent: host UI path (Claims only).
pub async fn channel_websocket(
    State(db): State<DatabaseConnection>,
    ws: WebSocketUpgrade,
    Extension(claims): Extension<Claims>,
    headers: HeaderMap,
    axum::extract::Path(channel_id): axum::extract::Path<String>,
    Query(query): Query<FederationWsQuery>,
) -> Response {
    let allowed = allowed_origins_from_global_config().await;
    if let Err(err) = assert_ws_origin_for_cookie_session(&headers, &allowed) {
        return err.into_response();
    }

    let tapp_attr = match resolve_ws_ticket(
        &db,
        query.tapp_ws_ticket.as_deref(),
        &claims,
        WsTicketKind::Channel,
        &channel_id,
    )
    .await
    {
        Ok(attr) => attr,
        Err(err) => return err.into_response(),
    };

    let user_id: i32 = claims.sub.parse().unwrap_or(-1);
    let username = claims.username.clone();

    ws.on_upgrade(move |socket| {
        handle_channel_socket(socket, db, user_id, username, channel_id, tapp_attr)
    })
    .into_response()
}

async fn resolve_ws_ticket(
    db: &DatabaseConnection,
    ticket: Option<&str>,
    claims: &Claims,
    kind: WsTicketKind,
    resource_id: &str,
) -> Result<Option<ConsumedWsTicket>, (axum::http::StatusCode, axum::Json<serde_json::Value>)> {
    let Some(ticket) = ticket.map(str::trim).filter(|t| !t.is_empty()) else {
        return Ok(None);
    };
    // Fail closed: never fall open to host identity when a ticket was supplied.
    let subject_id: i32 = claims
        .sub
        .parse()
        .map_err(|_| ws_ticket_http_error(WsTicketError::InvalidSubject))?;
    let consumed =
        tapp_ws_ticket::consume_ws_ticket(db, ticket, subject_id, kind, resource_id)
            .await
            .map_err(ws_ticket_http_error)?;
    Ok(Some(consumed))
}

/// 处理单个 WebSocket 连接
async fn handle_channel_socket(
    socket: WebSocket,
    db: DatabaseConnection,
    user_id: i32,
    username: String,
    channel_id: String,
    tapp_attr: Option<ConsumedWsTicket>,
) {
    if let Some(ref attr) = tapp_attr {
        tracing::info!(
            tapp_id = %attr.tapp_id,
            runtime_id = %attr.runtime_id,
            subject_id = attr.subject_id,
            owner_id = attr.owner_id,
            kind = ?attr.kind,
            resource_id = %attr.resource_id,
            "[WS] Channel {} connected: user={} ({}) [Tapp-attributed]",
            channel_id,
            username,
            user_id
        );
    } else {
        tracing::info!(
            "[WS] Channel {} connected: user={} ({})",
            channel_id,
            username,
            user_id
        );
    }

    // 验证用户拥有该 Channel
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    let owns_channel = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT 1 FROM federation_channels WHERE user_id = $1 AND channel_id = $2 AND status != 'closed'",
            [user_id.into(), channel_id.clone().into()],
        ))
        .await
        .ok()
        .flatten()
        .is_some();

    if !owns_channel {
        tracing::warn!("[WS] User {} does not own channel {}", user_id, channel_id);
        return;
    }

    let (mut ws_sender, mut ws_receiver) = socket.split();

    // 加入 Channel 广播
    let tx = get_or_create_channel_tx(&channel_id).await;
    let mut rx = tx.subscribe();

    let base_url = get_base_url().await;
    let local_actor = crate::federation::types::actor_url(&base_url, &username);

    // 发送欢迎消息
    let welcome = json!({
        "type": "connected",
        "channel_id": &channel_id,
        "user_id": user_id,
        "actor": &local_actor
    });
    if ws_sender
        .send(Message::Text(
            serde_json::to_string(&welcome).unwrap_or_default().into(),
        ))
        .await
        .is_err()
    {
        return;
    }

    let channel_id_clone = channel_id.clone();

    // 并发处理：广播接收 + 客户端消息
    loop {
        tokio::select! {
            // 从广播接收消息 → 发送给 WebSocket 客户端
            broadcast_result = rx.recv() => {
                match broadcast_result {
                    Ok(msg) => {
                        if ws_sender.send(Message::Text(msg.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("[WS] Channel {} lagged {} messages", channel_id, n);
                        // 通知客户端消息丢失
                        let lag_msg = json!({
                            "type": "lagged",
                            "channel_id": &channel_id,
                            "missed": n
                        });
                        if ws_sender
                            .send(Message::Text(serde_json::to_string(&lag_msg).unwrap_or_default().into()))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            // 从 WebSocket 客户端接收消息 → 处理
            ws_msg = ws_receiver.next() => {
                match ws_msg {
                    Some(Ok(Message::Text(text))) => {
                        // 解析客户端发来的消息
                        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&text) {
                            if let Some(direct_reply) = handle_ws_client_message(
                                &db,
                                user_id,
                                &username,
                                &channel_id,
                                &parsed,
                                &tx,
                            )
                            .await {
                                // 直接回复给发送者（错误/pong 等），不广播
                                if ws_sender.send(Message::Text(direct_reply.into())).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                    Some(Ok(Message::Ping(data))) => {
                        if ws_sender.send(Message::Pong(data)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {} // 忽略其他消息类型
                }
            }
        }
    }

    tracing::info!(
        "[WS] Channel {} disconnected: user={}",
        channel_id_clone,
        user_id
    );
    cleanup_channel(&channel_id_clone).await;
}

// Room WebSocket 处理器

/// Room WebSocket 升级端点
///
/// GET /api/federation/rooms/{room_id}/ws
/// GET /api/federation/rooms/{room_id}/ws?tapp_ws_ticket=...
pub async fn room_websocket(
    State(db): State<DatabaseConnection>,
    ws: WebSocketUpgrade,
    Extension(claims): Extension<Claims>,
    headers: HeaderMap,
    axum::extract::Path(room_id): axum::extract::Path<String>,
    Query(query): Query<FederationWsQuery>,
) -> Response {
    let allowed = allowed_origins_from_global_config().await;
    if let Err(err) = assert_ws_origin_for_cookie_session(&headers, &allowed) {
        return err.into_response();
    }

    let tapp_attr = match resolve_ws_ticket(
        &db,
        query.tapp_ws_ticket.as_deref(),
        &claims,
        WsTicketKind::Room,
        &room_id,
    )
    .await
    {
        Ok(attr) => attr,
        Err(err) => return err.into_response(),
    };

    let user_id: i32 = claims.sub.parse().unwrap_or(-1);
    let username = claims.username.clone();
    ws.on_upgrade(move |socket| {
        handle_room_socket(socket, db, user_id, username, room_id, tapp_attr)
    })
    .into_response()
}

/// 处理单个 Room WebSocket 连接
async fn handle_room_socket(
    socket: WebSocket,
    db: DatabaseConnection,
    user_id: i32,
    username: String,
    room_id: String,
    tapp_attr: Option<ConsumedWsTicket>,
) {
    if let Some(ref attr) = tapp_attr {
        tracing::info!(
            tapp_id = %attr.tapp_id,
            runtime_id = %attr.runtime_id,
            subject_id = attr.subject_id,
            owner_id = attr.owner_id,
            kind = ?attr.kind,
            resource_id = %attr.resource_id,
            "[WS] Room {} connected: user={} ({}) [Tapp-attributed]",
            room_id,
            username,
            user_id
        );
    } else {
        tracing::info!(
            "[WS] Room {} connected: user={} ({})",
            room_id,
            username,
            user_id
        );
    }

    let base_url = get_base_url().await;
    let local_actor = crate::federation::types::actor_url(&base_url, &username);

    // 验证用户是该 Room 的**活跃**成员。
    //
    // 这里过去只看行是否存在，于是一个还没接受邀请（membership_status =
    // 'pending'）的用户也能连上房间 socket，拿到此后所有消息、typing 与系统事件
    // —— 而 REST 侧的 get_room_messages 走 require_active_member_role，明确不给
    // pending 看历史。两条路必须同一个门槛。
    use sea_orm::{ConnectionTrait as _, DatabaseBackend, Statement};
    let is_member = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT 1 FROM federation_room_members
               WHERE room_id = $1 AND actor_url = $2
                 AND COALESCE(membership_status, 'active') = 'active'"#,
            [room_id.clone().into(), local_actor.clone().into()],
        ))
        .await
        .ok()
        .flatten()
        .is_some();

    if !is_member {
        tracing::warn!(
            "[WS] User {} is not an active member of room {}",
            user_id,
            room_id
        );
        return;
    }

    let (mut ws_sender, mut ws_receiver) = socket.split();

    let tx = get_or_create_room_tx(&room_id).await;
    let mut rx = tx.subscribe();

    // 发送欢迎消息
    let welcome = json!({
        "type": "connected",
        "room_id": &room_id,
        "user_id": user_id,
        "actor": &local_actor
    });
    if ws_sender
        .send(Message::Text(
            serde_json::to_string(&welcome).unwrap_or_default().into(),
        ))
        .await
        .is_err()
    {
        return;
    }

    let room_id_clone = room_id.clone();

    loop {
        tokio::select! {
            broadcast_result = rx.recv() => {
                match broadcast_result {
                    Ok(msg) => {
                        if ws_sender.send(Message::Text(msg.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("[WS] Room {} lagged {} messages", room_id, n);
                        let lag_msg = json!({ "type": "lagged", "room_id": &room_id, "missed": n });
                        if ws_sender
                            .send(Message::Text(serde_json::to_string(&lag_msg).unwrap_or_default().into()))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            ws_msg = ws_receiver.next() => {
                match ws_msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&text) {
                            if let Some(direct_reply) = handle_ws_room_client_message(
                                &db, user_id, &username, &room_id, &parsed, &tx,
                            ).await {
                                // 直接回复给发送者（错误/pong 等），不广播
                                if ws_sender.send(Message::Text(direct_reply.into())).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                    Some(Ok(Message::Ping(data))) => {
                        if ws_sender.send(Message::Pong(data)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }

    tracing::info!("[WS] Room {} disconnected: user={}", room_id_clone, user_id);
    cleanup_room(&room_id_clone).await;
}

/// 处理 Room 客户端 WebSocket 消息
///
/// 返回 Some(msg) 表示需要直接回复给发送者（不广播）
async fn handle_ws_room_client_message(
    db: &DatabaseConnection,
    user_id: i32,
    username: &str,
    room_id: &str,
    msg: &serde_json::Value,
    tx: &broadcast::Sender<String>,
) -> Option<String> {
    let msg_type = msg.get("type").and_then(|v| v.as_str()).unwrap_or("text");

    match msg_type {
        "message" => {
            let payload = msg.get("payload").cloned().unwrap_or(json!(null));
            let thread_id = msg.get("thread_id").and_then(|v| v.as_str());
            let reply_to = msg.get("reply_to").and_then(|v| v.as_str());

            let req = crate::federation::room::SendRoomMessageRequest {
                message_type: msg
                    .get("message_type")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                payload,
                thread_id: thread_id.map(|s| s.to_string()),
                reply_to: reply_to.map(|s| s.to_string()),
                encrypt: msg.get("encrypt").and_then(|v| v.as_bool()),
            };
            if let Err((status, json_err)) =
                crate::federation::room::send_room_message(user_id, username, room_id, db, &req)
                    .await
            {
                // 错误仅回复给发送者，不广播给其他人
                let err_msg = json!({
                    "type": "error",
                    "status": status.as_u16(),
                    "error": json_err.0
                });
                return Some(serde_json::to_string(&err_msg).unwrap_or_default());
            }
            None
        }
        "typing" => {
            let base_url = get_base_url().await;
            let actor = crate::federation::types::actor_url(&base_url, username);
            let typing_msg = json!({
                "type": "typing",
                "room_id": room_id,
                "actor": actor,
                "is_typing": msg.get("is_typing").and_then(|v| v.as_bool()).unwrap_or(true)
            });
            let _ = tx.send(serde_json::to_string(&typing_msg).unwrap_or_default());
            None
        }
        "ping" => {
            // 应用层 ping — 仅回复给发送者
            Some(
                serde_json::to_string(&json!({"type": "pong", "room_id": room_id}))
                    .unwrap_or_default(),
            )
        }
        _ => {
            tracing::debug!("[WS] Unknown room message type: {}", msg_type);
            None
        }
    }
}

/// 处理客户端通过 WebSocket 发送的消息
///
/// 返回 Some(msg) 表示需要直接回复给发送者（不广播）
async fn handle_ws_client_message(
    db: &DatabaseConnection,
    user_id: i32,
    username: &str,
    channel_id: &str,
    msg: &serde_json::Value,
    tx: &broadcast::Sender<String>,
) -> Option<String> {
    let msg_type = msg.get("type").and_then(|v| v.as_str()).unwrap_or("text");

    match msg_type {
        "message" => {
            // 用户通过 WS 直接发消息
            let payload = msg.get("payload").cloned().unwrap_or(json!(null));
            let reply_to = msg.get("reply_to").and_then(|v| v.as_str());

            let req = crate::federation::channel::SendMessageRequest {
                message_type: msg
                    .get("message_type")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                payload,
                reply_to: reply_to.map(|s| s.to_string()),
                encrypt: msg.get("encrypt").and_then(|v| v.as_bool()),
            };
            match crate::federation::channel::send_message(
                user_id, username, channel_id, db, &req,
            )
            .await
            {
                Ok(_resp) => {
                    // send_message 内部已经调用了 broadcast_to_channel
                }
                Err((status, json_err)) => {
                    // 错误仅回复给发送者，不广播给其他人
                    let err_msg = json!({
                        "type": "error",
                        "status": status.as_u16(),
                        "error": json_err.0
                    });
                    return Some(serde_json::to_string(&err_msg).unwrap_or_default());
                }
            }
            None
        }
        "typing" => {
            // 打字指示器 — 广播给该 Channel 所有连接
            let base_url = get_base_url().await;
            let local_actor = crate::federation::types::actor_url(&base_url, username);
            let typing_msg = json!({
                "type": "typing",
                "channel_id": channel_id,
                "actor": local_actor,
                "is_typing": msg.get("is_typing").and_then(|v| v.as_bool()).unwrap_or(true)
            });
            let _ = tx.send(serde_json::to_string(&typing_msg).unwrap_or_default());
            None
        }
        "ping" => {
            // 应用层 ping — 仅回复给发送者
            Some(
                serde_json::to_string(&json!({"type": "pong", "channel_id": channel_id}))
                    .unwrap_or_default(),
            )
        }
        _ => {
            tracing::debug!("[WS] Unknown message type: {}", msg_type);
            None
        }
    }
}
