//! 云端笔记文档：草稿、定时、协同。公开文章仍只从 `phantasi_items` 读。

use axum::{
    Json,
    extract::{
        Path, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::StatusCode,
    response::IntoResponse,
};
use chrono::Utc;
use myriad_phantasi_notes::{NoteDocStatus, schedule_at, validate_note};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect, Set, TransactionTrait,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::helpers::{admin_user_id, phantasi_http_err, phantasi_store_http};
use super::note_collab::{NoteCollabEvent, note_collab_hub};
use crate::error::HttpError;
use crate::extract::AdminClaims;
use crate::models::entities::phantasi_note_docs;
use crate::services::note_authors::{
    NoteAuthorFace, add_note_author, ensure_note_author,
    list_note_author_candidates as load_note_author_candidates, load_authors_for_docs,
    remove_note_author, sync_published_author_line,
};
use crate::services::note_publish::{
    datetime_to_millis, millis_to_datetime, publish_doc, upsert_doc_for_published_item,
};

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
}

// Omitted preserves the value; explicit null clears it.
fn present_option<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

fn patched_text(current: Option<String>, patch: Option<Option<String>>) -> Option<String> {
    patch.map(empty_to_none).unwrap_or(current)
}

#[derive(Debug, Deserialize)]
pub(crate) struct NoteDocTopicRequest {
    #[serde(deserialize_with = "present_option")]
    pub topic: Option<Option<String>>,
    pub revision: i64,
}

#[derive(Debug, Deserialize)]
pub(crate) struct NoteAuthorWriteRequest {
    pub user_id: i32,
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
}

fn note_excerpt(md: &str) -> String {
    let trimmed = md.trim();
    if trimmed.chars().count() <= 80 {
        return trimmed.to_string();
    }
    trimmed.chars().take(80).collect()
}

fn to_response(doc: phantasi_note_docs::Model) -> NoteDocResponse {
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
    }
}

// Only called with the bounded excerpt and derived cover from list_query().
fn to_list_response(doc: phantasi_note_docs::Model) -> NoteDocResponse {
    let has_body = !doc.content_md.trim().is_empty();
    let excerpt = has_body.then(|| note_excerpt(&doc.content_md));
    let mut row = to_response(doc);
    row.content_md = String::new();
    row.has_body = has_body;
    row.excerpt = excerpt;
    row
}

fn note_docs_with_authors(
    docs: Vec<phantasi_note_docs::Model>,
    authors: std::collections::HashMap<i32, Vec<NoteAuthorFace>>,
) -> Vec<NoteDocResponse> {
    docs.into_iter()
        .map(|doc| {
            let list = authors.get(&doc.id).cloned().unwrap_or_default();
            attach_authors(to_list_response(doc), list)
        })
        .collect()
}

fn attach_authors(mut row: NoteDocResponse, authors: Vec<NoteAuthorFace>) -> NoteDocResponse {
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

async fn respond_doc(
    db: &DatabaseConnection,
    doc: phantasi_note_docs::Model,
) -> Result<NoteDocResponse, HttpError> {
    let mut authors = load_authors_for_docs(db, &[doc.id]).await?;
    let list = authors.remove(&doc.id).unwrap_or_default();
    Ok(attach_authors(to_response(doc), list))
}

pub(super) async fn credit_and_respond(
    db: &DatabaseConnection,
    doc: phantasi_note_docs::Model,
    actor_id: i32,
) -> Result<NoteDocResponse, HttpError> {
    ensure_note_author(db, doc.id, actor_id, doc.user_id).await?;
    respond_doc(db, doc).await
}

/// 存草稿必须带 revision；对不上就是 409，不带是 400。不给「跳过锁」的口子。
fn expected_revision(revision: Option<i64>) -> Result<i64, HttpError> {
    revision.ok_or_else(|| phantasi_http_err(StatusCode::BAD_REQUEST, "A revision is required"))
}

fn revision_matches(expected: i64, actual: i64) -> bool {
    expected == actual
}

/// WS 单帧上限。整篇快照也用不到这么大；再大就是别的东西。
const WS_MAX_FRAME_BYTES: usize = 256 * 1024;
/// 客户端能发的事件种类。别的一律丢。
const WS_CLIENT_KINDS: [&str; 2] = ["presence", "edit"];

fn ws_client_kind_allowed(kind: &str) -> bool {
    kind.is_empty() || WS_CLIENT_KINDS.contains(&kind)
}

fn empty_to_none(value: Option<String>) -> Option<String> {
    value.and_then(|text| {
        let trimmed = text.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

pub(super) async fn find_doc(
    db: &DatabaseConnection,
    id: i32,
) -> Result<phantasi_note_docs::Model, HttpError> {
    phantasi_note_docs::Entity::find_by_id(id)
        .one(db)
        .await
        .map_err(|e| phantasi_store_http("find note doc", e))?
        .ok_or_else(|| phantasi_http_err(StatusCode::NOT_FOUND, "Note draft not found"))
}

/// `GET /notes/docs` — 管理端文档列表，含草稿和定时。
pub(crate) async fn list_note_docs(
    State(db): State<DatabaseConnection>,
    _admin: AdminClaims,
) -> Result<Json<serde_json::Value>, HttpError> {
    let docs = phantasi_note_docs::list_query()
        .order_by_desc(phantasi_note_docs::Column::UpdatedAt)
        .all(&db)
        .await
        .map_err(|e| phantasi_store_http("list note docs", e))?;
    let ids: Vec<i32> = docs.iter().map(|doc| doc.id).collect();
    let authors = load_authors_for_docs(&db, &ids).await?;
    Ok(Json(json!({
        "success": true,
        "docs": note_docs_with_authors(docs, authors),
    })))
}

/// `POST /notes/docs` — 建一篇云端草稿。标题可空。
pub(crate) async fn create_note_doc(
    State(db): State<DatabaseConnection>,
    admin: AdminClaims,
    Json(req): Json<NoteDocWriteRequest>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let user_id = admin_user_id(&admin)?;
    let now = Utc::now();
    let doc = phantasi_note_docs::ActiveModel {
        user_id: Set(user_id),
        title: Set(req.title.unwrap_or_default()),
        content_md: Set(req.content_md.unwrap_or_default()),
        topic: Set(empty_to_none(req.topic.flatten())),
        image: Set(empty_to_none(req.image.flatten())),
        status: Set(NoteDocStatus::Draft.as_str().to_string()),
        published_at: Set(req
            .published_at
            .and_then(millis_to_datetime)
            .map(|at| at.into())),
        revision: Set(1),
        last_edited_by: Set(Some(user_id)),
        created_at: Set(now.into()),
        updated_at: Set(now.into()),
        ..Default::default()
    };
    let txn = db
        .begin()
        .await
        .map_err(|e| phantasi_store_http("begin note doc create", e))?;
    let doc = doc
        .insert(&txn)
        .await
        .map_err(|e| phantasi_store_http("create note doc", e))?;
    crate::services::media::bind_note_draft(
        &txn,
        doc.id,
        doc.revision,
        doc.image.as_deref(),
        &doc.content_md,
        &[],
    )
    .await
    .map_err(|error| HttpError(error.into()))?;
    txn.commit()
        .await
        .map_err(|e| phantasi_store_http("commit note doc create", e))?;
    Ok(Json(json!({
        "success": true,
        "doc": credit_and_respond(&db, doc, user_id).await?,
    })))
}

/// `GET /notes/docs/{id}`
pub(crate) async fn get_note_doc(
    State(db): State<DatabaseConnection>,
    _admin: AdminClaims,
    Path(id): Path<i32>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let doc = find_doc(&db, id).await?;
    Ok(Json(
        json!({ "success": true, "doc": respond_doc(&db, doc).await? }),
    ))
}

/// `GET /notes/docs/for-item/{item_id}` — 给已发布笔记找或建对应文档。
pub(crate) async fn get_note_doc_for_item(
    State(db): State<DatabaseConnection>,
    admin: AdminClaims,
    Path(item_id): Path<i32>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let user_id = admin_user_id(&admin)?;
    let source_id = crate::models::entities::phantasi_items::Entity::find_by_id(item_id)
        .select_only()
        .column(crate::models::entities::phantasi_items::Column::SourceId)
        .into_tuple::<i32>()
        .one(&db)
        .await
        .map_err(|e| phantasi_store_http("find note", e))?
        .ok_or_else(|| phantasi_http_err(StatusCode::NOT_FOUND, "Note not found"))?;
    let source = crate::models::entities::phantasi_sources::Entity::find_by_id(source_id)
        .one(&db)
        .await
        .map_err(|e| phantasi_store_http("find note source", e))?;
    if !source.as_ref().is_some_and(|source| {
        source.source_type == crate::models::entities::phantasi_sources::SourceType::Note
    }) {
        return Err(phantasi_http_err(StatusCode::NOT_FOUND, "Note not found"));
    }
    if let Some(doc) = phantasi_note_docs::Entity::find()
        .filter(phantasi_note_docs::Column::ItemId.eq(item_id))
        .one(&db)
        .await
        .map_err(|e| phantasi_store_http("find note doc", e))?
    {
        return Ok(Json(
            json!({ "success": true, "doc": respond_doc(&db, doc).await? }),
        ));
    }
    // Only legacy published notes without a cloud document need this backfill.
    let item = crate::models::entities::phantasi_items::Entity::find_by_id(item_id)
        .one(&db)
        .await
        .map_err(|e| phantasi_store_http("find note", e))?
        .ok_or_else(|| phantasi_http_err(StatusCode::NOT_FOUND, "Note not found"))?;
    let published = crate::services::note_publish::PublishedNote {
        id: item.id,
        link: item.link.clone(),
    };
    let doc = upsert_doc_for_published_item(
        &db,
        user_id,
        &published,
        &item.title,
        item.content_md.as_deref().unwrap_or(""),
        item.topic,
        item.image,
        Some(datetime_to_millis(item.published_at)),
    )
    .await?;
    Ok(Json(json!({
        "success": true,
        "doc": credit_and_respond(&db, doc, user_id).await?,
    })))
}

/// `PUT /notes/docs/{id}` — 存草稿。带 revision，对不上 409。
pub(crate) async fn update_note_doc(
    State(db): State<DatabaseConnection>,
    admin: AdminClaims,
    Path(id): Path<i32>,
    Json(req): Json<NoteDocWriteRequest>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let user_id = admin_user_id(&admin)?;
    let expected = expected_revision(req.revision)?;
    let doc = find_doc(&db, id).await?;
    if !revision_matches(expected, doc.revision) {
        return Err(phantasi_http_err(
            StatusCode::CONFLICT,
            "Note draft was updated elsewhere",
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
    active.updated_at = Set(Utc::now().into());
    active.revision = Set(expected + 1);
    active.last_edited_by = Set(Some(user_id));
    active.last_error = Set(None);
    // RETURNING binds the acknowledgement to this exact revision. A separate
    // SELECT could observe another author's later save and mislabel it as ours.
    let txn = db
        .begin()
        .await
        .map_err(|e| phantasi_store_http("begin note doc save", e))?;
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
    crate::services::media::bind_note_draft(
        &txn,
        saved.id,
        expected,
        saved.image.as_deref(),
        &saved.content_md,
        &[],
    )
    .await
    .map_err(|error| HttpError(error.into()))?;
    txn.commit()
        .await
        .map_err(|e| phantasi_store_http("commit note doc save", e))?;
    broadcast_saved_doc(&saved, user_id, req.client_request_id);
    Ok(Json(json!({
        "success": true,
        "doc": credit_and_respond(&db, saved, user_id).await?,
    })))
}

fn saved_doc_event(
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

pub(super) fn broadcast_saved_doc(
    saved: &phantasi_note_docs::Model,
    user_id: i32,
    request_id: Option<String>,
) {
    note_collab_hub().publish(saved.id, saved_doc_event(saved, user_id, request_id));
}

/// `PUT /notes/docs/{id}/topic` — classify without publishing draft content.
pub(crate) async fn update_note_doc_topic(
    State(db): State<DatabaseConnection>,
    admin: AdminClaims,
    Path(id): Path<i32>,
    Json(req): Json<NoteDocTopicRequest>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let user_id = admin_user_id(&admin)?;
    let topic = req
        .topic
        .ok_or_else(|| phantasi_http_err(StatusCode::BAD_REQUEST, "A topic is required"))?;
    let saved = crate::services::note_publish::update_note_doc_topic(
        &db,
        id,
        req.revision,
        empty_to_none(topic),
    )
    .await?;
    broadcast_saved_doc(&saved, user_id, None);
    Ok(Json(
        json!({ "success": true, "doc": respond_doc(&db, saved).await? }),
    ))
}

/// `DELETE /notes/docs/{id}` — 删云端文档。已发布的文章另走 DELETE /notes/{item}。
pub(crate) async fn delete_note_doc(
    State(db): State<DatabaseConnection>,
    _admin: AdminClaims,
    Path(id): Path<i32>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let status = phantasi_note_docs::Entity::find_by_id(id)
        .select_only()
        .column(phantasi_note_docs::Column::Status)
        .into_tuple::<String>()
        .one(&db)
        .await
        .map_err(|e| phantasi_store_http("find note doc", e))?
        .ok_or_else(|| phantasi_http_err(StatusCode::NOT_FOUND, "Note draft not found"))?;
    if status == NoteDocStatus::Published.as_str() {
        return Err(phantasi_http_err(
            StatusCode::BAD_REQUEST,
            "Published notes must be deleted from the article",
        ));
    }
    let txn = db
        .begin()
        .await
        .map_err(|e| phantasi_store_http("begin note doc delete", e))?;
    crate::services::media::clear_note_doc(&txn, id, None)
        .await
        .map_err(|error| HttpError(error.into()))?;
    phantasi_note_docs::Entity::delete_by_id(id)
        .exec(&txn)
        .await
        .map_err(|e| phantasi_store_http("delete note doc", e))?;
    txn.commit()
        .await
        .map_err(|e| phantasi_store_http("commit note doc delete", e))?;
    Ok(Json(json!({ "success": true })))
}

/// `POST /notes/docs/{id}/publish`
pub(crate) async fn publish_note_doc(
    State(db): State<DatabaseConnection>,
    admin: AdminClaims,
    Path(id): Path<i32>,
    Json(req): Json<NoteDocWriteRequest>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let user_id = admin_user_id(&admin)?;
    let expected = expected_revision(req.revision)?;
    let mut doc = find_doc(&db, id).await?;
    if !revision_matches(expected, doc.revision) {
        return Err(phantasi_http_err(
            StatusCode::CONFLICT,
            "Note draft was updated elsewhere",
        ));
    }
    if let Some(title) = req.title.clone() {
        doc.title = title;
    }
    if let Some(content_md) = req.content_md.clone() {
        doc.content_md = content_md;
    }
    doc.topic = patched_text(doc.topic, req.topic.clone());
    doc.image = patched_text(doc.image, req.image.clone());
    let published_at = req
        .published_at
        .or(doc.published_at.map(datetime_to_millis));
    let mut active = <phantasi_note_docs::ActiveModel as std::default::Default>::default();
    active.title = Set(doc.title.clone());
    active.content_md = Set(doc.content_md.clone());
    active.topic = Set(doc.topic.clone());
    active.image = Set(doc.image.clone());
    active.updated_at = Set(Utc::now().into());
    active.revision = Set(expected + 1);
    active.last_edited_by = Set(Some(user_id));
    let claimed = phantasi_note_docs::Entity::update_many()
        .set(active)
        .filter(phantasi_note_docs::Column::Id.eq(id))
        .filter(phantasi_note_docs::Column::Revision.eq(expected))
        .exec(&db)
        .await
        .map_err(|e| phantasi_store_http("claim note publish", e))?;
    if claimed.rows_affected == 0 {
        return Err(phantasi_http_err(
            StatusCode::CONFLICT,
            "Note draft was updated elsewhere",
        ));
    }
    doc.revision = expected + 1;
    ensure_note_author(&db, doc.id, user_id, doc.user_id).await?;
    let (item, saved) = publish_doc(&db, doc, published_at).await?;
    broadcast_saved_doc(&saved, user_id, req.client_request_id);
    Ok(Json(json!({
        "success": true,
        "id": item.id,
        "link": item.link,
        "doc": respond_doc(&db, saved).await?,
    })))
}

/// `POST /notes/docs/{id}/schedule`
pub(crate) async fn schedule_note_doc(
    State(db): State<DatabaseConnection>,
    admin: AdminClaims,
    Path(id): Path<i32>,
    Json(req): Json<NoteDocWriteRequest>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let user_id = admin_user_id(&admin)?;
    let doc = find_doc(&db, id).await?;
    if doc.status == NoteDocStatus::Published.as_str() {
        return Err(phantasi_http_err(
            StatusCode::BAD_REQUEST,
            "Published notes cannot be scheduled",
        ));
    }
    let now_ms = Utc::now().timestamp_millis();
    let at = schedule_at(now_ms, req.scheduled_at).map_err(|err| match err {
        myriad_phantasi_notes::ScheduleError::AlreadyDue => {
            phantasi_http_err(StatusCode::BAD_REQUEST, "That time has already passed")
        }
        myriad_phantasi_notes::ScheduleError::MissingTime => {
            phantasi_http_err(StatusCode::BAD_REQUEST, "A schedule time is required")
        }
    })?;
    let expected = expected_revision(req.revision)?;
    if !revision_matches(expected, doc.revision) {
        return Err(phantasi_http_err(
            StatusCode::CONFLICT,
            "Note draft was updated elsewhere",
        ));
    }
    let title = req.title.clone().unwrap_or_else(|| doc.title.clone());
    let content_md = req
        .content_md
        .clone()
        .unwrap_or_else(|| doc.content_md.clone());
    validate_note(&title, &content_md)
        .map_err(|err| phantasi_http_err(StatusCode::BAD_REQUEST, err.message()))?;
    let mut active = <phantasi_note_docs::ActiveModel as std::default::Default>::default();
    active.title = Set(title);
    active.content_md = Set(content_md);
    active.topic = Set(patched_text(doc.topic, req.topic));
    active.image = Set(patched_text(doc.image, req.image));
    active.status = Set(NoteDocStatus::Scheduled.as_str().to_string());
    active.scheduled_at = Set(millis_to_datetime(at).map(|value| value.into()));
    active.published_at = Set(millis_to_datetime(at).map(|value| value.into()));
    active.updated_at = Set(Utc::now().into());
    active.revision = Set(expected + 1);
    active.last_edited_by = Set(Some(user_id));
    active.last_error = Set(None);
    let txn = db
        .begin()
        .await
        .map_err(|e| phantasi_store_http("begin note schedule", e))?;
    let mut saved_rows = phantasi_note_docs::Entity::update_many()
        .set(active)
        .filter(phantasi_note_docs::Column::Id.eq(id))
        .filter(phantasi_note_docs::Column::Revision.eq(expected))
        .exec_with_returning(&txn)
        .await
        .map_err(|e| phantasi_store_http("schedule note doc", e))?;
    let saved = saved_rows.pop().ok_or_else(|| {
        phantasi_http_err(StatusCode::CONFLICT, "Note draft was updated elsewhere")
    })?;
    crate::services::media::bind_note_draft(
        &txn,
        saved.id,
        expected,
        saved.image.as_deref(),
        &saved.content_md,
        &[],
    )
    .await
    .map_err(|error| HttpError(error.into()))?;
    txn.commit()
        .await
        .map_err(|e| phantasi_store_http("commit note schedule", e))?;
    broadcast_saved_doc(&saved, user_id, req.client_request_id);
    Ok(Json(json!({
        "success": true,
        "doc": credit_and_respond(&db, saved, user_id).await?,
    })))
}

/// `POST /notes/docs/{id}/unschedule`
pub(crate) async fn unschedule_note_doc(
    State(db): State<DatabaseConnection>,
    admin: AdminClaims,
    Path(id): Path<i32>,
    Json(req): Json<NoteDocWriteRequest>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let user_id = admin_user_id(&admin)?;
    let expected = expected_revision(req.revision)?;
    let doc = find_doc(&db, id).await?;
    if !revision_matches(expected, doc.revision) {
        return Err(phantasi_http_err(
            StatusCode::CONFLICT,
            "Note draft was updated elsewhere",
        ));
    }
    if doc.status != NoteDocStatus::Scheduled.as_str()
        && req.title.is_none()
        && req.content_md.is_none()
        && req.topic.is_none()
        && req.image.is_none()
        && req.published_at.is_none()
    {
        return Ok(Json(json!({
            "success": true,
            "doc": credit_and_respond(&db, doc, user_id).await?,
        })));
    }
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
    active.status = Set(if doc.status == NoteDocStatus::Scheduled.as_str() {
        NoteDocStatus::Draft.as_str().to_string()
    } else {
        doc.status
    });
    active.scheduled_at = Set(None);
    active.last_error = Set(None);
    active.updated_at = Set(Utc::now().into());
    active.revision = Set(expected + 1);
    active.last_edited_by = Set(Some(user_id));
    let txn = db
        .begin()
        .await
        .map_err(|e| phantasi_store_http("begin note unschedule", e))?;
    let mut saved_rows = phantasi_note_docs::Entity::update_many()
        .set(active)
        .filter(phantasi_note_docs::Column::Id.eq(id))
        .filter(phantasi_note_docs::Column::Revision.eq(expected))
        .exec_with_returning(&txn)
        .await
        .map_err(|e| phantasi_store_http("unschedule note doc", e))?;
    let saved = saved_rows.pop().ok_or_else(|| {
        phantasi_http_err(StatusCode::CONFLICT, "Note draft was updated elsewhere")
    })?;
    crate::services::media::bind_note_draft(
        &txn,
        saved.id,
        expected,
        saved.image.as_deref(),
        &saved.content_md,
        &[],
    )
    .await
    .map_err(|error| HttpError(error.into()))?;
    txn.commit()
        .await
        .map_err(|e| phantasi_store_http("commit note unschedule", e))?;
    broadcast_saved_doc(&saved, user_id, req.client_request_id);
    Ok(Json(json!({
        "success": true,
        "doc": credit_and_respond(&db, saved, user_id).await?,
    })))
}

/// `GET /notes/author-candidates`
pub(crate) async fn list_note_author_candidates(
    State(db): State<DatabaseConnection>,
    _admin: AdminClaims,
) -> Result<Json<serde_json::Value>, HttpError> {
    Ok(Json(json!({
        "success": true,
        "candidates": load_note_author_candidates(&db).await?,
    })))
}

/// Author management and collaboration admission only need owner/item IDs.
pub(super) async fn find_doc_owner(
    db: &DatabaseConnection,
    id: i32,
) -> Result<(i32, Option<i32>), HttpError> {
    phantasi_note_docs::Entity::find_by_id(id)
        .select_only()
        .columns([
            phantasi_note_docs::Column::UserId,
            phantasi_note_docs::Column::ItemId,
        ])
        .into_tuple::<(i32, Option<i32>)>()
        .one(db)
        .await
        .map_err(|e| phantasi_store_http("find note doc", e))?
        .ok_or_else(|| phantasi_http_err(StatusCode::NOT_FOUND, "Note draft not found"))
}

/// `GET /notes/docs/{id}/authors` — settings do not need the document body.
pub(crate) async fn list_note_doc_authors(
    State(db): State<DatabaseConnection>,
    _admin: AdminClaims,
    Path(id): Path<i32>,
) -> Result<Json<serde_json::Value>, HttpError> {
    find_doc_owner(&db, id).await?;
    let mut authors = load_authors_for_docs(&db, &[id]).await?;
    Ok(Json(json!({
        "success": true,
        "authors": authors.remove(&id).unwrap_or_default(),
    })))
}

/// `POST /notes/docs/{id}/authors`
pub(crate) async fn add_note_doc_author(
    State(db): State<DatabaseConnection>,
    _admin: AdminClaims,
    Path(id): Path<i32>,
    Json(req): Json<NoteAuthorWriteRequest>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let (owner_id, item_id) = find_doc_owner(&db, id).await?;
    let authors = add_note_author(&db, id, owner_id, req.user_id, item_id).await?;
    Ok(Json(json!({ "success": true, "authors": authors })))
}

/// `DELETE /notes/docs/{id}/authors/{user_id}`
pub(crate) async fn remove_note_doc_author(
    State(db): State<DatabaseConnection>,
    _admin: AdminClaims,
    Path((id, user_id)): Path<(i32, i32)>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let (_, item_id) = find_doc_owner(&db, id).await?;
    let authors = remove_note_author(&db, id, user_id, item_id).await?;
    Ok(Json(json!({ "success": true, "authors": authors })))
}

/// `GET /notes/docs/{id}/ws`
pub(crate) async fn note_doc_websocket(
    ws: WebSocketUpgrade,
    State(db): State<DatabaseConnection>,
    admin: AdminClaims,
    headers: axum::http::HeaderMap,
    Path(id): Path<i32>,
) -> Result<impl IntoResponse, HttpError> {
    // `admin_middleware` on the route already verified the current admin;
    // `AdminClaims` reuses that proof instead of querying again.
    let allowed = crate::middleware::ws_origin::allowed_origins_from_global_config().await;
    crate::middleware::ws_origin::assert_ws_origin_for_cookie_session(&headers, &allowed)?;
    let user_id = admin_user_id(&admin)?;
    let username = admin.0.username;
    let (owner_id, item_id) = find_doc_owner(&db, id).await?;
    Ok(ws.on_upgrade(move |socket| {
        handle_note_doc_socket(socket, db, id, user_id, owner_id, item_id, username)
    }))
}

async fn handle_note_doc_socket(
    mut socket: WebSocket,
    db: DatabaseConnection,
    doc_id: i32,
    user_id: i32,
    owner_id: i32,
    item_id: Option<i32>,
    username: String,
) {
    let mut credited = false;
    let hub = note_collab_hub();
    let mut rx = hub.subscribe(doc_id);
    let peer_id = uuid::Uuid::new_v4().to_string();
    hub.publish(
        doc_id,
        NoteCollabEvent {
            kind: "join".into(),
            peer_id: peer_id.clone(),
            user_id,
            name: Some(username.clone()),
            revision: None,
            client_request_id: None,
            published_at: None,
            cursor: None,
            title: None,
            content_md: None,
            topic: None,
            image: None,
        },
    );
    loop {
        tokio::select! {
            event = rx.recv() => {
                let Ok(event) = event else {
                    // Never keep a collaborative client silently on a missed revision.
                    let _ = socket.send(Message::Close(None)).await;
                    break;
                };
                if event.peer_id == peer_id {
                    continue;
                }
                let msg = serde_json::to_string(&event).unwrap_or_default();
                if socket.send(Message::Text(msg.into())).await.is_err() {
                    break;
                }
            }
            Some(msg) = socket.recv() => {
                match msg {
                    Ok(Message::Ping(data)) => {
                        if socket.send(Message::Pong(data)).await.is_err() {
                            break;
                        }
                    }
                    Ok(Message::Text(text)) => {
                        // 太大的帧直接断开：正常客户端不会发，发了就是出问题了。
                        if text.len() > WS_MAX_FRAME_BYTES {
                            tracing::warn!(doc_id, user_id, bytes = text.len(), "note collab frame too large");
                            break;
                        }
                        let Ok(mut incoming) = serde_json::from_str::<NoteCollabEvent>(&text) else {
                            continue;
                        };
                        if !ws_client_kind_allowed(&incoming.kind) {
                            continue;
                        }
                        incoming.peer_id = peer_id.clone();
                        incoming.user_id = user_id;
                        incoming.name = Some(username.clone());
                        incoming.revision = None;
                        incoming.client_request_id = None;
                        incoming.published_at = None;
                        if incoming.kind.is_empty() {
                            incoming.kind = "presence".into();
                        }
                        if incoming.kind == "edit" && !credited {
                            credited = true;
                            if ensure_note_author(&db, doc_id, user_id, owner_id)
                                .await
                                .is_ok()
                            {
                                let _ = sync_published_author_line(&db, item_id, doc_id).await;
                            }
                        }
                        hub.publish(doc_id, incoming);
                    }
                    Ok(Message::Close(_)) | Err(_) => break,
                    _ => {}
                }
            }
            else => break,
        }
    }
    hub.publish(
        doc_id,
        NoteCollabEvent {
            kind: "leave".into(),
            peer_id,
            user_id,
            name: None,
            revision: None,
            client_request_id: None,
            published_at: None,
            cursor: None,
            title: None,
            content_md: None,
            topic: None,
            image: None,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topic_command_requires_topic_and_revision_but_accepts_null() {
        assert!(serde_json::from_value::<NoteDocTopicRequest>(json!({"revision": 1})).is_err());
        assert!(serde_json::from_value::<NoteDocTopicRequest>(json!({"topic": null})).is_err());
        let clear: NoteDocTopicRequest =
            serde_json::from_value(json!({"topic": null, "revision": 1})).unwrap();
        assert_eq!(clear.topic, Some(None));
    }

    #[test]
    fn empty_strings_become_none() {
        assert_eq!(empty_to_none(Some("  ".into())), None);
        assert_eq!(empty_to_none(Some("ai".into())), Some("ai".into()));
    }

    #[test]
    fn stale_revision_is_a_conflict_and_missing_is_rejected() {
        assert!(revision_matches(3, 3));
        assert!(!revision_matches(2, 3));
        assert!(expected_revision(None).is_err());
        assert_eq!(expected_revision(Some(4)).ok(), Some(4));
    }

    #[test]
    fn metadata_patch_distinguishes_omitted_and_clear() {
        let missing: NoteDocWriteRequest = serde_json::from_str("{}").unwrap();
        let clear: NoteDocWriteRequest =
            serde_json::from_str(r#"{"topic":null,"image":null}"#).unwrap();
        assert_eq!(missing.topic, None);
        assert_eq!(missing.image, None);
        assert_eq!(clear.topic, Some(None));
        assert_eq!(clear.image, Some(None));
        for patch in [clear.topic, clear.image] {
            assert_eq!(patched_text(Some("old".into()), patch), None);
        }
        assert_eq!(
            patched_text(Some("old".into()), missing.topic),
            Some("old".into())
        );
        let changed: NoteDocWriteRequest =
            serde_json::from_str(r#"{"topic":"new","image":""}"#).unwrap();
        assert_eq!(
            patched_text(Some("old".into()), changed.topic),
            Some("new".into())
        );
        assert_eq!(patched_text(Some("old".into()), changed.image), None);
    }

    #[test]
    fn saved_event_identifies_revision_and_explicit_clears() {
        let now = Utc::now().fixed_offset();
        let doc = phantasi_note_docs::Model {
            id: 7,
            user_id: 1,
            item_id: None,
            title: "Title".into(),
            content_md: "Body".into(),
            topic: None,
            image: None,
            status: "draft".into(),
            scheduled_at: None,
            published_at: Some(now),
            revision: 4,
            last_edited_by: Some(1),
            last_error: None,
            created_at: now,
            updated_at: now,
        };
        let event = saved_doc_event(&doc, 2, Some("request-4".into()));
        let wire = serde_json::to_value(event).unwrap();
        assert_eq!(wire["client_request_id"], "request-4");
        assert_eq!(wire["revision"], 4);
        assert_eq!(wire["user_id"], 2);
        assert_eq!(wire["content_md"], "Body");
        assert!(wire.get("topic").unwrap().is_null());
        assert!(wire.get("image").unwrap().is_null());
        assert_eq!(wire["published_at"], now.timestamp_millis());
    }

    #[test]
    fn draft_save_is_a_conditional_update() {
        let src = include_str!("note_docs.rs");
        let start = src
            .find("pub(crate) async fn update_note_doc")
            .expect("update_note_doc");
        let body = &src[start..];
        let end = body[1..]
            .find("\npub(crate) async fn ")
            .map(|index| index + 1)
            .unwrap_or(body.len());
        let update = &body[..end];
        assert!(update.contains("update_many()"));
        assert!(update.contains("Column::Revision.eq(expected)"));
        assert!(update.contains("exec_with_returning(&txn)"));
        assert!(update.contains("bind_note_draft"));
        assert!(update.contains("saved_rows.pop().ok_or_else"));
    }

    #[test]
    fn ws_only_relays_client_kinds() {
        assert!(ws_client_kind_allowed(""));
        assert!(ws_client_kind_allowed("presence"));
        assert!(ws_client_kind_allowed("edit"));
        assert!(!ws_client_kind_allowed("doc"));
        assert!(!ws_client_kind_allowed("join"));
        assert!(!ws_client_kind_allowed("leave"));
    }

    #[test]
    fn creating_a_doc_does_not_write_phantasi_items() {
        let src = include_str!("note_docs.rs");
        let start = src
            .find("pub(crate) async fn create_note_doc")
            .expect("create_note_doc");
        let body = &src[start..];
        let end = body[1..]
            .find("\npub(crate) async fn ")
            .map(|index| index + 1)
            .unwrap_or(body.len());
        let create = &body[..end];
        assert!(create.contains("NoteDocStatus::Draft"));
        assert!(
            !create.contains("write_published_item"),
            "cloud drafts must stay out of phantasi_items"
        );
    }

    #[test]
    fn publish_does_not_overwrite_owner() {
        let src = include_str!("note_docs.rs");
        let start = src
            .find("pub(crate) async fn publish_note_doc")
            .expect("publish_note_doc");
        let body = &src[start..];
        let end = body[1..]
            .find("\npub(crate) async fn ")
            .map(|index| index + 1)
            .unwrap_or(body.len());
        let publish = &body[..end];
        assert!(
            !publish.contains("doc.user_id = user_id"),
            "publish must keep the original owner"
        );
        assert!(publish.contains("ensure_note_author"));
        assert!(publish.contains("expected_revision"));
        assert!(publish.contains("update_many()"));
        assert!(publish.contains("Column::Revision.eq(expected)"));
    }

    #[test]
    fn schedule_and_unschedule_use_revision_lock() {
        let src = include_str!("note_docs.rs");
        let schedule = {
            let start = src
                .find("pub(crate) async fn schedule_note_doc")
                .expect("schedule_note_doc");
            let body = &src[start..];
            let end = body[1..]
                .find("\npub(crate) async fn ")
                .map(|index| index + 1)
                .unwrap_or(body.len());
            &body[..end]
        };
        let unschedule = {
            let start = src
                .find("pub(crate) async fn unschedule_note_doc")
                .expect("unschedule_note_doc");
            let body = &src[start..];
            let end = body[1..]
                .find("\npub(crate) async fn ")
                .map(|index| index + 1)
                .unwrap_or(body.len());
            &body[..end]
        };
        assert!(schedule.contains("expected_revision"));
        assert!(schedule.contains("update_many()"));
        assert!(schedule.contains("Column::Revision.eq(expected)"));
        assert!(unschedule.contains("expected_revision"));
        assert!(unschedule.contains("update_many()"));
        assert!(unschedule.contains("Column::Revision.eq(expected)"));
        assert!(unschedule.contains("Json<NoteDocWriteRequest>"));
    }

    #[test]
    fn list_docs_strip_body() {
        let src = include_str!("note_docs.rs");
        let start = src.find("fn to_list_response").expect("to_list_response");
        let body = &src[start..];
        assert!(body.contains("row.content_md = String::new()"));
        assert!(body.contains("note_excerpt"));
        assert!(body.contains("phantasi_note_docs::list_query()"));
    }
}
