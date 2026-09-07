//! Strict-Lite semantic motion selection for Merope.
//!
//! The model selects only a bounded expression/posture baseline and semantic
//! cues. Anime2.5DRig driver values, lip sync, blinking, breathing and secondary
//! motion remain deterministic on the client.

use std::time::{Duration, Instant};

use myriad_merope::{
    cue_is_playable, cue_survives_state, grounded_speech_phrases, parse_performance_plan,
    plan_is_empty, refine_performance_plan, round_motion_style, ChatPerformanceBaseline,
    ChatPerformanceCue, ChatPerformancePlan, RigStateSummary, SpeechPhrase,
    PERFORMANCE_BASELINE_EXPRESSIONS, PERFORMANCE_CUE_INTENTS, PERFORMANCE_INTERRUPT_MODES,
    PERFORMANCE_PHRASE_INTENTS, PERFORMANCE_POSTURES, RIG_STATE_MOTION_STYLES,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::models::entities::agent_persona;

use super::motion_local::local_performance_plan;
use super::store::get_persona;
use super::MoodTransition;

/// At 4s/5s production dropped 196 of 217 director calls, every one of them
/// sitting exactly on the request timeout; the two that returned took 4065ms
/// and 7168ms. Lite is simply slower than that wall. Widening it is only safe
/// because `local_performance_plan` now carries the round on its own: no
/// caller waits on Lite for acting, so a long call costs nothing but arrives
/// as a refinement or not at all.
const MOTION_TIMEOUT: Duration = Duration::from_secs(9);
const MOTION_TOTAL_TIMEOUT: Duration = Duration::from_secs(10);
const MOTION_SCHEMA_NAME: &str = "merope_motion";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MotionPhase {
    Reaction,
    Delivery,
    Outcome,
    Proactive,
    Mood,
}

impl MotionPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Reaction => "reaction",
            Self::Delivery => "delivery",
            Self::Outcome => "outcome",
            Self::Proactive => "proactive",
            Self::Mood => "mood",
        }
    }

    pub fn activity(self) -> &'static str {
        match self {
            Self::Reaction => "thinking",
            Self::Delivery | Self::Outcome | Self::Proactive => "talking",
            Self::Mood => "idle",
        }
    }
}

#[derive(Debug, Clone)]
pub struct MotionContext {
    pub user_id: i32,
    pub phase: MotionPhase,
    pub mood: MoodTransition,
    pub activity: String,
    pub user_text: String,
    pub response_text: Option<String>,
    pub task_success: Option<bool>,
    pub rig_state: Option<RigStateSummary>,
    /// Resolved once per Chat/Work round. Client style is only a fallback.
    pub motion_style: String,
}

#[derive(Debug, Clone, PartialEq)]
enum MotionDecision {
    Continue,
    Perform(ChatPerformancePlan),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PerformanceDirective {
    pub phase: MotionPhase,
    pub mood_revision: i64,
    pub motion_style: String,
    pub plan: ChatPerformancePlan,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub phrases: Vec<SpeechPhrase>,
}

/// Runs exactly one Lite-tier call, falling back to the deterministic plan for
/// callers (such as proactive speech) that have no live floor publisher.
pub async fn direct_motion(context: MotionContext) -> Option<PerformanceDirective> {
    direct_motion_inner(context, true).await
}

/// Refines a floor that has already been published. A timeout, invalid answer
/// or `continue` produces nothing so the same local beat is never replayed.
pub async fn refine_motion(context: MotionContext) -> Option<PerformanceDirective> {
    direct_motion_inner(context, false).await
}

async fn direct_motion_inner(
    context: MotionContext,
    fallback_to_local: bool,
) -> Option<PerformanceDirective> {
    let started = Instant::now();
    let phase = context.phase.as_str();
    let rig_state = apply_round_motion_style(context.rig_state.clone(), &context.motion_style);
    if face_is_hidden(rig_state.as_ref()) {
        tracing::debug!(phase, "[MeropeMotion] Face hidden; ambient motion only");
        return None;
    }
    let analyzer =
        crate::services::ai::create_strict_lite_ai_analyzer_with_timeout(Some(MOTION_TIMEOUT))
            .await;
    let result = if let Some(analyzer) = analyzer {
        let persona_row = match crate::services::tapp_registry::database().await {
            Ok(db) => get_persona(&db).await.ok().flatten(),
            Err(_) => None,
        };
        let input = serde_json::json!({
            "phase": phase,
            "mood": {
                "value": context.mood.after,
                "arousal": context.mood.arousal_after,
                "arousalDelta": context.mood.arousal_after - context.mood.arousal_before,
                "band": context.mood.band_after,
                "previousBand": context.mood.band_before,
                "delta": context.mood.delta,
                "cause": context.mood.cause,
                "revision": context.mood.revision,
            },
            "activity": context.activity,
            "userText": truncate(&context.user_text, 600),
            "responseText": context.response_text.as_deref().map(|value| truncate(value, 900)),
            "taskSuccess": context.task_success,
            "rig": rig_state.as_ref(),
            "persona": motion_persona_payload(persona_row.as_ref(), &context.motion_style),
        })
        .to_string();

        // Offer only what this face can actually play. Constrained decoding
        // then cannot spend the round's one cue on something the filter below
        // would delete.
        let offered = offered_cue_intents(rig_state.as_ref());
        let schema = motion_schema(&offered);
        let system_prompt = motion_system_prompt(&offered);
        let call = analyzer.analyze_json(&system_prompt, &input, MOTION_SCHEMA_NAME, Some(&schema));
        Some(
            tokio::time::timeout(
                MOTION_TOTAL_TIMEOUT,
                crate::services::ai_cost_ledger::with_site_ai_ledger(
                    context.user_id,
                    "merope",
                    &format!("motion_{phase}"),
                    call,
                ),
            )
            .await,
        )
    } else {
        tracing::debug!(
            phase,
            "[MeropeMotion] Lite unavailable; requested cues may still play"
        );
        None
    };

    let elapsed_ms = started.elapsed().as_millis() as u64;
    let mut phrases = Vec::new();
    let lite_plan = match result {
        None => None,
        Some(Ok(Ok(raw))) => match parse_motion_decision(&raw) {
            Some(MotionDecision::Continue) => {
                tracing::debug!(
                    phase,
                    elapsed_ms,
                    "[MeropeMotion] Lite continued current acting"
                );
                None
            }
            Some(MotionDecision::Perform(parsed)) => {
                if let Ok(value) = serde_json::from_str::<Value>(strip_motion_json(&raw)) {
                    phrases = grounded_speech_phrases(
                        &value["phrases"],
                        context.response_text.as_deref(),
                    );
                }
                Some(parsed)
            }
            None => {
                tracing::warn!(
                    phase,
                    elapsed_ms,
                    "[MeropeMotion] Invalid Lite plan dropped"
                );
                None
            }
        },
        Some(Ok(Err(error))) => {
            tracing::warn!(phase, elapsed_ms, error = %error, "[MeropeMotion] Lite call dropped");
            None
        }
        Some(Err(_)) => {
            tracing::warn!(
                phase,
                elapsed_ms,
                "[MeropeMotion] Lite total timeout; plan dropped"
            );
            None
        }
    };
    let refined_by_lite = lite_plan.is_some();
    let mut plan = if let Some(plan) = lite_plan {
        plan
    } else if fallback_to_local {
        local_performance_plan(
            context.phase,
            &context.mood,
            context.task_success,
            context.response_text.as_deref(),
            &context.motion_style,
            Some(&context.user_text),
        )
    } else {
        return None;
    };
    if let Some(state) = rig_state.as_ref() {
        plan = refine_performance_plan(plan, state);
    }
    // A named ask is not a command; overlay only after this turn's reply.
    plan = apply_user_requested_cue(
        plan,
        context.phase,
        &context.user_text,
        context.response_text.as_deref(),
        rig_state.as_ref(),
    );
    if plan_is_empty(&plan) && phrases.is_empty() {
        tracing::debug!(
            phase,
            elapsed_ms,
            "[MeropeMotion] Lite plan had no capable cues"
        );
        return None;
    }
    tracing::info!(
        phase,
        elapsed_ms,
        source = if refined_by_lite { "lite" } else { "local" },
        requested = play_along_requested_cue(
            context.phase,
            &context.user_text,
            context.response_text.as_deref(),
        )
        .is_some(),
        "[MeropeMotion] plan ready"
    );
    Some(PerformanceDirective {
        phase: context.phase,
        mood_revision: context.mood.revision,
        motion_style: context.motion_style,
        plan,
        phrases,
    })
}

/// The deterministic floor as a directive, with no network call and no await.
///
/// Chat plays this the moment the round starts so the character reacts before
/// it speaks, and every response carries it so a non-streaming client still
/// gets acting. The Lite refinement, when it lands, publishes over the run hub
/// and replaces this.
pub fn local_directive(context: &MotionContext) -> Option<PerformanceDirective> {
    let rig_state = apply_round_motion_style(context.rig_state.clone(), &context.motion_style);
    if face_is_hidden(rig_state.as_ref()) {
        return None;
    }
    let mut plan = local_performance_plan(
        context.phase,
        &context.mood,
        context.task_success,
        context.response_text.as_deref(),
        &context.motion_style,
        Some(&context.user_text),
    );
    if let Some(state) = rig_state.as_ref() {
        plan = refine_performance_plan(plan, state);
    }
    plan = apply_user_requested_cue(
        plan,
        context.phase,
        &context.user_text,
        context.response_text.as_deref(),
        rig_state.as_ref(),
    );
    if plan_is_empty(&plan) {
        return None;
    }
    Some(PerformanceDirective {
        phase: context.phase,
        mood_revision: context.mood.revision,
        motion_style: context.motion_style.clone(),
        plan,
        phrases: Vec::new(),
    })
}

/// One persona read per Chat/Work round. Client style is only used when the
/// site persona cannot be loaded.
pub async fn resolve_round_motion_style(
    client: Option<&RigStateSummary>,
    mood: i32,
    arousal: i32,
) -> String {
    let client_style = client.map(|summary| summary.motion_style.as_str());
    if let Ok(db) = crate::services::tapp_registry::database().await {
        if let Ok(Some(persona)) = get_persona(&db).await {
            return round_motion_style(
                client_style,
                persona.persona_json.as_ref(),
                true,
                mood,
                arousal,
            );
        }
    }
    round_motion_style(client_style, None, false, mood, arousal)
}

fn apply_round_motion_style(
    summary: Option<RigStateSummary>,
    motion_style: &str,
) -> Option<RigStateSummary> {
    let mut summary = summary?;
    summary.motion_style = if RIG_STATE_MOTION_STYLES.contains(&motion_style) {
        motion_style.to_string()
    } else {
        summary.motion_style
    };
    Some(summary)
}

fn face_is_hidden(state: Option<&RigStateSummary>) -> bool {
    state.is_some_and(|summary| !summary.page_visible || !summary.face_visible)
}

fn parse_motion_decision(raw: &str) -> Option<MotionDecision> {
    let stripped = strip_motion_json(raw);
    let value: serde_json::Value = serde_json::from_str(stripped).ok()?;
    let object = value.as_object()?;
    match object.get("continue") {
        Some(flag) if !flag.is_boolean() => return None,
        Some(flag) if flag.as_bool() == Some(true) => {
            if motion_payload_present(object) {
                return None;
            }
            return Some(MotionDecision::Continue);
        }
        _ => {}
    }
    if let Some(plan) = parse_performance_plan(stripped) {
        return Some(MotionDecision::Perform(plan));
    }
    // A grounded phrase refinement does not need to replace the standing face.
    // Do not turn an invalid baseline/cue payload into a phrase-only success.
    let empty_plan = object.get("baseline").is_none_or(Value::is_null)
        && object
            .get("cues")
            .is_none_or(|value| value.as_array().is_some_and(Vec::is_empty));
    let valid_phrase = object
        .get("phrases")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items.iter().take(6).any(|item| {
                !grounded_speech_phrases(
                    &serde_json::json!([item]),
                    item.get("text").and_then(Value::as_str),
                )
                .is_empty()
            })
        });
    (empty_plan && valid_phrase).then(|| MotionDecision::Perform(ChatPerformancePlan::default()))
}

fn motion_payload_present(object: &serde_json::Map<String, serde_json::Value>) -> bool {
    let has_baseline = object.get("baseline").is_some_and(|value| !value.is_null());
    let has_cues = object
        .get("cues")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|cues| !cues.is_empty());
    has_baseline
        || has_cues
        || object
            .get("phrases")
            .and_then(Value::as_array)
            .is_some_and(|items| !items.is_empty())
}

fn strip_motion_json(raw: &str) -> &str {
    let trimmed = raw.trim();
    trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .and_then(|inner| inner.strip_suffix("```"))
        .unwrap_or(trimmed)
        .trim()
}

fn truncate(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

const USER_CUE_REQUEST_MARKERS: &[&str] = &[
    "做",
    "来个",
    "来一下",
    "给我做",
    "给我来",
    "表演",
    "露出",
    "摆出",
    "make a",
    "do a",
    "show me",
];

const USER_CUE_ALIASES: &[(&str, &[&str])] = &[
    ("maniac", &["狂笑", "疯笑", "发狂", "maniac"]),
    ("silly", &["呆呆", "发呆", "犯傻", "傻眼", "silly"]),
    ("cry", &["哭脸", "哭一个", "哭泣", "哭一下", "cry"]),
    ("angry", &["生气", "愤怒", "怒脸", "angry"]),
    ("speechless", &["无语", "无语脸", "speechless"]),
    ("dizzy", &["晕倒", "眩晕", "dizzy"]),
    ("lovestruck", &["心动", "花痴", "lovestruck"]),
    ("think", &["思考脸", "思考的表情", "think"]),
];

fn user_requested_cue(text: &str) -> Option<&'static str> {
    let folded = text.to_lowercase();
    let mut best: Option<(usize, &'static str)> = None;
    for (intent, aliases) in USER_CUE_ALIASES {
        for alias in *aliases {
            let Some(pos) = alias_request_pos(text, &folded, alias) else {
                continue;
            };
            if best.is_none_or(|(seen, _)| pos < seen) {
                best = Some((pos, *intent));
            }
        }
    }
    best.map(|(_, intent)| intent)
}

fn alias_request_pos(text: &str, folded: &str, alias: &str) -> Option<usize> {
    let alias_folded = alias.to_lowercase();
    // A Latin alias must stand on its own here too: `cry` sits inside
    // `cryptic`, and `think` in `thinking about it` is not a request for the
    // think face — the marker below is what turns a mention into an ask.
    let pos = text_mention_pos(text, folded, alias)?;
    if alias.contains("一下") || alias.contains("一个") {
        return Some(pos);
    }
    let asked = USER_CUE_REQUEST_MARKERS
        .iter()
        .any(|marker| text.contains(marker) || folded.contains(&marker.to_lowercase()));
    if asked {
        return Some(pos);
    }
    let compounds = [
        format!("{alias}一下"),
        format!("一下{alias}"),
        format!("{alias}一个"),
        format!("{alias}的表情"),
        format!("做个{alias}"),
        format!("make a {alias_folded}"),
        format!("do a {alias_folded}"),
        format!("show me {alias_folded}"),
    ];
    compounds
        .iter()
        .any(|phrase| text.contains(phrase) || folded.contains(&phrase.to_lowercase()))
        .then_some(pos)
}

pub fn text_mentions_any(text: &str, markers: &[&str]) -> bool {
    let folded = text.to_lowercase();
    markers
        .iter()
        .any(|marker| text_mention_pos(text, &folded, marker).is_some())
}

/// A Latin marker has to stand as its own word.
///
/// `hi` sits inside `this` and `which`, `hey` inside `they`, and `ww` inside
/// `www.` — with a plain substring test the floor greeted on most English
/// sentences and read any link as a joke. Chinese and Japanese have no word
/// boundary to test, so those markers stay substrings.
///
/// A repeated-letter marker may still be extended by more of the same letter,
/// because that is how `hhh` and `ww` are actually written; a following `.`
/// still rules the match out, which is what separates `www` from a hostname.
fn text_mention_pos(text: &str, folded: &str, marker: &str) -> Option<usize> {
    if !marker.is_ascii() {
        return text.find(marker).or_else(|| folded.find(marker));
    }
    let lowered = marker.to_lowercase();
    let repeated = lowered
        .chars()
        .next()
        .filter(|first| lowered.len() > 1 && lowered.chars().all(|letter| letter == *first));
    let blocks = |letter: Option<char>| letter.is_some_and(|letter| letter.is_ascii_alphanumeric());
    folded.match_indices(&lowered).find_map(|(index, found)| {
        // Widen to the whole run first: `www.` is a hostname however much of
        // it a two-letter marker happens to land on.
        let (head, tail) = (&folded[..index], &folded[index + found.len()..]);
        let (head, tail) = match repeated {
            Some(letter) => (
                head.trim_end_matches(letter),
                tail.trim_start_matches(letter),
            ),
            None => (head, tail),
        };
        let standalone = !blocks(head.chars().next_back())
            && !blocks(tail.chars().next())
            && !tail.starts_with('.');
        standalone.then_some(index)
    })
}

fn requested_cue(intent: &str) -> ChatPerformanceCue {
    let sticker = matches!(
        intent,
        "maniac" | "silly" | "cry" | "dizzy" | "lovestruck" | "angry"
    );
    ChatPerformanceCue {
        intent: intent.to_string(),
        at_ms: 0,
        intensity: 1.15,
        tempo: 1.0,
        fade_in_ms: if sticker { 180 } else { 100 },
        fade_out_ms: if sticker { 420 } else { 220 },
        interrupt: "replace".to_string(),
    }
}

fn landing_baseline(_intent: &str) -> ChatPerformanceBaseline {
    ChatPerformanceBaseline {
        expression: "steady".to_string(),
        posture: "neutral".to_string(),
        motion_energy: 1.0,
        attention: 0.65,
    }
}

const REQUEST_REFUSALS: &[&str] = &[
    "才不",
    "才不会",
    "才不要",
    "才不给",
    "不要做",
    "不做",
    "不想做",
    "拒绝",
    "别做",
    "i won't",
    "i will not",
    "no way",
];

fn response_refuses_requested_cue(response: &str) -> bool {
    let folded = response.to_lowercase();
    REQUEST_REFUSALS
        .iter()
        .any(|marker| response.contains(marker) || folded.contains(&marker.to_lowercase()))
}

fn play_along_requested_cue(
    phase: MotionPhase,
    user_text: &str,
    response_text: Option<&str>,
) -> Option<&'static str> {
    if phase != MotionPhase::Delivery {
        return None;
    }
    let response = response_text
        .map(str::trim)
        .filter(|text| !text.is_empty())?;
    let intent = user_requested_cue(user_text)?;
    if response_refuses_requested_cue(response) {
        return None;
    }
    Some(intent)
}

fn apply_user_requested_cue(
    mut plan: ChatPerformancePlan,
    phase: MotionPhase,
    user_text: &str,
    response_text: Option<&str>,
    rig: Option<&RigStateSummary>,
) -> ChatPerformancePlan {
    let Some(intent) = play_along_requested_cue(phase, user_text, response_text) else {
        return plan;
    };
    if let Some(state) = rig {
        if !cue_is_playable(&state.capabilities, intent) {
            return plan;
        }
    }
    plan.cues.retain(|cue| cue.intent != intent);
    plan.cues.insert(0, requested_cue(intent));
    if plan.cues.len() > 3 {
        plan.cues.truncate(3);
    }
    plan.baseline = Some(landing_baseline(intent));
    plan
}

/// Contract catalog the director may call. Keys must stay aligned with
/// `PERFORMANCE_*` so a new expression cannot ship unindexed.
const BASELINE_INDEX: &[(&str, &str)] = &[
    (
        "withdrawn",
        "收着、回避、不想展开。慢热、低心情、被冒犯时的底。",
    ),
    ("subdued", "压着但仍在场。克制、认真、不想热闹。"),
    ("steady", "平常脸。中性底，仍要配 cue，不能当成没表情。"),
    ("warm", "放松、亲近、带笑意。外向或软的人设常用。"),
    (
        "tense",
        "烦躁、绷着、被磨得没耐心。心情低但精神绷紧时的底——不是难过，是坐不住；眉压着而眼睛更睁着。",
    ),
];

const POSTURE_INDEX: &[(&str, &str)] = &[
    ("closed", "收着、不想占空间。"),
    ("neutral", "平常站位。"),
    ("open", "打开、靠近、欢迎。"),
];

const CUE_INDEX: &[(&str, &str, &str)] = &[
    ("greet", "打招呼、点头致意", "head-body"),
    ("respond", "接住对方刚说的话", "head-body"),
    ("question", "疑惑、反问、没听清", "head-body"),
    ("delight", "开心、被逗到、事情顺利", "head-body"),
    ("emphasize", "加重、认真说一句", "head-body"),
    ("listen", "在听、等对方说完", "head-body"),
    ("notify", "提醒、告知一件事", "head-body"),
    ("think", "在想、回忆、斟酌", "head-body"),
    (
        "dizzy",
        "晕、转、过载。人设会晕或过载时用，不必等台词说「我晕了」",
        "dizzy-eye",
    ),
    (
        "cry",
        "难过到脸上。人设会露伤心时用，不必等台词说自己在哭",
        "cry-eye|cry-mouth",
    ),
    (
        "angry",
        "生气、被惹到。嘴硬或边界感强的人设可更快上来",
        "head-body",
    ),
    ("speechless", "无语、尴尬、愣住", "head-body"),
    (
        "maniac",
        "失控的兴奋或夸张狂气。爱闹的人设在高潮时可用",
        "maniac-mouth + head-body",
    ),
    (
        "silly",
        "自嘲、犯蠢、出糗、发呆、被逗到。对方让你讲糗事或你正在讲时用，不必出现「呆呆」",
        "silly-eye|silly-mouth",
    ),
    (
        "lovestruck",
        "被说动、害羞、心动。亲近时可用，不必等情话",
        "lovestruck",
    ),
];

/// The cues this face can play, in contract order. No rig state means an old
/// client, which keeps the full vocabulary exactly as before.
fn offered_cue_intents(state: Option<&RigStateSummary>) -> Vec<&'static str> {
    let Some(state) = state else {
        return PERFORMANCE_CUE_INTENTS.to_vec();
    };
    PERFORMANCE_CUE_INTENTS
        .iter()
        .copied()
        .filter(|intent| cue_survives_state(state, intent))
        .collect()
}

fn motion_expression_index(offered: &[&str]) -> String {
    let mut lines = Vec::new();
    lines.push("表情底 baseline.expression（每回合必选一个）：".to_string());
    for (name, meaning) in BASELINE_INDEX {
        lines.push(format!("- {name}：{meaning}"));
    }
    lines.push("姿态 baseline.posture：".to_string());
    for (name, meaning) in POSTURE_INDEX {
        lines.push(format!("- {name}：{meaning}"));
    }
    lines.push(
        "瞬时表情 cues.intent（每回合 0–2 个；没有明确行为意义就留空。读这一轮对话的意思取用，不要等表情名字）："
            .to_string(),
    );
    for (name, meaning, capability) in CUE_INDEX {
        if !offered.contains(name) {
            continue;
        }
        if capability.is_empty() {
            lines.push(format!("- {name}：{meaning}"));
        } else {
            lines.push(format!("- {name}：{meaning}。能力：{capability}"));
        }
    }
    lines.join("\n")
}

fn motion_system_prompt(offered: &[&str]) -> String {
    format!(
        r#"你是这个人设的动作导演。只选语义表演。读 persona 和这一轮 userText/responseText 的意思，按这个人会怎么露脸。不要等「呆呆」「狂笑」「做一下」这类字。mood 是事实，不要改。

{}

枚举：{}；姿态 {}；cue {}。每回合必须有 baseline，cue 只在确有表达功能时选 0–2 个；同一功能不要为了热闹重复。不要输出 continue，空对象无效。
phrases 是配合 responseText 的句段表达意图，0–6 个，按原文顺序。每项 text 必须逐字摘取 responseText 中唯一出现的短句（含结尾标点，2–120 字符），不要引用 userText、代码、他人的引语或编造还没生成的后文。intent 可用 ask（真正询问）、hesitate（犹豫斟酌）、tease（亲近调侃/玩笑式反问）、explain（转念解释/认真说明）、check-in（说完后确认对方反应）、laugh（本人确实在笑）、none（克制、不应按问号/笑字自动表演）。区分本人表达与提到他人情绪；描述难过不是本人难过，描述笑声不是本人发笑。让相邻句段延续表达动机，例如 hesitate→explain→check-in，别把每句都做成独立高潮。已分配给 phrases 的同一表达不要再放入 cues；cue 留给不依赖具体台词的整轮反应。现场只修改尚未发力的句段，已说过的短句会跳过，不用补演。
只丢掉物理上做不到的：缺能力层不要选；说话时 maniac 抢嘴所以不要选，silly/cry 用眼睛照演。唱歌占身不要抢头身。
按性格取表情：慢热用 withdrawn/subdued，确实在持续听时才用 listen；外向可用 warm + greet/delight，玩笑和自嘲用 silly、兴奋 maniac；嘴硬多用 speechless/angry；认真多用 question/think；软可用 lovestruck。没有人设时按 even；baseline 必须有，cue 可以没有。
restrained 的 motionEnergy 0.55–0.9、cue 0.75–1.05；even 0.75–1.15 / 0.9–1.25；open 1.0–1.4 / 1.05–1.4。
像人一样安排反应：起势快、落势慢；一个明确反应完成或进入落势前，不要再叠同功能动作。rig.activeBehaviors 是同时在进行或准备中的语义行为，lifecycle 是 planned/preparing/committed/holding/recovering，resources 是它正在使用的脸、视线、头、躯干或肢体。已有同功能时不重复；资源冲突时删掉低意义 cue，确实要接续才用 queue 并把 atMs 放到 remainingMs 之后。音乐的 entrain 是持续的人体节律，不是特殊动画：唱歌占头身时只叠不冲突的脸/视线反应。
reaction 回应用户已经说完的内容，不要假装仍在聆听；delivery 配合即将说的话（讲糗事、自嘲出糗用 silly）；outcome 配合任务结果；proactive 配合自己找上门的那句。atMs/fade 只给宽松的先后和风格，不要试图逐帧导演；现场调度器会按真实语音重音、节拍证据、资源占用和中断状态重定时，并保证 preparation→stroke→hold→recovery。"#,
        motion_expression_index(offered),
        PERFORMANCE_BASELINE_EXPRESSIONS.join("/"),
        PERFORMANCE_POSTURES.join("/"),
        offered.join("/")
    )
}

fn motion_persona_payload(persona: Option<&agent_persona::Model>, motion_style: &str) -> Value {
    let (name, personality, json) = match persona {
        Some(row) => (
            row.name.as_str(),
            row.personality.as_str(),
            row.persona_json.as_ref(),
        ),
        None => ("", "", None),
    };
    let display = if name.trim().is_empty() {
        "Arael"
    } else {
        name.trim()
    };
    serde_json::json!({
        "name": truncate(display, 50),
        "personality": truncate(personality.trim(), 800),
        "summary": json_text(json, "summary", 400),
        "temperament": json_text_list(json, "temperament", 8, 48),
        "socialStyle": json_text(json, "socialStyle", 240),
        "speechStyle": json_text(json, "speechStyle", 240),
        "motionStyle": motion_style,
    })
}

fn json_text(value: Option<&Value>, key: &str, max_chars: usize) -> String {
    value
        .and_then(|item| item.get(key))
        .and_then(Value::as_str)
        .map(|text| truncate(text.trim(), max_chars))
        .filter(|text| !text.is_empty())
        .unwrap_or_default()
}

fn json_text_list(
    value: Option<&Value>,
    key: &str,
    max_items: usize,
    max_chars: usize,
) -> Vec<String> {
    value
        .and_then(|item| item.get(key))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .take(max_items)
                .map(|text| truncate(text, max_chars))
                .collect()
        })
        .unwrap_or_default()
}

fn motion_schema(offered: &[&str]) -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "phrases": {
                "type": "array", "maxItems": 6,
                "items": {
                    "type": "object", "additionalProperties": false,
                    "properties": {
                        "text": { "type": "string", "minLength": 2, "maxLength": 120 },
                        "intent": { "type": "string", "enum": PERFORMANCE_PHRASE_INTENTS }
                    },
                    "required": ["text", "intent"]
                }
            },
            "baseline": {
                "type": "object",
                "properties": {
                    "expression": { "type": "string", "enum": PERFORMANCE_BASELINE_EXPRESSIONS },
                    "posture": { "type": "string", "enum": PERFORMANCE_POSTURES },
                    "motionEnergy": { "type": "number", "minimum": 0.2, "maximum": 1.4 },
                    "attention": { "type": "number", "minimum": 0.0, "maximum": 1.0 }
                },
                "required": ["expression", "posture", "motionEnergy", "attention"]
            },
            "cues": {
                "type": "array",
                "maxItems": 2,
                "items": {
                    "type": "object",
                    "properties": {
                        "intent": { "type": "string", "enum": offered },
                        "atMs": { "type": "integer", "minimum": 0, "maximum": 5000 },
                        "intensity": { "type": "number", "minimum": 0.2, "maximum": 1.4 },
                        "tempo": { "type": "number", "minimum": 0.5, "maximum": 1.6 },
                        "fadeInMs": { "type": "integer", "minimum": 40, "maximum": 600 },
                        "fadeOutMs": { "type": "integer", "minimum": 60, "maximum": 800 },
                        "interrupt": { "type": "string", "enum": PERFORMANCE_INTERRUPT_MODES }
                    },
                    "required": ["intent", "atMs", "intensity", "tempo", "fadeInMs", "fadeOutMs", "interrupt"]
                }
            },
        },
        "additionalProperties": false
    })
}

#[cfg(test)]
pub(super) mod live_probe {
    use super::*;

    pub fn contract(
        persona: &agent_persona::Model,
        rig: &RigStateSummary,
    ) -> (String, Value, Value) {
        let offered = offered_cue_intents(Some(rig));
        (
            motion_system_prompt(&offered),
            motion_schema(&offered),
            motion_persona_payload(Some(persona), "even"),
        )
    }

    pub fn valid(raw: &str, rig: &RigStateSummary) -> bool {
        let Ok(unfiltered) = serde_json::from_str::<ChatPerformancePlan>(raw) else {
            return false;
        };
        let Some(MotionDecision::Perform(plan)) = parse_motion_decision(raw) else {
            return false;
        };
        // A smoke test must not pass merely because production's safe parser
        // removed an invalid cue or clamped an out-of-range value.
        plan == unfiltered
            && plan.cues.len() <= 2
            && plan.baseline.is_some()
            && plan
                .cues
                .iter()
                .all(|cue| cue_survives_state(rig, &cue.intent))
    }

    #[test]
    fn smoke_rejects_cues_discarded_by_the_production_sanitizer() {
        let rig = myriad_merope::sanitize_rig_state(&serde_json::json!({})).unwrap();
        let valid = serde_json::json!({"baseline":{"expression":"steady","posture":"neutral","motionEnergy":1.0,"attention":0.5},"cues":[]});
        assert!(self::valid(&valid.to_string(), &rig));
        let mut invalid = valid;
        invalid["cues"] = serde_json::json!([{"intent":"not_a_cue","atMs":0,"intensity":1.0,"tempo":1.0,"fadeInMs":100,"fadeOutMs":200,"interrupt":"blend"}]);
        assert!(!self::valid(&invalid.to_string(), &rig));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncates_on_character_boundary() {
        assert_eq!(truncate("你好吗", 2), "你好");
    }

    #[test]
    fn phases_have_stable_wire_names() {
        assert_eq!(MotionPhase::Reaction.as_str(), "reaction");
        assert_eq!(MotionPhase::Reaction.activity(), "thinking");
        assert_eq!(MotionPhase::Delivery.activity(), "talking");
        assert_eq!(MotionPhase::Mood.activity(), "idle");
        assert_eq!(
            serde_json::to_string(&MotionPhase::Delivery).unwrap(),
            "\"delivery\""
        );
    }

    #[test]
    fn directive_wire_shape_is_camel_case_and_semantic_only() {
        let value = serde_json::to_value(PerformanceDirective {
            phase: MotionPhase::Reaction,
            mood_revision: 42,
            motion_style: "open".to_string(),
            phrases: vec![SpeechPhrase {
                text: "你觉得呢？".to_string(),
                intent: "check-in".to_string(),
            }],
            plan: ChatPerformancePlan {
                baseline: None,
                cues: vec![myriad_merope::ChatPerformanceCue {
                    intent: "listen".to_string(),
                    at_ms: 0,
                    intensity: 1.0,
                    tempo: 1.0,
                    fade_in_ms: 100,
                    fade_out_ms: 200,
                    interrupt: "if-lower".to_string(),
                }],
            },
        })
        .unwrap();
        assert_eq!(value["phase"], "reaction");
        assert_eq!(value["moodRevision"], 42);
        assert_eq!(value["motionStyle"], "open");
        assert!(value.pointer("/plan/cues/0/atMs").is_some());
        assert!(value.get("driver").is_none());
        assert_eq!(value["phrases"][0]["intent"], "check-in");
        assert_eq!(value["phrases"][0]["text"], "你觉得呢？");
    }

    #[test]
    fn motion_schema_exposes_new_expressions_only_as_semantic_cues() {
        let schema = motion_schema(PERFORMANCE_CUE_INTENTS);
        assert_eq!(
            schema.pointer("/properties/phrases/maxItems"),
            Some(&serde_json::json!(6))
        );
        assert_eq!(
            schema.pointer("/properties/phrases/items/properties/intent/enum"),
            Some(&serde_json::json!(PERFORMANCE_PHRASE_INTENTS))
        );
        let intents = schema
            .pointer("/properties/cues/items/properties/intent/enum")
            .and_then(serde_json::Value::as_array)
            .unwrap();
        assert!(intents.iter().any(|value| value == "think"));
        assert!(intents.iter().any(|value| value == "dizzy"));
        assert!(intents.iter().any(|value| value == "cry"));
        assert!(intents.iter().any(|value| value == "angry"));
        assert!(intents.iter().any(|value| value == "speechless"));
        assert!(intents.iter().any(|value| value == "maniac"));
        assert!(intents.iter().any(|value| value == "silly"));
        assert!(intents.iter().any(|value| value == "lovestruck"));
        let prompt = motion_system_prompt(PERFORMANCE_CUE_INTENTS);
        assert!(prompt.contains("按这个人会怎么露脸"));
        assert!(prompt.contains("按性格取表情"));
        assert!(!prompt.contains("只有文本明确表现"));
        assert!(!prompt.contains("不要夸张"));
        assert!(!prompt.contains("不要连续重复"));
        assert!(!prompt.contains("rig.capabilities 为空"));
        assert!(prompt.contains(&PERFORMANCE_BASELINE_EXPRESSIONS.join("/")));
        assert!(prompt.contains(&PERFORMANCE_POSTURES.join("/")));
        assert!(prompt.contains(&PERFORMANCE_CUE_INTENTS.join("/")));
        assert!(!prompt.contains("angleZ"));
        assert!(prompt.contains("不要输出 continue"));
        assert!(prompt.contains("空对象无效"));
        assert!(prompt.contains("不要等「呆呆」「狂笑」「做一下」这类字"));
        assert!(prompt.contains("自嘲"));
        assert!(prompt.contains("犯蠢"));
        assert!(prompt.contains("讲糗事、自嘲出糗用 silly"));
        assert!(prompt.contains("silly/cry 用眼睛照演"));
        assert!(prompt.contains("rig.activeBehaviors"));
        assert!(prompt.contains("preparation→stroke→hold→recovery"));
        assert!(prompt.contains("音乐的 entrain 是持续的人体节律"));
        assert!(prompt.contains("persona"));
        assert!(schema.pointer("/properties/continue").is_none());
        assert!(schema.get("required").is_none());
    }

    fn rig(capabilities: &[&str], speaking: bool) -> RigStateSummary {
        myriad_merope::sanitize_rig_state(&serde_json::json!({
            "expression": "steady",
            "posture": "neutral",
            "owners": { "mouth": "idle", "expression": "idle", "gaze": "idle", "headBody": "idle" },
            "speaking": speaking,
            "capabilities": capabilities,
        }))
        .expect("summary")
    }

    /// The offered set and the enforced set are one predicate.
    ///
    /// They used to be two: the schema enum listed all fifteen intents while
    /// `refine_performance_plan` deleted the ones this face cannot play. A
    /// round that spent its only cue on a sticker the rig has no layer for
    /// came back empty, and an empty plan drops the whole refinement.
    #[test]
    fn the_director_is_only_offered_cues_that_survive_the_filter() {
        for (capabilities, speaking) in [
            (&["head-body", "mouth-shapes"][..], false),
            (&["head-body", "cry-eye", "silly-eye"][..], false),
            (&["head-body", "maniac-mouth", "silly-mouth"][..], true),
            (&[][..], false),
        ] {
            let state = rig(capabilities, speaking);
            let offered = offered_cue_intents(Some(&state));
            assert!(!offered.is_empty(), "{capabilities:?}");

            let plan = ChatPerformancePlan {
                baseline: None,
                cues: PERFORMANCE_CUE_INTENTS
                    .iter()
                    .map(|intent| ChatPerformanceCue {
                        intent: (*intent).to_string(),
                        at_ms: 0,
                        intensity: 1.0,
                        tempo: 1.0,
                        fade_in_ms: 120,
                        fade_out_ms: 200,
                        interrupt: "replace".to_string(),
                    })
                    .collect(),
            };
            let survived: Vec<String> = refine_performance_plan(plan, &state)
                .cues
                .into_iter()
                .map(|cue| cue.intent)
                .collect();
            assert_eq!(survived, offered, "{capabilities:?} speaking={speaking}");

            let schema = motion_schema(&offered);
            let enumerated = schema
                .pointer("/properties/cues/items/properties/intent/enum")
                .and_then(|value| value.as_array())
                .expect("intent enum");
            assert_eq!(enumerated.len(), offered.len(), "{capabilities:?}");

            // The prompt must not describe a cue the schema forbids.
            let prompt = motion_system_prompt(&offered);
            for intent in PERFORMANCE_CUE_INTENTS {
                assert_eq!(
                    prompt.contains(&format!("- {intent}：")),
                    offered.contains(intent),
                    "{intent} for {capabilities:?}"
                );
            }
        }
    }

    /// A client that sends no rig state keeps the whole vocabulary.
    #[test]
    fn a_missing_summary_still_offers_every_cue() {
        assert_eq!(offered_cue_intents(None), PERFORMANCE_CUE_INTENTS.to_vec());
    }

    #[test]
    fn motion_prompt_indexes_every_contract_expression() {
        assert_eq!(
            BASELINE_INDEX
                .iter()
                .map(|(name, _)| *name)
                .collect::<Vec<_>>(),
            PERFORMANCE_BASELINE_EXPRESSIONS.to_vec()
        );
        assert_eq!(
            POSTURE_INDEX
                .iter()
                .map(|(name, _)| *name)
                .collect::<Vec<_>>(),
            PERFORMANCE_POSTURES.to_vec()
        );
        assert_eq!(
            CUE_INDEX
                .iter()
                .map(|(name, _, _)| *name)
                .collect::<Vec<_>>(),
            PERFORMANCE_CUE_INTENTS.to_vec()
        );
        let prompt = motion_system_prompt(PERFORMANCE_CUE_INTENTS);
        for name in PERFORMANCE_BASELINE_EXPRESSIONS {
            assert!(prompt.contains(&format!("- {name}：")), "{name}");
        }
        for name in PERFORMANCE_POSTURES {
            assert!(prompt.contains(&format!("- {name}：")), "{name}");
        }
        for name in PERFORMANCE_CUE_INTENTS {
            assert!(prompt.contains(&format!("- {name}：")), "{name}");
        }
    }

    #[test]
    fn motion_persona_payload_carries_temperament() {
        let blank = crate::models::entities::agent_persona::Model {
            id: "site".into(),
            name: "瞳".into(),
            personality: "气质：认真\n社交：慢热".into(),
            persona_json: Some(serde_json::json!({
                "summary": "认真，慢热，亲近之后会软。",
                "temperament": ["慢热", "嘴硬心软", "认真起来很轴"],
                "socialStyle": "先看，再靠近。",
                "speechStyle": "话短，不客套。",
            })),
            visual_profile: None,
            portrait_asset_id: None,
            portrait_generation: None,
            avatar_asset_id: None,
            avatar_generation: None,
            updated_by: None,
            updated_at: chrono::Utc::now().into(),
        };
        let payload = motion_persona_payload(Some(&blank), "restrained");
        assert_eq!(payload["name"], "瞳");
        assert_eq!(payload["motionStyle"], "restrained");
        assert!(payload["personality"].as_str().unwrap().contains("认真"));
        assert_eq!(payload["temperament"][0], "慢热");
        assert_eq!(payload["socialStyle"], "先看，再靠近。");
        let fallback = motion_persona_payload(None, "even");
        assert_eq!(fallback["name"], "Arael");
        assert_eq!(fallback["motionStyle"], "even");
        assert!(fallback["temperament"].as_array().unwrap().is_empty());
    }

    #[test]
    fn explicit_continue_is_not_a_plan() {
        assert_eq!(
            parse_motion_decision(r#"{"continue":true}"#),
            Some(MotionDecision::Continue)
        );
        assert_eq!(
            parse_motion_decision(r#"{"continue":true,"cues":[]}"#),
            Some(MotionDecision::Continue)
        );
        assert_eq!(
            parse_motion_decision("```json\n{\"continue\": true}\n```"),
            Some(MotionDecision::Continue)
        );
    }

    #[test]
    fn empty_object_is_invalid_not_continue() {
        assert_eq!(parse_motion_decision("{}"), None);
        assert_eq!(parse_motion_decision(r#"{"cues":[]}"#), None);
        assert_eq!(parse_motion_decision(r#"{"continue":false}"#), None);
    }

    #[test]
    fn illegal_baseline_without_cues_is_invalid() {
        assert_eq!(
            parse_motion_decision(
                r#"{"baseline":{"expression":"angry","posture":"attack"},"cues":[]}"#
            ),
            None
        );
    }

    #[test]
    fn legal_plan_is_perform() {
        let decision = parse_motion_decision(
            r#"{"cues":[{"intent":"listen","atMs":0,"intensity":1,"tempo":1,"fadeInMs":80,"fadeOutMs":120,"interrupt":"if-lower"}]}"#,
        );
        match decision {
            Some(MotionDecision::Perform(plan)) => {
                assert_eq!(plan.cues.len(), 1);
                assert_eq!(plan.cues[0].intent, "listen");
            }
            other => panic!("expected perform, got {other:?}"),
        }
    }

    #[test]
    fn continue_with_a_plan_is_rejected() {
        assert_eq!(
            parse_motion_decision(r#"{"continue":true,"cues":[{"intent":"listen"}]}"#),
            None
        );
        assert_eq!(
            parse_motion_decision(
                r#"{"continue":true,"baseline":{"expression":"warm","posture":"open","motionEnergy":1,"attention":0.8}}"#
            ),
            None
        );
    }

    /// Chat speaks plain prose; the director observes it on a separate task.
    #[test]
    fn streaming_chat_refines_actual_delivery_without_delaying_text() {
        let src = include_str!("../process_chat.rs");
        assert!(src.contains("let performance = None;"));
        let chat = src
            .find("stream_strict_lite_chat_response")
            .expect("chat lite call");
        let local_reaction = src
            .find("local_directive(&reaction_context)")
            .expect("local reaction");
        assert!(
            local_reaction < chat,
            "Chat must react before the reply stream starts"
        );
        assert!(src.contains("spawn_chat_motion_refinement(reaction_context"));
        let streaming = include_str!("../confirmation_and_tasks/chat_stream.rs");
        assert!(streaming.contains("emit_chat_delta(&tx, delta, speech_delivery.as_ref()).await"));
        assert!(!streaming.contains("SpeechDeliveryStream"));
        assert!(!include_str!("../chat_prompt.rs").contains("[[delivery:"));
    }

    #[test]
    fn immediate_reaction_precedes_text_and_landing_cannot_delay_stream_close() {
        let src = include_str!("../process_chat.rs");
        let floor = src.find("local_directive(&reaction_context)").unwrap();
        let delivery = src.find("local_directive(&delivery_context)").unwrap();
        assert!(floor < delivery);
        let diary = src[floor..].find("note_chat_diary").unwrap() + floor;
        let chat = src[floor..]
            .find("stream_strict_lite_chat_response")
            .unwrap()
            + floor;
        assert!(floor < diary && diary < chat);
        assert!(chat < delivery);
        let finish = src
            .find("response_agent::finish_stream(&progress_tx)")
            .unwrap();
        assert!(finish < delivery);
        let stop = src.find("guard.stop().await").unwrap();
        assert!(finish < stop && stop < delivery);
        let overlay = include_str!("../motion_overlay.rs");
        assert!(overlay.contains("self.task.abort();"));
        assert!(src[delivery..].contains("performance.plan.cues.clear()"));
    }

    #[test]
    fn chat_director_phrase_only_decision_needs_no_replacement_pose() {
        assert_eq!(
            parse_motion_decision(r#"{"phrases":[{"text":"你觉得呢？","intent":"check-in"}]}"#),
            Some(MotionDecision::Perform(ChatPerformancePlan::default()))
        );
        for invalid in [
            r#"{"phrases":[{"text":"你觉得呢？","intent":"driver"}]}"#,
            r#"{"baseline":{},"phrases":[{"text":"你觉得呢？","intent":"ask"}]}"#,
            r#"{"continue":true,"phrases":[{"text":"你觉得呢？","intent":"ask"}]}"#,
        ] {
            assert_eq!(parse_motion_decision(invalid), None);
        }
    }

    /// Production dropped 196 of 217 calls sitting exactly on the old 4s wall;
    /// the two that returned took 4065ms and 7168ms. The budget is only allowed
    /// to be this wide because no caller waits on it — see `local_directive`.
    #[test]
    fn motion_lite_budget_clears_the_observed_success_latency() {
        assert!(MOTION_TIMEOUT >= Duration::from_secs(8));
        assert!(MOTION_TOTAL_TIMEOUT > MOTION_TIMEOUT);
        let src = concat!(
            include_str!("../process_and_recipe.rs"),
            include_str!("../process_chat.rs"),
            include_str!("../process_work.rs")
        );
        assert!(
            !src.contains("handle.await.ok().flatten()"),
            "no request path may block on the director's budget"
        );
    }

    #[test]
    fn a_dropped_lite_call_still_leaves_the_round_something_to_play() {
        let plan = local_performance_plan(
            MotionPhase::Delivery,
            &MoodTransition {
                before: 50.0,
                after: 50.0,
                arousal_before: 48.0,
                arousal_after: 48.0,
                band_before: "calm".to_string(),
                band_after: "calm".to_string(),
                delta: 0.0,
                cause: "test".to_string(),
                revision: 1,
            },
            None,
            Some("已经好了。"),
            "even",
            None,
        );
        assert!(!plan_is_empty(&plan));
    }

    #[test]
    fn user_can_ask_for_a_named_expression() {
        assert_eq!(user_requested_cue("你能做一下呆呆的表情吗"), Some("silly"));
        assert_eq!(user_requested_cue("做一下狂笑"), Some("maniac"));
        assert_eq!(user_requested_cue("狂笑一下"), Some("maniac"));
        assert_eq!(user_requested_cue("来个哭脸"), Some("cry"));
        assert_eq!(user_requested_cue("哭一下"), Some("cry"));
        assert_eq!(user_requested_cue("make a silly face"), Some("silly"));
        assert_eq!(user_requested_cue("好开心"), None);
        assert_eq!(user_requested_cue("昨晚我狂笑了一路"), None);
        assert_eq!(user_requested_cue("做点好玩的表情"), None);
        assert_eq!(user_requested_cue("你能告诉我昨天狂笑的事吗"), None);
        assert_eq!(user_requested_cue("说说你犯蠢的事吧"), None);
        // A Latin alias inside a longer word is not an ask, even next to a
        // request marker: `cry` lives in `cryptic`, `think` in `thinking`.
        assert_eq!(user_requested_cue("make a cryptic joke"), None);
        assert_eq!(user_requested_cue("make a plan, thinking it through"), None);
        assert_eq!(user_requested_cue("make a think face"), Some("think"));
    }

    #[test]
    fn asked_expression_waits_for_the_character_to_answer() {
        assert_eq!(
            play_along_requested_cue(MotionPhase::Reaction, "做一下狂笑", None),
            None
        );
        assert_eq!(
            play_along_requested_cue(MotionPhase::Delivery, "做一下狂笑", None),
            None
        );
        assert_eq!(
            play_along_requested_cue(MotionPhase::Delivery, "做一下狂笑", Some("  ")),
            None
        );
        assert_eq!(
            play_along_requested_cue(MotionPhase::Delivery, "做一下狂笑", Some("才不给你做。")),
            None
        );
        assert_eq!(
            play_along_requested_cue(MotionPhase::Delivery, "做一下狂笑", Some("好啊，看我的。")),
            Some("maniac")
        );
    }

    #[test]
    fn asked_expression_survives_lite_failure_when_the_face_can_play_it() {
        let plan = apply_user_requested_cue(
            ChatPerformancePlan::default(),
            MotionPhase::Delivery,
            "做一下狂笑",
            Some("好啊，看我的。"),
            None,
        );
        assert_eq!(plan.cues[0].intent, "maniac");
        assert_eq!(plan.cues[0].fade_in_ms, 180);
        assert_eq!(plan.cues[0].fade_out_ms, 420);
        assert_eq!(plan.baseline.as_ref().unwrap().expression, "steady");
        let cry = apply_user_requested_cue(
            ChatPerformancePlan::default(),
            MotionPhase::Delivery,
            "来个哭脸",
            Some("行，给你哭一个。"),
            None,
        );
        assert_eq!(cry.baseline.as_ref().unwrap().expression, "steady");
        let blocked = myriad_merope::sanitize_rig_state(&serde_json::json!({
            "capabilities": ["head-body"]
        }))
        .unwrap();
        let skipped = apply_user_requested_cue(
            ChatPerformancePlan::default(),
            MotionPhase::Delivery,
            "做一下狂笑",
            Some("好啊，看我的。"),
            Some(&blocked),
        );
        assert!(skipped.cues.is_empty());
        let too_early = apply_user_requested_cue(
            ChatPerformancePlan::default(),
            MotionPhase::Reaction,
            "做一下狂笑",
            None,
            None,
        );
        assert!(too_early.cues.is_empty());
    }

    #[test]
    fn asked_mouth_expression_plays_even_while_speaking() {
        let state = myriad_merope::sanitize_rig_state(&serde_json::json!({
            "speaking": true,
            "capabilities": ["head-body", "maniac-mouth"]
        }))
        .unwrap();
        let lite = parse_performance_plan(
            r#"{"cues":[{"intent":"listen","atMs":0,"intensity":1,"tempo":1,"fadeInMs":80,"fadeOutMs":120,"interrupt":"replace"},{"intent":"maniac","atMs":0,"intensity":1,"tempo":1,"fadeInMs":80,"fadeOutMs":120,"interrupt":"replace"}]}"#,
        )
        .unwrap();
        let refined = refine_performance_plan(lite, &state);
        assert!(refined.cues.iter().all(|cue| cue.intent != "maniac"));
        let plan = apply_user_requested_cue(
            refined,
            MotionPhase::Delivery,
            "做一下狂笑",
            Some("好啊，看我的。"),
            Some(&state),
        );
        assert_eq!(plan.cues[0].intent, "maniac");
    }

    #[test]
    fn hidden_face_skips_motion() {
        let hidden = myriad_merope::sanitize_rig_state(&serde_json::json!({
            "pageVisible": false,
            "faceVisible": true
        }))
        .unwrap();
        assert!(face_is_hidden(Some(&hidden)));
        let no_face = myriad_merope::sanitize_rig_state(&serde_json::json!({
            "pageVisible": true,
            "faceVisible": false
        }))
        .unwrap();
        assert!(face_is_hidden(Some(&no_face)));
        assert!(!face_is_hidden(None));
    }
}
