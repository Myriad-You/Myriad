//! Phantasi feed items list and subscription topics.
use crate::error::HttpError;
use crate::extract::{AdminClaims, OptionalViewer};

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use myriad_phantasi::{
    FEED_TOPIC_CARDS_KEY, feed_topic_cards_from_value, sanitize_feed_topic_cards,
};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseBackend, DatabaseConnection, EntityTrait, PaginatorTrait,
    QueryFilter, QuerySelect, QueryTrait, Statement,
};
use serde::Deserialize;
use serde_json::json;

use crate::models::entities::phantasi_sources::SourceType;
use crate::models::entities::{phantasi_items, phantasi_sources, phantasi_user_states};
use crate::services::phantasi_topics::{
    TopicSuggestError, TopicWriteError, list_subscription_topic_names, normalize_topic_name,
    set_subscription_item_topic, suggest_subscription_item_topic,
};

use super::helpers::{
    get_phantasi_viewer, materialize_source_icon, phantasi_http_err, phantasi_store_http,
    require_viewer_admin,
};

// 订阅主题

#[derive(Deserialize)]
pub(crate) struct UpdateItemTopicRequest {
    topic: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct UpdateTopicCardsRequest {
    cards: Vec<String>,
}

async fn load_feed_topic_cards(db: &impl ConnectionTrait) -> Vec<String> {
    let result = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT value FROM configurations WHERE key = $1",
            vec![FEED_TOPIC_CARDS_KEY.into()],
        ))
        .await;
    match result {
        Ok(Some(row)) => match row.try_get::<serde_json::Value>("", "value") {
            Ok(value) => feed_topic_cards_from_value(&value),
            Err(error) => {
                tracing::warn!(%error, "failed to read feed topic cards");
                Vec::new()
            }
        },
        Ok(None) => Vec::new(),
        Err(error) => {
            tracing::warn!(%error, "failed to load feed topic cards");
            Vec::new()
        }
    }
}

fn topic_write_http(err: TopicWriteError) -> HttpError {
    match err {
        TopicWriteError::NotFound => phantasi_http_err(StatusCode::NOT_FOUND, "Article not found"),
        TopicWriteError::NoteItem => phantasi_http_err(
            StatusCode::BAD_REQUEST,
            "Note categories are edited in the note editor",
        ),
        TopicWriteError::Store => phantasi_store_http("update topic", "store failed"),
    }
}

fn topic_suggest_http(err: TopicSuggestError) -> HttpError {
    match err {
        TopicSuggestError::NotFound => {
            phantasi_http_err(StatusCode::NOT_FOUND, "Article not found")
        }
        TopicSuggestError::NoteItem => phantasi_http_err(
            StatusCode::BAD_REQUEST,
            "Note categories are edited in the note editor",
        ),
        TopicSuggestError::Unavailable => {
            phantasi_http_err(StatusCode::SERVICE_UNAVAILABLE, "AI service unavailable")
        }
        TopicSuggestError::Failed => {
            phantasi_http_err(StatusCode::INTERNAL_SERVER_ERROR, "Failed to suggest topic")
        }
        TopicSuggestError::Store => phantasi_store_http("suggest topic", "store failed"),
    }
}

/// 本站已有的订阅主题名。笔记分类不进这里。
pub(crate) async fn list_subscription_topics(
    State(db): State<DatabaseConnection>,
    viewer: OptionalViewer,
) -> Result<Json<serde_json::Value>, HttpError> {
    let (_, is_admin) = get_phantasi_viewer(&viewer, &db).await?;
    let topics = list_subscription_topic_names(&db, is_admin)
        .await
        .map_err(|e| phantasi_store_http("list topics", e))?;
    let cards = sanitize_feed_topic_cards(&load_feed_topic_cards(&db).await, &topics);
    Ok(Json(json!({
        "success": true,
        "topics": topics,
        "cards": cards,
    })))
}

/// 站长勾选哪些已有主题在订阅墙出混排卡。访客只读 GET `/topics` 里的 `cards`。
pub(crate) async fn put_feed_topic_cards(
    State(db): State<DatabaseConnection>,
    _admin: AdminClaims,
    Json(body): Json<UpdateTopicCardsRequest>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let topics = list_subscription_topic_names(&db, true)
        .await
        .map_err(|e| phantasi_store_http("list topics", e))?;
    let requested: Vec<String> = body
        .cards
        .iter()
        .filter_map(|name| normalize_topic_name(name))
        .collect();
    let cards = sanitize_feed_topic_cards(&requested, &topics);
    let config_service = crate::services::config_service::ConfigService::new(db);
    if let Err(error) = config_service
        .update_config(FEED_TOPIC_CARDS_KEY, json!(cards))
        .await
    {
        tracing::error!(%error, "failed to save feed topic cards");
        return Err(phantasi_http_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to save topic cards",
        ));
    }
    Ok(Json(json!({ "success": true, "cards": cards })))
}

/// 站长手填订阅文章主题。笔记走编辑器，不走这条。
pub(crate) async fn update_item_topic(
    State(db): State<DatabaseConnection>,
    _admin: AdminClaims,
    Path(id): Path<i32>,
    Json(body): Json<UpdateItemTopicRequest>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let topic = set_subscription_item_topic(&db, id, body.topic.as_deref())
        .await
        .map_err(topic_write_http)?;
    Ok(Json(json!({ "success": true, "topic": topic })))
}

/// 按正文建议主题并写回。站长可再手改。
pub(crate) async fn suggest_item_topic(
    State(db): State<DatabaseConnection>,
    _admin: AdminClaims,
    Path(id): Path<i32>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let topic = suggest_subscription_item_topic(&db, id, true)
        .await
        .map_err(topic_suggest_http)?;
    Ok(Json(json!({
        "success": true,
        "topic": topic,
    })))
}

// 文章获取

/// 获取文章列表（游客可访问）
/// 游客不计算已读/收藏状态以节约计算
/// 非管理员看不到 admin_only 源下的文章
pub(crate) async fn list_items(
    State(db): State<DatabaseConnection>,
    viewer: OptionalViewer,
    Query(query): Query<phantasi_items::ItemsQuery>,
) -> Result<Json<serde_json::Value>, HttpError> {
    // 获取可选用户 ID 与管理员状态
    let (user_id, is_admin) = get_phantasi_viewer(&viewer, &db).await?;

    let mut visible_sources = phantasi_sources::Entity::find()
        .select_only()
        .column(phantasi_sources::Column::Id);
    if !is_admin {
        visible_sources = visible_sources.filter(phantasi_sources::Column::AdminOnly.eq(false));
    }
    let visible_source_ids = visible_sources.into_query();

    let page = query.page.unwrap_or(1).max(1);
    let per_page = query.per_page.unwrap_or(20).clamp(1, 100);
    let cursor = match query
        .cursor
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        None => None,
        Some(raw) => match phantasi_items::decode_item_cursor(raw) {
            Some(cursor) => Some(cursor),
            None => {
                return Err(phantasi_http_err(
                    StatusCode::BAD_REQUEST,
                    "Invalid list cursor",
                ));
            }
        },
    };
    let filter_type = query.filter.as_deref().unwrap_or("all");
    if filter_type == "starred" {
        require_viewer_admin(&viewer)?;
    }

    // 构建查询
    let mut items_query = phantasi_items::Entity::find()
        .filter(phantasi_items::Column::SourceId.in_subquery(visible_source_ids));

    // 未读给登录用户；收藏筛选只给管理员。
    if user_id.is_some() {
        match filter_type {
            "starred" => {
                if let Some(uid) = user_id {
                    let starred_subquery = phantasi_user_states::Entity::find()
                        .filter(phantasi_user_states::Column::UserId.eq(uid))
                        .filter(phantasi_user_states::Column::IsStarred.eq(true))
                        .select_only()
                        .column(phantasi_user_states::Column::ItemId)
                        .into_query();
                    items_query = items_query
                        .filter(phantasi_items::Column::Id.in_subquery(starred_subquery));
                }
            }
            "unread" => {
                // 用子查询替代 NOT IN (ids)，避免已读文章数万条时生成巨型参数列表
                if let Some(uid) = user_id {
                    let read_subquery = phantasi_user_states::Entity::find()
                        .filter(phantasi_user_states::Column::UserId.eq(uid))
                        .filter(phantasi_user_states::Column::IsRead.eq(true))
                        .select_only()
                        .column(phantasi_user_states::Column::ItemId)
                        .into_query(); // QueryTrait::into_query() 消耗 Select 返回 SelectStatement
                    items_query = items_query
                        .filter(phantasi_items::Column::Id.not_in_subquery(read_subquery));
                }
            }
            _ => {}
        }
    }

    // 按订阅源筛选
    if let Some(source_id) = query.source_id {
        items_query = items_query.filter(phantasi_items::Column::SourceId.eq(source_id));
    }

    // 按主题筛选。与 category 同级：只回该主题的订阅文章，笔记分类同名也不进来。
    // `topic IS NULL` 的天然落空。读路径只读已有列，绝不在这里现算。
    if let Some(topic) = query
        .topic
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        items_query = items_query.filter(phantasi_items::Column::Topic.eq(topic));
        let note_source_ids = phantasi_sources::Entity::find()
            .filter(phantasi_sources::Column::SourceType.eq(SourceType::Note))
            .select_only()
            .column(phantasi_sources::Column::Id)
            .into_query();
        items_query =
            items_query.filter(phantasi_items::Column::SourceId.not_in_subquery(note_source_ids));
    }

    // 按分类筛选（支持多分类：category 字段可能是逗号分隔的多个分类）
    if let Some(ref category) = query.category {
        let cat_source_ids = phantasi_sources::Entity::find()
            .filter(
                sea_orm::Condition::any()
                    .add(phantasi_sources::Column::Category.eq(category.clone()))
                    .add(phantasi_sources::Column::Category.starts_with(format!("{}, ", category)))
                    .add(phantasi_sources::Column::Category.ends_with(format!(", {}", category)))
                    .add(phantasi_sources::Column::Category.contains(format!(", {}, ", category))),
            )
            .select_only()
            .column(phantasi_sources::Column::Id)
            .into_query();
        items_query =
            items_query.filter(phantasi_items::Column::SourceId.in_subquery(cat_source_ids));
    }

    // 排序
    let sort_order = query.sort_order.as_deref().unwrap_or("desc");
    items_query = phantasi_items::ordered_list_query(items_query, sort_order == "asc");
    if let Some(ref cursor) = cursor {
        items_query = phantasi_items::apply_item_cursor(items_query, sort_order == "asc", cursor);
    }

    // Extra row tells has-more. COUNT only on the first page; cursor pages skip it.
    let total = if cursor.is_some() {
        0
    } else {
        items_query
            .clone()
            .count(&db)
            .await
            .map_err(|error| phantasi_store_http("count articles", error))?
    };

    items_query = phantasi_items::preview_query(items_query);

    let fetch = per_page as u64 + 1;
    let items = if cursor.is_some() {
        items_query.limit(fetch).all(&db).await
    } else {
        items_query
            .offset((page as u64 - 1) * per_page as u64)
            .limit(fetch)
            .all(&db)
            .await
    };

    match items {
        Ok(items) => {
            let (items, next_cursor) = phantasi_items::split_list_page(items, per_page);
            let item_ids: Vec<i32> = items.iter().map(|i| i.id).collect();
            let page_source_ids: Vec<i32> = items.iter().map(|i| i.source_id).collect();

            // 性能优化：并行执行多个独立查询
            let (states_result, sources_result) = tokio::try_join!(
                async {
                    if let Some(uid) = user_id {
                        phantasi_user_states::Entity::find()
                            .filter(phantasi_user_states::Column::UserId.eq(uid))
                            .filter(phantasi_user_states::Column::ItemId.is_in(item_ids.clone()))
                            .all(&db)
                            .await
                    } else {
                        Ok(Vec::new())
                    }
                },
                phantasi_sources::Entity::find()
                    .filter(phantasi_sources::Column::Id.is_in(page_source_ids))
                    .select_only()
                    .columns([
                        phantasi_sources::Column::Id,
                        phantasi_sources::Column::Name,
                        phantasi_sources::Column::Icon,
                    ])
                    .into_tuple::<(i32, String, Option<String>)>()
                    .all(&db),
            )
            .map_err(|error| phantasi_store_http("list article extras", error))?;

            let states_map: std::collections::HashMap<i32, phantasi_user_states::Model> =
                states_result.into_iter().map(|s| (s.item_id, s)).collect();

            let mut sources_map: std::collections::HashMap<i32, (String, Option<String>)> =
                sources_result
                    .into_iter()
                    .map(|(id, name, icon)| (id, (name, icon)))
                    .collect();
            for (id, (_name, icon)) in sources_map.iter_mut() {
                let raw = icon.take();
                *icon = materialize_source_icon(&db, *id, raw).await;
            }

            // 构建响应
            let response_items: Vec<phantasi_items::ItemResponse> = items
                .into_iter()
                .map(|item| {
                    // 游客所有文章都是未读、未收藏
                    let state = states_map.get(&item.id);
                    let is_read = user_id.is_some() && state.map(|s| s.is_read).unwrap_or(false);
                    let is_starred = is_admin && state.map(|s| s.is_starred).unwrap_or(false);
                    let read_progress = if user_id.is_some() {
                        state.and_then(|s| s.read_progress)
                    } else {
                        None
                    };

                    let source = sources_map.get(&item.source_id);
                    let source_name = source.map(|s| s.0.clone());
                    let source_icon = source.and_then(|s| s.1.clone());

                    phantasi_items::ItemResponse::from_model_with_ai(
                        item,
                        source_name,
                        source_icon,
                        is_read,
                        is_starred,
                        read_progress,
                        false,
                        false,
                    )
                })
                .collect();

            let response_items = phantasi_items::list_response_items(response_items)
                .map_err(|error| phantasi_store_http("serialize articles", error))?;
            Ok(Json(json!({
                "success": true,
                "items": response_items,
                "total": total,
                "page": page,
                "per_page": per_page,
                "next_cursor": next_cursor,
            })))
        }
        Err(e) => Err(phantasi_store_http("list articles", e)),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn topic_catalog_exposes_enabled_cards() {
        let src = include_str!("feeds_list.rs");
        assert!(src.contains("FEED_TOPIC_CARDS_KEY"));
        assert!(src.contains("\"cards\": cards"));
        let put = src
            .split("pub(crate) async fn put_feed_topic_cards")
            .nth(1)
            .expect("put_feed_topic_cards");
        assert!(put.contains("_admin: AdminClaims"));
        assert!(put.contains("sanitize_feed_topic_cards"));
    }

    #[test]
    fn starred_filter_is_admin_only() {
        let src = include_str!("feeds_list.rs");
        let start = src
            .find("pub(crate) async fn list_items")
            .expect("list_items");
        let body = &src[start..];
        let end = body[1..]
            .find("\n#[cfg(test)]")
            .map(|index| index + 1)
            .unwrap_or(body.len());
        let list = &body[..end];
        assert!(list.contains("filter_type == \"starred\""));
        assert!(list.contains("require_viewer_admin(&viewer)"));
        assert!(list.contains("materialize_source_icon"));
    }
}
