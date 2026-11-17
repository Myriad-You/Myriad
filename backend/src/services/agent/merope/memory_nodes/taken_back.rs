//! What rests on a memory they took back, forgotten on her nodes
//! ([`super`]): at once as it is taken back here, and by a sweep before her
//! nodes are read or gone over for what was taken back elsewhere, or left
//! when forgetting there failed; versions written from what went while
//! they were made are left out; and how all of it went, counted.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionSession, TransactionTrait};

use super::Error;

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
    /// Versions left out as they were written, for resting on what went
    /// while they were being made ([`resting_on_what_went`]).
    pub(crate) left_out: u64,
}

/// The memories taken back whose nodes could not be forgotten at once, until
/// a sweep of their person's talk goes through ([`taken_back_here`]).
static LEFT_TO_SWEEP: std::sync::LazyLock<std::sync::Mutex<BTreeSet<String>>> =
    std::sync::LazyLock::new(Default::default);
static SWEPT: AtomicU64 = AtomicU64::new(0);
static SWEEP_ERRORS: AtomicU64 = AtomicU64::new(0);
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
    }
}

/// Of the rows `fresh` (id, kind, body) about to be written in `user_id`'s
/// `audience`, the versions resting on what went while they were being
/// made (a model's answer in flight as a memory was forgotten or taken
/// back): one naming a memory known to be gone ([`super::super::known::Named::sure`]),
/// an entry or a span forgotten, or written on a version forgotten, and
/// any of them written on another such. Each is left out alone, and
/// counted ([`Failures::left_out`]).
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
    // Each version with what it rests on and what it was written on.
    let versions: Vec<(String, Vec<String>, Vec<String>)> = fresh
        .iter()
        .filter(|(_, kind, _)| super::VERSIONED.contains(kind))
        .filter_map(|(id, _, body)| {
            let body: serde_json::Value = serde_json::from_str(body).ok()?;
            let mut because = strings(&body["because"]);
            for sentence in body["value"].as_array().into_iter().flatten() {
                because.extend(strings(&sentence["because"]));
            }
            Some((id.to_string(), because, strings(&body["base"])))
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
    let gone: BTreeSet<String> = super::super::known::named(db, memories)
        .await?
        .into_iter()
        .filter(|(_, named)| named.taken_back && named.sure)
        .map(|(id, _)| format!("{}{id}", myriad_nodes::body::MEMORY_REF))
        .collect();
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
fn kind(error: &Error) -> &'static str {
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
/// replaced ([`with_what_they_replaced`]), forgotten on her nodes in a
/// savepoint of `db`'s transaction. The memories are let go whatever
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
            super::forget(&savepoint, &line, &[]).await
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
            kind = kind(&error),
            "[memory-nodes] what rests on memories taken back is left to the sweep"
        );
    }
}

/// What rests in `user_id`'s `audience` on a memory they took back
/// (「当我没说」, or deleted), forgotten as if it were forgotten now
/// ([`forget`]): taking a memory back forgets it there at once, but one
/// taken back on another of her ends, or before forgetting reached 「现在」,
/// left it. Run before what is here is read for her (a stretch's facts,
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
    let started = crate::clock::now();
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
    if swept.is_ok()
        && let Ok(mut at) = SWEPT_AT.lock()
    {
        at.insert((user_id, audience.to_string()), started);
    }
    if let Err(error) = &swept {
        SWEEP_ERRORS.fetch_add(1, Relaxed);
        tracing::warn!(
            kind = kind(error),
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

/// [`taken_back_here`] before `user_id`'s `audience` is read for her turn,
/// when anything of theirs was taken back, put aside or forgotten since it
/// was last swept (here, on another end, or as forgetting on her nodes
/// failed), or it was not swept since the site started: so a summary
/// sentence, a view or what is not wrapped up resting on what they took
/// back is not shown until the next stretch or upkeep. One small read when
/// nothing changed.
pub(crate) async fn before_reading(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    audience: &str,
) -> Result<(), Error> {
    let key = (user_id, audience.to_string());
    let swept = SWEPT_AT.lock().ok().and_then(|at| at.get(&key).copied());
    if let Some(swept) = swept {
        let changed: Result<Option<chrono::DateTime<chrono::FixedOffset>>, Error> = async {
            Ok(db
                .query_one_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "SELECT GREATEST( \
                   (SELECT max(updated_at) FROM agent_memories \
                    WHERE user_id = $1 AND (invalid_reason = 'deleted' \
                          OR (invalid_reason = 'superseded' AND superseded_by IS NULL))), \
                   (SELECT max(forgotten_at) FROM agent_forgotten \
                    WHERE user_id = $1 AND what = $2)) AS at",
                    [
                        user_id.into(),
                        crate::services::agent::memory::forgotten::MEMORY.into(),
                    ],
                ))
                .await?
                .map(|row| row.try_get("", "at"))
                .transpose()?
                .flatten())
        }
        .await;
        // Counted as a sweep that stopped: her nodes are not read this turn.
        let changed = changed.inspect_err(|error| {
            SWEEP_ERRORS.fetch_add(1, Relaxed);
            tracing::warn!(
                kind = kind(error),
                "[memory-nodes] whether anything was taken back could not be read"
            );
        })?;
        if changed.is_none_or(|at| at.with_timezone(&chrono::Utc) < swept) {
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
            // On her nodes (an intent too), and in her inbox: a memory only
            // an inbox item or an intent names is found as well.
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
               WHERE i.user_id = $1 AND i.audience = $2) cited \
             WHERE cited.ref LIKE 'memory:%'",
            [user_id.into(), audience.into()],
        ))
        .await?;
    let ids: Vec<String> = rows
        .iter()
        .map(|row| row.try_get::<String>("", "id"))
        .collect::<Result<_, _>>()?;
    let taken_back: Vec<String> = super::super::known::named(db, ids)
        .await?
        .into_iter()
        // Not here and with no mark of being forgotten, it may only not
        // have come yet: hidden where she reads, but nothing is forgotten.
        .filter(|(_, named)| named.taken_back && named.sure)
        .map(|(id, _)| id)
        .collect();
    let left = left_resting(db, user_id, audience).await?;
    if taken_back.is_empty() && left == 0 {
        return Ok(());
    }
    let txn = db.begin().await?;
    if !taken_back.is_empty() {
        super::forget(&txn, &taken_back, &[]).await?;
    }
    if left > 0 {
        super::forget_rows(&txn, user_id, audience, &BTreeSet::new(), BTreeSet::new()).await?;
    }
    txn.commit().await?;
    SWEPT.fetch_add(1, Relaxed);
    // What was left to the sweep is done now, as far as these go.
    tracing::info!(
        memories = taken_back.len(),
        left,
        "[memory-nodes] what rested on memories taken back, or on entries forgotten before, forgotten"
    );
    Ok(())
}

/// What a version rests on (`because`, or each summary sentence's) that is
/// an entry or a span already forgotten (a stub), as SQL over a row `d`.
const RESTS_ON_A_STUB: &str = "agent_node_rows d \
     CROSS JOIN LATERAL ( \
       SELECT jsonb_array_elements_text(COALESCE(d.body::jsonb->'because', '[]'::jsonb)) AS ref \
       UNION ALL \
       SELECT jsonb_array_elements_text(COALESCE(s->'because', '[]'::jsonb)) \
       FROM jsonb_array_elements(CASE WHEN jsonb_typeof(d.body::jsonb->'value') = 'array' \
                                      THEN d.body::jsonb->'value' ELSE '[]'::jsonb END) s \
       UNION ALL \
       SELECT jsonb_array_elements_text(COALESCE(d.body::jsonb->'base', '[]'::jsonb)) \
     ) refs \
     JOIN agent_node_rows e ON e.user_id = d.user_id AND e.audience = d.audience AND e.body = '' \
       AND ((e.kind IN ('node.entry', 'node.span') \
             AND refs.ref = (CASE e.kind WHEN 'node.entry' THEN 'entry:' ELSE 'span:' END) || e.row_id) \
         OR (e.kind IN ('node.view_changed', 'node.open_item', 'node.standing', 'node.revised') \
             AND refs.ref = e.row_id)) \
     WHERE d.body <> '' \
       AND d.kind IN ('node.view_changed', 'node.open_item', 'node.standing', 'node.revised')";

/// How many versions in `user_id`'s `audience` still rest on an entry or a
/// span forgotten before upkeep's evidence was followed ([`resting_on`]).
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
