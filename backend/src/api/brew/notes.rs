//! 手记：站长自己写的 Markdown 内容。
//!
//! 手记不是第二套文章系统 —— 它就是 `brew_items` 里的一条，挂在一个
//! `source_type = note` 的本地源下面。这样阅读器、评论、AI 注释、播客、收藏、
//! 已读、sitemap、联邦投递和 Agent 的 brew 技能全部零改动生效。
//!
//! 与抓来的文章只有两点不同：
//! - `content_md` 有值（原文），`content` 是它渲染并消毒之后的 HTML
//! - `guid` 是平台生成的 `note:<uuid>`，没有上游 feed
//!
//! 渲染只在写入这一侧发生。读路径永远读 `content`，绝不在渲染一次 ——
//! 否则阅读器、RSS、联邦三处会各自拿到一份不同的 HTML。

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::{TimeZone, Utc};
use myriad_brew_notes::{note_guid, note_link, render_markdown, render_note, validate_note};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter,
    Set,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::helpers::{brew_http_err, brew_store_http, get_admin_user_id_from_headers};
use crate::error::HttpError;
use crate::models::entities::{brew_items, brew_sources};

/// 手记源的固定名字。和分类「我」一样是**数据值**而不是界面文案 ——
/// 站长可以像改任何订阅源一样把它改掉，改了也不影响这里的查找（按
/// `source_type` 找，不按名字找）。
const NOTE_SOURCE_NAME: &str = "手记";

/// 手记源的 URL。非 http 协议，任何抓取路径看到它都会绕开。
const NOTE_SOURCE_URL: &str = "myriad:notes";

/// 手记落在「我」分类下 —— 这是站内唯一可做文章级 SEO 的分类。
/// 直接引用 `api::seo` 的那份取值，不另抄一个字面量。
use crate::api::seo::BREW_MINE_CATEGORY as NOTE_SOURCE_CATEGORY;

#[derive(Debug, Deserialize)]
pub(crate) struct NoteWriteRequest {
    pub title: String,
    /// Markdown 原文。字段名与 `brew_items.content_md` 同名。
    #[serde(default)]
    pub content_md: String,
    /// 预定义主题 key。留空表示不参与主题聚类。
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

/// 编辑器要读回的那份原文。比 `ItemResponse` 多一个 `content_md`，
/// 少掉所有阅读态字段 —— 这个响应只服务编辑器。
#[derive(Debug, Serialize)]
pub(crate) struct NoteDraftResponse {
    pub id: i32,
    pub title: String,
    pub content_md: String,
    pub topic: Option<String>,
    pub image: Option<String>,
    pub published_at: i64,
}

fn validation_err(err: myriad_brew_notes::NoteError) -> HttpError {
    brew_http_err(StatusCode::BAD_REQUEST, err.message())
}

/// 找到（必要时创建）该站长的手记源。
///
/// 按 `source_type` 查，不按名字或 URL 查 —— 站长改了名字之后仍要找得到同一个源。
/// 一个用户只有一个手记源；真出现多个（手工改库）时取 id 最小的那个，
/// 不去合并，也不报错。
async fn ensure_note_source(
    db: &DatabaseConnection,
    user_id: i32,
) -> Result<brew_sources::Model, HttpError> {
    let existing = brew_sources::Entity::find()
        .filter(brew_sources::Column::UserId.eq(user_id))
        .filter(brew_sources::Column::SourceType.eq(brew_sources::SourceType::Note))
        .one(db)
        .await
        .map_err(|e| brew_store_http("find note source", e))?;

    if let Some(source) = existing {
        return Ok(source);
    }

    let now = Utc::now();
    let source = brew_sources::ActiveModel {
        user_id: Set(user_id),
        name: Set(NOTE_SOURCE_NAME.to_string()),
        url: Set(NOTE_SOURCE_URL.to_string()),
        feed_type: Set(brew_sources::FeedType::Rss),
        source_type: Set(brew_sources::SourceType::Note),
        category: Set(Some(NOTE_SOURCE_CATEGORY.to_string())),
        // 不抓取：调度器按 source_type 就会绕开，间隔置 0 只是让它在界面上
        // 也读得出「这个源不更新」
        update_interval: Set(0),
        enabled: Set(true),
        error_count: Set(0),
        item_count: Set(0),
        unread_count: Set(0),
        admin_only: Set(false),
        created_at: Set(now.into()),
        updated_at: Set(now.into()),
        ..Default::default()
    };

    source
        .insert(db)
        .await
        .map_err(|e| brew_store_http("create note source", e))
}

/// 校验这条 item 确实是该站长的手记。
///
/// 两道关都要过：源属于当前用户，且源确实是手记源。少了第二道，这些接口就
/// 变成了「可以改任何抓来的文章」的后门。
async fn find_own_note(
    db: &DatabaseConnection,
    user_id: i32,
    item_id: i32,
) -> Result<(brew_items::Model, brew_sources::Model), HttpError> {
    let item = brew_items::Entity::find_by_id(item_id)
        .one(db)
        .await
        .map_err(|e| brew_store_http("find note", e))?
        .ok_or_else(|| brew_http_err(StatusCode::NOT_FOUND, "Note not found"))?;

    let source = brew_sources::Entity::find_by_id(item.source_id)
        .filter(brew_sources::Column::UserId.eq(user_id))
        .filter(brew_sources::Column::SourceType.eq(brew_sources::SourceType::Note))
        .one(db)
        .await
        .map_err(|e| brew_store_http("find note source", e))?
        .ok_or_else(|| brew_http_err(StatusCode::NOT_FOUND, "Note not found"))?;

    Ok((item, source))
}

fn millis_to_datetime(ms: i64) -> Option<chrono::DateTime<Utc>> {
    Utc.timestamp_millis_opt(ms).single()
}

/// 维护源上的条目计数缓存。手记不走抓取路径，没人替它更新这个数。
async fn sync_item_count(db: &DatabaseConnection, source: &brew_sources::Model) {
    let count = brew_items::Entity::find()
        .filter(brew_items::Column::SourceId.eq(source.id))
        .count(db)
        .await
        .unwrap_or(0);
    let mut active: brew_sources::ActiveModel = source.clone().into();
    active.item_count = Set(i32::try_from(count).unwrap_or(i32::MAX));
    active.updated_at = Set(Utc::now().into());
    if let Err(error) = active.update(db).await {
        tracing::warn!(%error, source_id = source.id, "failed to sync note item count");
    }
}

/// `POST /api/brew/notes/preview` — 编辑器预览。
///
/// 预览走的是和发布**同一个**渲染函数。前端不自己渲染 Markdown，就不存在
/// 「预览好看、发出去变形」这种问题。
pub(crate) async fn preview_note(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Json(req): Json<NotePreviewRequest>,
) -> Result<Json<serde_json::Value>, HttpError> {
    get_admin_user_id_from_headers(&headers, &db).await?;
    validate_note("预览", &req.content_md).map_err(validation_err)?;
    Ok(Json(json!({
        "success": true,
        "html": render_markdown(&req.content_md),
    })))
}

/// `GET /api/brew/notes/{id}` — 取回原文供编辑。
pub(crate) async fn get_note_draft(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(id): Path<i32>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let user_id = get_admin_user_id_from_headers(&headers, &db).await?;
    let (item, _) = find_own_note(&db, user_id, id).await?;

    Ok(Json(json!({
        "success": true,
        "note": NoteDraftResponse {
            id: item.id,
            title: item.title,
            content_md: item.content_md.unwrap_or_default(),
            topic: item.topic,
            image: item.image,
            published_at: item.published_at.timestamp_millis(),
        },
    })))
}

/// `POST /api/brew/notes` — 写一篇手记。
pub(crate) async fn create_note(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Json(req): Json<NoteWriteRequest>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let user_id = get_admin_user_id_from_headers(&headers, &db).await?;
    validate_note(&req.title, &req.content_md).map_err(validation_err)?;

    let source = ensure_note_source(&db, user_id).await?;
    let rendered = render_note(&req.title, &req.content_md);
    let now = Utc::now();
    let published_at = req
        .published_at
        .and_then(millis_to_datetime)
        .unwrap_or(now);

    let new_item = brew_items::ActiveModel {
        source_id: Set(source.id),
        // guid 先落一个 uuid；`link` 需要 id，插入后再补
        guid: Set(note_guid(&uuid::Uuid::new_v4().to_string())),
        title: Set(rendered.title),
        link: Set(String::new()),
        summary: Set(rendered.summary),
        content: Set(Some(rendered.html)),
        content_md: Set(Some(req.content_md.clone())),
        image: Set(req.image.clone().or(rendered.image)),
        published_at: Set(published_at.into()),
        fetched_at: Set(now.into()),
        word_count: Set(Some(rendered.word_count)),
        reading_time: Set(Some(rendered.reading_time)),
        // 手记就是全文，没有「再去抓一次原文」这回事
        fulltext_fetched: Set(true),
        topic: Set(req.topic.clone().filter(|t| !t.trim().is_empty())),
        ..Default::default()
    };

    let item = new_item
        .insert(&db)
        .await
        .map_err(|e| brew_store_http("save note", e))?;

    // link 指向站内规范路径，需要 id 才能拼出来
    let item_id = item.id;
    let mut active: brew_items::ActiveModel = item.into();
    active.link = Set(note_link(item_id));
    let item = active
        .update(&db)
        .await
        .map_err(|e| brew_store_http("save note", e))?;

    sync_item_count(&db, &source).await;

    Ok(Json(json!({
        "success": true,
        "id": item.id,
        "link": item.link,
    })))
}

/// `PUT /api/brew/notes/{id}` — 改一篇手记。
pub(crate) async fn update_note(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(id): Path<i32>,
    Json(req): Json<NoteWriteRequest>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let user_id = get_admin_user_id_from_headers(&headers, &db).await?;
    validate_note(&req.title, &req.content_md).map_err(validation_err)?;

    let (item, _) = find_own_note(&db, user_id, id).await?;
    let rendered = render_note(&req.title, &req.content_md);

    let mut active: brew_items::ActiveModel = item.into();
    active.title = Set(rendered.title);
    active.summary = Set(rendered.summary);
    active.content = Set(Some(rendered.html));
    active.content_md = Set(Some(req.content_md.clone()));
    active.image = Set(req.image.clone().or(rendered.image));
    active.word_count = Set(Some(rendered.word_count));
    active.reading_time = Set(Some(rendered.reading_time));
    active.topic = Set(req.topic.clone().filter(|t| !t.trim().is_empty()));
    // 不给发布时间就保持原值：改一个错别字不该把文章顶到列表最前面
    if let Some(published_at) = req.published_at.and_then(millis_to_datetime) {
        active.published_at = Set(published_at.into());
    }

    let item = active
        .update(&db)
        .await
        .map_err(|e| brew_store_http("save note", e))?;

    Ok(Json(json!({
        "success": true,
        "id": item.id,
        "link": item.link,
    })))
}

/// `DELETE /api/brew/notes/{id}` — 删一篇手记。
pub(crate) async fn delete_note(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(id): Path<i32>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let user_id = get_admin_user_id_from_headers(&headers, &db).await?;
    let (item, source) = find_own_note(&db, user_id, id).await?;

    brew_items::Entity::delete_by_id(item.id)
        .exec(&db)
        .await
        .map_err(|e| brew_store_http("delete note", e))?;

    sync_item_count(&db, &source).await;

    Ok(Json(json!({ "success": true })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_source_lands_in_the_own_content_category() {
        // 手记必须落在「我」分类下，否则 api::seo 不会把它当自有内容收录，
        // 手记板块也读不到它（板块按这个分类取合并文章流）
        assert_eq!(NOTE_SOURCE_CATEGORY, "我");
    }

    #[test]
    fn note_source_url_is_not_fetchable() {
        // 非 http(s)：即便某天有人漏掉了 source_type 判断，抓取也发不出请求
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
}
