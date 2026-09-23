//! Phantasi offline reading-state sync.
use axum::{Json, extract::State, http::StatusCode};
use chrono::Utc;
use sea_orm::{
    ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait, QueryFilter,
    QuerySelect, QueryTrait, TransactionTrait,
    sea_query::{LockType, OnConflict},
};
use serde_json::json;

use crate::error::HttpError;
use crate::extract::OptionalViewer;
use crate::models::entities::{phantasi_items, phantasi_user_states};

use super::helpers::{get_phantasi_user_and_admin_status, phantasi_http_err, phantasi_store_http};
use super::reading_mark::visible_state_sources;

#[derive(Debug)]
enum SyncedStateApply {
    Confirmed { revision: i64 },
    Conflict(phantasi_user_states::SyncConflict),
    Failed,
}

fn apply_sync_fields(
    mut active: phantasi_user_states::ActiveModel,
    state_item: &phantasi_user_states::SyncStateItem,
    now: chrono::DateTime<Utc>,
    allow_starred: bool,
) -> phantasi_user_states::ActiveModel {
    if let Some(is_read) = state_item.is_read {
        active.is_read = Set(is_read);
        if is_read {
            active.read_at = Set(Some(now.into()));
        }
    }
    if allow_starred {
        if let Some(is_starred) = state_item.is_starred {
            active.is_starred = Set(is_starred);
            if is_starred {
                active.starred_at = Set(Some(now.into()));
            }
        }
    }
    if let Some(progress) = phantasi_user_states::normalize_read_progress(state_item.read_progress)
    {
        active.read_progress = Set(Some(progress));
    }
    active.updated_at = Set(now.into());
    active
}

/// One article: lock the user's state row and compare-and-swap it by revision.
///
/// Article existence and visibility are validated for the whole batch by the
/// caller before the first write; unread counts are per-user SQL projections, so
/// there is no article/source row to lock or update here.
async fn apply_synced_state(
    transaction: &impl ConnectionTrait,
    user_id: i32,
    is_admin: bool,
    now: chrono::DateTime<Utc>,
    state_item: &phantasi_user_states::SyncStateItem,
) -> SyncedStateApply {
    let existing = match phantasi_user_states::Entity::find()
        .filter(phantasi_user_states::Column::UserId.eq(user_id))
        .filter(phantasi_user_states::Column::ItemId.eq(state_item.item_id))
        .lock_exclusive()
        .one(transaction)
        .await
    {
        Ok(existing) => existing,
        Err(_) => return SyncedStateApply::Failed,
    };

    if let Some(server_state) = existing {
        let server_ts = server_state.updated_at.timestamp_millis();
        if state_item.conflicts_with(server_state.revision, server_ts) {
            let conflict = phantasi_user_states::SyncConflict {
                server_revision: server_state.revision,
                item_id: state_item.item_id,
                server_updated_at: server_ts,
                client_updated_at: state_item.updated_at,
            };
            return SyncedStateApply::Conflict(conflict);
        }

        let state_id = server_state.id;
        let previous_revision = server_state.revision;
        let active = apply_sync_fields(server_state.into(), state_item, now, is_admin);
        let updated = phantasi_user_states::Entity::update_many()
            .set(active)
            .filter(phantasi_user_states::Column::Id.eq(state_id))
            .filter(phantasi_user_states::Column::Revision.eq(previous_revision))
            .exec(transaction)
            .await;
        if matches!(&updated, Ok(result) if result.rows_affected == 1) {
            SyncedStateApply::Confirmed {
                revision: previous_revision + 1,
            }
        } else if matches!(&updated, Ok(result) if result.rows_affected == 0) {
            match current_sync_conflict(transaction, user_id, state_item).await {
                Ok(conflict) => SyncedStateApply::Conflict(conflict),
                Err(_) => SyncedStateApply::Failed,
            }
        } else {
            SyncedStateApply::Failed
        }
    } else {
        if state_item
            .expected_revision
            .is_some_and(|revision| revision != 0)
        {
            return SyncedStateApply::Conflict(phantasi_user_states::SyncConflict {
                item_id: state_item.item_id,
                server_revision: 0,
                server_updated_at: 0,
                client_updated_at: state_item.updated_at,
            });
        }

        let is_read = state_item.is_read.unwrap_or(false);
        let new_state = phantasi_user_states::ActiveModel {
            user_id: Set(user_id),
            item_id: Set(state_item.item_id),
            is_read: Set(is_read),
            is_starred: Set(is_admin && state_item.is_starred.unwrap_or(false)),
            read_progress: Set(phantasi_user_states::normalize_read_progress(
                state_item.read_progress,
            )),
            read_at: Set(if is_read { Some(now.into()) } else { None }),
            starred_at: Set(if is_admin && state_item.is_starred == Some(true) {
                Some(now.into())
            } else {
                None
            }),
            updated_at: Set(now.into()),
            ..Default::default()
        };
        // DO NOTHING instead of a unique violation: the violation would abort the
        // enclosing transaction, so the follow-up read could not report the conflict.
        let inserted = phantasi_user_states::Entity::insert(new_state)
            .on_conflict(
                OnConflict::columns([
                    phantasi_user_states::Column::UserId,
                    phantasi_user_states::Column::ItemId,
                ])
                .do_nothing()
                .to_owned(),
            )
            .exec_with_returning(transaction)
            .await;
        match inserted {
            Ok(inserted) => SyncedStateApply::Confirmed {
                revision: inserted.revision,
            },
            // sea-orm reports a DO NOTHING conflict as "record not inserted/found".
            Err(sea_orm::DbErr::RecordNotInserted | sea_orm::DbErr::RecordNotFound(_)) => {
                match current_sync_conflict(transaction, user_id, state_item).await {
                    Ok(conflict) if conflict.server_revision > 0 => {
                        SyncedStateApply::Conflict(conflict)
                    }
                    _ => SyncedStateApply::Failed,
                }
            }
            Err(_) => SyncedStateApply::Failed,
        }
    }
}

// A failed compare-and-swap is a conflict, not a retryable storage failure.
// Re-read after the competing write so the response describes current state.
async fn current_sync_conflict(
    db: &impl ConnectionTrait,
    user_id: i32,
    state: &phantasi_user_states::SyncStateItem,
) -> Result<phantasi_user_states::SyncConflict, sea_orm::DbErr> {
    let current = phantasi_user_states::Entity::find()
        .filter(phantasi_user_states::Column::UserId.eq(user_id))
        .filter(phantasi_user_states::Column::ItemId.eq(state.item_id))
        .one(db)
        .await?;
    Ok(phantasi_user_states::SyncConflict {
        item_id: state.item_id,
        server_revision: current.as_ref().map_or(0, |value| value.revision),
        server_updated_at: current
            .as_ref()
            .map_or(0, |value| value.updated_at.timestamp_millis()),
        client_updated_at: state.updated_at,
    })
}

// 离线同步

pub(crate) async fn sync_states(
    State(db): State<DatabaseConnection>,
    viewer: OptionalViewer,
    Json(req): Json<phantasi_user_states::SyncStatesRequest>,
) -> Result<Json<serde_json::Value>, HttpError> {
    req.validate_targets()
        .map_err(|message| phantasi_http_err(StatusCode::BAD_REQUEST, message))?;
    let (user_id, is_admin) = get_phantasi_user_and_admin_status(&viewer, &db).await?;

    let now = Utc::now();
    let mut synced = 0;
    let mut confirmed = Vec::new();
    let mut revisions = std::collections::HashMap::new();
    let mut failed = Vec::new();
    let mut conflicts = Vec::new();
    let item_ids: Vec<i32> = req.states.iter().map(|s| s.item_id).collect();

    let transaction = db
        .begin()
        .await
        .map_err(|error| phantasi_store_http("begin reading sync", error))?;

    // Validate the entire batch inside the write transaction, before the first
    // write. FOR KEY SHARE keeps the articles from being deleted until commit
    // without blocking ordinary article updates. Missing and hidden articles
    // share the same response, without disclosing private source data.
    let visible_item_ids: std::collections::HashSet<i32> = if !item_ids.is_empty() {
        phantasi_items::Entity::find()
            .filter(phantasi_items::Column::Id.is_in(item_ids))
            .filter(
                phantasi_items::Column::SourceId
                    .in_subquery(visible_state_sources(is_admin).into_query()),
            )
            .select_only()
            .column(phantasi_items::Column::Id)
            .lock(LockType::KeyShare)
            .into_tuple::<i32>()
            .all(&transaction)
            .await
            .map_err(|error| phantasi_store_http("find visible articles", error))?
            .into_iter()
            .collect()
    } else {
        std::collections::HashSet::new()
    };
    if req
        .states
        .iter()
        .any(|state| !visible_item_ids.contains(&state.item_id))
    {
        let _ = transaction.rollback().await;
        return Err(phantasi_http_err(StatusCode::NOT_FOUND, "Item not found"));
    }
    for state_item in req.states {
        if transaction
            .execute_unprepared("SAVEPOINT phantasi_sync_item")
            .await
            .is_err()
        {
            failed.push(state_item.item_id);
            continue;
        }
        match apply_synced_state(&transaction, user_id, is_admin, now, &state_item).await {
            SyncedStateApply::Confirmed { revision } => {
                let _ = transaction
                    .execute_unprepared("RELEASE SAVEPOINT phantasi_sync_item")
                    .await;
                synced += 1;
                confirmed.push(state_item.item_id);
                revisions.insert(state_item.item_id, revision);
            }
            other => {
                let _ = transaction
                    .execute_unprepared("ROLLBACK TO SAVEPOINT phantasi_sync_item")
                    .await;
                match other {
                    SyncedStateApply::Conflict(conflict) => conflicts.push(conflict),
                    SyncedStateApply::Failed => failed.push(state_item.item_id),
                    SyncedStateApply::Confirmed { .. } => {}
                }
            }
        }
    }
    transaction
        .commit()
        .await
        .map_err(|error| phantasi_store_http("commit reading sync", error))?;

    Ok(Json(json!(phantasi_user_states::SyncStatesResponse {
        synced,
        revisions,
        confirmed,
        failed,
        conflicts
    })))
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
    fn sync_states_uses_one_transaction_and_savepoints() {
        let sync = impl_fn(include_str!("reading_sync.rs"), "sync_states");
        assert!(sync.contains("begin()"));
        assert!(sync.contains("SAVEPOINT phantasi_sync_item"));
        assert!(sync.contains("RELEASE SAVEPOINT phantasi_sync_item"));
        assert!(sync.contains("ROLLBACK TO SAVEPOINT phantasi_sync_item"));
    }

    #[test]
    fn per_state_apply_does_not_touch_articles_or_sources() {
        let src = include_str!("reading_sync.rs");
        let start = src
            .find("async fn apply_synced_state")
            .expect("apply_synced_state");
        let body = &src[start..];
        let end = body[1..]
            .find("\nasync fn ")
            .map(|index| index + 1)
            .unwrap_or(body.len());
        let apply = &body[..end];
        assert!(
            !apply.contains("phantasi_items::"),
            "batch pre-validation covers articles; per-state apply must not re-read or lock them"
        );
        assert!(
            !apply.contains("phantasi_sources"),
            "offline sync must not write site-wide source unread_count"
        );
    }
}

#[cfg(test)]
mod sync_transaction_tests {
    use super::*;
    use sea_orm::{DatabaseBackend, DatabaseConnection, Statement};

    #[test]
    fn member_sync_cannot_write_starred() {
        let src = include_str!("reading_sync.rs");
        let start = src.find("fn apply_sync_fields").expect("apply_sync_fields");
        let body = &src[start..];
        let end = body[1..]
            .find("\nasync fn ")
            .map(|index| index + 1)
            .unwrap_or(body.len());
        let apply = &body[..end];
        assert!(apply.contains("allow_starred"));
        let insert = src
            .split("let new_state = phantasi_user_states::ActiveModel")
            .nth(1)
            .expect("insert state");
        assert!(insert.contains("is_admin && state_item.is_starred"));
    }

    fn isolated_url() -> String {
        std::env::var("PHANTASI_REVISION_TEST_DATABASE_URL").expect("isolated test database URL")
    }

    async fn isolated_db(max_connections: u32) -> DatabaseConnection {
        let mut options = sea_orm::ConnectOptions::new(isolated_url());
        options.max_connections(max_connections);
        sea_orm::Database::connect(options).await.unwrap()
    }

    fn sync_item(
        expected_revision: Option<i64>,
        is_read: Option<bool>,
    ) -> phantasi_user_states::SyncStateItem {
        phantasi_user_states::SyncStateItem {
            item_id: 10,
            expected_revision,
            is_read,
            is_starred: None,
            read_progress: None,
            updated_at: 1,
        }
    }

    async fn apply_read(
        db: &DatabaseConnection,
        expected_revision: Option<i64>,
        is_read: Option<bool>,
    ) -> SyncedStateApply {
        apply_synced_state(
            db,
            7,
            true,
            Utc::now(),
            &sync_item(expected_revision, is_read),
        )
        .await
    }

    async fn scalar_i32(db: &DatabaseConnection, sql: &str) -> i32 {
        db.query_one_raw(Statement::from_string(DatabaseBackend::Postgres, sql))
            .await
            .unwrap()
            .unwrap()
            .try_get::<i32>("", "v")
            .unwrap()
    }

    async fn scalar_i64(db: &DatabaseConnection, sql: &str) -> i64 {
        db.query_one_raw(Statement::from_string(DatabaseBackend::Postgres, sql))
            .await
            .unwrap()
            .unwrap()
            .try_get::<i64>("", "v")
            .unwrap()
    }

    const SYNC_TABLES: &str = r#"
        CREATE TABLE phantasi_sources (
          id INTEGER PRIMARY KEY,
          admin_only BOOLEAN NOT NULL DEFAULT FALSE,
          unread_count INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE phantasi_items (
          id INTEGER PRIMARY KEY,
          source_id INTEGER NOT NULL
        );
        CREATE TABLE phantasi_user_states (
          id SERIAL PRIMARY KEY,
          user_id INTEGER NOT NULL,
          item_id INTEGER NOT NULL,
          is_read BOOLEAN NOT NULL DEFAULT FALSE,
          is_starred BOOLEAN NOT NULL DEFAULT FALSE,
          read_at TIMESTAMPTZ,
          read_progress REAL,
          starred_at TIMESTAMPTZ,
          notes TEXT,
          updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
          revision BIGINT NOT NULL DEFAULT 1,
          UNIQUE (user_id, item_id)
        );
        CREATE OR REPLACE FUNCTION phantasi_advance_state_revision() RETURNS trigger AS $$
        BEGIN
            NEW.revision := OLD.revision + 1;
            RETURN NEW;
        END;
        $$ LANGUAGE plpgsql;
        CREATE TRIGGER phantasi_state_revision BEFORE UPDATE ON phantasi_user_states
            FOR EACH ROW EXECUTE FUNCTION phantasi_advance_state_revision();
        INSERT INTO phantasi_sources VALUES (1, FALSE, 1);
        INSERT INTO phantasi_items VALUES (10, 1);
    "#;

    #[tokio::test]
    #[ignore = "requires explicit PHANTASI_REVISION_TEST_DATABASE_URL for an isolated database"]
    async fn apply_confirms_then_conflicts() {
        let db = isolated_db(1).await;
        db.execute_unprepared("CREATE TEMP TABLE phantasi_sources (id INTEGER PRIMARY KEY, admin_only BOOLEAN NOT NULL DEFAULT FALSE, unread_count INTEGER NOT NULL); CREATE TEMP TABLE phantasi_items (id INTEGER PRIMARY KEY, source_id INTEGER NOT NULL); CREATE TEMP TABLE phantasi_user_states (id SERIAL PRIMARY KEY, user_id INTEGER NOT NULL, item_id INTEGER NOT NULL, is_read BOOLEAN NOT NULL DEFAULT FALSE, is_starred BOOLEAN NOT NULL DEFAULT FALSE, read_at TIMESTAMPTZ, read_progress REAL, starred_at TIMESTAMPTZ, notes TEXT, updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), revision BIGINT NOT NULL DEFAULT 1, UNIQUE (user_id, item_id)); CREATE OR REPLACE FUNCTION phantasi_advance_state_revision() RETURNS trigger AS $$ BEGIN NEW.revision := OLD.revision + 1; RETURN NEW; END; $$ LANGUAGE plpgsql; CREATE TRIGGER phantasi_state_revision BEFORE UPDATE ON phantasi_user_states FOR EACH ROW EXECUTE FUNCTION phantasi_advance_state_revision(); INSERT INTO phantasi_sources VALUES (1, FALSE, 1); INSERT INTO phantasi_items VALUES (10, 1); INSERT INTO phantasi_items VALUES (11, 1);").await.unwrap();

        match apply_read(&db, Some(0), Some(true)).await {
            SyncedStateApply::Confirmed { revision } => assert_eq!(revision, 1),
            other => panic!("insert should confirm, got {other:?}"),
        }

        match apply_read(&db, Some(0), Some(true)).await {
            SyncedStateApply::Conflict(conflict) => {
                assert_eq!(conflict.item_id, 10);
                assert_eq!(conflict.server_revision, 1);
            }
            other => panic!("stale expected revision should conflict, got {other:?}"),
        }

        match apply_read(&db, Some(1), None).await {
            SyncedStateApply::Confirmed { revision } => assert_eq!(revision, 2),
            other => panic!("matching revision should confirm, got {other:?}"),
        }

        db.close().await.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires explicit PHANTASI_REVISION_TEST_DATABASE_URL for an isolated database"]
    async fn concurrent_first_inserts_confirm_once() {
        let db = isolated_db(4).await;
        db.execute_unprepared("DROP TABLE IF EXISTS phantasi_user_states, phantasi_items, phantasi_sources CASCADE; DROP FUNCTION IF EXISTS phantasi_advance_state_revision() CASCADE;").await.unwrap();
        db.execute_unprepared(SYNC_TABLES).await.unwrap();

        let now = Utc::now();
        let left_db = db.clone();
        let right_db = db.clone();
        let left_item = sync_item(Some(0), Some(true));
        let right_item = sync_item(Some(0), Some(true));
        let (left, right) = tokio::join!(
            apply_synced_state(&left_db, 7, true, now, &left_item),
            apply_synced_state(&right_db, 7, true, now, &right_item),
        );
        let outcomes = [left, right];
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| matches!(outcome, SyncedStateApply::Confirmed { .. }))
                .count(),
            1,
            "{outcomes:?}"
        );
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| matches!(outcome, SyncedStateApply::Conflict(_)))
                .count(),
            1,
            "{outcomes:?}"
        );
        assert_eq!(
            scalar_i32(&db, "SELECT COUNT(*)::int AS v FROM phantasi_user_states").await,
            1
        );
        assert_eq!(
            scalar_i64(&db, "SELECT revision AS v FROM phantasi_user_states").await,
            1
        );
        db.execute_unprepared("DROP TABLE IF EXISTS phantasi_user_states, phantasi_items, phantasi_sources CASCADE; DROP FUNCTION IF EXISTS phantasi_advance_state_revision() CASCADE;").await.unwrap();
        db.close().await.unwrap();
    }

    /// Two batch transactions race on the same first state. The loser must
    /// report a conflict and keep its transaction usable for the rest of the batch.
    #[tokio::test]
    async fn racing_first_insert_in_a_transaction_reports_conflict() {
        let Ok(url) = std::env::var("PHANTASI_TEST_DATABASE_URL") else {
            return;
        };
        let isolated = crate::db::IsolatedSchema::migrated(&url, "reading_sync_test").await;
        let db = isolated.db.clone();
        db.execute_unprepared(
            "INSERT INTO users (id, username) VALUES (7, 'reader');
             INSERT INTO phantasi_sources (id, user_id, name, url) VALUES (1, 7, 'S', 'https://s.example/feed');
             INSERT INTO phantasi_items (id, source_id, guid, title, link, published_at, fetched_at)
             VALUES (10, 1, 'g10', 'T', 'https://s.example/10', NOW(), NOW());",
        )
        .await
        .unwrap();
        let now = Utc::now();
        let first = db.begin().await.unwrap();
        match apply_synced_state(&first, 7, true, now, &sync_item(Some(0), Some(true))).await {
            SyncedStateApply::Confirmed { revision } => assert_eq!(revision, 1),
            other => panic!("first insert should confirm, got {other:?}"),
        }
        let second = db.begin().await.unwrap();
        second
            .execute_unprepared("SAVEPOINT phantasi_sync_item")
            .await
            .unwrap();
        let racing = async {
            apply_synced_state(&second, 7, true, now, &sync_item(Some(0), Some(true))).await
        };
        let commit_first = async {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            first.commit().await.unwrap();
        };
        let (outcome, ()) = tokio::join!(racing, commit_first);
        match outcome {
            SyncedStateApply::Conflict(conflict) => assert_eq!(conflict.server_revision, 1),
            other => panic!("losing first insert must be a conflict, got {other:?}"),
        }
        second
            .execute_unprepared("RELEASE SAVEPOINT phantasi_sync_item")
            .await
            .expect("transaction must stay usable after the conflict");
        second.commit().await.unwrap();
        drop(db);
        isolated.drop().await;
    }
}
