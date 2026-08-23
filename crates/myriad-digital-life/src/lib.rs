//! Pure Digital Life Companion domain rules.
//!
//! Database repositories, model-provider calls, worker scheduling, and HTTP
//! adapters belong to the backend crate.

mod decision;
mod fingerprint;
mod memory;
mod onboarding;
mod performance;
mod persona;
mod policy;
mod prompt;
mod quality;
mod report_dna;
mod rig;
mod rig_contract;
mod rig_outfit;
mod rig_semantics;
mod rig_spatial;
mod runtime;
mod safety;
mod visual_prompt;
mod visual_contract;
mod visual_design;

pub use decision::{
    apply_decision, parse_and_validate_decision, AppliedDecision, DecisionError, LiteDecision,
    SpeakGate, ValidatedDecision, DECISION_SCHEMA_VERSION,
};
pub use fingerprint::{companion_fingerprint, FingerprintInput};
pub use memory::{rank_memories, MemoryCandidate, ScoredMemory};
pub use onboarding::{
    sanitize_onboarding_tags, MAX_ONBOARDING_TAGS, MAX_ONBOARDING_TAG_CHARS,
};
pub use performance::{
    parse_chat_performance, parse_performance_plan, ChatPerformanceBaseline, ChatPerformanceCue,
    ChatPerformancePlan, ParsedChatPerformance,
};
pub use persona::{
    fallback_persona_draft, persona_draft_is_complete, sanitize_persona_draft,
};
pub use policy::{AutonomyFrequency, CompanionPolicy, FrequencyProfile};
pub use prompt::{
    build_chat_system_prompt, build_consolidation_prompt, build_deliberation_prompt, PromptMemory,
};
pub use quality::{
    chat_quality_guidance, deliberation_quality_guidance, deterministic_consolidation_note,
    deterministic_fallback_decision, is_low_quality_speak, preferred_consolidate_tier,
    score_memory_importance, select_autonomy_model_tier, select_chat_tier, select_deliberate_tier,
    should_desire_proactive_speak, should_enqueue_consolidate, AutonomyModelTier,
    DeliberateTierContext, ProEscalationContext, MAX_PRO_CONSOLIDATES_PER_DAY,
    MAX_PRO_DELIBERATES_PER_DAY, MAX_STANDARD_DELIBERATES_PER_DAY, MIN_HOURS_BETWEEN_PRO_CONSOLIDATES,
    MIN_HOURS_BETWEEN_PRO_DELIBERATES, MIN_HOURS_BETWEEN_STANDARD_DELIBERATES,
};
pub use report_dna::{
    build_report_dna_bundle, complete_ai_tag_deck, fallback_tag_deck, is_reasonable_persona_tag,
    localize_report_seed_keys, looks_like_job_or_identity_label, looks_like_media_catalog_label,
    report_dna_json, sample_tag_deck, sanitize_report_dna_tags, seed_shuffle, ReportDnaBundle,
    ReportDnaEvidence, ReportDnaProvenance, ReportDnaSource, MAX_REPORT_DNA_REPORTS,
    MAX_REPORT_INSIGHT_CHARS, MAX_REPORT_NOTE_CHARS, MAX_REPORT_SUMMARY_CHARS, PERSONA_POOL_KEYS,
};
pub use rig::{
    compile_layered_rig, default_rig_motion_profile,
    infer_outfit_profile, migrate_rig_manifest, validate_character_asset_source,
    RigBlinkMotionProfile, RigBone, RigBoneHandle,
    RigBreathMotionProfile, RigCompileError, RigCompileSource,
    RigLayerMeshSource, RigLayerSource, RigManifest,
    RigMotionProfile, RigOutfitProfile, RigOutfitTopology, RigPart, RigPoint,
    RigQuality,
    RigRect, RigSecondaryMotionProfile, RigSemanticAnchor, RigSize, RigTexture,
    RigValidationError, RigVertex, MAX_RIG_BONES,
    MAX_RIG_COLLISION_VOLUMES, MAX_RIG_PARTS,
    MAX_RIG_TEXTURES, MAX_RIG_TOTAL_VERTICES, MAX_RIG_VERTICES_PER_PART,
    MIN_SUPPORTED_RIG_IR_VERSION, RIG_IR_VERSION, RIG_SCHEMA_VERSION,
};
pub use rig_semantics::RigSemantics;
pub use rig_spatial::{RigCollisionVolume, RigSpatialProfile};
pub use rig_contract::{
    CHARACTER_ASSET_CONTRACT_VERSION, CHARACTER_ASSET_REQUIRED_CAPABILITIES,
    MAX_RIGID_ARM_ROTATION_DEGREES, PORTRAIT_ASPECT_HEIGHT, PORTRAIT_ASPECT_WIDTH,
    PORTRAIT_CANVAS_HEIGHT, PORTRAIT_CANVAS_WIDTH, PORTRAIT_GENERATION_HEIGHT,
    PORTRAIT_GENERATION_WIDTH,
};
pub use runtime::{
    apply_chat_message_buff, apply_life_event, catch_up, should_apply_chat_idle, should_deliberate,
    Activity, CatchUpResult, CharacterStatus, DeliberationContext, LifeEvent, RuntimeState,
    CHAT_IDLE_AFTER_MINUTES,
};
pub use safety::is_safe_companion_output;
pub use visual_prompt::{
    body_proportion_drift_in, build_character_visual_edit_prompt, build_character_visual_prompt,
    camera_composition_drift_in, facial_construction_drift_in, literary_sludge_in,
    normalize_visual_identity_for_prompt, normalize_visual_requirements_for_design,
    normalize_visual_requirements_for_design_with_gender, persona_has_literary_sludge,
    persona_literary_sludge_in,
    portrait_adjustment_changes_identity, portrait_adjustment_is_within_scope,
    style_lock_violation_in, visual_identity_has_body_proportion_drift,
    visual_identity_has_camera_composition_drift,
    visual_identity_has_facial_construction_drift, visual_identity_has_literary_sludge,
    visual_identity_matches_gender_presentation, visual_identity_violates_style_lock,
    COMPANION_STYLE_REFERENCE_SHA256, COMPANION_VISUAL_SCHOOL,
    COMPANION_VISUAL_SCHOOL_VERSION,
};
pub use visual_contract::{
    appearance_visual_profile, build_character_asset_contract,
    character_asset_contract_fingerprint,
};
pub use visual_design::{
    character_module, clothing_style_grammar, clothing_style_of, flatten_visual_identity,
    normalize_clothing_style, outfit_module, sanitize_upper_body_visual_identity,
    stamp_clothing_style, upper_body_visual_identity_is_complete, CHARACTER_VISUAL_FIELDS,
    CLOTHING_STYLES, OUTFIT_VISUAL_FIELDS, UPPER_BODY_VISUAL_IDENTITY_FIELDS,
};

pub const MIN_TICK_SECONDS: u64 = 15;
pub const MAX_TICK_SECONDS: u64 = 3_600;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeatureRuntimeConfig {
    pub enabled: bool,
    pub worker_enabled: bool,
    pub tick_seconds: u64,
}

impl FeatureRuntimeConfig {
    pub fn new(enabled: bool, worker_requested: bool, tick_seconds: u64) -> Self {
        Self {
            enabled,
            worker_enabled: enabled && worker_requested,
            tick_seconds: tick_seconds.clamp(MIN_TICK_SECONDS, MAX_TICK_SECONDS),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_feature_never_reports_worker_active() {
        let config = FeatureRuntimeConfig::new(false, true, 2);
        assert!(!config.worker_enabled);
        assert_eq!(config.tick_seconds, MIN_TICK_SECONDS);
    }
}
