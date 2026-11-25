//! What was said before her persona was cleared: the next her never reads
//! it. The people it was said with keep it, and see it as ever.
//!
//! - **Whole conversations.** Clearing her marks every conversation with her
//!   there is, in private and in groups, as one of the past
//!   (`agent_sessions.context.past`, when she was cleared), in that
//!   transaction. A reply under way as she was cleared is written into the
//!   conversation it was asked in, so it is of the past with the rest.
//!   Nothing in them changes but the mark.
//! - **Read by no one of hers.** Every read of what was said that she says
//!   anything from leaves a marked conversation out (`(s.context->>'past') IS
//!   NULL`): `past_readers_tests` goes over all of them.
//! - **Never gone on in.** A message sent into a marked one goes into the
//!   chat they have had in that place since, or a new one
//!   (`sessions::ensure_session_in`): the old one can only be read.
//! - **A group's lines in mind** (and kept for a restart) end where she was
//!   cleared ([`after`]).
//! - **Undone by taking the marks of one clearing off again and letting its
//!   record go** ([`REOPEN`], [`UNRECORD`]): nothing else was changed. With
//!   the record left, a group's kept lines and lines that come late would
//!   still end where she was cleared.
//!
//! Work's conversations are the site's, and are not marked.

use std::sync::RwLock;
use std::time::Instant;

use chrono::{DateTime, Utc};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, DbErr, Statement};
use serde::{Deserialize, Serialize};

pub(crate) const NAMESPACE: &str = "merope_cleared";
pub(crate) const RECORD: &str = "last";
/// Kept far past anything it is asked about: a group's lines are kept for
/// hours, a line from a device that was away for as long as it was.
const KEEP_DAYS: i64 = 3650;

/// The marks of the clearing `$1` taken off, so what was said is read again.
#[cfg(test)]
const REOPEN: &str = "UPDATE agent_sessions SET context = context::jsonb - 'past' \
     WHERE context->>'past' = $1";

/// The record of the last clearing gone (the one before has none).
#[cfg(test)]
const UNRECORD: &str = "DELETE FROM runtime_registry WHERE namespace = 'merope_cleared'";

/// When she was last cleared.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Cleared {
    pub at: DateTime<Utc>,
}

fn identity() -> crate::services::runtime_registry::RegistryIdentity<'static> {
    crate::services::runtime_registry::RegistryIdentity {
        subject_id: None,
        owner_id: None,
        tapp_id: None,
        runtime_id: None,
    }
}

async fn mark<C: ConnectionTrait>(
    db: &C,
    at: DateTime<Utc>,
    begun_by: Option<DateTime<Utc>>,
) -> Result<u64, DbErr> {
    Ok(db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE agent_sessions SET context = \
               COALESCE(context::jsonb, '{}'::jsonb) || jsonb_build_object('past', $1::text) \
             WHERE COALESCE(context->>'mode', 'chat') = 'chat' AND (context->>'past') IS NULL \
               AND ($2::timestamptz IS NULL OR created_at <= $2)",
            [at.to_rfc3339().into(), begun_by.into()],
        ))
        .await?
        .rows_affected())
}

/// Her persona is being cleared, in this transaction: all that was said
/// with her until now is of the past.
pub(super) async fn close<C: ConnectionTrait>(db: &C) -> Result<(), DbErr> {
    let at = crate::clock::now();
    crate::services::runtime_registry::put(
        db,
        NAMESPACE,
        RECORD,
        identity(),
        &Cleared { at },
        (at + chrono::Duration::days(KEEP_DAYS)).timestamp(),
    )
    .await?;
    mark(db, at, None).await?;
    Ok(())
}

/// She was cleared, and that is committed as of `committed`: conversations
/// begun until then and not there to be marked are marked too, and what the
/// groups said until she was cleared goes from mind. Only for clearing her:
/// moving her takes nothing from mind.
pub async fn close_late(db: &DatabaseConnection, committed: DateTime<Utc>) {
    let marked = match cleared_at(db).await {
        Ok(Some(at)) => {
            crate::services::channel_group::forget_lines(at);
            mark(db, at, Some(committed)).await.map(|_| ())
        }
        Ok(None) => Ok(()),
        Err(error) => Err(error),
    };
    if let Err(error) = marked {
        tracing::warn!(%error, "[Merope] conversations begun as she was cleared were not marked");
    }
}

/// When she was last cleared, if ever.
pub(crate) async fn cleared_at(db: &DatabaseConnection) -> Result<Option<DateTime<Utc>>, DbErr> {
    Ok(
        crate::services::runtime_registry::get::<Cleared>(db, NAMESPACE, RECORD)
            .await?
            .map(|cleared| cleared.at),
    )
}

/// `lines`, each said at its `at`, without those of before she was cleared.
pub(crate) fn after<L>(
    cleared: Option<Cleared>,
    mut lines: Vec<L>,
    at: impl Fn(&L) -> DateTime<Utc>,
) -> Vec<L> {
    if let Some(cleared) = cleared {
        lines.retain(|line| at(line) > cleared.at);
    }
    lines
}

/// One conversation, marked: a line of before she was cleared that came only
/// now goes in it.
pub(crate) async fn mark_one(
    db: &DatabaseConnection,
    session: &str,
    at: DateTime<Utc>,
) -> Result<(), DbErr> {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE agent_sessions SET context = \
           COALESCE(context::jsonb, '{}'::jsonb) || jsonb_build_object('past', $2::text) \
         WHERE id = $1",
        [session.into(), at.to_rfc3339().into()],
    ))
    .await?;
    Ok(())
}

/// When this process last forgot her ([`super::forget_in_memory`]).
static FORGOTTEN: RwLock<Option<Instant>> = RwLock::new(None);

pub(super) fn forget() {
    if let Ok(mut forgotten) = FORGOTTEN.write() {
        *forgotten = Some(Instant::now());
    }
}

/// Whether what was read at `at` and kept in memory is of her still.
pub(super) fn of_her(at: Instant) -> bool {
    FORGOTTEN
        .read()
        .map_or(true, |forgotten| forgotten.is_none_or(|forgotten| at > forgotten))
}

#[cfg(test)]
#[path = "past_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "past_readers_tests.rs"]
mod readers_tests;
