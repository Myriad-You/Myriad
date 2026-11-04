//! The topics she keeps what she knows of someone under
//! (`myriad_merope::topics`, `migrations/topic_nodes.sql`): a kind of thing in
//! their life they come back to. Going over a stretch of talk she is shown
//! the likely ones and puts each fact she keeps under one, starts one, or
//! leaves it under none; going through all of something, what she kept
//! under the same topic comes to mind together.
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

/// Where a fact she keeps goes, as going over a stretch decided.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GoesUnder {
    /// A topic she keeps, by its id.
    Topic(String),
    /// One she starts.
    New { name: String, aliases: Vec<String> },
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

/// Which topic each of their memories goes under, by memory id.
async fn pointers<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
) -> Result<std::collections::HashMap<String, String>, DbErr> {
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
                row.try_get("", "topic_id").ok()?,
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
        if let Some(topic) = pointers.get(&row.id) {
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
/// thing is now); else where going over put it. A topic she starts under a
/// name she already keeps one by is that one. Nothing outside a private
/// talk, or for a memory already under one.
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
    let topic = match replaced.iter().find_map(|old| pointers.get(old)) {
        Some(topic) => Some(topic.clone()),
        None => match goes {
            Some(GoesUnder::Topic(id)) => {
                kept.iter().any(|topic| &topic.id == id).then(|| id.clone())
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
            None => None,
        },
    };
    let Some(topic) = topic else {
        return Ok(());
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
        if let Some(topic) = pointers.get(id)
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
        assert_eq!(pointers.get(&cleared), Some(&id));
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

        // He has her forget all of it: the topic's name goes with it.
        db.execute_unprepared("DELETE FROM agent_memories WHERE user_id = 7")
            .await
            .unwrap();
        assert_eq!(
            count(db, "SELECT COUNT(*) AS n FROM agent_topic_nodes").await,
            0
        );
    }
}
