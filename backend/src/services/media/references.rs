//! Asset references. Callers pass an open transaction so business writes stay atomic.

use chrono::{DateTime, Utc};
use sea_orm::{
    ColumnTrait, Condition, ConnectionTrait, DatabaseBackend, EntityTrait, PaginatorTrait,
    QueryFilter, Set, Statement,
};

use crate::models::entities::media_references;

use super::assets;
use super::error::MediaError;
use super::types::MediaState;

const CONSUMER_TYPES: &[&str] = &[
    "rss_item",
    "note_draft",
    "note_published",
    "note_history",
    "persona_outfit",
    "persona_portrait",
    "sticker",
    "ai_task",
    "federation_activity",
    "federation_outbox",
    "channel_message",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewReference {
    pub asset_id: i32,
    pub slot: String,
    pub requires_public: bool,
    pub expires_at: Option<DateTime<Utc>>,
}

pub fn parse_consumer_type(value: &str) -> Result<&str, MediaError> {
    CONSUMER_TYPES
        .iter()
        .copied()
        .find(|allowed| *allowed == value)
        .ok_or_else(|| MediaError::invalid("Invalid media consumer"))
}

pub async fn active_count(db: &impl ConnectionTrait, asset_id: i32) -> Result<i64, MediaError> {
    Ok(media_references::Entity::find()
        .filter(media_references::Column::AssetId.eq(asset_id))
        .filter(
            Condition::any()
                .add(media_references::Column::ExpiresAt.is_null())
                .add(media_references::Column::ExpiresAt.gt(Utc::now().fixed_offset())),
        )
        .count(db)
        .await? as i64)
}

pub(super) async fn has_active(
    db: &impl ConnectionTrait,
    asset_id: i32,
    public_only: bool,
) -> Result<bool, MediaError> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT EXISTS (SELECT 1 FROM media_references WHERE asset_id = $1
            AND (expires_at IS NULL OR expires_at > $2)
            AND (NOT $3 OR requires_public)) AS present",
            [
                asset_id.into(),
                Utc::now().fixed_offset().into(),
                public_only.into(),
            ],
        ))
        .await?
        .ok_or(MediaError::StoreFailed)?;
    Ok(row.try_get("", "present")?)
}

/// Replace every slot for one consumer. Locks assets in id order.
pub async fn replace_for_consumer(
    txn: &impl ConnectionTrait,
    consumer_type: &str,
    consumer_id: &str,
    refs: &[NewReference],
) -> Result<(), MediaError> {
    let consumer_type = parse_consumer_type(consumer_type)?;
    if consumer_id.trim().is_empty() {
        return Err(MediaError::invalid("Invalid media consumer"));
    }
    if refs.iter().any(|item| item.slot.trim().is_empty()) {
        return Err(MediaError::invalid("Invalid media slot"));
    }
    let ids = refs.iter().map(|item| item.asset_id).collect();
    let locked = assets::lock_by_ids_sorted(txn, ids).await?;
    for row in &locked {
        let state = row.state.as_deref().unwrap_or("");
        if MediaState::parse(state).ok() != Some(MediaState::Ready) {
            return Err(MediaError::NotReady);
        }
    }
    media_references::Entity::delete_many()
        .filter(media_references::Column::ConsumerType.eq(consumer_type))
        .filter(media_references::Column::ConsumerId.eq(consumer_id))
        .exec(txn)
        .await?;
    if refs.is_empty() {
        return Ok(());
    }
    let now = Utc::now().fixed_offset();
    let rows = refs.iter().map(|item| media_references::ActiveModel {
        asset_id: Set(item.asset_id),
        consumer_type: Set(consumer_type.to_string()),
        consumer_id: Set(consumer_id.to_string()),
        slot: Set(item.slot.clone()),
        requires_public: Set(item.requires_public),
        expires_at: Set(item.expires_at.map(|ts| ts.fixed_offset())),
        created_at: Set(now),
        ..Default::default()
    });
    media_references::Entity::insert_many(rows)
        .exec_without_returning(txn)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consumer_type_is_server_whitelist() {
        assert!(parse_consumer_type("note_draft").is_ok());
        assert!(parse_consumer_type("phantasi_note_docs").is_err());
        assert!(parse_consumer_type("DROP TABLE media_assets").is_err());
        assert!(parse_consumer_type("").is_err());
    }
}
