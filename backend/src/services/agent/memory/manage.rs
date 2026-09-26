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

use super::unified::{OWN_EXPERIENCE, OWN_VENUE, OWN_VIEW, normalize_content};

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
            _ => "other",
        },
        Whose::Person(_) => match source {
            "chat" => "chat",
            "event" => "event",
            "lookup" => "lookup",
            "import" => "import",
            "presence" => "playing",
            "game" => "game",
            "bit" => "bit",
            "thread" => "thread",
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
}
