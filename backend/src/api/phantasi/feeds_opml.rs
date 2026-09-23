//! Phantasi OPML import and export.
use crate::error::HttpError;
use crate::extract::{AdminClaims, OptionalViewer};
use myriad_error::AppError;

use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use chrono::Utc;
use sea_orm::{
    ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder,
};
use serde::Deserialize;
use serde_json::json;

use crate::models::entities::phantasi_sources;

use super::helpers::{
    admin_user_id, generate_opml, get_phantasi_viewer, parse_opml, phantasi_store_http,
};

// OPML 导入导出

/// 导入 OPML
#[derive(Debug, Deserialize)]
pub struct ImportOpmlRequest {
    opml: String,
}

pub(crate) async fn import_opml(
    State(db): State<DatabaseConnection>,
    admin: AdminClaims,
    Json(req): Json<ImportOpmlRequest>,
) -> Result<Json<serde_json::Value>, HttpError> {
    // 导入 OPML 需要管理员权限
    let user_id = admin_user_id(&admin)?;

    // 解析 OPML
    let feeds = parse_opml(&req.opml);
    if feeds.is_empty() {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(AppError::fail_json("No feeds found in OPML")),
        )));
    }

    // 批量查询已存在的 URL（避免 N+1）
    let feed_urls: Vec<String> = feeds.iter().map(|f| f.url.clone()).collect();
    let existing_urls: std::collections::HashSet<String> = phantasi_sources::Entity::find()
        .filter(phantasi_sources::Column::Url.is_in(&feed_urls))
        .all(&db)
        .await
        .map_err(|error| phantasi_store_http("find existing sources", error))?
        .into_iter()
        .map(|s| s.url)
        .collect();

    let now = Utc::now();

    // 收集需要插入的新订阅源
    let new_sources: Vec<phantasi_sources::ActiveModel> = feeds
        .into_iter()
        .filter(|feed| !existing_urls.contains(&feed.url))
        .map(|feed| {
            let mut source = phantasi_sources::ActiveModel {
                user_id: Set(user_id),
                name: Set(feed.title),
                url: Set(feed.url),
                feed_type: Set(phantasi_sources::FeedType::Rss),
                category: Set(feed.category),
                site_url: Set(feed.site_url),
                enabled: Set(true),
                error_count: Set(0),
                item_count: Set(0),
                update_interval: Set(30),
                created_at: Set(now.into()),
                updated_at: Set(now.into()),
                ..Default::default()
            };
            // insert_many 不经过 before_save 钩子
            source.sync_url_keys();
            source
        })
        .collect();

    let imported = new_sources.len();
    let skipped = feed_urls.len() - imported;

    // 批量插入新订阅源
    if !new_sources.is_empty() {
        if let Err(e) = phantasi_sources::Entity::insert_many(new_sources)
            .exec(&db)
            .await
        {
            return Err(phantasi_store_http("import sources", e));
        }
    }

    Ok(Json(json!({
        "success": true,
        "imported": imported,
        "skipped": skipped,
    })))
}

/// 导出 OPML（公开读：关访客门 404；坏凭据 401；非管理员不导出 admin_only 源）
pub(crate) async fn export_opml(
    State(db): State<DatabaseConnection>,
    viewer: OptionalViewer,
) -> Result<impl IntoResponse, HttpError> {
    let (_, is_admin) = get_phantasi_viewer(&viewer, &db).await?;

    let mut query = phantasi_sources::Entity::find()
        // 笔记源的 url 是 `myriad:notes`，不是一个可订阅的 feed。导出来别人
        // 导进去只会得到一个永远抓不动的源。
        .filter(phantasi_sources::Column::SourceType.ne(phantasi_sources::SourceType::Note))
        .order_by_asc(phantasi_sources::Column::Category)
        .order_by_asc(phantasi_sources::Column::Name);

    if !is_admin {
        query = query.filter(phantasi_sources::Column::AdminOnly.eq(false));
    }

    let sources = query.all(&db).await;

    match sources {
        Ok(sources) => {
            let opml = generate_opml(&sources);
            Ok((StatusCode::OK, [("Content-Type", "application/xml")], opml))
        }
        Err(e) => Err(phantasi_store_http("export sources", e)),
    }
}
