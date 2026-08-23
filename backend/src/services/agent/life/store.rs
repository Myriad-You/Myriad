use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseBackend,
    DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, QuerySelect, Statement,
};
use serde_json::Value;
use uuid::Uuid;

use crate::models::entities::{
    agent_addressee_state, agent_diary, agent_persona, agent_proactive_messages, agent_sessions,
};

pub const PERSONA_ROW_ID: &str = "site";

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

fn visual_generation_inputs_changed(
    current: Option<&Value>,
    update: &JsonDocumentUpdate,
) -> bool {
    match update {
        JsonDocumentUpdate::Keep => false,
        JsonDocumentUpdate::Clear => current.is_some(),
        JsonDocumentUpdate::Set(value) => {
            current.map(myriad_digital_life::appearance_visual_profile).as_ref()
                != Some(&myriad_digital_life::appearance_visual_profile(value))
        }
    }
}

fn apply_persona_update(
    existing: agent_persona::Model,
    name: String,
    personality: String,
    portrait: &PortraitUpdate,
    contract: &PersonaContractUpdate,
    updated_by: i32,
) -> agent_persona::ActiveModel {
    let generation_inputs_changed = existing.name != name
        || visual_generation_inputs_changed(
            existing.visual_profile.as_ref(),
            &contract.visual_profile,
        );
    let mut active: agent_persona::ActiveModel = existing.into();
    active.name = Set(name);
    active.personality = Set(personality);
    match portrait {
        PortraitUpdate::Keep if generation_inputs_changed => {
            active.portrait_asset_id = Set(None)
        }
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
    if (!matches!(portrait, PortraitUpdate::Keep) || generation_inputs_changed)
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
        return Ok(
            apply_persona_update(existing, name, personality, &portrait, &contract, updated_by)
                .update(db)
                .await?,
        );
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
            Ok(
                apply_persona_update(existing, name, personality, &portrait, &contract, updated_by)
                    .update(db)
                    .await?,
            )
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
        dnd_start_minute: Set(None),
        dnd_end_minute: Set(None),
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
    fn changing_generation_inputs_invalidates_portrait_contract() {
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
            "quiet".to_string(),
            &PortraitUpdate::Keep,
            &PersonaContractUpdate {
                visual_profile: JsonDocumentUpdate::Set(json!({
                    "gender": "unspecified",
                    "visualIdentity": { "hairShape": "short bob" }
                })),
                ..PersonaContractUpdate::default()
            },
            1,
        );
        assert_eq!(active.portrait_generation, Set(None));
        assert_eq!(active.portrait_asset_id, Set(None));
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
