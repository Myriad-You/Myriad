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
//! Each row also has a finer category from where it came. The admin may read,
//! edit and retire any of it; retired rows are kept, not deleted.

use sea_orm::{
    ColumnTrait, ConnectionTrait, DbErr, EntityTrait, QueryFilter, QueryOrder, QuerySelect, Set,
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

/// Retire any active memory: kept, no longer recalled.
pub async fn retire_any<C: ConnectionTrait>(db: &C, id: &str) -> Result<bool, DbErr> {
    let now = chrono::Utc::now().fixed_offset();
    let result = agent_memories::Entity::update_many()
        .set(agent_memories::ActiveModel {
            invalid_at: Set(Some(now)),
            invalid_reason: Set(Some("deleted".into())),
            updated_at: Set(now),
            ..Default::default()
        })
        .filter(agent_memories::Column::InvalidAt.is_null())
        .filter(agent_memories::Column::Id.eq(id))
        .exec(db)
        .await?;
    Ok(result.rows_affected == 1)
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
        assert!(retire_any(db, "g1").await.unwrap());
        assert!(!retire_any(db, "g1").await.unwrap(), "already retired");
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
