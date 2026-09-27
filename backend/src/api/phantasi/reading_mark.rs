//! Phantasi per-item and bulk reading marks.
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use chrono::Utc;
#[cfg(test)]
use sea_orm::QueryTrait;
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseBackend, DatabaseConnection, EntityTrait, QueryFilter,
    QuerySelect, Statement, TransactionTrait,
};
use serde_json::json;

use crate::error::HttpError;
use crate::extract::OptionalViewer;
use crate::models::entities::{phantasi_sources, phantasi_user_states};

use super::helpers::{get_phantasi_user_and_admin_status, phantasi_http_err, phantasi_store_http};

// 阅读状态

pub(crate) async fn mark_read(
    State(db): State<DatabaseConnection>,
    viewer: OptionalViewer,
    Path(item_id): Path<i32>,
) -> Result<Json<serde_json::Value>, HttpError> {
    update_item_state(&db, &viewer, item_id, Some(true), None).await
}

pub(crate) async fn mark_unread(
    State(db): State<DatabaseConnection>,
    viewer: OptionalViewer,
    Path(item_id): Path<i32>,
) -> Result<Json<serde_json::Value>, HttpError> {
    update_item_state(&db, &viewer, item_id, Some(false), None).await
}

pub(crate) async fn star_item(
    State(db): State<DatabaseConnection>,
    viewer: OptionalViewer,
    Path(item_id): Path<i32>,
) -> Result<Json<serde_json::Value>, HttpError> {
    update_item_state(&db, &viewer, item_id, None, Some(true)).await
}

pub(crate) async fn unstar_item(
    State(db): State<DatabaseConnection>,
    viewer: OptionalViewer,
    Path(item_id): Path<i32>,
) -> Result<Json<serde_json::Value>, HttpError> {
    update_item_state(&db, &viewer, item_id, None, Some(false)).await
}

pub(crate) use crate::services::phantasi_reading::visible_state_sources;

pub(crate) async fn update_item_state(
    db: &DatabaseConnection,
    viewer: &OptionalViewer,
    item_id: i32,
    is_read: Option<bool>,
    is_starred: Option<bool>,
) -> Result<Json<serde_json::Value>, HttpError> {
    use crate::services::phantasi_reading::{MarkStateError, mark_item_state};
    let (user_id, is_admin) = get_phantasi_user_and_admin_status(viewer, db).await?;
    if is_starred.is_some() && !is_admin {
        return Err(phantasi_http_err(StatusCode::FORBIDDEN, "Forbidden"));
    }
    match mark_item_state(db, user_id, is_admin, item_id, is_read, is_starred).await {
        Ok(marked) => Ok(Json(json!({
            "success": true,
            "previous_revision": marked.previous_revision,
            "revision": marked.revision,
        }))),
        Err(MarkStateError::NotVisible) => {
            Err(phantasi_http_err(StatusCode::NOT_FOUND, "Item not found"))
        }
        Err(MarkStateError::Database(step, error)) => Err(phantasi_store_http(step, error)),
    }
}

pub(crate) async fn mark_all_read(
    State(db): State<DatabaseConnection>,
    viewer: OptionalViewer,
    Json(req): Json<phantasi_user_states::MarkAllReadRequest>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let (user_id, is_admin) = get_phantasi_user_and_admin_status(&viewer, &db).await?;
    // 共享订阅库：按当前用户可见源标记，而非「我创建的源」

    let mut source_ids_query = phantasi_sources::Entity::find()
        .select_only()
        .column(phantasi_sources::Column::Id);
    if !is_admin {
        source_ids_query = source_ids_query.filter(phantasi_sources::Column::AdminOnly.eq(false));
    }

    if let Some(source_id) = req.source_id {
        source_ids_query = source_ids_query.filter(phantasi_sources::Column::Id.eq(source_id));
    }
    if let Some(ref category) = req.category {
        source_ids_query = source_ids_query.filter(phantasi_sources::Column::Category.eq(category));
    }

    let source_ids: Vec<i32> = source_ids_query
        .into_tuple()
        .all(&db)
        .await
        .map_err(|error| phantasi_store_http("find visible sources", error))?;

    if source_ids.is_empty() {
        return Ok(Json(json!({ "success": true, "marked": 0 })));
    }

    let before =
        match req.before {
            Some(before) => Some(chrono::DateTime::from_timestamp_millis(before).ok_or_else(
                || phantasi_http_err(StatusCode::BAD_REQUEST, "Invalid reading cutoff"),
            )?),
            None => None,
        };

    let transaction = db
        .begin()
        .await
        .map_err(|error| phantasi_store_http("begin bulk reading state write", error))?;

    let now = Utc::now();
    let bind = [
        now.into(),
        user_id.into(),
        source_ids.clone().into(),
        before.into(),
    ];

    let updated = transaction
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
UPDATE phantasi_user_states AS us
SET is_read = TRUE, read_at = $1, updated_at = $1
FROM phantasi_items AS i
WHERE us.item_id = i.id
  AND us.user_id = $2
  AND us.is_read = FALSE
  AND i.source_id = ANY($3)
  AND ($4::timestamptz IS NULL OR i.published_at < $4)
RETURNING us.item_id, us.revision, i.source_id
"#,
            bind.clone(),
        ))
        .await
        .map_err(|error| phantasi_store_http("mark existing unread states", error))?;

    let inserted = transaction
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
INSERT INTO phantasi_user_states (user_id, item_id, is_read, is_starred, read_at, updated_at)
SELECT $2, i.id, TRUE, FALSE, $1, $1
FROM phantasi_items AS i
WHERE i.source_id = ANY($3)
  AND ($4::timestamptz IS NULL OR i.published_at < $4)
  AND NOT EXISTS (
    SELECT 1 FROM phantasi_user_states us
    WHERE us.user_id = $2 AND us.item_id = i.id
  )
RETURNING item_id, revision, (
  SELECT source_id FROM phantasi_items WHERE id = item_id
) AS source_id
"#,
            bind,
        ))
        .await
        .map_err(|error| phantasi_store_http("insert missing reading states", error))?;

    let mut marked = 0;
    let mut changes = Vec::new();

    for row in updated.iter().chain(inserted.iter()) {
        let item_id: i32 = row
            .try_get("", "item_id")
            .map_err(|error| phantasi_store_http("read marked item", error))?;
        let revision: i64 = row
            .try_get("", "revision")
            .map_err(|error| phantasi_store_http("read marked revision", error))?;
        marked += 1;
        let previous_revision = revision.saturating_sub(1);
        changes.push(json!({
            "item_id": item_id,
            "previous_revision": previous_revision,
            "revision": revision,
        }));
    }

    transaction
        .commit()
        .await
        .map_err(|error| phantasi_store_http("commit bulk reading states", error))?;
    Ok(Json(
        json!({ "success": true, "marked": marked, "changes": changes }),
    ))
}

#[cfg(test)]
mod journal_audit_contracts {
    fn impl_fn<'a>(src: &'a str, name: &str) -> &'a str {
        let start = src
            .find(&format!("pub(crate) async fn {name}"))
            .expect(name);
        let body = &src[start..];
        let end = body[1..]
            .find("\npub(crate) async fn ")
            .or_else(|| body[1..].find("\n#[cfg(test)]"))
            .map(|index| index + 1)
            .unwrap_or(body.len());
        &body[..end]
    }

    #[test]
    fn mark_all_read_loads_source_ids_not_rows() {
        let body = impl_fn(include_str!("reading_mark.rs"), "mark_all_read");
        assert!(body.contains("select_only()"));
        assert!(body.contains("into_tuple()"));
        assert!(!body.contains("phantasi_sources::Model"));
        let mark = body
            .split("\nasync fn apply_unread_delta")
            .next()
            .unwrap_or(body);
        assert!(
            !mark.contains("lock_exclusive"),
            "mark_all_read must not lock every item row"
        );
        assert!(
            mark.contains("INSERT INTO phantasi_user_states"),
            "mark_all_read must insert missing states in SQL"
        );
        assert!(
            !mark.contains("phantasi_items::Entity::find()"),
            "mark_all_read must not materialize item models"
        );
        assert!(
            !mark.contains("unread_count"),
            "mark_all_read must not write site-wide source unread_count"
        );
    }

    #[test]
    fn item_mark_does_not_write_site_unread() {
        let body = impl_fn(include_str!("reading_mark.rs"), "update_item_state");
        assert!(
            !body.contains("unread_count"),
            "HTTP mark must not write site-wide source unread_count"
        );
        assert!(
            body.contains("is_starred.is_some() && !is_admin"),
            "star writes are admin-only host identity, not TAPP grants"
        );
    }
}

#[cfg(test)]
mod state_visibility_tests {
    use super::*;

    #[test]
    fn non_admin_state_targets_require_public_sources() {
        let sql = visible_state_sources(false)
            .build(DatabaseBackend::Postgres)
            .to_string();
        assert!(sql.contains("\"admin_only\" = FALSE"), "{sql}");
        let admin = visible_state_sources(true)
            .build(DatabaseBackend::Postgres)
            .to_string();
        assert!(!admin.contains("admin_only"), "{admin}");
    }
}
