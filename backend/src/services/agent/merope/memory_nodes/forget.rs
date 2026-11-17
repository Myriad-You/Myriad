//! Forgetting on her nodes what goes with a forgotten memory (decision D7):
//! its entries, the spans and inbox items whose words were forgotten with
//! it (the lines `memory::origins` deletes), the versions of her views,
//! of what is not wrapped up and of how their life stands (「现在」) that
//! rest on either (one resting on several goes when any of them does: its
//! words may carry what was forgotten), and a topic she started that
//! holds no entry any more, for its name can say as much as what was on it
//! (the rule the old topic tables keep, `migrations/topic_nodes.sql`). An
//! intent resting on a forgotten line goes too, with what the site kept of
//! the instruction ([`super::intent_store::forget_resting_on`]).
//!
//! The core forgets a whole node (a `forget` row about `node:<id>`) or one
//! row of it (about `row:<id>`), and every fold reads both. Here each entry
//! or span is forgotten by a forget of its row, so another side holding it
//! forgets it as well once her nodes sync, and becomes the core's stub of
//! it at once (`Row::stub`: no payload, marked forgotten, its place in the
//! log kept) with its body taken away, which is what a forget makes of a
//! row on every side; a forgotten node's rows become stubs too, so no word
//! of it is kept.
//!
//! Forgetting is the person's own act (`owner` rows, no pen needed), runs
//! in the forgetting's own transaction under each audience's lock.

use std::collections::{BTreeMap, BTreeSet};

use myriad_nodes::Kind;
use myriad_persona_log::{
    Row,
    row::{Author, LOG_VERSION, Replicate},
};
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionSession, TransactionTrait};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

use super::{Error, heads, inbox, logged, write};
use crate::services::her_log::{self, NODE};

/// The rows citing any of `refs`: entries and spans by their references,
/// her views, what is not wrapped up and how their life stands (「现在」)
/// by what they rest on (`because`); each with its account, audience and
/// node.
async fn citing<C: ConnectionTrait>(
    db: &C,
    refs: &[String],
) -> Result<Vec<(i32, String, String, String)>, Error> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT DISTINCT r.user_id, r.audience, r.node_id, r.row_id FROM agent_node_rows r \
             CROSS JOIN LATERAL jsonb_array_elements_text( \
               CASE WHEN r.body = '' THEN '[]'::jsonb \
                    ELSE COALESCE((r.body::jsonb)->(CASE \
                      WHEN r.kind IN ('node.view_changed', 'node.open_item', 'node.standing') THEN 'because' \
                      ELSE 'refs' END), '[]'::jsonb) END) AS cited(ref) \
             WHERE r.kind IN ('node.entry', 'node.span', 'node.view_changed', 'node.open_item', 'node.standing') \
               AND cited.ref = ANY($1)",
            [refs.to_vec().into()],
        ))
        .await?;
    rows.iter()
        .map(|row| {
            Ok((
                row.try_get("", "user_id")?,
                row.try_get("", "audience")?,
                row.try_get("", "node_id")?,
                row.try_get("", "row_id")?,
            ))
        })
        .collect()
}

/// The kinds of row that are versions of an item, each written on the
/// ones before it (its `base`): her views, what is not wrapped up, how
/// their life stands, and a head field (a summary, …).
const VERSIONED: &[&str] = &[
    "node.view_changed",
    "node.open_item",
    "node.standing",
    "node.revised",
];

/// `doomed` (row ids of `kept`) and every version that rests on an entry
/// or a span among them, or on one forgotten before: what her upkeep writes (a summary sentence, a
/// view, what is not wrapped up) names its evidence by row (`entry:<row>`,
/// `span:<row>`), not by the memory, and its words were written from it.
pub(super) fn resting_on(kept: &[heads::KeptRow], doomed: BTreeSet<String>) -> BTreeSet<String> {
    // What goes now, and what went before: a version a forgetting before
    // this one left resting on an entry it took.
    let evidence: BTreeSet<String> = kept
        .iter()
        .filter(|kept| doomed.contains(&kept.row.id) || kept.row.forgotten)
        .filter_map(|kept| match kept.row.kind.as_str() {
            "node.entry" => Some(format!("entry:{}", kept.row.id)),
            "node.span" => Some(format!("span:{}", kept.row.id)),
            _ => None,
        })
        .collect();
    if evidence.is_empty() {
        return doomed;
    }
    let names = |because: &serde_json::Value| {
        because.as_array().is_some_and(|ids| {
            ids.iter()
                .any(|id| id.as_str().is_some_and(|id| evidence.contains(id)))
        })
    };
    let more: Vec<String> = kept
        .iter()
        .filter(|kept| VERSIONED.contains(&kept.row.kind.as_str()))
        .filter(|kept| {
            kept.body
                .as_deref()
                .and_then(|body| serde_json::from_str::<serde_json::Value>(body).ok())
                .is_some_and(|body| {
                    names(&body["because"])
                        || body["value"].as_array().is_some_and(|sentences| {
                            sentences.iter().any(|sentence| names(&sentence["because"]))
                        })
                })
        })
        .map(|kept| kept.row.id.clone())
        .collect();
    let mut doomed = doomed;
    doomed.extend(more);
    doomed
}

/// `doomed` (row ids of `kept`) and every version written on one of them,
/// however far down: its words were written from what goes, so they go
/// too, and the item stands again as it was before the first of them.
pub(super) fn written_on(
    kept: &[heads::KeptRow],
    mut doomed: BTreeSet<String>,
) -> BTreeSet<String> {
    loop {
        let more: Vec<String> = kept
            .iter()
            .filter(|kept| {
                !doomed.contains(&kept.row.id) && VERSIONED.contains(&kept.row.kind.as_str())
            })
            .filter(|kept| {
                kept.body
                    .as_deref()
                    .and_then(|body| serde_json::from_str::<serde_json::Value>(body).ok())
                    .and_then(|body| body.get("base").cloned())
                    .and_then(|base| base.as_array().cloned())
                    .is_some_and(|base| {
                        base.iter()
                            .any(|row| row.as_str().is_some_and(|row| doomed.contains(row)))
                    })
            })
            .map(|kept| kept.row.id.clone())
            .collect();
        if more.is_empty() {
            return doomed;
        }
        doomed.extend(more);
    }
}

/// Rows of `user_id` made stubs: no payload, no body.
pub(super) async fn stub<C: ConnectionTrait>(db: &C, user_id: i32, rows: &[Row]) -> Result<(), Error> {
    // A forget is never a stub: it holds no words.
    for row in rows
        .iter()
        .filter(|row| !row.forgotten && row.payload.is_some())
    {
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE agent_node_rows SET body = '', envelope = $3 WHERE user_id = $1 AND row_id = $2",
            [
                user_id.into(),
                row.id.clone().into(),
                row.stub().to_value().into(),
            ],
        ))
        .await?;
    }
    Ok(())
}

/// What a forget names: a whole node, or one row on it.
pub(super) enum Forgets<'a> {
    Node,
    Row(&'a str),
}

/// The person's forget of `node`, or of one row on it, in `user_id`'s
/// `audience`, kept as a row (filed under `node`).
pub(super) async fn forget_one<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
    node: &str,
    what: Forgets<'_>,
) -> Result<(), Error> {
    let (source, object) = match what {
        Forgets::Node => ("forget".to_string(), format!("node:{node}")),
        Forgets::Row(row) => (
            format!("{}:{row}", myriad_persona_log::kinds::ROW),
            format!("{}:{row}", myriad_persona_log::kinds::ROW),
        ),
    };
    let id = myriad_nodes::entries::row_id_from("forget", node, &source, "");
    // The same forget again: kept already, and no count is taken for it.
    // Asked under the audience's lock, so two at once do not both write.
    write::lock(db, user_id, audience).await?;
    let known = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT 1 AS one FROM agent_node_rows WHERE user_id = $1 AND row_id = $2",
            [user_id.into(), id.clone().into()],
        ))
        .await?;
    if known.is_some() {
        return Ok(());
    }
    // In an audience her log keeps, while she syncs it, the forget goes
    // into the log too, after what is not in it yet (`logged`).
    let into_log = logged::into_log(db, user_id, audience).await?;
    if let Some(owner) = &into_log {
        logged::catch_up(db, her_log::keys::root(), owner, audience).await?;
    }
    let hlc = her_log::store::next_rev(db).await?;
    let count = match &into_log {
        Some(_) => her_log::store::own_count(db, user_id, audience).await?,
        None => write::last_count(db, user_id, audience).await? + 1,
    };
    let row = Row {
        version: LOG_VERSION,
        id,
        origin: NODE.to_string(),
        author: Author::Owner,
        epoch: None,
        dseq: Some(count),
        hlc,
        audience: audience.to_string(),
        subject: None,
        object,
        kind: "forget".to_string(),
        payload: None,
        replicate: Replicate::Shared,
        forgotten: false,
        to: None,
        takes: Vec::new(),
    };
    row.check()
        .map_err(|_| Error::Refused("a forget's envelope does not check".into()))?;
    let at = chrono::DateTime::from_timestamp_millis(row.hlc.wall_ms as i64)
        .ok_or_else(|| Error::Refused("a row's clock is out of range".into()))?
        .fixed_offset();
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_node_rows (user_id, audience, node_id, row_id, kind, body, said_by, \
         author, at, hlc, origin, dseq, epoch, envelope) \
         VALUES ($1, $2, $3, $4, 'forget', '', '[]'::jsonb, 'owner', $5, $6, $7, $8, NULL, $9) \
         ON CONFLICT (user_id, row_id) DO NOTHING",
        [
            user_id.into(),
            audience.into(),
            node.into(),
            row.id.clone().into(),
            at.into(),
            row.hlc.to_string().into(),
            row.origin.clone().into(),
            (count as i64).into(),
            row.to_value().into(),
        ],
    ))
    .await?;
    if let Some(owner) = &into_log {
        her_log::store::keep_own(db, her_log::keys::root(), owner, &row, None).await?;
    }
    Ok(())
}

/// Whether `node` is a topic she started that holds no entry any more.
fn empty_topic(node: &myriad_nodes::Node) -> bool {
    !node.forgotten
        && node.absorbed_into.is_none()
        && node.entries.is_empty()
        && node.created_by == Some(Author::Her)
        && node
            .kind
            .as_ref()
            .is_some_and(|kind| kind.current.value.kind == Kind::Topic)
}

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
}

static LEFT_TO_SWEEP: AtomicU64 = AtomicU64::new(0);
static SWEPT: AtomicU64 = AtomicU64::new(0);
static SWEEP_ERRORS: AtomicU64 = AtomicU64::new(0);

/// [`Failures`] so far.
pub(crate) fn failures() -> Failures {
    Failures {
        left_to_sweep: LEFT_TO_SWEEP.load(Relaxed),
        swept: SWEPT.load(Relaxed),
        sweep_errors: SWEEP_ERRORS.load(Relaxed),
    }
}

/// Which error it was, for a log line that carries no words.
fn kind(error: &Error) -> &'static str {
    match error {
        Error::Refused(_) => "refused",
        Error::NoPen => "no_pen",
        Error::Db(_) => "database",
    }
}

/// How far back a line of memories, each replaced by the next, is followed.
const LINE_STEPS: i32 = 16;

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
            "WITH RECURSIVE line(id, depth) AS ( \
               SELECT id, 0 FROM agent_memories WHERE id = ANY($1) \
               UNION \
               SELECT m.id, line.depth + 1 FROM agent_memories m \
               JOIN line ON m.superseded_by = line.id WHERE line.depth < $2) \
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
            forget(&savepoint, &line, &[]).await
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
        LEFT_TO_SWEEP.fetch_add(memories.len() as u64, Relaxed);
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
    let swept = sweep(db, user_id, audience).await;
    if let Err(error) = &swept {
        SWEEP_ERRORS.fetch_add(1, Relaxed);
        tracing::warn!(
            kind = kind(error),
            "[memory-nodes] forgetting what rests on memories taken back stopped"
        );
    }
    swept
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
            "SELECT DISTINCT substr(cited.ref, 8) AS id FROM agent_node_rows r \
             CROSS JOIN LATERAL jsonb_array_elements_text( \
               COALESCE((r.body::jsonb)->(CASE \
                 WHEN r.kind IN ('node.view_changed', 'node.open_item', 'node.standing') THEN 'because' \
                 ELSE 'refs' END), '[]'::jsonb)) AS cited(ref) \
             WHERE r.user_id = $1 AND r.audience = $2 AND r.body <> '' \
               AND r.kind IN ('node.entry', 'node.view_changed', 'node.open_item', 'node.standing') \
               AND cited.ref LIKE 'memory:%'",
            [user_id.into(), audience.into()],
        ))
        .await?;
    let ids: Vec<String> = rows
        .iter()
        .map(|row| row.try_get::<String>("", "id"))
        .collect::<Result<_, _>>()?;
    let taken_back: Vec<String> = super::known::named(db, ids)
        .await?
        .into_iter()
        .filter(|(_, named)| named.taken_back)
        .map(|(id, _)| id)
        .collect();
    let left = left_resting(db, user_id, audience).await?;
    if taken_back.is_empty() && left == 0 {
        return Ok(());
    }
    let txn = db.begin().await?;
    if !taken_back.is_empty() {
        forget(&txn, &taken_back, &[]).await?;
    }
    if left > 0 {
        forget_rows(&txn, user_id, audience, &BTreeSet::new(), BTreeSet::new()).await?;
    }
    txn.commit().await?;
    SWEPT.fetch_add(1, Relaxed);
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
     ) refs \
     JOIN agent_node_rows e ON e.user_id = d.user_id AND e.audience = d.audience AND e.body = '' \
       AND e.kind IN ('node.entry', 'node.span') \
       AND refs.ref = (CASE e.kind WHEN 'node.entry' THEN 'entry:' ELSE 'span:' END) || e.row_id \
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

/// Forgets on her nodes what goes with the memories `memories` (ids) and
/// the lines `lines` (`agent_messages.id`) being forgotten, on `db`'s
/// transaction ([module docs](self)).
pub(crate) async fn forget<C: ConnectionTrait>(
    db: &C,
    memories: &[String],
    lines: &[i32],
) -> Result<(), Error> {
    let refs: Vec<String> = memories
        .iter()
        .map(|id| format!("{}{id}", myriad_nodes::body::MEMORY_REF))
        .chain(lines.iter().map(|id| format!("message:{id}")))
        .collect();
    if refs.is_empty() {
        return Ok(());
    }
    inbox::forget_citing(db, &refs).await?;
    // Her intents resting on the lines, or on the rows, go with them.
    super::intent_store::forget_resting_on(db, lines, memories).await?;
    let mut by_audience: BTreeMap<(i32, String), (BTreeSet<String>, BTreeSet<String>)> =
        BTreeMap::new();
    for (user_id, audience, node, row) in citing(db, &refs).await? {
        let (nodes, rows) = by_audience.entry((user_id, audience)).or_default();
        nodes.insert(node);
        rows.insert(row);
    }
    for ((user_id, audience), (nodes, cited)) in by_audience {
        forget_rows(db, user_id, &audience, &nodes, cited).await?;
    }
    Ok(())
}

/// Forgets `cited` (rows of `user_id`'s `audience`, on `nodes`) with what
/// rests on them or was written on them, then a topic of `nodes` they
/// leave empty; the audience's heads refreshed.
async fn forget_rows<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
    nodes: &BTreeSet<String>,
    cited: BTreeSet<String>,
) -> Result<(), Error> {
    let audience = audience.to_string();
    write::lock(db, user_id, &audience).await?;
    let kept = heads::rows_of(db, user_id, &audience).await?;
    let cited = written_on(&kept, resting_on(&kept, cited));
    let doomed: Vec<Row> = kept
        .iter()
        .filter(|kept| cited.contains(&kept.row.id))
        .map(|kept| kept.row.clone())
        .collect();
    for row in doomed.iter().filter(|row| !row.forgotten) {
        forget_one(db, user_id, &audience, &row_node(row), Forgets::Row(&row.id)).await?;
    }
    stub(db, user_id, &doomed).await?;
    // What held them: the nodes they were written on, or went into.
    let folded = heads::fold_rows(&heads::rows_of(db, user_id, &audience).await?)?;
    let mut holders: BTreeSet<String> = nodes.clone();
    for node in &folded.nodes {
        if nodes.contains(&node.id)
            && let Some(into) = &node.absorbed_into
        {
            holders.insert(into.into.clone());
            holders.insert(myriad_nodes::node_id(&audience, &into.into).unwrap_or_default());
        }
    }
    let emptied: Vec<String> = folded
        .nodes
        .iter()
        .filter(|node| holders.contains(&node.id) && empty_topic(node))
        .map(|node| node.id.clone())
        .collect();
    for node in &emptied {
        forget_one(db, user_id, &audience, node, Forgets::Node).await?;
    }
    if !emptied.is_empty() {
        let gone: Vec<Row> = heads::rows_of(db, user_id, &audience)
            .await?
            .into_iter()
            .filter(|kept| emptied.contains(&row_node(&kept.row)))
            .map(|kept| kept.row)
            .collect();
        stub(db, user_id, &gone).await?;
    }
    let folded = heads::fold_rows(&heads::rows_of(db, user_id, &audience).await?)?;
    heads::refresh(db, user_id, &audience, &folded).await?;
    Ok(())
}

/// The node a row is about, without `node:`.
pub(super) fn row_node(row: &Row) -> String {
    row.object
        .strip_prefix("node:")
        .unwrap_or(&row.object)
        .to_string()
}

/// Whether a kept row's body names `reference` in `because` or `refs`.
fn names(kept: &heads::KeptRow, reference: &str) -> bool {
    kept.body
        .as_deref()
        .and_then(|body| serde_json::from_str::<serde_json::Value>(body).ok())
        .is_some_and(|body| myriad_nodes::sessions::names_session(&body, reference))
}

/// Forgets for good, in `user_id`'s `audience`, everything that names
/// `reference` (a device's grant to tell her of its sessions): a node whose
/// key starts with `whole` that such a row is on goes whole, every other
/// row that names it goes alone. A node it only added to stays, with what
/// it holds of anything else: a forgotten node never comes back under its
/// key, and a repository is one whole for every grant. The person's own
/// act, as any forget: no pen.
pub(crate) async fn forget_named<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
    reference: &str,
    whole: &str,
) -> Result<(), Error> {
    write::lock(db, user_id, audience).await?;
    let kept = heads::rows_of(db, user_id, audience).await?;
    let folded = heads::fold_rows(&kept)?;
    let key_of = |node: &str| {
        folded
            .nodes
            .iter()
            .find(|folded| folded.id == node)
            .and_then(|folded| folded.key.clone())
            .unwrap_or_default()
    };
    let mut wholes: BTreeSet<String> = BTreeSet::new();
    let mut alone: Vec<Row> = Vec::new();
    for kept in kept
        .iter()
        .filter(|kept| !kept.row.forgotten && names(kept, reference))
    {
        let node = row_node(&kept.row);
        if key_of(&node).starts_with(whole) {
            wholes.insert(node);
        } else {
            alone.push(kept.row.clone());
        }
    }
    for row in &alone {
        forget_one(db, user_id, audience, &row_node(row), Forgets::Row(&row.id)).await?;
    }
    stub(db, user_id, &alone).await?;
    forget_wholes(db, user_id, audience, &wholes).await?;
    let folded = heads::fold_rows(&heads::rows_of(db, user_id, audience).await?)?;
    heads::refresh(db, user_id, audience, &folded).await?;
    Ok(())
}

/// Forgets for good, in `user_id`'s `audience`, each of `nodes` whole: for
/// nodes whose key can never come again (a removed device's folders).
pub(crate) async fn forget_nodes<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
    nodes: &BTreeSet<String>,
) -> Result<(), Error> {
    if nodes.is_empty() {
        return Ok(());
    }
    write::lock(db, user_id, audience).await?;
    forget_wholes(db, user_id, audience, nodes).await?;
    let folded = heads::fold_rows(&heads::rows_of(db, user_id, audience).await?)?;
    heads::refresh(db, user_id, audience, &folded).await?;
    Ok(())
}

/// Forgets each of `nodes` whole: a forget of the node, its rows stubs.
async fn forget_wholes<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
    nodes: &BTreeSet<String>,
) -> Result<(), Error> {
    if nodes.is_empty() {
        return Ok(());
    }
    for node in nodes {
        forget_one(db, user_id, audience, node, Forgets::Node).await?;
    }
    let gone: Vec<Row> = heads::rows_of(db, user_id, audience)
        .await?
        .into_iter()
        .filter(|kept| nodes.contains(&row_node(&kept.row)))
        .map(|kept| kept.row)
        .collect();
    stub(db, user_id, &gone).await
}
