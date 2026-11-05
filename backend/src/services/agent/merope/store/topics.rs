//! The topics she keeps what she knows of someone under
//! (`myriad_merope::topics`, `migrations/topic_nodes.sql`): a kind of thing in
//! their life they come back to. Going over a stretch of talk she is shown
//! the likely ones and puts each fact she keeps under one, starts one, or
//! leaves it under none; what she knew before she kept topics, or kept
//! without one, she sorts the same way at upkeep (`chat_remember::sort`).
//! Going through all of something, what she kept under the same topic comes
//! to mind together.
//!
//! Only a private talk's facts go under topics for now: a group's and
//! anyone else's stay as they were. The fact stays on its memory row; a
//! topic holds only its names and the memories that point at it, so
//! forgetting a memory leaves nothing of it here.

use sea_orm::{ConnectionTrait, DatabaseBackend, DbErr, Statement};
use serde::{Deserialize, Serialize};

use crate::services::agent::memory::unified::{self, Audience, MemoryKind, MemoryRecord};

/// The node audience a private talk's topics are made in. On the site an
/// account has her to itself, so its private talk is her partner's, which
/// the shared core names one fixed way (`myriad_persona_log::partner::LEGACY`).
const PRIVATE_TALK: &str = "personal:local-owner";
/// Facts under a topic she is shown with it, newest first.
const HOLDS_SHOWN: usize = 3;
/// What she knows for good of someone, which she sorts under topics.
const SORTED_SOURCES: &[&str] = &["chat", "import"];

/// Where a fact she keeps goes, as going over a stretch decided.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GoesUnder {
    /// A topic she keeps, by its id.
    Topic(String),
    /// One she starts.
    New { name: String, aliases: Vec<String> },
    /// None: of no kind. Kept, so it is not asked of again.
    Nowhere,
    /// Only about them as a person: under no topic here either (kept, so it
    /// is not asked of again); on her nodes, on their own (「关于你」).
    AboutThem,
}

/// A topic offered beside a stretch of talk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfferedTopic {
    pub id: String,
    pub name: String,
    pub aliases: Vec<String>,
    /// Facts under it, newest first, as she would recall them.
    pub holds: Vec<String>,
}

struct Kept {
    id: String,
    name: String,
    aliases: Vec<String>,
}

fn admitted(present: &Audience) -> bool {
    !present.is_group()
}

async fn topics<C: ConnectionTrait>(db: &C, user_id: i32) -> Result<Vec<Kept>, DbErr> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id, name, aliases FROM agent_topic_nodes \
             WHERE user_id = $1 AND audience = $2 ORDER BY created_at, id",
            [user_id.into(), PRIVATE_TALK.into()],
        ))
        .await?;
    Ok(rows
        .iter()
        .filter_map(|row| {
            Some(Kept {
                id: row.try_get("", "id").ok()?,
                name: row.try_get("", "name").ok()?,
                aliases: row
                    .try_get::<serde_json::Value>("", "aliases")
                    .ok()
                    .and_then(|aliases| serde_json::from_value(aliases).ok())
                    .unwrap_or_default(),
            })
        })
        .collect())
}

/// Which topic each of their memories she has sorted goes under, by memory
/// id; `None` for under none.
async fn pointers<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
) -> Result<std::collections::HashMap<String, Option<String>>, DbErr> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT memory_id, topic_id FROM agent_memory_topics WHERE user_id = $1",
            [user_id.into()],
        ))
        .await?;
    Ok(rows
        .iter()
        .filter_map(|row| {
            Some((
                row.try_get("", "memory_id").ok()?,
                row.try_get::<Option<String>>("", "topic_id").ok()?,
            ))
        })
        .collect())
}

/// What may be said here of them that goes under a topic, newest first per
/// topic, by topic id.
async fn under_each<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    present: &Audience,
) -> Result<std::collections::HashMap<String, Vec<MemoryRecord>>, DbErr> {
    let pointers = pointers(db, user_id).await?;
    let mut rows = unified::active_in(db, user_id, present, &MemoryKind::ABOUT_PERSON).await?;
    rows.sort_by_key(|row| std::cmp::Reverse(row.created_at));
    let mut under: std::collections::HashMap<String, Vec<MemoryRecord>> =
        std::collections::HashMap::new();
    for row in rows {
        if let Some(Some(topic)) = pointers.get(&row.id) {
            under.entry(topic.clone()).or_default().push(row);
        }
    }
    Ok(under)
}

/// The topics to show beside a stretch of `talk` with them, most likely
/// first (`myriad_merope::topics::offered`); the host's order behind that
/// is the most recently added to first. None outside a private talk.
pub async fn offered_topics<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    present: &Audience,
    talk: &str,
) -> Result<Vec<OfferedTopic>, DbErr> {
    if user_id <= 0 || !admitted(present) {
        return Ok(Vec::new());
    }
    let under = under_each(db, user_id, present).await?;
    let mut kept: Vec<OfferedTopic> = topics(db, user_id)
        .await?
        .into_iter()
        .map(|topic| {
            let holds = under
                .get(&topic.id)
                .into_iter()
                .flatten()
                .take(HOLDS_SHOWN)
                .map(super::as_known)
                .collect();
            OfferedTopic {
                id: topic.id,
                name: topic.name,
                aliases: topic.aliases,
                holds,
            }
        })
        .collect();
    let latest = |topic: &OfferedTopic| {
        under
            .get(&topic.id)
            .and_then(|rows| rows.first())
            .map(|row| row.created_at)
    };
    kept.sort_by_key(|topic| std::cmp::Reverse(latest(topic)));
    let seen: Vec<myriad_merope::topics::Topic> = kept
        .iter()
        .map(|topic| myriad_merope::topics::Topic {
            name: &topic.name,
            aliases: &topic.aliases,
            holds: &topic.holds,
        })
        .collect();
    let order = myriad_merope::topics::offered(talk, &seen);
    Ok(order.into_iter().map(|index| kept[index].clone()).collect())
}

/// Puts memory `memory_id` under a topic, once: under the one a memory it
/// replaces was under, whatever going over decided (it says how that same
/// thing is now); else where going over or sorting put it. A topic she
/// starts under a name she already keeps one by is that one. Nothing
/// outside a private talk, for a memory already sorted, or when where it
/// goes was not decided (it stays to be sorted).
pub async fn put_under<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    present: &Audience,
    memory_id: &str,
    replaced: &[String],
    goes: Option<&GoesUnder>,
) -> Result<(), DbErr> {
    if user_id <= 0 || !admitted(present) {
        return Ok(());
    }
    let pointers = pointers(db, user_id).await?;
    if pointers.contains_key(memory_id) {
        return Ok(());
    }
    let kept = topics(db, user_id).await?;
    let topic: Option<String> = match replaced
        .iter()
        .find_map(|old| pointers.get(old).cloned().flatten())
    {
        Some(topic) => Some(topic),
        None => match goes {
            Some(GoesUnder::Topic(id)) => {
                // Gone since she was shown it: still to be sorted.
                if !kept.iter().any(|topic| &topic.id == id) {
                    return Ok(());
                }
                Some(id.clone())
            }
            Some(GoesUnder::New { name, aliases }) => {
                match kept
                    .iter()
                    .find(|topic| myriad_merope::topics::same_name(name, &topic.name))
                {
                    Some(topic) => Some(topic.id.clone()),
                    None => Some(start(db, user_id, name, aliases).await?),
                }
            }
            Some(GoesUnder::Nowhere | GoesUnder::AboutThem) => None,
            None => return Ok(()),
        },
    };
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_memory_topics (memory_id, user_id, topic_id) VALUES ($1, $2, $3) \
         ON CONFLICT (memory_id) DO NOTHING",
        [memory_id.into(), user_id.into(), topic.into()],
    ))
    .await?;
    Ok(())
}

/// A topic she starts: a topic node with a minted key, its id as the shared
/// core makes it, so it stays the same once her log carries it.
async fn start<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    name: &str,
    aliases: &[String],
) -> Result<String, DbErr> {
    let key = format!("minted:{}", uuid::Uuid::new_v4().simple());
    let id = myriad_nodes::node_id(PRIVATE_TALK, &key)
        .map_err(|error| DbErr::Custom(format!("topic id: {error}")))?;
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_topic_nodes (id, user_id, audience, node_key, name, aliases, created_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
        [
            id.clone().into(),
            user_id.into(),
            PRIVATE_TALK.into(),
            key.into(),
            name.into(),
            serde_json::json!(aliases).into(),
            crate::clock::now().fixed_offset().into(),
        ],
    ))
    .await?;
    Ok(id)
}

/// What going through all of something adds to what came to mind: the rest
/// of what she keeps under the topics of the memories `named` (ids, most
/// sure first), topic by topic in that order, at most `at_most` lines, none
/// already in `have`; and each topic whose facts are not all there, with how
/// many are and how many she keeps.
pub async fn kept_together<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    present: &Audience,
    named: &[String],
    have: &[String],
    at_most: usize,
) -> Result<Together, DbErr> {
    if user_id <= 0 || !admitted(present) || named.is_empty() {
        return Ok(Together::default());
    }
    let pointers = pointers(db, user_id).await?;
    let mut order: Vec<&String> = Vec::new();
    for id in named {
        if let Some(Some(topic)) = pointers.get(id)
            && !order.contains(&topic)
        {
            order.push(topic);
        }
    }
    if order.is_empty() {
        return Ok(Together::default());
    }
    let names: std::collections::HashMap<String, String> = topics(db, user_id)
        .await?
        .into_iter()
        .map(|topic| (topic.id, topic.name))
        .collect();
    let under = under_each(db, user_id, present).await?;
    let mut added: Vec<String> = Vec::new();
    let mut said = std::collections::HashMap::new();
    let mut cut = Vec::new();
    for topic in order {
        let rows: Vec<(String, &MemoryRecord)> = under
            .get(topic)
            .into_iter()
            .flatten()
            .map(|row| (super::as_known(row), row))
            .filter(|(line, _)| !line.is_empty())
            .collect();
        let lines: Vec<String> = rows.iter().map(|(line, _)| line.clone()).collect();
        for (line, row) in &rows {
            if added.len() < at_most && !have.contains(line) && !added.contains(line) {
                added.push(line.clone());
                if let Some(words) = super::their_words(row) {
                    said.insert(line.clone(), words);
                }
            }
        }
        let given = lines
            .iter()
            .filter(|line| have.contains(line) || added.contains(line))
            .count();
        if let Some(name) = names.get(topic) {
            cut.push((name.clone(), given, lines.len()));
        }
    }
    Ok(Together {
        more: added,
        said,
        cut,
    })
}

/// What going through all of something adds ([`kept_together`]).
#[derive(Debug, Default)]
pub struct Together {
    /// The rest of what she keeps under the same topics.
    pub more: Vec<String>,
    /// Their words behind each of `more`, where there are any.
    pub said: std::collections::HashMap<String, String>,
    /// Each topic whose facts are not all there: its name, how many are, and
    /// how many she keeps.
    pub cut: Vec<(String, usize, usize)>,
}

/// People with facts still to sort, at most `at_most` of them.
pub async fn with_unsorted<C: ConnectionTrait>(db: &C, at_most: i64) -> Result<Vec<i32>, DbErr> {
    Ok(db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT DISTINCT m.user_id FROM agent_memories m \
             WHERE m.user_id IS NOT NULL AND m.source = ANY($1) AND m.invalid_at IS NULL \
               AND m.superseded_by IS NULL AND m.venue NOT LIKE 'group:%' \
               AND m.kind IN ('fact', 'preference') \
               AND NOT EXISTS (SELECT 1 FROM agent_memory_topics t WHERE t.memory_id = m.id) \
             LIMIT $2",
            [
                SORTED_SOURCES
                    .iter()
                    .map(|source| source.to_string())
                    .collect::<Vec<_>>()
                    .into(),
                at_most.into(),
            ],
        ))
        .await?
        .iter()
        .filter_map(|row| row.try_get::<i32>("", "user_id").ok())
        .collect())
}

/// What she knows for good of them in a private talk that she has not
/// sorted under a topic yet, oldest first, at most `at_most`. None outside
/// a private talk.
pub async fn unsorted<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    present: &Audience,
    at_most: usize,
) -> Result<Vec<MemoryRecord>, DbErr> {
    if user_id <= 0 || !admitted(present) {
        return Ok(Vec::new());
    }
    let pointers = pointers(db, user_id).await?;
    let mut rows = unified::active_in(db, user_id, present, &MemoryKind::ABOUT_PERSON).await?;
    rows.retain(|row| {
        SORTED_SOURCES.contains(&row.source.as_str()) && !pointers.contains_key(&row.id)
    });
    rows.sort_by_key(|row| row.created_at);
    rows.truncate(at_most);
    Ok(rows)
}

/// Other names a topic keeps, at most, once others have gone into it.
const KEPT_ALIASES: usize = 10;

/// A topic to tidy: as she is shown it, and when it last changed.
#[derive(Debug, Clone)]
pub struct DueTopic {
    pub topic: OfferedTopic,
    pub changed: chrono::DateTime<chrono::FixedOffset>,
}

/// People whose topics are to tidy, at most `at_most` of them, longest
/// waiting first: at least `ask_at` topics holding few facts
/// (`myriad_merope::topics::SMALL`) changed since they were last tidied (or
/// never were), or one of them changed `waited` ago or longer.
pub async fn with_untidied<C: ConnectionTrait>(
    db: &C,
    at_most: i64,
    ask_at: usize,
    waited: chrono::Duration,
) -> Result<Vec<i32>, DbErr> {
    Ok(db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT user_id FROM ( \
                SELECT t.user_id, MAX(m.linked_at) AS changed FROM agent_topic_nodes t \
                JOIN agent_memory_topics m ON m.user_id = t.user_id AND m.topic_id = t.id \
                GROUP BY t.user_id, t.id, t.tidied_at \
                HAVING COUNT(*) <= $1 AND (t.tidied_at IS NULL OR MAX(m.linked_at) > t.tidied_at) \
             ) due GROUP BY user_id \
             HAVING COUNT(*) >= $3 OR MIN(changed) <= now() - $4::float8 * interval '1 second' \
             ORDER BY MIN(changed) LIMIT $2",
            [
                (myriad_merope::topics::SMALL as i64).into(),
                at_most.into(),
                (ask_at as i64).into(),
                (waited.num_seconds() as f64).into(),
            ],
        ))
        .await?
        .iter()
        .filter_map(|row| row.try_get::<i32>("", "user_id").ok())
        .collect())
}

/// Their topics to tidy, longest waiting first, and the others: a topic
/// holding few of what may be said here of them, changed since it was last
/// tidied. None outside a private talk. One that changed but holds nothing
/// that may be said here (all of it faded) is marked tidied, so it is not
/// waited on.
pub async fn untidied<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    present: &Audience,
) -> Result<(Vec<DueTopic>, Vec<OfferedTopic>), DbErr> {
    if user_id <= 0 || !admitted(present) {
        return Ok((Vec::new(), Vec::new()));
    }
    let mut vacant = Vec::new();
    let under = under_each(db, user_id, present).await?;
    let marks: std::collections::HashMap<
        String,
        (
            Option<chrono::DateTime<chrono::FixedOffset>>,
            Option<chrono::DateTime<chrono::FixedOffset>>,
        ),
    > = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT t.id, t.tidied_at, MAX(m.linked_at) AS changed FROM agent_topic_nodes t \
             LEFT JOIN agent_memory_topics m ON m.user_id = t.user_id AND m.topic_id = t.id \
             WHERE t.user_id = $1 GROUP BY t.id, t.tidied_at",
            [user_id.into()],
        ))
        .await?
        .iter()
        .filter_map(|row| {
            Some((
                row.try_get::<String>("", "id").ok()?,
                (
                    row.try_get("", "tidied_at").ok()?,
                    row.try_get("", "changed").ok()?,
                ),
            ))
        })
        .collect();
    let mut due = Vec::new();
    let mut others = Vec::new();
    for topic in topics(db, user_id).await? {
        let held = under.get(&topic.id).map(Vec::len).unwrap_or(0);
        let (tidied, changed) = marks.get(&topic.id).cloned().unwrap_or((None, None));
        // What is under it has all faded or gone: nothing to show of it.
        if held == 0 {
            if changed.is_some_and(|changed| tidied.is_none_or(|tidied| changed > tidied)) {
                vacant.push(topic.id);
            }
            continue;
        }
        let holds = under
            .get(&topic.id)
            .into_iter()
            .flatten()
            .take(HOLDS_SHOWN)
            .map(super::as_known)
            .collect();
        let shown = OfferedTopic {
            id: topic.id.clone(),
            name: topic.name,
            aliases: topic.aliases,
            holds,
        };
        match changed {
            Some(changed)
                if held <= myriad_merope::topics::SMALL
                    && tidied.is_none_or(|tidied| changed > tidied) =>
            {
                due.push(DueTopic {
                    topic: shown,
                    changed,
                })
            }
            _ => others.push(shown),
        }
    }
    mark_tidied(db, user_id, &vacant).await?;
    due.sort_by_key(|due| due.changed);
    Ok((due, others))
}

async fn mark_tidied<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    ids: &[String],
) -> Result<(), DbErr> {
    if ids.is_empty() {
        return Ok(());
    }
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE agent_topic_nodes SET tidied_at = now() WHERE user_id = $1 AND id = ANY($2)",
        [user_id.into(), ids.to_vec().into()],
    ))
    .await?;
    Ok(())
}

/// Topics she keeps under one name: one whose name is another's name or
/// one of its names goes into the one holding more (the older on a tie).
/// Free, before she is asked anything. How many went.
pub async fn merge_same_names<C: ConnectionTrait>(db: &C, user_id: i32) -> Result<usize, DbErr> {
    let mut merged = 0;
    loop {
        let kept = topics(db, user_id).await?;
        let pointers = pointers(db, user_id).await?;
        let held = |id: &str| {
            pointers
                .values()
                .filter(|topic| topic.as_deref() == Some(id))
                .count()
        };
        let named = |a: &Kept, b: &Kept| {
            myriad_merope::topics::same_name(&a.name, &b.name)
                || b.aliases
                    .iter()
                    .any(|alias| myriad_merope::topics::same_name(&a.name, alias))
        };
        let pair = kept.iter().enumerate().find_map(|(n, one)| {
            kept.iter()
                .skip(n + 1)
                .find(|other| named(one, other) || named(other, one))
                .map(|other| (one, other))
        });
        let Some((one, other)) = pair else {
            return Ok(merged);
        };
        // `kept` is oldest first: the older wins a tie.
        let (from, into) = if held(&other.id) > held(&one.id) {
            (one, other)
        } else {
            (other, one)
        };
        merge(db, user_id, &from.id, &into.id).await?;
        merged += 1;
    }
}

/// What tidying made of `due` topics, `others` shown beside them, written:
/// a topic going into another takes what is under it there and its names
/// with it; one let go leaves what was under it under none. Every due topic
/// still kept, and every one taking another in, is marked tidied: she has
/// just looked at it.
pub async fn apply_tidied<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    due: &[DueTopic],
    others: &[OfferedTopic],
    tidied: &[myriad_merope::topics::Tidied],
) -> Result<(), DbErr> {
    use myriad_merope::topics::{Target, Tidied};
    let kept = topics(db, user_id).await?;
    let find = |id: &str| kept.iter().find(|topic| topic.id == id);
    let mut looked_at: Vec<String> = due.iter().map(|due| due.topic.id.clone()).collect();
    for (at, what) in tidied.iter().enumerate() {
        let Some(from) = due.get(at).and_then(|due| find(&due.topic.id)) else {
            continue;
        };
        match what {
            Tidied::Keep => {}
            Tidied::Into(target) => {
                let into = match target {
                    Target::Due(to) => due.get(*to).map(|due| due.topic.id.as_str()),
                    Target::Other(to) => others.get(*to).map(|other| other.id.as_str()),
                };
                if let Some(into) = into {
                    merge(db, user_id, &from.id, into).await?;
                    looked_at.push(into.to_string());
                }
            }
            Tidied::LetGo => {
                db.execute_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "UPDATE agent_memory_topics SET topic_id = NULL, linked_at = now() \
                     WHERE user_id = $1 AND topic_id = $2",
                    [user_id.into(), from.id.clone().into()],
                ))
                .await?;
                forget_topic(db, user_id, &from.id).await?;
            }
        }
    }
    mark_tidied(db, user_id, &looked_at).await
}

/// Topic `from` into `into`, by id: what is under it, and its names.
async fn merge<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    from: &str,
    into: &str,
) -> Result<(), DbErr> {
    let kept = topics(db, user_id).await?;
    let (Some(from), Some(into)) = (
        kept.iter().find(|topic| topic.id == from),
        kept.iter().find(|topic| topic.id == into),
    ) else {
        return Ok(());
    };
    if from.id == into.id {
        return Ok(());
    }
    let mut aliases = into.aliases.clone();
    for name in std::iter::once(&from.name).chain(&from.aliases) {
        if !myriad_merope::topics::same_name(name, &into.name)
            && !aliases
                .iter()
                .any(|alias| myriad_merope::topics::same_name(alias, name))
        {
            aliases.push(name.clone());
        }
    }
    aliases.truncate(KEPT_ALIASES);
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE agent_topic_nodes SET aliases = $3 WHERE user_id = $1 AND id = $2",
        [
            user_id.into(),
            into.id.clone().into(),
            serde_json::json!(aliases).into(),
        ],
    ))
    .await?;
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE agent_memory_topics SET topic_id = $3, linked_at = now() \
         WHERE user_id = $1 AND topic_id = $2",
        [
            user_id.into(),
            from.id.clone().into(),
            into.id.clone().into(),
        ],
    ))
    .await?;
    forget_topic(db, user_id, &from.id).await
}

async fn forget_topic<C: ConnectionTrait>(db: &C, user_id: i32, id: &str) -> Result<(), DbErr> {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM agent_topic_nodes WHERE user_id = $1 AND id = $2",
        [user_id.into(), id.into()],
    ))
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::super::chat_remember::ChatMemoryUpdate;
    use super::*;
    use sea_orm::TransactionTrait;

    fn fact(text: &str, supersedes: &[&str], topic: Option<GoesUnder>) -> ChatMemoryUpdate {
        ChatMemoryUpdate {
            fact: Some(text.into()),
            supersedes: supersedes.iter().map(|old| old.to_string()).collect(),
            evidence: Some(text.into()),
            concepts: Vec::new(),
            cues: Vec::new(),
            topic,
        }
    }

    async fn keep(db: &sea_orm::DatabaseConnection, user_id: i32, update: ChatMemoryUpdate) {
        let transaction = db.begin().await.unwrap();
        super::super::apply_chat_memory_update_on(
            &transaction,
            user_id,
            crate::clock::now().fixed_offset(),
            &update,
            &Audience::private(user_id),
            super::super::Keep::Lasting,
        )
        .await
        .unwrap();
        transaction.commit().await.unwrap();
    }

    async fn kept(db: &sea_orm::DatabaseConnection, user_id: i32) -> Vec<MemoryRecord> {
        unified::active_in(
            db,
            user_id,
            &Audience::private(user_id),
            &MemoryKind::ABOUT_PERSON,
        )
        .await
        .unwrap()
    }

    fn id_of(rows: &[MemoryRecord], content: &str) -> String {
        rows.iter()
            .find(|row| row.content == content)
            .map(|row| row.id.clone())
            .unwrap()
    }

    async fn count(db: &sea_orm::DatabaseConnection, sql: &str) -> i64 {
        db.query_one_raw(Statement::from_string(DatabaseBackend::Postgres, sql))
            .await
            .unwrap()
            .unwrap()
            .try_get::<i64>("", "n")
            .unwrap()
    }

    /// What she keeps of a kind goes under one topic, what changes stays
    /// where it was, a kind comes to mind together going through all of it,
    /// and a topic none points at any more is gone.
    #[tokio::test]
    async fn a_kind_is_kept_under_one_topic_and_comes_to_mind_together() {
        let Ok(url) = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL") else {
            return;
        };
        let schema = crate::db::IsolatedSchema::migrated(&url, "topic_nodes_store").await;
        let db = &schema.db;
        db.execute_unprepared("INSERT INTO users (id, username) VALUES (7, 'seven')")
            .await
            .unwrap();
        let doctors = GoesUnder::New {
            name: "seeing doctors".into(),
            aliases: vec!["doctor".into()],
        };
        keep(db, 7, fact("sees Dr. Lee about a rash", &[], Some(doctors))).await;
        // Started again under a name she keeps one by: that one.
        let again = GoesUnder::New {
            name: "Seeing  Doctors".into(),
            aliases: Vec::new(),
        };
        keep(
            db,
            7,
            fact("had a check-up with Dr. Patel", &[], Some(again)),
        )
        .await;
        keep(db, 7, fact("lives in Austin", &[], None)).await;
        assert_eq!(
            count(db, "SELECT COUNT(*) AS n FROM agent_topic_nodes").await,
            1
        );
        assert_eq!(
            count(db, "SELECT COUNT(*) AS n FROM agent_memory_topics").await,
            2
        );

        let present = Audience::private(7);
        let offered = offered_topics(db, 7, &present, "Dr. Lee moved his clinic")
            .await
            .unwrap();
        assert_eq!(offered.len(), 1);
        assert_eq!(offered[0].name, "seeing doctors");
        assert_eq!(offered[0].holds.len(), 2);
        let id = offered[0].id.clone();
        assert_eq!(id.len(), 32, "a node id as the shared core makes it");

        // How it is now stays where it was, whatever going over said.
        keep(
            db,
            7,
            fact(
                "sees Dr. Lee about a rash, now cleared",
                &["sees Dr. Lee about a rash"],
                None,
            ),
        )
        .await;
        let rows = kept(db, 7).await;
        let cleared = id_of(&rows, "sees Dr. Lee about a rash, now cleared");
        let patel = id_of(&rows, "had a check-up with Dr. Patel");
        let austin = id_of(&rows, "lives in Austin");
        let pointers = pointers(db, 7).await.unwrap();
        assert_eq!(pointers.get(&cleared), Some(&Some(id.clone())));
        assert_eq!(pointers.get(&austin), None);

        // Going through all of it from Dr. Patel, Dr. Lee comes along.
        let line = |id: &str| super::super::as_known(rows.iter().find(|row| row.id == id).unwrap());
        let have = vec![line(&patel)];
        db.execute_unprepared(&format!(
            "UPDATE agent_memories SET evidence = 'Dr. Lee says the rash is gone' WHERE id = '{cleared}'"
        ))
        .await
        .unwrap();
        let together = kept_together(db, 7, &present, &[patel.clone()], &have, 24)
            .await
            .unwrap();
        assert_eq!(together.more, vec![line(&cleared)]);
        assert_eq!(
            together.said.get(&line(&cleared)).map(String::as_str),
            Some("Dr. Lee says the rash is gone"),
            "what they said comes with it"
        );
        assert_eq!(together.cut, vec![("seeing doctors".to_string(), 2, 2)]);
        let together = kept_together(db, 7, &present, &[patel.clone()], &have, 0)
            .await
            .unwrap();
        assert!(together.more.is_empty() && together.said.is_empty());
        assert_eq!(
            together.cut,
            vec![("seeing doctors".to_string(), 1, 2)],
            "she is told"
        );

        // Not in a group.
        let group = Audience::group("telegram:-1", 7);
        assert!(
            offered_topics(db, 7, &group, "Dr. Lee")
                .await
                .unwrap()
                .is_empty()
        );

        // What going over left without a place is still to be sorted.
        let left: Vec<String> = unsorted(db, 7, &present, 40)
            .await
            .unwrap()
            .into_iter()
            .map(|row| row.content)
            .collect();
        assert_eq!(left, vec!["lives in Austin".to_string()]);
        assert_eq!(with_unsorted(db, 10).await.unwrap(), vec![7]);

        // He has her forget all of it: the topic's name goes with it.
        db.execute_unprepared("DELETE FROM agent_memories WHERE user_id = 7")
            .await
            .unwrap();
        assert_eq!(
            count(db, "SELECT COUNT(*) AS n FROM agent_topic_nodes").await,
            0
        );
    }

    /// What she knew before topics is sorted the same way: under one she
    /// keeps, one she starts, or none, which is kept so it is not asked of
    /// again; a fact her answer gives no place stays to be sorted.
    #[tokio::test]
    async fn what_she_knew_before_topics_is_sorted_once() {
        let Ok(url) = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL") else {
            return;
        };
        let schema = crate::db::IsolatedSchema::migrated(&url, "topic_nodes_sort").await;
        let db = &schema.db;
        db.execute_unprepared("INSERT INTO users (id, username) VALUES (8, 'eight')")
            .await
            .unwrap();
        let plants = GoesUnder::New {
            name: "their plants".into(),
            aliases: Vec::new(),
        };
        keep(db, 8, fact("has a snake plant", &[], Some(plants))).await;
        keep(
            db,
            8,
            fact("cannot eat mango", &[], Some(GoesUnder::Nowhere)),
        )
        .await;
        for old in [
            "bought a fern on May 6",
            "sees Dr. Lee",
            "had a long day",
            "likes tea",
        ] {
            keep(db, 8, fact(old, &[], None)).await;
        }
        let present = Audience::private(8);
        let rows = unsorted(db, 8, &present, 40).await.unwrap();
        let texts: Vec<&str> = rows.iter().map(|row| row.content.as_str()).collect();
        assert_eq!(
            texts,
            vec![
                "bought a fern on May 6",
                "sees Dr. Lee",
                "had a long day",
                "likes tea"
            ],
            "under none is sorted; oldest first"
        );
        let offered = offered_topics(db, 8, &present, "fern").await.unwrap();
        assert_eq!(offered[0].name, "their plants");
        let raw = serde_json::json!({"facts": [
            {"fact": 1, "topic": {"on": "t1", "name": "", "aliases": []}},
            {"fact": 2, "topic": {"on": "", "name": "seeing doctors", "aliases": []}},
            {"fact": 3, "topic": {"on": "", "name": "", "aliases": []}}
        ]})
        .to_string();
        let placed = super::super::super::chat_remember::sort::placed(&raw, &rows, &offered);
        assert_eq!(placed.len(), 3, "the fourth was given no place");
        for (id, goes) in &placed {
            put_under(db, 8, &present, id, &[], Some(goes))
                .await
                .unwrap();
        }
        let left: Vec<String> = unsorted(db, 8, &present, 40)
            .await
            .unwrap()
            .into_iter()
            .map(|row| row.content)
            .collect();
        assert_eq!(left, vec!["likes tea".to_string()]);
        let pointers = pointers(db, 8).await.unwrap();
        let rows = kept(db, 8).await;
        assert_eq!(
            pointers.get(&id_of(&rows, "bought a fern on May 6")),
            pointers.get(&id_of(&rows, "has a snake plant"))
        );
        assert_eq!(pointers.get(&id_of(&rows, "had a long day")), Some(&None));
        assert_eq!(
            count(db, "SELECT COUNT(*) AS n FROM agent_topic_nodes").await,
            2
        );
        // Under none points at no topic: going through all of it brings
        // nothing of it along.
        let together = kept_together(db, 8, &present, &[id_of(&rows, "had a long day")], &[], 24)
            .await
            .unwrap();
        assert!(together.more.is_empty() && together.cut.is_empty());
    }

    /// Topics under one name go together for free; the small ones changed
    /// since they were last tidied are what she is asked of, and once she
    /// has looked they wait again only when they change.
    #[tokio::test]
    async fn small_topics_are_tidied_once_until_they_change() {
        let Ok(url) = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL") else {
            return;
        };
        let schema = crate::db::IsolatedSchema::migrated(&url, "topic_nodes_tidy").await;
        let db = &schema.db;
        db.execute_unprepared("INSERT INTO users (id, username) VALUES (9, 'nine')")
            .await
            .unwrap();
        let new = |name: &str, aliases: &[&str]| {
            Some(GoesUnder::New {
                name: name.into(),
                aliases: aliases.iter().map(|alias| alias.to_string()).collect(),
            })
        };
        for (text, topic) in [
            ("made a clay cat", new("sculpting", &[])),
            ("takes a sculpting class on Tuesdays", new("sculpting", &[])),
            ("bought carving tools", new("sculpting", &[])),
            (
                "finished a bust of their dad",
                new("sculpting projects", &[]),
            ),
            ("sees Dr. Lee", new("Dr. Lee", &[])),
            ("sees a dentist on Fridays", new("seeing doctors", &[])),
            ("had a long day", new("being tired", &[])),
            ("has a snake plant", new("their plants", &[])),
            ("bought a fern", new("plants", &["Their  Plants"])),
        ] {
            keep(db, 9, fact(text, &[], topic)).await;
        }
        let names = || async {
            let mut names: Vec<String> = topics(db, 9)
                .await
                .unwrap()
                .into_iter()
                .map(|topic| topic.name)
                .collect();
            names.sort();
            names
        };
        assert_eq!(merge_same_names(db, 9).await.unwrap(), 1);
        let named = topics(db, 9).await.unwrap();
        let plants = named
            .iter()
            .find(|topic| topic.name == "their plants")
            .unwrap();
        assert_eq!(
            plants.aliases,
            vec!["plants".to_string()],
            "the older keeps its name"
        );
        assert!(!named.iter().any(|topic| topic.name == "plants"));

        let day = chrono::Duration::days(1);
        assert!(
            with_untidied(db, 10, 6, day).await.unwrap().is_empty(),
            "five wait, not long"
        );
        assert_eq!(with_untidied(db, 10, 5, day).await.unwrap(), vec![9]);
        assert_eq!(
            with_untidied(db, 10, 6, chrono::Duration::zero())
                .await
                .unwrap(),
            vec![9],
            "one has waited long enough"
        );
        let present = Audience::private(9);
        let (due, others) = untidied(db, 9, &present).await.unwrap();
        let mut waiting: Vec<&str> = due.iter().map(|due| due.topic.name.as_str()).collect();
        waiting.sort();
        assert_eq!(
            waiting,
            vec![
                "Dr. Lee",
                "being tired",
                "sculpting projects",
                "seeing doctors",
                "their plants"
            ]
        );
        assert_eq!(
            others
                .iter()
                .map(|other| other.name.as_str())
                .collect::<Vec<_>>(),
            vec!["sculpting"],
            "it holds more than a few"
        );
        let label = |name: &str| {
            format!(
                "d{}",
                due.iter().position(|due| due.topic.name == name).unwrap() + 1
            )
        };
        let raw = serde_json::json!({"topics": [
            {"topic": label("sculpting projects"), "do": "into", "into": "o1"},
            {"topic": label("Dr. Lee"), "do": "into", "into": label("seeing doctors")},
            {"topic": label("being tired"), "do": "let_go", "into": ""},
            {"topic": label("their plants"), "do": "keep", "into": ""}
        ]})
        .to_string();
        let tidied = myriad_merope::topics::read_tidied(&raw, due.len(), others.len());
        apply_tidied(db, 9, &due, &others, &tidied).await.unwrap();
        assert_eq!(
            names().await,
            vec!["sculpting", "seeing doctors", "their plants"]
        );
        let rows = kept(db, 9).await;
        let pointers = pointers(db, 9).await.unwrap();
        let under = |content: &str| pointers.get(&id_of(&rows, content)).cloned().flatten();
        assert_eq!(
            under("finished a bust of their dad"),
            under("made a clay cat")
        );
        assert_eq!(under("sees Dr. Lee"), under("sees a dentist on Fridays"));
        assert_eq!(
            under("had a long day"),
            None,
            "let go: under none, still sorted"
        );
        assert!(pointers.contains_key(&id_of(&rows, "had a long day")));
        let doctors = topics(db, 9).await.unwrap();
        let doctors = doctors
            .iter()
            .find(|topic| topic.name == "seeing doctors")
            .unwrap();
        assert_eq!(doctors.aliases, vec!["Dr. Lee".to_string()]);

        assert!(
            with_untidied(db, 10, 1, chrono::Duration::zero())
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            untidied(db, 9, &present).await.unwrap().0.is_empty(),
            "all looked at"
        );
        let waiting = || async {
            let (due, others) = untidied(db, 9, &present).await.unwrap();
            let names: Vec<String> = due.iter().map(|due| due.topic.name.clone()).collect();
            (names, due, others)
        };
        keep(
            db,
            9,
            fact("has a cat named Mochi", &[], new("their cat", &[])),
        )
        .await;
        let (names, due, others) = waiting().await;
        assert_eq!(names, vec!["their cat"], "only what changed");
        let keep_it = serde_json::json!({"topics": [{"topic": "d1", "do": "keep", "into": ""}]});
        let tidied = myriad_merope::topics::read_tidied(&keep_it.to_string(), 1, others.len());
        apply_tidied(db, 9, &due, &others, &tidied).await.unwrap();
        assert!(waiting().await.0.is_empty());
        keep(db, 9, fact("Mochi is three", &[], new("their cat", &[]))).await;
        assert_eq!(waiting().await.0, vec!["their cat"], "changed again");
        keep(
            db,
            9,
            fact("Mochi sleeps on the piano", &[], new("their cat", &[])),
        )
        .await;
        assert!(waiting().await.0.is_empty(), "three now: more than a few");
    }
}
