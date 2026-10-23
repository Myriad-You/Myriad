//! Asset row lifecycle. File bytes live in [`super::store`].

use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseBackend, EntityTrait, QueryFilter, Set,
    Statement,
};
use uuid::Uuid;

use crate::models::entities::media_assets;

use super::error::MediaError;
use super::types::{MediaAsset, MediaContext, MediaExposure, MediaScope, MediaSource, MediaState};
use super::urls::{compatible_url, content_path, filename_for_mime, staging_url, storage_key};
use super::validate::ValidatedPayload;

pub async fn find_by_producer(
    db: &impl ConnectionTrait,
    ctx: &MediaContext,
) -> Result<Option<media_assets::Model>, MediaError> {
    let Some(key) = ctx.producer_key.as_deref() else {
        return Ok(None);
    };
    let mut query = media_assets::Entity::find()
        .filter(media_assets::Column::ProducerKey.eq(key))
        .filter(media_assets::Column::Scope.eq(ctx.scope.as_str()));
    query = match ctx.owner_user_id() {
        Some(owner) => query.filter(media_assets::Column::OwnerUserId.eq(owner)),
        None => query.filter(media_assets::Column::OwnerUserId.is_null()),
    };
    Ok(query.one(db).await?)
}

pub async fn insert_staging(
    db: &impl ConnectionTrait,
    ctx: &MediaContext,
    payload: &ValidatedPayload,
    filename: &str,
    derived_from_id: Option<i32>,
    exposure: MediaExposure,
    write_token: Uuid,
    lease_secs: i64,
) -> Result<media_assets::Model, MediaError> {
    ctx.validate()?;
    let public_id = Uuid::new_v4();
    let key = storage_key(public_id, payload.ext)?;
    let name = filename_for_mime(filename, &payload.mime, public_id)?;
    let now = crate::clock::now().fixed_offset();
    let lease = now + chrono::Duration::seconds(lease_secs);
    let published = (exposure == MediaExposure::Public).then_some(now);
    let row = media_assets::ActiveModel {
        kind: Set(ctx.source.catalog_kind().to_string()),
        url: Set(staging_url(public_id)),
        mime: Set(payload.mime.clone()),
        name: Set(name),
        size: Set(payload.size),
        created_at: Set(now),
        public_id: Set(Some(public_id)),
        scope: Set(Some(ctx.scope.as_str().to_string())),
        owner_user_id: Set(ctx.owner_user_id()),
        created_by: Set(ctx.created_by()),
        storage_key: Set(Some(key)),
        state: Set(Some(MediaState::Staging.as_str().to_string())),
        exposure: Set(Some(exposure.as_str().to_string())),
        first_published_at: Set(published),
        source: Set(Some(ctx.source.as_str().to_string())),
        derived_from_id: Set(derived_from_id),
        checksum_sha256: Set(Some(payload.checksum_sha256.clone())),
        width: Set(payload.width),
        height: Set(payload.height),
        updated_at: Set(Some(now)),
        state_since: Set(Some(now)),
        references_complete: Set(true),
        write_token: Set(Some(write_token)),
        write_lease_until: Set(Some(lease)),
        producer_key: Set(ctx.producer_key.clone()),
        ..Default::default()
    };
    Ok(row.insert(db).await?)
}

pub async fn commit_ready(
    db: &impl ConnectionTrait,
    id: i32,
    write_token: Uuid,
    catalog_url: &str,
) -> Result<bool, MediaError> {
    let result = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
UPDATE media_assets
SET state = 'ready',
    url = $1,
    write_token = NULL,
    write_lease_until = NULL,
    updated_at = $4::timestamptz,
    state_since = $4::timestamptz
WHERE id = $2
  AND write_token = $3
  AND state = 'staging'
"#,
            [
                catalog_url.into(),
                id.into(),
                write_token.into(),
                crate::clock::now().into(),
            ],
        ))
        .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn mark_public(
    txn: &impl ConnectionTrait,
    id: i32,
    catalog_url: &str,
) -> Result<bool, MediaError> {
    let result = txn
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
UPDATE media_assets
SET exposure = 'public',
    url = $1,
    first_published_at = COALESCE(first_published_at, $3::timestamptz),
    updated_at = $3::timestamptz
WHERE id = $2
  AND state = 'ready'
"#,
            [catalog_url.into(), id.into(), crate::clock::now().into()],
        ))
        .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn mark_private(
    txn: &impl ConnectionTrait,
    id: i32,
    catalog_url: &str,
) -> Result<bool, MediaError> {
    let result = txn
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
UPDATE media_assets
SET exposure = 'private',
    url = $1,
    updated_at = $3::timestamptz
WHERE id = $2
  AND state = 'ready'
  AND exposure = 'public'
"#,
            [catalog_url.into(), id.into(), crate::clock::now().into()],
        ))
        .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn renew_write_lease(
    db: &impl ConnectionTrait,
    id: i32,
    write_token: Uuid,
    lease_secs: i64,
) -> Result<bool, MediaError> {
    let result = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
UPDATE media_assets
SET write_lease_until = $4::timestamptz + make_interval(secs => $1::double precision),
    updated_at = $4::timestamptz
WHERE id = $2
  AND write_token = $3
  AND state = 'staging'
  AND write_lease_until > $4::timestamptz
"#,
            [
                lease_secs.into(),
                id.into(),
                write_token.into(),
                crate::clock::now().into(),
            ],
        ))
        .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn lock_by_id(
    txn: &impl ConnectionTrait,
    id: i32,
) -> Result<Option<media_assets::Model>, MediaError> {
    let found = txn
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id FROM media_assets WHERE id = $1 FOR UPDATE",
            [id.into()],
        ))
        .await?;
    if found.is_none() {
        return Ok(None);
    }
    Ok(media_assets::Entity::find_by_id(id).one(txn).await?)
}

pub async fn lock_by_ids_sorted(
    txn: &impl ConnectionTrait,
    mut ids: Vec<i32>,
) -> Result<Vec<media_assets::Model>, MediaError> {
    ids.sort_unstable();
    ids.dedup();
    let mut rows = Vec::new();
    for id in ids {
        if let Some(row) = lock_by_id(txn, id).await? {
            rows.push(row);
        }
    }
    Ok(rows)
}

pub async fn mark_missing(
    db: &impl ConnectionTrait,
    id: i32,
    write_token: Uuid,
) -> Result<bool, MediaError> {
    let result = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
UPDATE media_assets
SET state = 'missing',
    write_token = NULL,
    write_lease_until = NULL,
    updated_at = $3::timestamptz,
    state_since = $3::timestamptz
WHERE id = $1
  AND write_token = $2
  AND state = 'staging'
"#,
            [id.into(), write_token.into(), crate::clock::now().into()],
        ))
        .await?;
    Ok(result.rows_affected() == 1)
}

/// Detach a terminal (missing/deleted) row from its producer key so the
/// producer can write again; the unique index spans every state.
pub async fn release_producer_key(db: &impl ConnectionTrait, id: i32) -> Result<bool, MediaError> {
    let result = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
UPDATE media_assets
SET producer_key = NULL,
    updated_at = $2::timestamptz
WHERE id = $1
  AND state IN ('missing', 'deleted')
"#,
            [id.into(), crate::clock::now().into()],
        ))
        .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn mark_deleting(txn: &impl ConnectionTrait, id: i32) -> Result<bool, MediaError> {
    let result = txn
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
UPDATE media_assets
SET state = 'deleting',
    updated_at = $2::timestamptz,
    state_since = $2::timestamptz
WHERE id = $1
  AND state = 'ready'
"#,
            [id.into(), crate::clock::now().into()],
        ))
        .await?;
    Ok(result.rows_affected() == 1)
}

/// A `missing` row has no file to unlink; retire it in one step.
pub async fn retire_missing(txn: &impl ConnectionTrait, id: i32) -> Result<bool, MediaError> {
    let result = txn
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
UPDATE media_assets
SET state = 'deleted',
    producer_key = NULL,
    updated_at = $2::timestamptz,
    state_since = $2::timestamptz
WHERE id = $1
  AND state = 'missing'
"#,
            [id.into(), crate::clock::now().into()],
        ))
        .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn mark_deleted(db: &impl ConnectionTrait, id: i32) -> Result<bool, MediaError> {
    let result = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
UPDATE media_assets
SET state = 'deleted',
    updated_at = $2::timestamptz,
    state_since = $2::timestamptz
WHERE id = $1
  AND state = 'deleting'
"#,
            [id.into(), crate::clock::now().into()],
        ))
        .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn find_by_id(
    db: &impl ConnectionTrait,
    id: i32,
) -> Result<Option<media_assets::Model>, MediaError> {
    Ok(media_assets::Entity::find_by_id(id).one(db).await?)
}

pub async fn find_by_public_id(
    db: &impl ConnectionTrait,
    public_id: Uuid,
) -> Result<Option<media_assets::Model>, MediaError> {
    Ok(media_assets::Entity::find()
        .filter(media_assets::Column::PublicId.eq(public_id))
        .one(db)
        .await?)
}

pub fn to_domain(row: media_assets::Model, usage_count: i64) -> Result<MediaAsset, MediaError> {
    let public_id = row
        .public_id
        .ok_or_else(|| MediaError::invalid("Asset is not migrated"))?;
    let state = row
        .state
        .as_deref()
        .ok_or_else(|| MediaError::invalid("Asset is not migrated"))?;
    let scope = row
        .scope
        .as_deref()
        .ok_or_else(|| MediaError::invalid("Asset is not migrated"))?;
    let exposure = row
        .exposure
        .as_deref()
        .unwrap_or(MediaExposure::Public.as_str());
    let source = row
        .source
        .as_deref()
        .unwrap_or(MediaSource::Legacy.as_str());
    let filename = filename_for_mime(&row.name, &row.mime, public_id)?;
    let state = MediaState::parse(state)?;
    let url = compatible_url(public_id, &filename);
    let public_path = (state == MediaState::Ready && exposure == MediaExposure::Public.as_str())
        .then(|| url.clone());
    Ok(MediaAsset {
        id: row.id,
        public_id,
        scope: MediaScope::parse(scope)?,
        owner_user_id: row.owner_user_id,
        name: row.name,
        mime: row.mime,
        size: row.size,
        source: MediaSource::parse(source)?,
        state,
        exposure: MediaExposure::parse(exposure)?,
        kind: row.kind,
        url,
        content_path: content_path(row.id),
        public_path,
        created_at: row.created_at.with_timezone(&Utc),
        usage_count,
        references_complete: row.references_complete,
        checksum_sha256: row.checksum_sha256,
        width: row.width,
        height: row.height,
        derived_from_id: row.derived_from_id,
        first_published_at: row.first_published_at.map(|ts| ts.with_timezone(&Utc)),
    })
}
