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
    // Written beside what else marks the line (a card it carries), never
    // over it.
    let mut marks = serde_json::Map::new();
    marks.insert(
        crate::services::agent::chat_prompt::CUT_OFF_KEY.into(),
        crate::services::agent::chat_prompt::cut_off_kind(true).into(),
    );
    if let Some(heard) = heard_chars {
        marks.insert("heardChars".into(), heard.into());
    }
    db.execute_raw(sea_orm::Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Postgres,
        "UPDATE agent_proactive_messages \
         SET metadata = (CASE WHEN jsonb_typeof(metadata) = 'object' THEN metadata \
           ELSE '{}'::jsonb END) || $2::jsonb WHERE id = $1",
        [line.id.into(), Value::Object(marks).into()],
    ))
    .await?;
    Ok(true)
}

pub async fn recent_proactive(
    db: &DatabaseConnection,
    user_id: i32,
    limit: u64,
) -> Result<Vec<agent_proactive_messages::Model>, anyhow::Error> {
    Ok(agent_proactive_messages::Entity::find()
        .filter(agent_proactive_messages::Column::UserId.eq(user_id))
        .filter(agent_proactive_messages::Column::Role.eq("assistant"))
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

/// How many of their lines after her first words make up what they said
/// back, and how much of it she reads.
const REPLY_LINES: i64 = 3;
const REPLY_CHARS: usize = 200;

/// What they said back to her last first words to `user_id` under
/// `event_key`, by when she wrote: their first lines to her in private
/// within `within_hours` after (a group's talk does not count), joined and
/// cut short. A time she wrote that they did not answer is absent.
pub async fn first_words_replies(
    db: &DatabaseConnection,
    event_key: &str,
    user_id: i32,
    within_hours: i64,
    limit: i64,
) -> Result<std::collections::HashMap<chrono::DateTime<Utc>, String>, sea_orm::DbErr> {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT p.created_at, \
               (SELECT string_agg(r.content, E'\\n' ORDER BY r.created_at) FROM \
                 (SELECT m.content, m.created_at FROM agent_messages m \
                   JOIN agent_sessions s ON s.id = m.session_id \
                   WHERE s.user_id = p.user_id AND s.context->>'mode' = 'chat' \
                     AND s.context->>'venue' IS NULL AND m.role = 'user' \
                     AND m.created_at > p.created_at \
                     AND m.created_at <= p.created_at + make_interval(hours => $3) \
                   ORDER BY m.created_at LIMIT $5) r) AS replied \
             FROM agent_proactive_messages p \
             WHERE p.role = 'assistant' AND p.event_key = $2 AND p.user_id = $1 \
             ORDER BY p.created_at DESC LIMIT $4",
            [
                user_id.into(),
                event_key.into(),
                (within_hours as i32).into(),
                limit.into(),
                REPLY_LINES.into(),
            ],
        ))
        .await?;
    Ok(rows
        .iter()
        .filter_map(|row| {
            let at = row
                .try_get::<chrono::DateTime<chrono::FixedOffset>>("", "created_at")
                .ok()?
                .with_timezone(&Utc);
            let replied = row.try_get::<Option<String>>("", "replied").ok()??;
            let replied = replied.trim();
            if replied.is_empty() {
                return None;
            }
            let mut cut: String = replied.chars().take(REPLY_CHARS).collect();
            if replied.chars().count() > REPLY_CHARS {
                cut.push('…');
            }
            Some((at, cut))
        })
        .collect())
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
        fixture.close().await;
    }

    /// What they said back to her first words: their first three private
    /// lines within the day, in order, cut short when long; a group's talk,
    /// another person's chat, a line after the day, a time they never
    /// answered and her first words of another kind give nothing.
    #[tokio::test]
    async fn first_words_come_back_with_what_they_said_back() {
        let Some(fixture) = crate::federation::test_db::SchemaDb::new_or_media().await else {
            return;
        };
        let db = &fixture.db;
        for statement in [
            "INSERT INTO users (id, username) VALUES (61, 'owner61'), (62, 'guest62')",
            "INSERT INTO agent_sessions (id, user_id, title, context, message_count, archived, \
             created_at, last_active_at) VALUES \
             ('r_own', 61, NULL, '{\"mode\":\"chat\"}', 1, false, '2026-11-01Z', '2026-11-01Z'), \
             ('r_grp', 61, NULL, '{\"mode\":\"chat\",\"venue\":\"telegram:-1\"}', 1, false, \
              '2026-11-01Z', '2026-11-01Z'), \
             ('r_other', 62, NULL, '{\"mode\":\"chat\"}', 1, false, '2026-11-01Z', '2026-11-01Z')",
            "INSERT INTO agent_proactive_messages (user_id, role, content, event_key, notified, \
             created_at) VALUES \
             (61, 'assistant', '周末在干嘛呀？', 'agent.merope.reach', false, '2026-11-01T10:00:00Z'), \
             (61, 'assistant', '数个数', 'agent.merope.reach', false, '2026-11-03T10:00:00Z'), \
             (61, 'assistant', '说长点', 'agent.merope.reach', false, '2026-11-04T10:00:00Z'), \
             (61, 'assistant', '今天上课咋样？', 'agent.merope.reach', false, '2026-11-05T10:00:00Z'), \
             (61, 'assistant', '别的事', 'agent.merope.other', false, '2026-11-06T10:00:00Z')",
            "INSERT INTO agent_messages (session_id, role, content, created_at) VALUES \
             ('r_own', 'user', '周末别主动找我哈', '2026-11-01T10:05:00Z'), \
             ('r_own', 'assistant', '好嘞', '2026-11-01T10:06:00Z'), \
             ('r_own', 'user', '陪孩子呢', '2026-11-01T10:07:00Z'), \
             ('r_grp', 'user', '群里的话', '2026-11-01T10:08:00Z'), \
             ('r_own', 'user', '一', '2026-11-03T10:01:00Z'), \
             ('r_own', 'user', '二', '2026-11-03T10:02:00Z'), \
             ('r_own', 'user', '三', '2026-11-03T10:03:00Z'), \
             ('r_own', 'user', '四', '2026-11-03T10:04:00Z'), \
             ('r_own', 'user', repeat('长', 250), '2026-11-04T10:01:00Z'), \
             ('r_other', 'user', '别人的私聊', '2026-11-05T10:05:00Z'), \
             ('r_own', 'user', '回别的事', '2026-11-06T10:30:00Z'), \
             ('r_own', 'user', '隔了两天才说', '2026-11-07T12:00:00Z')",
        ] {
            db.execute_unprepared(statement).await.unwrap();
        }
        let replies = super::first_words_replies(db, "agent.merope.reach", 61, 24, 6)
            .await
            .unwrap();
        let mut got: Vec<(String, &str)> = replies
            .iter()
            .map(|(at, line)| (at.to_rfc3339(), line.as_str()))
            .collect();
        got.sort();
        let long = format!("{}…", "长".repeat(200));
        assert_eq!(
            got,
            [
                (
                    "2026-11-01T10:00:00+00:00".to_string(),
                    "周末别主动找我哈\n陪孩子呢"
                ),
                // Their first three lines, not the fourth.
                ("2026-11-03T10:00:00+00:00".to_string(), "一\n二\n三"),
                ("2026-11-04T10:00:00+00:00".to_string(), long.as_str()),
            ]
        );
        fixture.close().await;
    }
}
