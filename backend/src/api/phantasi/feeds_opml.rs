//! Phantasi OPML import and export.
use crate::error::HttpError;
use crate::extract::{AdminClaims, OptionalViewer};
use myriad_error::AppError;

use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use sea_orm::{
    ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder,
    TransactionTrait,
};
use serde::Deserialize;
use serde_json::json;

use crate::models::entities::phantasi_sources;
use crate::services::phantasi_subscribe::{NewSource, SourceCreation, create_or_find_source};

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

    // 逐条走共享的创建入口：与手动添加、友链审核、Agent 订阅同一去重规则与锁；
    // 同一份 OPML 里的重复项在同一事务里也只收一次。整份导入同一事务，失败全回滚。
    let total = feeds.len();
    let now = crate::clock::now();
    let imported = async {
        let txn = db.begin().await?;
        let mut imported = 0usize;
        for feed in feeds {
            let source = phantasi_sources::ActiveModel {
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
            if let SourceCreation::Created(_) =
                create_or_find_source(&txn, NewSource::new(source)).await?
            {
                imported += 1;
            }
        }
        txn.commit().await?;
        Ok::<_, sea_orm::DbErr>(imported)
    }
    .await
    .map_err(|error| phantasi_store_http("import sources", error))?;
    let skipped = total - imported;

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
