//! When someone asked her not to come to them for a while (「今晚别找我了」):
//! until then she says nothing to them unasked. It is do-not-disturb with an
//! end (`speech::admit`), read from its row, not from what comes to mind.
//!
//! Their words are judged as she takes from them what they leave her for
//! later (`memory_nodes::intending`): the model names the stretch, and when
//! it ends is worked out from when they said it, on their clock, or on hers
//! where none of theirs is kept (the site keeps none)
//! (`myriad_merope::bound_end`). One with no end that can be worked out
//! (「忙完再说」), or that only starts later (「下周别找我」), makes no row: it
//! is weighed with the rest, as she decides whether to write. What they told
//! her to say or do, she still does.
//!
//! One row each: what they asked last and what they asked before are the
//! later of the two ends (「这三天别找我」, then 「今晚也别找我」), unless they
//! say the end is another (「改成今晚就行」) or take it back beside it: then the
//! new one is theirs, also when it is shorter. Their saying she may come to
//! them again ends it now. It is kept under its own source, with no
//! word of theirs in it (until when, and which message asked); seen with
//! their other memories, and deleted there. Nothing else ends it: not their
//! asking her not to keep something, for one. Heard by no one ([`NO_ONE`]),
//! so a build that does not know its source recalls it nowhere and sends it
//! to none of her other ends. Past its end it stays ([`KEPT`], and longer
//! while anything of hers that came due meanwhile waits), so that is told
//! from the rest of what she means to say.
//!
//! Only their words make one. A judgment that failed or does not read makes
//! none: it is asked once more, and then they are weighed as anyone.

use std::time::Duration;

use chrono::{DateTime, Utc};
use myriad_merope::bound_end::{self, BoundEnd};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseBackend, DatabaseConnection, DbErr, EntityTrait,
    QueryFilter, Statement, TransactionTrait,
};
use serde_json::{Value, json};

use super::clock;
use crate::models::entities::agent_memories;
use crate::services::agent::memory::unified::{self, Audience, NO_ONE};
use crate::services::agent::speech::Wish;

pub use unified::LEFT_ALONE_SOURCE as SOURCE;

/// One whose end does not read holds this long from when it was written.
const UNREAD_HOLDS: chrono::Duration = chrono::Duration::days(7);
/// How long one over is kept, for what came due before its end; longer while
/// anything of that waits.
const KEPT: chrono::Duration = chrono::Duration::days(2);
/// The rows looked at: there is one, unless two were written at once.
const ROWS: u64 = 20;
/// How long whether she is to leave them be may take to read.
const READ_WITHIN: Duration = Duration::from_secs(5);

/// What stands between her and coming to someone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::services::agent) enum Standing {
    /// They asked to be left alone, and it is not over: until when.
    Held(DateTime<Utc>),
    /// They did not, or it is over: when the last was over, while its row
    /// is kept.
    Free { ended: Option<DateTime<Utc>> },
    /// Not told (the row cannot be read, in time or at all).
    Unknown,
}

/// What their message `message` (its id), said at `at`, came to: to be left
/// alone till `ends`, as the model named them, or that she may come to them
/// again (`unbound`); `changed` when they say the end they gave is another.
/// A message before the one that made the row is not heeded: two judgments
/// may come in the other order.
pub(in crate::services::agent::merope) async fn heed(
    db: &DatabaseConnection,
    user_id: i32,
    message: i32,
    marks: &Marks,
    at: DateTime<Utc>,
) -> Result<(), DbErr> {
    let zone = clock::zone();
    let asked = marks
        .ends
        .iter()
        .filter_map(|end| end.ends_at(at, None, &zone))
        .max();
    if asked.is_none() && !marks.unbound {
        return Ok(());
    }
    let txn = db.begin().await?;
    // One at a time of each person's: what was asked last is what holds.
    txn.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT pg_advisory_xact_lock(hashtext('left_alone'), $1)",
        [user_id.into()],
    ))
    .await?;
    let rows = unified::latest_of(&txn, user_id, SOURCE, ROWS).await?;
    if rows.iter().any(|row| told_by(row) >= message) {
        return Ok(());
    }
    let held = rows.iter().map(till).filter(|until| *until > at).max();
    let until = match (asked, held) {
        (Some(asked), Some(held)) if !marks.replaces() => asked.max(held),
        (Some(asked), _) => asked,
        // An unbound: over now. Nothing to end, it is kept all the same (over
        // when the last was), so that one they asked before and that is judged
        // after it is not heeded.
        (None, Some(_)) => at,
        (None, None) => rows
            .iter()
            .map(till)
            .max()
            .filter(|ended| *ended + KEPT > at)
            .unwrap_or(at),
    };
    agent_memories::Entity::delete_many()
        .filter(agent_memories::Column::UserId.eq(user_id))
        .filter(agent_memories::Column::Source.eq(SOURCE))
        .exec(&txn)
        .await?;
    unified::remember(
        &txn,
        unified::NewMemory {
            user_id,
            kind: unified::MemoryKind::Preference,
            content: format!("→ {}", until.with_timezone(&zone).format("%Y-%m-%d %H:%M")),
            evidence: Some(
                json!({ "until": until.to_rfc3339(), "from": format!("message:{message}") })
                    .to_string(),
            ),
            speaker: unified::Speaker::User,
            source: SOURCE,
            audience: Audience::private(NO_ONE),
            importance: 1.0,
            concepts: Vec::new(),
        },
    )
    .await?;
    txn.commit().await?;
    tracing::info!(user_id, %until, "[Merope] until when they are to be left alone");
    Ok(())
}

/// What their message was judged to say of it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(in crate::services::agent::merope) struct Marks {
    pub ends: Vec<BoundEnd>,
    pub unbound: bool,
    pub changed: bool,
}

impl Marks {
    /// The marks of `answer`, as the judgment gave them.
    pub(in crate::services::agent::merope) fn of(answer: &str) -> Self {
        Self {
            ends: bound_end::read_bounds(answer),
            unbound: bound_end::read_unbound(answer),
            changed: bound_end::read_change(answer),
        }
    }

    /// Whether what they asked is in place of what they asked before.
    fn replaces(&self) -> bool {
        self.unbound || self.changed
    }
}

/// Whether `user_id` asked to be left alone and it is not over. Not told
/// (the rows cannot be read), she does not come to them now, and looks
/// again when she next would: nothing is kept of that.
pub(in crate::services::agent) async fn holds(db: &DatabaseConnection, user_id: i32) -> bool {
    !matches!(standing(db, user_id).await, Standing::Free { .. })
}

/// What stands between her and coming to `user_id` now.
pub(in crate::services::agent) async fn standing(
    db: &DatabaseConnection,
    user_id: i32,
) -> Standing {
    match tokio::time::timeout(READ_WITHIN, read(db, user_id)).await {
        Ok(Ok(standing)) => standing,
        Ok(Err(error)) => {
            tracing::warn!(%error, user_id, "[Merope] whether they asked to be left alone is not known");
            Standing::Unknown
        }
        Err(_) => Standing::Unknown,
    }
}

async fn read(db: &DatabaseConnection, user_id: i32) -> Result<Standing, DbErr> {
    let now = clock::now();
    let rows = unified::latest_of(db, user_id, SOURCE, ROWS).await?;
    let mut held = None;
    let mut ended = None;
    let mut gone = Vec::new();
    for row in &rows {
        let ends = till(row);
        if ends > now {
            held = held.max(Some(ends));
        } else {
            ended = ended.max(Some(ends));
            if ends + KEPT <= now {
                gone.push(row.id.clone());
            }
        }
    }
    if !gone.is_empty()
        && !super::memory_nodes::late::any_left(db, user_id, ended.unwrap_or(now)).await
    {
        ended = None;
        // Left for later it costs nothing; it is not worth not knowing over.
        let deleted = agent_memories::Entity::delete_many()
            .filter(agent_memories::Column::UserId.eq(user_id))
            .filter(agent_memories::Column::Source.eq(SOURCE))
            .filter(agent_memories::Column::Id.is_in(gone))
            .exec(db)
            .await;
        if let Err(error) = deleted {
            tracing::warn!(%error, user_id, "[Merope] what is over is not deleted");
        }
    }
    Ok(match held {
        Some(until) => Standing::Held(until),
        None => Standing::Free { ended },
    })
}

/// What the model is told of what they asked already, for the judgment of
/// what they say next (`memory_nodes::intending`): nothing when there is no
/// end to come.
pub(in crate::services::agent::merope) async fn known(
    db: &DatabaseConnection,
    user_id: i32,
) -> Option<String> {
    let Standing::Held(until) = standing(db, user_id).await else {
        return None;
    };
    Some(format!(
        "They asked you not to come to them until {}.",
        until
            .with_timezone(&clock::zone())
            .format("%Y-%m-%d %H:%M (%A)")
    ))
}

fn evidence(row: &agent_memories::Model) -> Value {
    row.evidence
        .as_deref()
        .and_then(|evidence| serde_json::from_str(evidence).ok())
        .unwrap_or_default()
}

/// Until when `row` holds.
fn till(row: &agent_memories::Model) -> DateTime<Utc> {
    evidence(row)["until"]
        .as_str()
        .and_then(|until| DateTime::parse_from_rfc3339(until).ok())
        .map(|until| until.to_utc())
        .unwrap_or_else(|| row.created_at.to_utc() + UNREAD_HOLDS)
}

/// The message whose words made `row`: its id, 0 when that is not told.
fn told_by(row: &agent_memories::Model) -> i32 {
    evidence(row)["from"]
        .as_str()
        .and_then(|from| from.strip_prefix("message:"))
        .and_then(|id| id.parse().ok())
        .unwrap_or(0)
}

/// Whether `wish` is hers to leave unsaid while they are to be left alone:
/// not what they called for, nor what they told her to say or do, come due.
pub(in crate::services::agent) fn is_hers(wish: &Wish) -> bool {
    let told = wish
        .thing
        .as_deref()
        .is_some_and(|thing| thing.starts_with(TOLD));
    !wish.called && !told
}

/// The thing a wish to say what they told her to, come due, is about, before
/// its id; hers come due are under [`OWN`].
pub(in crate::services::agent) const TOLD: &str = "intent|";
/// As [`TOLD`], what she meant to bring up herself.
pub(in crate::services::agent) const OWN: &str = "own|";
