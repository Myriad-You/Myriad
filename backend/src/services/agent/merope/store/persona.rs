//! Her persona row: reading, normalizing, updating and clearing it.

use super::*;

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

/// What a persona write does to the portrait. Absent `portrait_asset_id` is Keep.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortraitUpdate {
    Keep,
    Clear,
    Set(String),
}

impl PortraitUpdate {
    pub(super) fn stored(&self) -> Option<String> {
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

pub(super) fn apply_json_update(
    field: &mut sea_orm::ActiveValue<Option<Value>>,
    update: &JsonDocumentUpdate,
) {
    match update {
        JsonDocumentUpdate::Keep => {}
        JsonDocumentUpdate::Clear => *field = Set(None),
        JsonDocumentUpdate::Set(value) => *field = Set(Some(value.clone())),
    }
}

pub(super) fn visual_generation_inputs_changed(
    current: Option<&Value>,
    update: &JsonDocumentUpdate,
) -> bool {
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

pub(super) fn apply_persona_update(
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

/// Acquire before locking the persona row when a transaction may replace her.
pub(crate) async fn lock_persona_on(db: &impl ConnectionTrait) -> anyhow::Result<()> {
    memory_jobs::lock(db).await
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
    memory_jobs::lock(db).await?;
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

/// Memory sources that belong to the persona, not to Work.
pub(crate) const PERSONA_MEMORY_SOURCES: [&str; 23] = [
    "chat",
    "event",
    "narrative",
    "lookup",
    crate::services::agent::memory::unified::OWN_EXPERIENCE,
    crate::services::agent::memory::unified::OWN_VIEW,
    "presence",
    "game",
    super::super::bits::SOURCE,
    super::super::strangers::SOURCE,
    super::super::threads::SOURCE,
    super::super::self_story::SOURCE,
    super::super::self_story::CORRECTED,
    super::super::explore::QUESTION,
    // What she is to each person and each group, and what passed between
    // them: hers, not the next persona's.
    SAID_SOURCE,
    super::super::making_sense::SOURCE,
    super::super::sore::SOURCE,
    super::super::bits::US_SOURCE,
    super::super::bits::LANDS_SOURCE,
    super::super::bits::DAY_SOURCE,
    super::super::chat_days::SOURCE,
    super::super::others::SPOKE_UP,
    super::super::recognizing::SOURCE,
];

pub async fn clear_persona_on<C>(db: &C) -> Result<(), anyhow::Error>
where
    C: ConnectionTrait,
{
    memory_jobs::lock(db).await?;
    memory_jobs::forget(db).await?;
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
    super::super::strangers::forget_counts(db).await?;
    // Turtle soups on now.
    super::super::soup::forget_games(db).await?;
    // The serial she followed and the books she finished or let go.
    super::super::serial::forget(db).await?;
    // Her usual pace and the days she lazed.
    super::super::pace::forget(db).await?;
    // Her vital signs, day by day.
    super::super::vitals::forget(db).await?;
    // What she was in the middle of.
    super::super::doing::forget_kept(db).await?;
    // Her stickers are pictures of her.
    crate::models::entities::merope_stickers::Entity::delete_many()
        .exec(db)
        .await?;
    resync_persona_avatar_snapshots(db, None).await?;
    Ok(())
}

pub(super) fn is_unique_conflict(err: &impl std::fmt::Display) -> bool {
    let lower = err.to_string().to_ascii_lowercase();
    lower.contains("23505") || lower.contains("duplicate key")
}

#[cfg(test)]
pub(super) mod persona_sources_tests {
    use super::PERSONA_MEMORY_SOURCES;

    /// Deleting the persona must take everything she learned or lived: a
    /// source a persona module writes but this list misses would survive
    /// into the next persona as if it were hers.
    #[test]
    fn every_source_the_persona_writes_goes_with_her() {
        let writers = [
            include_str!("../curiosity.rs"),
            include_str!("../playing.rs"),
            include_str!("../soup.rs"),
            include_str!("../chat_remember.rs"),
            include_str!("../bits.rs"),
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
        // Whatever is kept apart from ordinary memory is hers alone.
        for source in crate::services::agent::memory::unified::KEPT_APART {
            assert!(
                PERSONA_MEMORY_SOURCES.contains(&source),
                "kept-apart source {source:?} would survive deleting her"
            );
        }
    }
}
