//! What is on her mind about someone (`thread`) and what she wants for
//! herself (`want`) as her intents on her nodes (decision D5; the native
//! host's `NodeHomeIntents`), through the core's one gate.
//!
//! - **A thread** about an account, in its own private talk: her intent to
//!   bring it up (`say`, hers), on 「关于你」 there, waking at its due time
//!   in her zone or, with none, waiting for her next chance. It rests on
//!   their message the thread grew from (kept on the row as `from` since her
//!   inner pass names it, [`origin_of`]) and on the row (`memory:<id>`), and
//!   must pass `intents::admit`: one with no such message of theirs (kept
//!   before, or grown from her night) is refused and stays only a thread.
//! - **A want**: her intent on 「她自己」 in her own audience, resting on
//!   the row; hers only, only to say, nothing to wake it, as the native host
//!   takes it (the core's gate is for what rests on someone's words).
//!
//! The rows stay where they are, with their other readers (her inner pass,
//! her night, what she is shown). Each row and its intent are kept in step
//! both ways ([`in_step`], [`in_step_own`]), wherever either changes: a
//! row retired settles its intent (a thread she wrote first about is said,
//! one taken up done, anything else dropped; a want come true is done,
//! anything else dropped); an intent settled (said when it came due,
//! called off by their words) retires its row, so it is not raised again
//! the old way. A row once an intent whose intent is gone since (its node
//! forgotten) is let go too, never intended again. Past
//! [`THREADS_WAITING`] waiting on 「关于你」, the oldest is let go, with its
//! row.
//!
//! While she reads her nodes, a thread that is an intent is no longer
//! raised by her writing first the old way ([`not_intents`]): it comes
//! due as an intent. Only while her nodes are written, only with her pen.
//! A group's threads stay as they are.

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use myriad_nodes::{
    body::{Action, IntentStatus, Origin, Trigger},
    entries::{self, FactRowsInput, Start, row_id_from},
    intents::{self, AdmitInput},
    trust::Known,
};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, EntityTrait, Statement};
use serde_json::{Value, json};

use super::{
    Author, Error, RowIn, audience, hanging, homes,
    intent_store::{self, Held},
    read, write,
};
use crate::models::entities::agent_memories;
use crate::services::agent::memory::unified;

/// The intent of a thread row is `thread-<row id>`, of a want `want-<row id>`.
pub(crate) const THREAD: &str = "thread-";
pub(crate) const WANT: &str = "want-";
/// Her waiting intents from threads on 「关于你」, at most: the bound the
/// threads themselves keep, so they never crowd out what they told her.
pub(crate) const THREADS_WAITING: usize = 8;
/// Rows of a kind looked at in one pass, at most.
const ROWS: u64 = 64;

/// The intent of the row `id`, of the kind `prefix` names.
pub(crate) fn intent_id(prefix: &str, id: &str) -> String {
    let id: String = id
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_')
        .collect();
    format!("{prefix}{id}").chars().take(64).collect()
}

/// What a row retiring as `reason` says of its intent.
pub(crate) fn status_of(source: &str, reason: Option<&str>) -> IntentStatus {
    if let Some(word) = reason.and_then(|reason| reason.strip_prefix("intent_"))
        && let Ok(status) = serde_json::from_value::<IntentStatus>(json!(word))
        && status != IntentStatus::Waiting
    {
        return status;
    }
    match (source, reason) {
        ("thread", Some("reached_out")) => IntentStatus::Said,
        ("thread", Some("taken_up")) => IntentStatus::Done,
        ("want", Some("came_true")) => IntentStatus::Done,
        _ => IntentStatus::Dropped,
    }
}

fn word(status: IntentStatus) -> String {
    serde_json::to_value(status)
        .ok()
        .and_then(|word| word.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// Their message `said` in their own private talk at `at`, which her inner
/// pass grew a thread from (`message:<id>`): the one saying it, else their
/// latest then.
pub(crate) async fn origin_of(
    db: &DatabaseConnection,
    user_id: i32,
    said: &str,
    at: DateTime<Utc>,
) -> Option<String> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT m.id FROM agent_messages m JOIN agent_sessions s ON s.id = m.session_id \
             WHERE s.user_id = $1 AND s.context->>'mode' = 'chat' \
               AND (s.context->>'venue') IS NULL AND m.role = 'user' AND m.created_at <= $3 \
             ORDER BY (left(m.content, 1500) = $2) DESC, m.id DESC LIMIT 1",
            [
                user_id.into(),
                said.into(),
                (at + chrono::Duration::minutes(2)).fixed_offset().into(),
            ],
        ))
        .await
        .ok()
        .flatten()?;
    let id: i32 = row.try_get("", "id").ok()?;
    Some(format!("message:{id}"))
}

/// Whether `message` (`message:<id>`) is `user_id`'s own, in their own
/// private talk.
async fn theirs<C: ConnectionTrait>(db: &C, user_id: i32, message: &str) -> Result<bool, Error> {
    let Some(id) = message
        .strip_prefix("message:")
        .and_then(|id| id.parse::<i32>().ok())
    else {
        return Ok(false);
    };
    Ok(db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT 1 AS one FROM agent_messages m JOIN agent_sessions s ON s.id = m.session_id \
             WHERE m.id = $1 AND s.user_id = $2 AND s.context->>'mode' = 'chat' \
               AND (s.context->>'venue') IS NULL AND m.role = 'user'",
            [id.into(), user_id.into()],
        ))
        .await?
        .is_some())
}

fn evidence(row: &agent_memories::Model) -> Value {
    row.evidence
        .as_deref()
        .and_then(|evidence| serde_json::from_str(evidence).ok())
        .unwrap_or(Value::Null)
}

fn cut(text: &str) -> String {
    homes::clean(text)
        .chars()
        .take(myriad_nodes::limits::TEXT_CHARS)
        .collect::<String>()
        .trim()
        .to_string()
}

/// One side kept in step: whose and which audience, the node its intents
/// are on, and its rows' kind.
struct Side {
    user_id: i32,
    audience: String,
    node: String,
    prefix: &'static str,
    source: &'static str,
}

impl Side {
    async fn live(&self, db: &DatabaseConnection) -> Result<Vec<agent_memories::Model>, Error> {
        Ok(match self.source {
            "want" => unified::own_rows(db, self.source, ROWS).await?,
            _ => {
                unified::venue_source_rows(db, Some(self.user_id), "private", self.source, ROWS)
                    .await?
            }
        })
    }

    /// Retires the row `id` as `reason`; whether it was still there.
    async fn retire(&self, db: &DatabaseConnection, id: &str, reason: &str) -> Result<bool, Error> {
        Ok(match self.source {
            "want" => unified::retire_own(db, id, reason).await?,
            _ => unified::retire(db, self.user_id, &[id.to_string()], reason).await? > 0,
        })
    }

    /// Whether the intent `id` was ever written on her nodes.
    async fn ever(&self, db: &DatabaseConnection, id: &str) -> Result<bool, Error> {
        let row = row_id_from(
            "node.intent",
            &self.node,
            &format!("intent:{id}"),
            "waiting",
        );
        Ok(db
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT 1 AS one FROM agent_node_rows WHERE user_id = $1 AND row_id = $2",
                [self.user_id.into(), row.into()],
            ))
            .await?
            .is_some())
    }

    /// The intents of this side, as the fold has them now.
    async fn held(&self, db: &DatabaseConnection) -> Result<Vec<Held>, Error> {
        Ok(intent_store::held(db, self.user_id, &self.audience)
            .await?
            .into_iter()
            .filter(|held| held.node == self.node && held.id.starts_with(self.prefix))
            .collect())
    }
}

/// What a pass did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Stepped {
    pub intended: usize,
    pub settled: usize,
    pub retired: usize,
    pub refused: usize,
}

/// `user_id`'s threads in their private talk and her intents from them,
/// kept in step ([module docs](self)). Never fails the caller.
pub(crate) async fn in_step(db: &DatabaseConnection, user_id: i32) {
    if !super::mode().writes() || user_id <= 0 {
        return;
    }
    let site_owner = hanging::site_owner(db).await;
    if let Err(error) = threads_in_step(db, user_id, site_owner, crate::clock::now()).await {
        tracing::warn!(%error, "[memory-nodes] threads and her intents not kept in step");
    }
}

/// Her wants and her intents from them, kept in step. Never fails the
/// caller.
pub(crate) async fn in_step_own(db: &DatabaseConnection) {
    if !super::mode().writes() {
        return;
    }
    let site_owner = hanging::site_owner(db).await;
    if let Err(error) = wants_in_step(db, site_owner).await {
        tracing::warn!(%error, "[memory-nodes] wants and her intents not kept in step");
    }
}

/// [`in_step`] at `now`.
pub(crate) async fn threads_in_step(
    db: &DatabaseConnection,
    user_id: i32,
    site_owner: Option<i32>,
    now: DateTime<Utc>,
) -> Result<Stepped, Error> {
    let talk = audience::private_talk(user_id, site_owner);
    let key = audience::person_key(&talk, user_id);
    let side = Side {
        user_id,
        node: myriad_nodes::node_id(&talk, &key)?,
        audience: talk,
        prefix: THREAD,
        source: "thread",
    };
    let mut stepped = step(db, &side, now).await?;
    // Past the bound, the oldest waiting go, and their rows with them.
    let Some(pen) = crate::services::her_log::pen::think() else {
        return Ok(stepped);
    };
    let mut waiting: Vec<Held> = side
        .held(db)
        .await?
        .into_iter()
        .filter(Held::waiting)
        .collect();
    waiting.sort_by(|a, b| a.at.cmp(&b.at).then_with(|| a.id.cmp(&b.id)));
    let over = waiting.len().saturating_sub(THREADS_WAITING);
    for held in waiting.iter().take(over) {
        let row = intent_store::settle_row(held, IntentStatus::Dropped);
        write::write(db, side.user_id, &side.audience, &[row], Some(&pen)).await?;
        stepped.settled += 1;
        if let Some(id) = held.id.strip_prefix(THREAD)
            && side.retire(db, id, "intent_dropped").await?
        {
            stepped.retired += 1;
        }
    }
    Ok(stepped)
}

/// [`in_step_own`]: her own audience is kept with the site owner.
pub(crate) async fn wants_in_step(
    db: &DatabaseConnection,
    site_owner: Option<i32>,
) -> Result<Stepped, Error> {
    let Some(home) = audience::hers(site_owner) else {
        return Ok(Stepped::default());
    };
    let side = Side {
        user_id: home.user_id,
        node: myriad_nodes::node_id(&home.audience, "her")?,
        audience: home.audience,
        prefix: WANT,
        source: "want",
    };
    step(db, &side, crate::clock::now()).await
}

/// The rows that start 「她自己」 in her own audience, when it is not there.
async fn her_node(db: &DatabaseConnection, side: &Side) -> Result<Vec<RowIn>, Error> {
    if read::node(db, side.user_id, &side.audience, &side.node)
        .await?
        .is_some()
    {
        return Ok(Vec::new());
    }
    let made = entries::fact_rows(FactRowsInput {
        audience: side.audience.clone(),
        sync: myriad_nodes::Sync::Shared,
        start: Some(Start {
            key: Some("her".into()),
            name: homes::HER_NAME.into(),
            aliases: Vec::new(),
        }),
        key: None,
        facts: Vec::new(),
    })?;
    Ok(made
        .rows
        .into_iter()
        .map(|row| RowIn {
            id: row.id,
            node: made.node.clone(),
            kind: row.kind,
            body: row.body,
            author: Author::Her,
        })
        .collect())
}

/// What `row` is as her intent, if the gate takes it: its text, what it
/// rests on, what wakes it.
async fn as_intent(
    db: &DatabaseConnection,
    side: &Side,
    row: &agent_memories::Model,
    now: DateTime<Utc>,
) -> Result<Option<(String, Vec<String>, Option<Trigger>)>, Error> {
    let cite = format!("{}{}", myriad_nodes::body::MEMORY_REF, row.id);
    let content = cut(&row.content);
    if content.is_empty() {
        return Ok(None);
    }
    if side.source == "want" {
        // Hers alone, to say, resting on a live want of her own.
        let rests = row.source == "want"
            && row.venue == unified::OWN_VENUE
            && row.user_id.is_none()
            && row.invalid_at.is_none();
        return Ok(rests.then(|| (content, vec![cite], None)));
    }
    let evidence = evidence(row);
    let about = evidence["about"]
        .as_str()
        .map(homes::clean)
        .unwrap_or_default();
    let text = if about.is_empty() {
        content
    } else {
        cut(&format!("{about} — {content}"))
    };
    let from = evidence["from"].as_str().map(str::to_string);
    let mut known = Known::default();
    if let Some(from) = &from
        && theirs(db, side.user_id, from).await?
    {
        known.partner.insert(from.clone());
    }
    let because: Vec<String> = from
        .into_iter()
        .chain([cite])
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let trigger = evidence["due"]
        .as_str()
        .and_then(|due| DateTime::parse_from_rfc3339(due).ok())
        .map(|due| Trigger::time(due.timestamp_micros(), &intent_store::zone_name(now)));
    let admitted = intents::admit(AdmitInput {
        origin: Origin::Her,
        action: Some(Action::Say),
        because: because.clone(),
        message: None,
        trigger: trigger.clone(),
        known,
    })?;
    Ok(admitted.admitted.then_some((text, because, trigger)))
}

/// One side kept in step at `now`.
async fn step(db: &DatabaseConnection, side: &Side, now: DateTime<Utc>) -> Result<Stepped, Error> {
    let Some(pen) = crate::services::her_log::pen::think() else {
        return Ok(Stepped::default());
    };
    let mut stepped = Stepped::default();
    let mut live = side.live(db).await?;
    // Oldest first: her intents are as old as their rows, for the bound.
    live.sort_by(|a, b| {
        a.created_at
            .cmp(&b.created_at)
            .then_with(|| a.id.cmp(&b.id))
    });
    let held = side.held(db).await?;
    // Intents to rows.
    for held in &held {
        let Some(id) = held.id.strip_prefix(side.prefix) else {
            continue;
        };
        let alive = live
            .iter()
            .any(|row| intent_id(side.prefix, &row.id) == held.id);
        if held.waiting() && !alive {
            let reason = agent_memories::Entity::find_by_id(id.to_string())
                .one(db)
                .await?
                .and_then(|row| row.invalid_reason);
            let status = status_of(side.source, reason.as_deref());
            let row = intent_store::settle_row(held, status);
            write::write(db, side.user_id, &side.audience, &[row], Some(&pen)).await?;
            stepped.settled += 1;
        } else if !held.waiting() && alive {
            let reason = format!("intent_{}", word(held.status));
            if side.retire(db, id, &reason).await? {
                stepped.retired += 1;
            }
        }
    }
    // Rows to intents.
    for row in &live {
        let id = intent_id(side.prefix, &row.id);
        if held.iter().any(|held| held.id == id) {
            continue;
        }
        // An intent once, gone since: its row is let go.
        if side.ever(db, &id).await? {
            if side.retire(db, &row.id, "intent_dropped").await? {
                stepped.retired += 1;
            }
            continue;
        }
        let Some((text, because, trigger)) = as_intent(db, side, row, now).await? else {
            stepped.refused += 1;
            continue;
        };
        let (node, mut rows) = if side.source == "want" {
            (side.node.clone(), her_node(db, side).await?)
        } else {
            intent_store::about_them(db, side.user_id, &side.audience).await?
        };
        let held = Held {
            node,
            id,
            text,
            because,
            status: IntentStatus::Waiting,
            trigger: trigger.as_ref().map(|trigger| json!(trigger)),
            origin: Some(Origin::Her),
            action: Some(Action::Say),
            said_by: json!(["her"]),
            sync: json!("shared"),
            base: Vec::new(),
            at: now.timestamp_micros(),
        };
        rows.push(intent_store::settle_row(&held, IntentStatus::Waiting));
        write::write(db, side.user_id, &side.audience, &rows, Some(&pen)).await?;
        stepped.intended += 1;
    }
    Ok(stepped)
}

/// `threads` of `user_id`'s private talk without those that are her
/// intents, while she reads her nodes: those come due as intents. Shown
/// as they are otherwise.
pub(crate) async fn not_intents(
    db: &DatabaseConnection,
    user_id: i32,
    threads: Vec<crate::services::agent::merope::threads::Thread>,
) -> Vec<crate::services::agent::merope::threads::Thread> {
    if threads.is_empty() {
        return threads;
    }
    let intents = as_intents(db, user_id).await;
    threads
        .into_iter()
        .filter(|thread| !intents.contains(&intent_id(THREAD, &thread.id)))
        .collect()
}

/// The intents (`thread-<row id>`) `user_id`'s threads are, while she reads
/// her nodes: the old ways of raising and fading them leave those be (the
/// bound of [`THREADS_WAITING`] is theirs instead). None otherwise, or when
/// they cannot be read: then the old ways go on as before.
pub(crate) async fn as_intents(db: &DatabaseConnection, user_id: i32) -> BTreeSet<String> {
    if !super::mode().reads() || user_id <= 0 {
        return BTreeSet::new();
    }
    let site_owner = hanging::site_owner(db).await;
    let talk = audience::private_talk(user_id, site_owner);
    match intent_store::held(db, user_id, &talk).await {
        Ok(held) => held
            .into_iter()
            .filter(|held| held.id.starts_with(THREAD))
            .map(|held| held.id)
            .collect(),
        Err(error) => {
            tracing::warn!(%error, "[memory-nodes] her intents could not be read");
            BTreeSet::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_retiring_says_how_its_intent_ended() {
        assert_eq!(status_of("thread", Some("reached_out")), IntentStatus::Said);
        assert_eq!(status_of("thread", Some("taken_up")), IntentStatus::Done);
        assert_eq!(status_of("thread", Some("faded")), IntentStatus::Dropped);
        assert_eq!(
            status_of("thread", Some("superseded")),
            IntentStatus::Dropped
        );
        assert_eq!(status_of("want", Some("came_true")), IntentStatus::Done);
        assert_eq!(status_of("want", Some("let_go")), IntentStatus::Dropped);
        assert_eq!(status_of("want", None), IntentStatus::Dropped);
        // Retired by its intent: as the intent ended.
        assert_eq!(status_of("thread", Some("intent_said")), IntentStatus::Said);
        assert_eq!(status_of("want", Some("intent_done")), IntentStatus::Done);
        assert_eq!(
            status_of("thread", Some("intent_waiting")),
            IntentStatus::Dropped
        );
    }

    #[test]
    fn an_intent_id_is_an_item_id() {
        let id = intent_id(THREAD, "mem_0123456789abcdef0123456789abcdef");
        assert_eq!(id, "thread-mem_0123456789abcdef0123456789abcdef");
        assert!(id.len() <= 64);
        assert_eq!(intent_id(WANT, "own_x:y"), "want-own_xy");
    }
}
