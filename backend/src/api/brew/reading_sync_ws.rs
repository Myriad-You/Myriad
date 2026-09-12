//! Brew reading state, item detail, stats, and WebSocket.
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Extension, Json,
};
use chrono::Utc;
use sea_orm::{
    sea_query::Expr, ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait,
    DatabaseBackend, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder,
    QuerySelect, QueryTrait, Statement, TransactionTrait,
};
use serde_json::json;

use crate::error::HttpError;
use crate::middleware::auth::Claims;
use crate::models::entities::{
    brew_annotations, brew_items, brew_podcasts, brew_sources, brew_user_states,
};
use crate::services::brew_scheduler::get_brew_scheduler;

use super::helpers::{
    brew_http_err, brew_store_http, get_user_and_admin_status, get_user_id_from_headers,
};

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
                        Json(AppError::fail_json("Item not found")),
                    )));
                }
                // 只有登录用户才查询已读/收藏状态，游客跳过以节约计算
                let (is_read, is_starred, read_progress, state_revision) = if let Some(uid) =
                    user_id
                {
                    let state = brew_user_states::Entity::find()
                        .filter(brew_user_states::Column::UserId.eq(uid))
                        .filter(brew_user_states::Column::ItemId.eq(id))
                        .one(&db)
                        .await
                        .map_err(|error| brew_store_http("load article reading state", error))?;

                    (
                        state.as_ref().map(|s| s.is_read).unwrap_or(false),
                        state.as_ref().map(|s| s.is_starred).unwrap_or(false),
                        state.as_ref().and_then(|s| s.read_progress),
                        Some(state.as_ref().map_or(0, |s| s.revision)),
                    )
                } else {
                    // 游客不需要查询状态
                    (false, false, None, None)
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

                let mut response = brew_items::ItemResponse::from_model_with_ai(
                    item,
                    Some(source.name),
                    source.icon,
                    is_read,
                    is_starred,
                    read_progress,
                    has_ai_annotations,
                    has_ai_podcast,
                );

                response.state_revision = state_revision;
                Ok(Json(json!({ "success": true, "item": response })))
            } else {
                Err(HttpError::from((
                    StatusCode::NOT_FOUND,
                    Json(AppError::fail_json("Item not found")),
                )))
            }
        }
        Ok(None) => Err(HttpError::from((
            StatusCode::NOT_FOUND,
            Json(AppError::fail_json("Item not found")),
        ))),
        Err(e) => Err(brew_store_http("find article", e)),
    }
}

pub(crate) async fn fetch_fulltext(
    State(_db): State<DatabaseConnection>,
    _headers: axum::http::HeaderMap,
    Path(_id): Path<i32>,
) -> Result<Json<serde_json::Value>, HttpError> {
    Err(HttpError::from((
        StatusCode::NOT_IMPLEMENTED,
        Json(AppError::fail_json("Fulltext fetching not yet implemented")),
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

fn visible_state_sources(is_admin: bool) -> sea_orm::Select<brew_sources::Entity> {
    let query = brew_sources::Entity::find()
        .select_only()
        .column(brew_sources::Column::Id);
    if is_admin {
        query
    } else {
        query.filter(brew_sources::Column::AdminOnly.eq(false))
    }
}

pub(crate) async fn update_item_state(
    db: &DatabaseConnection,
    headers: &axum::http::HeaderMap,
    item_id: i32,
    is_read: Option<bool>,
    is_starred: Option<bool>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let user_id = get_user_id_from_headers(headers, db).await?;

    let (_, is_admin) = get_user_and_admin_status(headers, db).await;
    let visible_sources = visible_state_sources(is_admin).into_query();

    let now = Utc::now();

    let transaction = db
        .begin()
        .await
        .map_err(|error| brew_store_http("begin reading state write", error))?;
    // Lock the article even before a state row exists, then lock existing state.
    // This serializes first writes and keeps the count delta tied to the state read.
    let item_result = brew_items::Entity::find_by_id(item_id)
        .filter(brew_items::Column::SourceId.in_subquery(visible_sources))
        .lock_exclusive()
        .one(&transaction)
        .await;
    let existing = brew_user_states::Entity::find()
        .filter(brew_user_states::Column::UserId.eq(user_id))
        .filter(brew_user_states::Column::ItemId.eq(item_id))
        .lock_exclusive()
        .one(&transaction)
        .await;

    // 获取文章所属的 source_id（用于更新 unread_count）
    let item = match item_result {
        Ok(Some(item)) => item,
        Ok(None) => {
            return Err(brew_http_err(StatusCode::NOT_FOUND, "Item not found"));
        }
        Err(e) => {
            return Err(brew_store_http("find article", e));
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
            let previous_revision = state.revision;
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

            match active.update(&transaction).await {
                Ok(saved) => {
                    // 更新 source 的 unread_count
                    if let Some(read) = is_read {
                        if read != was_read {
                            update_source_unread_count(
                                &transaction,
                                source_id,
                                if read { -1 } else { 1 },
                            )
                            .await
                            .map_err(|error| brew_store_http("update unread count", error))?;
                        }
                    }
                    transaction
                        .commit()
                        .await
                        .map_err(|error| brew_store_http("commit reading state", error))?;
                    if is_starred == Some(true) && !was_starred {
                        crate::services::agent::merope::spawn_ingest(
                            user_id,
                            "brew.starred",
                            format!("Starred \"{}\"", item.title),
                        );
                    }
                    Ok(Json(
                        json!({ "success": true, "previous_revision": previous_revision, "revision": saved.revision }),
                    ))
                }
                Err(e) => Err(brew_store_http("update reading state", e)),
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

            match new_state.insert(&transaction).await {
                Ok(saved) => {
                    // 新记录：如果标记为已读，减少 unread_count
                    if is_read == Some(true) {
                        update_source_unread_count(&transaction, source_id, -1)
                            .await
                            .map_err(|error| brew_store_http("update unread count", error))?;
                    }
                    transaction
                        .commit()
                        .await
                        .map_err(|error| brew_store_http("commit reading state", error))?;
                    if is_starred == Some(true) {
                        crate::services::agent::merope::spawn_ingest(
                            user_id,
                            "brew.starred",
                            format!("Starred \"{}\"", item.title),
                        );
                    }
                    Ok(Json(
                        json!({ "success": true, "previous_revision": 0, "revision": saved.revision }),
                    ))
                }
                Err(e) => Err(brew_store_http("create reading state", e)),
            }
        }
        Err(e) => Err(brew_store_http("find reading state", e)),
    }
}

/// 更新订阅源的未读计数
/// 使用原子 SQL 避免并发读改写竞态（两个请求同时读取相同值后各自写回导致数据丢失）
pub(crate) async fn update_source_unread_count<C: ConnectionTrait>(
    db: &C,
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

    let sources: Vec<brew_sources::Model> = source_filter
        .all(&db)
        .await
        .map_err(|error| brew_store_http("find visible sources", error))?;

    let source_ids: Vec<i32> = sources.iter().map(|s| s.id).collect();

    if source_ids.is_empty() {
        return Ok(Json(json!({ "success": true, "marked": 0 })));
    }

    query = query.filter(brew_items::Column::SourceId.is_in(source_ids.clone()));

    if let Some(before) = req.before {
        let dt = chrono::DateTime::from_timestamp_millis(before)
            .ok_or_else(|| brew_http_err(StatusCode::BAD_REQUEST, "Invalid reading cutoff"))?;
        query = query.filter(brew_items::Column::PublishedAt.lt(dt));
    }

    let transaction = db
        .begin()
        .await
        .map_err(|error| brew_store_http("begin bulk reading state write", error))?;

    // 获取文章 ID 和对应的 source_id
    let items: Vec<(i32, i32)> = query
        .select_only()
        .column(brew_items::Column::Id)
        .column(brew_items::Column::SourceId)
        .order_by_asc(brew_items::Column::Id)
        .lock_exclusive()
        .into_tuple()
        .all(&transaction)
        .await
        .map_err(|error| brew_store_http("load bulk articles", error))?;

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
        .order_by_asc(brew_user_states::Column::ItemId)
        .lock_exclusive()
        .all(&transaction)
        .await
        .map_err(|error| brew_store_http("load bulk reading states", error))?;

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
    let mut source_marked_counts: std::collections::BTreeMap<i32, i32> =
        std::collections::BTreeMap::new();

    // 批量更新已存在的未读状态
    if !unread_state_ids.is_empty() {
        let update_result = brew_user_states::Entity::update_many()
            .col_expr(brew_user_states::Column::IsRead, Expr::value(true))
            .col_expr(brew_user_states::Column::ReadAt, Expr::value(now))
            .col_expr(brew_user_states::Column::UpdatedAt, Expr::value(now))
            .filter(brew_user_states::Column::Id.is_in(unread_state_ids))
            .exec(&transaction)
            .await
            .map_err(|error| brew_store_http("update bulk reading states", error))?;

        {
            let result = update_result;
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
        brew_user_states::Entity::insert_many(new_states)
            .exec(&transaction)
            .await
            .map_err(|error| brew_store_http("insert bulk reading states", error))?;
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
        update_source_unread_count(&transaction, source_id, -count)
            .await
            .map_err(|error| brew_store_http("update bulk unread counts", error))?;
    }

    let changes: Vec<serde_json::Value> = existing_states.iter()
        .filter(|state| !state.is_read)
        .map(|state| json!({ "item_id": state.item_id, "previous_revision": state.revision, "revision": state.revision + 1 }))
        .chain(missing_item_ids.iter().map(|item_id| json!({ "item_id": item_id, "previous_revision": 0, "revision": 1 })))
        .collect();
    transaction
        .commit()
        .await
        .map_err(|error| brew_store_http("commit bulk reading states", error))?;
    Ok(Json(
        json!({ "success": true, "marked": marked, "changes": changes }),
    ))
}

/// Commit a state write together with its derived count, or roll both back.
async fn commit_synced_state(
    transaction: sea_orm::DatabaseTransaction,
    source_id: i32,
    delta: i32,
) -> Result<(), sea_orm::DbErr> {
    if delta != 0 {
        if let Err(error) = update_source_unread_count(&transaction, source_id, delta).await {
            let _ = transaction.rollback().await;
            return Err(error);
        }
    }
    transaction.commit().await
}

async fn discard_sync_tx(transaction: sea_orm::DatabaseTransaction) {
    let _ = transaction.rollback().await;
}

#[derive(Debug)]
enum SyncedStateApply {
    Confirmed { revision: i64 },
    Conflict(brew_user_states::SyncConflict),
    Failed,
}

fn unread_delta(previous: bool, next: Option<bool>) -> i32 {
    match next {
        Some(read) if read != previous => {
            if read {
                -1
            } else {
                1
            }
        }
        _ => 0,
    }
}

fn apply_sync_fields(
    mut active: brew_user_states::ActiveModel,
    state_item: &brew_user_states::SyncStateItem,
    now: chrono::DateTime<Utc>,
) -> brew_user_states::ActiveModel {
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
    if let Some(progress) = brew_user_states::normalize_read_progress(state_item.read_progress) {
        active.read_progress = Set(Some(progress));
    }
    active.updated_at = Set(now.into());
    active
}

/// One article: lock article then state, write both the row and its count, or roll back.
async fn apply_synced_state(
    db: &DatabaseConnection,
    user_id: i32,
    is_admin: bool,
    now: chrono::DateTime<Utc>,
    state_item: &brew_user_states::SyncStateItem,
) -> SyncedStateApply {
    let transaction = match db.begin().await {
        Ok(transaction) => transaction,
        Err(_) => return SyncedStateApply::Failed,
    };
    let source_id = match brew_items::Entity::find_by_id(state_item.item_id)
        .filter(
            brew_items::Column::SourceId.in_subquery(visible_state_sources(is_admin).into_query()),
        )
        .select_only()
        .column(brew_items::Column::SourceId)
        .lock_exclusive()
        .into_tuple::<i32>()
        .one(&transaction)
        .await
    {
        Ok(Some(source_id)) => source_id,
        _ => {
            discard_sync_tx(transaction).await;
            return SyncedStateApply::Failed;
        }
    };

    let existing = match brew_user_states::Entity::find()
        .filter(brew_user_states::Column::UserId.eq(user_id))
        .filter(brew_user_states::Column::ItemId.eq(state_item.item_id))
        .lock_exclusive()
        .one(&transaction)
        .await
    {
        Ok(existing) => existing,
        Err(_) => {
            discard_sync_tx(transaction).await;
            return SyncedStateApply::Failed;
        }
    };

    if let Some(server_state) = existing {
        let server_ts = server_state.updated_at.timestamp_millis();
        if state_item.conflicts_with(server_state.revision, server_ts) {
            let conflict = brew_user_states::SyncConflict {
                server_revision: server_state.revision,
                item_id: state_item.item_id,
                server_updated_at: server_ts,
                client_updated_at: state_item.updated_at,
            };
            discard_sync_tx(transaction).await;
            return SyncedStateApply::Conflict(conflict);
        }

        let state_id = server_state.id;
        let previous_revision = server_state.revision;
        let was_read = server_state.is_read;
        let active = apply_sync_fields(server_state.into(), state_item, now);
        let updated = brew_user_states::Entity::update_many()
            .set(active)
            .filter(brew_user_states::Column::Id.eq(state_id))
            .filter(brew_user_states::Column::Revision.eq(previous_revision))
            .exec(&transaction)
            .await;
        if matches!(&updated, Ok(result) if result.rows_affected == 1) {
            if commit_synced_state(
                transaction,
                source_id,
                unread_delta(was_read, state_item.is_read),
            )
            .await
            .is_err()
            {
                return SyncedStateApply::Failed;
            }
            SyncedStateApply::Confirmed {
                revision: previous_revision + 1,
            }
        } else if matches!(&updated, Ok(result) if result.rows_affected == 0) {
            discard_sync_tx(transaction).await;
            match current_sync_conflict(db, user_id, state_item).await {
                Ok(conflict) => SyncedStateApply::Conflict(conflict),
                Err(_) => SyncedStateApply::Failed,
            }
        } else {
            discard_sync_tx(transaction).await;
            SyncedStateApply::Failed
        }
    } else {
        if state_item
            .expected_revision
            .is_some_and(|revision| revision != 0)
        {
            discard_sync_tx(transaction).await;
            return SyncedStateApply::Conflict(brew_user_states::SyncConflict {
                item_id: state_item.item_id,
                server_revision: 0,
                server_updated_at: 0,
                client_updated_at: state_item.updated_at,
            });
        }

        let is_read = state_item.is_read.unwrap_or(false);
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
        match new_state.insert(&transaction).await {
            Ok(inserted) => {
                if commit_synced_state(transaction, source_id, if is_read { -1 } else { 0 })
                    .await
                    .is_err()
                {
                    return SyncedStateApply::Failed;
                }
                SyncedStateApply::Confirmed {
                    revision: inserted.revision,
                }
            }
            Err(_) => {
                discard_sync_tx(transaction).await;
                match current_sync_conflict(db, user_id, state_item).await {
                    Ok(conflict) if conflict.server_revision > 0 => {
                        SyncedStateApply::Conflict(conflict)
                    }
                    _ => SyncedStateApply::Failed,
                }
            }
        }
    }
}

// A failed compare-and-swap is a conflict, not a retryable storage failure.
// Re-read after the competing write so the response describes current state.
async fn current_sync_conflict(
    db: &DatabaseConnection,
    user_id: i32,
    state: &brew_user_states::SyncStateItem,
) -> Result<brew_user_states::SyncConflict, sea_orm::DbErr> {
    let current = brew_user_states::Entity::find()
        .filter(brew_user_states::Column::UserId.eq(user_id))
        .filter(brew_user_states::Column::ItemId.eq(state.item_id))
        .one(db)
        .await?;
    Ok(brew_user_states::SyncConflict {
        item_id: state.item_id,
        server_revision: current.as_ref().map_or(0, |value| value.revision),
        server_updated_at: current
            .as_ref()
            .map_or(0, |value| value.updated_at.timestamp_millis()),
        client_updated_at: state.updated_at,
    })
}

// 离线同步

pub(crate) async fn sync_states(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Json(req): Json<brew_user_states::SyncStatesRequest>,
) -> Result<Json<serde_json::Value>, HttpError> {
    req.validate_targets()
        .map_err(|message| brew_http_err(StatusCode::BAD_REQUEST, message))?;
    let user_id = get_user_id_from_headers(&headers, &db).await?;

    let (_, is_admin) = get_user_and_admin_status(&headers, &db).await;

    let now = Utc::now();
    let mut synced = 0;
    let mut confirmed = Vec::new();
    let mut revisions = std::collections::HashMap::new();
    let mut failed = Vec::new();
    let mut conflicts = Vec::new();
    let item_ids: Vec<i32> = req.states.iter().map(|s| s.item_id).collect();

    let item_source_map: std::collections::HashMap<i32, i32> = if !item_ids.is_empty() {
        brew_items::Entity::find()
            .filter(brew_items::Column::Id.is_in(item_ids))
            .filter(
                brew_items::Column::SourceId
                    .in_subquery(visible_state_sources(is_admin).into_query()),
            )
            .select_only()
            .column(brew_items::Column::Id)
            .column(brew_items::Column::SourceId)
            .into_tuple::<(i32, i32)>()
            .all(&db)
            .await
            .map_err(|error| brew_store_http("find visible articles", error))?
            .into_iter()
            .collect()
    } else {
        std::collections::HashMap::new()
    };

    // Validate the entire batch before the first write. Missing and hidden
    // articles share the same response, without disclosing private source data.
    if req
        .states
        .iter()
        .any(|state| !item_source_map.contains_key(&state.item_id))
    {
        return Err(brew_http_err(StatusCode::NOT_FOUND, "Item not found"));
    }

    for state_item in req.states {
        match apply_synced_state(&db, user_id, is_admin, now, &state_item).await {
            SyncedStateApply::Confirmed { revision } => {
                synced += 1;
                confirmed.push(state_item.item_id);
                revisions.insert(state_item.item_id, revision);
            }
            SyncedStateApply::Conflict(conflict) => conflicts.push(conflict),
            SyncedStateApply::Failed => failed.push(state_item.item_id),
        }
    }

    Ok(Json(json!(brew_user_states::SyncStatesResponse {
        synced,
        revisions,
        confirmed,
        failed,
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

#[cfg(test)]
mod state_visibility_tests {
    use super::*;

    #[test]
    fn non_admin_state_targets_require_public_sources() {
        let sql = visible_state_sources(false)
            .build(DatabaseBackend::Postgres)
            .to_string();
        assert!(sql.contains("\"admin_only\" = FALSE"), "{sql}");
        let admin = visible_state_sources(true)
            .build(DatabaseBackend::Postgres)
            .to_string();
        assert!(!admin.contains("admin_only"), "{admin}");
    }
}

#[cfg(test)]
mod sync_transaction_tests {
    use super::*;

    #[test]
    fn unread_delta_only_changes_when_read_flag_flips() {
        assert_eq!(unread_delta(false, Some(true)), -1);
        assert_eq!(unread_delta(true, Some(false)), 1);
        assert_eq!(unread_delta(true, Some(true)), 0);
        assert_eq!(unread_delta(false, None), 0);
        assert_eq!(unread_delta(false, Some(false)), 0);
    }

    fn isolated_url() -> String {
        std::env::var("BREW_REVISION_TEST_DATABASE_URL").expect("isolated test database URL")
    }

    async fn isolated_db(max_connections: u32) -> DatabaseConnection {
        let mut options = sea_orm::ConnectOptions::new(isolated_url());
        options.max_connections(max_connections);
        sea_orm::Database::connect(options).await.unwrap()
    }

    fn sync_item(
        expected_revision: Option<i64>,
        is_read: Option<bool>,
    ) -> brew_user_states::SyncStateItem {
        brew_user_states::SyncStateItem {
            item_id: 10,
            expected_revision,
            is_read,
            is_starred: None,
            read_progress: None,
            updated_at: 1,
        }
    }

    async fn apply_read(
        db: &DatabaseConnection,
        expected_revision: Option<i64>,
        is_read: Option<bool>,
    ) -> SyncedStateApply {
        apply_synced_state(
            db,
            7,
            true,
            Utc::now(),
            &sync_item(expected_revision, is_read),
        )
        .await
    }

    async fn scalar_i32(db: &DatabaseConnection, sql: &str) -> i32 {
        db.query_one_raw(Statement::from_string(DatabaseBackend::Postgres, sql))
            .await
            .unwrap()
            .unwrap()
            .try_get::<i32>("", "v")
            .unwrap()
    }

    async fn scalar_i64(db: &DatabaseConnection, sql: &str) -> i64 {
        db.query_one_raw(Statement::from_string(DatabaseBackend::Postgres, sql))
            .await
            .unwrap()
            .unwrap()
            .try_get::<i64>("", "v")
            .unwrap()
    }

    const SYNC_TABLES: &str = r#"
        CREATE TABLE brew_sources (
          id INTEGER PRIMARY KEY,
          admin_only BOOLEAN NOT NULL DEFAULT FALSE,
          unread_count INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE brew_items (
          id INTEGER PRIMARY KEY,
          source_id INTEGER NOT NULL
        );
        CREATE TABLE brew_user_states (
          id SERIAL PRIMARY KEY,
          user_id INTEGER NOT NULL,
          item_id INTEGER NOT NULL,
          is_read BOOLEAN NOT NULL DEFAULT FALSE,
          is_starred BOOLEAN NOT NULL DEFAULT FALSE,
          read_at TIMESTAMPTZ,
          read_progress REAL,
          starred_at TIMESTAMPTZ,
          notes TEXT,
          updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
          revision BIGINT NOT NULL DEFAULT 1,
          UNIQUE (user_id, item_id)
        );
        CREATE OR REPLACE FUNCTION brew_advance_state_revision() RETURNS trigger AS $$
        BEGIN
            NEW.revision := OLD.revision + 1;
            RETURN NEW;
        END;
        $$ LANGUAGE plpgsql;
        CREATE TRIGGER brew_state_revision BEFORE UPDATE ON brew_user_states
            FOR EACH ROW EXECUTE FUNCTION brew_advance_state_revision();
        INSERT INTO brew_sources VALUES (1, FALSE, 1);
        INSERT INTO brew_items VALUES (10, 1);
    "#;

    #[tokio::test]
    #[ignore = "requires explicit BREW_REVISION_TEST_DATABASE_URL for an isolated database"]
    async fn count_failure_rolls_back_state_and_success_commits_both() {
        let db = isolated_db(1).await;
        // Temporary tables shadow real names only on this test connection.
        db.execute_unprepared("CREATE TEMP TABLE brew_sources (id INTEGER PRIMARY KEY, unread_count INTEGER CHECK (unread_count <= 10)); CREATE TEMP TABLE brew_atomic_probe (id INTEGER PRIMARY KEY, revision INTEGER); INSERT INTO brew_sources VALUES (1, 10); INSERT INTO brew_atomic_probe VALUES (1, 1)").await.unwrap();
        let failed = db.begin().await.unwrap();
        failed
            .execute_unprepared("UPDATE brew_atomic_probe SET revision = revision + 1 WHERE id = 1")
            .await
            .unwrap();
        assert!(commit_synced_state(failed, 1, 1).await.is_err());
        let row = db
            .query_one_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                "SELECT revision, unread_count FROM brew_atomic_probe CROSS JOIN brew_sources",
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.try_get::<i32>("", "revision").unwrap(), 1);
        assert_eq!(row.try_get::<i32>("", "unread_count").unwrap(), 10);
        let success = db.begin().await.unwrap();
        success
            .execute_unprepared("UPDATE brew_atomic_probe SET revision = revision + 1 WHERE id = 1")
            .await
            .unwrap();
        commit_synced_state(success, 1, -1).await.unwrap();
        let row = db
            .query_one_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                "SELECT revision, unread_count FROM brew_atomic_probe CROSS JOIN brew_sources",
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.try_get::<i32>("", "revision").unwrap(), 2);
        assert_eq!(row.try_get::<i32>("", "unread_count").unwrap(), 9);
        db.close().await.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires explicit BREW_REVISION_TEST_DATABASE_URL for an isolated database"]
    async fn apply_confirms_then_conflicts_and_rolls_count_failure_back() {
        let db = isolated_db(1).await;
        db.execute_unprepared("CREATE TEMP TABLE brew_sources (id INTEGER PRIMARY KEY, admin_only BOOLEAN NOT NULL DEFAULT FALSE, unread_count INTEGER NOT NULL); CREATE TEMP TABLE brew_items (id INTEGER PRIMARY KEY, source_id INTEGER NOT NULL); CREATE TEMP TABLE brew_user_states (id SERIAL PRIMARY KEY, user_id INTEGER NOT NULL, item_id INTEGER NOT NULL, is_read BOOLEAN NOT NULL DEFAULT FALSE, is_starred BOOLEAN NOT NULL DEFAULT FALSE, read_at TIMESTAMPTZ, read_progress REAL, starred_at TIMESTAMPTZ, notes TEXT, updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), revision BIGINT NOT NULL DEFAULT 1, UNIQUE (user_id, item_id)); CREATE OR REPLACE FUNCTION brew_advance_state_revision() RETURNS trigger AS $$ BEGIN NEW.revision := OLD.revision + 1; RETURN NEW; END; $$ LANGUAGE plpgsql; CREATE TRIGGER brew_state_revision BEFORE UPDATE ON brew_user_states FOR EACH ROW EXECUTE FUNCTION brew_advance_state_revision(); INSERT INTO brew_sources VALUES (1, FALSE, 1); INSERT INTO brew_items VALUES (10, 1); INSERT INTO brew_items VALUES (11, 1);").await.unwrap();

        match apply_read(&db, Some(0), Some(true)).await {
            SyncedStateApply::Confirmed { revision } => assert_eq!(revision, 1),
            other => panic!("insert should confirm, got {other:?}"),
        }
        assert_eq!(
            scalar_i32(
                &db,
                "SELECT unread_count AS v FROM brew_sources WHERE id = 1"
            )
            .await,
            0
        );

        match apply_read(&db, Some(0), Some(true)).await {
            SyncedStateApply::Conflict(conflict) => {
                assert_eq!(conflict.item_id, 10);
                assert_eq!(conflict.server_revision, 1);
            }
            other => panic!("stale expected revision should conflict, got {other:?}"),
        }

        match apply_read(&db, Some(1), None).await {
            SyncedStateApply::Confirmed { revision } => assert_eq!(revision, 2),
            other => panic!("matching revision should confirm, got {other:?}"),
        }

        db.execute_unprepared("UPDATE brew_sources SET unread_count = 1; ALTER TABLE brew_sources ADD CHECK (unread_count >= 1)").await.unwrap();
        match apply_synced_state(
            &db,
            7,
            true,
            Utc::now(),
            &brew_user_states::SyncStateItem {
                item_id: 11,
                expected_revision: Some(0),
                is_read: Some(true),
                is_starred: None,
                read_progress: None,
                updated_at: 1,
            },
        )
        .await
        {
            SyncedStateApply::Failed => {}
            other => panic!("count check should fail the write, got {other:?}"),
        }
        assert_eq!(
            scalar_i32(
                &db,
                "SELECT COUNT(*)::int AS v FROM brew_user_states WHERE item_id = 11"
            )
            .await,
            0
        );
        assert_eq!(
            scalar_i32(
                &db,
                "SELECT unread_count AS v FROM brew_sources WHERE id = 1"
            )
            .await,
            1
        );
        db.close().await.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires explicit BREW_REVISION_TEST_DATABASE_URL for an isolated database"]
    async fn concurrent_first_inserts_confirm_once_and_keep_count() {
        let db = isolated_db(4).await;
        db.execute_unprepared("DROP TABLE IF EXISTS brew_user_states, brew_items, brew_sources CASCADE; DROP FUNCTION IF EXISTS brew_advance_state_revision() CASCADE;").await.unwrap();
        db.execute_unprepared(SYNC_TABLES).await.unwrap();

        let now = Utc::now();
        let left_db = db.clone();
        let right_db = db.clone();
        let left_item = sync_item(Some(0), Some(true));
        let right_item = sync_item(Some(0), Some(true));
        let (left, right) = tokio::join!(
            apply_synced_state(&left_db, 7, true, now, &left_item),
            apply_synced_state(&right_db, 7, true, now, &right_item),
        );
        let outcomes = [left, right];
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| matches!(outcome, SyncedStateApply::Confirmed { .. }))
                .count(),
            1,
            "{outcomes:?}"
        );
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| matches!(outcome, SyncedStateApply::Conflict(_)))
                .count(),
            1,
            "{outcomes:?}"
        );
        assert_eq!(
            scalar_i32(&db, "SELECT COUNT(*)::int AS v FROM brew_user_states").await,
            1
        );
        assert_eq!(
            scalar_i32(
                &db,
                "SELECT unread_count AS v FROM brew_sources WHERE id = 1"
            )
            .await,
            0
        );
        assert_eq!(
            scalar_i64(&db, "SELECT revision AS v FROM brew_user_states").await,
            1
        );
        db.execute_unprepared("DROP TABLE IF EXISTS brew_user_states, brew_items, brew_sources CASCADE; DROP FUNCTION IF EXISTS brew_advance_state_revision() CASCADE;").await.unwrap();
        db.close().await.unwrap();
    }
}
use myriad_error::AppError;
