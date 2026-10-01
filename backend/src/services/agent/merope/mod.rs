//! Merope: site persona, per-addressee state, hidden proactive speech.

pub mod api;
mod appraisal;
pub(in crate::services::agent) mod background;
pub(in crate::services::agent) mod bits;
mod call;
pub(in crate::services::agent) mod chat_days;
pub(in crate::services::agent) mod clock;
pub(in crate::services::agent) mod chat_remember;
pub(in crate::services::agent) mod curiosity;
pub(in crate::services::agent) mod doing;
pub(in crate::services::agent) mod explore;
pub(in crate::services::agent) mod gates;
pub mod group;
pub(in crate::services::agent) mod heard;
pub(in crate::services::agent) mod hearing;
pub(in crate::services::agent) mod ingest;
pub(in crate::services::agent) mod inner;
pub(in crate::services::agent) mod joining;
mod library;
pub(in crate::services::agent) mod life;
pub mod lifecycle;
mod likeness;
mod making;
pub(in crate::services::agent) mod making_sense;
pub(in crate::services::agent) mod memes;
#[cfg(test)]
mod memory_bench;
#[cfg(test)]
mod size_guard_tests;
pub(in crate::services::agent) mod memory_jobs;
pub(in crate::services::agent) mod motion;
mod motion_local;
pub(in crate::services::agent) mod motion_preview;
pub(in crate::services::agent) mod observe;
pub(in crate::services::agent) mod onboarding_ai;
mod onboarding_prompts;
pub(in crate::services::agent) mod others;
mod outfit_overlay;
mod pace;
pub(in crate::services::agent) mod playing;
mod priming;
pub(in crate::services::agent) mod reach;
pub(in crate::services::agent) mod recognizing;
pub(in crate::services::agent) mod remembering;
pub(in crate::services::agent) mod report_dna;
pub(in crate::services::agent) mod seeing;
pub(in crate::services::agent) mod self_state;
pub(in crate::services::agent) mod self_story;
mod senses;
pub(in crate::services::agent) mod serial;
pub(in crate::services::agent) mod sharing;
pub(in crate::services::agent) mod sore;
pub(in crate::services::agent) mod soup;
mod sources;
mod speaking_context;
mod speaking_prompts;
pub(in crate::services::agent) mod stage;
pub(in crate::services::agent) mod state;
pub(in crate::services::agent) mod stickers;
pub(in crate::services::agent) mod store;
pub(in crate::services::agent) mod strangers;
pub(in crate::services::agent) mod threads;
pub(in crate::services::agent) mod timing;
pub(in crate::services::agent) mod touch;
pub(in crate::services::agent) mod views;
mod vitals;
pub(in crate::services::agent) mod wander;
mod wants;

pub use chat_remember::enqueue_chat_remember;
pub use curiosity::spawn_curiosity;
pub use ingest::{
    allow_existing_notify, is_enabled, spawn as spawn_ingest, spawn_presence,
    tick_speak_intents,
};
pub use motion::{
    MotionContext, MotionPhase, PerformanceDirective, direct_motion, local_directive,
    refine_motion, resolve_round_motion_style,
};
pub use outfit_overlay::{apply_model_wear_directive, chat_wardrobe_section, overlay_outfit_id};
pub use store::{
    JsonDocumentUpdate, PersonaContractUpdate, PortraitUpdate, acquire_avatar_generation,
    acquire_portrait_generation, avatar_generation_is_pending, clear_persona_on,
    complete_avatar_generation, complete_portrait_generation, credit_music_listening,
    get_or_create_state, get_persona, get_persona_on, insert_diary, latest_diary,
    list_diary_from_sources, portrait_generation_is_pending, promote_activity,
    release_avatar_generation, release_portrait_generation, rewrite_persona_media_urls,
    set_activity, set_dnd_schedule, set_do_not_disturb, sticker_avatar_asset_id, update_affect,
    upsert_persona_on,
};

/// Logged-in users only. Guests use negative ids; heartbeat is `SYSTEM_USER_ID` (0).
pub fn is_logged_in_addressee(user_id: i32) -> bool {
    user_id > 0
}

pub async fn mark_activity(db: &sea_orm::DatabaseConnection, user_id: i32, activity: &str) {
    if !is_logged_in_addressee(user_id) {
        return;
    }
    if !is_enabled().await {
        return;
    }
    let executing = crate::services::agent::run_hub::user_executing_run_count(user_id).await;
    if activity == "idle" && executing > 1 {
        return;
    }
    if executing > 1 {
        let _ = promote_activity(db, user_id, activity).await;
    } else {
        let _ = set_activity(db, user_id, activity).await;
    }
}

use crate::models::entities::agent_persona;

/// Soul text for user-facing speech: site persona when the flag is on, else SOUL.md.
pub async fn resolve_speaking_soul() -> Option<String> {
    let enabled = crate::GLOBAL_DYNAMIC_CONFIG
        .read()
        .await
        .merope_enabled_resolved();
    if enabled {
        if let Ok(db) = crate::services::process_db::database() {
            if let Ok(Some(persona)) = get_persona(&db).await {
                if let Some(text) = format_persona(&persona) {
                    return Some(text);
                }
            }
        }
    }
    crate::services::agent::identity::get_identity()
        .await
        .and_then(|id| id.soul)
}

/// Returns the persisted mood transition for this utterance, if Merope applied.
pub async fn note_user_turn(
    db: &sea_orm::DatabaseConnection,
    request: &crate::services::agent::UserRequest,
    utterance_index: u32,
) -> Option<(MoodTransition, chrono::DateTime<chrono::FixedOffset>)> {
    let user_id = request.user_id;
    let text = &request.raw_input;
    if !is_logged_in_addressee(user_id) {
        return None;
    }
    if !crate::GLOBAL_DYNAMIC_CONFIG
        .read()
        .await
        .merope_enabled_resolved()
    {
        return None;
    }
    let (praised, scolded) = detect_mood_cue(text);
    let (previous, saved) = update_affect(db, user_id, true, |affect| {
        apply_user_utterance(affect, utterance_index, praised, scolded);
    })
    .await
    .ok()?;
    let after = store::affect_from_state(&saved);
    if !praised && !scolded && !text.trim().is_empty() {
        appraisal::spawn(db.clone(), request, &saved);
    }
    // What to try to remember is thought of while the words land.
    if !text.trim().is_empty() {
        let venue = audience_for(request).venue();
        remembering::recall_last_heard(db, user_id, &venue).await;
        remembering::begin(user_id, &venue, text);
    }
    if !is_extremely_low(previous.mood) && is_extremely_low(after.mood) {
        spawn_ingest(
            user_id,
            "agent.merope.mood_floor",
            "跟这个人的心情掉到了极低",
        );
    }
    let cause = if scolded {
        "user_scold"
    } else if praised {
        "user_praise"
    } else {
        "user_turn"
    };
    let transition = MoodTransition::from_affect(
        &previous,
        &store::affect_from_state(&saved),
        cause,
        saved
            .updated_at
            .with_timezone(&chrono::Utc)
            .timestamp_millis(),
    );
    // Carry the actual persisted input anchor, not a later mood revision.
    Some((transition, saved.last_user_message_at?))
}

/// What surrounded a chat turn, for the calls that follow it: what she said
/// just before, what was on their screen or playing, whether it was a move in
/// a game, and how many images came with it.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TurnContext {
    pub before: Option<String>,
    pub scene: Option<String>,
    pub in_game: bool,
    pub images: usize,
}

pub fn turn_context(request: &crate::services::agent::UserRequest) -> TurnContext {
    let context = request.context.as_ref();
    let before = context
        .and_then(|context| context.conversation_history.as_ref())
        .and_then(|history| {
            history
                .iter()
                .rev()
                .find(|message| message.role == "assistant")
        })
        .map(|message| {
            crate::services::agent::chat_prompt::chat_safe_content(&message.content)
                .chars()
                .take(300)
                .collect::<String>()
        })
        .filter(|line| !line.trim().is_empty());
    let custom = context.and_then(|context| context.custom_data.as_ref());
    let scene = crate::services::agent::chat_prompt::format_chat_scene(
        custom.and_then(|data| data.get("perception")),
        None,
        &request.raw_input,
    );
    TurnContext {
        before,
        scene: (!scene.trim().is_empty()).then(|| scene.chars().take(600).collect()),
        in_game: soup::in_game(request),
        images: context.map(|context| context.images.len()).unwrap_or(0),
    }
}

/// After the persona is deleted: nothing of her stays in memory either, so
/// the next one does not carry on her song, game, state or thoughts.
pub fn forget_in_memory() {
    doing::forget();
    soup::forget();
    inner::forget();
    views::forget();
    self_story::forget();
    priming::forget();
    wander::forget();
}

/// After a chat reply, let her state catch up with the exchange; the next
/// turn starts from it without waiting.
pub fn spawn_inner_after(
    db: sea_orm::DatabaseConnection,
    request: &crate::services::agent::UserRequest,
    reply: &str,
) {
    if is_logged_in_addressee(request.user_id) {
        inner::spawn_after(db, request, reply);
    }
}

/// Chat writes this before the model; Work writes after `plan_for`.
pub async fn note_chat_diary(db: &sea_orm::DatabaseConnection, user_id: i32, text: &str) {
    if !is_logged_in_addressee(user_id) {
        return;
    }
    if !is_enabled().await {
        return;
    }
    maybe_write_chat_diary(db, user_id, text).await;
}

const CHAT_DIARY_MIN_CHARS: usize = 8;
const CHAT_DIARY_GAP_MINUTES: i64 = 20;

pub fn should_write_chat_diary(text: &str, last_chat_age_minutes: Option<i64>) -> bool {
    let summary = crate::services::agent::merope::ingest::compact_summary(text);
    if summary.chars().count() < CHAT_DIARY_MIN_CHARS {
        return false;
    }
    last_chat_age_minutes.is_none_or(|age| age >= CHAT_DIARY_GAP_MINUTES)
}

async fn maybe_write_chat_diary(db: &sea_orm::DatabaseConnection, user_id: i32, text: &str) {
    let last_age = match latest_diary(db, user_id, store::DIARY_SOURCE_CHAT).await {
        Ok(Some(last)) => {
            Some((chrono::Utc::now() - last.created_at.with_timezone(&chrono::Utc)).num_minutes())
        }
        Ok(None) => None,
        Err(_) => return,
    };
    if !should_write_chat_diary(text, last_age) {
        return;
    }
    let summary = crate::services::agent::merope::ingest::compact_summary(text);
    let _ = insert_diary(db, user_id, &summary, store::DIARY_SOURCE_CHAT).await;
}

/// Public face: 人设 off → Agent (product). Empty 人设 name → Arael.
pub fn public_persona_name(is_enabled: bool, stored_name: Option<&str>) -> String {
    if !is_enabled {
        return "Agent".to_string();
    }
    stored_name
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or("Arael")
        .to_string()
}

pub use speaking_prompts::{
    addressee_speaking_section, format_activity_section, format_bits_section,
    format_brought_to_mind_section, format_curious_section, format_doing_section,
    format_emotion_section, format_found_out_section, format_group_days_section,
    format_inner_moment_ago_section, format_mood_section, format_on_your_mind_section,
    format_own_days_section, format_persona, format_playing_section, format_recent_section,
    format_remembered_section, format_self_story_section, format_since_section,
    format_taste_section, format_us_section, format_views_section, group_speaking_section,
    guest_speaking_section, mood_tone_instruction,
};

pub use speaking_context::{
    audience_for, her_openers, resolve_addressee_label, speaking_prompt, speaking_prompt_for_event,
    speaking_prompt_in_group, speaking_prompt_plain, speaking_prompt_to_reach,
    speaking_prompt_with_query, their_typing, with_said_unprompted,
};
#[cfg(test)]
pub use speaking_context::{format_addressee_label, merge_said_unprompted};

pub fn has_custom_persona(persona: &agent_persona::Model) -> bool {
    format_persona(persona).is_some()
}

#[cfg(test)]
pub use gates::{IngestSight, decide_ingest};
#[cfg(test)]
pub use state::Affect;
pub use state::{
    MUSIC_LISTENING_MIN_SECS, MoodTransition, apply_task_outcome, apply_user_utterance, clamp_mood,
    detect_mood_cue, effective_activity, is_extremely_low, mood_band,
};

/// The activity to act on, with a stale one read as idle.
pub fn current_activity(state: &crate::models::entities::agent_addressee_state::Model) -> &str {
    let age =
        (chrono::Utc::now() - state.activity_updated_at.with_timezone(&chrono::Utc)).num_seconds();
    effective_activity(&state.activity, age)
}

pub fn activity_is_busy(activity: &str) -> bool {
    matches!(activity, "thinking" | "talking" | "working")
}

pub fn parse_clock_minute(raw: &str) -> Option<i32> {
    let raw = raw.trim();
    let (hour, minute) = raw.split_once(':')?;
    let hour: i32 = hour.parse().ok()?;
    let minute: i32 = minute.parse().ok()?;
    if (0..24).contains(&hour) && (0..60).contains(&minute) {
        Some(hour * 60 + minute)
    } else {
        None
    }
}

pub fn format_clock_minute(minute: i32) -> Option<String> {
    if !(0..1440).contains(&minute) {
        return None;
    }
    Some(format!("{:02}:{:02}", minute / 60, minute % 60))
}

pub fn minute_in_window(now: i32, start: i32, end: i32) -> bool {
    if start == end {
        return false;
    }
    if start < end {
        now >= start && now < end
    } else {
        now >= start || now < end
    }
}

pub fn effective_do_not_disturb(
    state: &crate::models::entities::agent_addressee_state::Model,
) -> bool {
    if state.do_not_disturb {
        return true;
    }
    match (state.dnd_start_minute, state.dnd_end_minute) {
        (Some(start), Some(end)) if (0..1440).contains(&start) && (0..1440).contains(&end) => {
            use chrono::Timelike;
            let now = chrono::Local::now();
            let minute = (now.hour() * 60 + now.minute()) as i32;
            minute_in_window(minute, start, end)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{activity_is_busy, minute_in_window, parse_clock_minute, should_write_chat_diary};

    #[test]
    fn every_live_agent_activity_blocks_proactive_speech() {
        assert!(activity_is_busy("thinking"));
        assert!(activity_is_busy("talking"));
        assert!(activity_is_busy("working"));
        assert!(!activity_is_busy("idle"));
    }

    #[test]
    fn chat_diary_skips_short_and_recent_turns() {
        assert!(!should_write_chat_diary("嗯", None));
        assert!(should_write_chat_diary("今天晚上想打会独立游戏", None));
        assert!(!should_write_chat_diary("今天晚上想打会独立游戏", Some(5)));
        assert!(should_write_chat_diary("今天晚上想打会独立游戏", Some(20)));
    }

    #[test]
    fn addressee_label_prefers_display_name() {
        assert_eq!(
            super::format_addressee_label(7, Some("  瞳  "), Some("hitomi")),
            "瞳"
        );
        assert_eq!(
            super::format_addressee_label(7, Some("   "), Some("hitomi")),
            "hitomi"
        );
        assert_eq!(super::format_addressee_label(7, None, None), "User#7");
        assert_eq!(
            super::format_addressee_label(-12, Some("瞳"), None),
            "Guest"
        );
        assert_eq!(super::public_persona_name(false, Some("瞳")), "Agent");
        assert_eq!(super::public_persona_name(true, Some("  瞳  ")), "瞳");
        assert_eq!(super::public_persona_name(true, Some("   ")), "Arael");
        assert_eq!(super::public_persona_name(true, None), "Arael");
        assert!(!super::is_logged_in_addressee(0));
        assert!(!super::is_logged_in_addressee(-1));
        assert!(super::is_logged_in_addressee(1));
    }

    #[test]
    fn dnd_window_covers_same_day_and_overnight() {
        assert_eq!(parse_clock_minute("22:30"), Some(22 * 60 + 30));
        assert!(minute_in_window(23 * 60, 22 * 60, 7 * 60));
        assert!(minute_in_window(6 * 60, 22 * 60, 7 * 60));
        assert!(!minute_in_window(12 * 60, 22 * 60, 7 * 60));
        assert!(minute_in_window(13 * 60, 12 * 60, 14 * 60));
        assert!(!minute_in_window(14 * 60, 12 * 60, 14 * 60));
        assert!(!minute_in_window(12 * 60, 12 * 60, 12 * 60));
    }

    #[test]
    fn empty_persona_is_not_custom() {
        let blank = crate::models::entities::agent_persona::Model {
            id: "site".into(),
            name: "  ".into(),
            personality: String::new(),
            persona_json: None,
            visual_profile: None,
            portrait_asset_id: None,
            portrait_generation: None,
            avatar_asset_id: None,
            avatar_generation: None,
            updated_by: None,
            updated_at: chrono::Utc::now().into(),
        };
        assert!(super::format_persona(&blank).is_none());
        assert!(!super::has_custom_persona(&blank));
        let named = crate::models::entities::agent_persona::Model {
            name: "瞳".into(),
            ..blank.clone()
        };
        let named_text = super::format_persona(&named).unwrap();
        assert!(named_text.starts_with("You are 瞳."));
        assert!(named_text.contains(super::speaking_prompts::PERSONA_SPEAKING_CONTRACT));
        assert!(super::has_custom_persona(&named));
    }

    #[test]
    fn persona_fields_trim_whitespace() {
        let (name, personality) =
            crate::services::agent::merope::store::normalize_persona_fields("  瞳  ", "  认真  ");
        assert_eq!(name, "瞳");
        assert_eq!(personality, "认真");
        let (empty, _) =
            crate::services::agent::merope::store::normalize_persona_fields(" \t ", "");
        assert!(empty.is_empty());
    }

    #[test]
    fn mood_tone_stays_quiet_about_the_number() {
        assert!(
            super::speaking_prompts::PERSONA_SPEAKING_CONTRACT.contains("Do not name the mood")
        );
        assert!(super::mood_tone_instruction(8.0, 48.0).contains("very low"));
        assert!(super::mood_tone_instruction(30.0, 40.0).contains("a bit low"));
        assert!(super::mood_tone_instruction(30.0, 70.0).contains("on edge"));
        assert!(super::mood_tone_instruction(90.0, 48.0).contains("at ease"));
        assert!(super::mood_tone_instruction(90.0, 70.0).contains("bright"));
        let section = super::format_mood_section(72.4, 48.0);
        assert!(!section.contains("72/100"));
        assert!(!section.contains("72.4"));
    }

    #[test]
    fn diary_section_skips_empty_and_compacts() {
        assert!(super::format_recent_section(&[]).is_none());
        let block = super::format_remembered_section(&["今天晚上想打独立游戏".into()]).unwrap();
        assert!(block.contains("## About this person"));
        assert!(block.contains("Facts you kept"));
        assert!(block.contains("- 今天晚上想打独立游戏"));
        assert!(
            super::format_recent_section(&["Steam 解锁了成就".into()])
                .unwrap()
                .contains("## Recently")
        );
    }

    #[test]
    fn speaking_recent_omits_event_diary_and_keeps_remembered() {
        assert_eq!(
            super::speaking_context::RECENT_SPEAKING_DIARY_SOURCES,
            &[super::store::DIARY_SOURCE_CHAT]
        );
        assert!(
            !super::speaking_context::RECENT_SPEAKING_DIARY_SOURCES
                .contains(&super::store::DIARY_SOURCE_EVENT)
        );
        let remembered = super::format_remembered_section(&["晚上想打独立游戏".into()]).unwrap();
        let event_line = "正在收尾一篇文章，还差最后一段";
        let prompt = super::speaking_prompt_plain(&[remembered]);
        assert!(prompt.contains("## About this person"));
        assert!(prompt.contains("晚上想打独立游戏"));
        assert!(!prompt.contains(event_line));
        // Every section of the speaking prompt is built there.
        let recent_src = include_str!("speaking_context/sections.rs");
        assert!(recent_src.contains("RECENT_SPEAKING_DIARY_SOURCES"));
        assert!(recent_src.contains("format_remembered_section"));
        assert!(!recent_src.contains("DIARY_SOURCE_EVENT"));
    }

    /// One diary table is safe only while every read names its source.
    ///
    /// `remember` holds facts the user stated; `event` and `chat` hold
    /// summaries the platform wrote about them. An unscoped "latest row" would
    /// let one arrive where the other is expected, which is the only way the
    /// shared table could actually hurt — so the query cannot express it.
    #[test]
    fn every_diary_read_names_its_source() {
        let store = include_str!("store/diary.rs");
        assert!(
            !store.contains("source: Option<&str>"),
            "latest_diary accepts an unscoped read again"
        );
        for signature in [
            "pub async fn latest_diary(",
            "pub async fn list_diary_from_sources(",
        ] {
            let body = store
                .split(signature)
                .nth(1)
                .unwrap_or_else(|| panic!("{signature} is gone"));
            assert!(
                body.contains("Column::Source"),
                "{signature} no longer filters by source"
            );
        }
    }

    #[test]
    fn what_she_said_on_her_own_sits_in_the_history_by_time() {
        use crate::services::agent::ConversationMessage;
        let at = |minute: u32| {
            chrono::DateTime::parse_from_rfc3339(&format!("2026-09-25T10:{minute:02}:00Z"))
                .unwrap()
                .with_timezone(&chrono::Utc)
        };
        let message = |role: &str, content: &str, minute: u32| ConversationMessage {
            role: role.into(),
            content: content.into(),
            created_at: Some(at(minute).to_rfc3339()),
        };
        let history = vec![
            message("user", "早", 0),
            message("assistant", "早啊", 1),
            message("user", "我去忙了", 2),
        ];
        let said = [
            (at(20), "周报理好了，放资料库了".to_string()),
            (at(1), "早啊".to_string()),
        ];
        let merged = super::merge_said_unprompted(&history, &said);
        let lines: Vec<(&str, &str)> = merged
            .iter()
            .map(|message| (message.role.as_str(), message.content.as_str()))
            .collect();
        assert_eq!(
            lines,
            vec![
                ("user", "早"),
                ("assistant", "早啊"),
                ("user", "我去忙了"),
                ("assistant", "周报理好了，放资料库了"),
            ],
            "by time, and never twice"
        );
        let untimed = vec![ConversationMessage {
            role: "user".into(),
            content: "在吗".into(),
            created_at: None,
        }];
        let merged = super::merge_said_unprompted(&untimed, &said[..1]);
        assert_eq!(merged.last().unwrap().content, "周报理好了，放资料库了");
    }

    #[test]
    fn guest_and_addressee_sections_name_the_other_person() {
        assert!(super::guest_speaking_section().contains("guest"));
        assert!(super::addressee_speaking_section("瞳").contains("speaking to 瞳"));
        assert_eq!(
            super::speaking_prompt_plain(&[
                super::addressee_speaking_section("瞳"),
                super::format_mood_section(70.0, 48.0)
            ]),
            format!(
                "{}\n\n{}",
                super::addressee_speaking_section("瞳"),
                super::format_mood_section(70.0, 48.0)
            )
        );
    }
}
