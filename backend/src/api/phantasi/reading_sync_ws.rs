//! Phantasi reading WebSocket.
use axum::{Extension, extract::State, http::StatusCode, response::IntoResponse};
use sea_orm::DatabaseConnection;

use crate::error::HttpError;
use crate::middleware::auth::Claims;
use crate::services::phantasi_scheduler::get_phantasi_scheduler;

use super::helpers::{phantasi_http_err, require_phantasi_module_access};

// WebSocket

pub(crate) async fn phantasi_websocket(
    ws: axum::extract::ws::WebSocketUpgrade,
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    headers: axum::http::HeaderMap,
) -> Result<impl IntoResponse, HttpError> {
    let allowed = crate::middleware::ws_origin::allowed_origins_from_global_config().await;
    crate::middleware::ws_origin::assert_ws_origin_for_cookie_session(&headers, &allowed)?;

    let user_id = claims
        .subject()
        .map(|subject| subject.id())
        .ok_or_else(|| phantasi_http_err(StatusCode::UNAUTHORIZED, "Unauthorized"))?;
    // Only "not a current admin" downgrades; a database failure fails the upgrade.
    let is_admin = crate::middleware::auth::current_admin_status(&claims, &db)
        .await
        .map_err(HttpError::from)?;
    require_phantasi_module_access(&db, Some(user_id), is_admin).await?;
    Ok(ws.on_upgrade(move |socket| handle_phantasi_websocket(socket, db, user_id)))
}

pub(crate) async fn handle_phantasi_websocket(
    mut socket: axum::extract::ws::WebSocket,
    _db: DatabaseConnection,
    user_id: i32,
) {
    use axum::extract::ws::Message;

    // 订阅通知
    if let Some(scheduler) = get_phantasi_scheduler() {
        let mut rx = scheduler.subscribe_notifications();

        loop {
            tokio::select! {
                // 接收来自调度器的通知
                Ok(notification) = rx.recv() => {
                    if notification.user_id != user_id {
                        continue;
                    }
                    let msg = serde_json::to_string(&notification).unwrap_or_default();
                    if socket.send(Message::Text(msg.into())).await.is_err() {
                        break;
                    }
                }
                // 接收来自客户端的消息（心跳等）
                Some(msg) = socket.recv() => {
                    match msg {
                        Ok(Message::Ping(data)) => {
                            if socket.send(Message::Pong(data)).await.is_err() {
                                break;
                            }
                        }
                        Ok(Message::Close(_)) => break,
                        Err(_) => break,
                        _ => {}
                    }
                }
                else => break,
            }
        }
    }
}
