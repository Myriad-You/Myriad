use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter,
    QueryOrder, QuerySelect,
};
use uuid::Uuid;

use crate::models::entities::{
    agent_addressee_state, agent_diary, agent_persona, agent_proactive_messages, agent_sessions,
};

pub const PERSONA_ROW_ID: &str = "site";

pub async fn get_persona(
    db: &DatabaseConnection,
) -> Result<Option<agent_persona::Model>, anyhow::Error> {
    Ok(agent_persona::Entity::find_by_id(PERSONA_ROW_ID)
        .one(db)
        .await?)
}

pub fn normalize_persona_fields(name: &str, personality: &str) -> (String, String) {
    (name.trim().to_string(), personality.trim().to_string())
}

/// What a persona write does to the portrait. An absent field must not wipe it —
/// the manage drawer only ever sends name and personality.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortraitUpdate {
    Keep,
    Clear,
    Set(String),
}

impl PortraitUpdate {
    fn stored(&self) -> Option<String> {
        match self {
            PortraitUpdate::Set(value) => Some(value.clone()),
            _ => None,
        }
    }
}

fn apply_persona_update(
    existing: agent_persona::Model,
    name: String,
    personality: String,
    portrait: &PortraitUpdate,
    updated_by: i32,
) -> agent_persona::ActiveModel {
    let mut active: agent_persona::ActiveModel = existing.into();
    active.name = Set(name);
    active.personality = Set(personality);
    match portrait {
        PortraitUpdate::Keep => {}
        PortraitUpdate::Clear => active.portrait_asset_id = Set(None),
        PortraitUpdate::Set(value) => active.portrait_asset_id = Set(Some(value.clone())),
    }
    active.updated_by = Set(Some(updated_by));
    active.updated_at = Set(Utc::now().into());
    active
}

pub async fn upsert_persona(
    db: &DatabaseConnection,
    name: String,
    personality: String,
    portrait: PortraitUpdate,
    updated_by: i32,
) -> Result<agent_persona::Model, anyhow::Error> {
    let (name, personality) = normalize_persona_fields(&name, &personality);
    if let Some(existing) = get_persona(db).await? {
        return Ok(
            apply_persona_update(existing, name, personality, &portrait, updated_by)
                .update(db)
                .await?,
        );
    }
    let active = agent_persona::ActiveModel {
        id: Set(PERSONA_ROW_ID.to_string()),
        name: Set(name.clone()),
        personality: Set(personality.clone()),
        portrait_asset_id: Set(portrait.stored()),
        updated_by: Set(Some(updated_by)),
        updated_at: Set(Utc::now().into()),
    };
    match active.insert(db).await {
        Ok(model) => Ok(model),
        Err(err) if is_unique_conflict(&err) => {
            let existing = get_persona(db).await?.ok_or_else(|| anyhow::anyhow!(err))?;
            Ok(
                apply_persona_update(existing, name, personality, &portrait, updated_by)
                    .update(db)
                    .await?,
            )
        }
        Err(err) => Err(err.into()),
    }
}

pub async fn clear_persona(db: &DatabaseConnection) -> Result<(), anyhow::Error> {
    agent_persona::Entity::delete_by_id(PERSONA_ROW_ID)
        .exec(db)
        .await?;
    Ok(())
}

fn is_unique_conflict(err: &impl std::fmt::Display) -> bool {
    let lower = err.to_string().to_ascii_lowercase();
    lower.contains("23505") || lower.contains("duplicate key")
}

pub async fn get_or_create_state(
    db: &DatabaseConnection,
    user_id: i32,
) -> Result<agent_addressee_state::Model, anyhow::Error> {
    if let Some(existing) = agent_addressee_state::Entity::find_by_id(user_id)
        .one(db)
        .await?
    {
        return Ok(existing);
    }
    let now = Utc::now().into();
    let active = agent_addressee_state::ActiveModel {
        user_id: Set(user_id),
        mood: Set(70.0),
        activity: Set("idle".to_string()),
        do_not_disturb: Set(false),
        last_user_message_at: Set(None),
        last_proactive_at: Set(None),
        last_departure_at: Set(None),
        updated_at: Set(now),
    };
    match active.insert(db).await {
        Ok(model) => Ok(model),
        Err(err) if is_unique_conflict(&err) => agent_addressee_state::Entity::find_by_id(user_id)
            .one(db)
            .await?
            .ok_or_else(|| err.into()),
        Err(err) => Err(err.into()),
    }
}

pub async fn save_mood(
    db: &DatabaseConnection,
    user_id: i32,
    mood: f64,
    touch_user_message: bool,
) -> Result<agent_addressee_state::Model, anyhow::Error> {
    let mut state = get_or_create_state(db, user_id).await?;
    let now = Utc::now().into();
    let mut active: agent_addressee_state::ActiveModel = state.clone().into();
    active.mood = Set(super::clamp_mood(mood));
    active.updated_at = Set(now);
    if touch_user_message {
        active.last_user_message_at = Set(Some(now));
    }
    state = active.update(db).await?;
    Ok(state)
}

/// Departure decay. Stamps `last_departure_at` so the same silence window is not
/// charged twice — `updated_at` cannot carry that, every activity write touches it.
pub async fn save_departure_mood(
    db: &DatabaseConnection,
    user_id: i32,
    mood: f64,
) -> Result<agent_addressee_state::Model, anyhow::Error> {
    let state = get_or_create_state(db, user_id).await?;
    let now = Utc::now().into();
    let mut active: agent_addressee_state::ActiveModel = state.into();
    active.mood = Set(super::clamp_mood(mood));
    active.last_departure_at = Set(Some(now));
    active.updated_at = Set(now);
    Ok(active.update(db).await?)
}

pub async fn insert_diary(
    db: &DatabaseConnection,
    user_id: i32,
    content: &str,
    source: &str,
) -> Result<agent_diary::Model, anyhow::Error> {
    let active = agent_diary::ActiveModel {
        id: Set(Uuid::new_v4().simple().to_string()),
        user_id: Set(user_id),
        content: Set(content.to_string()),
        source: Set(source.to_string()),
        created_at: Set(Utc::now().into()),
    };
    Ok(active.insert(db).await?)
}

pub async fn latest_diary(
    db: &DatabaseConnection,
    user_id: i32,
    source: Option<&str>,
) -> Result<Option<agent_diary::Model>, anyhow::Error> {
    let mut query = agent_diary::Entity::find()
        .filter(agent_diary::Column::UserId.eq(user_id))
        .order_by_desc(agent_diary::Column::CreatedAt);
    if let Some(source) = source {
        query = query.filter(agent_diary::Column::Source.eq(source));
    }
    Ok(query.one(db).await?)
}

pub async fn list_diary(
    db: &DatabaseConnection,
    user_id: i32,
    limit: u64,
) -> Result<Vec<agent_diary::Model>, anyhow::Error> {
    Ok(agent_diary::Entity::find()
        .filter(agent_diary::Column::UserId.eq(user_id))
        .order_by_desc(agent_diary::Column::CreatedAt)
        .limit(limit)
        .all(db)
        .await?)
}

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

pub async fn set_do_not_disturb(
    db: &DatabaseConnection,
    user_id: i32,
    do_not_disturb: bool,
) -> Result<agent_addressee_state::Model, anyhow::Error> {
    let state = get_or_create_state(db, user_id).await?;
    if state.do_not_disturb == do_not_disturb {
        return Ok(state);
    }
    let mut active: agent_addressee_state::ActiveModel = state.into();
    active.do_not_disturb = Set(do_not_disturb);
    active.updated_at = Set(Utc::now().into());
    Ok(active.update(db).await?)
}

pub async fn set_activity(
    db: &DatabaseConnection,
    user_id: i32,
    activity: &str,
) -> Result<agent_addressee_state::Model, anyhow::Error> {
    let state = get_or_create_state(db, user_id).await?;
    if state.activity == activity {
        return Ok(state);
    }
    let mut active: agent_addressee_state::ActiveModel = state.into();
    active.activity = Set(activity.to_string());
    active.updated_at = Set(Utc::now().into());
    Ok(active.update(db).await?)
}

pub async fn touch_proactive(
    db: &DatabaseConnection,
    user_id: i32,
) -> Result<agent_addressee_state::Model, anyhow::Error> {
    let state = get_or_create_state(db, user_id).await?;
    let now = Utc::now().into();
    let mut active: agent_addressee_state::ActiveModel = state.into();
    active.last_proactive_at = Set(Some(now));
    active.activity = Set("idle".to_string());
    active.updated_at = Set(now);
    Ok(active.update(db).await?)
}

#[cfg(test)]
mod tests {
    use super::is_unique_conflict;

    #[test]
    fn postgres_duplicate_key_is_unique_conflict() {
        assert!(is_unique_conflict(
            &"error returned from database: 23505 duplicate key value violates unique constraint \"agent_addressee_state_pkey\""
        ));
        assert!(!is_unique_conflict(&"connection reset"));
        assert!(!is_unique_conflict(&"null value in column unique_id"));
    }
}
