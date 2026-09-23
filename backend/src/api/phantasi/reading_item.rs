//! Phantasi item detail.
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter};
use serde_json::json;

use crate::error::HttpError;
use crate::extract::OptionalViewer;
use crate::models::entities::{
    phantasi_annotations, phantasi_items, phantasi_podcasts, phantasi_sources, phantasi_user_states,
};

use super::helpers::{get_phantasi_viewer, materialize_source_icon, phantasi_store_http};
use myriad_error::AppError;

/// 获取单篇文章详情（游客可访问）
/// 游客不查询已读/收藏状态以节约计算
/// 性能优化：并行查询 AI 状态
/// admin_only 源下的文章仅管理员可见
pub(crate) async fn get_item(
    State(db): State<DatabaseConnection>,
    viewer: OptionalViewer,
    Path(id): Path<i32>,
) -> Result<Json<serde_json::Value>, HttpError> {
    // 获取可选用户 ID 与管理员状态
    let (user_id, is_admin) = get_phantasi_viewer(&viewer, &db).await?;

    // 获取文章
    let item = phantasi_items::Entity::find_by_id(id)
        .one(&db)
        .await
        .map_err(|e| phantasi_store_http("find article", e))?;

    match item {
        Some(item) => {
            let source_id = item.source_id;
            let (source, state) = tokio::join!(
                phantasi_sources::Entity::find_by_id(source_id).one(&db),
                async {
                    let Some(uid) = user_id else {
                        return Ok(None);
                    };
                    phantasi_user_states::Entity::find()
                        .filter(phantasi_user_states::Column::UserId.eq(uid))
                        .filter(phantasi_user_states::Column::ItemId.eq(id))
                        .one(&db)
                        .await
                }
            );

            let source =
                source.map_err(|error| phantasi_store_http("find article source", error))?;
            if let Some(source) = source {
                if source.admin_only && !is_admin {
                    return Err(HttpError::from((
                        StatusCode::NOT_FOUND,
                        Json(AppError::fail_json("Item not found")),
                    )));
                }
                let state = state
                    .map_err(|error| phantasi_store_http("load article reading state", error))?;
                let (is_read, is_starred, read_progress, state_revision) = if user_id.is_some() {
                    (
                        state.as_ref().map(|s| s.is_read).unwrap_or(false),
                        is_admin && state.as_ref().map(|s| s.is_starred).unwrap_or(false),
                        state.as_ref().and_then(|s| s.read_progress),
                        Some(state.as_ref().map_or(0, |s| s.revision)),
                    )
                } else {
                    (false, false, None, None)
                };

                let note = source.source_type == phantasi_sources::SourceType::Note;
                let (annotations_count, podcast_count) = if note {
                    (0, 0)
                } else {
                    tokio::try_join!(
                        phantasi_annotations::Entity::find()
                            .filter(phantasi_annotations::Column::ItemId.eq(id))
                            .count(&db),
                        phantasi_podcasts::Entity::find()
                            .filter(phantasi_podcasts::Column::ItemId.eq(id))
                            .count(&db)
                    )
                    .map_err(|error| phantasi_store_http("count article extras", error))?
                };

                let has_ai_annotations = annotations_count > 0;
                let has_ai_podcast = podcast_count > 0;

                let source_icon =
                    materialize_source_icon(&db, source.id, source.icon.clone()).await;
                let mut response = phantasi_items::ItemResponse::from_model_with_ai(
                    item,
                    Some(source.name),
                    source_icon,
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
        None => Err(HttpError::from((
            StatusCode::NOT_FOUND,
            Json(AppError::fail_json("Item not found")),
        ))),
    }
}
