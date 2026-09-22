//! 把云端笔记文档落成公开 `phantasi_items`。草稿不走这里。
//!
//! 写 `phantasi_items` 和把文档标成已发布是一个事务：要么公开文章和文档状态一起落地，
//! 要么什么都不变。调度器发定时稿之前先用 revision 「认领」一次，多实例同时到点
//! 也只有一个能拿到。

use chrono::{TimeZone, Utc};
use myriad_phantasi_notes::{
    NoteDocStatus, is_due, note_guid, note_link, render_note, validate_note,
};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseBackend, DatabaseConnection,
    EntityTrait, PaginatorTrait, QueryFilter, QuerySelect, Set, Statement, TransactionTrait,
    Value as SeaValue,
};
use serde::Serialize;

use crate::api::seo::PHANTASI_MINE_CATEGORY as NOTE_SOURCE_CATEGORY;
use crate::error::HttpError;
use crate::models::entities::{phantasi_items, phantasi_note_docs, phantasi_sources};
use axum::{Json, http::StatusCode};
use myriad_error::AppError;

fn phantasi_http_err(status: StatusCode, error: impl Into<String>) -> HttpError {
    HttpError::from((status, Json(AppError::fail_json(error))))
}

fn media_bind_http(error: crate::services::media::MediaError) -> HttpError {
    HttpError(error.into())
}

fn phantasi_store_http(context: &'static str, error: impl std::fmt::Display) -> HttpError {
    tracing::error!(%error, context, "phantasi store failed");
    phantasi_http_err(
        StatusCode::INTERNAL_SERVER_ERROR,
        format!("Failed to {context}"),
    )
}

pub(crate) const NOTE_SOURCE_NAME: &str = "笔记";
pub(crate) const NOTE_SOURCE_URL: &str = "myriad:notes";

#[derive(Debug, Clone, Serialize)]
pub struct PublishedNote {
    pub id: i32,
    pub link: String,
}

pub fn millis_to_datetime(ms: i64) -> Option<chrono::DateTime<Utc>> {
    Utc.timestamp_millis_opt(ms).single()
}

pub fn datetime_to_millis(value: sea_orm::prelude::DateTimeWithTimeZone) -> i64 {
    value.timestamp_millis()
}

fn validation_err(err: myriad_phantasi_notes::NoteError) -> HttpError {
    phantasi_http_err(StatusCode::BAD_REQUEST, err.message())
}

fn unique_violation(err: &impl std::fmt::Display) -> bool {
    let lower = err.to_string().to_ascii_lowercase();
    lower.contains("23505") || lower.contains("duplicate key") || lower.contains("unique")
}

async fn find_note_source<C: ConnectionTrait>(
    db: &C,
) -> Result<Option<phantasi_sources::Model>, HttpError> {
    phantasi_sources::Entity::find()
        .filter(phantasi_sources::Column::SourceType.eq(phantasi_sources::SourceType::Note))
        .one(db)
        .await
        .map_err(|e| phantasi_store_http("find note source", e))
}

async fn ensure_note_source<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
) -> Result<phantasi_sources::Model, HttpError> {
    if let Some(source) = find_note_source(db).await? {
        return Ok(source);
    }

    let now = Utc::now();
    let source = phantasi_sources::ActiveModel {
        user_id: Set(user_id),
        name: Set(NOTE_SOURCE_NAME.to_string()),
        url: Set(NOTE_SOURCE_URL.to_string()),
        feed_type: Set(phantasi_sources::FeedType::Rss),
        source_type: Set(phantasi_sources::SourceType::Note),
        category: Set(Some(NOTE_SOURCE_CATEGORY.to_string())),
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

    match source.insert(db).await {
        Ok(created) => Ok(created),
        Err(error) if unique_violation(&error) => find_note_source(db)
            .await?
            .ok_or_else(|| phantasi_store_http("create note source", error)),
        Err(error) => Err(phantasi_store_http("create note source", error)),
    }
}

async fn sync_item_count<C: ConnectionTrait>(
    db: &C,
    source: &phantasi_sources::Model,
) -> Result<(), HttpError> {
    // Serialize recounts before reading committed items. NO KEY UPDATE remains
    // compatible with the foreign-key KEY SHARE held by concurrent insertions.
    phantasi_sources::Entity::find_by_id(source.id)
        .lock(sea_orm::sea_query::LockType::NoKeyUpdate)
        .one(db)
        .await
        .map_err(|error| phantasi_store_http("lock note source count", error))?
        .ok_or_else(|| phantasi_http_err(StatusCode::NOT_FOUND, "Note source not found"))?;
    let count = phantasi_items::Entity::find()
        .filter(phantasi_items::Column::SourceId.eq(source.id))
        .count(db)
        .await
        .map_err(|error| phantasi_store_http("count notes", error))?;
    let mut active: phantasi_sources::ActiveModel = source.clone().into();
    active.item_count = Set(i32::try_from(count).unwrap_or(i32::MAX));
    active.updated_at = Set(Utc::now().into());
    active
        .update(db)
        .await
        .map_err(|error| phantasi_store_http("sync note item count", error))?;
    Ok(())
}

/// 把一篇已经校验过的笔记写进 `phantasi_items`。有 `item_id` 就改，没有就新建。
async fn write_published_item<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    item_id: Option<i32>,
    title: &str,
    content_md: &str,
    topic: Option<String>,
    image: Option<String>,
    published_at_ms: Option<i64>,
    author: Option<String>,
) -> Result<PublishedNote, HttpError> {
    let (rewritten_cover, rewritten_body) =
        crate::services::media::publish_cited_media(db, &[], image.as_deref(), content_md)
            .await
            .map_err(media_bind_http)?;
    let content_md = rewritten_body;
    let image = rewritten_cover.or(image);
    validate_note(title, &content_md).map_err(validation_err)?;
    let source = ensure_note_source(db, user_id).await?;
    let rendered = render_note(title, &content_md);
    let now = Utc::now();
    let topic = topic.filter(|value| !value.trim().is_empty());
    let cover = image
        .filter(|value| !value.trim().is_empty())
        .or(rendered.image);

    if let Some(id) = item_id {
        let item = phantasi_items::Entity::find_by_id(id)
            .one(db)
            .await
            .map_err(|e| phantasi_store_http("find note", e))?
            .ok_or_else(|| phantasi_http_err(StatusCode::NOT_FOUND, "Note not found"))?;
        if item.source_id != source.id {
            return Err(phantasi_http_err(StatusCode::NOT_FOUND, "Note not found"));
        }
        let mut active: phantasi_items::ActiveModel = item.into();
        active.title = Set(rendered.title);
        active.summary = Set(rendered.summary);
        active.content = Set(Some(rendered.html));
        active.content_md = Set(Some(content_md.to_string()));
        active.image = Set(cover);
        active.word_count = Set(Some(rendered.word_count));
        active.reading_time = Set(Some(rendered.reading_time));
        active.topic = Set(topic);
        active.author = Set(author);
        if let Some(published_at) = published_at_ms.and_then(millis_to_datetime) {
            active.published_at = Set(published_at.into());
        }
        let item = active
            .update(db)
            .await
            .map_err(|e| phantasi_store_http("save note", e))?;
        return Ok(PublishedNote {
            id: item.id,
            link: item.link,
        });
    }

    let published_at = published_at_ms.and_then(millis_to_datetime).unwrap_or(now);
    let new_item = phantasi_items::ActiveModel {
        source_id: Set(source.id),
        guid: Set(note_guid(&uuid::Uuid::new_v4().to_string())),
        title: Set(rendered.title),
        link: Set(String::new()),
        summary: Set(rendered.summary),
        content: Set(Some(rendered.html)),
        content_md: Set(Some(content_md.to_string())),
        image: Set(cover),
        published_at: Set(published_at.into()),
        fetched_at: Set(now.into()),
        word_count: Set(Some(rendered.word_count)),
        reading_time: Set(Some(rendered.reading_time)),
        fulltext_fetched: Set(true),
        topic: Set(topic),
        author: Set(author),
        ..Default::default()
    };
    let item = new_item
        .insert(db)
        .await
        .map_err(|e| phantasi_store_http("save note", e))?;
    let item_id = item.id;
    let mut active: phantasi_items::ActiveModel = item.into();
    active.link = Set(note_link(item_id));
    let item = active
        .update(db)
        .await
        .map_err(|e| phantasi_store_http("save note", e))?;
    sync_item_count(db, &source).await?;
    Ok(PublishedNote {
        id: item.id,
        link: item.link,
    })
}

async fn mark_doc_published<C: ConnectionTrait>(
    db: &C,
    mut doc: phantasi_note_docs::Model,
    item: &PublishedNote,
    published_at_ms: Option<i64>,
) -> Result<phantasi_note_docs::Model, HttpError> {
    let now = Utc::now();
    let published_at = published_at_ms
        .and_then(millis_to_datetime)
        .or_else(|| doc.published_at.map(|dt| dt.with_timezone(&Utc)))
        .unwrap_or(now);
    let expected = doc.revision;
    let mut active = <phantasi_note_docs::ActiveModel as std::default::Default>::default();
    active.item_id = Set(Some(item.id));
    active.status = Set(NoteDocStatus::Published.as_str().to_string());
    active.scheduled_at = Set(None);
    active.published_at = Set(Some(published_at.into()));
    active.last_error = Set(None);
    active.updated_at = Set(now.into());
    active.revision = Set(expected + 1);
    let result = phantasi_note_docs::Entity::update_many()
        .set(active)
        .filter(phantasi_note_docs::Column::Id.eq(doc.id))
        .filter(phantasi_note_docs::Column::Revision.eq(expected))
        .exec(db)
        .await
        .map_err(|e| phantasi_store_http("save note doc", e))?;
    if result.rows_affected == 0 {
        return Err(phantasi_http_err(
            StatusCode::CONFLICT,
            "Note draft was updated elsewhere",
        ));
    }
    doc.item_id = Some(item.id);
    doc.status = NoteDocStatus::Published.as_str().to_string();
    doc.scheduled_at = None;
    doc.published_at = Some(published_at.into());
    doc.last_error = None;
    doc.updated_at = now.into();
    doc.revision = expected + 1;
    Ok(doc)
}

/// 发布一篇云端文档：写 `phantasi_items` + 标文档已发布，一个事务。
///
/// `doc` 里的字段就是要发布的内容（调用方已把请求里的改动合进去）。
pub async fn publish_doc(
    db: &DatabaseConnection,
    doc: phantasi_note_docs::Model,
    published_at_ms: Option<i64>,
) -> Result<(PublishedNote, phantasi_note_docs::Model), HttpError> {
    let txn = db
        .begin()
        .await
        .map_err(|e| phantasi_store_http("begin note publish", e))?;
    let outcome = async {
        let author = crate::services::note_authors::note_author_line(&txn, doc.id).await?;
        let item = write_published_item(
            &txn,
            doc.user_id,
            doc.item_id,
            &doc.title,
            &doc.content_md,
            doc.topic.clone(),
            doc.image.clone(),
            published_at_ms,
            author,
        )
        .await?;
        let saved = mark_doc_published(&txn, doc, &item, published_at_ms).await?;
        crate::services::media::bind_note_draft(
            &txn,
            saved.id,
            saved.revision - 1,
            saved.image.as_deref(),
            &saved.content_md,
            &[],
        )
        .await
        .map_err(media_bind_http)?;
        crate::services::media::bind_note_published(
            &txn,
            item.id,
            saved.image.as_deref(),
            &saved.content_md,
            &[],
        )
        .await
        .map_err(media_bind_http)?;
        Ok::<_, HttpError>((item, saved))
    }
    .await;
    match outcome {
        Ok(result) => {
            txn.commit()
                .await
                .map_err(|e| phantasi_store_http("commit note publish", e))?;
            Ok(result)
        }
        Err(error) => {
            if let Err(rollback) = txn.rollback().await {
                tracing::warn!(error = %rollback, "note publish rollback failed");
            }
            Err(error)
        }
    }
}

/// Delete the public article and its editor document through one owner.
pub async fn delete_note_with_doc(
    db: &DatabaseConnection,
    item_id: i32,
    source: &phantasi_sources::Model,
) -> Result<(), HttpError> {
    let txn = db
        .begin()
        .await
        .map_err(|e| phantasi_store_http("begin note delete", e))?;
    let outcome = async {
        let docs = phantasi_note_docs::Entity::find()
            .filter(phantasi_note_docs::Column::ItemId.eq(item_id))
            .all(&txn)
            .await
            .map_err(|e| phantasi_store_http("find note docs for delete", e))?;
        for doc in docs {
            crate::services::media::clear_note_doc(&txn, doc.id, Some(item_id))
                .await
                .map_err(media_bind_http)?;
        }
        crate::services::media::bind_note_published(&txn, item_id, None, "", &[])
            .await
            .map_err(media_bind_http)?;
        // Match publication and metadata lock order: article, then document.
        phantasi_items::Entity::delete_by_id(item_id)
            .exec(&txn)
            .await
            .map_err(|e| phantasi_store_http("delete note", e))?;
        phantasi_note_docs::Entity::delete_many()
            .filter(phantasi_note_docs::Column::ItemId.eq(item_id))
            .exec(&txn)
            .await
            .map_err(|e| phantasi_store_http("delete note doc", e))?;
        sync_item_count(&txn, source).await?;
        Ok::<_, HttpError>(())
    }
    .await;
    match outcome {
        Ok(()) => txn
            .commit()
            .await
            .map_err(|e| phantasi_store_http("commit note delete", e)),
        Err(error) => {
            if let Err(rollback) = txn.rollback().await {
                tracing::warn!(error = %rollback, "note delete rollback failed");
            }
            Err(error)
        }
    }
}

/// Change note classification without publishing the editor's draft body.
/// The document revision owns the linked article metadata write as well.
pub async fn update_note_doc_topic(
    db: &DatabaseConnection,
    doc_id: i32,
    expected_revision: i64,
    topic: Option<String>,
) -> Result<phantasi_note_docs::Model, HttpError> {
    let txn = db
        .begin()
        .await
        .map_err(|e| phantasi_store_http("begin note topic update", e))?;
    let outcome = async {
        let doc = phantasi_note_docs::Entity::find_by_id(doc_id)
            .one(&txn)
            .await
            .map_err(|e| phantasi_store_http("find note doc", e))?
            .ok_or_else(|| phantasi_http_err(StatusCode::NOT_FOUND, "Note draft not found"))?;
        if doc.revision != expected_revision {
            return Err(phantasi_http_err(
                StatusCode::CONFLICT,
                "Note draft was updated elsewhere",
            ));
        }
        if let Some(item_id) = doc.item_id {
            let mut item = <phantasi_items::ActiveModel as Default>::default();
            item.topic = Set(topic.clone());
            let updated = phantasi_items::Entity::update_many()
                .set(item)
                .filter(phantasi_items::Column::Id.eq(item_id))
                .exec(&txn)
                .await
                .map_err(|e| phantasi_store_http("update published note topic", e))?;
            if updated.rows_affected == 0 {
                return Err(phantasi_http_err(StatusCode::NOT_FOUND, "Note not found"));
            }
        }
        // All published-note writers acquire article before document: publish/delete use
        // the same order. A failed revision CAS rolls back the metadata update above.
        let now = Utc::now();
        let mut active = <phantasi_note_docs::ActiveModel as Default>::default();
        active.topic = Set(topic.clone());
        active.revision = Set(expected_revision + 1);
        active.updated_at = Set(now.into());
        let updated = phantasi_note_docs::Entity::update_many()
            .set(active)
            .filter(phantasi_note_docs::Column::Id.eq(doc_id))
            .filter(phantasi_note_docs::Column::Revision.eq(expected_revision))
            .exec(&txn)
            .await
            .map_err(|e| phantasi_store_http("update note topic", e))?;
        if updated.rows_affected == 0 {
            return Err(phantasi_http_err(
                StatusCode::CONFLICT,
                "Note draft was updated elsewhere",
            ));
        }
        crate::services::media::sync_note_history_refs(&txn, doc_id, expected_revision, &[])
            .await
            .map_err(media_bind_http)?;
        phantasi_note_docs::Entity::find_by_id(doc_id)
            .one(&txn)
            .await
            .map_err(|e| phantasi_store_http("reload note doc", e))?
            .ok_or_else(|| phantasi_http_err(StatusCode::NOT_FOUND, "Note draft not found"))
    }
    .await;
    match outcome {
        Ok(doc) => {
            txn.commit()
                .await
                .map_err(|e| phantasi_store_http("commit note topic update", e))?;
            Ok(doc)
        }
        Err(error) => {
            if let Err(rollback) = txn.rollback().await {
                tracing::warn!(error = %rollback, "note topic rollback failed");
            }
            Err(error)
        }
    }
}

/// 认领一篇到点的定时稿：只在 status/revision 都没变时把 revision 推一格。
/// 推不动说明别的实例（或用户）先动了它，这一轮跳过。
async fn claim_due_doc(
    db: &DatabaseConnection,
    doc: &phantasi_note_docs::Model,
) -> Result<Option<phantasi_note_docs::Model>, HttpError> {
    let claimed_revision = doc.revision + 1;
    let result = phantasi_note_docs::Entity::update_many()
        .col_expr(
            phantasi_note_docs::Column::Revision,
            sea_orm::sea_query::Expr::value(claimed_revision),
        )
        .col_expr(
            phantasi_note_docs::Column::UpdatedAt,
            sea_orm::sea_query::Expr::value(Utc::now()),
        )
        .filter(phantasi_note_docs::Column::Id.eq(doc.id))
        .filter(phantasi_note_docs::Column::Status.eq(NoteDocStatus::Scheduled.as_str()))
        .filter(phantasi_note_docs::Column::Revision.eq(doc.revision))
        .exec(db)
        .await
        .map_err(|e| phantasi_store_http("claim scheduled note", e))?;
    if result.rows_affected == 0 {
        return Ok(None);
    }
    let mut claimed = doc.clone();
    claimed.revision = claimed_revision;
    Ok(Some(claimed))
}

/// 调度器：把到点的定时稿写成公开文章。
pub async fn publish_due_note_docs(db: &DatabaseConnection) -> Result<usize, String> {
    let now = Utc::now();
    let rows = phantasi_note_docs::Entity::find()
        .filter(phantasi_note_docs::Column::Status.eq(NoteDocStatus::Scheduled.as_str()))
        .filter(phantasi_note_docs::Column::ScheduledAt.lte(now))
        .all(db)
        .await
        .map_err(|e| format!("list due note docs: {e}"))?;

    let mut published = 0;
    for doc in rows {
        let scheduled_ms = doc.scheduled_at.map(datetime_to_millis);
        if !is_due(
            NoteDocStatus::parse(&doc.status).unwrap_or(NoteDocStatus::Draft),
            scheduled_ms,
            now.timestamp_millis(),
        ) {
            continue;
        }
        let doc = match claim_due_doc(db, &doc).await {
            Ok(Some(claimed)) => claimed,
            Ok(None) => continue,
            Err(error) => {
                tracing::error!(error = ?error, doc_id = doc.id, "failed to claim scheduled note");
                continue;
            }
        };
        let published_at = scheduled_ms.or(doc.published_at.map(datetime_to_millis));
        let doc_id = doc.id;
        match publish_doc(db, doc.clone(), published_at).await {
            Ok(_) => published += 1,
            Err(error) => {
                tracing::error!(error = ?error, doc_id, "failed to publish scheduled note");
                // 409 是发布权已转移，不再写；其它 4xx 仅能退回仍由本次认领的稿件。
                // 5xx 保持 scheduled 下一轮再试。
                if error.0.status().is_client_error() && error.0.status() != StatusCode::CONFLICT {
                    if let Err(revert) = revert_due_doc(db, doc, error.0.error_label()).await {
                        tracing::error!(error = ?revert, "failed to revert scheduled note");
                    }
                }
            }
        }
    }
    Ok(published)
}

async fn revert_due_doc(
    db: &DatabaseConnection,
    doc: phantasi_note_docs::Model,
    label: &str,
) -> Result<(), HttpError> {
    let mut active = <phantasi_note_docs::ActiveModel as Default>::default();
    active.status = Set(NoteDocStatus::Draft.as_str().to_string());
    active.last_error = Set(Some(label.to_string()));
    active.updated_at = Set(Utc::now().into());
    active.revision = Set(doc.revision + 1);
    // A user edit, reschedule, or another publisher invalidates this worker's ownership.
    // Zero rows means the current owner decides the state; never undo their work.
    phantasi_note_docs::Entity::update_many()
        .set(active)
        .filter(phantasi_note_docs::Column::Id.eq(doc.id))
        .filter(phantasi_note_docs::Column::Status.eq(NoteDocStatus::Scheduled.as_str()))
        .filter(phantasi_note_docs::Column::Revision.eq(doc.revision))
        .exec(db)
        .await
        .map_err(|e| phantasi_store_http("revert scheduled note", e))?;
    Ok(())
}

pub async fn upsert_doc_for_published_item<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    item: &PublishedNote,
    title: &str,
    content_md: &str,
    topic: Option<String>,
    image: Option<String>,
    published_at_ms: Option<i64>,
) -> Result<phantasi_note_docs::Model, HttpError> {
    let now = Utc::now();
    let published_at = published_at_ms.and_then(millis_to_datetime);
    if let Some(existing) = phantasi_note_docs::Entity::find()
        .filter(phantasi_note_docs::Column::ItemId.eq(item.id))
        .one(db)
        .await
        .map_err(|e| phantasi_store_http("find note doc", e))?
    {
        let mut active: phantasi_note_docs::ActiveModel = existing.clone().into();
        active.title = Set(title.to_string());
        active.content_md = Set(content_md.to_string());
        active.topic = Set(topic.filter(|value| !value.trim().is_empty()));
        active.image = Set(image.filter(|value| !value.trim().is_empty()));
        active.status = Set(NoteDocStatus::Published.as_str().to_string());
        active.last_error = Set(None);
        if let Some(at) = published_at {
            active.published_at = Set(Some(at.into()));
        }
        active.updated_at = Set(now.into());
        active.revision = Set(existing.revision + 1);
        return active
            .update(db)
            .await
            .map_err(|e| phantasi_store_http("save note doc", e));
    }

    let doc = phantasi_note_docs::ActiveModel {
        user_id: Set(user_id),
        item_id: Set(Some(item.id)),
        title: Set(title.to_string()),
        content_md: Set(content_md.to_string()),
        topic: Set(topic.filter(|value| !value.trim().is_empty())),
        image: Set(image.filter(|value| !value.trim().is_empty())),
        status: Set(NoteDocStatus::Published.as_str().to_string()),
        published_at: Set(published_at.map(|at| at.into())),
        revision: Set(1),
        created_at: Set(now.into()),
        updated_at: Set(now.into()),
        ..Default::default()
    };
    doc.insert(db)
        .await
        .map_err(|e| phantasi_store_http("create note doc", e))
}

/// 删订阅源会 CASCADE 掉文章；笔记文档没有这条外键，先把指向这些文章的文档解开。
pub async fn detach_note_docs_for_source<C: ConnectionTrait>(
    db: &C,
    source_id: i32,
) -> Result<(), HttpError> {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"
        UPDATE phantasi_note_docs AS d
        SET item_id = NULL,
            status = CASE WHEN d.status = 'published' THEN 'draft' ELSE d.status END,
            revision = d.revision + 1,
            updated_at = NOW()
        WHERE d.item_id IN (SELECT id FROM phantasi_items WHERE source_id = $1)
        "#,
        [SeaValue::Int(Some(source_id))],
    ))
    .await
    .map_err(|e| phantasi_store_http("detach note docs for source", e))?;
    Ok(())
}

/// 公开笔记的写入 + 对应云端文档同步，一个事务。`create_note` / `update_note` 用。
pub async fn write_note_with_doc(
    db: &DatabaseConnection,
    user_id: i32,
    item_id: Option<i32>,
    title: &str,
    content_md: &str,
    topic: Option<String>,
    image: Option<String>,
    published_at_ms: Option<i64>,
) -> Result<PublishedNote, HttpError> {
    let txn = db
        .begin()
        .await
        .map_err(|e| phantasi_store_http("begin note write", e))?;
    let outcome = async {
        let item = write_published_item(
            &txn,
            user_id,
            item_id,
            title,
            content_md,
            topic.clone(),
            image.clone(),
            published_at_ms,
            None,
        )
        .await?;
        let doc = upsert_doc_for_published_item(
            &txn,
            user_id,
            &item,
            title,
            content_md,
            topic,
            image,
            published_at_ms,
        )
        .await?;
        crate::services::note_authors::ensure_note_author(&txn, doc.id, user_id, doc.user_id)
            .await?;
        crate::services::note_authors::sync_published_author_line(&txn, Some(item.id), doc.id)
            .await?;
        crate::services::media::bind_note_draft(
            &txn,
            doc.id,
            doc.revision - 1,
            doc.image.as_deref(),
            &doc.content_md,
            &[],
        )
        .await
        .map_err(media_bind_http)?;
        crate::services::media::bind_note_published(
            &txn,
            item.id,
            doc.image.as_deref(),
            &doc.content_md,
            &[],
        )
        .await
        .map_err(media_bind_http)?;
        Ok::<_, HttpError>(item)
    }
    .await;
    match outcome {
        Ok(item) => {
            txn.commit()
                .await
                .map_err(|e| phantasi_store_http("commit note write", e))?;
            Ok(item)
        }
        Err(error) => {
            if let Err(rollback) = txn.rollback().await {
                tracing::warn!(error = %rollback, "note write rollback failed");
            }
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn isolated_note_db() -> Option<DatabaseConnection> {
        use sea_orm::{ConnectOptions, Database, Schema};
        let url = std::env::var("PHANTASI_TEST_DATABASE_URL").ok()?;
        let mut options = ConnectOptions::new(url);
        options
            .max_connections(1)
            .min_connections(1)
            .sqlx_logging(false);
        let db = Database::connect(options)
            .await
            .expect("connect note test database");
        let schema = Schema::new(DatabaseBackend::Postgres);
        for statement in [
            schema.create_table_from_entity(phantasi_sources::Entity),
            schema.create_table_from_entity(phantasi_note_docs::Entity),
            schema.create_table_from_entity(phantasi_items::Entity),
            schema.create_table_from_entity(crate::models::entities::media_assets::Entity),
            schema.create_table_from_entity(crate::models::entities::media_references::Entity),
        ] {
            let sql = statement
                .to_string(sea_orm::sea_query::PostgresQueryBuilder)
                .replacen("CREATE TABLE", "CREATE TEMP TABLE", 1);
            db.execute_unprepared(&sql)
                .await
                .expect("create isolated temporary table");
        }
        db.execute_unprepared(
            "CREATE TEMP TABLE phantasi_note_history (
            doc_id integer, revision bigint, snapshot jsonb
        )",
        )
        .await
        .unwrap();
        db.execute_unprepared(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_phantasi_sources_note_type \
             ON phantasi_sources ((true)) WHERE source_type = 'note'",
        )
        .await
        .expect("note source unique");
        insert_test_source(&db).await;
        Some(db)
    }

    async fn insert_test_source(db: &DatabaseConnection) {
        let now = Utc::now();
        phantasi_sources::ActiveModel {
            user_id: Set(1),
            name: Set("Notes".into()),
            url: Set("myriad:notes".into()),
            feed_type: Set(phantasi_sources::FeedType::Rss),
            source_type: Set(phantasi_sources::SourceType::Note),
            update_interval: Set(0),
            enabled: Set(true),
            error_count: Set(0),
            item_count: Set(1),
            unread_count: Set(0),
            admin_only: Set(false),
            created_at: Set(now.into()),
            updated_at: Set(now.into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
    }

    async fn insert_test_doc(db: &DatabaseConnection) -> phantasi_note_docs::Model {
        let now = Utc::now();
        phantasi_note_docs::ActiveModel {
            user_id: Set(1),
            item_id: Set(None),
            title: Set("Draft title".into()),
            content_md: Set("Unpublished body".into()),
            topic: Set(Some("Old".into())),
            image: Set(Some("draft.png".into())),
            status: Set("scheduled".into()),
            scheduled_at: Set(Some(now.into())),
            published_at: Set(Some(now.into())),
            revision: Set(10),
            last_error: Set(None),
            created_at: Set(now.into()),
            updated_at: Set(now.into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn insert_test_item(db: &DatabaseConnection) -> phantasi_items::Model {
        let now = Utc::now();
        phantasi_items::ActiveModel {
            source_id: Set(1),
            guid: Set(format!("test-note-{}", uuid::Uuid::new_v4())),
            title: Set("Published title".into()),
            link: Set("/journal/articles/1".into()),
            content: Set(Some("<p>Published body</p>".into())),
            content_md: Set(Some("Published body".into())),
            image: Set(Some("published.png".into())),
            topic: Set(Some("Old".into())),
            published_at: Set(now.into()),
            fetched_at: Set(now.into()),
            fulltext_fetched: Set(true),
            content_revision: Set(7),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn scheduled_failure_cannot_revert_a_newer_user_change() {
        for (status, revision) in [("scheduled", 14), ("published", 10)] {
            let Some(db) = isolated_note_db().await else {
                return;
            };
            let claimed = insert_test_doc(&db).await;
            let mut changed: phantasi_note_docs::ActiveModel = claimed.clone().into();
            changed.status = Set(status.into());
            changed.revision = Set(revision);
            let newer = changed.update(&db).await.unwrap();
            revert_due_doc(&db, claimed, "stale publication")
                .await
                .unwrap();
            let saved = phantasi_note_docs::Entity::find_by_id(newer.id)
                .one(&db)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                saved, newer,
                "a stale scheduler must not mutate the new owner state"
            );
        }
    }

    #[tokio::test]
    async fn concurrent_note_count_writes_follow_committed_item_changes() {
        use sea_orm::{ConnectOptions, Database, QuerySelect, Schema};
        let Ok(url) = std::env::var("PHANTASI_TEST_DATABASE_URL") else {
            return;
        };
        let admin = Database::connect(&url).await.unwrap();
        let scope = format!("journal_count_{}", uuid::Uuid::new_v4().simple());
        admin
            .execute_unprepared(&format!("CREATE SCHEMA {scope}"))
            .await
            .unwrap();
        let connect = || {
            let mut options = ConnectOptions::new(url.clone());
            options
                .max_connections(1)
                .min_connections(1)
                .sqlx_logging(false)
                .set_schema_search_path(scope.clone());
            Database::connect(options)
        };
        let a = connect().await.unwrap();
        let b = connect().await.unwrap();
        let blocker = connect().await.unwrap();
        let schema = Schema::new(DatabaseBackend::Postgres);
        for statement in [
            schema.create_table_from_entity(phantasi_sources::Entity),
            schema.create_table_from_entity(phantasi_note_docs::Entity),
            schema.create_table_from_entity(phantasi_items::Entity),
        ] {
            a.execute_unprepared(&statement.to_string(sea_orm::sea_query::PostgresQueryBuilder))
                .await
                .unwrap();
        }
        insert_test_source(&a).await;
        let first = insert_test_item(&a).await;
        let second = insert_test_item(&a).await;
        let source = phantasi_sources::Entity::find_by_id(1)
            .one(&a)
            .await
            .unwrap()
            .unwrap();
        let mut initial: phantasi_sources::ActiveModel = source.clone().into();
        initial.item_count = Set(2);
        initial.update(&a).await.unwrap();
        let pid_a: i32 = a
            .query_one_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                "SELECT pg_backend_pid() AS pid",
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get("", "pid")
            .unwrap();
        let pid_b: i32 = b
            .query_one_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                "SELECT pg_backend_pid() AS pid",
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get("", "pid")
            .unwrap();
        let held = blocker.begin().await.unwrap();
        phantasi_sources::Entity::find_by_id(1)
            .lock(sea_orm::sea_query::LockType::NoKeyUpdate)
            .one(&held)
            .await
            .unwrap();
        let (delete_a, delete_b) = {
            let (a, b, sa, sb) = (a.clone(), b.clone(), source.clone(), source.clone());
            (
                tokio::spawn(async move { delete_note_with_doc(&a, first.id, &sa).await }),
                tokio::spawn(async move { delete_note_with_doc(&b, second.id, &sb).await }),
            )
        };
        let mut both_blocked = false;
        for _ in 0..500 {
            let row = admin.query_one_raw(Statement::from_sql_and_values(DatabaseBackend::Postgres,
                "SELECT count(*) AS n FROM pg_stat_activity WHERE pid IN ($1, $2) AND wait_event_type = 'Lock'",
                [pid_a.into(), pid_b.into()])).await.unwrap().unwrap();
            if row.try_get::<i64>("", "n").unwrap() == 2 {
                both_blocked = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        held.commit().await.unwrap();
        let deletion_results = (delete_a.await.unwrap(), delete_b.await.unwrap());
        let after_delete = phantasi_sources::Entity::find_by_id(1)
            .one(&a)
            .await
            .unwrap()
            .unwrap()
            .item_count;

        // Each insertion holds a foreign-key KEY SHARE on the same source. The
        // count lock must coexist with those locks rather than upgrade to FOR UPDATE.
        let left = a.begin().await.unwrap();
        let right = b.begin().await.unwrap();
        let mut added: phantasi_items::ActiveModel = first.into();
        added.id = sea_orm::ActiveValue::NotSet;
        added.guid = Set(format!("test-note-{}", uuid::Uuid::new_v4()));
        phantasi_items::Entity::insert(added.clone().reset_all())
            .exec(&left)
            .await
            .unwrap();
        added.guid = Set(format!("test-note-{}", uuid::Uuid::new_v4()));
        phantasi_items::Entity::insert(added.reset_all())
            .exec(&right)
            .await
            .unwrap();
        let additions = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            tokio::join!(
                async {
                    sync_item_count(&left, &source).await?;
                    left.commit()
                        .await
                        .map_err(|e| phantasi_store_http("commit test insert", e))
                },
                async {
                    sync_item_count(&right, &source).await?;
                    right
                        .commit()
                        .await
                        .map_err(|e| phantasi_store_http("commit test insert", e))
                }
            )
        })
        .await;
        let after_add = phantasi_sources::Entity::find_by_id(1)
            .one(&a)
            .await
            .unwrap()
            .unwrap()
            .item_count;
        a.close().await.unwrap();
        b.close().await.unwrap();
        blocker.close().await.unwrap();
        admin
            .execute_unprepared(&format!("DROP SCHEMA {scope} CASCADE"))
            .await
            .unwrap();
        assert!(
            both_blocked,
            "both deletes must reach the controlled count-write interleaving"
        );
        assert!(deletion_results.0.is_ok() && deletion_results.1.is_ok());
        assert_eq!(
            after_delete, 0,
            "concurrent deletes must count both committed removals"
        );
        let additions = additions.expect("count locks must not deadlock with source FK locks");
        assert!(additions.0.is_ok() && additions.1.is_ok());
        assert_eq!(after_add, 2, "both committed inserts must be counted");
    }

    #[tokio::test]
    async fn failed_note_doc_delete_preserves_article_document_and_count() {
        let Some(db) = isolated_note_db().await else {
            return;
        };
        let source = phantasi_sources::Entity::find_by_id(1)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        let item = insert_test_item(&db).await;
        let doc = insert_test_doc(&db).await;
        let mut linked: phantasi_note_docs::ActiveModel = doc.into();
        linked.item_id = Set(Some(item.id));
        let doc = linked.update(&db).await.unwrap();
        db.execute_unprepared("CREATE TEMP TABLE protected_note_doc (doc_id INTEGER REFERENCES phantasi_note_docs(id)); INSERT INTO protected_note_doc VALUES (1)").await.unwrap();
        assert!(delete_note_with_doc(&db, item.id, &source).await.is_err());
        assert_eq!(
            phantasi_items::Entity::find_by_id(item.id)
                .one(&db)
                .await
                .unwrap(),
            Some(item.clone()),
            "a failed document delete must restore the already-deleted public article"
        );
        assert_eq!(
            phantasi_note_docs::Entity::find_by_id(doc.id)
                .one(&db)
                .await
                .unwrap(),
            Some(doc.clone())
        );
        assert_eq!(
            phantasi_sources::Entity::find_by_id(source.id)
                .one(&db)
                .await
                .unwrap()
                .unwrap()
                .item_count,
            1
        );
        db.execute_unprepared("DROP TABLE protected_note_doc")
            .await
            .unwrap();
        db.execute_unprepared(
            "ALTER TABLE phantasi_sources ADD CONSTRAINT protect_note_count CHECK (item_count > 0)",
        )
        .await
        .unwrap();
        assert!(
            delete_note_with_doc(&db, item.id, &source).await.is_err(),
            "count persistence failure must not report successful deletion"
        );
        assert!(
            phantasi_items::Entity::find_by_id(item.id)
                .one(&db)
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            phantasi_note_docs::Entity::find_by_id(doc.id)
                .one(&db)
                .await
                .unwrap()
                .is_some()
        );
        db.execute_unprepared("ALTER TABLE phantasi_sources DROP CONSTRAINT protect_note_count")
            .await
            .unwrap();
        delete_note_with_doc(&db, item.id, &source).await.unwrap();
        assert!(
            phantasi_items::Entity::find_by_id(item.id)
                .one(&db)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            phantasi_note_docs::Entity::find_by_id(doc.id)
                .one(&db)
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(
            phantasi_sources::Entity::find_by_id(source.id)
                .one(&db)
                .await
                .unwrap()
                .unwrap()
                .item_count,
            0
        );
    }

    #[tokio::test]
    async fn note_topic_changes_preserve_both_bodies_and_reject_stale_revision() {
        let Some(db) = isolated_note_db().await else {
            return;
        };
        let doc = insert_test_doc(&db).await;
        let item = insert_test_item(&db).await;
        let mut linked: phantasi_note_docs::ActiveModel = doc.into();
        linked.item_id = Set(Some(item.id));
        linked.status = Set("published".into());
        let doc = linked.update(&db).await.unwrap();
        let saved = update_note_doc_topic(&db, doc.id, doc.revision, Some("New".into()))
            .await
            .unwrap();
        let mut expected_doc = doc.clone();
        expected_doc.topic = Some("New".into());
        expected_doc.revision += 1;
        expected_doc.updated_at = saved.updated_at;
        assert_eq!(
            saved, expected_doc,
            "metadata edit must preserve the unpublished draft"
        );
        let saved_item = phantasi_items::Entity::find_by_id(item.id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        let mut expected_item = item;
        expected_item.topic = Some("New".into());
        assert_eq!(
            saved_item, expected_item,
            "metadata edit must not publish the draft body"
        );
        let conflict = update_note_doc_topic(&db, doc.id, doc.revision, None)
            .await
            .unwrap_err();
        assert_eq!(conflict.0.status(), StatusCode::CONFLICT);
        assert_eq!(
            phantasi_note_docs::Entity::find_by_id(doc.id)
                .one(&db)
                .await
                .unwrap()
                .unwrap(),
            saved
        );
        let cleared = update_note_doc_topic(&db, doc.id, saved.revision, None)
            .await
            .unwrap();
        assert_eq!(cleared.topic, None);
        db.execute_unprepared("ALTER TABLE phantasi_note_docs ADD CONSTRAINT fail_topic_update CHECK (topic IS DISTINCT FROM 'Rejected')").await.unwrap();
        assert!(
            update_note_doc_topic(&db, doc.id, cleared.revision, Some("Rejected".into()))
                .await
                .is_err()
        );
        assert_eq!(
            phantasi_note_docs::Entity::find_by_id(doc.id)
                .one(&db)
                .await
                .unwrap()
                .unwrap(),
            cleared
        );

        assert_eq!(
            phantasi_items::Entity::find_by_id(saved_item.id)
                .one(&db)
                .await
                .unwrap()
                .unwrap()
                .topic,
            None
        );
    }

    #[tokio::test]
    async fn note_topic_missing_article_rolls_back_document_metadata() {
        let Some(db) = isolated_note_db().await else {
            return;
        };
        let doc = insert_test_doc(&db).await;
        let mut linked: phantasi_note_docs::ActiveModel = doc.into();
        linked.item_id = Set(Some(999));
        let before = linked.update(&db).await.unwrap();
        assert!(
            update_note_doc_topic(&db, before.id, before.revision, None)
                .await
                .is_err()
        );
        let after = phantasi_note_docs::Entity::find_by_id(before.id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            after, before,
            "failed article update must roll back the document CAS"
        );
    }

    #[tokio::test]
    async fn note_topic_on_scheduled_doc_keeps_schedule_and_does_not_publish() {
        let Some(db) = isolated_note_db().await else {
            return;
        };
        let before = insert_test_doc(&db).await;
        let after = update_note_doc_topic(&db, before.id, before.revision, None)
            .await
            .unwrap();
        assert_eq!(after.item_id, None);
        assert_eq!(after.status, "scheduled");
        assert_eq!(after.scheduled_at, before.scheduled_at);
        assert_eq!(after.content_md, before.content_md);
        assert_eq!(phantasi_items::Entity::find().count(&db).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn scheduled_failure_reverts_only_its_owned_revision() {
        let Some(db) = isolated_note_db().await else {
            return;
        };
        let claimed = insert_test_doc(&db).await;
        revert_due_doc(&db, claimed.clone(), "invalid note")
            .await
            .unwrap();
        let saved = phantasi_note_docs::Entity::find_by_id(claimed.id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(saved.status, "draft");
        assert_eq!(saved.revision, claimed.revision + 1);
        assert_eq!(saved.last_error.as_deref(), Some("invalid note"));
        assert_eq!(saved.content_md, claimed.content_md);
    }

    #[test]
    fn note_source_is_shared_catalog_not_creator_keyed() {
        let src = include_str!("note_publish.rs");
        let start = src
            .find("async fn ensure_note_source")
            .expect("ensure_note_source");
        let body = &src[start..];
        let end = body[1..]
            .find("\nasync fn ")
            .map(|index| index + 1)
            .unwrap_or(body.len());
        let finder = &body[..end];
        assert!(finder.contains("SourceType::Note"));
        assert!(finder.contains("unique_violation"));
        assert!(
            !finder.contains("UserId.eq"),
            "shared note source must not be keyed by the creating admin"
        );
    }

    #[tokio::test]
    async fn ensure_note_source_collapses_concurrent_first_publish() {
        let Some(db) = isolated_note_db().await else {
            return;
        };
        phantasi_sources::Entity::delete_many()
            .exec(&db)
            .await
            .unwrap();
        let first = super::ensure_note_source(&db, 11).await.unwrap();
        let second = super::ensure_note_source(&db, 22).await.unwrap();
        assert_eq!(first.id, second.id);
        assert_eq!(
            phantasi_sources::Entity::find()
                .filter(phantasi_sources::Column::SourceType.eq(phantasi_sources::SourceType::Note))
                .count(&db)
                .await
                .unwrap(),
            1
        );
        let duplicate = phantasi_sources::ActiveModel {
            user_id: Set(33),
            name: Set("Notes".into()),
            url: Set("myriad:notes-other".into()),
            feed_type: Set(phantasi_sources::FeedType::Rss),
            source_type: Set(phantasi_sources::SourceType::Note),
            update_interval: Set(0),
            enabled: Set(true),
            error_count: Set(0),
            item_count: Set(0),
            unread_count: Set(0),
            admin_only: Set(false),
            created_at: Set(Utc::now().into()),
            updated_at: Set(Utc::now().into()),
            ..Default::default()
        }
        .insert(&db)
        .await;
        assert!(
            duplicate.is_err(),
            "the note catalog unique index must reject a second Note source"
        );
    }

    #[test]
    fn millis_round_trip_for_doc_timestamps() {
        let ms = 1_700_000_000_000;
        let dt = millis_to_datetime(ms).expect("valid millis");
        assert_eq!(datetime_to_millis(dt.into()), ms);
    }

    fn body_of(src: &str, signature: &str) -> String {
        let start = src.find(signature).expect(signature);
        let body = &src[start..];
        let end = body[1..]
            .find("\npub async fn ")
            .or_else(|| body[1..].find("\nasync fn "))
            .map(|index| index + 1)
            .unwrap_or(body.len());
        body[..end].to_string()
    }

    #[test]
    fn due_publish_claims_then_publishes_and_reverts_client_failures() {
        let src = include_str!("note_publish.rs");
        let due = body_of(src, "pub async fn publish_due_note_docs");
        assert!(
            due.contains("claim_due_doc"),
            "must claim before publishing"
        );
        assert!(
            due.contains("publish_doc("),
            "must go through the transactional path"
        );
        assert!(due.contains("is_client_error"));
        assert!(due.contains("revert_due_doc"));
        let revert = body_of(src, "async fn revert_due_doc");
        assert!(
            revert.contains("NoteDocStatus::Draft"),
            "4xx must put the doc back to draft"
        );
        assert!(revert.contains("last_error"));
    }

    #[test]
    fn claim_is_conditional_on_status_and_revision() {
        let src = include_str!("note_publish.rs");
        let claim = body_of(src, "async fn claim_due_doc");
        assert!(claim.contains("Column::Status.eq(NoteDocStatus::Scheduled"));
        assert!(claim.contains("Column::Revision.eq(doc.revision)"));
        assert!(claim.contains("rows_affected == 0"));
    }

    #[test]
    fn source_delete_unpublishes_docs_before_items_cascade() {
        let src = include_str!("note_publish.rs");
        let detach = body_of(src, "pub async fn detach_note_docs_for_source");
        assert!(
            detach.contains("item_id = NULL"),
            "must clear the dangling article id"
        );
        assert!(
            detach.contains("THEN 'draft'"),
            "published docs without an article must go back to draft"
        );
        let delete = include_str!("../api/phantasi/feeds_sources.rs");
        let start = delete
            .find("pub(crate) async fn delete_source")
            .expect("delete_source");
        let body = &delete[start..];
        assert!(
            body.contains("detach_note_docs_for_source"),
            "deleting a source must detach note docs first"
        );
        assert!(
            body.contains(".begin()"),
            "source delete must be transactional"
        );
        let heal = include_str!("../db/schema_check/ensure_heals.rs");
        assert!(
            heal.contains("AND NOT EXISTS (SELECT 1 FROM phantasi_items i WHERE i.id = d.item_id)"),
            "boot heal must unpublish docs whose article is already gone"
        );
    }

    #[test]
    fn publish_and_write_run_in_a_transaction() {
        let src = include_str!("note_publish.rs");
        for signature in [
            "pub async fn publish_doc",
            "pub async fn write_note_with_doc",
        ] {
            let body = body_of(src, signature);
            assert!(
                body.contains(".begin()"),
                "{signature} must open a transaction"
            );
            assert!(body.contains("commit()"), "{signature} must commit");
            assert!(
                body.contains("rollback()"),
                "{signature} must roll back on error"
            );
        }
    }
}
