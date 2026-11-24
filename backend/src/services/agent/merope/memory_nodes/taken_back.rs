//! What rests on a memory they took back, forgotten on her nodes
//! ([`super`]): at once as it is taken back here, and by a sweep before her
//! nodes are read or gone over for what was taken back elsewhere, or left
//! when forgetting there failed; versions written from what went while
//! they were made are left out; and how all of it went, counted.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionSession, TransactionTrait};

use super::lines::{lines_cited, lines_of};
use super::{Error, Gone};

/// How forgetting what rests on a memory taken back went since the site
/// started, for her status: memories taken back whose nodes could not be
/// forgotten at once (left to the sweep, [`taken_back_here`]), sweeps that
/// forgot something, and sweeps that stopped on an error (what was to be
/// read or gone over there waits, and the sweep is tried again at the next
/// stretch or the next hourly upkeep). Counts only.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub(crate) struct Failures {
    pub(crate) left_to_sweep: u64,
    pub(crate) swept: u64,
    pub(crate) sweep_errors: u64,
    /// Versions and intents left out as they were written, for resting on
    /// what went while they were being made ([`resting_on_what_went`]).
    pub(crate) left_out: u64,
    /// Of her talks, the most sweeps in a row that stopped on an error in
    /// one: while it is not 0, that talk's nodes are not read and its
    /// intents not acted on.
    pub(crate) sweep_errors_in_a_row: u64,
}

/// The memories taken back whose nodes could not be forgotten at once, until
/// a sweep of their person's talk goes through ([`taken_back_here`]).
static LEFT_TO_SWEEP: std::sync::LazyLock<std::sync::Mutex<BTreeSet<String>>> =
    std::sync::LazyLock::new(Default::default);
static SWEPT: AtomicU64 = AtomicU64::new(0);
static SWEEP_ERRORS: AtomicU64 = AtomicU64::new(0);
/// Sweeps in a row that stopped on an error, by account and audience: a
/// talk whose sweep went through, or that needed none, is not here.
static SWEEP_ERRORS_IN_A_ROW: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<(i32, String), u64>>,
> = std::sync::LazyLock::new(Default::default);
static LEFT_OUT: AtomicU64 = AtomicU64::new(0);

/// Whether the memory `id` waits for the sweep ([`LEFT_TO_SWEEP`]).
#[cfg(test)]
pub(crate) fn left_to_sweep_holds(id: &str) -> bool {
    LEFT_TO_SWEEP.lock().is_ok_and(|left| left.contains(id))
}

/// [`Failures`] so far.
pub(crate) fn failures() -> Failures {
    Failures {
        left_to_sweep: LEFT_TO_SWEEP.lock().map_or(0, |left| left.len() as u64),
        swept: SWEPT.load(Relaxed),
        sweep_errors: SWEEP_ERRORS.load(Relaxed),
        left_out: LEFT_OUT.load(Relaxed),
        sweep_errors_in_a_row: SWEEP_ERRORS_IN_A_ROW
            .lock()
            .map_or(0, |in_a_row| in_a_row.values().copied().max().unwrap_or(0)),
    }
}

/// A sweep of `user_id`'s `audience` that stopped on an error, counted
/// ([`Failures`]).
fn sweep_failed(user_id: i32, audience: &str) {
    SWEEP_ERRORS.fetch_add(1, Relaxed);
    if let Ok(mut in_a_row) = SWEEP_ERRORS_IN_A_ROW.lock() {
        *in_a_row.entry((user_id, audience.to_string())).or_default() += 1;
    }
}

/// `user_id`'s `audience` swept, or needing none: its errors in a row end.
fn sweep_held(user_id: i32, audience: &str) {
    if let Ok(mut in_a_row) = SWEEP_ERRORS_IN_A_ROW.lock() {
        in_a_row.remove(&(user_id, audience.to_string()));
    }
}

/// Whether a version is part of a life (what she made of something, what
/// is not wrapped up, a summary sentence, an item of how someone's life
/// stands): what a memory taken back can stand under. A rule's item (where
/// a session is now) or her line on a repository is not; nor what a device
/// wrote under a grant.
pub(super) fn of_a_life(kind: &str, body: Option<&str>) -> bool {
    match kind {
        "node.view_changed" | "node.open_item" | "node.revised" => true,
        "node.standing" => body
            .and_then(|body| serde_json::from_str::<serde_json::Value>(body).ok())
            // The SQL test in [`RESTS_ON_A_STUB`] is the same but for not
            // reading a person's name as UTF-8: no item is minted so.
            .and_then(|body| {
                body["item"]
                    .as_str()
                    .map(|item| myriad_nodes::stands::whose_of(item).is_some())
            })
            .unwrap_or(false),
        _ => false,
    }
}

/// Of the rows `fresh` (id, kind, body) about to be written in `user_id`'s
/// `audience`, the versions and the waiting intents resting on what went
/// while they were being made (a model's answer in flight as a memory was
/// forgotten or taken back): one naming a memory known to be gone
/// ([`super::super::known::Named::sure`]), a line of theirs forgotten, or an
/// entry or a span forgotten;
/// a version of a life ([`of_a_life`]) or an intent written on a row
/// forgotten; and any of them written on another such. An intent settled
/// (said, done, dropped) is written whatever it rests on, unless it is
/// written on a row of it forgotten: it says nothing new, but would bring
/// the words back. Each is left out alone, and counted
/// ([`Failures::left_out`]).
pub(in super::super) async fn resting_on_what_went<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
    fresh: &[(&str, &str, &str)],
) -> Result<BTreeSet<String>, Error> {
    let strings = |value: &serde_json::Value| -> Vec<String> {
        value
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|item| item.as_str().map(str::to_string))
            .collect()
    };
    // Each version (and intent) with what it rests on and, for a version
    // of a life, what it was written on: one written on a version gone
    // before goes with it only there ([`of_a_life`]).
    let versions: Vec<(String, Vec<String>, Vec<String>)> = fresh
        .iter()
        .filter(|(_, kind, _)| super::VERSIONED.contains(kind) || *kind == "node.intent")
        .filter_map(|(id, kind, body)| {
            let life = of_a_life(kind, Some(body));
            let body: serde_json::Value = serde_json::from_str(body).ok()?;
            let intent = *kind == "node.intent";
            // An intent settled says nothing new: only one written on a row
            // of it already forgotten goes (the words would come back).
            let settled = intent && body["status"] != "waiting";
            let mut because = if settled {
                Vec::new()
            } else {
                strings(&body["because"])
            };
            for sentence in body["value"].as_array().into_iter().flatten() {
                because.extend(strings(&sentence["because"]));
            }
            let base = if life || intent {
                strings(&body["base"])
            } else {
                Vec::new()
            };
            Some((id.to_string(), because, base))
        })
        .collect();
    if versions.is_empty() {
        return Ok(BTreeSet::new());
    }
    let memories: Vec<String> = versions
        .iter()
        .flat_map(|(_, because, _)| because.iter())
        .filter_map(|reference| reference.strip_prefix(myriad_nodes::body::MEMORY_REF))
        .map(str::to_string)
        .collect();
    let mut gone: BTreeSet<String> = super::super::known::named(db, memories)
        .await?
        .into_iter()
        .filter(|(_, named)| named.taken_back && named.sure)
        .map(|(id, _)| format!("{}{id}", myriad_nodes::body::MEMORY_REF))
        .collect();
    let because: Vec<String> = versions
        .iter()
        .flat_map(|(_, because, _)| because.clone())
        .collect();
    gone.extend(super::intent_grounds::lines_gone(db, user_id, &because).await?);
    let rows: Vec<String> = versions
        .iter()
        .flat_map(|(_, because, base)| {
            because
                .iter()
                .filter_map(|reference| {
                    reference
                        .strip_prefix("entry:")
                        .or_else(|| reference.strip_prefix("span:"))
                })
                .map(str::to_string)
                .chain(base.iter().cloned())
                .collect::<Vec<_>>()
        })
        .collect();
    let stubs: BTreeSet<String> = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT row_id FROM agent_node_rows \
             WHERE user_id = $1 AND audience = $2 AND body = '' AND row_id = ANY($3)",
            [user_id.into(), audience.into(), rows.into()],
        ))
        .await?
        .iter()
        .map(|row| row.try_get::<String>("", "row_id"))
        .collect::<Result<_, _>>()?;
    let on_a_stub = |reference: &String| {
        reference
            .strip_prefix("entry:")
            .or_else(|| reference.strip_prefix("span:"))
            .is_some_and(|row| stubs.contains(row))
    };
    let mut went: BTreeSet<String> = versions
        .iter()
        .filter(|(_, because, base)| {
            because
                .iter()
                .any(|reference| gone.contains(reference) || on_a_stub(reference))
                || base.iter().any(|row| stubs.contains(row))
        })
        .map(|(id, _, _)| id.clone())
        .collect();
    loop {
        let more: Vec<String> = versions
            .iter()
            .filter(|(id, _, base)| !went.contains(id) && base.iter().any(|row| went.contains(row)))
            .map(|(id, _, _)| id.clone())
            .collect();
        if more.is_empty() {
            break;
        }
        went.extend(more);
    }
    if !went.is_empty() {
        LEFT_OUT.fetch_add(went.len() as u64, Relaxed);
        tracing::warn!(
            rows = went.len(),
            "[memory-nodes] versions resting on what went while they were made are left out"
        );
    }
    Ok(went)
}

/// Which error it was, for a log line that carries no words.
pub(crate) fn error_kind(error: &Error) -> &'static str {
    match error {
        Error::Refused(_) => "refused",
        Error::NoPen => "no_pen",
        Error::Db(_) => "database",
    }
}

/// How far back a line of memories, each replaced by the next, is followed:
/// as far as whether one was taken back is read
/// (`known::taken_back_sql`).
const LINE_STEPS: i32 = 64;

/// The memories `memories` they took back with every one each replaced, as
/// far back as it goes: taking back the last word on something takes back
/// what it corrected too (「当我没说」 of 「改到周六」 is not 「还是周五」),
/// as forgetting takes a memory's whole line (`memory::manage`).
async fn with_what_they_replaced<C: ConnectionTrait>(
    db: &C,
    memories: &[String],
) -> Result<Vec<String>, Error> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "WITH RECURSIVE line(id, user_id, depth) AS ( \
               SELECT id, user_id, 0 FROM agent_memories WHERE id = ANY($1) \
               UNION \
               SELECT m.id, m.user_id, line.depth + 1 FROM agent_memories m \
               JOIN line ON m.superseded_by = line.id \
                 AND m.user_id IS NOT DISTINCT FROM line.user_id \
               WHERE line.depth < $2) \
             SELECT DISTINCT id FROM line",
            [memories.to_vec().into(), LINE_STEPS.into()],
        ))
        .await?;
    rows.iter()
        .map(|row| Ok(row.try_get::<String>("", "id")?))
        .collect()
}

/// What rests on the memories `memories` they took back, and on each they
/// replaced ([`with_what_they_replaced`]), hidden on her nodes at once
/// (their heads written again, [`super::super::hidden`]) in a savepoint of
/// `db`'s transaction, before the second look forgets it or not. If this
/// fails, it is undone alone, and the sweep before her nodes are next read
/// hides it ([`taken_back_here`]: the memory was let go since).
pub(crate) async fn hide_taken_back<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    memories: &[String],
) {
    if memories.is_empty() {
        return;
    }
    let result = async {
        let savepoint = db.begin().await?;
        let hidden = async {
            let line = with_what_they_replaced(&savepoint, memories).await?;
            let lines = lines_of(&savepoint, &line).await?;
            for (user_id, audience) in
                super::super::hidden::citing(&savepoint, &line, &lines).await?
            {
                super::super::hidden::rewrite(&savepoint, user_id, &audience).await?;
            }
            Ok::<(), Error>(())
        }
        .await;
        match hidden {
            Ok(()) => savepoint.commit().await.map_err(Error::from),
            Err(error) => {
                savepoint.rollback().await?;
                Err(error)
            }
        }
    }
    .await;
    if let Err(error) = result {
        tracing::warn!(
            memories = memories.len(),
            kind = error_kind(&error),
            "[memory-nodes] what rests on memories taken back is left to the sweep to hide"
        );
    }
}

/// What rests on the memories `memories` they took back, and on each they
/// replaced ([`with_what_they_replaced`]), forgotten on her nodes in a
/// savepoint of `db`'s transaction; what rests only on the lines they were
/// said in stays hidden ([`super::lines`]). The memories are let go whatever
/// happens here: if this fails, it is undone alone, counted, and the sweep
/// before her nodes are next read or gone over forgets it
/// ([`taken_back_here`]).
pub(crate) async fn forget_taken_back<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    memories: &[String],
) {
    if memories.is_empty() {
        return;
    }
    let result = async {
        let savepoint = db.begin().await?;
        let forgotten = async {
            let line = with_what_they_replaced(&savepoint, memories).await?;
            let lines = lines_of(&savepoint, &line).await?;
            super::forget(&savepoint, &line, &[]).await?;
            // What was made of the lines they said it in is only hidden.
            for (user_id, audience) in super::super::hidden::citing(&savepoint, &[], &lines).await?
            {
                super::super::hidden::rewrite(&savepoint, user_id, &audience).await?;
            }
            Ok::<(), Error>(())
        }
        .await;
        match forgotten {
            Ok(()) => savepoint.commit().await.map_err(Error::from),
            Err(error) => {
                savepoint.rollback().await?;
                Err(error)
            }
        }
    }
    .await;
    if let Err(error) = result {
        // Each it replaced too: what rests on any of them waits. Read again
        // past the savepoint undone; failing that, the ones taken back.
        let line = with_what_they_replaced(db, memories)
            .await
            .unwrap_or_else(|_| memories.to_vec());
        if let Ok(mut left) = LEFT_TO_SWEEP.lock() {
            left.extend(line);
        }
        tracing::warn!(
            memories = memories.len(),
            kind = error_kind(&error),
            "[memory-nodes] what rests on memories taken back is left to the sweep"
        );
    }
}

/// What rests in `user_id`'s `audience` on a memory they took back
/// (「当我没说」, confirmed by the second look, `take_back_check`), or
/// deleted, forgotten as if it were forgotten now ([`forget`]): confirming
/// forgets it there at once, but one whose forgetting failed, or that came
/// before forgetting reached 「现在」, left it. One taken back on another of
/// her ends, or here before the second look, is only hidden: her nodes
/// here are written again as they read with it hidden ([`super::super::hidden`]).
/// Run before what is here is read for her (a stretch's facts,
/// her upkeep), in a transaction of its own.
///
/// An error stops what was to follow (nothing resting on what they took
/// back is read or gone over), is counted, and the sweep is tried again the
/// next time.
pub(crate) async fn taken_back_here(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    audience: &str,
) -> Result<(), Error> {
    let began = crate::clock::now();
    // Taken back in a transaction still open as this began is written at
    // a time before it, and only seen once it commits: the next read
    // sweeps again for what came within this much before.
    let started = began - COMMIT_SLACK;
    // What was left to the sweep before this one began: only that can be
    // found done by it.
    let left: Vec<String> = LEFT_TO_SWEEP
        .lock()
        .map(|left| left.iter().cloned().collect())
        .unwrap_or_default();
    let mut swept = sweep(db, user_id, audience).await;
    if swept.is_ok() && !left.is_empty() {
        swept = swept_for(db, &left).await;
    }
    if swept.is_ok() {
        sweep_held(user_id, audience);
        // One that began before what another end brought was landed may
        // not have seen it ([`sweep_again`]): the next read sweeps again.
        let landed = LANDED
            .lock()
            .ok()
            .and_then(|landed| landed.get(&user_id).copied());
        if landed.is_none_or(|landed| began > landed)
            && let Ok(mut at) = SWEPT_AT.lock()
        {
            at.insert((user_id, audience.to_string()), started);
        }
    }
    if let Err(error) = &swept {
        sweep_failed(user_id, audience);
        tracing::warn!(
            kind = error_kind(error),
            "[memory-nodes] forgetting what rests on memories taken back stopped"
        );
    }
    swept
}

/// Of `left` ([`LEFT_TO_SWEEP`] as a sweep began), what is done: one not
/// taken back after all (what took it back was undone), or one nothing of
/// hers cites any more, in any audience (her nodes, her inbox, her
/// intents). One is judged only where it is known (its row is here, or the
/// mark forgetting it kept), and her rows are read only for those.
async fn swept_for(db: &sea_orm::DatabaseConnection, left: &[String]) -> Result<(), Error> {
    let known: Vec<String> = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id FROM unnest($1::text[]) AS left_(id) \
             WHERE EXISTS (SELECT 1 FROM agent_memories m WHERE m.id = left_.id) \
                OR EXISTS (SELECT 1 FROM agent_forgotten f \
                           WHERE f.what = $2 AND f.mark = left_.id)",
            [
                left.to_vec().into(),
                crate::services::agent::memory::forgotten::MEMORY.into(),
            ],
        ))
        .await?
        .iter()
        .map(|row| row.try_get::<String>("", "id"))
        .collect::<Result<_, _>>()?;
    if known.is_empty() {
        return Ok(());
    }
    let named = super::super::known::named(db, known.clone()).await?;
    let mut done: BTreeSet<String> = named
        .iter()
        .filter(|(_, named)| !named.taken_back)
        .map(|(id, _)| id.clone())
        .collect();
    let refs: Vec<String> = known
        .iter()
        .filter(|id| !done.contains(*id))
        .map(|id| format!("{}{id}", myriad_nodes::body::MEMORY_REF))
        .collect();
    if !refs.is_empty() {
        let cited: BTreeSet<String> = db
            .query_all_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT cited.ref FROM agent_node_rows r \
                 CROSS JOIN LATERAL jsonb_array_elements_text( \
                   COALESCE((r.body::jsonb)->(CASE \
                     WHEN r.kind IN ('node.view_changed', 'node.open_item', 'node.standing', \
                                     'node.intent') THEN 'because' \
                     ELSE 'refs' END), '[]'::jsonb)) AS cited(ref) \
                 WHERE r.body <> '' \
                   AND r.kind IN ('node.entry', 'node.span', 'node.view_changed', \
                                  'node.open_item', 'node.standing', 'node.intent') \
                   AND cited.ref = ANY($1) \
                 UNION \
                 SELECT cited.ref FROM agent_node_inbox i \
                 CROSS JOIN LATERAL jsonb_array_elements_text( \
                   COALESCE(i.item->'refs', '[]'::jsonb)) AS cited(ref) \
                 WHERE cited.ref = ANY($1)",
                [refs.clone().into()],
            ))
            .await?
            .iter()
            .map(|row| row.try_get::<String>("", "ref"))
            .collect::<Result<_, _>>()?;
        done.extend(
            refs.iter()
                .filter(|reference| !cited.contains(*reference))
                .filter_map(|reference| reference.strip_prefix(myriad_nodes::body::MEMORY_REF))
                .map(str::to_string),
        );
    }
    if let Ok(mut kept) = LEFT_TO_SWEEP.lock() {
        kept.retain(|id| !done.contains(id));
    }
    Ok(())
}

/// When each of her audiences was last swept ([`taken_back_here`]) since
/// the site started, by account and audience.
static SWEPT_AT: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<(i32, String), chrono::DateTime<chrono::Utc>>>,
> = std::sync::LazyLock::new(Default::default);

/// How long a transaction taking something back may stay open before it
/// commits: a sweep is taken to have begun this much earlier than it did
/// ([`taken_back_here`]), as what it took back is written with the time
/// it was written, not the time it was committed.
const COMMIT_SLACK: chrono::TimeDelta = chrono::TimeDelta::minutes(5);

/// When what another of her ends brought was last landed, by account
/// ([`sweep_again`]).
static LANDED: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<i32, chrono::DateTime<chrono::Utc>>>,
> = std::sync::LazyLock::new(Default::default);

/// Each of `user_id`'s talks is swept again before it is next read, once
/// what another of her ends brought is committed here (`persona_sync::merge`,
/// `her_log::state`): it was written with that end's times, which may be
/// before the last sweep here. A sweep that began before this does not
/// count as that one ([`taken_back_here`]).
pub(crate) fn sweep_again(user_id: i32) {
    if let Ok(mut landed) = LANDED.lock() {
        landed.insert(user_id, crate::clock::now());
    }
    if let Ok(mut at) = SWEPT_AT.lock() {
        at.retain(|(user, _), _| *user != user_id);
    }
}

/// [`taken_back_here`] before `user_id`'s `audience` is read for her turn
/// or her intents there are read or acted on, when anything of theirs was
/// taken back (or a taking back confirmed), put aside or forgotten since it
/// was last swept (here, on
/// another end, or as forgetting on her nodes failed), one of theirs waits
/// in [`LEFT_TO_SWEEP`], or it was not swept since the site started or
/// since another end's records landed: so a summary sentence, a view, what
/// is not wrapped up or an intent resting on what they took back is not
/// shown or acted on. One small read when nothing changed; for
/// [`COMMIT_SLACK`] after something was taken back, each read sweeps.
pub(crate) async fn before_reading(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    audience: &str,
) -> Result<(), Error> {
    let key = (user_id, audience.to_string());
    let swept = SWEPT_AT.lock().ok().and_then(|at| at.get(&key).copied());
    if let Some(swept) = swept {
        // Left to the sweep when forgetting failed here ([`LEFT_TO_SWEEP`]):
        // one of theirs, or one forgotten for good whose person is not
        // known any more, is swept for whatever the times say.
        let left: Vec<String> = LEFT_TO_SWEEP
            .lock()
            .map(|left| left.iter().cloned().collect())
            .unwrap_or_default();
        let changed: Result<(Option<chrono::DateTime<chrono::FixedOffset>>, bool), Error> = async {
            let row = db
                .query_one_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "SELECT GREATEST( \
                   (SELECT max(updated_at) FROM agent_memories \
                    WHERE user_id = $1 AND (invalid_reason = 'deleted' \
                          OR (invalid_reason = 'superseded' AND superseded_by IS NULL))), \
                   (SELECT max(forgotten_at) FROM agent_forgotten \
                    WHERE user_id = $1 AND what = $2), \
                   (SELECT max(at) FROM agent_takeback_check \
                    WHERE user_id = $1 AND state = 'confirmed')) AS at, \
                   EXISTS (SELECT 1 FROM unnest($3::text[]) AS left_(id) \
                     WHERE EXISTS (SELECT 1 FROM agent_memories m \
                                   WHERE m.id = left_.id AND m.user_id = $1) \
                        OR (NOT EXISTS (SELECT 1 FROM agent_memories m WHERE m.id = left_.id) \
                            AND EXISTS (SELECT 1 FROM agent_forgotten f \
                                        WHERE f.what = $2 AND f.mark = left_.id))) AS left_here",
                    [
                        user_id.into(),
                        crate::services::agent::memory::forgotten::MEMORY.into(),
                        left.into(),
                    ],
                ))
                .await?;
            Ok(match row {
                Some(row) => (row.try_get("", "at")?, row.try_get("", "left_here")?),
                None => (None, false),
            })
        }
        .await;
        // Counted as a sweep that stopped: her nodes are not read this turn.
        let changed = changed.inspect_err(|error| {
            sweep_failed(user_id, audience);
            tracing::warn!(
                kind = error_kind(error),
                "[memory-nodes] whether anything was taken back could not be read"
            );
        })?;
        let (changed, left_here) = changed;
        if !left_here && changed.is_none_or(|at| at.with_timezone(&chrono::Utc) < swept) {
            sweep_held(user_id, audience);
            return Ok(());
        }
    }
    taken_back_here(db, user_id, audience).await
}

/// [`taken_back_here`], once.
async fn sweep(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    audience: &str,
) -> Result<(), Error> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            // On her nodes (an intent too), in her inbox, and in what was
            // seen to still hold: a memory only one of those names is found
            // as well.
            "SELECT DISTINCT substr(cited.ref, 8) AS id FROM ( \
               SELECT c.ref FROM agent_node_rows r \
               CROSS JOIN LATERAL jsonb_array_elements_text( \
                 COALESCE((r.body::jsonb)->(CASE \
                   WHEN r.kind IN ('node.view_changed', 'node.open_item', 'node.standing', \
                                   'node.intent') THEN 'because' \
                   ELSE 'refs' END), '[]'::jsonb)) AS c(ref) \
               WHERE r.user_id = $1 AND r.audience = $2 AND r.body <> '' \
                 AND r.kind IN ('node.entry', 'node.view_changed', 'node.open_item', \
                                'node.standing', 'node.intent') \
               UNION ALL \
               SELECT c.ref FROM agent_node_inbox i \
               CROSS JOIN LATERAL jsonb_array_elements_text( \
                 COALESCE(i.item->'refs', '[]'::jsonb)) AS c(ref) \
               WHERE i.user_id = $1 AND i.audience = $2 \
               UNION ALL \
               SELECT 'memory:' || k.by_memory FROM agent_node_kept k \
               WHERE k.user_id = $1 AND k.audience = $2) cited \
             WHERE cited.ref LIKE 'memory:%'",
            [user_id.into(), audience.into()],
        ))
        .await?;
    let ids: Vec<String> = rows
        .iter()
        .map(|row| row.try_get::<String>("", "id"))
        .collect::<Result<_, _>>()?;
    let named = super::super::known::named(db, ids).await?;
    let taken_back: Vec<String> = named
        .iter()
        // Not here and with no mark of being forgotten, it may only not
        // have come yet; only judged taken back, the second look has not
        // agreed (`take_back_check`), or it was taken back before there
        // was one: hidden where she reads, but nothing is forgotten.
        .filter(|(_, named)| named.taken_back && named.forgets)
        .map(|(id, _)| id.clone())
        .collect();
    // Hidden only: her nodes here are written again as they read with it
    // hidden ([`super::super::hidden`]), nothing forgotten.
    // Written hidden before: written again, so what came back shows.
    let hidden = named
        .values()
        .any(|named| named.taken_back && !named.forgets)
        || super::super::hidden::written_hidden(user_id, audience);
    let hidden = hidden || lines_cited(db, user_id, audience).await?;
    let left = left_resting(db, user_id, audience).await?;
    if taken_back.is_empty() && left == 0 && !hidden {
        return Ok(());
    }
    let txn = db.begin().await?;
    if !taken_back.is_empty() {
        super::forget(&txn, &taken_back, &[]).await?;
    }
    if left > 0 {
        let none = BTreeSet::new();
        super::forget_rows(
            &txn,
            user_id,
            audience,
            &none,
            none.clone(),
            Gone::default(),
        )
        .await?;
    }
    if hidden {
        super::super::hidden::rewrite(&txn, user_id, audience).await?;
    }
    txn.commit().await?;
    SWEPT.fetch_add(1, Relaxed);
    // What was left to the sweep is done now, as far as these go.
    tracing::info!(
        memories = taken_back.len(),
        left,
        hidden,
        "[memory-nodes] what rested on memories taken back, or on entries forgotten before, forgotten (or only hidden)"
    );
    Ok(())
}

/// What a version rests on (`because`, or each summary sentence's) that is
/// an entry or a span already forgotten (a stub), as SQL over a row `d`;
/// and, for a version of a life ([`of_a_life`]: the same test in SQL), the
/// version it was written on (`base`) that is: what [`super::forget_rows`]
/// forgets, so a sweep finds none of it left after.
const RESTS_ON_A_STUB: &str = "agent_node_rows d \
     CROSS JOIN LATERAL ( \
       SELECT jsonb_array_elements_text(COALESCE(d.body::jsonb->'because', '[]'::jsonb)), false \
       UNION ALL \
       SELECT jsonb_array_elements_text(COALESCE(s->'because', '[]'::jsonb)), false \
       FROM jsonb_array_elements(CASE WHEN jsonb_typeof(d.body::jsonb->'value') = 'array' \
                                      THEN d.body::jsonb->'value' ELSE '[]'::jsonb END) s \
       UNION ALL \
       SELECT jsonb_array_elements_text(COALESCE(d.body::jsonb->'base', '[]'::jsonb)), true \
     ) AS refs(ref, base) \
     JOIN agent_node_rows e ON e.user_id = d.user_id AND e.audience = d.audience AND e.body = '' \
       AND ((e.kind IN ('node.entry', 'node.span') \
             AND refs.ref = (CASE e.kind WHEN 'node.entry' THEN 'entry:' ELSE 'span:' END) || e.row_id) \
         OR (e.kind IN ('node.view_changed', 'node.open_item', 'node.standing', 'node.revised') \
             AND refs.base AND refs.ref = e.row_id \
             AND (d.kind IN ('node.view_changed', 'node.open_item', 'node.revised') \
               OR (d.kind = 'node.standing' \
                 AND (d.body::jsonb->>'item') ~ '^(them|p([0-9a-fA-F]{2})+)-[0-9a-fA-F]{10}$')))) \
     WHERE d.body <> '' \
       AND d.kind IN ('node.view_changed', 'node.open_item', 'node.standing', 'node.revised')";

/// How many versions in `user_id`'s `audience` still rest on an entry or a
/// span forgotten before upkeep's evidence was followed ([`resting_on`]),
/// or, of a life, were written on a version forgotten ([`RESTS_ON_A_STUB`]).
async fn left_resting(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    audience: &str,
) -> Result<i64, Error> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            format!(
                "SELECT count(DISTINCT d.row_id) AS n FROM {RESTS_ON_A_STUB} \
                 AND d.user_id = $1 AND d.audience = $2"
            ),
            [user_id.into(), audience.into()],
        ))
        .await?;
    Ok(row
        .map(|row| row.try_get::<i64>("", "n"))
        .transpose()?
        .unwrap_or(0))
}
