//! What she said unprompted, first words, open sessions, and their lines.

use super::*;

pub async fn insert_proactive(
    db: &DatabaseConnection,
    user_id: i32,
    content: &str,
    event_key: Option<&str>,
    notified: bool,
) -> Result<agent_proactive_messages::Model, anyhow::Error> {
    let active = agent_proactive_messages::ActiveModel {
        user_id: Set(user_id),
        role: Set("assistant".to_string()),
        content: Set(content.to_string()),
        event_key: Set(event_key.map(str::to_string)),
        notified: Set(notified),
        created_at: Set(Utc::now().into()),
        ..Default::default()
    };
    Ok(active.insert(db).await?)
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
               p.created_at < NOW() - make_interval(hours => $3) AS settled, \
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
    let age = Utc::now() - latest.created_at.with_timezone(&Utc);
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
    let now = Utc::now().into();
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
    Ok(rows
        .iter()
        .filter_map(|row| {
            Some((
                row.try_get::<chrono::DateTime<chrono::FixedOffset>>("", "created_at")
                    .ok()?,
                row.try_get::<String>("", "content").ok()?,
            ))
        })
        .collect())
}
