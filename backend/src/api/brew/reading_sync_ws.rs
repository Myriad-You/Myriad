//! Brew reading state, item detail, stats, and WebSocket.
use axum::{
    extract::{
        Path, State,
    },
    http::StatusCode,
    response::IntoResponse,
    Extension, Json,
};
use chrono::Utc;
use sea_orm::{
    sea_query::Expr, ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait,
    DatabaseBackend, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter,
    QuerySelect, Statement,
};
use serde_json::json;

use crate::error::HttpError;
use crate::middleware::auth::Claims;
use crate::models::entities::{
    brew_annotations, brew_items, brew_podcasts, brew_sources, brew_user_states,
};
use crate::services::brew_scheduler::get_brew_scheduler;

use super::helpers::{brew_http_err, get_user_and_admin_status, get_user_id_from_headers};

/// 获取单篇文章详情（游客可访问）
/// 游客不查询已读/收藏状态以节约计算
/// 性能优化：并行查询 AI 状态
/// admin_only 源下的文章仅管理员可见
pub(crate) async fn get_item(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(id): Path<i32>,
) -> Result<Json<serde_json::Value>, HttpError> {
    // 获取可选用户 ID 与管理员状态
    let (user_id, is_admin) = get_user_and_admin_status(&headers, &db).await;

    // 获取文章
    let item = brew_items::Entity::find_by_id(id).one(&db).await;

    match item {
        Ok(Some(item)) => {
            // 获取订阅源信息
            let source = brew_sources::Entity::find_by_id(item.source_id)
                .one(&db)
                .await;

            if let Ok(Some(source)) = source {
                if source.admin_only && !is_admin {
                    return Err(HttpError::from((
                        StatusCode::NOT_FOUND,
                        Json(json!({ "success": false, "error": "Item not found" })),
                    )));
                }
                // 只有登录用户才查询已读/收藏状态，游客跳过以节约计算
                let (is_read, is_starred, read_progress) = if let Some(uid) = user_id {
                    let state = brew_user_states::Entity::find()
                        .filter(brew_user_states::Column::UserId.eq(uid))
                        .filter(brew_user_states::Column::ItemId.eq(id))
                        .one(&db)
                        .await
                        .ok()
                        .flatten();

                    (
                        state.as_ref().map(|s| s.is_read).unwrap_or(false),
                        state.as_ref().map(|s| s.is_starred).unwrap_or(false),
                        state.and_then(|s| s.read_progress),
                    )
                } else {
                    // 游客不需要查询状态
                    (false, false, None)
                };

                // 性能优化：并行查询 AI 状态（注释和播客）
                let (annotations_result, podcast_result) = tokio::join!(
                    brew_annotations::Entity::find()
                        .filter(brew_annotations::Column::ItemId.eq(id))
                        .count(&db),
                    brew_podcasts::Entity::find()
                        .filter(brew_podcasts::Column::ItemId.eq(id))
                        .count(&db)
                );

                let has_ai_annotations = annotations_result.unwrap_or(0) > 0;
                let has_ai_podcast = podcast_result.unwrap_or(0) > 0;

                let response = brew_items::ItemResponse::from_model_with_ai(
                    item,
                    Some(source.name),
                    source.icon,
                    is_read,
                    is_starred,
                    read_progress,
                    has_ai_annotations,
                    has_ai_podcast,
                );

                Ok(Json(json!({ "success": true, "item": response })))
            } else {
                Err(HttpError::from((
                    StatusCode::NOT_FOUND,
                    Json(json!({ "success": false, "error": "Item not found" })),
                )))
            }
        }
        Ok(None) => Err(HttpError::from((
            StatusCode::NOT_FOUND,
            Json(json!({ "success": false, "error": "Item not found" })),
        ))),
        Err(e) => {
            tracing::error!(error = %e, "Database error");
            Err(brew_http_err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Database error",
            ))
        }
    }
}

pub(crate) async fn fetch_fulltext(
    State(_db): State<DatabaseConnection>,
    _headers: axum::http::HeaderMap,
    Path(_id): Path<i32>,
) -> Result<Json<serde_json::Value>, HttpError> {
    // TODO: 实现全文抓取
    Err(HttpError::from((
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({ "success": false, "error": "Fulltext fetching not yet implemented" })),
    )))
}

// 阅读状态

pub(crate) async fn mark_read(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(item_id): Path<i32>,
) -> Result<Json<serde_json::Value>, HttpError> {
    update_item_state(&db, &headers, item_id, Some(true), None).await
}

pub(crate) async fn mark_unread(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(item_id): Path<i32>,
) -> Result<Json<serde_json::Value>, HttpError> {
    update_item_state(&db, &headers, item_id, Some(false), None).await
}

pub(crate) async fn star_item(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(item_id): Path<i32>,
) -> Result<Json<serde_json::Value>, HttpError> {
    update_item_state(&db, &headers, item_id, None, Some(true)).await
}

pub(crate) async fn unstar_item(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(item_id): Path<i32>,
) -> Result<Json<serde_json::Value>, HttpError> {
    update_item_state(&db, &headers, item_id, None, Some(false)).await
}

pub(crate) async fn update_item_state(
    db: &DatabaseConnection,
    headers: &axum::http::HeaderMap,
    item_id: i32,
    is_read: Option<bool>,
    is_starred: Option<bool>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let user_id = get_user_id_from_headers(headers, db).await?;

    let now = Utc::now();

    // 性能优化：并行查询文章信息和用户状态
    let (item_result, existing) = tokio::join!(
        brew_items::Entity::find_by_id(item_id).one(db),
        brew_user_states::Entity::find()
            .filter(brew_user_states::Column::UserId.eq(user_id))
            .filter(brew_user_states::Column::ItemId.eq(item_id))
            .one(db)
    );

    // 获取文章所属的 source_id（用于更新 unread_count）
    let item = match item_result {
        Ok(Some(item)) => item,
        Ok(None) => {
            return Err(brew_http_err(StatusCode::NOT_FOUND, "Item not found"));
        }
        Err(e) => {
            tracing::error!(error = %e, "Database error");
            return Err(brew_http_err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Database error",
            ));
        }
    };
    let source_id = item.source_id;

    // 记录之前的已读状态，用于计算 unread_count 变化
    let was_read = match &existing {
        Ok(Some(state)) => state.is_read,
        _ => false,
    };
    let was_starred = match &existing {
        Ok(Some(state)) => state.is_starred,
        _ => false,
    };

    match existing {
        Ok(Some(state)) => {
            let mut active: brew_user_states::ActiveModel = state.into();

            if let Some(read) = is_read {
                active.is_read = Set(read);
                if read {
                    active.read_at = Set(Some(now.into()));
                }
            }
            if let Some(starred) = is_starred {
                active.is_starred = Set(starred);
                if starred {
                    active.starred_at = Set(Some(now.into()));
                }
            }
            active.updated_at = Set(now.into());

            match active.update(db).await {
                Ok(_) => {
                    // 更新 source 的 unread_count
                    if let Some(read) = is_read {
                        if read != was_read {
                            let _ = update_source_unread_count(
                                db,
                                source_id,
                                if read { -1 } else { 1 },
                            )
                            .await;
                        }
                    }
                    if is_starred == Some(true) && !was_starred {
                        crate::services::agent::life::spawn_ingest(
                            user_id,
                            "brew.starred",
                            format!("把《{}》标了星", item.title),
                        );
                    }
                    Ok(Json(json!({ "success": true })))
                }
                Err(e) => {
                    tracing::error!(error = %e, "Database error");
                    Err(brew_http_err(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Database error",
                    ))
                }
            }
        }
        Ok(None) => {
            // 创建新记录
            let new_state = brew_user_states::ActiveModel {
                user_id: Set(user_id),
                item_id: Set(item_id),
                is_read: Set(is_read.unwrap_or(false)),
                is_starred: Set(is_starred.unwrap_or(false)),
                read_at: Set(if is_read == Some(true) {
                    Some(now.into())
                } else {
                    None
                }),
                starred_at: Set(if is_starred == Some(true) {
                    Some(now.into())
                } else {
                    None
                }),
                updated_at: Set(now.into()),
                ..Default::default()
            };

            match new_state.insert(db).await {
                Ok(_) => {
                    // 新记录：如果标记为已读，减少 unread_count
                    if is_read == Some(true) {
                        let _ = update_source_unread_count(db, source_id, -1).await;
                    }
                    if is_starred == Some(true) {
                        crate::services::agent::life::spawn_ingest(
                            user_id,
                            "brew.starred",
                            format!("把《{}》标了星", item.title),
                        );
                    }
                    Ok(Json(json!({ "success": true })))
                }
                Err(e) => {
                    tracing::error!(error = %e, "Database error");
                    Err(brew_http_err(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Database error",
                    ))
                }
            }
        }
        Err(e) => {
            tracing::error!(error = %e, "Database error");
            Err(brew_http_err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Database error",
            ))
        }
    }
}

/// 更新订阅源的未读计数
/// 使用原子 SQL 避免并发读改写竞态（两个请求同时读取相同值后各自写回导致数据丢失）
pub(crate) async fn update_source_unread_count(
    db: &DatabaseConnection,
    source_id: i32,
    delta: i32,
) -> Result<(), sea_orm::DbErr> {
    let stmt = Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE brew_sources SET unread_count = GREATEST(0, unread_count + $1) WHERE id = $2",
        [delta.into(), source_id.into()],
    );
    db.execute_raw(stmt).await?;
    Ok(())
}

pub(crate) async fn mark_all_read(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Json(req): Json<brew_user_states::MarkAllReadRequest>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let user_id = get_user_id_from_headers(&headers, &db).await?;
    // 共享订阅库：按当前用户可见源标记，而非「我创建的源」
    let (_, is_admin) = get_user_and_admin_status(&headers, &db).await;

    // 获取要标记的文章
    let mut query = brew_items::Entity::find();

    // 可见订阅源（非管理员排除 admin_only）
    let mut source_filter = brew_sources::Entity::find();
    if !is_admin {
        source_filter = source_filter.filter(brew_sources::Column::AdminOnly.eq(false));
    }

    if let Some(source_id) = req.source_id {
        source_filter = source_filter.filter(brew_sources::Column::Id.eq(source_id));
    }
    if let Some(ref category) = req.category {
        source_filter = source_filter.filter(brew_sources::Column::Category.eq(category));
    }

    let sources: Vec<brew_sources::Model> = source_filter.all(&db).await.unwrap_or_default();

    let source_ids: Vec<i32> = sources.iter().map(|s| s.id).collect();

    if source_ids.is_empty() {
        return Ok(Json(json!({ "success": true, "marked": 0 })));
    }

    query = query.filter(brew_items::Column::SourceId.is_in(source_ids.clone()));

    if let Some(before) = req.before {
        if let Some(dt) = chrono::DateTime::from_timestamp_millis(before) {
            query = query.filter(brew_items::Column::PublishedAt.lt(dt));
        }
    }

    // 获取文章 ID 和对应的 source_id
    let items: Vec<(i32, i32)> = query
        .select_only()
        .column(brew_items::Column::Id)
        .column(brew_items::Column::SourceId)
        .into_tuple()
        .all(&db)
        .await
        .unwrap_or_default();

    if items.is_empty() {
        return Ok(Json(json!({ "success": true, "marked": 0 })));
    }

    let now = Utc::now();

    // 收集所有 item_id
    let item_ids: Vec<i32> = items.iter().map(|(id, _)| *id).collect();

    // 构建 item_id -> source_id 映射
    let item_source_map: std::collections::HashMap<i32, i32> = items.into_iter().collect();

    // 批量查询所有已存在的状态
    let existing_states: Vec<brew_user_states::Model> = brew_user_states::Entity::find()
        .filter(brew_user_states::Column::UserId.eq(user_id))
        .filter(brew_user_states::Column::ItemId.is_in(item_ids.clone()))
        .all(&db)
        .await
        .unwrap_or_default();

    let existing_item_ids: std::collections::HashSet<i32> =
        existing_states.iter().map(|s| s.item_id).collect();

    // 找出需要更新的（已存在但未读的）
    let unread_state_ids: Vec<i32> = existing_states
        .iter()
        .filter(|s| !s.is_read)
        .map(|s| s.id)
        .collect();

    // 找出需要插入的（不存在的）
    let missing_item_ids: Vec<i32> = item_ids
        .iter()
        .filter(|id| !existing_item_ids.contains(id))
        .copied()
        .collect();

    let mut marked = 0;
    let mut source_marked_counts: std::collections::HashMap<i32, i32> =
        std::collections::HashMap::new();

    // 批量更新已存在的未读状态
    if !unread_state_ids.is_empty() {
        let update_result = brew_user_states::Entity::update_many()
            .col_expr(brew_user_states::Column::IsRead, Expr::value(true))
            .col_expr(brew_user_states::Column::ReadAt, Expr::value(now))
            .col_expr(brew_user_states::Column::UpdatedAt, Expr::value(now))
            .filter(brew_user_states::Column::Id.is_in(unread_state_ids))
            .exec(&db)
            .await;

        if let Ok(result) = update_result {
            marked += result.rows_affected as i32;
            // 统计每个 source 被标记的数量
            for state in existing_states.iter().filter(|s| !s.is_read) {
                if let Some(&source_id) = item_source_map.get(&state.item_id) {
                    *source_marked_counts.entry(source_id).or_insert(0) += 1;
                }
            }
        }
    }

    // 批量插入不存在的状态
    if !missing_item_ids.is_empty() {
        let new_states: Vec<brew_user_states::ActiveModel> = missing_item_ids
            .iter()
            .map(|&item_id| brew_user_states::ActiveModel {
                user_id: Set(user_id),
                item_id: Set(item_id),
                is_read: Set(true),
                is_starred: Set(false),
                read_at: Set(Some(now.into())),
                updated_at: Set(now.into()),
                ..Default::default()
            })
            .collect();

        let insert_count = new_states.len() as i32;
        if brew_user_states::Entity::insert_many(new_states)
            .exec(&db)
            .await
            .is_ok()
        {
            marked += insert_count;
            // 统计每个 source 被标记的数量
            for &item_id in &missing_item_ids {
                if let Some(&source_id) = item_source_map.get(&item_id) {
                    *source_marked_counts.entry(source_id).or_insert(0) += 1;
                }
            }
        }
    }

    // 更新每个 source 的 unread_count
    for (source_id, count) in source_marked_counts {
        let _ = update_source_unread_count(&db, source_id, -count).await;
    }

    Ok(Json(json!({ "success": true, "marked": marked })))
}

// 离线同步

pub(crate) async fn sync_states(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Json(req): Json<brew_user_states::SyncStatesRequest>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let user_id = get_user_id_from_headers(&headers, &db).await?;

    let now = Utc::now();
    let mut synced = 0;
    let mut conflicts = Vec::new();
    // source_id -> unread_count delta（与 update_item_state 一致维护全局缓存列）
    let mut source_unread_deltas: std::collections::HashMap<i32, i32> =
        std::collections::HashMap::new();

    // 性能优化：批量查询所有相关状态与文章 source_id，避免 N+1
    let item_ids: Vec<i32> = req.states.iter().map(|s| s.item_id).collect();
    let existing_states: std::collections::HashMap<i32, brew_user_states::Model> =
        if !item_ids.is_empty() {
            brew_user_states::Entity::find()
                .filter(brew_user_states::Column::UserId.eq(user_id))
                .filter(brew_user_states::Column::ItemId.is_in(item_ids.clone()))
                .all(&db)
                .await
                .unwrap_or_default()
                .into_iter()
                .map(|s| (s.item_id, s))
                .collect()
        } else {
            std::collections::HashMap::new()
        };

    let item_source_map: std::collections::HashMap<i32, i32> = if !item_ids.is_empty() {
        brew_items::Entity::find()
            .filter(brew_items::Column::Id.is_in(item_ids))
            .select_only()
            .column(brew_items::Column::Id)
            .column(brew_items::Column::SourceId)
            .into_tuple::<(i32, i32)>()
            .all(&db)
            .await
            .unwrap_or_default()
            .into_iter()
            .collect()
    } else {
        std::collections::HashMap::new()
    };

    for state_item in req.states {
        if let Some(server_state) = existing_states.get(&state_item.item_id) {
            let server_ts = server_state.updated_at.timestamp_millis();

            // 检查冲突：服务器更新时间比客户端新
            if server_ts > state_item.updated_at {
                conflicts.push(brew_user_states::SyncConflict {
                    item_id: state_item.item_id,
                    server_updated_at: server_ts,
                    client_updated_at: state_item.updated_at,
                });
                continue;
            }

            let was_read = server_state.is_read;

            // 应用客户端更新
            let mut active: brew_user_states::ActiveModel = server_state.clone().into();
            if let Some(is_read) = state_item.is_read {
                active.is_read = Set(is_read);
                if is_read {
                    active.read_at = Set(Some(now.into()));
                }
            }
            if let Some(is_starred) = state_item.is_starred {
                active.is_starred = Set(is_starred);
                if is_starred {
                    active.starred_at = Set(Some(now.into()));
                }
            }
            if let Some(progress) =
                brew_user_states::normalize_read_progress(state_item.read_progress)
            {
                active.read_progress = Set(Some(progress));
            }
            active.updated_at = Set(now.into());

            if active.update(&db).await.is_ok() {
                synced += 1;
                if let Some(is_read) = state_item.is_read {
                    if is_read != was_read {
                        if let Some(&source_id) = item_source_map.get(&state_item.item_id) {
                            let delta = if is_read { -1 } else { 1 };
                            *source_unread_deltas.entry(source_id).or_insert(0) += delta;
                        }
                    }
                }
            }
        } else {
            // 跳过不存在的文章，避免为任意 item_id 写状态
            if !item_source_map.contains_key(&state_item.item_id) {
                continue;
            }

            let is_read = state_item.is_read.unwrap_or(false);
            // 创建新记录
            let new_state = brew_user_states::ActiveModel {
                user_id: Set(user_id),
                item_id: Set(state_item.item_id),
                is_read: Set(is_read),
                is_starred: Set(state_item.is_starred.unwrap_or(false)),
                read_progress: Set(brew_user_states::normalize_read_progress(
                    state_item.read_progress,
                )),
                read_at: Set(if is_read { Some(now.into()) } else { None }),
                starred_at: Set(if state_item.is_starred == Some(true) {
                    Some(now.into())
                } else {
                    None
                }),
                updated_at: Set(now.into()),
                ..Default::default()
            };
            if new_state.insert(&db).await.is_ok() {
                synced += 1;
                if is_read {
                    if let Some(&source_id) = item_source_map.get(&state_item.item_id) {
                        *source_unread_deltas.entry(source_id).or_insert(0) -= 1;
                    }
                }
            }
        }
    }

    for (source_id, delta) in source_unread_deltas {
        if delta != 0 {
            let _ = update_source_unread_count(&db, source_id, delta).await;
        }
    }

    Ok(Json(json!(brew_user_states::SyncStatesResponse {
        synced,
        conflicts
    })))
}

// 统计信息

/// 获取统计信息（游客可访问）
/// 游客不计算已读/收藏统计以节约计算
pub(crate) async fn get_stats(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
) -> Result<Json<serde_json::Value>, HttpError> {
    // 获取可选用户 ID 与管理员状态
    let (user_id, is_admin) = get_user_and_admin_status(&headers, &db).await;

    // 可见订阅源统计（非管理员排除 admin_only）
    let mut sources_q = brew_sources::Entity::find();
    if !is_admin {
        sources_q = sources_q.filter(brew_sources::Column::AdminOnly.eq(false));
    }
    let sources = sources_q.all(&db).await.unwrap_or_default();

    let total_sources = sources.len();
    let total_items: i32 = sources.iter().map(|s| s.item_count).sum();

    // 只有登录用户才计算未读数和收藏数，游客跳过以节约计算
    let (total_unread, starred_count) = if let Some(uid) = user_id {
        let source_ids: Vec<i32> = sources.iter().map(|s| s.id).collect();

        // 获取所有文章 ID
        let all_item_ids: Vec<i32> = if !source_ids.is_empty() {
            brew_items::Entity::find()
                .filter(brew_items::Column::SourceId.is_in(source_ids))
                .select_only()
                .column(brew_items::Column::Id)
                .into_tuple()
                .all(&db)
                .await
                .unwrap_or_default()
        } else {
            vec![]
        };

        // 性能优化：并行查询已读数和收藏数
        let (read_count, starred) = if !all_item_ids.is_empty() {
            tokio::join!(
                // 获取已读文章数
                brew_user_states::Entity::find()
                    .filter(brew_user_states::Column::UserId.eq(uid))
                    .filter(brew_user_states::Column::ItemId.is_in(all_item_ids.clone()))
                    .filter(brew_user_states::Column::IsRead.eq(true))
                    .count(&db),
                // 获取收藏数
                brew_user_states::Entity::find()
                    .filter(brew_user_states::Column::UserId.eq(uid))
                    .filter(brew_user_states::Column::IsStarred.eq(true))
                    .count(&db)
            )
        } else {
            // 空列表时仍需查询收藏数
            let starred = brew_user_states::Entity::find()
                .filter(brew_user_states::Column::UserId.eq(uid))
                .filter(brew_user_states::Column::IsStarred.eq(true))
                .count(&db)
                .await;
            (Ok(0), starred)
        };

        let unread = (all_item_ids.len() as i32) - read_count.unwrap_or(0) as i32;
        (unread, starred.unwrap_or(0) as i64)
    } else {
        // 游客不计算未读和收藏
        (0, 0)
    };

    Ok(Json(json!({
        "success": true,
        "stats": {
            "total_sources": total_sources,
            "total_items": total_items,
            "total_unread": total_unread,
            "total_starred": starred_count,
        }
    })))
}

// WebSocket

pub(crate) async fn brew_websocket(
    ws: axum::extract::ws::WebSocketUpgrade,
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    headers: axum::http::HeaderMap,
) -> Result<impl IntoResponse, HttpError> {
    let allowed = crate::middleware::ws_origin::allowed_origins_from_global_config().await;
    crate::middleware::ws_origin::assert_ws_origin_for_cookie_session(&headers, &allowed)?;

    let user_id = claims
        .sub
        .parse::<i32>()
        .map_err(|_| brew_http_err(StatusCode::UNAUTHORIZED, "Unauthorized"))?;
    Ok(ws.on_upgrade(move |socket| handle_brew_websocket(socket, db, user_id)))
}

pub(crate) async fn handle_brew_websocket(
    mut socket: axum::extract::ws::WebSocket,
    _db: DatabaseConnection,
    user_id: i32,
) {
    use axum::extract::ws::Message;

    // 订阅通知
    if let Some(scheduler) = get_brew_scheduler() {
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
