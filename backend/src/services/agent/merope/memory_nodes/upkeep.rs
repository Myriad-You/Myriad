//! Her upkeep of her nodes through the shared core (decision N2), once a
//! night while she sleeps (the site's hourly upkeep looks, and goes over
//! her nodes the first time it finds her asleep on a night it has not yet):
//! which nodes are due (`myriad_nodes::queue`: only on a change, never by
//! the calendar), what she is shown of one, with the inbox, her day and
//! the language she speaks with them, and how her answer becomes rows
//! (`myriad_nodes::upkeep`), all the core's. She judges, on the prompt the native host asks; the rows go in
//! through the one way rows get in ([`super::write::write_with`]), and an
//! inbox item she put on a node leaves the inbox with them.
//!
//! How much: one call per node, each written before the next is chosen
//! (what one wrote may make another due), each node once a night: a whole
//! and its part each waking the other go on to the next night, so a night
//! makes at most one call per node. An answer the core could not read is
//! the same: the node waits for the next night (the core sees nothing of a
//! first such answer, and all of a second in a row). The site owner's
//! private talk is gone over however much waits, with no count set on
//! her; every other account's at most [`PER_DAY`] calls a day, counted in the
//! runtime registry ([`DAY_NAMESPACE`]) so a restart does not reset it.
//! Nothing due and nothing in the inbox: no call, no row. Only private
//! talks for now; only while her mind is here. A node holding nothing of
//! hers in 「现在」 yet is built first (`super::unbuilt`).
//!
//! An answer cut off at its output limit is not hers to read: nothing is
//! written, not even the mark, and the node stays due. Cut off twice with
//! nothing new on it, it waits for something new or a day
//! ([`CUT_OFF_NAMESPACE`]), so the same node is not asked hour after hour.

use std::collections::HashSet;
use std::time::Duration;

use myriad_nodes::{
    limits::{QUEUE_MAX, UPKEEP_INBOX},
    queue::{self, QueueInput},
    upkeep::{self, InboxItem, ReadInput, UpkeepInput},
};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::json;

use super::upkeep_candidates::{candidates, holding_sessions};
pub(crate) use super::upkeep_prompt::{plain, schema, system_prompt};
use super::{
    Author, Error, RowIn, audience, heads, inbox, known,
    write::{Beside, write_with},
};
use crate::services::agent::merope::call::{Ask, Failure, Voice};
use crate::services::her_log::pen::Thought;
use crate::services::runtime_registry as registry;

/// The nights her nodes were gone over, by her date (`clock::her_date`):
/// once a night.
const NIGHT_NAMESPACE: &str = "merope_memory_node_upkeep_night";
/// Calls a day for an account other than the site owner's, at most.
pub(crate) const PER_DAY: u32 = 6;
/// Audiences gone over in one upkeep, at most: the site owner's first.
const AUDIENCES: i64 = 10;
const TIMEOUT: Duration = Duration::from_secs(90);
const OPERATION: &str = "memory_node_upkeep";
const SCHEMA_NAME: &str = "node_upkeep";
/// Where the calls an account's upkeep made today are counted.
pub(crate) const DAY_NAMESPACE: &str = "merope_memory_node_upkeep";
/// The mark written when the core read her answer and the site could not
/// write it, so the same answer is not asked for again and again.
const REFUSED: &str = "refused_here";
/// Where an upkeep cut off at its output limit is kept, by node (or the
/// inbox), ids only: what was newest on it then, how many times in a row,
/// and when.
pub(crate) const CUT_OFF_NAMESPACE: &str = "merope_memory_node_upkeep_cut_off";
/// Cut off this many times with nothing new on it, a node waits ...
const CUT_OFF_WAITS_AFTER: u32 = 2;
/// ... for something new, or this long.
const CUT_OFF_WAIT: i64 = 86_400;
/// How long what was cut off is kept at all.
const CUT_OFF_KEPT: i64 = 7 * 86_400;

/// A node's upkeep (or the inbox's) cut off at its output limit.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct CutOff {
    /// The id of what was newest on it then.
    seen: String,
    /// Cut off this many times in a row with that newest.
    times: u32,
    /// When it last was, Unix seconds.
    at: i64,
}

impl CutOff {
    /// When it is next asked, if not now: how many times it was cut off,
    /// and when the wait is over.
    fn waits(&self, seen: &str, now: i64) -> Option<(u32, i64)> {
        let until = self.at + CUT_OFF_WAIT;
        (self.seen == seen && self.times >= CUT_OFF_WAITS_AFTER && now < until)
            .then_some((self.times, until))
    }

    /// What is kept once it is cut off again, `seen` newest on it now.
    fn again(kept: Option<Self>, seen: String, now: i64) -> Self {
        let times = match kept {
            Some(kept) if kept.seen == seen => kept.times + 1,
            _ => 1,
        };
        Self {
            seen,
            times,
            at: now,
        }
    }
}

fn cut_off_key(user_id: i32, audience: &str, node: Option<&str>) -> String {
    format!("{user_id}:{audience}:{}", node.unwrap_or("inbox"))
}

/// The id of the newest row on `node`: what a change to it changes.
fn newest_on(rows: &[myriad_persona_log::Row], node: &str) -> String {
    let object = format!("node:{node}");
    rows.iter()
        .filter(|row| row.object == object)
        .max_by(|left, right| left.hlc.cmp(&right.hlc))
        .map(|row| row.id.clone())
        .unwrap_or_default()
}

/// What `node`'s upkeep (the inbox's, none) was cut off with before, and
/// whether it waits now, told in one line when it does.
async fn kept_cut_off(
    db: &DatabaseConnection,
    user_id: i32,
    audience: &str,
    node: Option<&str>,
    seen: &str,
) -> (Option<CutOff>, bool) {
    // Unread, it was never cut off: asked as ever, the run not stopped.
    let kept: Option<CutOff> = match registry::get(
        db,
        CUT_OFF_NAMESPACE,
        &cut_off_key(user_id, audience, node),
    )
    .await
    {
        Ok(kept) => kept,
        Err(error) => {
            tracing::warn!(%error, "[memory-nodes] what upkeep was cut off with could not be read");
            None
        }
    };
    let now = crate::clock::now().timestamp();
    let waits = kept.as_ref().and_then(|kept| kept.waits(seen, now));
    if let Some((times, until)) = waits {
        tracing::info!(
            node = node.unwrap_or("inbox"),
            times,
            again_at = %chrono::DateTime::from_timestamp(until, 0).unwrap_or_default().to_rfc3339(),
            "[memory-nodes] upkeep waits: cut off at its output limit before"
        );
    }
    (kept, waits.is_some())
}

/// Keeps that `node`'s upkeep (the inbox's, none) was cut off once more.
async fn keep_cut_off(
    db: &DatabaseConnection,
    user_id: i32,
    audience: &str,
    node: Option<&str>,
    kept: Option<CutOff>,
    seen: String,
) {
    let now = crate::clock::now().timestamp();
    let cut = CutOff::again(kept, seen, now);
    let times = cut.times;
    let written = registry::put(
        db,
        CUT_OFF_NAMESPACE,
        &cut_off_key(user_id, audience, node),
        registry::RegistryIdentity {
            subject_id: Some(user_id),
            owner_id: Some(user_id),
            tapp_id: None,
            runtime_id: None,
        },
        &cut,
        now + CUT_OFF_KEPT,
    )
    .await;
    match written {
        Ok(_) => tracing::info!(
            node = node.unwrap_or("inbox"),
            times,
            "[memory-nodes] upkeep cut off at its output limit: nothing written, asked again later"
        ),
        Err(error) => {
            tracing::warn!(%error, "[memory-nodes] upkeep cut off, and that could not be kept")
        }
    }
}

/// The private audiences with nodes or an inbox, the site owner's first.
async fn audiences<C: ConnectionTrait>(db: &C) -> Result<Vec<(i32, String)>, Error> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT user_id, audience FROM ( \
               SELECT user_id, audience FROM agent_node_heads WHERE audience LIKE 'personal:%' \
               UNION SELECT user_id, audience FROM agent_node_inbox \
                 WHERE placed_at IS NULL AND audience LIKE 'personal:%' \
             ) talks ORDER BY (audience = $1) DESC, user_id LIMIT $2",
            [audience::owners_talk().into(), AUDIENCES.into()],
        ))
        .await?;
    rows.iter()
        .map(|row| Ok((row.try_get("", "user_id")?, row.try_get("", "audience")?)))
        .collect()
}

fn day_key(user_id: i32, audience: &str) -> String {
    format!(
        "{user_id}:{audience}:{}",
        crate::services::agent::merope::clock::today()
    )
}

/// Calls the upkeep of `user_id`'s `audience` made today.
pub(crate) async fn calls_today<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
) -> Result<u32, Error> {
    Ok(
        registry::get::<u32>(db, DAY_NAMESPACE, &day_key(user_id, audience))
            .await?
            .unwrap_or(0),
    )
}

pub(crate) async fn count_calls<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
    calls: u32,
) -> Result<(), Error> {
    let total = calls_today(db, user_id, audience).await? + calls;
    registry::put(
        db,
        DAY_NAMESPACE,
        &day_key(user_id, audience),
        registry::RegistryIdentity {
            subject_id: Some(user_id),
            owner_id: Some(user_id),
            tapp_id: None,
            runtime_id: None,
        },
        &total,
        crate::clock::now().timestamp() + 2 * 86_400,
    )
    .await?;
    Ok(())
}

/// Whether her nodes are to be gone over now: she is asleep, and they have
/// not been yet this night.
async fn nights_upkeep_due<C: ConnectionTrait>(db: &C) -> Result<bool, Error> {
    if crate::services::agent::merope::timing::asleep_now().is_none() {
        return Ok(false);
    }
    Ok(registry::get::<bool>(db, NIGHT_NAMESPACE, &night_key())
        .await?
        .is_none())
}

fn night_key() -> String {
    crate::services::agent::merope::timing::her_date().to_string()
}

async fn night_done<C: ConnectionTrait>(db: &C) -> Result<(), Error> {
    registry::put(
        db,
        NIGHT_NAMESPACE,
        &night_key(),
        registry::RegistryIdentity {
            subject_id: None,
            owner_id: None,
            tapp_id: None,
            runtime_id: None,
        },
        &true,
        crate::clock::now().timestamp() + 3 * 86_400,
    )
    .await?;
    Ok(())
}

/// The site's hourly upkeep: what waits to be placed or hung each time,
/// her nodes once a night, the first time it finds her asleep.
pub(crate) async fn at_upkeep(db: &DatabaseConnection) {
    meanwhile(db).await;
    match nights_upkeep_due(db).await {
        Ok(true) => {}
        Ok(false) => return,
        Err(error) => {
            tracing::warn!(%error, "[memory-nodes] upkeep could not tell whether tonight's ran");
            return;
        }
    }
    if crate::services::her_log::pen::think().is_none() {
        return;
    }
    if let Err(error) = night_done(db).await {
        tracing::warn!(%error, "[memory-nodes] tonight's upkeep not marked; not going over");
        return;
    }
    go_over(db).await;
}

/// One whole upkeep now, whatever the hour: what waits to be placed or
/// hung, and her nodes.
pub(crate) async fn tick(db: &DatabaseConnection) {
    meanwhile(db).await;
    go_over(db).await;
}

/// What waits to be placed or hung, each hour.
async fn meanwhile(db: &DatabaseConnection) {
    // What a stretch could not place first, so what it puts in the inbox is
    // filed with the rest.
    super::placing::backlog::place_later(db).await;
    // What they took back and nobody has looked at again yet: asked here
    // too, in case they say nothing more (`take_back_check`).
    if crate::services::her_log::pen::think().is_some() {
        super::take_back_check::go_over_waiting(db).await;
        // What could not be put on her nodes when it was kept.
        super::hanging::hang_later(db).await;
    }
}

/// Her nodes of every private talk with something due or waiting.
async fn go_over(db: &DatabaseConnection) {
    let talks = match audiences(db).await {
        Ok(talks) => talks,
        Err(error) => {
            tracing::warn!(%error, "[memory-nodes] upkeep could not find what to go over");
            return;
        }
    };
    for (user_id, audience) in talks {
        let owners = audience == audience::owners_talk();
        let result = async {
            let at_most = if owners {
                usize::MAX
            } else {
                PER_DAY.saturating_sub(calls_today(db, user_id, &audience).await?) as usize
            };
            if at_most == 0 {
                return Ok(0);
            }
            let Some(pen) = crate::services::her_log::pen::think() else {
                tracing::info!("[memory-nodes] no upkeep: her mind is on another device");
                return Ok(0);
            };
            // A node with something on it and nothing of hers standing
            // there is built first (`super::unbuilt`): in the site owner's
            // talk however many wait, in another's within the day's calls.
            let built = match super::standing::build_waiting(
                db,
                user_id,
                &audience,
                (!owners).then_some(at_most),
                &pen,
            )
            .await
            {
                Ok(built) => built,
                Err(error) => {
                    tracing::warn!(%error, "[memory-nodes] how things stand not built at upkeep");
                    0
                }
            };
            let left = if owners {
                at_most
            } else {
                // Counted before upkeep goes on, so a failure there does
                // not leave them out of the day's calls.
                if built > 0 {
                    count_calls(db, user_id, &audience, built as u32).await?;
                }
                at_most.saturating_sub(built)
            };
            let calls = run(db, user_id, &audience, left, &pen).await?;
            if !owners && calls > 0 {
                count_calls(db, user_id, &audience, calls as u32).await?;
            }
            Ok::<usize, Error>(built + calls)
        }
        .await;
        match result {
            Ok(calls) if calls > 0 => tracing::info!(calls, owners, "[memory-nodes] upkeep ran"),
            Ok(_) => {}
            Err(error) => tracing::warn!(%error, "[memory-nodes] upkeep stopped"),
        }
    }
}

/// How long what she made of a session waits in her inbox, at most: as
/// long as the session's picture is kept (`sessions::PICTURE_SECS`).
const SESSION_DAYS: u64 = 14;

/// The language they chose on the site (`users.locale`), as a language
/// tag; none when they chose none.
async fn language_of<C: ConnectionTrait>(db: &C, user_id: i32) -> Option<String> {
    db.query_one_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT locale FROM users WHERE id = $1",
        [user_id.into()],
    ))
    .await
    .ok()
    .flatten()
    .and_then(|row| row.try_get::<Option<String>>("", "locale").ok().flatten())
    .map(|locale| locale.trim().replace('_', "-"))
    .filter(|locale| {
        !locale.is_empty()
            && locale.len() <= 35
            && locale
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    })
}

fn run_token() -> String {
    format!("u{}", &uuid::Uuid::new_v4().simple().to_string()[..24])
}

/// Up to `at_most` calls over `user_id`'s `audience`; how many were made.
pub(crate) async fn run(
    db: &DatabaseConnection,
    user_id: i32,
    audience: &str,
    at_most: usize,
    pen: &Thought,
) -> Result<usize, Error> {
    let mut calls = 0;
    // What is set aside (the nodes the rules made of sessions) or a rule's
    // bare name is not gone over.
    let mut gone: HashSet<String> = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT node_id FROM agent_node_heads \
             WHERE user_id = $1 AND audience = $2 AND status IN ('set_aside', 'dormant')",
            [user_id.into(), audience.into()],
        ))
        .await?
        .iter()
        .map(|row| row.try_get::<String>("", "node_id"))
        .collect::<Result<_, _>>()?;
    let today = crate::services::agent::merope::clock::today();
    let language = language_of(db, user_id).await;
    // What she made of a session and never put on a node, once its
    // picture is gone.
    if let Some(before) = today.checked_sub_days(chrono::Days::new(SESSION_DAYS)) {
        inbox::let_go_of_sessions(db, user_id, audience, before).await?;
    }
    // Nothing resting on a memory they took back is gone over again.
    super::forget::taken_back_here(db, user_id, audience).await?;
    while calls < at_most {
        // What rests on a memory they took back is not gone over: hidden
        // until the second look forgets it (`super::hidden`).
        let kept = super::hidden::rows_of(db, user_id, audience).await?;
        let bodies = heads::bodies(&kept)?;
        let rows: Vec<myriad_persona_log::Row> = kept.into_iter().map(|kept| kept.row).collect();
        let waiting =
            super::hidden::inbox_waiting(db, user_id, audience, UPKEEP_INBOX as u64).await?;
        let due = if rows.is_empty() {
            Vec::new()
        } else {
            queue::queue(QueueInput {
                rows: rows.clone(),
                bodies: bodies.clone(),
                ledger: Vec::new(),
                max: QUEUE_MAX,
            })?
            .due
        };
        let mut target = None;
        let mut cut_before = None;
        let due_nodes: Vec<String> = due
            .iter()
            .filter(|due| due.audience == audience && !gone.contains(&due.node))
            .map(|due| due.node.clone())
            .collect();
        for node in due_nodes {
            let seen = newest_on(&rows, &node);
            let (kept, waits) = kept_cut_off(db, user_id, audience, Some(&node), &seen).await;
            if waits {
                gone.insert(node);
                continue;
            }
            target = Some(node);
            cut_before = Some((kept, seen));
            break;
        }
        // The inbox alone is filed once a run, before anything else is.
        if target.is_none() && (waiting.is_empty() || calls > 0) {
            break;
        }
        if target.is_none() {
            let seen = format!(
                "{}:{}",
                waiting.len(),
                waiting.last().map(|item| item.id.as_str()).unwrap_or("")
            );
            let (kept, waits) = kept_cut_off(db, user_id, audience, None, &seen).await;
            if waits {
                break;
            }
            cut_before = Some((kept, seen));
        }
        let (kept_cut, seen) = cut_before.unwrap_or_default();
        let known = known::known(db, user_id, audience).await?;
        let input = UpkeepInput {
            rows,
            bodies,
            ledger: Vec::new(),
            audience: audience.to_string(),
            node: target.clone(),
            candidates: candidates(
                &known,
                target.as_deref(),
                &waiting,
                &holding_sessions(db, user_id, audience, &waiting).await?,
            ),
            inbox: waiting
                .iter()
                .map(|item| InboxItem {
                    id: item.id.clone(),
                    said_by: item.said_by.clone(),
                    text: item.text.clone(),
                    audience: None,
                })
                .collect(),
            today: Some(today.format("%Y-%m-%d").to_string()),
            language: language.clone(),
        };
        let shown = serde_json::to_string(&upkeep::prepare(input.clone())?)
            .map_err(|_| Error::Refused("what she is shown does not serialize".into()))?;
        calls += 1;
        let answer = Ask::new(Voice::Keeping, user_id, OPERATION)
            .within(TIMEOUT)
            // At most 5,683 measured, thinking included, but a live upkeep
            // was cut off at 10,000 with under a thousand characters said:
            // what deep thinking had before limits were named.
            .output_at_most(16_000)
            .json_raw(&system_prompt(), &shown, SCHEMA_NAME, &schema())
            .await;
        let raw = match answer {
            Ok(raw) => raw,
            Err(failure) => {
                if failure == Failure::CutOff {
                    keep_cut_off(db, user_id, audience, target.as_deref(), kept_cut, seen).await;
                }
                match target {
                    Some(target) => {
                        gone.insert(target);
                        continue;
                    }
                    None => break,
                }
            }
        };
        // Answered whole: what was cut off before is let go.
        if kept_cut.is_some()
            && let Err(error) = registry::delete(
                db,
                CUT_OFF_NAMESPACE,
                &cut_off_key(user_id, audience, target.as_deref()),
            )
            .await
        {
            tracing::warn!(%error, "[memory-nodes] upkeep answered, what was cut off not let go");
        }
        let read = upkeep::read(ReadInput {
            input,
            run: run_token(),
            sync: myriad_nodes::Sync::Shared,
            output: plain(&raw),
        })?;
        if read.fallback.is_some() {
            tracing::info!(
                fallback = ?read.fallback,
                action = ?read.failed,
                "[memory-nodes] an upkeep answer could not be read"
            );
        }
        // Once a night: what it wrote may wake another, not itself again,
        // and an answer that could not be read is asked again tomorrow.
        if let Some(target) = &target {
            gone.insert(target.clone());
        }
        let rows: Vec<RowIn> = read
            .writes
            .iter()
            .map(|write| RowIn {
                id: write.id.clone(),
                node: write.node.clone(),
                kind: write.kind.clone(),
                body: write.body.clone(),
                author: Author::Her,
            })
            .collect();
        let beside = Beside {
            taken_out: read.placed.clone(),
            ..Beside::default()
        };
        let written = write_with(db, user_id, audience, &rows, Some(pen), &beside).await;
        if written.is_ok() {
            // What still stings that she closed there: let go of.
            super::hanging::closed(db, user_id, audience, &rows).await;
        }
        if let Err(error) = written {
            tracing::warn!(%error, "[memory-nodes] her upkeep could not be written");
            // The mark alone, saying so; without one the node stays as it was.
            let mark = rows.last().filter(|row| row.kind == "node.upkept").cloned();
            let written = match mark {
                Some(mut mark) => {
                    mark.body["fallback"] = json!(REFUSED);
                    write_with(
                        db,
                        user_id,
                        audience,
                        &[mark],
                        Some(pen),
                        &Beside::default(),
                    )
                    .await
                    .is_ok()
                }
                None => false,
            };
            if !written {
                match &target {
                    Some(target) => {
                        gone.insert(target.clone());
                    }
                    None => break,
                }
            }
        }
        if target.is_none() {
            break;
        }
    }
    Ok(calls)
}

#[cfg(test)]
mod tests {
    use super::CutOff;

    #[test]
    fn a_node_cut_off_twice_with_nothing_new_waits_a_day() {
        let now = 1_000_000;
        let once = CutOff::again(None, "r1".into(), now);
        assert_eq!(once.times, 1);
        assert_eq!(
            once.waits("r1", now + 1),
            None,
            "asked again at the next upkeep"
        );
        let twice = CutOff::again(Some(once), "r1".into(), now + 3_600);
        assert_eq!(twice.times, 2);
        assert_eq!(
            twice.waits("r1", now + 3_601),
            Some((2, now + 3_600 + super::CUT_OFF_WAIT))
        );
        assert_eq!(twice.waits("r2", now + 3_601), None, "something new on it");
        assert_eq!(
            twice.waits("r1", now + 3_600 + super::CUT_OFF_WAIT),
            None,
            "a day on, asked again"
        );
        // Cut off again on something new: counted from one.
        assert_eq!(CutOff::again(Some(twice), "r2".into(), now).times, 1);
    }
}
