//! Agent API — notifications
use super::*;
use crate::error::HttpError;

// 通知 API

/// 通知 SSE 流
pub(crate) async fn notification_stream(
    Extension(claims): Extension<Claims>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, HttpError> {
    let user_id = parse_user_id(&claims)?;
    let manager = crate::services::agent::notifications::get_notification_manager().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({"error": "Notification system not initialized"})),
    ))?;

    let mut rx = manager.subscribe();
    let (tx, mpsc_rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(32);

    // 发送初始未读计数
    let unread = manager.unread_count_for_user(user_id).await;
    let init_data = json!({"event": "init", "unread_count": unread});
    let _ = tx
        .send(Ok(
            Event::default().data(serde_json::to_string(&init_data).unwrap_or_default())
        ))
        .await;

    // 后台转发 broadcast → mpsc（过滤非当前用户的通知）
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let should_send =
                        crate::services::agent::notifications::event_is_for_user(&event, user_id);
                    if should_send {
                        let data = serde_json::to_string(&event).unwrap_or_default();
                        if tx.send(Ok(Event::default().data(data))).await.is_err() {
                            break;
                        }
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!("Notification stream lagged by {} messages", n);
                    // 告知客户端丢事件，前端应重新 list() 补全历史
                    let resync = crate::services::agent::notifications::NotificationEvent::Resync {
                        lagged_by: n,
                    };
                    let data = serde_json::to_string(&resync).unwrap_or_default();
                    if tx.send(Ok(Event::default().data(data))).await.is_err() {
                        break;
                    }
                    continue;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    let stream = tokio_stream::wrappers::ReceiverStream::new(mpsc_rx);
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(30))))
}

/// 获取历史通知
pub(crate) async fn list_notifications(
    Extension(claims): Extension<Claims>,
    Query(params): Query<NotificationListParams>,
) -> Result<Json<Value>, HttpError> {
    let user_id = parse_user_id(&claims)?;
    let manager = crate::services::agent::notifications::get_notification_manager().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({"error": "Notification system not initialized"})),
    ))?;

    let limit = params.limit.unwrap_or(50).min(200);
    let notifications = manager.get_history_for_user(user_id, limit).await;
    let unread = manager.unread_count_for_user(user_id).await;
    let total = manager.total_count_for_user(user_id).await;

    Ok(Json(json!({
        "notifications": notifications,
        "unread_count": unread,
        "total": total,
    })))
}

/// 标记通知已读
pub(crate) async fn mark_notification_read(
    Extension(claims): Extension<Claims>,
    Path(notification_id): Path<String>,
) -> Result<Json<Value>, HttpError> {
    let user_id = parse_user_id(&claims)?;
    let manager = crate::services::agent::notifications::get_notification_manager().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({"error": "Notification system not initialized"})),
    ))?;

    let found = manager
        .mark_read(&notification_id, user_id)
        .await
        .map_err(|error| {
            HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": error})),
            ))
        })?;
    Ok(Json(json!({"success": found})))
}

/// 标记全部已读
pub(crate) async fn mark_all_notifications_read(
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpError> {
    let user_id = parse_user_id(&claims)?;
    let manager = crate::services::agent::notifications::get_notification_manager().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({"error": "Notification system not initialized"})),
    ))?;

    manager.mark_all_read(user_id).await.map_err(|error| {
        HttpError::from((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": error})),
        ))
    })?;
    Ok(Json(json!({"success": true})))
}

/// 删除单条通知
pub(crate) async fn delete_notification(
    Extension(claims): Extension<Claims>,
    Path(notification_id): Path<String>,
) -> Result<Json<Value>, HttpError> {
    let user_id = parse_user_id(&claims)?;
    let manager = crate::services::agent::notifications::get_notification_manager().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({"error": "Notification system not initialized"})),
    ))?;

    let removed = manager
        .delete_notification(&notification_id, user_id)
        .await
        .map_err(|error| {
            HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": error})),
            ))
        })?;
    if !removed {
        return Err(HttpError::from((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "Notification not found"})),
        )));
    }
    Ok(Json(json!({"success": true})))
}

/// 清空全部通知
pub(crate) async fn clear_notifications(
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpError> {
    let user_id = parse_user_id(&claims)?;
    let manager = crate::services::agent::notifications::get_notification_manager().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({"error": "Notification system not initialized"})),
    ))?;

    let deleted = manager.clear_all(user_id).await.map_err(|error| {
        HttpError::from((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": error})),
        ))
    })?;
    Ok(Json(json!({"success": true, "deleted": deleted})))
}

#[derive(Deserialize)]
pub(crate) struct NotificationListParams {
    limit: Option<usize>,
}
