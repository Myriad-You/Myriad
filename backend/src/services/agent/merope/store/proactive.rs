//! What she said unprompted, first words, open sessions, and their lines.

use super::*;

/// Keeps a line she said unprompted; `say_id` is the `say` it went out as
/// on a native child, so how far it was said aloud can be told later.
pub async fn insert_proactive(
    db: &DatabaseConnection,
    user_id: i32,
    content: &str,
    event_key: Option<&str>,
    notified: bool,
    say_id: Option<&str>,
) -> Result<agent_proactive_messages::Model, anyhow::Error> {
    let active = agent_proactive_messages::ActiveModel {
        user_id: Set(user_id),
        role: Set("assistant".to_string()),
        content: Set(content.to_string()),
        event_key: Set(event_key.map(str::to_string)),
        notified: Set(notified),
        created_at: Set(crate::clock::now().into()),
        say_id: Set(say_id.map(str::to_string)),
        ..Default::default()
    };
    Ok(active.insert(db).await?)
}

/// Her line `say_id` to `user_id` was cut off while a native child said it
/// aloud: marked as her replies are (`cutOff`, and `heardChars` when the
/// child knows), so her next turn knows they may not have heard it to the
/// end. False while the line is not kept yet.
pub async fn mark_line_cut_off(
    db: &DatabaseConnection,
    user_id: i32,
    say_id: &str,
    heard_chars: Option<u64>,
) -> Result<bool, sea_orm::DbErr> {
    let Some(line) = agent_proactive_messages::Entity::find()
        .filter(agent_proactive_messages::Column::UserId.eq(user_id))
        .filter(agent_proactive_messages::Column::SayId.eq(say_id))
        .order_by_desc(agent_proactive_messages::Column::CreatedAt)
        .one(db)
        .await?
    else {
        return Ok(false);
    };
    let mut metadata = line
        .metadata
        .clone()
        .filter(Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    if let Some(object) = metadata.as_object_mut() {
        object.insert(
            crate::services::agent::chat_prompt::CUT_OFF_KEY.into(),
            crate::services::agent::chat_prompt::cut_off_kind(true).into(),
        );
        if let Some(heard) = heard_chars {
            object.insert("heardChars".into(), heard.into());
        }
    }
    let mut active: agent_proactive_messages::ActiveModel = line.into();
    active.metadata = Set(Some(metadata));
    active.update(db).await?;
    Ok(true)
}

pub async fn recent_proactive(
    db: &DatabaseConnection,
    user_id: i32,
    limit: u64,
) -> Result<Vec<agent_proactive_messages::Model>, anyhow::Error> {
    Ok(agent_proactive_messages::Entity::find()
        .filter(agent_proactive_messages::Column::UserId.eq(user_id))
        .order_by_desc(agent_proactive_messages::Column::CreatedAt)
        .limit(limit)
        .all(db)
        .await?)
}

/// Her last `limit` lines to `user_id` said unasked, newest first: when,
/// what she said, and whether they have written to her in private since.
pub async fn said_lately(
    db: &DatabaseConnection,
    user_id: i32,
    limit: i64,
) -> Result<Vec<(chrono::DateTime<Utc>, String, bool)>, sea_orm::DbErr> {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT p.created_at, p.content, \
               EXISTS (SELECT 1 FROM agent_messages m JOIN agent_sessions s ON s.id = m.session_id \
                 WHERE s.user_id = p.user_id AND s.context->>'mode' = 'chat' \
                   AND s.context->>'venue' IS NULL AND m.role = 'user' \
                   AND m.created_at > p.created_at) AS answered \
             FROM agent_proactive_messages p \
             WHERE p.user_id = $1 AND p.role = 'assistant' \
             ORDER BY p.created_at DESC LIMIT $2",
            [user_id.into(), limit.into()],
        ))
        .await?;
    Ok(rows
        .iter()
        .filter_map(|row| {
            let at = row
                .try_get::<chrono::DateTime<chrono::FixedOffset>>("", "created_at")
                .ok()?
                .with_timezone(&Utc);
            let line = row.try_get::<String>("", "content").ok()?;
            let answered = row.try_get::<bool>("", "answered").ok()?;
            Some((at, line, answered))
        })
        .collect())
}

/// The times she wrote to someone first (`event_key`) since `since`, to one
/// person or to anyone, newest first: who, when, and whether they said
/// anything to her in private within `within_hours` after; `None` while
/// that long has not passed and they have not.
pub async fn first_words(
    db: &DatabaseConnection,
    event_key: &str,
    user_id: Option<i32>,
    since: chrono::DateTime<Utc>,
    within_hours: i64,
    limit: i64,
) -> Result<Vec<(i32, chrono::DateTime<Utc>, Option<bool>)>, sea_orm::DbErr> {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT p.user_id, p.created_at, \
               p.created_at < $6::timestamptz - make_interval(hours => $3) AS settled, \
               EXISTS (SELECT 1 FROM agent_messages m JOIN agent_sessions s ON s.id = m.session_id \
                 WHERE s.user_id = p.user_id AND s.context->>'mode' = 'chat' \
                   AND s.context->>'venue' IS NULL AND m.role = 'user' \
                   AND m.created_at > p.created_at \
                   AND m.created_at <= p.created_at + make_interval(hours => $3)) AS answered \
             FROM agent_proactive_messages p \
             WHERE p.role = 'assistant' AND p.event_key = $5 AND p.created_at >= $1 \
               AND ($2::int IS NULL OR p.user_id = $2) \
             ORDER BY p.created_at DESC LIMIT $4",
            [
                since.fixed_offset().into(),
                user_id.into(),
                (within_hours as i32).into(),
                limit.into(),
                event_key.into(),
                crate::clock::now().into(),
            ],
        ))
        .await?;
    Ok(rows
        .iter()
        .filter_map(|row| {
            let user_id = row.try_get::<i32>("", "user_id").ok()?;
            let at = row
                .try_get::<chrono::DateTime<chrono::FixedOffset>>("", "created_at")
                .ok()?
                .with_timezone(&Utc);
            let settled = row.try_get::<bool>("", "settled").ok()?;
            let answered = row.try_get::<bool>("", "answered").ok()?;
            Some((user_id, at, (answered || settled).then_some(answered)))
        })
        .collect())
}

/// Her last lines to `user_id` under `event_key`, newest first: (when, what
/// she wrote).
pub async fn first_words_said(
    db: &DatabaseConnection,
    event_key: &str,
    user_id: i32,
    limit: u64,
) -> Result<Vec<(chrono::DateTime<Utc>, String)>, sea_orm::DbErr> {
    use sea_orm::QuerySelect;
    Ok(agent_proactive_messages::Entity::find()
        .filter(agent_proactive_messages::Column::UserId.eq(user_id))
        .filter(agent_proactive_messages::Column::EventKey.eq(event_key))
        .filter(agent_proactive_messages::Column::Role.eq("assistant"))
        .order_by_desc(agent_proactive_messages::Column::CreatedAt)
        .limit(limit)
        .all(db)
        .await?
        .into_iter()
        .map(|row| (row.created_at.with_timezone(&Utc), row.content))
        .collect())
}

pub async fn recently_spoke_event(
    db: &DatabaseConnection,
    user_id: i32,
    event_key: &str,
    within_minutes: i64,
) -> Result<bool, anyhow::Error> {
    let Some(latest) = agent_proactive_messages::Entity::find()
        .filter(agent_proactive_messages::Column::UserId.eq(user_id))
        .filter(agent_proactive_messages::Column::EventKey.eq(event_key))
        .order_by_desc(agent_proactive_messages::Column::CreatedAt)
        .one(db)
        .await?
    else {
        return Ok(false);
    };
    let age = crate::clock::now() - latest.created_at.with_timezone(&Utc);
    Ok(age.num_minutes() < within_minutes)
}

pub async fn latest_open_session(
    db: &DatabaseConnection,
    user_id: i32,
) -> Result<Option<(String, chrono::DateTime<Utc>)>, anyhow::Error> {
    let Some(session) = agent_sessions::Entity::find()
        .filter(agent_sessions::Column::UserId.eq(user_id))
        .filter(agent_sessions::Column::Archived.eq(false))
        // Her own lines to them go to a conversation of theirs, never a group.
        .filter(crate::services::agent::sessions::private_sessions())
        .order_by_desc(agent_sessions::Column::LastActiveAt)
        .one(db)
        .await?
    else {
        return Ok(None);
    };
    Ok(Some((
        session.id,
        session.last_active_at.with_timezone(&Utc),
    )))
}

pub async fn touch_proactive(
    db: &DatabaseConnection,
    user_id: i32,
) -> Result<agent_addressee_state::Model, anyhow::Error> {
    let state = get_or_create_state(db, user_id).await?;
    let now = crate::clock::now().into();
    let mut active: agent_addressee_state::ActiveModel = state.into();
    active.last_proactive_at = Set(Some(now));
    active.updated_at = Set(now);
    Ok(active.update(db).await?)
}

/// Someone's own lines to her in chat, in private and in groups, newest
/// first: (when, what they wrote).
pub async fn their_lines(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    limit: i64,
) -> Result<Vec<(chrono::DateTime<chrono::FixedOffset>, String)>, sea_orm::DbErr> {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT m.created_at, m.content FROM agent_messages m \
             JOIN agent_sessions s ON s.id = m.session_id \
             WHERE s.user_id = $1 AND m.role = 'user' AND s.context->>'mode' = 'chat' \
             ORDER BY m.created_at DESC LIMIT $2",
            [user_id.into(), limit.into()],
        ))
        .await?;
    Ok(dated_lines(&rows))
}

/// [`their_lines`] in one place only: in private (`None`), or in the group
/// `venue` (`telegram:-100123`). The same person types one way among a
/// group and another way alone with her.
pub async fn their_lines_in(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    venue: Option<&str>,
    limit: i64,
) -> Result<Vec<(chrono::DateTime<chrono::FixedOffset>, String)>, sea_orm::DbErr> {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT m.created_at, m.content FROM agent_messages m \
             JOIN agent_sessions s ON s.id = m.session_id \
             WHERE s.user_id = $1 AND m.role = 'user' AND s.context->>'mode' = 'chat' \
               AND (s.context->>'venue') IS NOT DISTINCT FROM $2 \
             ORDER BY m.created_at DESC LIMIT $3",
            [
                user_id.into(),
                venue.map(str::to_string).into(),
                limit.into(),
            ],
        ))
        .await?;
    Ok(dated_lines(&rows))
}

fn dated_lines(
    rows: &[sea_orm::QueryResult],
) -> Vec<(chrono::DateTime<chrono::FixedOffset>, String)> {
    rows.iter()
        .filter_map(|row| {
            Some((
                row.try_get::<chrono::DateTime<chrono::FixedOffset>>("", "created_at")
                    .ok()?,
                row.try_get::<String>("", "content").ok()?,
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use sea_orm::ConnectionTrait;

    /// What she said unasked comes back newest first, with whether they
    /// wrote to her in private after it; a group's talk or someone else's
    /// does not count (an isolated schema; skipped without a test database).
    #[tokio::test]
    async fn what_she_said_lately_knows_whether_they_wrote_back() {
        let Some(fixture) = crate::federation::test_db::SchemaDb::new_or_media().await else {
            return;
        };
        let db = &fixture.db;
        for statement in [
            "INSERT INTO users (id, username) VALUES (51, 'owner51'), (52, 'guest52')",
            "INSERT INTO agent_sessions (id, user_id, title, context, message_count, archived, \
             created_at, last_active_at) VALUES \
             ('s_own', 51, NULL, '{\"mode\":\"chat\"}', 1, false, '2026-10-26Z', '2026-10-26Z'), \
             ('s_grp', 51, NULL, '{\"mode\":\"chat\",\"venue\":\"telegram:-1\"}', 1, false, \
              '2026-10-26Z', '2026-10-26Z'), \
             ('s_other', 52, NULL, '{\"mode\":\"chat\"}', 1, false, '2026-10-26Z', '2026-10-26Z')",
            "INSERT INTO agent_proactive_messages (user_id, role, content, event_key, notified, \
             created_at) VALUES \
             (51, 'assistant', '报告好了，整份都在。', 'agent.merope.ai_chat', false, '2026-10-26T01:00:00Z'), \
             (51, 'assistant', 'x 那份报告好了。', 'agent.merope.ai_chat', false, '2026-10-26T02:00:00Z'), \
             (52, 'assistant', '给别人的', 'agent.merope.greeting', false, '2026-10-26T03:00:00Z')",
            // They wrote back between the two in private; later only in a group,
            // and someone else wrote after the second.
            "INSERT INTO agent_messages (session_id, role, content, created_at) VALUES \
             ('s_own', 'user', '看到了', '2026-10-26T01:30:00Z'), \
             ('s_grp', 'user', '群里的话', '2026-10-26T02:30:00Z'), \
             ('s_other', 'user', '别人的私聊', '2026-10-26T02:30:00Z')",
        ] {
            db.execute_unprepared(statement).await.unwrap();
        }
        let said = super::said_lately(db, 51, 6).await.unwrap();
        let lines: Vec<(&str, bool)> = said
            .iter()
            .map(|(_, line, answered)| (line.as_str(), *answered))
            .collect();
        assert_eq!(
            lines,
            [("x 那份报告好了。", false), ("报告好了，整份都在。", true)]
        );
        assert!(said[0].0 > said[1].0, "newest first");
        assert_eq!(super::said_lately(db, 51, 1).await.unwrap().len(), 1);
    }
}
