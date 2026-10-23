//! The one writer of explicit per-item read / star marks. The HTTP routes and
//! the Agent both go through here, so marks from either side serialize on the
//! same locks and a new star triggers its side effect exactly once.

use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, DbErr, EntityTrait,
    QueryFilter, QuerySelect, QueryTrait, TransactionTrait,
};

use crate::models::entities::{phantasi_items, phantasi_sources, phantasi_user_states};

/// Sources whose items a viewer may hold state on: everything for an admin,
/// only public sources otherwise.
pub(crate) fn visible_state_sources(is_admin: bool) -> sea_orm::Select<phantasi_sources::Entity> {
    let query = phantasi_sources::Entity::find()
        .select_only()
        .column(phantasi_sources::Column::Id);
    if is_admin {
        query
    } else {
        query.filter(phantasi_sources::Column::AdminOnly.eq(false))
    }
}

#[derive(Debug)]
pub(crate) struct MarkedState {
    pub title: String,
    pub previous_revision: i64,
    pub revision: i64,
}

#[derive(Debug)]
pub(crate) enum MarkStateError {
    /// The item does not exist or its source is not visible to this viewer.
    NotVisible,
    Database(&'static str, DbErr),
}

/// Apply `is_read` / `is_starred` (each optional) to one item for one user.
/// Callers decide who may star; this only enforces source visibility.
pub(crate) async fn mark_item_state(
    db: &DatabaseConnection,
    user_id: i32,
    is_admin: bool,
    item_id: i32,
    is_read: Option<bool>,
    is_starred: Option<bool>,
) -> Result<MarkedState, MarkStateError> {
    let db_err = |step: &'static str| move |error| MarkStateError::Database(step, error);
    let now = crate::clock::now();
    let transaction = db
        .begin()
        .await
        .map_err(db_err("begin reading state write"))?;
    // Lock the article even before a state row exists, then lock existing state.
    // This serializes first writes and keeps the count delta tied to the state read.
    let title = phantasi_items::Entity::find_by_id(item_id)
        .filter(
            phantasi_items::Column::SourceId
                .in_subquery(visible_state_sources(is_admin).into_query()),
        )
        .select_only()
        .column(phantasi_items::Column::Title)
        .lock_exclusive()
        .into_tuple::<String>()
        .one(&transaction)
        .await
        .map_err(db_err("find article"))?
        .ok_or(MarkStateError::NotVisible)?;
    let existing = phantasi_user_states::Entity::find()
        .filter(phantasi_user_states::Column::UserId.eq(user_id))
        .filter(phantasi_user_states::Column::ItemId.eq(item_id))
        .lock_exclusive()
        .one(&transaction)
        .await
        .map_err(db_err("find reading state"))?;

    let was_starred = existing.as_ref().is_some_and(|state| state.is_starred);
    let (previous_revision, saved) = match existing {
        Some(state) => {
            let previous_revision = state.revision;
            let mut active: phantasi_user_states::ActiveModel = state.into();
            if let Some(read) = is_read {
                active.is_read = Set(read);
                if read {
                    active.read_at = Set(Some(now.into()));
                }
            }
            if let Some(starred) = is_starred {
                active.is_starred = Set(starred);
                if starred {
                    active.starred_at = Set(Some(now.into()));
                }
            }
            active.updated_at = Set(now.into());
            let saved = active
                .update(&transaction)
                .await
                .map_err(db_err("update reading state"))?;
            (previous_revision, saved)
        }
        None => {
            let saved = phantasi_user_states::ActiveModel {
                user_id: Set(user_id),
                item_id: Set(item_id),
                is_read: Set(is_read.unwrap_or(false)),
                is_starred: Set(is_starred.unwrap_or(false)),
                read_at: Set((is_read == Some(true)).then(|| now.into())),
                starred_at: Set((is_starred == Some(true)).then(|| now.into())),
                updated_at: Set(now.into()),
                ..Default::default()
            }
            .insert(&transaction)
            .await
            .map_err(db_err("create reading state"))?;
            (0, saved)
        }
    };
    transaction
        .commit()
        .await
        .map_err(db_err("commit reading state"))?;

    if is_starred == Some(true) && !was_starred {
        crate::services::agent::merope::spawn_ingest(
            user_id,
            "phantasi.starred",
            format!("Starred \"{title}\""),
        );
    }
    Ok(MarkedState {
        title,
        previous_revision,
        revision: saved.revision,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_explicit_mark_goes_through_the_shared_writer() {
        for (name, source, entry) in [
            (
                "http",
                include_str!("../api/phantasi/reading_mark.rs"),
                "pub(crate) async fn update_item_state(",
            ),
            (
                "agent",
                include_str!("agent/executor/handlers/data_write.rs"),
                "async fn execute_phantasi_mark(",
            ),
        ] {
            let body = source
                .split(entry)
                .nth(1)
                .and_then(|rest| rest.split("\nasync fn ").next())
                .and_then(|rest| rest.split("\npub(crate) async fn ").next())
                .expect(name);
            assert!(
                body.contains("mark_item_state("),
                "{name} must use mark_item_state"
            );
            assert!(
                !body.contains("phantasi_user_states::"),
                "{name} writes reading state on its own"
            );
        }
    }
}
