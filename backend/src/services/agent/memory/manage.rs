//! The site admin's view of all of her memory, sorted by whose it is.
//!
//! Memory belongs to one of three places, and they never mix:
//! - hers: what she lived on her own (experiences, views, her days); it names
//!   no one;
//! - a person's: what she learned about one person in private with them,
//!   what only the two of them share, what she noticed them playing;
//! - a group's: what a group heard, that group's bits, her notes on people
//!   there from outside the community.
//!
//! Each row also has a finer category from where it came. The admin may read
//! and edit any of it, and have her forget any of it: forgetting deletes the
//! memory, every earlier version of it, what was derived from them and what
//! they came from ([`super::origins`]), so nothing of what it said is kept
//! and nothing teaches it to her again ([`forget_any`]).

use sea_orm::{
    ColumnTrait, ConnectionTrait, DbErr, EntityTrait, QueryFilter, QueryOrder, QuerySelect, Set,
    TransactionTrait,
};

use crate::models::entities::agent_memories;

use super::unified::{MemoryKind, OWN_EXPERIENCE, OWN_VENUE, OWN_VIEW, normalize_content};

/// Whose memory a row is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Whose {
    Hers,
    Person(i32),
    /// A group, as sessions know it (`telegram:-100123`).
    Group(String),
}

pub fn whose(row: &agent_memories::Model) -> Whose {
    if let Some(group) = row.venue.strip_prefix("group:") {
        return Whose::Group(group.to_string());
    }
    match row.user_id {
        Some(user_id) if row.venue != OWN_VENUE => Whose::Person(user_id),
        _ => Whose::Hers,
    }
}

/// Where a row came from, finer than whose it is.
pub fn category(row: &agent_memories::Model) -> &'static str {
    let source = row.source.as_str();
    match whose(row) {
        Whose::Hers => match source {
            s if s == OWN_EXPERIENCE => "experience",
            s if s == OWN_VIEW => "view",
            "narrative" => "day",
            "self" => "self",
            "corrected" => "corrected",
            "question" => "question",
            _ => "other",
        },
        Whose::Person(_) => match source {
            "chat" => "chat",
            "chat_note" => "note",
            "event" => "event",
            "lookup" => "lookup",
            "import" => "import",
            "presence" => "playing",
            "game" => "game",
            "bit" => "bit",
            "thread" => "thread",
            "work" if row.kind == MemoryKind::Done.as_str() => "done",
            "work" => "work",
            s if s == super::unified::WORK_TRIED_SOURCE => "tried",
            _ => "other",
        },
        Whose::Group(_) => match source {
            "bit" => "bit",
            "stranger" => "stranger",
            _ => "learned",
        },
    }
}

/// Every active memory, newest first.
pub async fn list_all<C: ConnectionTrait>(
    db: &C,
    limit: u64,
) -> Result<Vec<agent_memories::Model>, DbErr> {
    agent_memories::Entity::find()
        .filter(agent_memories::Column::InvalidAt.is_null())
        .order_by_desc(agent_memories::Column::CreatedAt)
        .limit(limit)
        .all(db)
        .await
}

/// Edit any active memory. Returns whether a row changed.
pub async fn update_any<C: ConnectionTrait>(
    db: &C,
    id: &str,
    content: &str,
) -> Result<bool, DbErr> {
    let content = normalize_content(content);
    if content.is_empty() {
        return Ok(false);
    }
    let result = agent_memories::Entity::update_many()
        .set(agent_memories::ActiveModel {
            content: Set(content),
            updated_at: Set(chrono::Utc::now().fixed_offset()),
            ..Default::default()
        })
        .filter(agent_memories::Column::InvalidAt.is_null())
        .filter(agent_memories::Column::Id.eq(id))
        .exec(db)
        .await?;
    Ok(result.rows_affected == 1)
}

/// Has her forget any active memory, truly: the row, every earlier version
/// of it (what it replaced, and what those replaced, along the replacement
/// edges, the short notes a lasting memory was kept from among them), what
/// was derived from them (their meaning vectors and cues, by the foreign
/// keys) and the words they came from ([`super::origins::forget`]) are
/// deleted in one transaction. Nothing of what they said is kept, not even
/// to stop it being learned again: what would teach it again goes instead.
/// Returns whether there was such a memory.
///
/// The database frees the deleted rows' space at its next vacuum; its write-
/// ahead log and the deployment's backups are outside what this reaches.
pub async fn forget_any(db: &sea_orm::DatabaseConnection, id: &str) -> Result<bool, DbErr> {
    let txn = db.begin().await?;
    // Before any row: queued memory work takes this lock, then writes rows.
    crate::services::agent::merope::store::memory_jobs::lock(&txn)
        .await
        .map_err(|error| DbErr::Custom(error.to_string()))?;
    let Some(row) = agent_memories::Entity::find_by_id(id.to_string())
        .filter(agent_memories::Column::InvalidAt.is_null())
        .one(&txn)
        .await?
    else {
        txn.commit().await?;
        return Ok(false);
    };
    let mut doomed = vec![row.id.clone()];
    let mut frontier = vec![row.id];
    for _ in 0..LINEAGE_STEPS {
        if frontier.is_empty() {
            break;
        }
        frontier = agent_memories::Entity::find()
            .filter(agent_memories::Column::SupersededBy.is_in(frontier))
            .filter(agent_memories::Column::Id.is_not_in(doomed.clone()))
            .all(&txn)
            .await?
            .into_iter()
            .map(|earlier| earlier.id)
            .collect();
        doomed.extend(frontier.iter().cloned());
    }
    let rows = agent_memories::Entity::find()
        .filter(agent_memories::Column::Id.is_in(doomed.clone()))
        .all(&txn)
        .await?;
    super::origins::forget(&txn, &rows).await?;
    agent_memories::Entity::delete_many()
        .filter(agent_memories::Column::Id.is_in(doomed))
        .exec(&txn)
        .await?;
    txn.commit().await?;
    Ok(true)
}

/// Confirm a Work lesson or pattern still only tried (drawn from one run):
/// from now on Work goes about things by it. Returns whether one was.
pub async fn confirm_tried<C: ConnectionTrait>(db: &C, id: &str) -> Result<bool, DbErr> {
    super::work_memory::hold(db, id, 0.0).await
}

/// Steps followed either way along what replaced what: a loop in old rows
/// cannot keep it going.
const LINEAGE_STEPS: usize = 32;

/// How a memory came to be as it is, oldest first: the rows it replaced
/// (and what those replaced), itself, and what replaced it since. Retired
/// rows keep why (`invalid_reason`). For the admin reading the history,
/// never for recall. Empty when no such row.
pub async fn lineage<C: ConnectionTrait>(
    db: &C,
    id: &str,
) -> Result<Vec<agent_memories::Model>, DbErr> {
    let Some(row) = agent_memories::Entity::find_by_id(id.to_string())
        .one(db)
        .await?
    else {
        return Ok(Vec::new());
    };
    let mut seen = std::collections::HashSet::from([row.id.clone()]);
    // Back: whatever named one of these as what took its place.
    let mut before = Vec::new();
    let mut frontier = vec![row.id.clone()];
    for _ in 0..LINEAGE_STEPS {
        if frontier.is_empty() {
            break;
        }
        let replaced = agent_memories::Entity::find()
            .filter(agent_memories::Column::SupersededBy.is_in(frontier.clone()))
            .all(db)
            .await?;
        frontier = replaced
            .iter()
            .filter(|row| seen.insert(row.id.clone()))
            .map(|row| row.id.clone())
            .collect();
        before.extend(
            replaced
                .into_iter()
                .filter(|row| frontier.contains(&row.id)),
        );
    }
    // On: what took its place, and what took that one's.
    let mut after = Vec::new();
    let mut next = row.superseded_by.clone();
    for _ in 0..LINEAGE_STEPS {
        let Some(id) = next.take().filter(|id| seen.insert(id.clone())) else {
            break;
        };
        let Some(newer) = agent_memories::Entity::find_by_id(id).one(db).await? else {
            break;
        };
        next = newer.superseded_by.clone();
        after.push(newer);
    }
    let mut all = before;
    all.push(row);
    all.extend(after);
    all.sort_by_key(|row| row.created_at);
    Ok(all)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(user_id: Option<i32>, venue: &str, source: &str) -> agent_memories::Model {
        let now = chrono::Utc::now().fixed_offset();
        agent_memories::Model {
            id: "m".into(),
            user_id,
            kind: "fact".into(),
            content: "x".into(),
            evidence: None,
            speaker: "agent".into(),
            source: source.into(),
            venue: venue.into(),
            audience: serde_json::json!([]),
            concepts: serde_json::json!([]),
            importance: 0.5,
            access_count: 0,
            last_accessed_at: None,
            valid_from: now,
            invalid_at: None,
            invalid_reason: None,
            created_at: now,
            updated_at: now,
            superseded_by: None,
            origin: crate::services::agent::memory::unified::ORIGIN_SITE.into(),
        }
    }

    #[test]
    fn every_memory_is_hers_a_persons_or_a_groups() {
        let hers = row(None, "own", "doing");
        assert_eq!((whose(&hers), category(&hers)), (Whose::Hers, "experience"));
        let view = row(None, "own", "view");
        assert_eq!(category(&view), "view");
        let day = row(None, "own", "narrative");
        assert_eq!(category(&day), "day");

        let fact = row(Some(7), "private", "chat");
        assert_eq!((whose(&fact), category(&fact)), (Whose::Person(7), "chat"));
        let bit = row(Some(7), "private", "bit");
        assert_eq!(category(&bit), "bit");
        let lesson = row(Some(7), "private", "work");
        assert_eq!(category(&lesson), "work");
        let tried = row(Some(7), "private", "work_tried");
        assert_eq!(category(&tried), "tried");
        let mut done = row(Some(7), "private", "work");
        done.kind = "done".into();
        assert_eq!(category(&done), "done");

        // A group's rows are the group's, whichever member they are kept with.
        let learned = row(Some(7), "group:telegram:-100123", "chat");
        assert_eq!(
            (whose(&learned), category(&learned)),
            (Whose::Group("telegram:-100123".into()), "learned")
        );
        let stranger = row(None, "group:discord:22", "stranger");
        assert_eq!(
            (whose(&stranger), category(&stranger)),
            (Whose::Group("discord:22".into()), "stranger")
        );
    }

    /// The admin sees and manages hers, a person's and a group's alike.
    #[tokio::test]
    async fn the_admin_sees_every_place_and_can_edit_or_retire_any() {
        let Ok(url) = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL") else {
            return;
        };
        let schema = crate::db::IsolatedSchema::migrated(&url, "memory_manage").await;
        let db = &schema.db;
        use sea_orm::ConnectionTrait;
        db.execute_unprepared(
            "INSERT INTO users (id, username) VALUES (31, 'ming');
             INSERT INTO agent_memories (id, user_id, kind, content, speaker, source, venue, audience, concepts, importance, access_count, valid_from, created_at, updated_at)
             VALUES ('own1', NULL, 'knowledge', '听了一首歌', 'agent', 'doing', 'own', '[]', '[]', 0.4, 0, NOW(), NOW(), NOW()),
                    ('p1', 31, 'fact', '喜欢猫', 'user', 'chat', 'private', '[31]', '[]', 0.5, 0, NOW(), NOW(), NOW()),
                    ('g1', NULL, 'fact', '在练吉他', 'agent', 'stranger', 'group:telegram:-100', '[]', '[]', 0.4, 0, NOW(), NOW(), NOW());",
        )
        .await
        .unwrap();
        let rows = list_all(db, 100).await.unwrap();
        let mut seen: Vec<(String, Whose, &str)> = rows
            .iter()
            .map(|row| (row.id.clone(), whose(row), category(row)))
            .collect();
        seen.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(
            seen,
            vec![
                (
                    "g1".into(),
                    Whose::Group("telegram:-100".into()),
                    "stranger"
                ),
                ("own1".into(), Whose::Hers, "experience"),
                ("p1".into(), Whose::Person(31), "chat"),
            ]
        );
        assert!(update_any(db, "own1", "听了两首歌").await.unwrap());
        assert!(forget_any(db, "g1").await.unwrap());
        assert!(!forget_any(db, "g1").await.unwrap(), "already forgotten");
        assert!(
            agent_memories::Entity::find_by_id("g1".to_string())
                .one(db)
                .await
                .unwrap()
                .is_none(),
            "gone, not kept"
        );
        let left: Vec<String> = list_all(db, 100)
            .await
            .unwrap()
            .into_iter()
            .map(|row| row.content)
            .collect();
        assert!(left.contains(&"听了两首歌".to_string()));
        assert_eq!(left.len(), 2);
        schema.drop().await;
    }

    /// Forgetting a memory takes its earlier versions and what was derived
    /// from them with it; a memory beside it stays.
    #[tokio::test]
    async fn forgetting_a_memory_deletes_its_earlier_versions_and_what_came_of_them() {
        let Ok(url) = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL") else {
            return;
        };
        let schema = crate::db::IsolatedSchema::migrated(&url, "memory_forget").await;
        let db = &schema.db;
        use sea_orm::ConnectionTrait;
        db.execute_unprepared(
            "INSERT INTO users (id, username) VALUES (33, 'lan');
             INSERT INTO agent_memories (id, user_id, kind, content, speaker, source, venue, audience, concepts, importance, access_count, valid_from, invalid_at, invalid_reason, superseded_by, created_at, updated_at)
             VALUES ('a', 33, 'fact', '住在大阪', 'user', 'chat', 'private', '[33]', '[]', 0.5, 0, NOW(), NOW(), 'superseded', 'b', NOW() - INTERVAL '2 days', NOW()),
                    ('b', 33, 'fact', '搬去了京都', 'user', 'chat', 'private', '[33]', '[]', 0.5, 0, NOW(), NULL, NULL, NULL, NOW() - INTERVAL '1 day', NOW()),
                    ('x', 33, 'fact', '喜欢猫', 'user', 'chat', 'private', '[33]', '[]', 0.5, 0, NOW(), NULL, NULL, NULL, NOW(), NOW());
             INSERT INTO agent_memory_embeddings (memory_id, model, digest, vector, created_at)
             VALUES ('a', 'm', 'd', decode('00', 'hex'), NOW()), ('b', 'm', 'd', decode('00', 'hex'), NOW()), ('x', 'm', 'd', decode('00', 'hex'), NOW());
             INSERT INTO agent_memory_cues (memory_id, cues, created_at) VALUES ('b', '搬家', NOW());",
        )
        .await
        .unwrap();
        assert!(
            !forget_any(db, "a").await.unwrap(),
            "only an active memory is forgotten"
        );
        assert!(forget_any(db, "b").await.unwrap());
        let left: Vec<String> = agent_memories::Entity::find()
            .all(db)
            .await
            .unwrap()
            .into_iter()
            .map(|row| row.id)
            .collect();
        assert_eq!(left, ["x"]);
        let count = |sql: &'static str| async move {
            db.query_one_raw(sea_orm::Statement::from_string(
                sea_orm::DatabaseBackend::Postgres,
                sql.to_string(),
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get_by_index::<i64>(0)
            .unwrap()
        };
        assert_eq!(
            count("SELECT COUNT(*) FROM agent_memory_embeddings").await,
            1
        );
        assert_eq!(count("SELECT COUNT(*) FROM agent_memory_cues").await, 0);
        schema.drop().await;
    }

    /// Forgetting takes the words it came from: the lines that said it in
    /// its place (with her answer), the same words in her diary and in work
    /// still queued, the notes it was kept from; for a day's line, the day.
    /// What another place or person said, or was said long before, stays.
    #[tokio::test]
    async fn forgetting_a_memory_deletes_the_words_it_came_from() {
        let Ok(url) = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL") else {
            return;
        };
        let schema = crate::db::IsolatedSchema::migrated(&url, "memory_forget_origins").await;
        let db = &schema.db;
        use sea_orm::ConnectionTrait;
        let yesterday = crate::services::agent::merope::clock::today()
            .pred_opt()
            .unwrap();
        let noon = crate::services::agent::merope::clock::day_start(yesterday).unwrap()
            + chrono::Duration::hours(12);
        db.execute_unprepared(&format!(
            "INSERT INTO users (id, username) VALUES (34, 'mei'), (35, 'other');
             INSERT INTO agent_sessions (id, user_id, context, message_count, created_at, last_active_at)
             VALUES ('s-mei', 34, '{{\"mode\":\"chat\"}}', 6, NOW(), NOW()),
                    ('s-group', 34, '{{\"mode\":\"chat\",\"venue\":\"telegram:-1\"}}', 1, NOW(), NOW()),
                    ('s-other', 35, '{{\"mode\":\"chat\"}}', 1, NOW(), NOW());
             INSERT INTO agent_messages (session_id, role, content, created_at) VALUES
               ('s-mei', 'user', '我家  养了一只猫叫年糕', NOW() - INTERVAL '60 minutes'),
               ('s-mei', 'assistant', '年糕这个名字真好', NOW() - INTERVAL '59 minutes'),
               ('s-mei', 'user', '今天下雨了', NOW() - INTERVAL '30 minutes'),
               ('s-mei', 'assistant', '记得带伞', NOW() - INTERVAL '29 minutes'),
               ('s-mei', 'user', '养了一只猫叫年糕', NOW() - INTERVAL '3 days'),
               ('s-group', 'user', '养了一只猫叫年糕', NOW() - INTERVAL '60 minutes'),
               ('s-other', 'user', '养了一只猫叫年糕', NOW() - INTERVAL '60 minutes'),
               ('s-mei', 'user', '昨天的事', '{noon}'),
               ('s-mei', 'assistant', '昨天的回答', '{noon}'::timestamptz + INTERVAL '1 minute');
             INSERT INTO agent_diary (id, user_id, content, source, created_at) VALUES
               ('d1', 34, '我家 养了一只猫叫年糕', 'chat', NOW() - INTERVAL '60 minutes'),
               ('d2', 34, '今天下雨了', 'chat', NOW() - INTERVAL '30 minutes');
             INSERT INTO runtime_registry (namespace, record_id, owner_id, payload, expires_at) VALUES
               ('merope_memory_jobs', 'j-mei', 34, '{{\"owner\":34,\"data\":{{\"Chat\":{{\"user_text\":\"养了一只猫叫年糕\",\"reply\":\"\"}}}}}}', 9999999999),
               ('merope_memory_jobs', 'j-other', 35, '{{\"owner\":35,\"data\":{{\"Chat\":{{\"user_text\":\"养了一只猫叫年糕\",\"reply\":\"\"}}}}}}', 9999999999);
             INSERT INTO agent_memories (id, user_id, kind, content, evidence, speaker, source, venue, audience, concepts, importance, access_count, valid_from, invalid_at, invalid_reason, superseded_by, created_at, updated_at)
             VALUES ('n', 34, 'fact', '养了猫', '养了一只猫叫年糕', 'user', 'chat_note', 'private', '[34]', '[]', 0.5, 0, NOW(), NOW(), 'settled', 'k', NOW() - INTERVAL '58 minutes', NOW()),
                    ('k', 34, 'fact', '家里有只猫叫年糕', '养了一只 猫叫年糕', 'user', 'chat', 'private', '[34]', '[]', 0.5, 0, NOW(), NULL, NULL, NULL, NOW() - INTERVAL '40 minutes', NOW()),
                    ('w', 34, 'fact', '今天下雨', '今天下雨了', 'user', 'chat', 'private', '[34]', '[]', 0.5, 0, NOW(), NULL, NULL, NULL, NOW(), NOW()),
                    ('day', 34, 'fact', '昨天聊了很多', '{{\"day\":\"{yesterday}\"}}', 'agent', 'day_with', 'private', '[34]', '[]', 0.3, 0, NOW(), NULL, NULL, NULL, NOW(), NOW());"
        ))
        .await
        .unwrap();
        assert!(forget_any(db, "k").await.unwrap());
        let texts = |sql: &'static str| async move {
            db.query_all_raw(sea_orm::Statement::from_string(
                sea_orm::DatabaseBackend::Postgres,
                sql.to_string(),
            ))
            .await
            .unwrap()
            .iter()
            .map(|row| row.try_get_by_index::<String>(0).unwrap())
            .collect::<Vec<String>>()
        };
        assert_eq!(
            texts("SELECT m.session_id || ':' || m.content FROM agent_messages m ORDER BY m.created_at, m.session_id").await,
            [
                "s-mei:养了一只猫叫年糕",
                "s-mei:昨天的事",
                "s-mei:昨天的回答",
                "s-group:养了一只猫叫年糕",
                "s-other:养了一只猫叫年糕",
                "s-mei:今天下雨了",
                "s-mei:记得带伞",
            ],
            "the line that said it and her answer go; elsewhere and long before stay"
        );
        assert_eq!(texts("SELECT id FROM agent_diary").await, ["d2"]);
        assert_eq!(
            texts("SELECT record_id FROM runtime_registry").await,
            ["j-other"]
        );
        assert_eq!(
            texts("SELECT id FROM agent_memories ORDER BY id").await,
            ["day", "w"]
        );
        assert_eq!(
            texts("SELECT message_count::text FROM agent_sessions WHERE id = 's-mei'").await,
            ["5"]
        );

        // A day's line is written from the whole day: the day goes with it.
        assert!(forget_any(db, "day").await.unwrap());
        assert_eq!(
            texts(
                "SELECT content FROM agent_messages WHERE session_id = 's-mei' ORDER BY created_at"
            )
            .await,
            ["养了一只猫叫年糕", "今天下雨了", "记得带伞"]
        );
        schema.drop().await;
    }

    /// From any row of it, the admin reads what a memory replaced and what
    /// replaced it, retired rows with why.
    #[tokio::test]
    async fn a_memory_shows_what_it_replaced_and_what_replaced_it() {
        let Ok(url) = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL") else {
            return;
        };
        let schema = crate::db::IsolatedSchema::migrated(&url, "memory_lineage").await;
        let db = &schema.db;
        use sea_orm::ConnectionTrait;
        db.execute_unprepared(
            "INSERT INTO users (id, username) VALUES (32, 'hua');
             INSERT INTO agent_memories (id, user_id, kind, content, speaker, source, venue, audience, concepts, importance, access_count, valid_from, invalid_at, invalid_reason, superseded_by, created_at, updated_at)
             VALUES ('a', 32, 'fact', '住在大阪', 'user', 'chat', 'private', '[32]', '[]', 0.5, 0, NOW(), NOW(), 'superseded', 'b', NOW() - INTERVAL '3 days', NOW()),
                    ('b', 32, 'fact', '搬去了京都', 'user', 'chat', 'private', '[32]', '[]', 0.5, 0, NOW(), NOW(), 'superseded', 'c', NOW() - INTERVAL '2 days', NOW()),
                    ('c', 32, 'fact', '又搬回了大阪', 'user', 'chat', 'private', '[32]', '[]', 0.5, 0, NOW(), NULL, NULL, NULL, NOW() - INTERVAL '1 day', NOW()),
                    ('x', 32, 'fact', '喜欢猫', 'user', 'chat', 'private', '[32]', '[]', 0.5, 0, NOW(), NULL, NULL, NULL, NOW(), NOW());",
        )
        .await
        .unwrap();
        let ids = |rows: Vec<agent_memories::Model>| -> Vec<String> {
            rows.into_iter().map(|row| row.id).collect()
        };
        for from in ["a", "b", "c"] {
            assert_eq!(ids(lineage(db, from).await.unwrap()), ["a", "b", "c"]);
        }
        let middle = lineage(db, "b").await.unwrap();
        assert_eq!(middle[0].invalid_reason.as_deref(), Some("superseded"));
        assert_eq!(ids(lineage(db, "x").await.unwrap()), ["x"]);
        assert!(lineage(db, "missing").await.unwrap().is_empty());
        schema.drop().await;
    }
}
