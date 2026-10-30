//! Shared note-document reads and revision-checked saves for the editor and Agent.

use axum::http::StatusCode;
use sea_orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, QuerySelect, Set,
    TransactionTrait,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::note_authors::{NoteAuthorFace, ensure_note_author, load_authors_for_docs};
use super::note_collab::{NoteCollabEvent, note_collab_hub};
use super::note_old_paths::{invalidate_old_path_cache, old_paths_of_item, set_note_old_paths};
use super::note_publish::{datetime_to_millis, millis_to_datetime, set_published_note_slug};
use super::phantasi_error::{phantasi_http_err, phantasi_store_http};
use crate::error::HttpError;
use crate::models::entities::{phantasi_items, phantasi_note_docs};

#[derive(Debug, Deserialize)]
pub(crate) struct NoteDocWriteRequest {
    pub title: Option<String>,
    pub content_md: Option<String>,
    #[serde(default, deserialize_with = "present_option")]
    pub topic: Option<Option<String>>,
    #[serde(default, deserialize_with = "present_option")]
    pub image: Option<Option<String>>,
    pub client_request_id: Option<String>,
    pub published_at: Option<i64>,
    pub scheduled_at: Option<i64>,
    pub revision: Option<i64>,
    /// 已发布笔记的公开 slug。省略表示不改；未发布时不能设置。
    #[serde(default)]
    pub slug: Option<String>,
    /// 已发布笔记的旧地址（搬家前的文章地址），整组替换。省略表示不改；未发布时不能设置。
    #[serde(default)]
    pub old_paths: Option<Vec<String>>,
}

// Omitted preserves the value; explicit null clears it.
pub(crate) fn present_option<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

pub(crate) fn patched_text(
    current: Option<String>,
    patch: Option<Option<String>>,
) -> Option<String> {
    patch.map(empty_to_none).unwrap_or(current)
}

#[derive(Debug, Serialize)]
pub(crate) struct NoteDocResponse {
    pub id: i32,
    pub item_id: Option<i32>,
    pub user_id: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_display_name: Option<String>,
    pub authors: Vec<NoteAuthorFace>,
    pub title: String,
    pub content_md: String,
    pub has_body: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub excerpt: Option<String>,
    pub topic: Option<String>,
    pub image: Option<String>,
    pub status: String,
    pub scheduled_at: Option<i64>,
    pub published_at: Option<i64>,
    pub revision: i64,
    pub last_error: Option<String>,
    pub updated_at: i64,
    pub slug: Option<String>,
    /// 已发布笔记的旧地址。列表接口不带；单篇读取时已发布的才有。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_paths: Option<Vec<String>>,
}

pub(crate) fn note_excerpt(md: &str) -> String {
    let trimmed = md.trim();
    if trimmed.chars().count() <= 80 {
        return trimmed.to_string();
    }
    trimmed.chars().take(80).collect()
}

pub(crate) fn to_response(doc: phantasi_note_docs::Model, slug: Option<String>) -> NoteDocResponse {
    let has_body = !doc.content_md.trim().is_empty();
    NoteDocResponse {
        id: doc.id,
        item_id: doc.item_id,
        user_id: doc.user_id,
        user_name: None,
        user_display_name: None,
        authors: Vec::new(),
        title: doc.title,
        content_md: doc.content_md,
        has_body,
        excerpt: None,
        topic: doc.topic,
        image: doc.image,
        status: doc.status,
        scheduled_at: doc.scheduled_at.map(datetime_to_millis),
        published_at: doc.published_at.map(datetime_to_millis),
        revision: doc.revision,
        last_error: doc.last_error,
        updated_at: datetime_to_millis(doc.updated_at),
        slug,
        old_paths: None,
    }
}

// Only called with the bounded excerpt and derived cover from list_query().
pub(crate) fn to_list_response(
    doc: phantasi_note_docs::Model,
    slug: Option<String>,
) -> NoteDocResponse {
    let has_body = !doc.content_md.trim().is_empty();
    let excerpt = has_body.then(|| note_excerpt(&doc.content_md));
    let mut row = to_response(doc, slug);
    row.content_md = String::new();
    row.has_body = has_body;
    row.excerpt = excerpt;
    row
}

pub(crate) fn note_docs_with_authors(
    docs: Vec<phantasi_note_docs::Model>,
    authors: std::collections::HashMap<i32, Vec<NoteAuthorFace>>,
    slugs: &std::collections::HashMap<i32, String>,
) -> Vec<NoteDocResponse> {
    docs.into_iter()
        .map(|doc| {
            let slug = doc.item_id.and_then(|item_id| slugs.get(&item_id).cloned());
            let list = authors.get(&doc.id).cloned().unwrap_or_default();
            attach_authors(to_list_response(doc, slug), list)
        })
        .collect()
}

pub(crate) async fn slug_of_item(
    db: &DatabaseConnection,
    item_id: Option<i32>,
) -> Result<Option<String>, HttpError> {
    let Some(item_id) = item_id else {
        return Ok(None);
    };
    let slug = phantasi_items::Entity::find_by_id(item_id)
        .select_only()
        .column(phantasi_items::Column::Slug)
        .into_tuple::<Option<String>>()
        .one(db)
        .await
        .map_err(|error| phantasi_store_http("read note slug", error))?;
    Ok(slug.flatten())
}

pub(crate) async fn slugs_for_items(
    db: &DatabaseConnection,
    docs: &[phantasi_note_docs::Model],
) -> Result<std::collections::HashMap<i32, String>, HttpError> {
    let ids: Vec<i32> = docs.iter().filter_map(|doc| doc.item_id).collect();
    if ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }
    let rows = phantasi_items::Entity::find()
        .filter(phantasi_items::Column::Id.is_in(ids))
        .select_only()
        .columns([phantasi_items::Column::Id, phantasi_items::Column::Slug])
        .into_tuple::<(i32, Option<String>)>()
        .all(db)
        .await
        .map_err(|error| phantasi_store_http("read note slugs", error))?;
    Ok(rows
        .into_iter()
        .filter_map(|(id, slug)| slug.map(|slug| (id, slug)))
        .collect())
}

pub(crate) fn attach_authors(
    mut row: NoteDocResponse,
    authors: Vec<NoteAuthorFace>,
) -> NoteDocResponse {
    if let Some(owner) = authors
        .iter()
        .find(|author| author.role == "owner")
        .or_else(|| authors.first())
    {
        row.user_id = owner.user_id;
        row.user_name = owner.user_name.clone();
        row.user_display_name = owner.user_display_name.clone();
    }
    row.authors = authors;
    row
}

pub(crate) async fn respond_doc(
    db: &DatabaseConnection,
    doc: phantasi_note_docs::Model,
) -> Result<NoteDocResponse, HttpError> {
    let mut authors = load_authors_for_docs(db, &[doc.id]).await?;
    let list = authors.remove(&doc.id).unwrap_or_default();
    let slug = slug_of_item(db, doc.item_id).await?;
    let old_paths = match doc.item_id {
        Some(item_id) => Some(old_paths_of_item(db, Some(item_id)).await?),
        None => None,
    };
    let mut row = to_response(doc, slug);
    row.old_paths = old_paths;
    Ok(attach_authors(row, list))
}

pub(crate) async fn credit_and_respond(
    db: &DatabaseConnection,
    doc: phantasi_note_docs::Model,
    actor_id: i32,
) -> Result<NoteDocResponse, HttpError> {
    ensure_note_author(db, doc.id, actor_id, doc.user_id).await?;
    respond_doc(db, doc).await
}

/// 存草稿必须带 revision；对不上就是 409，不带是 400。不给「跳过锁」的口子。
pub(crate) fn expected_revision(revision: Option<i64>) -> Result<i64, HttpError> {
    revision.ok_or_else(|| phantasi_http_err(StatusCode::BAD_REQUEST, "A revision is required"))
}

pub(crate) fn revision_matches(expected: i64, actual: i64) -> bool {
    expected == actual
}

pub(crate) fn empty_to_none(value: Option<String>) -> Option<String> {
    value.and_then(|text| {
        let trimmed = text.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

pub(crate) async fn find_doc(
    db: &DatabaseConnection,
    id: i32,
) -> Result<phantasi_note_docs::Model, HttpError> {
    phantasi_note_docs::Entity::find_by_id(id)
        .one(db)
        .await
        .map_err(|e| phantasi_store_http("find note doc", e))?
        .ok_or_else(|| phantasi_http_err(StatusCode::NOT_FOUND, "Note draft not found"))
}

/// Shared with Agent after checking the caller's current administrator status.
pub(crate) async fn load_note_docs(
    db: &DatabaseConnection,
) -> Result<serde_json::Value, HttpError> {
    let docs = phantasi_note_docs::list_query()
        .order_by_desc(phantasi_note_docs::Column::UpdatedAt)
        .all(db)
        .await
        .map_err(|e| phantasi_store_http("list note docs", e))?;
    let ids: Vec<i32> = docs.iter().map(|doc| doc.id).collect();
    let authors = load_authors_for_docs(db, &ids).await?;
    let slugs = slugs_for_items(db, &docs).await?;
    Ok(json!({
        "success": true,
        "docs": note_docs_with_authors(docs, authors, &slugs),
    }))
}

pub(crate) async fn load_note_doc(
    db: &DatabaseConnection,
    id: i32,
) -> Result<serde_json::Value, HttpError> {
    let doc = find_doc(db, id).await?;
    Ok(json!({ "success": true, "doc": respond_doc(db, doc).await? }))
}

/// The HTTP editor and Agent share revision checks, history, media and broadcasts.
pub(crate) async fn save_note_doc(
    db: &DatabaseConnection,
    user_id: i32,
    id: i32,
    req: NoteDocWriteRequest,
) -> Result<serde_json::Value, HttpError> {
    let expected = expected_revision(req.revision)?;
    let doc = find_doc(db, id).await?;
    if !revision_matches(expected, doc.revision) {
        return Err(phantasi_http_err(
            StatusCode::CONFLICT,
            "Note draft was updated elsewhere",
        ));
    }
    if req.slug.is_some() && doc.item_id.is_none() {
        return Err(phantasi_http_err(
            StatusCode::BAD_REQUEST,
            "Publish the note before setting a slug",
        ));
    }
    if req.old_paths.is_some() && doc.item_id.is_none() {
        return Err(phantasi_http_err(
            StatusCode::BAD_REQUEST,
            "Publish the note before setting old addresses",
        ));
    }
    // 只放 Set 过的列进 UPDATE；条件带 revision，两个并发写只有一个能落地。
    let mut active = <phantasi_note_docs::ActiveModel as std::default::Default>::default();
    if let Some(title) = req.title {
        active.title = Set(title);
    }
    if let Some(content_md) = req.content_md {
        active.content_md = Set(content_md);
    }
    active.topic = Set(patched_text(doc.topic, req.topic));
    active.image = Set(patched_text(doc.image, req.image));
    if let Some(published_at) = req.published_at.and_then(millis_to_datetime) {
        active.published_at = Set(Some(published_at.into()));
    }
    active.updated_at = Set(crate::clock::now().into());
    active.revision = Set(expected + 1);
    active.last_edited_by = Set(Some(user_id));
    active.last_error = Set(None);
    // RETURNING binds the acknowledgement to this exact revision. A separate
    // SELECT could observe another author's later save and mislabel it as ours.
    let txn = db
        .begin()
        .await
        .map_err(|e| phantasi_store_http("begin note doc save", e))?;
    if let Some(raw) = req.slug.as_deref() {
        let Some(item_id) = doc.item_id else {
            return Err(phantasi_http_err(
                StatusCode::BAD_REQUEST,
                "Publish the note before setting a slug",
            ));
        };
        let source_id = phantasi_items::Entity::find_by_id(item_id)
            .select_only()
            .column(phantasi_items::Column::SourceId)
            .into_tuple::<i32>()
            .one(&txn)
            .await
            .map_err(|error| phantasi_store_http("find published note", error))?
            .ok_or_else(|| phantasi_http_err(StatusCode::NOT_FOUND, "Note not found"))?;
        set_published_note_slug(&txn, item_id, source_id, raw).await?;
    }
    let mut saved_rows = phantasi_note_docs::Entity::update_many()
        .set(active)
        .filter(phantasi_note_docs::Column::Id.eq(id))
        .filter(phantasi_note_docs::Column::Revision.eq(expected))
        .exec_with_returning(&txn)
        .await
        .map_err(|e| phantasi_store_http("save note doc", e))?;
    let saved = saved_rows.pop().ok_or_else(|| {
        phantasi_http_err(StatusCode::CONFLICT, "Note draft was updated elsewhere")
    })?;
    // 在文档行之后才拿旧地址的锁，和发布同一个顺序，两边不会互相等。
    let mut old_paths_changed = false;
    if let (Some(paths), Some(item_id)) = (req.old_paths.as_deref(), doc.item_id) {
        old_paths_changed = set_note_old_paths(&txn, item_id, paths).await?.1;
    }
    crate::services::media::bind_note_draft(
        &txn,
        saved.id,
        expected,
        saved.image.as_deref(),
        &saved.content_md,
        &crate::services::media::configured_origins().await,
    )
    .await
    .map_err(|error| HttpError(error.into()))?;
    txn.commit()
        .await
        .map_err(|e| phantasi_store_http("commit note doc save", e))?;
    if req.slug.is_some() {
        crate::services::blog::request_rebuild();
    }
    if old_paths_changed {
        invalidate_old_path_cache();
    }
    broadcast_saved_doc(&saved, user_id, req.client_request_id);
    Ok(json!({
        "success": true,
        "doc": credit_and_respond(db, saved, user_id).await?,
    }))
}

pub(crate) fn saved_doc_event(
    saved: &phantasi_note_docs::Model,
    user_id: i32,
    client_request_id: Option<String>,
) -> NoteCollabEvent {
    NoteCollabEvent {
        kind: "doc".into(),
        peer_id: String::new(),
        user_id,
        name: None,
        revision: Some(saved.revision),
        client_request_id,
        published_at: saved.published_at.map(datetime_to_millis),
        cursor: None,
        title: Some(saved.title.clone()),
        content_md: Some(saved.content_md.clone()),
        topic: saved.topic.clone(),
        image: saved.image.clone(),
    }
}

pub(crate) fn broadcast_saved_doc(
    saved: &phantasi_note_docs::Model,
    user_id: i32,
    request_id: Option<String>,
) {
    note_collab_hub().publish(saved.id, saved_doc_event(saved, user_id, request_id));
}
