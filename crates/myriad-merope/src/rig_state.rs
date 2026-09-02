//! Semantic live-face summary for the Lite motion director.
//!
//! No bones, coordinates, angles, raw drivers, frames, or pixels.

use serde::{Deserialize, Serialize};

use crate::rig_contract::{
    PERFORMANCE_BASELINE_EXPRESSIONS, PERFORMANCE_CUE_INTENTS, PERFORMANCE_POSTURES,
};
use crate::ChatPerformancePlan;

pub const RIG_STATE_CHANNEL_OWNERS: &[&str] = &[
    "preview",
    "speech",
    "performance",
    "music",
    "coSpeech",
    "mood",
    "pointer",
    "ambient",
    "idle",
];
pub const RIG_STATE_SPECIAL_INTENTS: &[&str] = &[
    "dizzy",
    "cry",
    "angry",
    "speechless",
    "maniac",
    "silly",
    "lovestruck",
];
/// Cue intents that occupy the mouth. Capability names (`cry-mouth`) are not
/// intents; substring matching them would never fire on a parsed plan.
pub const RIG_STATE_MOUTH_INTENTS: &[&str] = &["cry", "maniac", "silly"];
pub const RIG_STATE_HEAD_BODY_INTENTS: &[&str] = &[
    "greet",
    "respond",
    "question",
    "delight",
    "emphasize",
    "listen",
    "notify",
    "think",
    "angry",
    "speechless",
    "maniac",
    "silly",
    "lovestruck",
];
pub const RIG_STATE_CAPABILITIES: &[&str] = &[
    "blink",
    "independent-eyes",
    "dizzy-eye",
    "squeeze-eye",
    "cry-eye",
    "silly-eye",
    "lovestruck",
    "cry-mouth",
    "maniac-mouth",
    "silly-mouth",
    "mouth-shapes",
    "head-body",
];
pub const RIG_STATE_MUSIC_ENERGIES: &[&str] = &["quiet", "soft", "present", "strong"];
pub const RIG_STATE_BEAT_PHASES: &[&str] = &["rest", "downbeat", "pulse", "hold"];
pub const RIG_STATE_MOTION_STYLES: &[&str] = &["restrained", "even", "open"];
/// What a behavior means, as the client is able to report it.
///
/// Every name here has a producer: the cue compiler, the speech prosody
/// compiler, or the music entrainment source. Words without one described a
/// richer body than exists and would never reach the director, so the
/// front-end contract test fails when this list and the producers disagree.
pub const RIG_STATE_BEHAVIOR_FUNCTIONS: &[&str] = &[
    "orient",
    "attend",
    "acknowledge",
    "uncertain",
    "prepareSpeech",
    "emphasize",
    "surprise",
    "celebrate",
    "relief",
    "entrain",
    "express",
];
pub const RIG_STATE_BEHAVIOR_PHASES: &[&str] = &[
    "planned",
    "preparing",
    "committed",
    "holding",
    "recovering",
    "complete",
    "rejected",
];
/// Mood and ambient hold leases; neither publishes a behavior, so neither can
/// appear on one.
pub const RIG_STATE_BEHAVIOR_SOURCES: &[&str] = &["performance", "coSpeech", "music"];
pub const RIG_STATE_BEHAVIOR_RESOURCES: &[&str] = &[
    "face.mouth",
    "face.expression",
    "face.gaze",
    "body.head",
    "body.torso",
    "body.arm.left",
    "body.arm.right",
    "body.hand.left",
    "body.hand.right",
    "secondary.hair",
    "secondary.clothing",
    "secondary.bust",
];
pub const MAX_RECENT_ACTIONS: usize = 6;
pub const MAX_ACTIVE_BEHAVIORS: usize = 8;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RigActingSummary {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<String>,
    pub phase: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifecycle: Option<String>,
    pub remaining_ms: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RigActiveBehavior {
    pub function: String,
    pub lifecycle: String,
    pub source: String,
    #[serde(default)]
    pub resources: Vec<String>,
    pub remaining_ms: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RigChannelOwners {
    pub mouth: String,
    pub expression: String,
    pub gaze: String,
    pub head_body: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RigMusicSummary {
    pub energy: String,
    pub beat: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RigStateSummary {
    pub expression: String,
    pub posture: String,
    pub acting: RigActingSummary,
    #[serde(default)]
    pub active_behaviors: Vec<RigActiveBehavior>,
    pub owners: RigChannelOwners,
    pub speaking: bool,
    pub singing: bool,
    pub music_playing: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub music: Option<RigMusicSummary>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub recent_intents: Vec<String>,
    pub motion_style: String,
    pub page_visible: bool,
    pub face_visible: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawSummary {
    #[serde(default)]
    expression: Option<String>,
    #[serde(default)]
    posture: Option<String>,
    #[serde(default)]
    acting: Option<RawActing>,
    #[serde(default)]
    active_behaviors: Vec<RawActiveBehavior>,
    #[serde(default)]
    owners: Option<RawOwners>,
    #[serde(default)]
    speaking: bool,
    #[serde(default)]
    singing: bool,
    #[serde(default)]
    music_playing: bool,
    #[serde(default)]
    music: Option<RawMusic>,
    #[serde(default)]
    capabilities: Vec<String>,
    #[serde(default)]
    recent_intents: Vec<String>,
    #[serde(default)]
    motion_style: Option<String>,
    #[serde(default = "default_true")]
    page_visible: bool,
    #[serde(default = "default_true")]
    face_visible: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawActing {
    #[serde(default)]
    intent: Option<String>,
    #[serde(default)]
    phase: Option<String>,
    #[serde(default)]
    function: Option<String>,
    #[serde(default)]
    lifecycle: Option<String>,
    #[serde(default)]
    remaining_ms: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawActiveBehavior {
    #[serde(default)]
    function: Option<String>,
    #[serde(default)]
    lifecycle: Option<String>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    resources: Vec<String>,
    #[serde(default)]
    remaining_ms: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawOwners {
    #[serde(default)]
    mouth: Option<String>,
    #[serde(default)]
    expression: Option<String>,
    #[serde(default)]
    gaze: Option<String>,
    #[serde(default)]
    head_body: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawMusic {
    #[serde(default)]
    energy: Option<String>,
    #[serde(default)]
    beat: Option<String>,
}

fn default_true() -> bool {
    true
}

/// Drops bones, drivers, frames, and unknown vocabulary. Missing fields idle out.
pub fn sanitize_rig_state(value: &serde_json::Value) -> Option<RigStateSummary> {
    let raw: RawSummary = serde_json::from_value(value.clone()).ok()?;
    Some(sanitize_raw(raw))
}

fn sanitize_raw(raw: RawSummary) -> RigStateSummary {
    let expression = allow(
        raw.expression.as_deref(),
        PERFORMANCE_BASELINE_EXPRESSIONS,
        "steady",
    );
    let posture = allow(raw.posture.as_deref(), PERFORMANCE_POSTURES, "neutral");
    let acting_intent = raw
        .acting
        .as_ref()
        .and_then(|acting| acting.intent.as_deref())
        .filter(|intent| PERFORMANCE_CUE_INTENTS.contains(intent))
        .map(str::to_string);
    let phase = allow(
        raw.acting
            .as_ref()
            .and_then(|acting| acting.phase.as_deref()),
        &[
            "reaction",
            "delivery",
            "outcome",
            "proactive",
            "mood",
            "idle",
        ],
        "idle",
    );
    let remaining_ms = raw
        .acting
        .as_ref()
        .map(|acting| acting.remaining_ms.min(12_000))
        .unwrap_or(0);
    let behavior_function = raw
        .acting
        .as_ref()
        .and_then(|acting| acting.function.as_deref())
        .filter(|value| RIG_STATE_BEHAVIOR_FUNCTIONS.contains(value))
        .map(str::to_string);
    let behavior_lifecycle = raw
        .acting
        .as_ref()
        .and_then(|acting| acting.lifecycle.as_deref())
        .filter(|value| RIG_STATE_BEHAVIOR_PHASES.contains(value))
        .map(str::to_string);
    let owners = RigChannelOwners {
        mouth: owner(
            raw.owners
                .as_ref()
                .and_then(|owners| owners.mouth.as_deref()),
        ),
        expression: owner(
            raw.owners
                .as_ref()
                .and_then(|owners| owners.expression.as_deref()),
        ),
        gaze: owner(
            raw.owners
                .as_ref()
                .and_then(|owners| owners.gaze.as_deref()),
        ),
        head_body: owner(
            raw.owners
                .as_ref()
                .and_then(|owners| owners.head_body.as_deref()),
        ),
    };
    let active_behaviors = raw
        .active_behaviors
        .into_iter()
        .filter_map(|behavior| {
            let function = behavior
                .function
                .filter(|value| RIG_STATE_BEHAVIOR_FUNCTIONS.contains(&value.as_str()))?;
            let lifecycle = behavior
                .lifecycle
                .filter(|value| RIG_STATE_BEHAVIOR_PHASES.contains(&value.as_str()))?;
            let source = behavior
                .source
                .filter(|value| RIG_STATE_BEHAVIOR_SOURCES.contains(&value.as_str()))?;
            Some(RigActiveBehavior {
                function,
                lifecycle,
                source,
                resources: behavior
                    .resources
                    .into_iter()
                    .filter(|resource| RIG_STATE_BEHAVIOR_RESOURCES.contains(&resource.as_str()))
                    .take(8)
                    .collect(),
                remaining_ms: behavior.remaining_ms.min(12_000),
            })
        })
        .take(MAX_ACTIVE_BEHAVIORS)
        .collect();
    let music = raw.music.and_then(|music| {
        let energy = allow(music.energy.as_deref(), RIG_STATE_MUSIC_ENERGIES, "");
        let beat = allow(music.beat.as_deref(), RIG_STATE_BEAT_PHASES, "");
        (energy != "quiet" || beat != "rest" || raw.music_playing).then_some(RigMusicSummary {
            energy: if energy.is_empty() {
                "quiet".to_string()
            } else {
                energy
            },
            beat: if beat.is_empty() {
                "rest".to_string()
            } else {
                beat
            },
        })
    });
    RigStateSummary {
        expression,
        posture,
        acting: RigActingSummary {
            intent: acting_intent,
            phase,
            function: behavior_function,
            lifecycle: behavior_lifecycle,
            remaining_ms,
        },
        active_behaviors,
        owners,
        speaking: raw.speaking,
        singing: raw.singing,
        music_playing: raw.music_playing,
        music,
        capabilities: raw
            .capabilities
            .into_iter()
            .filter(|cap| RIG_STATE_CAPABILITIES.contains(&cap.as_str()))
            .take(RIG_STATE_CAPABILITIES.len())
            .collect(),
        recent_intents: raw
            .recent_intents
            .into_iter()
            .filter(|intent| PERFORMANCE_CUE_INTENTS.contains(&intent.as_str()))
            .take(MAX_RECENT_ACTIONS)
            .collect(),
        motion_style: allow(raw.motion_style.as_deref(), RIG_STATE_MOTION_STYLES, "even"),
        page_visible: raw.page_visible,
        face_visible: raw.face_visible,
    }
}

fn owner(value: Option<&str>) -> String {
    allow(value, RIG_STATE_CHANNEL_OWNERS, "idle")
}

fn allow(value: Option<&str>, allowed: &[&str], fallback: &str) -> String {
    value
        .filter(|item| allowed.contains(item))
        .unwrap_or(fallback)
        .to_string()
}

pub fn plan_is_empty(plan: &ChatPerformancePlan) -> bool {
    plan.baseline.is_none() && plan.cues.is_empty()
}

/// Client `motionStyle` is a fallback. A loaded persona always wins.
pub fn round_motion_style(
    client_style: Option<&str>,
    persona_json: Option<&serde_json::Value>,
    persona_found: bool,
    mood: i32,
    arousal: i32,
) -> String {
    if persona_found {
        return motion_style_from_persona_json(persona_json, mood, arousal).to_string();
    }
    allow(client_style, RIG_STATE_MOTION_STYLES, "even")
}

pub fn motion_style_from_persona_json(
    persona_json: Option<&serde_json::Value>,
    mood: i32,
    arousal: i32,
) -> &'static str {
    let temperament = persona_json
        .and_then(|value| value.get("temperament"))
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let social = persona_json
        .and_then(|value| value.get("socialStyle"))
        .and_then(|value| value.as_str())
        .unwrap_or("");
    motion_style_from_persona(&temperament, social, mood, arousal)
}

pub fn motion_style_from_persona(
    temperament: &[String],
    social_style: &str,
    mood: i32,
    arousal: i32,
) -> &'static str {
    let blob = format!("{} {}", temperament.join(" "), social_style).to_lowercase();
    if mood <= 40
        || contains_any(
            &blob,
            &[
                "克制",
                "慢热",
                "内向",
                "quiet",
                "reserved",
                "shy",
                "restrained",
            ],
        )
    {
        return "restrained";
    }
    if (mood >= 75 && arousal >= 55)
        || contains_any(
            &blob,
            &[
                "活泼", "开放", "外向", "bright", "playful", "open", "cheerful",
            ],
        )
    {
        return "open";
    }
    "even"
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

/// Drop cues the current face cannot play.
///
/// Sticker expressions still need their layers. Generic acting is allowed when
/// `capabilities` is empty so a missing summary does not wipe the face. Callers
/// that have no `rigState` at all skip this function so old clients keep prior
/// behavior. Runtime resource conflicts belong to the behavior scheduler: the
/// backend must not silently erase a replace/queue decision just because music
/// currently owns the coarse body channel.
pub fn refine_performance_plan(
    mut plan: ChatPerformancePlan,
    state: &RigStateSummary,
) -> ChatPerformancePlan {
    plan.cues
        .retain(|cue| cue_survives_state(state, &cue.intent));
    plan
}

/// Whether this cue would survive `refine_performance_plan` for this face.
///
/// The director's own schema enum and its cue index are built from this too,
/// so it is never offered something the backend is about to delete. Offering
/// it was not free: a plan whose only cue was unplayable came back empty, and
/// an empty plan drops the whole Lite refinement for that round.
pub fn cue_survives_state(state: &RigStateSummary, intent: &str) -> bool {
    capability_allows(&state.capabilities, intent)
        && !cue_blocked_by_speech(intent, state.speaking, &state.capabilities)
}

fn has_cap(capabilities: &[String], name: &str) -> bool {
    capabilities.iter().any(|cap| cap == name)
}

/// Speech keeps the articulating mouth. Maniac needs its authored mouth, so it waits.
/// Silly/cry still play through the eyes; the client yields their mouth layers.
fn cue_blocked_by_speech(intent: &str, speaking: bool, capabilities: &[String]) -> bool {
    if !speaking {
        return false;
    }
    match intent {
        "maniac" => true,
        "silly" => !has_cap(capabilities, "silly-eye"),
        "cry" => !has_cap(capabilities, "cry-eye"),
        _ => false,
    }
}

/// Sticker cues need their layer; generic acting needs `head-body` when the
/// capability list is present.
pub fn cue_is_playable(capabilities: &[String], intent: &str) -> bool {
    capability_allows(capabilities, intent)
}

fn capability_allows(capabilities: &[String], intent: &str) -> bool {
    match intent {
        "dizzy" => has_cap(capabilities, "dizzy-eye"),
        "cry" => has_cap(capabilities, "cry-eye") || has_cap(capabilities, "cry-mouth"),
        "silly" => has_cap(capabilities, "silly-eye") || has_cap(capabilities, "silly-mouth"),
        "maniac" => has_cap(capabilities, "maniac-mouth") && has_cap(capabilities, "head-body"),
        "lovestruck" => has_cap(capabilities, "lovestruck"),
        _ => {
            if capabilities.is_empty() {
                return true;
            }
            !RIG_STATE_HEAD_BODY_INTENTS.contains(&intent) || has_cap(capabilities, "head-body")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn drops_driver_and_angle_fields() {
        let summary = sanitize_rig_state(&json!({
            "expression": "warm",
            "posture": "open",
            "angleX": 0.4,
            "driver": { "mouthOpen": 1 },
            "owners": {
                "mouth": "speech",
                "expression": "autonomy",
                "headBody": "music"
            },
            "speaking": true,
            "singing": true,
            "activeBehaviors": [
                {
                    "function": "attend",
                    "lifecycle": "holding",
                    "source": "performance",
                    "resources": ["face.gaze", "angleX"],
                    "remainingMs": 900
                },
                {
                    "function": "unknown",
                    "lifecycle": "holding",
                    "source": "performance"
                }
            ],
            "recentIntents": ["delight", "angleZ"],
            "capabilities": ["dizzy-eye", "psd-layer"],
            "motionStyle": "open"
        }))
        .unwrap();
        let encoded = serde_json::to_value(&summary).unwrap();
        assert!(encoded.get("angleX").is_none());
        assert!(encoded.get("driver").is_none());
        assert_eq!(summary.owners.mouth, "speech");
        assert_eq!(summary.owners.expression, "idle");
        assert_eq!(summary.owners.head_body, "music");
        assert_eq!(summary.recent_intents, vec!["delight"]);
        assert_eq!(summary.capabilities, vec!["dizzy-eye"]);
        assert_eq!(summary.active_behaviors.len(), 1);
        assert_eq!(summary.active_behaviors[0].function, "attend");
        assert_eq!(summary.active_behaviors[0].resources, vec!["face.gaze"]);
    }

    #[test]
    fn persona_style_is_restrained_when_mood_is_low() {
        assert_eq!(motion_style_from_persona(&[], "", 20, 48), "restrained");
        assert_eq!(
            motion_style_from_persona(&["活泼".into()], "", 80, 60),
            "open"
        );
        assert_eq!(motion_style_from_persona(&[], "", 90, 48), "even");
        assert_eq!(motion_style_from_persona(&[], "", 90, 70), "open");
    }

    #[test]
    fn refine_keeps_a_repeated_special_when_the_face_can_play_it() {
        let state = sanitize_rig_state(&json!({
            "recentIntents": ["silly"],
            "capabilities": ["silly-eye", "head-body"]
        }))
        .unwrap();
        let plan = ChatPerformancePlan {
            baseline: None,
            cues: vec![crate::ChatPerformanceCue {
                intent: "silly".into(),
                at_ms: 0,
                intensity: 1.0,
                tempo: 1.0,
                fade_in_ms: 80,
                fade_out_ms: 120,
                interrupt: "replace".into(),
            }],
        };
        let refined = refine_performance_plan(plan, &state);
        assert_eq!(refined.cues.len(), 1);
        assert_eq!(refined.cues[0].intent, "silly");
    }

    #[test]
    fn refine_leaves_music_conflicts_to_runtime_resource_scheduling() {
        let state = sanitize_rig_state(&json!({
            "owners": { "headBody": "music" },
            "singing": true,
            "recentIntents": ["silly"],
            "capabilities": ["silly-eye", "head-body"]
        }))
        .unwrap();
        let plan = ChatPerformancePlan {
            baseline: None,
            cues: vec![
                crate::ChatPerformanceCue {
                    intent: "silly".into(),
                    at_ms: 0,
                    intensity: 1.0,
                    tempo: 1.0,
                    fade_in_ms: 80,
                    fade_out_ms: 120,
                    interrupt: "replace".into(),
                },
                crate::ChatPerformanceCue {
                    intent: "greet".into(),
                    at_ms: 0,
                    intensity: 1.0,
                    tempo: 1.0,
                    fade_in_ms: 80,
                    fade_out_ms: 120,
                    interrupt: "replace".into(),
                },
                crate::ChatPerformanceCue {
                    intent: "listen".into(),
                    at_ms: 0,
                    intensity: 1.0,
                    tempo: 1.0,
                    fade_in_ms: 80,
                    fade_out_ms: 120,
                    interrupt: "if-lower".into(),
                },
            ],
        };
        let refined = refine_performance_plan(plan, &state);
        assert_eq!(refined.cues.len(), 3);
        assert_eq!(refined.cues[0].intent, "silly");
        assert_eq!(refined.cues[1].intent, "greet");
        assert_eq!(refined.cues[2].intent, "listen");
    }

    #[test]
    fn refine_keeps_persistent_bearing_while_singing() {
        let state = sanitize_rig_state(&json!({
            "owners": { "headBody": "music" },
            "singing": true,
            "capabilities": ["head-body"]
        }))
        .unwrap();
        let plan = ChatPerformancePlan {
            baseline: Some(crate::ChatPerformanceBaseline {
                expression: "warm".into(),
                posture: "open".into(),
                motion_energy: 1.0,
                attention: 1.0,
            }),
            cues: vec![crate::ChatPerformanceCue {
                intent: "listen".into(),
                at_ms: 0,
                intensity: 1.0,
                tempo: 1.0,
                fade_in_ms: 80,
                fade_out_ms: 120,
                interrupt: "replace".into(),
            }],
        };
        let refined = refine_performance_plan(plan, &state);
        assert_eq!(refined.baseline.as_ref().unwrap().posture, "open");
        assert_eq!(refined.cues[0].intent, "listen");
    }

    #[test]
    fn refine_drops_mouth_cues_while_speaking() {
        assert!(
            !RIG_STATE_MOUTH_INTENTS.is_empty(),
            "emptying RIG_STATE_MOUTH_INTENTS silently disables the speaking-mouth filter",
        );
        for intent in RIG_STATE_MOUTH_INTENTS {
            assert!(
                PERFORMANCE_CUE_INTENTS.contains(intent),
                "{intent} must survive parse_performance_plan",
            );
        }
        let state = sanitize_rig_state(&json!({
            "speaking": true,
            "capabilities": ["head-body", "maniac-mouth", "cry-mouth", "silly-mouth"]
        }))
        .unwrap();
        let plan = crate::parse_performance_plan(
            r#"{"cues":[{"intent":"listen","atMs":0,"intensity":1,"tempo":1,"fadeInMs":80,"fadeOutMs":120,"interrupt":"replace"},{"intent":"maniac","atMs":0,"intensity":1,"tempo":1,"fadeInMs":80,"fadeOutMs":120,"interrupt":"replace"}]}"#,
        )
        .unwrap();
        assert_eq!(plan.cues.len(), 2);
        let refined = refine_performance_plan(plan, &state);
        assert_eq!(refined.cues.len(), 1);
        assert_eq!(refined.cues[0].intent, "listen");
    }

    #[test]
    fn refine_keeps_silly_eyes_while_speaking() {
        let state = sanitize_rig_state(&json!({
            "speaking": true,
            "capabilities": ["head-body", "silly-eye", "silly-mouth"]
        }))
        .unwrap();
        let plan = crate::parse_performance_plan(
            r#"{"cues":[{"intent":"listen","atMs":0,"intensity":1,"tempo":1,"fadeInMs":80,"fadeOutMs":120,"interrupt":"replace"},{"intent":"silly","atMs":0,"intensity":1,"tempo":1,"fadeInMs":80,"fadeOutMs":120,"interrupt":"replace"}]}"#,
        )
        .unwrap();
        let refined = refine_performance_plan(plan, &state);
        assert!(refined.cues.iter().any(|cue| cue.intent == "silly"));
        let mouth_only = sanitize_rig_state(&json!({
            "speaking": true,
            "capabilities": ["head-body", "silly-mouth"]
        }))
        .unwrap();
        let dropped = refine_performance_plan(
            crate::parse_performance_plan(
                r#"{"cues":[{"intent":"silly","atMs":0,"intensity":1,"tempo":1,"fadeInMs":80,"fadeOutMs":120,"interrupt":"replace"}]}"#,
            )
            .unwrap(),
            &mouth_only,
        );
        assert!(dropped.cues.is_empty());
    }

    #[test]
    fn empty_capabilities_drop_stickers_but_keep_generic_acting() {
        let state = sanitize_rig_state(&json!({
            "capabilities": []
        }))
        .unwrap();
        assert!(state.capabilities.is_empty());
        let plan = ChatPerformancePlan {
            baseline: Some(crate::ChatPerformanceBaseline {
                expression: "warm".into(),
                posture: "open".into(),
                motion_energy: 1.0,
                attention: 0.8,
            }),
            cues: vec![
                crate::ChatPerformanceCue {
                    intent: "dizzy".into(),
                    at_ms: 0,
                    intensity: 1.0,
                    tempo: 1.0,
                    fade_in_ms: 80,
                    fade_out_ms: 120,
                    interrupt: "replace".into(),
                },
                crate::ChatPerformanceCue {
                    intent: "greet".into(),
                    at_ms: 0,
                    intensity: 1.0,
                    tempo: 1.0,
                    fade_in_ms: 80,
                    fade_out_ms: 120,
                    interrupt: "replace".into(),
                },
                crate::ChatPerformanceCue {
                    intent: "listen".into(),
                    at_ms: 0,
                    intensity: 1.0,
                    tempo: 1.0,
                    fade_in_ms: 80,
                    fade_out_ms: 120,
                    interrupt: "if-lower".into(),
                },
                crate::ChatPerformanceCue {
                    intent: "think".into(),
                    at_ms: 40,
                    intensity: 1.0,
                    tempo: 1.0,
                    fade_in_ms: 80,
                    fade_out_ms: 120,
                    interrupt: "if-lower".into(),
                },
            ],
        };
        let refined = refine_performance_plan(plan, &state);
        assert_eq!(refined.baseline.as_ref().unwrap().posture, "open");
        let intents: Vec<_> = refined.cues.iter().map(|cue| cue.intent.as_str()).collect();
        assert_eq!(intents, vec!["greet", "listen", "think"]);
    }

    #[test]
    fn every_generic_head_pose_requires_the_head_body_capability() {
        let without_body = vec!["blink".to_string()];
        let with_body = vec!["head-body".to_string()];
        for intent in ["respond", "listen", "think"] {
            assert!(!cue_is_playable(&without_body, intent), "{intent}");
            assert!(cue_is_playable(&with_body, intent), "{intent}");
        }
    }

    #[test]
    fn maniac_requires_both_its_mouth_artwork_and_head_body_motion() {
        assert!(!cue_is_playable(&["maniac-mouth".to_string()], "maniac"));
        assert!(!cue_is_playable(&["head-body".to_string()], "maniac"));
        assert!(cue_is_playable(
            &["maniac-mouth".to_string(), "head-body".to_string()],
            "maniac"
        ));
    }

    #[test]
    fn persona_overrides_client_motion_style() {
        assert_eq!(
            round_motion_style(
                Some("open"),
                Some(&json!({"socialStyle": "内向"})),
                true,
                70,
                48
            ),
            "restrained"
        );
        assert_eq!(
            round_motion_style(Some("open"), None, false, 20, 48),
            "open"
        );
        assert_eq!(round_motion_style(None, None, false, 20, 48), "even");
    }
}
