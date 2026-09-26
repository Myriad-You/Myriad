use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, Condition, ConnectionTrait, DatabaseBackend,
    DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, QuerySelect, Statement,
    TransactionTrait,
};
use serde_json::Value;
use uuid::Uuid;

use crate::models::entities::{
    agent_addressee_state, agent_diary, agent_persona, agent_proactive_messages, agent_sessions,
};
use crate::services::agent::memory::unified::Priming;

use super::state::{
    Affect, AffectBaseline, apply_music_listening, clamp, persona_affect_baseline, settle,
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

/// What a persona write does to the portrait. Absent `portrait_asset_id` is Keep.
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

/// Name or visual appearance changed: this write clears portrait and sticker avatar.
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
    // 贴纸头像的血统锚是主立绘。主立绘动了，那张 Q 版画的就不是这个人了——
    // 和作废 Rig 同一条理由，必须落在同一次写入里。
    if !matches!(portrait, PortraitUpdate::Keep) || inputs_changed {
        active.avatar_asset_id = Set(None);
        active.avatar_generation = Set(None);
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
        let saved = apply_persona_update(
            existing,
            name,
            personality,
            &portrait,
            &contract,
            updated_by,
        )
        .update(db)
        .await?;
        resync_persona_avatar_snapshots(db, saved.avatar_asset_id.as_deref()).await?;
        return Ok(saved);
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
        avatar_asset_id: Set(None),
        avatar_generation: Set(None),
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
/// portrait contract, if any. Another request's error path cannot unlock this token.
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
    avatar_asset_id = NULL,
    avatar_generation = NULL,
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
    let committed = result.rows_affected() == 1;
    if committed {
        // 新主立绘把旧贴纸头像一起作废了，选它的人不能停在旧脸上。
        resync_persona_avatar_snapshots(db, None).await?;
    }
    Ok(committed)
}

/// 贴纸头像的单次生成租约。和主立绘那把锁同一套形状，只是锚点多一个
/// `portrait_asset_id`——头像的血统在主立绘上，主立绘在生成途中被换掉，
/// 这批像素就已经作废了。崩溃的请求十五分钟后可被顶替。
pub async fn acquire_avatar_generation<C>(
    db: &C,
    expected_name: &str,
    expected_visual_profile: &Value,
    expected_portrait_asset_id: &str,
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
SET avatar_generation = jsonb_set(
        COALESCE(avatar_generation, '{}'::jsonb),
        '{pending}',
        $1::jsonb,
        true
    ),
    updated_at = CURRENT_TIMESTAMP
WHERE id = $2
  AND name = $3
  AND visual_profile = $4::jsonb
  AND portrait_asset_id = $5
  AND (
      avatar_generation IS NULL
      OR NOT (avatar_generation ? 'pending')
      OR updated_at < CURRENT_TIMESTAMP - INTERVAL '15 minutes'
  )
"#,
            vec![
                pending.clone().into(),
                PERSONA_ROW_ID.into(),
                expected_name.into(),
                expected_visual_profile.clone().into(),
                expected_portrait_asset_id.into(),
            ],
        ))
        .await?;
    Ok(result.rows_affected() == 1)
}

/// 只摘掉本次请求的锁，保住上一份已确认的头像契约。旧请求的错误路径永远
/// 解不开新请求的锁。
pub async fn release_avatar_generation<C>(db: &C, token: &str) -> Result<(), anyhow::Error>
where
    C: ConnectionTrait,
{
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"
UPDATE agent_persona
SET avatar_generation = CASE
        WHEN (avatar_generation - 'pending') = '{}'::jsonb THEN NULL
        ELSE avatar_generation - 'pending'
    END,
    updated_at = CURRENT_TIMESTAMP
WHERE id = $1
  AND avatar_generation #>> '{pending,token}' = $2
"#,
        vec![PERSONA_ROW_ID.into(), token.into()],
    ))
    .await?;
    Ok(())
}

/// URL normalization changes no visual inputs and must not invalidate generated art.
pub async fn rewrite_persona_media_urls<C: ConnectionTrait>(
    db: &C,
    persona: agent_persona::Model,
    portrait: Option<String>,
    avatar: Option<String>,
) -> Result<agent_persona::Model, anyhow::Error> {
    let mut active: agent_persona::ActiveModel = persona.into();
    active.portrait_asset_id = Set(portrait);
    active.avatar_asset_id = Set(avatar);
    let saved = active.update(db).await?;
    resync_persona_avatar_snapshots(db, saved.avatar_asset_id.as_deref()).await?;
    Ok(saved)
}

/// 只在本次请求仍持锁、且名字、外观与主立绘都没变时落盘。
pub async fn complete_avatar_generation<C>(
    db: &C,
    expected_name: &str,
    expected_visual_profile: &Value,
    expected_portrait_asset_id: &str,
    token: &str,
    avatar_asset_id: &str,
    avatar_generation: &Value,
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
SET avatar_asset_id = $1,
    avatar_generation = $2::jsonb,
    updated_by = $3,
    updated_at = CURRENT_TIMESTAMP
WHERE id = $4
  AND name = $5
  AND visual_profile = $6::jsonb
  AND portrait_asset_id = $7
  AND avatar_generation #>> '{pending,token}' = $8
"#,
            vec![
                avatar_asset_id.into(),
                avatar_generation.clone().into(),
                updated_by.into(),
                PERSONA_ROW_ID.into(),
                expected_name.into(),
                expected_visual_profile.clone().into(),
                expected_portrait_asset_id.into(),
                token.into(),
            ],
        ))
        .await?;
    let committed = result.rows_affected() == 1;
    if committed {
        resync_persona_avatar_snapshots(db, Some(avatar_asset_id)).await?;
    }
    Ok(committed)
}

pub fn avatar_generation_is_pending(value: Option<&Value>) -> bool {
    value
        .and_then(|document| document.get("pending"))
        .and_then(|pending| pending.get("token"))
        .and_then(Value::as_str)
        .is_some_and(|token| !token.is_empty())
}

/// 人设贴纸头像：`avatar_asset_id` trim 后空串视为无图。
pub async fn sticker_avatar_asset_id<C>(db: &C) -> Option<String>
where
    C: ConnectionTrait,
{
    get_persona_on(db)
        .await
        .ok()
        .flatten()
        .and_then(|persona| persona.avatar_asset_id)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// 贴纸头像动了就得把选它当画像源的人一起带上。列属于
/// [`crate::services::avatar`]，写入时机只有这里知道，所以由这里去调。
async fn resync_persona_avatar_snapshots<C>(
    db: &C,
    avatar: Option<&str>,
) -> Result<(), anyhow::Error>
where
    C: ConnectionTrait,
{
    crate::services::avatar::resync_persona_avatar_snapshots(db, avatar)
        .await
        .map_err(|message| anyhow::anyhow!(message))
}

pub fn portrait_generation_is_pending(value: Option<&Value>) -> bool {
    value
        .and_then(|document| document.get("pending"))
        .and_then(|pending| pending.get("token"))
        .and_then(Value::as_str)
        .is_some_and(|token| !token.is_empty())
}

/// Memory sources that belong to the persona, not to Work.
pub(crate) const PERSONA_MEMORY_SOURCES: [&str; 14] = [
    "chat",
    "event",
    "narrative",
    "lookup",
    crate::services::agent::memory::unified::OWN_EXPERIENCE,
    crate::services::agent::memory::unified::OWN_VIEW,
    "presence",
    "game",
    super::bits::SOURCE,
    super::strangers::SOURCE,
    super::threads::SOURCE,
    super::self_story::SOURCE,
    super::self_story::CORRECTED,
    super::explore::QUESTION,
];

pub async fn clear_persona_on<C>(db: &C) -> Result<(), anyhow::Error>
where
    C: ConnectionTrait,
{
    agent_proactive_messages::Entity::delete_many()
        .exec(db)
        .await?;
    agent_diary::Entity::delete_many().exec(db).await?;
    // Everything the persona learned or lived goes with her: what she heard
    // in conversation, what she looked up, played, saw them play, her days,
    // what she did on her own and her views. Work lessons stay.
    {
        use crate::models::entities::agent_memories::Column;
        crate::models::entities::agent_memories::Entity::delete_many()
            .filter(
                sea_orm::Condition::any()
                    .add(Column::Source.is_in(PERSONA_MEMORY_SOURCES))
                    .add(Column::Venue.eq(crate::services::agent::memory::unified::OWN_VENUE)),
            )
            .exec(db)
            .await?;
    }
    agent_addressee_state::Entity::delete_many()
        .exec(db)
        .await?;
    agent_persona::Entity::delete_by_id(PERSONA_ROW_ID)
        .exec(db)
        .await?;
    // How often she has talked with people outside the community.
    super::strangers::forget_counts(db).await?;
    // Turtle soups on now.
    super::soup::forget_games(db).await?;
    // The serial she followed and the books she finished or let go.
    super::serial::forget(db).await?;
    resync_persona_avatar_snapshots(db, None).await?;
    Ok(())
}

fn is_unique_conflict(err: &impl std::fmt::Display) -> bool {
    let lower = err.to_string().to_ascii_lowercase();
    lower.contains("23505") || lower.contains("duplicate key")
}

pub fn affect_baseline_from_persona_lookup(
    lookup: Result<Option<(Option<serde_json::Value>, String)>, anyhow::Error>,
) -> Result<AffectBaseline, anyhow::Error> {
    match lookup {
        Ok(Some((persona_json, personality))) => {
            Ok(persona_affect_baseline(persona_json.as_ref(), &personality))
        }
        Ok(None) => Ok(AffectBaseline::default()),
        Err(error) => Err(error),
    }
}

pub async fn load_affect_baseline<C>(db: &C) -> Result<AffectBaseline, anyhow::Error>
where
    C: ConnectionTrait,
{
    let lookup = get_persona_on(db)
        .await
        .map(|persona| persona.map(|row| (row.persona_json, row.personality)));
    affect_baseline_from_persona_lookup(lookup)
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

/// Overlay mood/emotion regression in memory. Does not write; activity staleness uses `activity_updated_at`.
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
    let base = load_affect_baseline(db).await?;
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

/// `state` is the row the caller already read (settled) under the addressee lock.
async fn save_affect_on<C>(
    db: &C,
    state: agent_addressee_state::Model,
    affect: Affect,
    touch_user_message: bool,
    touch_music_credit: bool,
) -> Result<agent_addressee_state::Model, anyhow::Error>
where
    C: ConnectionTrait,
{
    // Revisions travel as milliseconds. Consecutive writes must remain ordered
    // even within one clock tick (and across replicas under the same DB lock).
    let now = Utc::now()
        .max(
            state
                .updated_at
                .max(state.mood_settled_at)
                .with_timezone(&Utc)
                + chrono::Duration::milliseconds(1),
        )
        .into();
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
    let saved = save_affect_on(&transaction, state, after, touch_user_message, false).await?;
    transaction.commit().await?;
    Ok((before, saved))
}

/// A delayed interpretation belongs to one persisted input, not whichever
/// input happens to be current when the model finishes. The check and write
/// share the ordinary affect lock; an activity write is not a new input.
pub async fn update_utterance_appraisal<F>(
    db: &DatabaseConnection,
    user_id: i32,
    input_at: chrono::DateTime<chrono::FixedOffset>,
    update: F,
) -> Result<Option<(Affect, agent_addressee_state::Model)>, anyhow::Error>
where
    F: FnOnce(&mut Affect) + Send,
{
    let transaction = db.begin().await?;
    lock_addressee(&transaction, user_id).await?;
    let Some(state) = agent_addressee_state::Entity::find_by_id(user_id)
        .one(&transaction)
        .await?
    else {
        return Ok(None);
    };
    if !appraisal_is_current(state.last_user_message_at, input_at, Utc::now()) {
        return Ok(None);
    }
    let base = load_affect_baseline(&transaction).await?;
    let state = overlay_settled(state, base);
    let before = affect_from_state(&state);
    let mut after = before;
    update(&mut after);
    let saved = save_affect_on(&transaction, state, after, false, false).await?;
    transaction.commit().await?;
    Ok(Some((before, saved)))
}

pub(super) fn appraisal_is_current(
    latest_input: Option<chrono::DateTime<chrono::FixedOffset>>,
    expected_input: chrono::DateTime<chrono::FixedOffset>,
    now: chrono::DateTime<Utc>,
) -> bool {
    latest_input == Some(expected_input)
        && now.signed_duration_since(expected_input).num_milliseconds() <= 12_000
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
    let saved = save_affect_on(&transaction, state, after, false, true).await?;
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

/// One diary table, source-scoped reads. Superseded persona facts remain as
/// history but never participate in active recall.
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

pub async fn insert_diary<C: ConnectionTrait>(
    db: &C,
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

/// Event persona-memory insert. Dedup and the retraction check run under the
/// persona-memory lock, against the unified memory table.
pub(crate) async fn insert_remembered_if_new(
    db: &DatabaseConnection,
    user_id: i32,
    candidate: &str,
) -> Result<bool, anyhow::Error> {
    use crate::services::agent::memory::unified;
    let fact = super::ingest::compact_summary(candidate);
    if user_id <= 0 || fact.is_empty() {
        return Ok(false);
    }
    let transaction = db.begin().await?;
    lock_persona_memory(&transaction, user_id).await?;
    // Events may add facts, but cannot resurrect a fact the person retracted.
    // Only a new user assertion may re-establish it.
    if unified::retracted_by_person(&transaction, user_id, &fact).await? {
        transaction.commit().await?;
        return Ok(false);
    }
    let inserted = unified::remember(
        &transaction,
        unified::NewMemory {
            user_id,
            kind: unified::MemoryKind::Fact,
            content: fact,
            evidence: None,
            speaker: unified::Speaker::Agent,
            source: "event",
            audience: unified::Audience::private(user_id),
            importance: 0.5,
            concepts: Vec::new(),
        },
    )
    .await?
    .is_some();
    transaction.commit().await?;
    Ok(inserted)
}

async fn lock_persona_memory<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
) -> Result<(), sea_orm::DbErr> {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT pg_advisory_xact_lock($1, $2)",
        vec![1296388173_i32.into(), user_id.into()],
    ))
    .await?;
    Ok(())
}

/// Commit a validated extraction atomically. The input anchor is captured when
/// the utterance is persisted, before reply generation and model extraction.
/// Later activity/mood writes are not new inputs. A later user utterance is.
#[cfg(test)]
pub(crate) async fn apply_chat_memory_update(
    db: &DatabaseConnection,
    user_id: i32,
    input_at: chrono::DateTime<chrono::FixedOffset>,
    update: &super::chat_remember::ChatMemoryUpdate,
) -> Result<bool, anyhow::Error> {
    let present = crate::services::agent::memory::unified::Audience::private(user_id);
    apply_chat_memory_update_in(db, user_id, input_at, update, &present).await
}

/// [`apply_chat_memory_update`] for what was said in front of `present`: in a
/// group, the fact is kept for that group, and only facts the group heard can
/// be corrected there.
pub(crate) async fn apply_chat_memory_update_in(
    db: &DatabaseConnection,
    user_id: i32,
    input_at: chrono::DateTime<chrono::FixedOffset>,
    update: &super::chat_remember::ChatMemoryUpdate,
    present: &crate::services::agent::memory::unified::Audience,
) -> Result<bool, anyhow::Error> {
    if user_id <= 0 || (update.fact.is_none() && update.supersedes.is_empty()) {
        return Ok(false);
    }
    let transaction = db.begin().await?;
    // Always acquire in this order. Event-memory writers only take the second.
    lock_addressee(&transaction, user_id).await?;
    lock_persona_memory(&transaction, user_id).await?;
    if !chat_memory_input_is_current(&transaction, user_id, input_at).await? {
        transaction.commit().await?;
        return Ok(false);
    }
    use crate::services::agent::memory::unified;
    let mut targets = Vec::new();
    let mut found = std::collections::HashSet::new();
    let mut duplicate = false;
    for note in unified::active_in(
        &transaction,
        user_id,
        present,
        &unified::MemoryKind::ABOUT_PERSON,
    )
    .await?
    {
        let content = super::ingest::compact_summary(&note.content);
        if update.supersedes.contains(&content) {
            targets.push(note.id);
            found.insert(content);
        } else if update.fact.as_ref() == Some(&content) {
            duplicate = true;
        }
    }
    // Another extraction already replaced a target: reject the whole edit,
    // rather than appending an ungrounded new fact after a partial correction.
    if found.len() != update.supersedes.len() {
        transaction.commit().await?;
        return Ok(false);
    }
    unified::retire(&transaction, user_id, &targets, "superseded").await?;
    let insert = update.fact.as_ref().filter(|_| !duplicate);
    if let Some(fact) = insert {
        unified::remember(
            &transaction,
            unified::NewMemory {
                user_id,
                kind: unified::MemoryKind::Fact,
                content: fact.clone(),
                evidence: update.evidence.clone(),
                speaker: unified::Speaker::User,
                source: "chat",
                audience: present.clone(),
                importance: 0.6,
                concepts: update.concepts.clone(),
            },
        )
        .await?;
    }
    transaction.commit().await?;
    Ok(!targets.is_empty() || insert.is_some())
}

pub(crate) async fn chat_memory_input_is_current<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    input_at: chrono::DateTime<chrono::FixedOffset>,
) -> Result<bool, sea_orm::DbErr> {
    if user_id <= 0 {
        return Ok(false);
    }
    Ok(agent_addressee_state::Entity::find_by_id(user_id)
        .one(db)
        .await?
        .and_then(|state| state.last_user_message_at)
        == Some(input_at))
}

/// What the persona remembers about this person, most relevant to `query`
/// first (recent first without one). Private: only this person is present.
pub async fn recall_remembered(
    db: &DatabaseConnection,
    user_id: i32,
    query: Option<&str>,
    limit: usize,
) -> Result<Vec<String>, anyhow::Error> {
    let priming = Priming::default();
    let present = crate::services::agent::memory::unified::Audience::private(user_id);
    let (recalled, _) =
        recall_remembered_primed(db, user_id, &present, query, limit, &priming, 1.0).await?;
    Ok(recalled)
}

/// What a turn recalls, split: what they named (or recent context), and what
/// that brought to mind by association.
pub struct Recalled {
    pub named: Vec<String>,
    pub brought_to_mind: Vec<String>,
}

/// [`recall_remembered_primed`], keeping apart what was named and what it
/// brought to mind.
#[allow(clippy::too_many_arguments)]
pub async fn recall_remembered_split(
    db: &DatabaseConnection,
    user_id: i32,
    present: &crate::services::agent::memory::unified::Audience,
    query: Option<&str>,
    limit: usize,
    priming: &Priming,
    breadth: f64,
) -> Result<(Recalled, Priming), anyhow::Error> {
    use crate::services::agent::memory::unified;
    let (recalled, next) = unified::recall_primed(
        db,
        user_id,
        present,
        query.filter(|query| !query.trim().is_empty()),
        &unified::MemoryKind::ABOUT_PERSON,
        limit,
        priming,
        breadth,
    )
    .await?;
    let mut split = Recalled {
        named: Vec::new(),
        brought_to_mind: Vec::new(),
    };
    for note in recalled {
        let content = super::ingest::compact_summary(&note.content);
        if content.is_empty() {
            continue;
        }
        if note.brought_to_mind {
            split.brought_to_mind.push(content);
        } else {
            split.named.push(content);
        }
    }
    Ok((split, next))
}

/// [`recall_remembered`] for a chat turn: also starts from what the previous
/// turn left on the mind, and returns what this one leaves.
#[allow(clippy::too_many_arguments)]
pub async fn recall_remembered_primed(
    db: &DatabaseConnection,
    user_id: i32,
    present: &crate::services::agent::memory::unified::Audience,
    query: Option<&str>,
    limit: usize,
    priming: &Priming,
    breadth: f64,
) -> Result<(Vec<String>, Priming), anyhow::Error> {
    use crate::services::agent::memory::unified;
    let (recalled, next) = unified::recall_primed(
        db,
        user_id,
        present,
        query.filter(|query| !query.trim().is_empty()),
        &unified::MemoryKind::ABOUT_PERSON,
        limit,
        priming,
        breadth,
    )
    .await?;
    let recalled = recalled
        .into_iter()
        .map(|note| super::ingest::compact_summary(&note.content))
        .filter(|content| !content.is_empty())
        .collect();
    Ok((recalled, next))
}

#[cfg(test)]
#[path = "store_memory_tests.rs"]
mod memory_tests;

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
        // Her own lines to them go to a conversation of theirs, never a group.
        .filter(crate::api::agent::private_sessions())
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
    #[test]
    fn delayed_appraisal_requires_the_same_unexpired_persisted_input() {
        let input = chrono::Utc::now();
        let next = input + chrono::Duration::milliseconds(1);
        assert!(super::appraisal_is_current(
            Some(input.into()),
            input.into(),
            next
        ));
        assert!(!super::appraisal_is_current(
            Some(next.into()),
            input.into(),
            next
        ));
        assert!(!super::appraisal_is_current(None, input.into(), next));
        assert!(!super::appraisal_is_current(
            Some(input.into()),
            input.into(),
            input + chrono::Duration::seconds(13)
        ));
    }

    #[test]
    fn persona_lookup_error_is_not_default_baseline() {
        let err = super::affect_baseline_from_persona_lookup(Err(anyhow::anyhow!("db down")));
        assert!(
            err.is_err(),
            "DB failure must not become the default personality"
        );
        let missing = super::affect_baseline_from_persona_lookup(Ok(None)).unwrap();
        assert_eq!(missing, super::AffectBaseline::default());
    }

    #[tokio::test]
    #[ignore = "requires a disposable MEROPE_APPRAISAL_TEST_DATABASE_URL"]
    async fn appraisal_commit_rechecks_input_after_waiting_for_the_database_lock() {
        use sea_orm::{ConnectionTrait, Database, Schema, TransactionTrait};
        let url = std::env::var("MEROPE_APPRAISAL_TEST_DATABASE_URL").expect("disposable DB URL");
        let db = Database::connect(url).await.unwrap();
        let name = db
            .query_one_raw(sea_orm::Statement::from_string(
                sea_orm::DatabaseBackend::Postgres,
                "SELECT current_database() AS name",
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get::<String>("", "name")
            .unwrap();
        assert_eq!(
            name, "merope_appraisal_test",
            "refuse to create test tables in any other database"
        );
        let schema = Schema::new(sea_orm::DatabaseBackend::Postgres);
        for mut statement in [
            schema.create_table_from_entity(super::agent_persona::Entity),
            schema.create_table_from_entity(super::agent_addressee_state::Entity),
        ] {
            statement.if_not_exists();
            db.execute(&statement).await.unwrap();
        }
        let (_, first) = super::update_affect(&db, 7001, true, |_| {}).await.unwrap();
        let input_at = first.last_user_message_at.unwrap();
        super::set_activity(&db, 7001, "talking").await.unwrap();
        let (_, applied) =
            super::update_utterance_appraisal(&db, 7001, input_at, |affect| affect.mood += 1.0)
                .await
                .unwrap()
                .expect("activity is not a new input");
        assert!(applied.updated_at.timestamp_millis() > first.updated_at.timestamp_millis());
        assert_eq!(applied.last_user_message_at, Some(input_at));

        let transaction = db.begin().await.unwrap();
        super::lock_addressee(&transaction, 7001).await.unwrap();
        let mut late = Box::pin(super::update_utterance_appraisal(
            &db,
            7001,
            input_at,
            |affect| affect.mood = 0.0,
        ));
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(30), &mut late)
                .await
                .is_err()
        );
        let locked = super::get_or_create_state(&transaction, 7001)
            .await
            .unwrap();
        let second = super::save_affect_on(
            &transaction,
            locked,
            super::affect_from_state(&applied),
            true,
            false,
        )
        .await
        .unwrap();
        transaction.commit().await.unwrap();
        assert!(
            late.await.unwrap().is_none(),
            "the old result must be checked after acquiring the lock"
        );
        let current = super::get_or_create_state(&db, 7001).await.unwrap();
        assert!(current.mood > 60.0);
        assert_eq!(current.last_user_message_at, second.last_user_message_at);

        let mut previous = second;
        for _ in 0..16 {
            let (_, current) = super::update_affect(&db, 7001, true, |_| {}).await.unwrap();
            assert!(current.updated_at.timestamp_millis() > previous.updated_at.timestamp_millis());
            assert!(current.last_user_message_at > previous.last_user_message_at);
            previous = current;
        }
    }

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
            avatar_asset_id: None,
            avatar_generation: None,
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
            avatar_asset_id: None,
            avatar_generation: None,
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

    /// 一份完整到能通过 `appearance_visual_profile` 归一化的外观。
    /// 字段不全时归一化会整块丢掉 `visualIdentity`，改它就等于没改，
    /// 「改外观」这条断言会假绿。
    fn complete_visual_profile() -> Value {
        json!({
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
        })
    }

    fn persona_with_avatar() -> agent_persona::Model {
        agent_persona::Model {
            id: PERSONA_ROW_ID.to_string(),
            name: "Arael".to_string(),
            personality: "quiet".to_string(),
            persona_json: Some(json!({ "summary": "quiet" })),
            visual_profile: Some(complete_visual_profile()),
            portrait_asset_id: Some("/master.png".to_string()),
            portrait_generation: Some(json!({ "fingerprint": "a".repeat(64) })),
            avatar_asset_id: Some("/sticker.png".to_string()),
            avatar_generation: Some(json!({ "fingerprint": "b".repeat(64) })),
            updated_by: Some(1),
            updated_at: Utc::now().into(),
        }
    }

    /// 贴纸头像画的是主立绘上那个人。换主立绘还留着旧头像，站点上就会同时挂着
    /// 两张脸——和留着旧 Rig 是同一类错，必须在同一次写入里清掉。
    #[test]
    fn replacing_the_portrait_drops_the_sticker_avatar() {
        let active = apply_persona_update(
            persona_with_avatar(),
            "Arael".to_string(),
            "quiet".to_string(),
            &PortraitUpdate::Set("/uploaded.png".to_string()),
            &PersonaContractUpdate::default(),
            1,
        );
        assert_eq!(active.avatar_asset_id, Set(None));
        assert_eq!(active.avatar_generation, Set(None));
    }

    #[test]
    fn clearing_the_portrait_drops_the_sticker_avatar() {
        let active = apply_persona_update(
            persona_with_avatar(),
            "Arael".to_string(),
            "quiet".to_string(),
            &PortraitUpdate::Clear,
            &PersonaContractUpdate::default(),
            1,
        );
        assert_eq!(active.avatar_asset_id, Set(None));
        assert_eq!(active.avatar_generation, Set(None));
    }

    /// 外观变了主立绘会被作废，头像是从主立绘派生的，一起走。
    #[test]
    fn changing_the_appearance_drops_the_sticker_avatar() {
        let active = apply_persona_update(
            persona_with_avatar(),
            "Arael".to_string(),
            "quiet".to_string(),
            &PortraitUpdate::Keep,
            &PersonaContractUpdate {
                visual_profile: JsonDocumentUpdate::Set({
                    let mut next = complete_visual_profile();
                    next["visualIdentity"]["hairShape"] = json!("银灰高马尾与偏分刘海");
                    next
                }),
                ..PersonaContractUpdate::default()
            },
            1,
        );
        assert_eq!(active.portrait_asset_id, Set(None));
        assert_eq!(active.avatar_asset_id, Set(None));
    }

    /// 只改说话人格不动脸。头像跟着一起清掉的话，每次改性格都要重新烧一次图。
    #[test]
    fn changing_the_spoken_persona_keeps_the_sticker_avatar() {
        let active = apply_persona_update(
            persona_with_avatar(),
            "Arael".to_string(),
            "more curious".to_string(),
            &PortraitUpdate::Keep,
            &PersonaContractUpdate {
                persona: JsonDocumentUpdate::Set(json!({ "summary": "more curious" })),
                ..PersonaContractUpdate::default()
            },
            1,
        );
        assert_eq!(
            active.avatar_asset_id,
            sea_orm::ActiveValue::Unchanged(Some("/sticker.png".to_string()))
        );
    }

    /// 主立绘落盘的那条 SQL 也得清。它绕开 `apply_persona_update` 直接写库，
    /// 上面那几条断言管不到它。
    #[test]
    fn completing_a_portrait_generation_drops_the_sticker_avatar_in_the_same_write() {
        let source = include_str!("store.rs");
        let at = source
            .find("pub async fn complete_portrait_generation")
            .expect("complete_portrait_generation exists");
        let rest = &source[at..];
        // 切到下一个顶层函数为止，别按字节数硬截——中文注释会把切点落在字符中间。
        let end = rest[1..]
            .find("\npub ")
            .map(|offset| offset + 1)
            .unwrap_or(rest.len());
        let body = &rest[..end];
        assert!(
            body.contains("avatar_asset_id = NULL"),
            "落新主立绘却留着旧贴纸头像"
        );
        assert!(body.contains("avatar_generation = NULL"));
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
            avatar_asset_id: None,
            avatar_generation: None,
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
            avatar_asset_id: Set(None),
            avatar_generation: Set(None),
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
        assert!(
            !acquire_portrait_generation(
                &transaction,
                "Nova",
                &profile,
                &json!({ "token": "second" }),
            )
            .await
            .unwrap()
        );

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
        assert!(
            complete_portrait_generation(
                &transaction,
                "Nova",
                &profile,
                "first",
                "/portrait.png",
                &json!({ "fingerprint": "a".repeat(64), "contract": {} }),
                1,
            )
            .await
            .unwrap()
        );
        let saved = get_persona_on(&transaction).await.unwrap().unwrap();
        assert_eq!(saved.personality, "more curious");
        assert_eq!(saved.portrait_asset_id.as_deref(), Some("/portrait.png"));

        assert!(
            acquire_portrait_generation(
                &transaction,
                "Nova",
                &profile,
                &json!({ "token": "third" }),
            )
            .await
            .unwrap()
        );
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
        assert!(
            !complete_portrait_generation(
                &transaction,
                "Nova",
                &profile,
                "third",
                "/stale.png",
                &json!({ "fingerprint": "b".repeat(64), "contract": {} }),
                1,
            )
            .await
            .unwrap()
        );
        transaction.rollback().await.unwrap();
    }
}

#[cfg(test)]
mod persona_sources_tests {
    use super::PERSONA_MEMORY_SOURCES;

    /// Deleting the persona must take everything she learned or lived: a
    /// source a persona module writes but this list misses would survive
    /// into the next persona as if it were hers.
    #[test]
    fn every_source_the_persona_writes_goes_with_her() {
        let writers = [
            include_str!("curiosity.rs"),
            include_str!("playing.rs"),
            include_str!("soup.rs"),
            include_str!("chat_remember.rs"),
            include_str!("bits.rs"),
        ];
        for source in writers.iter().flat_map(|code| {
            code.match_indices("source: \"")
                .map(|(at, _)| {
                    let rest = &code[at + "source: \"".len()..];
                    &rest[..rest.find('"').unwrap_or(0)]
                })
                .collect::<Vec<_>>()
        }) {
            assert!(
                PERSONA_MEMORY_SOURCES.contains(&source),
                "persona source {source:?} would survive deleting her"
            );
        }
        assert!(
            !PERSONA_MEMORY_SOURCES.contains(&"work"),
            "Work lessons stay"
        );
    }
}
