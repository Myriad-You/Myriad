//! How long what stands of a life has been seen to still hold
//! (`agent_node_kept`): an item of a 「现在」 that a fact was put to and left
//! standing (the core's `stillSo`), or that a build started, with the
//! memory that showed it and when that memory was learned. The latest
//! [`PER_ITEM`] by that day are kept per item.
//!
//! An item's go when it is set or ended (its words changed: what was seen
//! of the old words says nothing of the new), a memory's when it is
//! forgotten or taken back, a node's with the node, everything with the
//! person. Only on this end: not on her log, so her other ends do not get
//! it.
use std::collections::{BTreeSet, HashMap};

use myriad_nodes::stands::{self, RowOut, StillSo};
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

use super::Error;

/// How many are kept per item: the latest by when the memory was learned.
pub(crate) const PER_ITEM: i64 = 3;

/// Notes that what `fact` (a memory) was put to and left standing, `still`
/// (the core's `stillSo`), still holds. What fails is logged, by kind; the
/// rows written stand either way.
pub(crate) async fn seen<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
    fact: &str,
    still: &[StillSo],
) {
    let seen: Vec<(String, String, String)> = still
        .iter()
        .map(|one| (one.node.clone(), one.item.clone(), fact.to_string()))
        .collect();
    logged(note(db, user_id, audience, &seen).await, "seen");
}

/// Notes each item a build started as seen to hold by the latest learned
/// of the memories it rests on (`because`). What fails is logged, by kind.
pub(crate) async fn started<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
    rows: &[RowOut],
) {
    let result = async {
        // Only what was written: a node that could not take its rows has
        // nothing of them to note.
        let ids: Vec<String> = rows.iter().map(|row| row.id.clone()).collect();
        let written: BTreeSet<String> = db
            .query_all_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT row_id FROM agent_node_rows \
                 WHERE user_id = $1 AND audience = $2 AND row_id = ANY($3) AND body <> ''",
                [user_id.into(), audience.into(), ids.into()],
            ))
            .await?
            .iter()
            .map(|row| row.try_get::<String>("", "row_id"))
            .collect::<Result<_, _>>()?;
        let mut each: Vec<(String, String, Vec<String>)> = Vec::new();
        for row in rows
            .iter()
            .filter(|row| row.kind == stands::KIND && written.contains(&row.id))
        {
            let Some(item) = row.body["item"].as_str() else {
                continue;
            };
            let memories: Vec<String> = row.body["because"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|cited| cited.as_str()?.strip_prefix(myriad_nodes::body::MEMORY_REF))
                .map(str::to_string)
                .collect();
            each.push((row.node.clone(), item.to_string(), memories));
        }
        let all: Vec<String> = each
            .iter()
            .flat_map(|(_, _, memories)| memories.clone())
            .collect();
        let learned = super::known::learned(db, all).await?;
        let seen: Vec<(String, String, String)> = each
            .into_iter()
            .filter_map(|(node, item, memories)| {
                let latest = memories
                    .into_iter()
                    .filter_map(|memory| Some((*learned.get(&memory)?, memory)))
                    .max()?;
                Some((node, item, latest.1))
            })
            .collect();
        note(db, user_id, audience, &seen).await
    }
    .await;
    logged(result, "started");
}

fn logged(result: Result<(), Error>, what: &'static str) {
    if let Err(error) = result {
        tracing::warn!(
            what,
            kind = super::forget::error_kind(&error),
            "[memory-nodes] what was seen to still hold not noted"
        );
    }
}

/// Notes that each of `seen` (node, item, memory) of `user_id`'s
/// `audience` was seen to still hold by that memory, at when it was
/// learned; a memory no longer held, or an item no longer standing on its
/// node (forgotten or ended in between), notes nothing. Then each item keeps
/// only its latest [`PER_ITEM`].
pub(crate) async fn note<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
    seen: &[(String, String, String)],
) -> Result<(), Error> {
    if seen.is_empty() {
        return Ok(());
    }
    let memories: Vec<String> = seen
        .iter()
        .map(|(_, _, memory)| memory.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let learned: HashMap<String, i64> = super::known::learned(db, memories).await?;
    let (mut nodes, mut items, mut by, mut at) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (node, item, memory) in seen {
        if let Some(when) = learned.get(memory) {
            nodes.push(node.clone());
            items.push(item.clone());
            by.push(memory.clone());
            at.push(*when);
        }
    }
    if nodes.is_empty() {
        return Ok(());
    }
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_node_kept (user_id, audience, node_id, item, by_memory, at) \
         SELECT $1, $2, s.node, s.item, s.memory, \
                'epoch'::timestamptz + s.at * interval '1 microsecond' \
         FROM unnest($3::text[], $4::text[], $5::text[], $6::bigint[]) AS s(node, item, memory, at) \
         WHERE EXISTS (SELECT 1 FROM agent_memories m \
                       WHERE m.id = s.memory AND m.invalid_at IS NULL) \
           AND EXISTS (SELECT 1 FROM agent_node_rows r \
                       WHERE r.user_id = $1 AND r.audience = $2 AND r.node_id = s.node \
                         AND r.kind = $7 AND r.body <> '' \
                         AND (r.body::jsonb->>'item') = s.item) \
           AND NOT EXISTS (SELECT 1 FROM agent_node_rows r \
                       WHERE r.user_id = $1 AND r.audience = $2 AND r.node_id = s.node \
                         AND r.kind = $7 AND r.body <> '' \
                         AND (r.body::jsonb->>'item') = s.item \
                         AND (r.body::jsonb->>'ended') = 'true') \
         ON CONFLICT (user_id, audience, node_id, item, by_memory) DO UPDATE SET at = EXCLUDED.at",
        [
            user_id.into(),
            audience.into(),
            nodes.clone().into(),
            items.clone().into(),
            by.into(),
            at.into(),
            stands::KIND.into(),
        ],
    ))
    .await?;
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM agent_node_kept k USING ( \
           SELECT node_id, item, by_memory, row_number() OVER ( \
             PARTITION BY node_id, item ORDER BY at DESC, by_memory DESC) AS n \
           FROM agent_node_kept \
           WHERE user_id = $1 AND audience = $2 \
             AND (node_id, item) IN (SELECT * FROM unnest($3::text[], $4::text[]))) old \
         WHERE k.user_id = $1 AND k.audience = $2 AND k.node_id = old.node_id \
           AND k.item = old.item AND k.by_memory = old.by_memory AND old.n > $5",
        [
            user_id.into(),
            audience.into(),
            nodes.into(),
            items.into(),
            PER_ITEM.into(),
        ],
    ))
    .await?;
    Ok(())
}

/// A row written on `node` of `user_id`'s `audience`, here or taken in
/// from another of her ends: a version of an item of a 「现在」 (`body`, its
/// words as stored) lets go of what was noted of that item, for its words
/// changed or it ended. On the transaction that writes the row.
pub(crate) async fn row_written<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
    node: &str,
    kind: &str,
    body: &str,
) -> Result<(), Error> {
    if kind != stands::KIND || body.is_empty() {
        return Ok(());
    }
    let Some(item) = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|body| body["item"].as_str().map(str::to_string))
    else {
        return Ok(());
    };
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM agent_node_kept \
         WHERE user_id = $1 AND audience = $2 AND node_id = $3 AND item = $4",
        [user_id.into(), audience.into(), node.into(), item.into()],
    ))
    .await?;
    Ok(())
}

/// A row of `user_id`'s `audience` about to be forgotten (`row`, read
/// before it is stubbed): a version of an item lets go of what was noted
/// of that item.
pub(crate) async fn row_forgotten<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
    row: &str,
) -> Result<(), Error> {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM agent_node_kept k USING agent_node_rows r \
         WHERE r.user_id = $1 AND r.audience = $2 AND r.row_id = $3 \
           AND r.kind = $4 AND r.body <> '' \
           AND k.user_id = r.user_id AND k.audience = r.audience \
           AND k.node_id = r.node_id AND k.item = (r.body::jsonb->>'item')",
        [
            user_id.into(),
            audience.into(),
            row.into(),
            stands::KIND.into(),
        ],
    ))
    .await?;
    Ok(())
}

/// What was noted in `user_id`'s `from` audience goes with their talk to
/// `to` (`partner::move_talk`), item by item as its standing rows went; each item
/// then keeps its latest [`PER_ITEM`].
pub(crate) async fn moved<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    from: &str,
    to: &str,
) -> Result<(), Error> {
    // Rows move one by one: the notes go where their item now stands, and
    // stay while any version of it still stands here.
    const WENT: &str = "EXISTS (SELECT 1 FROM agent_node_rows r \
         WHERE r.user_id = k.user_id AND r.audience = $3 AND r.node_id = k.node_id \
           AND r.kind = $4 AND r.body <> '' AND (r.body::jsonb->>'item') = k.item) \
         AND NOT EXISTS (SELECT 1 FROM agent_node_rows r \
         WHERE r.user_id = k.user_id AND r.audience = $2 AND r.node_id = k.node_id \
           AND r.kind = $4 AND r.body <> '' AND (r.body::jsonb->>'item') = k.item)";
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        &format!(
            "INSERT INTO agent_node_kept (user_id, audience, node_id, item, by_memory, at) \
             SELECT k.user_id, $3, k.node_id, k.item, k.by_memory, k.at FROM agent_node_kept k \
             WHERE k.user_id = $1 AND k.audience = $2 AND {WENT} \
             ON CONFLICT (user_id, audience, node_id, item, by_memory) DO NOTHING"
        ),
        [user_id.into(), from.into(), to.into(), stands::KIND.into()],
    ))
    .await?;
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        &format!(
            "DELETE FROM agent_node_kept k WHERE k.user_id = $1 AND k.audience = $2 AND {WENT}"
        ),
        [user_id.into(), from.into(), to.into(), stands::KIND.into()],
    ))
    .await?;
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM agent_node_kept k USING ( \
           SELECT node_id, item, by_memory, row_number() OVER ( \
             PARTITION BY node_id, item ORDER BY at DESC, by_memory DESC) AS n \
           FROM agent_node_kept WHERE user_id = $1 AND audience = $2) old \
         WHERE k.user_id = $1 AND k.audience = $2 AND k.node_id = old.node_id \
           AND k.item = old.item AND k.by_memory = old.by_memory AND old.n > $3",
        [user_id.into(), to.into(), PER_ITEM.into()],
    ))
    .await?;
    Ok(())
}

/// Before the log's stubs are made here (`logged::take_in`): what was
/// noted of the item of each version of `user_id`'s `audience` the log
/// forgot.
pub(crate) async fn log_forgotten<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
) -> Result<(), Error> {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM agent_node_kept k USING agent_node_rows n, agent_log_rows l \
         WHERE n.user_id = $1 AND n.audience = $2 AND l.user_id = n.user_id \
           AND l.id = n.row_id AND l.forgotten AND n.body <> '' AND n.kind = $3 \
           AND k.user_id = n.user_id AND k.audience = n.audience \
           AND k.node_id = n.node_id AND k.item = (n.body::jsonb->>'item')",
        [user_id.into(), audience.into(), stands::KIND.into()],
    ))
    .await?;
    Ok(())
}

/// Lets go of what the memories `memories` showed, wherever: they are
/// forgotten or taken back.
pub(crate) async fn forget<C: ConnectionTrait>(db: &C, memories: &[String]) -> Result<(), Error> {
    if memories.is_empty() {
        return Ok(());
    }
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM agent_node_kept WHERE by_memory = ANY($1)",
        [memories.to_vec().into()],
    ))
    .await?;
    Ok(())
}

/// Lets go of what was noted on `node` of `user_id`'s `audience`: it is
/// forgotten whole.
pub(crate) async fn forget_node<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
    node: &str,
) -> Result<(), Error> {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM agent_node_kept WHERE user_id = $1 AND audience = $2 AND node_id = $3",
        [user_id.into(), audience.into(), node.into()],
    ))
    .await?;
    Ok(())
}

/// What was noted on `nodes` of `user_id`'s `audience`: by (node, item),
/// the memories that showed it still holding, latest learned first. One
/// read, of those nodes only; whether each memory still stands to be said
/// is the reader's to tell, with the rest it names.
pub(crate) async fn on_nodes<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
    nodes: Vec<String>,
) -> Result<HashMap<(String, String), Vec<String>>, Error> {
    let mut kept: HashMap<(String, String), Vec<String>> = HashMap::new();
    if nodes.is_empty() {
        return Ok(kept);
    }
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT node_id, item, by_memory FROM agent_node_kept \
             WHERE user_id = $1 AND audience = $2 AND node_id = ANY($3) \
             ORDER BY node_id, item, at DESC, by_memory DESC",
            [user_id.into(), audience.into(), nodes.into()],
        ))
        .await?;
    for row in &rows {
        kept.entry((row.try_get("", "node_id")?, row.try_get("", "item")?))
            .or_default()
            .push(row.try_get("", "by_memory")?);
    }
    Ok(kept)
}

/// What was noted of `node`'s items of `user_id`'s `audience`, item by
/// item, latest first: (item, memory, Unix microseconds learned).
#[cfg(test)]
pub(crate) async fn of_node<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
    node: &str,
) -> Result<Vec<(String, String, i64)>, Error> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT item, by_memory, at FROM agent_node_kept \
             WHERE user_id = $1 AND audience = $2 AND node_id = $3 \
             ORDER BY item, at DESC, by_memory DESC",
            [user_id.into(), audience.into(), node.into()],
        ))
        .await?;
    rows.iter()
        .map(|row| {
            let at: chrono::DateTime<chrono::FixedOffset> = row.try_get("", "at")?;
            Ok((
                row.try_get::<String>("", "item")?,
                row.try_get::<String>("", "by_memory")?,
                at.timestamp_micros(),
            ))
        })
        .collect()
}
