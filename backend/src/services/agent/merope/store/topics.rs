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
    /// None: only about them, or of no kind. Kept, so it is not asked of
    /// again.
    Nowhere,
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
            Some(GoesUnder::Nowhere) => None,
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
) -> Result<(Vec<String>, Vec<(String, usize, usize)>), DbErr> {
    if user_id <= 0 || !admitted(present) || named.is_empty() {
        return Ok((Vec::new(), Vec::new()));
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
        return Ok((Vec::new(), Vec::new()));
    }
    let names: std::collections::HashMap<String, String> = topics(db, user_id)
        .await?
        .into_iter()
        .map(|topic| (topic.id, topic.name))
        .collect();
    let under = under_each(db, user_id, present).await?;
    let mut added: Vec<String> = Vec::new();
    let mut cut = Vec::new();
    for topic in order {
        let lines: Vec<String> = under
            .get(topic)
            .into_iter()
            .flatten()
            .map(super::as_known)
            .filter(|line| !line.is_empty())
            .collect();
        for line in &lines {
            if added.len() < at_most && !have.contains(line) && !added.contains(line) {
                added.push(line.clone());
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
    Ok((added, cut))
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
        let (more, cut) = kept_together(db, 7, &present, &[patel.clone()], &have, 24)
            .await
            .unwrap();
        assert_eq!(more, vec![line(&cleared)]);
        assert_eq!(cut, vec![("seeing doctors".to_string(), 2, 2)]);
        let (more, cut) = kept_together(db, 7, &present, &[patel.clone()], &have, 0)
            .await
            .unwrap();
        assert!(more.is_empty());
        assert_eq!(
            cut,
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
        let (more, cut) =
            kept_together(db, 8, &present, &[id_of(&rows, "had a long day")], &[], 24)
                .await
                .unwrap();
        assert!(more.is_empty() && cut.is_empty());
    }
}
