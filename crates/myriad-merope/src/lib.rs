//! Pure Merope domain rules.
//!
//! Database repositories, model-provider calls, worker scheduling, and HTTP
//! adapters belong to the backend crate.

mod anime25d_contract;
mod onboarding;
mod outfit_overlay;
mod performance;
mod persona;
mod rig;
mod rig_contract;
mod rig_outfit;
mod rig_semantics;
mod rig_spatial;
mod rig_state;
mod speech_plan;
mod sticker_avatar;
mod visual_contract;
mod visual_design;
mod visual_prompt;

pub use onboarding::{sanitize_onboarding_tags, MAX_ONBOARDING_TAGS, MAX_ONBOARDING_TAG_CHARS};
pub use outfit_overlay::{
    format_chat_wardrobe_section, hold_incomplete_wear_marker, looks_from_visual_profile,
    resolve_chat_outfit_overlay, resolve_wear_directive, split_chat_wear_directive, wardrobe_look,
    wear_directive_after_reply, worn_outfit_id, OverlayDecision, WardrobeLook, WearDirective,
};
pub use performance::{
    parse_performance_plan, ChatPerformanceBaseline, ChatPerformanceCue, ChatPerformancePlan,
};
pub use persona::{fallback_persona_draft, persona_draft_is_complete, sanitize_persona_draft};
pub use rig::{
    compile_layered_rig, migrate_rig_manifest, validate_character_asset_source, RigBone,
    RigCompileSource, RigLayerSource, RigManifest, RigMotionProfile, RigOutfitProfile, RigPart,
    RigPoint, RigQuality, RigSemanticAnchor, RigSize, RigTexture, RigVertex, RIG_IR_VERSION,
    RIG_SCHEMA_VERSION,
};
pub use rig_contract::{
    CHARACTER_ASSET_CONTRACT_VERSION, PERFORMANCE_BASELINE_EXPRESSIONS, PERFORMANCE_CUE_INTENTS,
    PERFORMANCE_INTERRUPT_MODES, PERFORMANCE_PHRASE_INTENTS, PERFORMANCE_POSTURES,
    PORTRAIT_CANVAS_HEIGHT, PORTRAIT_CANVAS_WIDTH, PORTRAIT_GENERATION_HEIGHT,
    PORTRAIT_GENERATION_WIDTH,
};
pub use rig_semantics::RigSemantics;
pub use rig_spatial::RigSpatialProfile;
pub use rig_state::{
    cue_is_playable, cue_survives_state, motion_style_from_persona, motion_style_from_persona_json,
    plan_is_empty, refine_performance_plan, round_motion_style, sanitize_rig_state,
    RigStateSummary, MAX_RECENT_ACTIONS, RIG_STATE_CAPABILITIES, RIG_STATE_MOTION_STYLES,
    RIG_STATE_MOUTH_INTENTS, RIG_STATE_SPECIAL_INTENTS,
};
pub use speech_plan::{grounded_speech_phrases, SpeechPhrase};
pub use sticker_avatar::{
    build_sticker_avatar_contract, build_sticker_avatar_prompt,
    MEROPE_STICKER_STYLE_REFERENCE_SHA256, STICKER_AVATAR_CONTRACT_VERSION, STICKER_AVATAR_SIZE,
};
pub use visual_contract::{
    appearance_visual_profile, build_character_asset_contract, character_asset_contract_fingerprint,
};
pub use visual_design::{
    active_outfit_generation_fingerprint, active_outfit_rig_asset_id, bind_active_outfit_rig,
    character_module, clothing_style_grammar, clothing_style_of, detach_active_outfit_rig,
    ensure_default_wardrobe, flatten_visual_identity, normalize_clothing_style,
    reconcile_wardrobe_rigs, sanitize_outfit_module, sanitize_upper_body_visual_identity,
    sanitize_upper_body_visual_identity_checked, sanitize_wardrobe, sanitize_wardrobe_checked,
    stamp_clothing_style, upper_body_visual_identity_is_complete, VisualProfileIssue,
    VisualProfileReason, CLOTHING_STYLES, DEFAULT_WARDROBE_ID, MAX_VISUAL_NOTES_CHARS,
    MAX_WARDROBE_ID_CHARS, MAX_WARDROBE_ITEMS, UPPER_BODY_VISUAL_IDENTITY_FIELDS,
};
pub use visual_prompt::{
    build_character_visual_edit_prompt, build_character_visual_prompt,
    ensure_visual_identity_states_gender, normalize_visual_identity_for_prompt,
    normalize_visual_identity_for_prompt_checked,
    normalize_visual_requirements_for_design_with_gender, persona_has_literary_sludge,
    portrait_adjustment_is_within_scope, visual_identity_has_body_proportion_drift,
    visual_identity_has_camera_composition_drift, visual_identity_has_facial_construction_drift,
    visual_identity_has_high_collar, visual_identity_has_literary_sludge,
    visual_identity_matches_gender_presentation, visual_identity_violates_style_lock,
    MEROPE_STYLE_REFERENCE_SHA256, MEROPE_VISUAL_SCHOOL, MEROPE_VISUAL_SCHOOL_VERSION,
};
