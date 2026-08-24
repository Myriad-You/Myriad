use crate::{AutonomyFrequency, MeropePolicy};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Activity {
    #[default]
    Idle,
    Thinking,
    Talking,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeState {
    pub energy: f64,
    pub mood: f64,
    pub boredom: f64,
    pub curiosity: f64,
    pub social: f64,
    pub affection: f64,
    pub activity: Activity,
    pub thought: Option<String>,
    pub thought_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub last_user_seen_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub last_chat_idle_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
}

impl RuntimeState {
    pub fn new(now: DateTime<Utc>) -> Self {
        Self {
            energy: 80.0,
            mood: 70.0,
            boredom: 20.0,
            curiosity: 60.0,
            social: 50.0,
            affection: 40.0,
            activity: Activity::Idle,
            thought: None,
            thought_at: None,
            last_user_seen_at: None,
            last_chat_idle_at: None,
            updated_at: now,
        }
    }

    pub fn sanitize(&mut self) {
        self.energy = clamp_score(self.energy);
        self.mood = clamp_score(self.mood);
        self.boredom = clamp_score(self.boredom);
        self.curiosity = clamp_score(self.curiosity);
        self.social = clamp_score(self.social);
        self.affection = clamp_score(self.affection);
    }
}

impl Default for RuntimeState {
    fn default() -> Self {
        Self::new(Utc::now())
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CatchUpResult {
    pub elapsed_hours: f64,
    pub decay_multiplier: f64,
}

pub fn catch_up(
    state: &mut RuntimeState,
    elapsed: Duration,
    frequency: AutonomyFrequency,
    now: DateTime<Utc>,
) -> CatchUpResult {
    let elapsed_hours = (elapsed.num_seconds().max(0) as f64 / 3_600.0).clamp(0.0, 72.0);
    let decay_multiplier = frequency.profile().decay_multiplier;
    state.energy -= 2.0 * elapsed_hours * decay_multiplier;
    state.boredom += 3.0 * elapsed_hours * decay_multiplier;
    state.curiosity -= 0.5 * elapsed_hours * decay_multiplier;
    state.mood -= 0.4 * elapsed_hours * decay_multiplier;
    state.social -= 0.8 * elapsed_hours * decay_multiplier;
    state.affection -= 0.2 * elapsed_hours * decay_multiplier;
    state.sanitize();
    state.updated_at = now;
    CatchUpResult {
        elapsed_hours,
        decay_multiplier,
    }
}

pub fn apply_chat_message_buff(state: &mut RuntimeState, now: DateTime<Utc>) {
    state.mood += 4.0;
    state.social += 6.0;
    state.boredom -= 10.0;
    state.affection += 2.0;
    state.sanitize();
    state.updated_at = now;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharacterStatus {
    Active,
    Disabled,
    Deleted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeropeEvent {
    Enabled,
    UserSeen,
    ChatMessage,
    ChatIdle,
}

pub fn apply_merope_event(state: &mut RuntimeState, event: MeropeEvent, now: DateTime<Utc>) -> bool {
    match event {
        MeropeEvent::Enabled => {
            state.curiosity += 12.0;
            state.mood += 3.0;
        }
        MeropeEvent::UserSeen => {
            if state
                .last_user_seen_at
                .is_some_and(|seen_at| seen_at.date_naive() == now.date_naive())
            {
                return false;
            }
            state.social += 5.0;
            state.mood += 2.0;
            state.last_user_seen_at = Some(now);
        }
        MeropeEvent::ChatMessage => apply_chat_message_buff(state, now),
        MeropeEvent::ChatIdle => {
            state.boredom += 4.0;
            state.curiosity += 2.0;
            state.last_chat_idle_at = Some(now);
        }
    }
    state.sanitize();
    state.updated_at = now;
    true
}

pub const CHAT_IDLE_AFTER_MINUTES: i64 = 30;

pub fn should_apply_chat_idle(
    last_user_message_at: Option<DateTime<Utc>>,
    last_chat_idle_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> bool {
    let Some(last_user_message_at) = last_user_message_at else {
        return false;
    };
    now - last_user_message_at >= Duration::minutes(CHAT_IDLE_AFTER_MINUTES)
        && last_chat_idle_at.is_none_or(|last_idle| last_idle < last_user_message_at)
}

#[derive(Debug, Clone)]
pub struct DeliberationContext<'a> {
    pub status: CharacterStatus,
    pub policy: &'a MeropePolicy,
    pub runtime: &'a RuntimeState,
    pub now: DateTime<Utc>,
    pub last_deliberated_at: Option<DateTime<Utc>>,
    pub next_deliberation_at: Option<DateTime<Utc>>,
    pub last_user_message_at: Option<DateTime<Utc>>,
    pub last_speak_at: Option<DateTime<Utc>>,
    pub last_simulated_at: DateTime<Utc>,
    pub has_queued_or_running_deliberate: bool,
    pub deliberate_calls_today: u16,
    pub event: Option<MeropeEvent>,
}

pub fn should_deliberate(context: &DeliberationContext<'_>) -> bool {
    if context.status != CharacterStatus::Active
        || !context.policy.enabled
        || context.has_queued_or_running_deliberate
        || context.deliberate_calls_today >= 48
    {
        return false;
    }
    if elapsed_minutes(context.last_deliberated_at, context.now)
        < context.policy.min_deliberate_interval_minutes as i64
    {
        return false;
    }
    if context
        .next_deliberation_at
        .is_some_and(|next| context.now < next)
    {
        return false;
    }

    let profile = context.policy.autonomy_frequency.profile();
    let boredom_due = context.runtime.boredom >= 55.0;
    let social_due = elapsed_minutes(context.last_user_message_at, context.now) >= 45
        && context.runtime.social <= 45.0;
    let low_mood_due = context.runtime.mood <= 32.0
        && elapsed_minutes(context.last_user_message_at, context.now) >= 20;
    let event_due = match context.event {
        Some(MeropeEvent::Enabled) => true,
        Some(MeropeEvent::UserSeen) => {
            elapsed_minutes(context.last_speak_at, context.now)
                >= context.policy.min_speak_interval_minutes as i64
        }
        // Mild deliberate after a quiet stretch in the current conversation.
        Some(MeropeEvent::ChatIdle) => {
            context.runtime.boredom >= 40.0 || context.runtime.social <= 48.0
        }
        Some(MeropeEvent::ChatMessage) | None => false,
    };
    let fallback_due =
        (context.now - context.last_simulated_at).num_minutes() >= profile.max_check_minutes as i64;

    boredom_due || social_due || low_mood_due || event_due || fallback_due
}

fn elapsed_minutes(earlier: Option<DateTime<Utc>>, now: DateTime<Utc>) -> i64 {
    earlier
        .map(|value| (now - value).num_minutes().max(0))
        .unwrap_or(i64::MAX)
}

fn clamp_score(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 100.0)
    } else {
        50.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catch_up_clamps_elapsed_and_scores() {
        let now = Utc::now();
        let mut state = RuntimeState::new(now - Duration::days(10));
        let result = catch_up(
            &mut state,
            Duration::days(10),
            AutonomyFrequency::Normal,
            now,
        );
        assert_eq!(result.elapsed_hours, 72.0);
        assert_eq!(state.energy, 0.0);
        assert_eq!(state.boredom, 100.0);
        assert_eq!(state.updated_at, now);
    }

    #[test]
    fn deliberation_requires_all_gates_and_one_trigger() {
        let now = Utc::now();
        let policy = MeropePolicy::default();
        let mut runtime = RuntimeState::new(now);
        runtime.boredom = 55.0;
        let context = DeliberationContext {
            status: CharacterStatus::Active,
            policy: &policy,
            runtime: &runtime,
            now,
            last_deliberated_at: Some(now - Duration::minutes(13)),
            next_deliberation_at: None,
            last_user_message_at: Some(now - Duration::minutes(10)),
            last_speak_at: None,
            last_simulated_at: now - Duration::minutes(10),
            has_queued_or_running_deliberate: false,
            deliberate_calls_today: 0,
            event: None,
        };
        assert!(should_deliberate(&context));

        let mut blocked = context.clone();
        blocked.has_queued_or_running_deliberate = true;
        assert!(!should_deliberate(&blocked));

        let mut deferred = context;
        deferred.next_deliberation_at = Some(now + Duration::minutes(1));
        assert!(!should_deliberate(&deferred));
    }

    #[test]
    fn user_seen_resonates_only_once_per_utc_day() {
        let now = DateTime::parse_from_rfc3339("2026-03-02T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut runtime = RuntimeState::new(now);
        assert!(apply_merope_event(&mut runtime, MeropeEvent::UserSeen, now));
        let social = runtime.social;
        assert!(!apply_merope_event(
            &mut runtime,
            MeropeEvent::UserSeen,
            now + Duration::hours(2)
        ));
        assert_eq!(runtime.social, social);
        assert!(apply_merope_event(
            &mut runtime,
            MeropeEvent::UserSeen,
            now + Duration::days(1)
        ));
    }

    #[test]
    fn enabled_is_an_explicit_deliberation_trigger() {
        let now = Utc::now();
        let policy = MeropePolicy::default();
        let runtime = RuntimeState::new(now);
        assert!(should_deliberate(&DeliberationContext {
            status: CharacterStatus::Active,
            policy: &policy,
            runtime: &runtime,
            now,
            last_deliberated_at: None,
            next_deliberation_at: None,
            last_user_message_at: None,
            last_speak_at: None,
            last_simulated_at: now,
            has_queued_or_running_deliberate: false,
            deliberate_calls_today: 0,
            event: Some(MeropeEvent::Enabled),
        }));
    }

    #[test]
    fn chat_idle_resonates_once_per_conversation() {
        let now = Utc::now();
        let last_message = now - Duration::minutes(CHAT_IDLE_AFTER_MINUTES);
        assert!(should_apply_chat_idle(Some(last_message), None, now));
        assert!(!should_apply_chat_idle(
            Some(last_message),
            Some(now),
            now + Duration::minutes(5)
        ));

        let mut runtime = RuntimeState::new(last_message);
        assert!(apply_merope_event(&mut runtime, MeropeEvent::ChatIdle, now));
        assert_eq!(runtime.last_chat_idle_at, Some(now));
        assert_eq!(runtime.boredom, 24.0);
    }
}
