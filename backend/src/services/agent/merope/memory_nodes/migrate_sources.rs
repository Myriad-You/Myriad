//! Her other sources, a child end's notes, and what is on her mind and
//! what she wants, moved onto her nodes ([`super::migrate`]) exactly as
//! each is written there today, by the same code: no mapping of its own
//! here, and no model.
//!
//! - **Her other sources** (`about_me`, `us`, `lands`, `sore`, `bit`,
//!   `group_day`, `jargon`, `narrative`, and a fact that let a sore spot
//!   go): every row still held, oldest first, hung where [`super::homes`]
//!   says ([`super::hanging::hang`]); one hung already writes nothing.
//!   Each account's go into its own private talk, a group's into the
//!   group's, hers into her own.
//! - **A child end's notes** (`agent_nodes`): each note a span on the site
//!   owner's repository or talks-with-other-AIs node ([`super::child::put`]).
//! - **Threads and wants**: each account's threads, and her wants, as her
//!   intents ([`super::home_intents`]); a thread grown from no message of
//!   theirs stays only a thread. Unlike the pass kept in step, this lets
//!   no row go past the bound: the old rows are not written here.

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};

use super::{
    Error, child, hanging, home_intents,
    migrate::{Counts, Halt, her_pen},
};
use crate::services::agent::merope::nodes;

/// The sources [`super::homes`] gives a home.
pub(crate) const HUNG: &[&str] = &[
    "about_me",
    "us",
    "lands",
    "sore",
    "bit",
    "group_day",
    "jargon",
    "narrative",
];

/// Rows still held of the sources hung, and the facts that let a sore
/// spot go, oldest first.
const HELD: &str = "FROM agent_memories m \
     WHERE m.invalid_at IS NULL AND m.superseded_by IS NULL \
       AND (m.source = ANY($1) OR (m.source = 'chat' AND EXISTS ( \
         SELECT 1 FROM agent_memories b WHERE b.superseded_by = m.id \
           AND b.source = 'sore' AND b.invalid_reason = 'forgiven')))";

fn hung() -> Vec<String> {
    HUNG.iter().map(|source| source.to_string()).collect()
}

async fn held<C: ConnectionTrait>(db: &C) -> Result<Vec<String>, Error> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            format!("SELECT m.id {HELD} ORDER BY m.created_at, m.id"),
            [hung().into()],
        ))
        .await?;
    rows.iter()
        .map(|row| Ok(row.try_get::<String>("", "id")?))
        .collect()
}

async fn number<C: ConnectionTrait>(db: &C, sql: &str) -> Result<usize, Error> {
    let row = db
        .query_one_raw(Statement::from_string(DatabaseBackend::Postgres, sql))
        .await?;
    Ok(row
        .map(|row| row.try_get::<i64>("", "n"))
        .transpose()?
        .unwrap_or(0)
        .max(0) as usize)
}

/// Accounts with threads still held in their private talk.
async fn with_threads<C: ConnectionTrait>(db: &C) -> Result<Vec<i32>, Error> {
    let rows = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT DISTINCT user_id FROM agent_memories \
             WHERE source = 'thread' AND venue = 'private' AND user_id > 0 \
               AND invalid_at IS NULL AND superseded_by IS NULL ORDER BY user_id",
        ))
        .await?;
    rows.iter()
        .map(|row| Ok(row.try_get::<i32>("", "user_id")?))
        .collect()
}

const THREADS: &str = "SELECT COUNT(*) AS n FROM agent_memories WHERE source = 'thread' \
     AND venue = 'private' AND user_id > 0 AND invalid_at IS NULL AND superseded_by IS NULL";
const WANTS: &str = "SELECT COUNT(*) AS n FROM agent_memories WHERE source = 'want' \
     AND user_id IS NULL AND invalid_at IS NULL AND superseded_by IS NULL";
const NOTES: &str = "SELECT COALESCE(SUM(jsonb_array_length(notes)), 0)::bigint AS n \
     FROM agent_nodes WHERE jsonb_typeof(notes) = 'array'";

/// What a dry run counts here.
pub(crate) async fn count<C: ConnectionTrait>(db: &C, counts: &mut Counts) -> Result<(), Error> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            format!("SELECT COUNT(*) AS n {HELD}"),
            [hung().into()],
        ))
        .await?;
    counts.sources = row
        .map(|row| row.try_get::<i64>("", "n"))
        .transpose()?
        .unwrap_or(0)
        .max(0) as usize;
    counts.child_notes = number(db, NOTES).await?;
    counts.threads = number(db, THREADS).await?;
    counts.wants = number(db, WANTS).await?;
    Ok(())
}

/// Moves them ([module docs](self)).
pub(crate) async fn run(
    db: &DatabaseConnection,
    site_owner: Option<i32>,
    counts: &mut Counts,
) -> Result<(), Halt> {
    for id in held(db).await? {
        counts.sources += 1;
        her_pen()?;
        counts.hung += usize::from(hanging::hang(db, site_owner, &id).await?);
    }
    let owners = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT DISTINCT user_id FROM agent_nodes WHERE user_id > 0 ORDER BY user_id",
        ))
        .await?;
    for row in owners {
        let user_id: i32 = row.try_get("", "user_id")?;
        for shown in nodes::list(db, user_id).await? {
            let place = nodes::Place {
                form: shown.form,
                key: shown.key.clone(),
                title: shown.title.clone(),
            };
            for note in &shown.notes {
                counts.child_notes += 1;
                her_pen()?;
                let put = child::put(db, user_id, site_owner, &place, &note.event, note).await?;
                counts.child_put += usize::from(put);
            }
        }
    }
    let now = crate::clock::now();
    counts.threads = number(db, THREADS).await?;
    counts.wants = number(db, WANTS).await?;
    for user_id in with_threads(db).await? {
        her_pen()?;
        let stepped = home_intents::threads_intended(db, user_id, site_owner, now).await?;
        counts.intended += stepped.intended;
        counts.refused += stepped.refused;
    }
    her_pen()?;
    let stepped = home_intents::wants_in_step(db, site_owner).await?;
    counts.intended += stepped.intended;
    counts.refused += stepped.refused;
    Ok(())
}
