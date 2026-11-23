//! What rests on a memory they took back, hidden where her nodes are read,
//! with nothing forgotten.
//!
//! A memory judged taken back is hidden at once: what she says reads it
//! through [`super::shown`] (`unsaid`), and everything else reads her nodes
//! as if what rests on it were forgotten: its entries and spans, the
//! versions resting on them or written on them, her intents resting on it,
//! and a topic of hers that holds nothing else ([`hide`], the same rule as
//! [`super::forget`]). Only the second look agreeing forgets it for good
//! (`take_back_check`); until then, and for one taken back on another of
//! her ends, it is hidden only: kept as it is, and back as soon as the
//! memory is.
//!
//! Her nodes as they stand for each audience (`agent_node_heads`) are
//! written hidden so ([`folded`], in [`super::heads::refresh`]); where her
//! rows themselves are read for her (her intents coming due, her upkeep),
//! they are read hidden ([`rows_of`]).

use std::collections::{BTreeSet, HashSet};
#[cfg(not(test))]
use std::sync::{LazyLock, Mutex};

use myriad_nodes::{Folded, body::MEMORY_REF};
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use serde_json::Value;

use super::{Error, forget, heads::KeptRow};
use crate::services::agent::merope::chat_lines;

/// What a line of theirs is named by.
const LINE_REF: &str = "message:";

/// Every memory and every line `value` names (`memory:<id>`,
/// `message:<id>`), wherever in it.
fn named_in(value: &Value, into: &mut BTreeSet<String>) {
    match value {
        Value::String(text) => {
            if text.starts_with(MEMORY_REF) || text.starts_with(LINE_REF) {
                into.insert(text.clone());
            }
        }
        Value::Array(items) => items.iter().for_each(|item| named_in(item, into)),
        Value::Object(fields) => fields.values().for_each(|field| named_in(field, into)),
        _ => {}
    }
}

/// Of the memories and the lines of `user_id`'s that `refs` name, those that
/// they took back (or deleted), and the lines that went with those
/// (`chat_lines`), as references. A memory held is never taken back: only those
/// let go are read for it, so a node naming none costs one look. One not
/// here (not come yet from another end, or forgotten there) is not hidden
/// here: [`super::shown`] keeps it unsaid, and forgetting reaches it once
/// its mark comes, as before the second look.
async fn taken_back<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    refs: BTreeSet<String>,
) -> Result<BTreeSet<String>, Error> {
    let lines: Vec<String> = refs
        .iter()
        .filter(|reference| reference.starts_with(LINE_REF))
        .cloned()
        .collect();
    let mut gone = BTreeSet::new();
    if !lines.is_empty() {
        let went = chat_lines::taken_back_with(db, user_id, |_| true).await?;
        gone.extend(lines.into_iter().filter(|line| {
            line.strip_prefix(LINE_REF)
                .and_then(|id| id.parse().ok())
                .is_some_and(|id: i32| went.contains(&id))
        }));
    }
    let ids: Vec<String> = refs
        .iter()
        .filter_map(|reference| reference.strip_prefix(MEMORY_REF))
        .map(str::to_string)
        .collect();
    gone.extend(memories_taken_back(db, ids).await?);
    Ok(gone)
}

async fn memories_taken_back<C: ConnectionTrait>(
    db: &C,
    ids: Vec<String>,
) -> Result<BTreeSet<String>, Error> {
    if ids.is_empty() {
        return Ok(BTreeSet::new());
    }
    let let_go: Vec<String> = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT m.id FROM agent_memories m \
             WHERE m.id = ANY($1::text[]) AND m.invalid_at IS NOT NULL",
            [ids.into()],
        ))
        .await?
        .iter()
        .map(|row| row.try_get::<String>("", "id"))
        .collect::<Result<_, _>>()?;
    if let_go.is_empty() {
        return Ok(BTreeSet::new());
    }
    Ok(super::known::named(db, let_go)
        .await?
        .into_iter()
        .filter(|(_, named)| named.taken_back)
        .map(|(id, _)| format!("{MEMORY_REF}{id}"))
        .collect())
}

/// What `kept` (`user_id`'s) rests on that they took back, as references
/// ([`taken_back`]).
pub(super) async fn gone_in<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    kept: &[KeptRow],
) -> Result<BTreeSet<String>, Error> {
    taken_back(db, user_id, names_in(kept)).await
}

/// Every memory and line `kept` names.
fn names_in(kept: &[KeptRow]) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    for body in kept.iter().filter_map(|kept| kept.body.as_deref()) {
        if let Ok(body) = serde_json::from_str::<Value>(body) {
            named_in(&body, &mut ids);
        }
    }
    ids
}

/// The references a row of `kind` rests on: an entry's or a span's
/// (`refs`), a version's (`because`) and each of its sentences', an
/// intent's (`because`).
fn rests_on(kind: &str, body: &Value) -> Vec<String> {
    let strings = |value: &Value| -> Vec<String> {
        value
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|item| item.as_str().map(str::to_string))
            .collect()
    };
    match kind {
        "node.entry" | "node.span" => strings(&body["refs"]),
        "node.view_changed" | "node.open_item" | "node.standing" | "node.intent" => {
            strings(&body["because"])
        }
        _ => Vec::new(),
    }
}

/// The rows of `kept` that go with `gone` (references), as
/// [`super::forget`] would forget them: entries, spans and versions citing
/// one, and every version resting on those or written on them; her
/// intents resting on one.
fn going(kept: &[KeptRow], gone: &BTreeSet<String>) -> BTreeSet<String> {
    if gone.is_empty() {
        return BTreeSet::new();
    }
    let mut cited = BTreeSet::new();
    let mut intents = BTreeSet::new();
    for row in kept.iter().filter(|kept| !kept.row.forgotten) {
        let Some(body) = row
            .body
            .as_deref()
            .and_then(|body| serde_json::from_str::<Value>(body).ok())
        else {
            continue;
        };
        // With a line that went, only what was made of the line itself
        // ([`forget::OF_A_LINE`]): not a fact or an intent, which may be of
        // the other thing it said.
        if rests_on(&row.row.kind, &body).iter().any(|reference| {
            gone.contains(reference)
                && (reference.starts_with(MEMORY_REF)
                    || forget::OF_A_LINE.contains(&row.row.kind.as_str()))
        }) {
            if row.row.kind == "node.intent" {
                intents.insert(row.row.id.clone());
            } else {
                cited.insert(row.row.id.clone());
            }
        }
    }
    if cited.is_empty() && intents.is_empty() {
        return BTreeSet::new();
    }
    let mut doomed = forget::written_on(kept, forget::resting_on(kept, cited));
    doomed.extend(intents);
    doomed
}

/// `kept` as it reads with what rests on `gone` (references) forgotten: each row that would go ([`going`]) a stub, as forgetting
/// makes it, and nothing written.
pub(super) fn hide(kept: Vec<KeptRow>, gone: &BTreeSet<String>) -> Vec<KeptRow> {
    let doomed = going(&kept, gone);
    stubbed(kept, &doomed)
}

fn stubbed(kept: Vec<KeptRow>, doomed: &BTreeSet<String>) -> Vec<KeptRow> {
    if doomed.is_empty() {
        return kept;
    }
    kept.into_iter()
        .map(|kept| {
            if doomed.contains(&kept.row.id) && !kept.row.forgotten {
                KeptRow {
                    row: kept.row.stub(),
                    body: None,
                }
            } else {
                kept
            }
        })
        .collect()
}

/// Of `refs`, the lines of a stretch of `user_id`'s private talk still to be
/// judged by what they asked her not to keep (`asked::lines`): not read here
/// until it is.
async fn not_judged<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    refs: &BTreeSet<String>,
) -> Result<BTreeSet<String>, Error> {
    let waiting =
        crate::services::agent::merope::asked::lines::waiting(db, Some(user_id), None).await?;
    let ids: Vec<i32> = refs
        .iter()
        .filter_map(|reference| reference.strip_prefix(LINE_REF)?.parse().ok())
        .collect();
    if waiting.is_empty() || ids.is_empty() {
        return Ok(BTreeSet::new());
    }
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id, created_at FROM agent_messages WHERE id = ANY($1)",
            [ids.into()],
        ))
        .await?;
    let mut lines = BTreeSet::new();
    for row in &rows {
        let at = row.try_get("", "created_at")?;
        if waiting.iter().any(|stretch| stretch.holds(user_id, at)) {
            lines.insert(format!("{LINE_REF}{}", row.try_get::<i32>("", "id")?));
        }
    }
    Ok(lines)
}

/// Every row of `user_id`'s `audience`, with what rests on a memory they
/// took back hidden ([`hide`]), or on lines not judged yet ([`not_judged`]):
/// her rows as they are read for her.
pub(crate) async fn rows_of<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
) -> Result<Vec<KeptRow>, Error> {
    let kept = super::heads::rows_of(db, user_id, audience).await?;
    let refs = names_in(&kept);
    let mut gone = taken_back(db, user_id, refs.clone()).await?;
    gone.extend(not_judged(db, user_id, &refs).await?);
    Ok(hide(kept, &gone))
}

/// The row ids of `user_id`'s `audience` that read as stubs, hidden
/// ([`hide`]).
pub(crate) async fn going_of<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
) -> Result<BTreeSet<String>, Error> {
    let kept = super::heads::rows_of(db, user_id, audience).await?;
    let gone = gone_in(db, user_id, &kept).await?;
    Ok(going(&kept, &gone))
}

/// The audiences whose heads were last written hidden ([`folded`]) since
/// the site started: written again when it is swept, so what comes back
/// (the memory brought back, or one not here yet come) is shown again.
/// Each test keeps its own (its schema is its own; the accounts and
/// audiences are the same): one per thread, as each `#[tokio::test]` runs
/// on a runtime of one thread; a test on more threads would share it.
#[cfg(not(test))]
static WRITTEN_HIDDEN: LazyLock<Mutex<HashSet<(i32, String)>>> = LazyLock::new(Default::default);
#[cfg(test)]
thread_local! {
    static WRITTEN_HIDDEN: std::cell::RefCell<HashSet<(i32, String)>> = Default::default();
}

fn written<R>(f: impl FnOnce(&mut HashSet<(i32, String)>) -> R) -> Option<R> {
    #[cfg(not(test))]
    {
        WRITTEN_HIDDEN
            .lock()
            .ok()
            .map(|mut written| f(&mut written))
    }
    #[cfg(test)]
    {
        Some(WRITTEN_HIDDEN.with(|written| f(&mut written.borrow_mut())))
    }
}

/// Whether `user_id`'s `audience` was last written hidden ([`folded`]).
pub(super) fn written_hidden(user_id: i32, audience: &str) -> bool {
    written(|written| written.contains(&(user_id, audience.to_string()))).unwrap_or(false)
}

fn mark_written(user_id: i32, audience: &str, hidden: bool) {
    let key = (user_id, audience.to_string());
    written(|written| {
        if hidden {
            written.insert(key);
        } else {
            written.remove(&key);
        }
    });
}

/// `folded` (`user_id`'s `audience`) as it reads with what rests on a
/// memory they took back hidden: folded again from the rows hidden, and a
/// topic of hers that held what goes and holds nothing now gone, as
/// forgetting leaves it ([`super::forget`]). None when nothing it names was
/// taken back: it stands as it is. `writing` when the heads are written
/// with it ([`super::heads::refresh`]): kept as how they were last written.
pub(super) async fn folded<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
    folded: &Folded,
    writing: bool,
) -> Result<Option<Folded>, Error> {
    let mut ids = BTreeSet::new();
    for node in &folded.nodes {
        let value = serde_json::to_value(node)
            .map_err(|_| Error::Refused("a folded node does not serialize".into()))?;
        named_in(&value, &mut ids);
    }
    let gone = taken_back(db, user_id, ids).await?;
    if gone.is_empty() {
        if writing {
            mark_written(user_id, audience, false);
        }
        return Ok(None);
    }
    let kept = super::heads::rows_of(db, user_id, audience).await?;
    let doomed = going(&kept, &gone);
    let mut holders: BTreeSet<String> = kept
        .iter()
        .filter(|kept| doomed.contains(&kept.row.id))
        .map(|kept| forget::row_node(&kept.row))
        .collect();
    let mut hidden = super::heads::fold_rows(&stubbed(kept, &doomed))?;
    for node in &hidden.nodes {
        if holders.contains(&node.id)
            && let Some(into) = &node.absorbed_into
        {
            holders.insert(into.into.clone());
            holders.insert(myriad_nodes::node_id(audience, &into.into).unwrap_or_default());
        }
    }
    for node in &mut hidden.nodes {
        if holders.contains(&node.id) && forget::empty_topic(node) {
            node.forgotten = true;
        }
    }
    if writing {
        mark_written(user_id, audience, true);
    }
    Ok(Some(hidden))
}

/// Writes `user_id`'s `audience`'s heads again, as they read now: hidden
/// where it rests on a memory they took back, shown again where that came
/// back. Under the audience's lock, on `db`'s transaction.
pub(super) async fn rewrite<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
) -> Result<(), Error> {
    super::write::lock(db, user_id, audience).await?;
    let folded = super::heads::fold_rows(&super::heads::rows_of(db, user_id, audience).await?)?;
    super::heads::refresh(db, user_id, audience, &folded).await?;
    Ok(())
}

/// The accounts and audiences with a row of her nodes resting on any of
/// `memories`, or of the lines `lines` that went with them: an entry or span
/// naming one, a version or an intent resting on one.
pub(super) async fn citing<C: ConnectionTrait>(
    db: &C,
    memories: &[String],
    lines: &HashSet<i32>,
) -> Result<Vec<(i32, String)>, Error> {
    let refs: Vec<String> = memories
        .iter()
        .map(|id| format!("{MEMORY_REF}{id}"))
        .chain(lines.iter().map(|id| format!("{LINE_REF}{id}")))
        .collect();
    if refs.is_empty() {
        return Ok(Vec::new());
    }
    db.query_all_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT DISTINCT r.user_id, r.audience FROM agent_node_rows r \
         CROSS JOIN LATERAL jsonb_array_elements_text( \
           CASE WHEN r.body = '' THEN '[]'::jsonb \
                ELSE COALESCE((r.body::jsonb)->(CASE \
                  WHEN r.kind IN ('node.view_changed', 'node.open_item', 'node.standing', \
                                  'node.intent') THEN 'because' \
                  ELSE 'refs' END), '[]'::jsonb) END) AS cited(ref) \
         WHERE r.body <> '' \
           AND r.kind IN ('node.entry', 'node.span', 'node.view_changed', 'node.open_item', \
                          'node.standing', 'node.intent') \
           AND cited.ref = ANY($1) \
         ORDER BY 1, 2",
        [refs.into()],
    ))
    .await?
    .iter()
    .map(|row| Ok((row.try_get("", "user_id")?, row.try_get("", "audience")?)))
    .collect()
}

/// What waits in `user_id`'s `audience`, oldest first, at most `limit`,
/// none resting on a memory they took back ([`inbox`]): one hidden does not
/// hold back those after it.
pub(crate) async fn inbox_waiting<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
    limit: u64,
) -> Result<Vec<super::inbox::Item>, Error> {
    let mut shown = Vec::new();
    let mut skipped = 0;
    loop {
        let page = super::inbox::waiting_from(db, user_id, audience, limit, skipped).await?;
        let read = page.len() as u64;
        shown.extend(inbox(db, user_id, page).await?);
        if shown.len() as u64 >= limit || read < limit {
            break;
        }
        skipped += read;
    }
    shown.truncate(limit as usize);
    Ok(shown)
}

/// Of the inbox items `items` (`user_id`'s), those resting on nothing they
/// took back, nor on lines not judged yet ([`not_judged`]).
pub(crate) async fn inbox<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    items: Vec<super::inbox::Item>,
) -> Result<Vec<super::inbox::Item>, Error> {
    let ids: BTreeSet<String> = items
        .iter()
        .flat_map(|item| item.refs.iter())
        .filter(|reference| reference.starts_with(MEMORY_REF) || reference.starts_with(LINE_REF))
        .cloned()
        .collect();
    let mut gone = taken_back(db, user_id, ids.clone()).await?;
    gone.extend(not_judged(db, user_id, &ids).await?);
    Ok(items
        .into_iter()
        .filter(|item| !item.refs.iter().any(|reference| gone.contains(reference)))
        .collect())
}
