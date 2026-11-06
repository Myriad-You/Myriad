//! Forgetting on her nodes what goes with a forgotten memory (decision D7):
//! its entries, the spans and inbox items whose words were forgotten with
//! it (the lines `memory::origins` deletes), the versions of her views and
//! of what is not wrapped up that rest on either, and a topic she started that
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
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

use super::{Error, heads, inbox, write};
use crate::services::her_log::{self, NODE};

/// The rows citing any of `refs`: entries and spans by their references,
/// her views and what is not wrapped up by what they rest on (`because`);
/// each with its account, audience and node.
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
                      WHEN r.kind IN ('node.view_changed', 'node.open_item') THEN 'because' \
                      ELSE 'refs' END), '[]'::jsonb) END) AS cited(ref) \
             WHERE r.kind IN ('node.entry', 'node.span', 'node.view_changed', 'node.open_item') \
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
    let hlc = her_log::store::next_rev(db).await?;
    let count = write::last_count(db, user_id, audience).await? + 1;
    let row = Row {
        version: LOG_VERSION,
        id: myriad_nodes::entries::row_id_from("forget", node, &source, ""),
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
        write::lock(db, user_id, &audience).await?;
        let kept = heads::rows_of(db, user_id, &audience).await?;
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
    }
    Ok(())
}

/// The node a row is about, without `node:`.
pub(super) fn row_node(row: &Row) -> String {
    row.object
        .strip_prefix("node:")
        .unwrap_or(&row.object)
        .to_string()
}
