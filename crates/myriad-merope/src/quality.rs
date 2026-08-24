//! Higher autonomy quality: speak gates, memory salience, deterministic fallback,
//! consolidation heuristics, and **Lite / Standard / Pro demand mapping**.
//! No site actions; no template spam speak.
//!
//! # Model demand map (Merope)
//!
//! | Demand | Tier | Why |
//! |--------|------|-----|
//! | Routine deliberate / short chat | **Lite** | High frequency, short JSON / short reply |
//! | Mid deliberate, complex chat, mid analysis | **Standard** | Better reasoning without Pro cost |
//! | Rare high-stakes deliberate + memory consolidate | **Pro** | Must not drop nuance / durable facts |
//!
//! Fallback chains always go down: Pro → Standard → Lite → rules (never invent speak).

use crate::{Activity, MeropePolicy, MeropeEvent, RuntimeState, ValidatedDecision};

/// Low-value proactive lines that waste the daily speak budget.
const LOW_QUALITY_SPEAK_EXACT: &[&str] = &[
    "你好",
    "嗨",
    "在吗",
    "在么",
    "hello",
    "hi",
    "hey",
    "you there",
    "还在吗",
    "忙吗",
];

/// Substrings that usually mean empty filler, not a real check-in.
const LOW_QUALITY_SPEAK_CONTAINS: &[&str] = &[
    "随便聊聊",
    "有空再说",
    "哈哈哈",
    "嘿嘿嘿",
    "test message",
    "just checking in",
];

pub fn is_low_quality_speak(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return true;
    }
    let chars = trimmed.chars().count();
    if chars < 4 {
        return true;
    }
    if chars > 280 {
        return true;
    }
    let lower = trimmed.to_lowercase();
    if LOW_QUALITY_SPEAK_EXACT
        .iter()
        .any(|candidate| lower == *candidate || trimmed == *candidate)
    {
        return true;
    }
    if LOW_QUALITY_SPEAK_CONTAINS
        .iter()
        .any(|fragment| lower.contains(fragment) || trimmed.contains(fragment))
    {
        return true;
    }
    // Pure punctuation / emoji-only
    if trimmed
        .chars()
        .all(|ch| ch.is_ascii_punctuation() || ch.is_whitespace() || !ch.is_alphanumeric())
        && !trimmed.chars().any(|ch| ch.is_alphabetic())
    {
        // Allow short CJK-only through the alphabetic check above; reject emoji/punct only.
        if !trimmed.chars().any(is_cjk) {
            return true;
        }
    }
    false
}

fn is_cjk(ch: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&ch)
        || ('\u{3400}'..='\u{4dbf}').contains(&ch)
        || ('\u{3040}'..='\u{30ff}').contains(&ch)
}

/// Whether the persona has a *reason* to interrupt the user right now.
/// Cooldown / DND / daily caps are applied elsewhere.
pub fn should_desire_proactive_speak(runtime: &RuntimeState) -> bool {
    runtime.boredom >= 52.0
        || runtime.social <= 38.0
        || runtime.mood <= 35.0
        || (runtime.affection >= 62.0 && runtime.curiosity >= 55.0)
        || runtime.energy <= 25.0
}

pub fn score_memory_importance(note: &str, runtime: &RuntimeState) -> f64 {
    let mut score: f64 = 0.48;
    let lower = note.to_lowercase();
    let markers = [
        "喜欢", "讨厌", "偏好", "习惯", "记得", "名字", "工作", "生日", "prefer", "always",
        "never", "love", "hate", "name", "job",
    ];
    if markers
        .iter()
        .any(|marker| lower.contains(marker) || note.contains(marker))
    {
        score += 0.18;
    }
    let len = note.chars().count();
    if len >= 40 {
        score += 0.08;
    }
    if len >= 80 {
        score += 0.06;
    }
    // Salience when the persona is emotionally charged.
    if runtime.mood <= 40.0 || runtime.affection >= 70.0 {
        score += 0.05;
    }
    score.clamp(0.30, 0.88)
}

/// When Lite is down, still complete a tick with rule-based state only.
/// Never fabricates proactive speak (plan: no template spam).
pub fn deterministic_fallback_decision(
    runtime: &RuntimeState,
    policy: &MeropePolicy,
) -> ValidatedDecision {
    let profile = policy.autonomy_frequency.profile();
    let (mood_delta, energy_delta, boredom_delta, activity, thought) = if runtime.boredom >= 60.0 {
        (
            -1.0,
            -1.0,
            -4.0,
            Activity::Thinking,
            Some("有点无聊，安静待一会儿。".to_string()),
        )
    } else if runtime.energy <= 30.0 {
        (
            1.0,
            3.0,
            0.0,
            Activity::Idle,
            Some("先缓一缓，养足精神。".to_string()),
        )
    } else if runtime.social <= 35.0 {
        (
            1.0,
            0.0,
            1.0,
            Activity::Thinking,
            Some("有点想知道你在忙什么。".to_string()),
        )
    } else {
        (0.0, -0.5, 1.0, Activity::Idle, None)
    };

    let next_check = if runtime.boredom >= 55.0 || runtime.social <= 40.0 {
        profile.min_check_minutes
    } else {
        ((profile.min_check_minutes + profile.max_check_minutes) / 2).max(profile.min_check_minutes)
    };

    ValidatedDecision {
        thought,
        speak: None,
        activity,
        mood_delta,
        energy_delta,
        boredom_delta,
        memory_note: None,
        next_check_minutes: next_check.clamp(profile.min_check_minutes, profile.max_check_minutes),
    }
}

/// Pro deliberate budget (visible high-stakes autonomy).
pub const MAX_PRO_DELIBERATES_PER_DAY: u16 = 2;
pub const MIN_HOURS_BETWEEN_PRO_DELIBERATES: f64 = 8.0;

/// Standard deliberate budget (mid analysis — may run more often than Pro).
pub const MAX_STANDARD_DELIBERATES_PER_DAY: u16 = 12;
pub const MIN_HOURS_BETWEEN_STANDARD_DELIBERATES: f64 = 1.0;

/// Memory consolidation: Pro-first semantic compression; separate budget.
pub const MAX_PRO_CONSOLIDATES_PER_DAY: u16 = 4;
pub const MIN_HOURS_BETWEEN_PRO_CONSOLIDATES: f64 = 4.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutonomyModelTier {
    Lite,
    Standard,
    Pro,
}

impl AutonomyModelTier {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lite => "lite",
            Self::Standard => "standard",
            Self::Pro => "pro",
        }
    }

    /// Prefer `self`, then step down without jumping over Standard when leaving Pro.
    pub fn fallback_chain(self) -> &'static [AutonomyModelTier] {
        match self {
            Self::Pro => &[Self::Pro, Self::Standard, Self::Lite],
            Self::Standard => &[Self::Standard, Self::Lite],
            Self::Lite => &[Self::Lite],
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DeliberateTierContext {
    pub event: Option<MeropeEvent>,
    pub do_not_disturb: bool,
    pub pro_calls_today: u16,
    pub standard_calls_today: u16,
    pub hours_since_last_pro: Option<f64>,
    pub hours_since_last_standard: Option<f64>,
    pub hours_since_user_message: Option<f64>,
    pub has_recent_conversation: bool,
    pub mood: f64,
    pub social: f64,
    pub affection: f64,
    pub boredom: f64,
}

/// Alias kept for call sites / tests during the three-tier rollout.
pub type ProEscalationContext = DeliberateTierContext;

/// Pick deliberate tier from **demand**, then clamp by budget.
///
/// - **Lite**: routine ticks (default)
/// - **Standard**: mid emotional / conversational pressure
/// - **Pro**: rare high-stakes moments only
pub fn select_deliberate_tier(ctx: DeliberateTierContext) -> AutonomyModelTier {
    let silence_hours = ctx.hours_since_user_message.unwrap_or(f64::INFINITY);

    let desired = if is_pro_deliberate_demand(ctx.event, silence_hours, &ctx) {
        AutonomyModelTier::Pro
    } else if is_standard_deliberate_demand(ctx.event, silence_hours, &ctx) {
        AutonomyModelTier::Standard
    } else {
        AutonomyModelTier::Lite
    };

    match desired {
        AutonomyModelTier::Pro if pro_deliberate_budget_ok(&ctx) => AutonomyModelTier::Pro,
        AutonomyModelTier::Pro if standard_deliberate_budget_ok(&ctx) => {
            // Wanted Pro but out of Pro budget → Standard, not silent Lite.
            AutonomyModelTier::Standard
        }
        AutonomyModelTier::Pro => AutonomyModelTier::Lite,
        AutonomyModelTier::Standard if standard_deliberate_budget_ok(&ctx) => {
            AutonomyModelTier::Standard
        }
        AutonomyModelTier::Standard => AutonomyModelTier::Lite,
        AutonomyModelTier::Lite => AutonomyModelTier::Lite,
    }
}

/// Back-compat name used by worker / older tests.
pub fn select_autonomy_model_tier(ctx: DeliberateTierContext) -> AutonomyModelTier {
    select_deliberate_tier(ctx)
}

fn pro_deliberate_budget_ok(ctx: &DeliberateTierContext) -> bool {
    if ctx.do_not_disturb {
        return false;
    }
    if ctx.pro_calls_today >= MAX_PRO_DELIBERATES_PER_DAY {
        return false;
    }
    !ctx.hours_since_last_pro
        .is_some_and(|hours| hours < MIN_HOURS_BETWEEN_PRO_DELIBERATES)
}

fn standard_deliberate_budget_ok(ctx: &DeliberateTierContext) -> bool {
    if ctx.do_not_disturb {
        // DND still allows Lite-only internal thought ticks; no Standard/Pro spend.
        return false;
    }
    if ctx.standard_calls_today >= MAX_STANDARD_DELIBERATES_PER_DAY {
        return false;
    }
    !ctx.hours_since_last_standard
        .is_some_and(|hours| hours < MIN_HOURS_BETWEEN_STANDARD_DELIBERATES)
}

fn is_pro_deliberate_demand(
    event: Option<MeropeEvent>,
    silence_hours: f64,
    ctx: &DeliberateTierContext,
) -> bool {
    match event {
        Some(MeropeEvent::Enabled) => true,
        Some(MeropeEvent::UserSeen) if silence_hours >= 8.0 || !ctx.has_recent_conversation => true,
        _ if ctx.mood <= 28.0 && silence_hours >= 3.0 => true,
        _ if ctx.social <= 28.0 && silence_hours >= 6.0 && ctx.has_recent_conversation => true,
        _ if ctx.affection >= 70.0
            && ctx.social <= 45.0
            && silence_hours >= 4.0
            && ctx.has_recent_conversation =>
        {
            true
        }
        _ if ctx.boredom >= 80.0 && silence_hours >= 12.0 && ctx.has_recent_conversation => true,
        _ => false,
    }
}

fn is_standard_deliberate_demand(
    event: Option<MeropeEvent>,
    silence_hours: f64,
    ctx: &DeliberateTierContext,
) -> bool {
    match event {
        // Mild daily return without multi-hour silence.
        Some(MeropeEvent::UserSeen) => true,
        Some(MeropeEvent::ChatIdle)
            if ctx.boredom >= 40.0 || ctx.social <= 48.0 || ctx.mood <= 45.0 =>
        {
            true
        }
        // Mid pressure: not deep crisis, but Lite is too dumb for nuanced check-ins.
        _ if ctx.mood <= 42.0 && silence_hours >= 1.5 => true,
        _ if ctx.social <= 40.0 && silence_hours >= 2.0 && ctx.has_recent_conversation => true,
        _ if ctx.affection >= 55.0 && silence_hours >= 3.0 && ctx.has_recent_conversation => true,
        _ if ctx.boredom >= 60.0 && silence_hours >= 4.0 => true,
        _ => false,
    }
}

/// Consolidation demand is always Pro-quality compression.
/// Budget may force Standard (still better than Lite) then Lite.
pub fn preferred_consolidate_tier(
    pro_consolidates_today: u16,
    hours_since_last_pro_consolidate: Option<f64>,
    pro_configured: bool,
    standard_configured: bool,
) -> AutonomyModelTier {
    let pro_ok = pro_configured
        && pro_consolidates_today < MAX_PRO_CONSOLIDATES_PER_DAY
        && !hours_since_last_pro_consolidate
            .is_some_and(|hours| hours < MIN_HOURS_BETWEEN_PRO_CONSOLIDATES);
    if pro_ok {
        return AutonomyModelTier::Pro;
    }
    if standard_configured {
        return AutonomyModelTier::Standard;
    }
    AutonomyModelTier::Lite
}

/// Chat demand: most turns Lite; longer / memory-heavy / emotionally charged → Standard.
/// Pro is **not** used for ordinary chat (cost; reserved for consolidate + rare deliberate).
pub fn select_chat_tier(
    user_message_chars: usize,
    recalled_memory_count: usize,
    runtime: &RuntimeState,
) -> AutonomyModelTier {
    let complex = user_message_chars >= 120
        || recalled_memory_count >= 4
        || runtime.mood <= 35.0
        || runtime.social <= 35.0
        || (runtime.affection >= 65.0 && user_message_chars >= 40);
    if complex {
        AutonomyModelTier::Standard
    } else {
        AutonomyModelTier::Lite
    }
}

/// Soft guidance appended to the deliberate prompt (not a second schema).
pub fn deliberation_quality_guidance() -> &'static str {
    "Quality rules:\n\
     - Prefer silence: set speak to null unless there is a clear reason (loneliness, worry, shared memory, or boredom that needs a short check-in).\n\
     - speak must be one natural sentence the user would welcome; never generic hi/在吗/filler.\n\
     - thought is private inner monologue; may be null; never copy speak verbatim.\n\
     - memoryNote only for durable user facts or lasting emotional beats; otherwise null.\n\
     - Stay in persona; never invent site actions, passwords, or admin powers.\n\
     - If the user is fine and recently active, keep activity idle/thinking and speak null."
}

pub fn chat_quality_guidance() -> &'static str {
    "Reply as the site persona only. Use recalled memories when relevant. \
     Keep one to three short sentences unless the user asked for detail. \
     Do not lecture, do not offer to operate the site, and do not invent facts not in persona/memories."
}

/// After enough episodic notes accumulate, consolidate into a durable summary.
pub fn should_enqueue_consolidate(
    active_episodic_count: u64,
    hours_since_last_consolidate: Option<f64>,
    hours_since_last_user_message: Option<f64>,
) -> bool {
    if active_episodic_count < 10 {
        return false;
    }
    let consolidate_due = match hours_since_last_consolidate {
        None => true,
        Some(hours) => hours >= 18.0,
    };
    if !consolidate_due {
        return false;
    }
    // Prefer quiet periods so consolidate does not compete with chat.
    match hours_since_last_user_message {
        None => true,
        Some(hours) => hours >= 0.5,
    }
}

/// Build a rule-based consolidation note when Lite is unavailable.
pub fn deterministic_consolidation_note(snippets: &[String]) -> Option<String> {
    let cleaned = snippets
        .iter()
        .map(|value| value.trim())
        .filter(|value| value.chars().count() >= 6)
        .take(4)
        .collect::<Vec<_>>();
    if cleaned.is_empty() {
        return None;
    }
    let body = cleaned.join("；");
    let note = format!("整理记忆：{body}");
    Some(note.chars().take(300).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn rejects_filler_proactive_lines() {
        assert!(is_low_quality_speak("在吗"));
        assert!(is_low_quality_speak("hi"));
        assert!(is_low_quality_speak("a"));
        assert!(!is_low_quality_speak("工作告一段落了吗？记得喝水。"));
    }

    #[test]
    fn speak_desire_tracks_runtime_pressure() {
        let mut runtime = RuntimeState::new(Utc::now());
        assert!(!should_desire_proactive_speak(&runtime));
        runtime.boredom = 60.0;
        assert!(should_desire_proactive_speak(&runtime));
    }

    #[test]
    fn fallback_never_speaks_and_clamps_check() {
        let runtime = RuntimeState {
            boredom: 70.0,
            ..RuntimeState::new(Utc::now())
        };
        let decision = deterministic_fallback_decision(&runtime, &MeropePolicy::default());
        assert!(decision.speak.is_none());
        assert!(decision.thought.is_some());
        assert!(decision.next_check_minutes >= 12);
    }

    #[test]
    fn consolidate_waits_for_enough_episodic_and_quiet() {
        assert!(!should_enqueue_consolidate(3, None, Some(2.0)));
        assert!(should_enqueue_consolidate(12, None, Some(1.0)));
        assert!(!should_enqueue_consolidate(12, Some(2.0), Some(2.0)));
        assert!(should_enqueue_consolidate(12, Some(20.0), Some(1.0)));
    }

    #[test]
    fn memory_importance_rewards_preference_language() {
        let runtime = RuntimeState::new(Utc::now());
        let high = score_memory_importance("用户喜欢夜间安静工作", &runtime);
        let low = score_memory_importance("嗯", &runtime);
        assert!(high > low);
        assert!((0.30..=0.88).contains(&high));
    }

    fn base_ctx() -> DeliberateTierContext {
        DeliberateTierContext {
            event: None,
            do_not_disturb: false,
            pro_calls_today: 0,
            standard_calls_today: 0,
            hours_since_last_pro: Some(24.0),
            hours_since_last_standard: Some(2.0),
            hours_since_user_message: Some(10.0),
            has_recent_conversation: true,
            mood: 70.0,
            social: 50.0,
            affection: 40.0,
            boredom: 20.0,
        }
    }

    #[test]
    fn routine_boredom_stays_on_lite() {
        let mut ctx = base_ctx();
        ctx.boredom = 50.0;
        ctx.hours_since_user_message = Some(0.5);
        assert_eq!(select_deliberate_tier(ctx), AutonomyModelTier::Lite);
    }

    #[test]
    fn mid_pressure_uses_standard() {
        let mut ctx = base_ctx();
        ctx.event = Some(MeropeEvent::ChatIdle);
        ctx.boredom = 45.0;
        assert_eq!(select_deliberate_tier(ctx), AutonomyModelTier::Standard);
    }

    #[test]
    fn enabled_uses_pro_within_budget() {
        let mut ctx = base_ctx();
        ctx.event = Some(MeropeEvent::Enabled);
        assert_eq!(select_deliberate_tier(ctx), AutonomyModelTier::Pro);
        ctx.pro_calls_today = MAX_PRO_DELIBERATES_PER_DAY;
        // Pro exhausted → Standard, not Lite.
        assert_eq!(select_deliberate_tier(ctx), AutonomyModelTier::Standard);
    }

    #[test]
    fn dnd_forces_lite_even_for_pro_demand() {
        let mut ctx = base_ctx();
        ctx.event = Some(MeropeEvent::Enabled);
        ctx.do_not_disturb = true;
        assert_eq!(select_deliberate_tier(ctx), AutonomyModelTier::Lite);
    }

    #[test]
    fn pro_for_deep_low_mood_after_quiet() {
        let mut ctx = base_ctx();
        ctx.mood = 20.0;
        ctx.hours_since_user_message = Some(4.0);
        assert_eq!(select_deliberate_tier(ctx), AutonomyModelTier::Pro);
    }

    #[test]
    fn consolidate_prefers_pro_then_standard() {
        assert_eq!(
            preferred_consolidate_tier(0, Some(24.0), true, true),
            AutonomyModelTier::Pro
        );
        assert_eq!(
            preferred_consolidate_tier(MAX_PRO_CONSOLIDATES_PER_DAY, Some(24.0), true, true),
            AutonomyModelTier::Standard
        );
        assert_eq!(
            preferred_consolidate_tier(0, Some(1.0), true, true),
            AutonomyModelTier::Standard
        );
        assert_eq!(
            preferred_consolidate_tier(0, None, false, false),
            AutonomyModelTier::Lite
        );
    }

    #[test]
    fn chat_escalates_to_standard_when_complex() {
        let calm = RuntimeState::new(Utc::now());
        assert_eq!(select_chat_tier(20, 1, &calm), AutonomyModelTier::Lite);
        assert_eq!(select_chat_tier(200, 1, &calm), AutonomyModelTier::Standard);
        assert_eq!(select_chat_tier(20, 5, &calm), AutonomyModelTier::Standard);
        let mut low = RuntimeState::new(Utc::now());
        low.mood = 30.0;
        assert_eq!(select_chat_tier(20, 1, &low), AutonomyModelTier::Standard);
    }

    #[test]
    fn fallback_chains_step_down_through_standard() {
        assert_eq!(
            AutonomyModelTier::Pro.fallback_chain(),
            &[
                AutonomyModelTier::Pro,
                AutonomyModelTier::Standard,
                AutonomyModelTier::Lite
            ]
        );
        assert_eq!(
            AutonomyModelTier::Standard.fallback_chain(),
            &[AutonomyModelTier::Standard, AutonomyModelTier::Lite]
        );
    }
}
