//! 笔记：站长自己写的 Markdown 内容。
//!
//! 笔记不是第二套文章系统 —— 它就是 `phantasi_items` 里的一条，挂在一个
//! `source_type = note` 的本地源下面。这样阅读器、评论、AI 注释、播客、收藏、
//! 已读、sitemap、联邦投递和 Agent 的 phantasi 技能全部零改动生效。
//!
//! 与抓来的文章只有两点不同：
//! - `content_md` 有值（原文），`content` 是它渲染并消毒之后的 HTML
//! - `guid` 是平台生成的 `note:<uuid>`，没有上游 feed
//!
//! 渲染只在写入这一侧发生。读路径永远读 `content`，绝不在渲染一次 ——
//! 否则阅读器、RSS、联邦三处会各自拿到一份不同的 HTML。
//! 对外订阅走 `GET /journal/notes.xml`（`/api/phantasi/notes.xml` 同一份）。
//! 默认关；站长在工作台打开，且 Phantasi 对访客开放，地址才存在。

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use myriad_phantasi_notes::{render_markdown_preview, validate_note};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QuerySelect};
use serde::Deserialize;
use serde_json::json;

use super::helpers::{admin_user_id, phantasi_http_err, phantasi_store_http};
use crate::error::HttpError;
use crate::extract::AdminClaims;
use crate::models::entities::{phantasi_items, phantasi_sources};

/// 笔记落在「我」分类下 —— 这是站内唯一可做文章级 SEO 的分类。
/// 直接引用 `api::seo` 的那份取值，不另抄一个字面量。
use crate::api::seo::PHANTASI_MINE_CATEGORY as NOTE_SOURCE_CATEGORY;

#[derive(Debug, Deserialize)]
pub(crate) struct NoteWriteRequest {
    pub title: String,
    /// Markdown 原文。字段名与 `phantasi_items.content_md` 同名。
    #[serde(default)]
    pub content_md: String,
    /// 主题字符串。留空则存 NULL。
    #[serde(default)]
    pub topic: Option<String>,
    /// 封面。不给就用正文里第一张图。
    #[serde(default)]
    pub image: Option<String>,
    /// 发布时间（毫秒）。不给就用当前时间；改稿时不给则保持原值。
    #[serde(default)]
    pub published_at: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct NotePreviewRequest {
    #[serde(default)]
    pub content_md: String,
}

fn validation_err(err: myriad_phantasi_notes::NoteError) -> HttpError {
    phantasi_http_err(StatusCode::BAD_REQUEST, err.message())
}

/// 校验这条 item 是共享目录里的笔记。
///
/// 必须是笔记源。少了这道，这些接口就变成了「可以改任何抓来的文章」的后门。
/// 不按源创建者过滤：第二管理员也能改。
async fn find_catalog_note(
    db: &DatabaseConnection,
    item_id: i32,
) -> Result<phantasi_sources::Model, HttpError> {
    let source_id = phantasi_items::Entity::find_by_id(item_id)
        .select_only()
        .column(phantasi_items::Column::SourceId)
        .into_tuple::<i32>()
        .one(db)
        .await
        .map_err(|e| phantasi_store_http("find note", e))?
        .ok_or_else(|| phantasi_http_err(StatusCode::NOT_FOUND, "Note not found"))?;

    let source = phantasi_sources::Entity::find_by_id(source_id)
        .filter(phantasi_sources::Column::SourceType.eq(phantasi_sources::SourceType::Note))
        .one(db)
        .await
        .map_err(|e| phantasi_store_http("find note source", e))?
        .ok_or_else(|| phantasi_http_err(StatusCode::NOT_FOUND, "Note not found"))?;

    Ok(source)
}

/// `POST /api/phantasi/notes/preview` — 编辑器预览。
///
/// 预览调 `render_markdown_preview`：和发布同一份 HTML，只多了每个顶层块的原文区间
/// （`data-md-start/end`），前端靠它做「点预览即编辑」。发布调 `render_note`。前端不自己渲染。
pub(crate) async fn preview_note(
    _admin: AdminClaims,
    Json(req): Json<NotePreviewRequest>,
) -> Result<Json<serde_json::Value>, HttpError> {
    validate_note("Preview", &req.content_md).map_err(validation_err)?;
    Ok(Json(json!({
        "success": true,
        "html": render_markdown_preview(&req.content_md),
    })))
}

/// `POST /api/phantasi/notes` — 写一篇笔记。
pub(crate) async fn create_note(
    State(db): State<DatabaseConnection>,
    admin: AdminClaims,
    Json(req): Json<NoteWriteRequest>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let user_id = admin_user_id(&admin)?;
    let item = crate::services::note_publish::write_note_with_doc(
        &db,
        user_id,
        None,
        &req.title,
        &req.content_md,
        req.topic.clone(),
        req.image.clone(),
        req.published_at,
    )
    .await?;

    Ok(Json(json!({
        "success": true,
        "id": item.id,
        "link": item.link,
    })))
}

/// `PUT /api/phantasi/notes/{id}` — 改一篇笔记。
pub(crate) async fn update_note(
    State(db): State<DatabaseConnection>,
    admin: AdminClaims,
    Path(id): Path<i32>,
    Json(req): Json<NoteWriteRequest>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let user_id = admin_user_id(&admin)?;
    find_catalog_note(&db, id).await?;
    let item = crate::services::note_publish::write_note_with_doc(
        &db,
        user_id,
        Some(id),
        &req.title,
        &req.content_md,
        req.topic.clone(),
        req.image.clone(),
        req.published_at,
    )
    .await?;

    Ok(Json(json!({
        "success": true,
        "id": item.id,
        "link": item.link,
    })))
}

/// `DELETE /api/phantasi/notes/{id}` — 删一篇笔记。
pub(crate) async fn delete_note(
    State(db): State<DatabaseConnection>,
    _admin: AdminClaims,
    Path(id): Path<i32>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let source = find_catalog_note(&db, id).await?;

    crate::services::note_publish::delete_note_with_doc(&db, id, &source).await?;

    Ok(Json(json!({ "success": true })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::note_publish::{NOTE_SOURCE_NAME, NOTE_SOURCE_URL, millis_to_datetime};

    #[test]
    fn note_source_lands_in_the_own_content_category() {
        // 笔记必须落在「我」分类下，否则 api::seo 不会把它当自有内容收录。
        // 笔记板块按 `source_type = note` 取。
        assert_eq!(NOTE_SOURCE_CATEGORY, "我");
    }

    #[test]
    fn note_source_url_is_not_fetchable() {
        // 非 http(s)：即便某天有人漏掉了 source_type 判断，抓取也发不出请求
        assert!(!NOTE_SOURCE_NAME.is_empty());
        assert!(!NOTE_SOURCE_URL.starts_with("http"));
    }

    #[test]
    fn millis_round_trip() {
        let ms = 1_700_000_000_000;
        assert_eq!(millis_to_datetime(ms).unwrap().timestamp_millis(), ms);
    }

    #[test]
    fn absurd_millis_are_rejected_rather_than_panicking() {
        assert!(millis_to_datetime(i64::MAX).is_none());
    }

    #[test]
    fn catalog_note_mutations_are_not_keyed_by_source_owner() {
        let src = include_str!("notes.rs");
        let start = src
            .find("async fn find_catalog_note")
            .expect("find_catalog_note");
        let body = &src[start..];
        let finder = body.split_once("\n}").expect("end of catalog finder").0;
        assert!(finder.contains("SourceType::Note"));
        assert!(
            !finder.contains("UserId.eq"),
            "shared catalog notes are not keyed by the creating admin"
        );
        for name in ["update_note", "delete_note"] {
            assert!(src.contains("find_catalog_note(&db"));
            let start = src
                .find(&format!("pub(crate) async fn {name}"))
                .unwrap_or_else(|| panic!("{name}"));
            let body = &src[start..];
            let end = body[1..]
                .find("\npub(crate) async fn ")
                .or_else(|| body[1..].find("\n#[cfg(test)]"))
                .map(|index| index + 1)
                .unwrap_or(body.len());
            assert!(
                !&body[..end].contains("UserId.eq(user_id)"),
                "{name} must not look up the creating admin"
            );
        }
    }

    #[test]
    fn public_item_list_does_not_read_note_docs() {
        let src = include_str!("feeds_list.rs");
        let start = src
            .find("pub(crate) async fn list_items")
            .expect("list_items");
        let body = &src[start..];
        let end = body[1..]
            .find("\npub(crate) async fn ")
            .map(|index| index + 1)
            .unwrap_or(body.len());
        let list = &body[..end];
        assert!(list.contains("phantasi_items"));
        assert!(
            !list.contains("phantasi_note_docs"),
            "drafts must not leak into the public item list"
        );
        assert!(list.contains("SourceId.in_subquery(visible_source_ids)"));
        assert!(list.contains("Id.in_subquery(starred_subquery)"));
        assert!(list.contains("not_in_subquery(read_subquery)"));
        assert!(list.contains("in_subquery(cat_source_ids)"));
        assert!(list.contains("page_source_ids"));
        assert!(
            !list.contains("cat_sources"),
            "category filter must stay a subquery, not materialize source IDs"
        );
        assert!(
            !list.contains("visible_sources.all"),
            "visibility must not materialize every source id"
        );
    }

    #[test]
    fn list_sources_unread_uses_per_user_sql_not_cached_column() {
        let src = include_str!("feeds_sources.rs");
        let start = src
            .find("pub(crate) async fn list_sources")
            .expect("list_sources");
        let body = &src[start..];
        let end = body[1..]
            .find("\npub(crate) async fn ")
            .map(|index| index + 1)
            .unwrap_or(body.len());
        let list = &body[..end];
        assert!(list.contains("s.is_read = TRUE"));
        assert!(list.contains("real_unread_count"));
        assert!(
            !list.contains("response.unread_count = source.unread_count"),
            "list_sources unread must not reuse the cached source column"
        );
    }
}
