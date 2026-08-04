//! Brew comments (annotations) and RSSHub instance admin.
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseBackend,
    DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect,
    Statement, Value as SeaValue,
};
use serde::Deserialize;
use serde_json::json;

use crate::error::HttpError;
use crate::models::entities::{brew_comments, brew_items, brew_sources, rsshub_instances};
use crate::services::rsshub_service::RsshubService;

use super::helpers::{
    brew_http_err, get_optional_user_id_from_headers, get_user_and_admin_status,
    get_user_id_from_headers,
};

// 用户评论（批注）

/// 获取文章的用户评论列表
/// 登录用户可以看到自己的评论
/// 性能优化：批量查询回复数量和用户信息，避免 N+1 问题
pub(crate) async fn list_comments(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(item_id): Path<i32>,
) -> Result<Json<serde_json::Value>, HttpError> {
    // 获取可选用户 ID（游客为 None）
    let user_id = get_optional_user_id_from_headers(&headers);

    // 游客无法查看评论
    let uid = match user_id {
        Some(id) => id,
        None => {
            return Ok(Json(
                json!({ "success": true, "comments": [], "has_comments": false }),
            ));
        }
    };

    // 获取用户在该文章的顶级评论（parent_id 为 NULL）
    let comments = brew_comments::Entity::find()
        .filter(brew_comments::Column::ItemId.eq(item_id))
        .filter(brew_comments::Column::UserId.eq(uid))
        .filter(brew_comments::Column::ParentId.is_null())
        .order_by_asc(brew_comments::Column::StartOffset)
        .all(&db)
        .await;

    match comments {
        Ok(comments) => {
            let has_comments = !comments.is_empty();

            if comments.is_empty() {
                return Ok(Json(
                    json!({ "success": true, "comments": [], "has_comments": false }),
                ));
            }

            // 性能优化：批量查询所有评论的回复数量
            let comment_ids: Vec<i32> = comments.iter().map(|c| c.id).collect();
            let reply_counts: std::collections::HashMap<i32, i32> = brew_comments::Entity::find()
                .filter(brew_comments::Column::ParentId.is_in(comment_ids.clone()))
                .select_only()
                .column(brew_comments::Column::ParentId)
                .column_as(brew_comments::Column::Id.count(), "count")
                .group_by(brew_comments::Column::ParentId)
                .into_tuple::<(i32, i64)>()
                .all(&db)
                .await
                .unwrap_or_default()
                .into_iter()
                .map(|(parent_id, count)| (parent_id, count as i32))
                .collect();

            // 性能优化：由于所有评论都属于同一用户，只需查询一次用户信息
            let user_info = db
                .query_one_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "SELECT username, display_name, avatar_url FROM users WHERE id = $1",
                    vec![SeaValue::Int(Some(uid))],
                ))
                .await
                .ok()
                .flatten();

            let (user_name, user_display_name, user_avatar) = user_info
                .map(|row| {
                    (
                        row.try_get::<String>("", "username").ok(),
                        row.try_get::<String>("", "display_name").ok(),
                        row.try_get::<String>("", "avatar_url").ok(),
                    )
                })
                .unwrap_or((None, None, None));

            // 构建响应
            let responses: Vec<brew_comments::CommentResponse> = comments
                .into_iter()
                .map(|comment| {
                    let mut response: brew_comments::CommentResponse = comment.clone().into();
                    response.reply_count = Some(*reply_counts.get(&comment.id).unwrap_or(&0));
                    response.user_name = user_name.clone();
                    response.user_display_name = user_display_name.clone();
                    response.user_avatar = user_avatar.clone();
                    response
                })
                .collect();

            Ok(Json(
                json!({ "success": true, "comments": responses, "has_comments": has_comments }),
            ))
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

/// 创建评论
/// 仅登录用户可用
pub(crate) async fn create_comment(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(item_id): Path<i32>,
    Json(req): Json<brew_comments::CreateCommentRequest>,
) -> Result<Json<serde_json::Value>, HttpError> {
    // 验证用户身份
    let user_id = get_user_id_from_headers(&headers, &db).await?;

    // 验证文章是否存在且对当前用户可见（admin_only 源需管理员）
    let (_, is_admin) = get_user_and_admin_status(&headers, &db).await;
    let item_visible = match brew_items::Entity::find_by_id(item_id).one(&db).await {
        Ok(Some(item)) => match brew_sources::Entity::find_by_id(item.source_id)
            .one(&db)
            .await
        {
            Ok(Some(source)) => !source.admin_only || is_admin,
            _ => false,
        },
        _ => false,
    };

    if !item_visible {
        return Err(HttpError::from((
            StatusCode::NOT_FOUND,
            Json(json!({ "success": false, "error": "Article not found" })),
        )));
    }

    // 如果是回复，验证父评论属于同一用户、同一文章；继承 color / is_public
    // when the client omits them (FE createReply only sends comment + parent_id).
    let mut inherited_color: Option<String> = None;
    let mut inherited_is_public: Option<bool> = None;
    if let Some(parent_id) = req.parent_id {
        let parent = brew_comments::Entity::find_by_id(parent_id)
            .filter(brew_comments::Column::ItemId.eq(item_id))
            .filter(brew_comments::Column::UserId.eq(user_id))
            .one(&db)
            .await
            .ok()
            .flatten();

        let Some(parent) = parent else {
            return Err(HttpError::from((
                StatusCode::NOT_FOUND,
                Json(json!({ "success": false, "error": "Parent comment not found" })),
            )));
        };
        inherited_color = parent.color.clone();
        inherited_is_public = Some(parent.is_public);
    }

    // 验证 color 格式（仅允许十六进制颜色）
    let validated_color = req
        .color
        .and_then(|c| {
            let color_regex =
                regex::Regex::new(r"^#([0-9A-Fa-f]{3}|[0-9A-Fa-f]{6}|[0-9A-Fa-f]{8})$").ok()?;
            if color_regex.is_match(&c) {
                Some(c)
            } else {
                None
            }
        })
        .or(inherited_color);

    // 验证输入长度限制
    if req.comment.len() > 2000 {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({ "success": false, "error": "Comment too long (max 2000 chars)" })),
        )));
    }
    if req.selected_text.len() > 5000 {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({ "success": false, "error": "Selected text too long (max 5000 chars)" })),
        )));
    }

    let now = Utc::now();
    let new_comment = brew_comments::ActiveModel {
        item_id: Set(item_id),
        user_id: Set(user_id),
        selected_text: Set(req.selected_text),
        comment: Set(req.comment),
        start_offset: Set(req.start_offset),
        end_offset: Set(req.end_offset),
        context_before: Set(req.context_before),
        context_after: Set(req.context_after),
        color: Set(validated_color),
        // Explicit body wins; replies inherit parent visibility when omitted.
        is_public: Set(req
            .is_public
            .or(inherited_is_public)
            .unwrap_or(false)),
        parent_id: Set(req.parent_id),
        created_at: Set(now.into()),
        updated_at: Set(now.into()),
        ..Default::default()
    };

    match new_comment.insert(&db).await {
        Ok(comment) => {
            let mut response: brew_comments::CommentResponse = comment.clone().into();

            // 查询用户信息
            if let Ok(Some(user_row)) = db
                .query_one_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "SELECT username, display_name, avatar_url FROM users WHERE id = $1",
                    vec![SeaValue::Int(Some(comment.user_id))],
                ))
                .await
            {
                response.user_name = user_row.try_get::<String>("", "username").ok();
                response.user_display_name = user_row.try_get::<String>("", "display_name").ok();
                response.user_avatar = user_row.try_get::<String>("", "avatar_url").ok();
            }

            Ok(Json(json!({ "success": true, "comment": response })))
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

/// 更新评论
/// 仅评论作者可用
pub(crate) async fn update_comment(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(comment_id): Path<i32>,
    Json(req): Json<brew_comments::UpdateCommentRequest>,
) -> Result<Json<serde_json::Value>, HttpError> {
    // 验证用户身份
    let user_id = get_user_id_from_headers(&headers, &db).await?;

    // 获取评论并验证所有权
    let comment = brew_comments::Entity::find_by_id(comment_id)
        .filter(brew_comments::Column::UserId.eq(user_id))
        .one(&db)
        .await;

    match comment {
        Ok(Some(comment)) => {
            let mut active: brew_comments::ActiveModel = comment.clone().into();

            if let Some(comment_text) = req.comment {
                if comment_text.len() > 2000 {
                    return Err(HttpError::from((
                        StatusCode::BAD_REQUEST,
                        Json(
                            json!({ "success": false, "error": "Comment too long (max 2000 chars)" }),
                        ),
                    )));
                }
                active.comment = Set(comment_text);
            }
            if let Some(color) = req.color {
                // 验证 color 格式（仅允许十六进制颜色）
                let color_regex =
                    regex::Regex::new(r"^#([0-9A-Fa-f]{3}|[0-9A-Fa-f]{6}|[0-9A-Fa-f]{8})$").ok();
                if color_regex.is_some_and(|r| r.is_match(&color)) {
                    active.color = Set(Some(color));
                }
            }
            if let Some(is_public) = req.is_public {
                active.is_public = Set(is_public);
            }
            active.updated_at = Set(Utc::now().into());

            match active.update(&db).await {
                Ok(updated) => {
                    let mut response: brew_comments::CommentResponse = updated.clone().into();

                    // 查询用户信息
                    if let Ok(Some(user_row)) = db
                        .query_one_raw(Statement::from_sql_and_values(
                            DatabaseBackend::Postgres,
                            "SELECT username, display_name, avatar_url FROM users WHERE id = $1",
                            vec![SeaValue::Int(Some(updated.user_id))],
                        ))
                        .await
                    {
                        response.user_name = user_row.try_get::<String>("", "username").ok();
                        response.user_display_name =
                            user_row.try_get::<String>("", "display_name").ok();
                        response.user_avatar = user_row.try_get::<String>("", "avatar_url").ok();
                    }

                    Ok(Json(json!({ "success": true, "comment": response })))
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
        Ok(None) => Err(HttpError::from((
            StatusCode::NOT_FOUND,
            Json(json!({ "success": false, "error": "Comment not found" })),
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

/// 删除评论
/// 仅评论作者可用
pub(crate) async fn delete_comment(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(comment_id): Path<i32>,
) -> Result<Json<serde_json::Value>, HttpError> {
    // 验证用户身份
    let user_id = get_user_id_from_headers(&headers, &db).await?;

    // 获取评论并验证所有权
    let comment = brew_comments::Entity::find_by_id(comment_id)
        .filter(brew_comments::Column::UserId.eq(user_id))
        .one(&db)
        .await;

    match comment {
        Ok(Some(_)) => {
            // Cascade nested replies first (no DB self-FK on parent_id)
            if let Err(e) = brew_comments::Entity::delete_many()
                .filter(brew_comments::Column::ParentId.eq(comment_id))
                .exec(&db)
                .await
            {
                tracing::error!(error = %e, "Failed to delete nested brew replies");
                return Err(brew_http_err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Database error",
                ));
            }
            match brew_comments::Entity::delete_by_id(comment_id)
                .exec(&db)
                .await
            {
                Ok(_) => Ok(Json(json!({ "success": true }))),
                Err(e) => {
                    tracing::error!(error = %e, "Database error");
                    Err(brew_http_err(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Database error",
                    ))
                }
            }
        }
        Ok(None) => Err(HttpError::from((
            StatusCode::NOT_FOUND,
            Json(json!({ "success": false, "error": "Comment not found" })),
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

/// 获取评论的回复列表
/// 性能优化：由于所有回复都属于同一用户，只查询一次用户信息
pub(crate) async fn list_comment_replies(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(comment_id): Path<i32>,
) -> Result<Json<serde_json::Value>, HttpError> {
    // 验证用户身份（可选，用于获取用户 ID）
    let uid = get_optional_user_id_from_headers(&headers);

    // 如果未登录，返回空列表
    let uid = match uid {
        Some(id) => id,
        None => {
            return Ok(Json(json!({ "success": true, "replies": [] })));
        }
    };

    // 获取评论的回复（属于当前用户的）
    let replies = brew_comments::Entity::find()
        .filter(brew_comments::Column::ParentId.eq(comment_id))
        .filter(brew_comments::Column::UserId.eq(uid))
        .order_by_asc(brew_comments::Column::CreatedAt)
        .all(&db)
        .await;

    match replies {
        Ok(replies) => {
            if replies.is_empty() {
                return Ok(Json(json!({ "success": true, "replies": [] })));
            }

            // 性能优化：由于所有回复都属于同一用户，只需查询一次用户信息
            let user_info = db
                .query_one_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "SELECT username, display_name, avatar_url FROM users WHERE id = $1",
                    vec![SeaValue::Int(Some(uid))],
                ))
                .await
                .ok()
                .flatten();

            let (user_name, user_display_name, user_avatar) = user_info
                .map(|row| {
                    (
                        row.try_get::<String>("", "username").ok(),
                        row.try_get::<String>("", "display_name").ok(),
                        row.try_get::<String>("", "avatar_url").ok(),
                    )
                })
                .unwrap_or((None, None, None));

            // 构建响应
            let responses: Vec<brew_comments::CommentResponse> = replies
                .into_iter()
                .map(|reply| {
                    let mut response: brew_comments::CommentResponse = reply.into();
                    response.user_name = user_name.clone();
                    response.user_display_name = user_display_name.clone();
                    response.user_avatar = user_avatar.clone();
                    response
                })
                .collect();

            Ok(Json(json!({ "success": true, "replies": responses })))
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

// RSSHub 实例管理

/// 获取 RSSHub 实例列表
pub(crate) async fn list_rsshub_instances(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
) -> Result<Json<serde_json::Value>, HttpError> {
    // 验证用户身份
    let user_id = get_user_id_from_headers(&headers, &db).await?;

    let rsshub_service = RsshubService::new(db);

    // 确保默认实例存在
    if let Err(e) = rsshub_service.ensure_default_instances().await {
        tracing::warn!("[RSSHub] Failed to ensure default instances: {}", e);
    }

    match rsshub_service.get_instances(Some(user_id)).await {
        Ok(instances) => {
            let responses: Vec<rsshub_instances::InstanceResponse> =
                instances.into_iter().map(|i| i.into()).collect();

            Ok(Json(json!({ "success": true, "instances": responses })))
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

/// 添加 RSSHub 实例
#[derive(Deserialize)]
pub(crate) struct AddRsshubInstanceRequest {
    name: String,
    url: String,
    access_key: Option<String>,
    priority: Option<i32>,
}

pub(crate) async fn add_rsshub_instance(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Json(req): Json<AddRsshubInstanceRequest>,
) -> Result<Json<serde_json::Value>, HttpError> {
    // 验证用户身份
    let user_id = get_user_id_from_headers(&headers, &db).await?;
    let (_, is_admin) = get_user_and_admin_status(&headers, &db).await;

    let rsshub_service = RsshubService::new(db);

    match rsshub_service
        .add_instance(
            Some(user_id),
            req.name,
            req.url,
            req.access_key,
            req.priority,
            is_admin,
        )
        .await
    {
        Ok(instance) => {
            let response: rsshub_instances::InstanceResponse = instance.into();
            Ok(Json(json!({ "success": true, "instance": response })))
        }
        Err(e) => Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({ "success": false, "error": e })),
        ))),
    }
}

/// 更新 RSSHub 实例
#[derive(Deserialize)]
pub(crate) struct UpdateRsshubInstanceRequest {
    name: Option<String>,
    url: Option<String>,
    access_key: Option<String>,
    priority: Option<i32>,
    enabled: Option<bool>,
}

pub(crate) async fn update_rsshub_instance(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(id): Path<i32>,
    Json(req): Json<UpdateRsshubInstanceRequest>,
) -> Result<Json<serde_json::Value>, HttpError> {
    // 验证用户身份
    let user_id = get_user_id_from_headers(&headers, &db).await?;
    let (_, is_admin) = get_user_and_admin_status(&headers, &db).await;

    let rsshub_service = RsshubService::new(db);

    match rsshub_service
        .update_instance(
            id,
            Some(user_id),
            req.name,
            req.url,
            req.access_key,
            req.priority,
            req.enabled,
            is_admin,
        )
        .await
    {
        Ok(instance) => {
            let response: rsshub_instances::InstanceResponse = instance.into();
            Ok(Json(json!({ "success": true, "instance": response })))
        }
        Err(e) => Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({ "success": false, "error": e })),
        ))),
    }
}

/// 删除 RSSHub 实例
pub(crate) async fn delete_rsshub_instance(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(id): Path<i32>,
) -> Result<Json<serde_json::Value>, HttpError> {
    // 验证用户身份
    let user_id = get_user_id_from_headers(&headers, &db).await?;
    let (_, is_admin) = get_user_and_admin_status(&headers, &db).await;

    let rsshub_service = RsshubService::new(db);

    match rsshub_service
        .delete_instance(id, Some(user_id), is_admin)
        .await
    {
        Ok(()) => Ok(Json(json!({ "success": true }))),
        Err(e) => Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({ "success": false, "error": e })),
        ))),
    }
}

/// 对单个实例执行健康检查
pub(crate) async fn health_check_rsshub_instance(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(id): Path<i32>,
) -> Result<Json<serde_json::Value>, HttpError> {
    // 验证用户身份
    let user_id = get_user_id_from_headers(&headers, &db).await?;

    let rsshub_service = RsshubService::new(db.clone());

    // 获取实例
    let instance = match rsshub_instances::Entity::find_by_id(id).one(&db).await {
        Ok(Some(i)) => i,
        Ok(None) => {
            return Err(HttpError::from((
                StatusCode::NOT_FOUND,
                Json(json!({ "success": false, "error": "Instance not found" })),
            )))
        }
        Err(e) => {
            tracing::error!(error = %e, "Database error");
            return Err(brew_http_err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Database error",
            ));
        }
    };

    // 检查权限
    if instance.user_id != Some(user_id) && instance.user_id.is_some() {
        return Err(HttpError::from((
            StatusCode::FORBIDDEN,
            Json(json!({ "success": false, "error": "Permission denied" })),
        )));
    }

    match rsshub_service.health_check(&instance).await {
        Ok(response_time) => Ok(Json(json!({
            "success": true,
            "healthy": true,
            "response_time_ms": response_time
        }))),
        Err(e) => Ok(Json(json!({
            "success": true,
            "healthy": false,
            "error": e
        }))),
    }
}

/// 重置实例统计
pub(crate) async fn reset_rsshub_instance(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(id): Path<i32>,
) -> Result<Json<serde_json::Value>, HttpError> {
    // 验证用户身份
    let user_id = get_user_id_from_headers(&headers, &db).await?;
    let (_, is_admin) = get_user_and_admin_status(&headers, &db).await;

    let rsshub_service = RsshubService::new(db);

    match rsshub_service
        .reset_instance_stats(id, Some(user_id), is_admin)
        .await
    {
        Ok(()) => Ok(Json(json!({ "success": true }))),
        Err(e) => Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({ "success": false, "error": e })),
        ))),
    }
}

/// 对所有实例执行健康检查
pub(crate) async fn health_check_all_rsshub_instances(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
) -> Result<Json<serde_json::Value>, HttpError> {
    // 验证用户身份
    let user_id = get_user_id_from_headers(&headers, &db).await?;

    let rsshub_service = RsshubService::new(db);

    match rsshub_service.check_all_instances(Some(user_id)).await {
        Ok(()) => Ok(Json(
            json!({ "success": true, "message": "Health check completed" }),
        )),
        Err(e) => {
            tracing::error!(error = %e, "Database error");
            Err(brew_http_err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Database error",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::helpers::build_feed_discovery_candidates;

    #[test]
    fn feed_discovery_candidates_cover_root_and_nested_paths() {
        let candidates = build_feed_discovery_candidates("example.com/blog").unwrap();

        assert_eq!(candidates.first().unwrap(), "https://example.com/blog");
        assert!(candidates.contains(&"https://example.com/blog/feed".to_string()));
        assert!(candidates.contains(&"https://example.com/rss.xml".to_string()));
    }

    #[test]
    fn feed_discovery_keeps_direct_feed_first_and_rejects_other_schemes() {
        let candidates = build_feed_discovery_candidates("https://example.com/feed.xml").unwrap();
        assert_eq!(candidates.first().unwrap(), "https://example.com/feed.xml");

        assert!(build_feed_discovery_candidates("ftp://example.com/feed.xml").is_err());
    }

    #[test]
    fn feed_discovery_rejects_empty_url() {
        assert!(build_feed_discovery_candidates("").is_err());
        assert!(build_feed_discovery_candidates("   ").is_err());
    }
}
