//! The nodes of an audience as placing, going over a stretch and sorting
//! see them: the live ones (not gone into another, forgotten or taken
//! back), each with its names and a little of what is on it, most recently
//! added to first. What is on a node is shown only while its memory is: an
//! entry whose memory was replaced, faded or let go is hidden, never shown
//! as if it still held.

use std::collections::{BTreeSet, HashMap, HashSet};

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use serde_json::Value;

use super::{Error, read};

/// Entries shown of a node, newest first, at most.
const HOLDS_SHOWN: usize = 3;
/// Steps followed from a node gone into another to where it went.
const LANDING_STEPS: usize = 8;

/// A live node, as shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Known {
    pub id: String,
    pub key: Option<String>,
    /// `topic`, `task` or `person`.
    pub kind: Option<String>,
    pub name: String,
    pub aliases: Vec<String>,
    /// What is on it, newest first, at most [`HOLDS_SHOWN`].
    pub holds: Vec<String>,
    /// Its newest entry or span, Unix microseconds.
    latest: i64,
}

impl Known {
    pub(crate) fn is_topic(&self) -> bool {
        self.kind.as_deref() == Some("topic")
    }

    /// A person's own node (「关于你」, a group, her): never where a stretch
    /// or a kind of thing goes.
    pub(crate) fn is_person(&self) -> bool {
        self.kind.as_deref() == Some("person")
    }

    /// Its newest entry or span (else when it was made), Unix microseconds.
    pub(crate) fn latest(&self) -> i64 {
        self.latest
    }

    pub(crate) fn topic(&self) -> myriad_merope::topics::Topic<'_> {
        myriad_merope::topics::Topic {
            name: &self.name,
            aliases: &self.aliases,
            holds: &self.holds,
        }
    }
}

/// Whether a node of this status is still there.
pub(crate) fn is_live(status: &str) -> bool {
    !matches!(status, "absorbed" | "forgotten" | "retracted" | "dormant")
}

fn memory_of(entry: &Value) -> Vec<String> {
    entry["refs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|reference| {
            reference
                .as_str()?
                .strip_prefix(myriad_nodes::body::MEMORY_REF)
        })
        .map(str::to_string)
        .collect()
}

/// The memory rows among `ids` still held: not replaced, faded or let go.
pub(crate) async fn held<C: ConnectionTrait>(
    db: &C,
    ids: Vec<String>,
) -> Result<HashSet<String>, Error> {
    if ids.is_empty() {
        return Ok(HashSet::new());
    }
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id FROM agent_memories \
             WHERE id = ANY($1) AND invalid_at IS NULL AND superseded_by IS NULL",
            [ids.into()],
        ))
        .await?;
    rows.iter()
        .map(|row| Ok(row.try_get::<String>("", "id")?))
        .collect()
}

/// One memory row a head names, as it is now.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Named {
    /// Still held: not replaced, faded or let go ([`held`]).
    pub held: bool,
    /// They took it back or had her let it go: deleted, or put aside with
    /// nothing in its place. One a later fact replaced is not (the same
    /// line `chat_lines` draws for their words).
    pub taken_back: bool,
    /// A later fact replaced it: corrected, it changed (`superseded` with
    /// what replaced it named). Not one gone over into another
    /// (`settled`), nor one taken back.
    pub replaced: bool,
    /// The day she learned it, Unix microseconds ([`learned`]): held or
    /// not, that is when it was said.
    pub learned: i64,
    /// Whether what it is, is known: the row is here, or it was forgotten
    /// (the mark forgetting keeps, `memory::forgotten`). A row not here
    /// with no such mark may only not have come from another end yet: it
    /// counts as taken back, so nothing resting on it is shown, but nothing
    /// is forgotten for it.
    pub sure: bool,
    /// Whether what rests on it may be forgotten on her nodes: it was
    /// deleted, forgotten (the mark forgetting keeps), or taken back and
    /// the second look agreed ([`super::take_back_check`]). One only judged
    /// taken back is hidden ([`Self::taken_back`]), never forgotten.
    pub forgets: bool,
}

/// How far a memory row is followed to what replaced it, and what replaced
/// that, to find whether the last of them was taken back: well past how
/// far forgetting follows a line (`memory::manage`'s 32), so a long line of
/// corrections is not cut short into one only replaced.
const TAKEN_BACK_STEPS: i32 = 64;

/// The one place it is said which memory rows were taken back, as SQL: two
/// common table expressions (`WITH RECURSIVE` first) ending in
/// `taken_back(id, sure)`, over the rows of `agent_memories` alias `s` that
/// `seeds` picks. A row is taken back when it was deleted, or put aside
/// with nothing in its place, or replaced (or gone over into, `settled`)
/// by one taken back, as far as the line goes, among the same person's
/// rows: 「当我没说」 of 「改到周六」 takes back 「周五」 too, not only the
/// correction. A line whose next row is not here (forgotten on another
/// end, or not come yet) is taken back too; `sure` only when forgetting
/// kept that row's mark (`memory::forgotten`), as for a row not here
/// ([`Named::sure`]). `forgets` when what rests on it may be forgotten
/// ([`Named::forgets`]): deleted, forgotten, or taken back and confirmed.
pub(crate) fn taken_back_sql(seeds: &str) -> String {
    format!(
        "taken_back_line(start, id, user_id, superseded_by, reason, depth) AS ( \
           SELECT s.id, s.id, s.user_id, s.superseded_by, s.invalid_reason, 0 \
           FROM agent_memories s WHERE {seeds} \
           UNION ALL \
           SELECT l.start, n.id, n.user_id, n.superseded_by, n.invalid_reason, l.depth + 1 \
           FROM taken_back_line l JOIN agent_memories n ON n.id = l.superseded_by \
             AND n.user_id IS NOT DISTINCT FROM l.user_id \
           WHERE l.depth < {TAKEN_BACK_STEPS}), \
         taken_back(id, sure, forgets) AS ( \
           SELECT start, bool_or(sure), bool_or(forgets) FROM ( \
             SELECT start, true AS sure, \
               reason = 'deleted' OR EXISTS (SELECT 1 FROM agent_takeback_check c \
                 WHERE c.memory_id = taken_back_line.id AND c.state = 'confirmed') AS forgets \
             FROM taken_back_line \
             WHERE reason = 'deleted' OR (reason = 'superseded' AND superseded_by IS NULL) \
             UNION ALL \
             SELECT l.start, f.forgotten, f.forgotten FROM taken_back_line l \
             CROSS JOIN LATERAL (SELECT EXISTS (SELECT 1 FROM agent_forgotten f \
                                 WHERE f.what = 'memory' AND f.mark = l.superseded_by) \
                                 AS forgotten) f \
             WHERE l.superseded_by IS NOT NULL AND NOT EXISTS ( \
               SELECT 1 FROM agent_memories n WHERE n.id = l.superseded_by \
                 AND n.user_id IS NOT DISTINCT FROM l.user_id)) gone \
           GROUP BY start)"
    )
}

/// Each of the memory rows `ids` as it is now, in one read: whether it is
/// held, whether it was taken back, and the day she learned it. One not
/// here is not held and is taken back: forgotten, if forgetting kept its
/// mark ([`Named::sure`]), or not come yet.
pub(crate) async fn named<C: ConnectionTrait>(
    db: &C,
    ids: Vec<String>,
) -> Result<HashMap<String, Named>, Error> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            format!(
                "WITH RECURSIVE {} \
                 SELECT h.id, (h.invalid_at IS NULL AND h.superseded_by IS NULL) AS held, \
                   (h.invalid_reason IS NOT DISTINCT FROM 'superseded' \
                     AND h.superseded_by IS NOT NULL) AS replaced, \
                   h.id IN (SELECT id FROM taken_back) AS taken_back, \
                   COALESCE((SELECT sure FROM taken_back t WHERE t.id = h.id), true) AS sure, \
                   COALESCE((SELECT forgets FROM taken_back t WHERE t.id = h.id), false) \
                     AS forgets, \
                   LEAST(h.created_at, min(n.created_at)) AS at \
                 FROM agent_memories h \
                 LEFT JOIN agent_memories n \
                   ON n.superseded_by = h.id AND n.invalid_reason = 'settled' \
                 WHERE h.id = ANY($1) \
                 GROUP BY h.id, h.created_at, h.invalid_at, h.superseded_by",
                taken_back_sql("s.id = ANY($1)")
            ),
            [ids.clone().into()],
        ))
        .await?;
    let mut named: HashMap<String, Named> = rows
        .iter()
        .map(|row| {
            let at: chrono::DateTime<chrono::FixedOffset> = row.try_get("", "at")?;
            Ok((
                row.try_get::<String>("", "id")?,
                Named {
                    held: row.try_get("", "held")?,
                    taken_back: row.try_get("", "taken_back")?,
                    replaced: row.try_get("", "replaced")?,
                    learned: at.timestamp_micros(),
                    sure: row.try_get("", "sure")?,
                    forgets: row.try_get("", "forgets")?,
                },
            ))
        })
        .collect::<Result<_, Error>>()?;
    let missing: Vec<String> = ids
        .iter()
        .filter(|id| !named.contains_key(*id))
        .cloned()
        .collect();
    if !missing.is_empty() {
        let forgotten: HashSet<String> = db
            .query_all_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT DISTINCT mark FROM agent_forgotten WHERE what = $1 AND mark = ANY($2)",
                [
                    crate::services::agent::memory::forgotten::MEMORY.into(),
                    missing.clone().into(),
                ],
            ))
            .await?
            .iter()
            .map(|row| row.try_get::<String>("", "mark"))
            .collect::<Result<_, _>>()?;
        for id in missing {
            let sure = forgotten.contains(&id);
            named.insert(
                id,
                Named {
                    held: false,
                    taken_back: true,
                    replaced: false,
                    learned: 0,
                    sure,
                    forgets: sure,
                },
            );
        }
    }
    Ok(named)
}

/// The day she learned each of the memory rows `ids`, Unix microseconds:
/// when it was kept, or when the first of the short notes settled into it
/// was taken ([`crate::services::agent::merope::chat_remember`]), which is
/// when it was said. Not when it was put on a node: an old memory put on
/// one today was still learned then.
pub(crate) async fn learned<C: ConnectionTrait>(
    db: &C,
    ids: Vec<String>,
) -> Result<HashMap<String, i64>, Error> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT h.id, LEAST(h.created_at, min(n.created_at)) AS at \
             FROM agent_memories h \
             LEFT JOIN agent_memories n ON n.superseded_by = h.id AND n.invalid_reason = 'settled' \
             WHERE h.id = ANY($1) GROUP BY h.id, h.created_at",
            [ids.into()],
        ))
        .await?;
    rows.iter()
        .map(|row| {
            let at: chrono::DateTime<chrono::FixedOffset> = row.try_get("", "at")?;
            Ok((row.try_get::<String>("", "id")?, at.timestamp_micros()))
        })
        .collect()
}

/// The live nodes of `user_id`'s `audience`, most recently added to first.
pub(crate) async fn known<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
) -> Result<Vec<Known>, Error> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT node_id, node_key, kind, name, aliases, status, node FROM agent_node_heads \
             WHERE user_id = $1 AND audience = $2 ORDER BY node_id",
            [user_id.into(), audience.into()],
        ))
        .await?;
    let mut nodes = Vec::new();
    for row in &rows {
        let status: String = row.try_get("", "status")?;
        let Some(name) = row.try_get::<Option<String>>("", "name")? else {
            continue;
        };
        if !is_live(&status) {
            continue;
        }
        let node: Value = row.try_get("", "node")?;
        let aliases: Value = row.try_get("", "aliases")?;
        nodes.push((
            row.try_get::<String>("", "node_id")?,
            row.try_get::<Option<String>>("", "node_key")?,
            row.try_get::<Option<String>>("", "kind")?,
            name,
            serde_json::from_value::<Vec<String>>(aliases).unwrap_or_default(),
            node,
        ));
    }
    let named: BTreeSet<String> = nodes
        .iter()
        .flat_map(|node| node.5["entries"].as_array().into_iter().flatten())
        .flat_map(memory_of)
        .collect();
    let held = held(db, named.into_iter().collect()).await?;
    let mut out: Vec<Known> = nodes
        .into_iter()
        .map(|(id, key, kind, name, aliases, node)| {
            let mut entries: Vec<&Value> = node["entries"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|entry| {
                    let memories = memory_of(entry);
                    memories.is_empty() || memories.iter().any(|memory| held.contains(memory))
                })
                .collect();
            entries.sort_by_key(|entry| std::cmp::Reverse(entry["at"].as_i64().unwrap_or(0)));
            let spans = node["spans"].as_array().into_iter().flatten();
            let latest = entries
                .iter()
                .copied()
                .chain(spans)
                .filter_map(|item| item["at"].as_i64())
                .chain(node["createdAt"].as_i64())
                .max()
                .unwrap_or(0);
            Known {
                id,
                key,
                kind,
                name,
                aliases,
                holds: entries
                    .iter()
                    .take(HOLDS_SHOWN)
                    .filter_map(|entry| entry["text"].as_str().map(str::to_string))
                    .collect(),
                latest,
            }
        })
        .collect();
    out.sort_by(|a, b| b.latest.cmp(&a.latest).then_with(|| a.id.cmp(&b.id)));
    Ok(out)
}

/// Where `node` of `user_id`'s `audience` is now: itself, or the node it
/// went into (the last one, when that one went on); none when it is gone.
pub(crate) async fn landing<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
    node: &str,
) -> Result<Option<String>, Error> {
    let mut at = node.to_string();
    for _ in 0..LANDING_STEPS {
        let Some(head) = read::node(db, user_id, audience, &at).await? else {
            return Ok(None);
        };
        let Some(into) = head["absorbedInto"]["into"].as_str() else {
            let gone = head["forgotten"].as_bool().unwrap_or(false)
                || head["retracted"]
                    .as_array()
                    .is_some_and(|retracted| !retracted.is_empty());
            return Ok((!gone).then_some(at));
        };
        at = if into.len() == 32 && into.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            into.to_string()
        } else {
            myriad_nodes::node_id(audience, into)?
        };
    }
    Ok(None)
}
