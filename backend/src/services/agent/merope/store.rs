use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseBackend,
    DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, QuerySelect, Statement,
    TransactionTrait,
};
use serde_json::Value;
use uuid::Uuid;

use crate::models::entities::{
    agent_addressee_state, agent_diary, agent_persona, agent_proactive_messages, agent_sessions,
};

use super::state::{
    apply_music_listening, clamp, persona_affect_baseline, settle, Affect, AffectBaseline,
};

pub const PERSONA_ROW_ID: &str = "site";
pub const MUSIC_MOOD_CREDIT_COOLDOWN_SECS: i64 = 30 * 60;

#[derive(Debug)]
pub struct MusicMoodCredit {
    pub before: Affect,
    pub state: agent_addressee_state::Model,
    pub credited: bool,
    pub next_credit_in_seconds: i64,
}

pub async fn get_persona(
    db: &DatabaseConnection,
) -> Result<Option<agent_persona::Model>, anyhow::Error> {
    get_persona_on(db).await
}

pub async fn get_persona_on<C>(db: &C) -> Result<Option<agent_persona::Model>, anyhow::Error>
where
    C: ConnectionTrait,
{
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

#[derive(Debug, Clone, Default)]
pub struct PersonaContractUpdate {
    pub persona: JsonDocumentUpdate,
    pub visual_profile: JsonDocumentUpdate,
    pub portrait_generation: JsonDocumentUpdate,
}

#[derive(Debug, Clone, Default)]
pub enum JsonDocumentUpdate {
    #[default]
    Keep,
    Clear,
    Set(Value),
}

fn apply_json_update(field: &mut sea_orm::ActiveValue<Option<Value>>, update: &JsonDocumentUpdate) {
    match update {
        JsonDocumentUpdate::Keep => {}
        JsonDocumentUpdate::Clear => *field = Set(None),
        JsonDocumentUpdate::Set(value) => *field = Set(Some(value.clone())),
    }
}

fn visual_generation_inputs_changed(current: Option<&Value>, update: &JsonDocumentUpdate) -> bool {
    match update {
        JsonDocumentUpdate::Keep => false,
        JsonDocumentUpdate::Clear => current.is_some(),
        JsonDocumentUpdate::Set(value) => {
            current
                .map(myriad_merope::appearance_visual_profile)
                .as_ref()
                != Some(&myriad_merope::appearance_visual_profile(value))
        }
    }
}

/// Name or visual appearance changed: old portrait and Rig must both go.
pub fn generation_inputs_changed(
    existing_name: &str,
    existing_visual: Option<&Value>,
    next_name: &str,
    visual_update: &JsonDocumentUpdate,
) -> bool {
    existing_name != next_name || visual_generation_inputs_changed(existing_visual, visual_update)
}

fn apply_persona_update(
    existing: agent_persona::Model,
    name: String,
    personality: String,
    portrait: &PortraitUpdate,
    contract: &PersonaContractUpdate,
    updated_by: i32,
) -> agent_persona::ActiveModel {
    let inputs_changed = generation_inputs_changed(
        &existing.name,
        existing.visual_profile.as_ref(),
        &name,
        &contract.visual_profile,
    );
    let mut active: agent_persona::ActiveModel = existing.into();
    active.name = Set(name);
    active.personality = Set(personality);
    match portrait {
        PortraitUpdate::Keep if inputs_changed => active.portrait_asset_id = Set(None),
        PortraitUpdate::Keep => {}
        PortraitUpdate::Clear => active.portrait_asset_id = Set(None),
        PortraitUpdate::Set(value) => active.portrait_asset_id = Set(Some(value.clone())),
    }
    apply_json_update(&mut active.persona_json, &contract.persona);
    apply_json_update(&mut active.visual_profile, &contract.visual_profile);
    apply_json_update(
        &mut active.portrait_generation,
        &contract.portrait_generation,
    );
    if (!matches!(portrait, PortraitUpdate::Keep) || inputs_changed)
        && matches!(contract.portrait_generation, JsonDocumentUpdate::Keep)
    {
        active.portrait_generation = Set(None);
    }
    active.updated_by = Set(Some(updated_by));
    active.updated_at = Set(Utc::now().into());
    active
}

pub async fn upsert_persona_on<C>(
    db: &C,
    name: String,
    personality: String,
    portrait: PortraitUpdate,
    contract: PersonaContractUpdate,
    updated_by: i32,
) -> Result<agent_persona::Model, anyhow::Error>
where
    C: ConnectionTrait,
{
    let (name, personality) = normalize_persona_fields(&name, &personality);
    if let Some(existing) = get_persona_on(db).await? {
        return Ok(apply_persona_update(
            existing,
            name,
            personality,
            &portrait,
            &contract,
            updated_by,
        )
        .update(db)
        .await?);
    }
    let active = agent_persona::ActiveModel {
        id: Set(PERSONA_ROW_ID.to_string()),
        name: Set(name.clone()),
        personality: Set(personality.clone()),
        persona_json: Set(match &contract.persona {
            JsonDocumentUpdate::Set(value) => Some(value.clone()),
            JsonDocumentUpdate::Keep | JsonDocumentUpdate::Clear => None,
        }),
        visual_profile: Set(match &contract.visual_profile {
            JsonDocumentUpdate::Set(value) => Some(value.clone()),
            JsonDocumentUpdate::Keep | JsonDocumentUpdate::Clear => None,
        }),
        portrait_asset_id: Set(portrait.stored()),
        portrait_generation: Set(match &contract.portrait_generation {
            JsonDocumentUpdate::Set(value) => Some(value.clone()),
            JsonDocumentUpdate::Keep | JsonDocumentUpdate::Clear => None,
        }),
        updated_by: Set(Some(updated_by)),
        updated_at: Set(Utc::now().into()),
    };
    match active.insert(db).await {
        Ok(model) => Ok(model),
        Err(err) if is_unique_conflict(&err) => {
            let existing = get_persona_on(db)
                .await?
                .ok_or_else(|| anyhow::anyhow!(err))?;
            Ok(apply_persona_update(
                existing,
                name,
                personality,
                &portrait,
                &contract,
                updated_by,
            )
            .update(db)
            .await?)
        }
        Err(err) => Err(err.into()),
    }
}

/// Acquire the single site-portrait generation lease only while the exact
/// visual inputs still match. The lease lives in the existing JSON document so
/// it is shared by every backend replica without adding a second source of
/// truth. A crashed request becomes replaceable after fifteen minutes.
pub async fn acquire_portrait_generation<C>(
    db: &C,
    expected_name: &str,
    expected_visual_profile: &Value,
    pending: &Value,
) -> Result<bool, anyhow::Error>
where
    C: ConnectionTrait,
{
    let result = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
UPDATE agent_persona
SET portrait_generation = jsonb_set(
        COALESCE(portrait_generation, '{}'::jsonb),
        '{pending}',
        $1::jsonb,
        true
    ),
    updated_at = CURRENT_TIMESTAMP
WHERE id = $2
  AND name = $3
  AND visual_profile = $4::jsonb
  AND (
      portrait_generation IS NULL
      OR NOT (portrait_generation ? 'pending')
      OR updated_at < CURRENT_TIMESTAMP - INTERVAL '15 minutes'
  )
"#,
            vec![
                pending.clone().into(),
                PERSONA_ROW_ID.into(),
                expected_name.into(),
                expected_visual_profile.clone().into(),
            ],
        ))
        .await?;
    Ok(result.rows_affected() == 1)
}

/// Remove only this request's lease while preserving the last confirmed
/// portrait contract, if any. A newer request can never be unlocked by an
/// older request's error path.
pub async fn release_portrait_generation<C>(db: &C, token: &str) -> Result<(), anyhow::Error>
where
    C: ConnectionTrait,
{
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"
UPDATE agent_persona
SET portrait_generation = CASE
        WHEN (portrait_generation - 'pending') = '{}'::jsonb THEN NULL
        ELSE portrait_generation - 'pending'
    END,
    updated_at = CURRENT_TIMESTAMP
WHERE id = $1
  AND portrait_generation #>> '{pending,token}' = $2
"#,
        vec![PERSONA_ROW_ID.into(), token.into()],
    ))
    .await?;
    Ok(())
}

/// Commit generated pixels only if this request still owns the lease and the
/// visual inputs have not changed. Spoken-persona fields are intentionally not
/// written here, so edits made during a slow image request are preserved.
pub async fn complete_portrait_generation<C>(
    db: &C,
    expected_name: &str,
    expected_visual_profile: &Value,
    token: &str,
    portrait_asset_id: &str,
    portrait_generation: &Value,
    updated_by: i32,
) -> Result<bool, anyhow::Error>
where
    C: ConnectionTrait,
{
    let result = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
UPDATE agent_persona
SET portrait_asset_id = $1,
    portrait_generation = $2::jsonb,
    updated_by = $3,
    updated_at = CURRENT_TIMESTAMP
WHERE id = $4
  AND name = $5
  AND visual_profile = $6::jsonb
  AND portrait_generation #>> '{pending,token}' = $7
"#,
            vec![
                portrait_asset_id.into(),
                portrait_generation.clone().into(),
                updated_by.into(),
                PERSONA_ROW_ID.into(),
                expected_name.into(),
                expected_visual_profile.clone().into(),
                token.into(),
            ],
        ))
        .await?;
    Ok(result.rows_affected() == 1)
}

pub fn portrait_generation_is_pending(value: Option<&Value>) -> bool {
    value
        .and_then(|document| document.get("pending"))
        .and_then(|pending| pending.get("token"))
        .and_then(Value::as_str)
        .is_some_and(|token| !token.is_empty())
}

pub async fn clear_persona_on<C>(db: &C) -> Result<(), anyhow::Error>
where
    C: ConnectionTrait,
{
    agent_proactive_messages::Entity::delete_many()
        .exec(db)
        .await?;
    agent_diary::Entity::delete_many().exec(db).await?;
    agent_addressee_state::Entity::delete_many()
        .exec(db)
        .await?;
    agent_persona::Entity::delete_by_id(PERSONA_ROW_ID)
        .exec(db)
        .await?;
    Ok(())
}

fn is_unique_conflict(err: &impl std::fmt::Display) -> bool {
    let lower = err.to_string().to_ascii_lowercase();
    lower.contains("23505") || lower.contains("duplicate key")
}

pub async fn load_affect_baseline<C>(db: &C) -> AffectBaseline
where
    C: ConnectionTrait,
{
    match get_persona_on(db).await {
        Ok(Some(persona)) => {
            persona_affect_baseline(persona.persona_json.as_ref(), &persona.personality)
        }
        _ => AffectBaseline::default(),
    }
}

pub fn affect_from_state(state: &agent_addressee_state::Model) -> Affect {
    Affect {
        mood: state.mood,
        arousal: state.arousal,
        emotion: state.emotion,
        emotion_arousal: state.emotion_arousal,
    }
}

fn hours_since(at: chrono::DateTime<chrono::FixedOffset>) -> f64 {
    let secs = (Utc::now() - at.with_timezone(&Utc)).num_seconds();
    (secs.max(0) as f64) / 3600.0
}

/// Overlay regression in memory. Writing on read would refresh `updated_at` and
/// keep a stale `working` activity alive.
fn overlay_settled(
    mut state: agent_addressee_state::Model,
    base: AffectBaseline,
) -> agent_addressee_state::Model {
    let settled = settle(
        affect_from_state(&state),
        base,
        hours_since(state.mood_settled_at),
        hours_since(state.emotion_settled_at),
    );
    state.mood = settled.mood;
    state.arousal = settled.arousal;
    state.emotion = settled.emotion;
    state.emotion_arousal = settled.emotion_arousal;
    state
}

pub async fn get_or_create_state<C>(
    db: &C,
    user_id: i32,
) -> Result<agent_addressee_state::Model, anyhow::Error>
where
    C: ConnectionTrait,
{
    let base = load_affect_baseline(db).await;
    if let Some(existing) = agent_addressee_state::Entity::find_by_id(user_id)
        .one(db)
        .await?
    {
        return Ok(overlay_settled(existing, base));
    }
    let rest = Affect::at_rest(base);
    let now = Utc::now().into();
    let active = agent_addressee_state::ActiveModel {
        user_id: Set(user_id),
        mood: Set(rest.mood),
        arousal: Set(rest.arousal),
        emotion: Set(rest.emotion),
        emotion_arousal: Set(rest.emotion_arousal),
        activity: Set("idle".to_string()),
        activity_updated_at: Set(now),
        do_not_disturb: Set(false),
        dnd_start_minute: Set(None),
        dnd_end_minute: Set(None),
        last_user_message_at: Set(None),
        last_proactive_at: Set(None),
        music_mood_credited_at: Set(None),
        mood_settled_at: Set(now),
        emotion_settled_at: Set(now),
        updated_at: Set(now),
    };
    match active.insert(db).await {
        Ok(model) => Ok(model),
        Err(err) if is_unique_conflict(&err) => {
            let existing = agent_addressee_state::Entity::find_by_id(user_id)
                .one(db)
                .await?
                .ok_or_else(|| anyhow::Error::from(err))?;
            Ok(overlay_settled(existing, base))
        }
        Err(err) => Err(err.into()),
    }
}

async fn save_affect_on<C>(
    db: &C,
    user_id: i32,
    affect: Affect,
    touch_user_message: bool,
    touch_music_credit: bool,
) -> Result<agent_addressee_state::Model, anyhow::Error>
where
    C: ConnectionTrait,
{
    let state = get_or_create_state(db, user_id).await?;
    let now = Utc::now().into();
    let mut active: agent_addressee_state::ActiveModel = state.into();
    active.mood = Set(clamp(affect.mood));
    active.arousal = Set(clamp(affect.arousal));
    active.emotion = Set(clamp(affect.emotion));
    active.emotion_arousal = Set(clamp(affect.emotion_arousal));
    active.mood_settled_at = Set(now);
    active.emotion_settled_at = Set(now);
    active.updated_at = Set(now);
    if touch_user_message {
        active.last_user_message_at = Set(Some(now));
    }
    if touch_music_credit {
        active.music_mood_credited_at = Set(Some(now));
    }
    Ok(active.update(db).await?)
}

/// Apply one affect delta to the latest row under a per-addressee database
/// lock. Chat turns, async appraisal and task outcomes can arrive from
/// different sessions or backend replicas; serializing the read-modify-write
/// keeps every delta instead of letting the last absolute write erase one.
pub async fn update_affect<F>(
    db: &DatabaseConnection,
    user_id: i32,
    touch_user_message: bool,
    update: F,
) -> Result<(Affect, agent_addressee_state::Model), anyhow::Error>
where
    F: FnOnce(&mut Affect) + Send,
{
    let transaction = db.begin().await?;
    lock_addressee(&transaction, user_id).await?;
    let state = get_or_create_state(&transaction, user_id).await?;
    let before = affect_from_state(&state);
    let mut after = before;
    update(&mut after);
    let saved = save_affect_on(&transaction, user_id, after, touch_user_message, false).await?;
    transaction.commit().await?;
    Ok((before, saved))
}

/// Credit one qualified listening block under the same per-addressee database
/// lock used by chat/task affect writes. Persisting the cooldown on the row
/// makes rapid events, multiple browser tabs and multiple backend replicas all
/// converge on one bounded mood change.
pub async fn credit_music_listening(
    db: &DatabaseConnection,
    user_id: i32,
    listened_seconds: u32,
) -> Result<MusicMoodCredit, anyhow::Error> {
    let transaction = db.begin().await?;
    lock_addressee(&transaction, user_id).await?;
    let state = get_or_create_state(&transaction, user_id).await?;
    let before = affect_from_state(&state);
    let now = Utc::now();

    if let Some(last) = state.music_mood_credited_at {
        let elapsed = (now - last.with_timezone(&Utc)).num_seconds().max(0);
        if elapsed < MUSIC_MOOD_CREDIT_COOLDOWN_SECS {
            transaction.commit().await?;
            return Ok(MusicMoodCredit {
                before,
                state,
                credited: false,
                next_credit_in_seconds: MUSIC_MOOD_CREDIT_COOLDOWN_SECS - elapsed,
            });
        }
    }

    let mut after = before;
    apply_music_listening(&mut after, listened_seconds);
    let saved = save_affect_on(&transaction, user_id, after, false, true).await?;
    transaction.commit().await?;
    Ok(MusicMoodCredit {
        before,
        state: saved,
        credited: true,
        next_credit_in_seconds: MUSIC_MOOD_CREDIT_COOLDOWN_SECS,
    })
}

async fn lock_addressee<C>(db: &C, user_id: i32) -> Result<(), anyhow::Error>
where
    C: ConnectionTrait,
{
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT pg_advisory_xact_lock($1, $2)",
        vec![1296388165_i32.into(), user_id.into()],
    ))
    .await?;
    Ok(())
}

/// One table, three sources.
///
/// The rows are the same shape, are created and deleted together, and are read
/// together (`list_diary_from_sources`), so a discriminator is the right split
/// and three tables would only buy a three-way union. What the sources do not
/// share is meaning: `remember` is a fact the user stated, while `event` and
/// `chat` are summaries the platform generated about them. Reads are therefore
/// always source-scoped — there is no "latest row of any kind" — so a stated
/// fact can never arrive somewhere expecting a generated summary.
pub const DIARY_SOURCE_EVENT: &str = "event";
pub const DIARY_SOURCE_CHAT: &str = "chat";
pub const DIARY_SOURCE_REMEMBER: &str = "remember";

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
    source: &str,
) -> Result<Option<agent_diary::Model>, anyhow::Error> {
    Ok(agent_diary::Entity::find()
        .filter(agent_diary::Column::UserId.eq(user_id))
        .filter(agent_diary::Column::Source.eq(source))
        .order_by_desc(agent_diary::Column::CreatedAt)
        .one(db)
        .await?)
}

pub async fn list_diary_from_sources(
    db: &DatabaseConnection,
    user_id: i32,
    sources: &[&str],
    limit: u64,
) -> Result<Vec<agent_diary::Model>, anyhow::Error> {
    if sources.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }
    Ok(agent_diary::Entity::find()
        .filter(agent_diary::Column::UserId.eq(user_id))
        .filter(agent_diary::Column::Source.is_in(sources.iter().copied()))
        .order_by_desc(agent_diary::Column::CreatedAt)
        .limit(limit)
        .all(db)
        .await?)
}

pub async fn list_remembered(
    db: &DatabaseConnection,
    user_id: i32,
    limit: u64,
) -> Result<Vec<agent_diary::Model>, anyhow::Error> {
    list_diary_from_sources(db, user_id, &[DIARY_SOURCE_REMEMBER], limit).await
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

pub async fn set_dnd_schedule(
    db: &DatabaseConnection,
    user_id: i32,
    start_minute: Option<i32>,
    end_minute: Option<i32>,
) -> Result<agent_addressee_state::Model, anyhow::Error> {
    let state = get_or_create_state(db, user_id).await?;
    if state.dnd_start_minute == start_minute && state.dnd_end_minute == end_minute {
        return Ok(state);
    }
    let mut active: agent_addressee_state::ActiveModel = state.into();
    active.dnd_start_minute = Set(start_minute);
    active.dnd_end_minute = Set(end_minute);
    active.updated_at = Set(Utc::now().into());
    Ok(active.update(db).await?)
}

pub async fn set_activity(
    db: &DatabaseConnection,
    user_id: i32,
    activity: &str,
) -> Result<agent_addressee_state::Model, anyhow::Error> {
    let transaction = db.begin().await?;
    lock_addressee(&transaction, user_id).await?;
    let state = get_or_create_state(&transaction, user_id).await?;
    if state.activity == activity {
        transaction.commit().await?;
        return Ok(state);
    }
    let mut active: agent_addressee_state::ActiveModel = state.into();
    let now = Utc::now().into();
    active.activity = Set(activity.to_string());
    active.activity_updated_at = Set(now);
    active.updated_at = Set(now);
    let saved = active.update(&transaction).await?;
    transaction.commit().await?;
    Ok(saved)
}

/// Keep the most active phase while more than one run is live. A second run
/// starting its talking phase must not downgrade another run already working.
pub async fn promote_activity(
    db: &DatabaseConnection,
    user_id: i32,
    activity: &str,
) -> Result<agent_addressee_state::Model, anyhow::Error> {
    let transaction = db.begin().await?;
    lock_addressee(&transaction, user_id).await?;
    let state = get_or_create_state(&transaction, user_id).await?;
    if activity_rank(activity) <= activity_rank(&state.activity) {
        transaction.commit().await?;
        return Ok(state);
    }
    let mut active: agent_addressee_state::ActiveModel = state.into();
    let now = Utc::now().into();
    active.activity = Set(activity.to_string());
    active.activity_updated_at = Set(now);
    active.updated_at = Set(now);
    let saved = active.update(&transaction).await?;
    transaction.commit().await?;
    Ok(saved)
}

fn activity_rank(activity: &str) -> u8 {
    match activity {
        "working" => 3,
        "thinking" => 2,
        "talking" => 1,
        _ => 0,
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{Database, TransactionTrait};
    use serde_json::json;

    #[test]
    fn postgres_duplicate_key_is_unique_conflict() {
        assert!(is_unique_conflict(
            &"error returned from database: 23505 duplicate key value violates unique constraint \"agent_addressee_state_pkey\""
        ));
        assert!(!is_unique_conflict(&"connection reset"));
        assert!(!is_unique_conflict(&"null value in column unique_id"));
    }

    #[test]
    fn concurrent_activity_only_moves_toward_the_busier_phase() {
        assert!(activity_rank("working") > activity_rank("thinking"));
        assert!(activity_rank("thinking") > activity_rank("talking"));
        assert!(activity_rank("talking") > activity_rank("idle"));
    }

    #[test]
    fn changing_generation_inputs_invalidates_portrait_contract() {
        let mut changed_visual_profile = json!({
            "gender": "female",
            "visualIdentity": {
                "faceDesign": "女性化读取，紧凑圆润鹅蛋脸",
                "eyeDesign": "中等偏大的紫色宝石眼，视线坚定",
                "hairShape": "银灰齐颌短发与偏分刘海",
                "hairLayerPlan": "后发、刘海和左右侧发形成独立轮廓",
                "upperBodySilhouette": "紧凑肩线、清楚领口与胸前焦点",
                "outfitConstruction": "敞开领口内搭叠短外套并止于高腰",
                "sleeveArmDesign": "左右袖片携局部前臂进入画面",
                "materialPlan": "哑光布料",
                "heroAccessory": "左胸星轨扣饰",
                "paletteHint": "雾蓝为主、银白为辅、金色点缀",
                "motif": "单一星轨弧线集中在胸前"
            }
        });
        let existing_visual_profile = changed_visual_profile.clone();
        changed_visual_profile["visualIdentity"]["hairShape"] = json!("银灰高马尾与偏分刘海");
        let existing = agent_persona::Model {
            id: PERSONA_ROW_ID.to_string(),
            name: "Arael".to_string(),
            personality: "quiet".to_string(),
            persona_json: Some(json!({ "summary": "quiet" })),
            visual_profile: Some(existing_visual_profile),
            portrait_asset_id: Some("/master.png".to_string()),
            portrait_generation: Some(json!({ "fingerprint": "a".repeat(64) })),
            updated_by: Some(1),
            updated_at: Utc::now().into(),
        };
        let active = apply_persona_update(
            existing,
            "Arael".to_string(),
            "quiet".to_string(),
            &PortraitUpdate::Keep,
            &PersonaContractUpdate {
                visual_profile: JsonDocumentUpdate::Set(changed_visual_profile),
                ..PersonaContractUpdate::default()
            },
            1,
        );
        assert_eq!(active.portrait_generation, Set(None));
        assert_eq!(active.portrait_asset_id, Set(None));
    }

    #[test]
    fn generation_inputs_changed_without_a_portrait() {
        let mut next = json!({
            "gender": "female",
            "visualIdentity": {
                "faceDesign": "女性化读取，紧凑圆润鹅蛋脸",
                "eyeDesign": "中等偏大的紫色宝石眼，视线坚定",
                "hairShape": "银灰齐颌短发与偏分刘海",
                "hairLayerPlan": "后发、刘海和左右侧发形成独立轮廓",
                "upperBodySilhouette": "紧凑肩线、清楚领口与胸前焦点",
                "outfitConstruction": "敞开领口内搭叠短外套并止于高腰",
                "sleeveArmDesign": "左右袖片携局部前臂进入画面",
                "materialPlan": "哑光布料",
                "heroAccessory": "左胸星轨扣饰",
                "paletteHint": "雾蓝为主、银白为辅、金色点缀",
                "motif": "单一星轨弧线集中在胸前"
            }
        });
        let existing = next.clone();
        next["visualIdentity"]["hairShape"] = json!("银灰高马尾与偏分刘海");
        assert!(generation_inputs_changed(
            "Arael",
            Some(&existing),
            "Arael",
            &JsonDocumentUpdate::Set(next),
        ));
        assert!(!generation_inputs_changed(
            "Arael",
            Some(&existing),
            "Arael",
            &JsonDocumentUpdate::Keep,
        ));
    }

    #[test]
    fn changing_spoken_persona_keeps_confirmed_visual_assets() {
        let existing = agent_persona::Model {
            id: PERSONA_ROW_ID.to_string(),
            name: "Arael".to_string(),
            personality: "quiet".to_string(),
            persona_json: Some(json!({ "summary": "quiet" })),
            visual_profile: Some(json!({ "gender": "unspecified" })),
            portrait_asset_id: Some("/master.png".to_string()),
            portrait_generation: Some(json!({ "fingerprint": "a".repeat(64) })),
            updated_by: Some(1),
            updated_at: Utc::now().into(),
        };
        let active = apply_persona_update(
            existing,
            "Arael".to_string(),
            "more curious".to_string(),
            &PortraitUpdate::Keep,
            &PersonaContractUpdate {
                persona: JsonDocumentUpdate::Set(json!({
                    "summary": "more curious"
                })),
                ..PersonaContractUpdate::default()
            },
            1,
        );
        assert_eq!(
            active.portrait_asset_id,
            sea_orm::ActiveValue::Unchanged(Some("/master.png".to_string()))
        );
        assert!(matches!(
            active.portrait_generation,
            sea_orm::ActiveValue::Unchanged(Some(_))
        ));
    }

    #[test]
    fn onboarding_seeds_do_not_invalidate_portrait() {
        let existing = agent_persona::Model {
            id: PERSONA_ROW_ID.to_string(),
            name: "Arael".to_string(),
            personality: "quiet".to_string(),
            persona_json: Some(json!({ "summary": "quiet" })),
            visual_profile: Some(json!({
                "gender": "unspecified",
                "visualIdentity": { "hairShape": "short bob" }
            })),
            portrait_asset_id: Some("/master.png".to_string()),
            portrait_generation: Some(json!({ "fingerprint": "a".repeat(64) })),
            updated_by: Some(1),
            updated_at: Utc::now().into(),
        };
        let active = apply_persona_update(
            existing,
            "Arael".to_string(),
            "quiet".to_string(),
            &PortraitUpdate::Keep,
            &PersonaContractUpdate {
                visual_profile: JsonDocumentUpdate::Set(json!({
                    "gender": "unspecified",
                    "language": "zh-CN",
                    "visualIdentity": { "hairShape": "short bob" },
                    "sourceTags": ["慢热"],
                    "personaExtraRequirements": "话少"
                })),
                ..PersonaContractUpdate::default()
            },
            1,
        );
        assert_eq!(
            active.portrait_asset_id,
            sea_orm::ActiveValue::Unchanged(Some("/master.png".to_string()))
        );
        assert!(matches!(
            active.portrait_generation,
            sea_orm::ActiveValue::Unchanged(Some(_))
        ));
    }

    #[tokio::test]
    async fn portrait_generation_lease_preserves_concurrent_persona_and_rejects_visual_change() {
        let Ok(database_url) = std::env::var("PORTRAIT_TEST_DATABASE_URL") else {
            return;
        };
        let db = Database::connect(database_url).await.unwrap();
        let transaction = db.begin().await.unwrap();
        agent_persona::Entity::delete_by_id(PERSONA_ROW_ID)
            .exec(&transaction)
            .await
            .unwrap();
        let profile = json!({
            "gender": "unspecified",
            "visualIdentity": { "hairShape": "short bob" }
        });
        agent_persona::ActiveModel {
            id: Set(PERSONA_ROW_ID.to_string()),
            name: Set("Nova".to_string()),
            personality: Set("quiet".to_string()),
            persona_json: Set(Some(json!({ "summary": "quiet" }))),
            visual_profile: Set(Some(profile.clone())),
            portrait_asset_id: Set(None),
            portrait_generation: Set(None),
            updated_by: Set(None),
            updated_at: Set(Utc::now().into()),
        }
        .insert(&transaction)
        .await
        .unwrap();

        let first_pending = json!({ "token": "first" });
        assert!(
            acquire_portrait_generation(&transaction, "Nova", &profile, &first_pending)
                .await
                .unwrap()
        );
        assert!(portrait_generation_is_pending(
            get_persona_on(&transaction)
                .await
                .unwrap()
                .unwrap()
                .portrait_generation
                .as_ref()
        ));
        assert!(!acquire_portrait_generation(
            &transaction,
            "Nova",
            &profile,
            &json!({ "token": "second" }),
        )
        .await
        .unwrap());

        upsert_persona_on(
            &transaction,
            "Nova".to_string(),
            "more curious".to_string(),
            PortraitUpdate::Keep,
            PersonaContractUpdate::default(),
            1,
        )
        .await
        .unwrap();
        assert!(complete_portrait_generation(
            &transaction,
            "Nova",
            &profile,
            "first",
            "/portrait.png",
            &json!({ "fingerprint": "a".repeat(64), "contract": {} }),
            1,
        )
        .await
        .unwrap());
        let saved = get_persona_on(&transaction).await.unwrap().unwrap();
        assert_eq!(saved.personality, "more curious");
        assert_eq!(saved.portrait_asset_id.as_deref(), Some("/portrait.png"));

        assert!(acquire_portrait_generation(
            &transaction,
            "Nova",
            &profile,
            &json!({ "token": "third" }),
        )
        .await
        .unwrap());
        let changed_profile = json!({
            "gender": "unspecified",
            "visualIdentity": { "hairShape": "long ponytail" }
        });
        upsert_persona_on(
            &transaction,
            "Nova".to_string(),
            "more curious".to_string(),
            PortraitUpdate::Keep,
            PersonaContractUpdate {
                visual_profile: JsonDocumentUpdate::Set(changed_profile),
                ..PersonaContractUpdate::default()
            },
            1,
        )
        .await
        .unwrap();
        assert!(!complete_portrait_generation(
            &transaction,
            "Nova",
            &profile,
            "third",
            "/stale.png",
            &json!({ "fingerprint": "b".repeat(64), "contract": {} }),
            1,
        )
        .await
        .unwrap());
        transaction.rollback().await.unwrap();
    }
}
