//! Merope: site persona, per-addressee state, hidden proactive speech.

pub mod chat_remember;
pub mod gates;
pub mod ingest;
pub mod motion;
pub mod motion_local;
pub mod onboarding_ai;
pub mod onboarding_prompts;
pub mod report_dna;
pub mod speaking_prompts;
pub mod state;
pub mod store;

pub use chat_remember::spawn_chat_remember;
pub use ingest::{
    allow_existing_notify, is_enabled, spawn as spawn_ingest, spawn_diary, spawn_presence,
    tick_speak_intents,
};
pub use motion::{
    direct_motion, local_directive, refine_motion, resolve_round_motion_style, MotionContext,
    MotionPhase, PerformanceDirective,
};
pub use myriad_merope::RigStateSummary;
pub use store::{
    acquire_portrait_generation, clear_persona_on, complete_portrait_generation,
    credit_music_listening, generation_inputs_changed, get_or_create_state, get_persona,
    get_persona_on, insert_diary, insert_proactive, latest_diary, list_diary_from_sources,
    list_remembered, normalize_persona_fields, portrait_generation_is_pending, promote_activity,
    recent_proactive, release_portrait_generation, set_activity, set_dnd_schedule,
    set_do_not_disturb, update_affect, upsert_persona_on, JsonDocumentUpdate,
    PersonaContractUpdate, PortraitUpdate,
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
        if let Ok(db) = crate::services::tapp_registry::database().await {
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

/// Analyzer for lines spoken in character.
/// Merope + Lite → Lite. Otherwise Standard so chat still answers.
pub async fn create_speaking_analyzer() -> Option<crate::services::analyzer::AiAnalyzer> {
    if is_enabled().await {
        if let Some(analyzer) =
            crate::services::ai::create_ai_analyzer_for_tier(crate::config::ModelTier::Lite).await
        {
            return Some(analyzer);
        }
    }
    crate::services::ai::create_ai_analyzer_for_tier(crate::config::ModelTier::Standard).await
}

pub fn refuse_new_task_message(mood_before: Option<f64>) -> Option<String> {
    if mood_before.is_some_and(is_extremely_low) {
        Some("我现在心情很低，不想接新的事情。我们先说说话吧。".to_string())
    } else {
        None
    }
}

pub async fn maybe_refuse_new_task(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
) -> Option<String> {
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
    let state = get_or_create_state(db, user_id).await.ok()?;
    refuse_new_task_message(Some(state.mood))
}

/// Returns the persisted mood transition for this utterance, if Merope applied.
pub async fn note_user_turn(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    text: &str,
    utterance_index: u32,
) -> Option<MoodTransition> {
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
    if !praised && !scolded && text.chars().count() >= CHAT_DIARY_MIN_CHARS {
        spawn_mood_hint(user_id, text);
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
    Some(MoodTransition::from_affect(
        &previous,
        &store::affect_from_state(&saved),
        cause,
        saved
            .updated_at
            .with_timezone(&chrono::Utc)
            .timestamp_millis(),
    ))
}

/// After planning, so this turn is not already sitting in the diary the model just read.
pub async fn note_chat_diary(db: &sea_orm::DatabaseConnection, user_id: i32, text: &str) {
    if !is_logged_in_addressee(user_id) {
        return;
    }
    if !is_enabled().await {
        return;
    }
    maybe_write_chat_diary(db, user_id, text).await;
}

fn spawn_mood_hint(user_id: i32, text: impl Into<String>) {
    let text = text.into();
    tokio::spawn(async move {
        if !is_logged_in_addressee(user_id) || !is_enabled().await {
            return;
        }
        let Some(analyzer) =
            crate::services::ai::create_ai_analyzer_for_tier(crate::config::ModelTier::Lite).await
        else {
            return;
        };
        let Ok(raw) = crate::services::ai_cost_ledger::with_site_ai_ledger(
            user_id,
            "merope",
            "mood_hint",
            analyzer.analyze_with_system(
                "只输出两个 -2 到 2 的整数，空格分隔：效价 唤醒。不要解释，不要输出别的字。",
                &text,
            ),
        )
        .await
        else {
            return;
        };
        let Some((valence, arousal)) = parse_appraisal_hint(&raw) else {
            return;
        };
        if valence == 0 && arousal == 0 {
            return;
        }
        let Ok(db) = crate::services::tapp_registry::database().await else {
            return;
        };
        let _ = update_affect(&db, user_id, false, |affect| {
            apply_mood_hint(affect, valence, arousal);
        })
        .await;
    });
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

pub fn format_addressee_label(
    user_id: i32,
    display_name: Option<&str>,
    username: Option<&str>,
) -> String {
    if !is_logged_in_addressee(user_id) {
        return "游客".to_string();
    }
    display_name
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .or_else(|| username.map(str::trim).filter(|name| !name.is_empty()))
        .map(str::to_string)
        .unwrap_or_else(|| format!("用户#{user_id}"))
}

pub async fn resolve_addressee_label(db: &sea_orm::DatabaseConnection, user_id: i32) -> String {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, Value as SeaValue};

    let Ok(Some(row)) = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT display_name, username FROM users WHERE id = $1",
            vec![SeaValue::Int(Some(user_id))],
        ))
        .await
    else {
        return format_addressee_label(user_id, None, None);
    };
    let display_name = row
        .try_get::<Option<String>>("", "display_name")
        .ok()
        .flatten();
    let username = row.try_get::<Option<String>>("", "username").ok().flatten();
    format_addressee_label(user_id, display_name.as_deref(), username.as_deref())
}

pub use speaking_prompts::{
    addressee_speaking_section, format_activity_section, format_mood_section, format_persona,
    format_recent_section, format_remembered_section, guest_speaking_section,
    mood_tone_instruction, rank_remembered,
};

/// Prompt sections for whoever this turn is speaking to. Empty when Merope is off.
pub async fn speaking_prompt(user_id: i32) -> Vec<String> {
    speaking_prompt_with_query(user_id, None).await
}

pub async fn speaking_prompt_with_query(user_id: i32, query: Option<&str>) -> Vec<String> {
    if !is_enabled().await {
        return Vec::new();
    }
    if user_id < 0 {
        return vec![guest_speaking_section()];
    }
    if !is_logged_in_addressee(user_id) {
        return Vec::new();
    }
    let Ok(db) = crate::services::tapp_registry::database().await else {
        return vec![addressee_speaking_section(&format_addressee_label(
            user_id, None, None,
        ))];
    };
    speaking_prompt_from_db(&db, user_id, query).await
}

const REMEMBERED_PROMPT_LIMIT: usize = 8;
const REMEMBERED_CANDIDATE_LIMIT: u64 = 32;
const RECENT_LEDGER_LIMIT: u64 = 4;
/// Chat diary only. Event diary reaches speaking via Remember, not this ledger.
const RECENT_SPEAKING_DIARY_SOURCES: &[&str] = &[store::DIARY_SOURCE_CHAT];

async fn speaking_prompt_from_db(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    query: Option<&str>,
) -> Vec<String> {
    let addressee = resolve_addressee_label(db, user_id).await;
    let mut sections = vec![addressee_speaking_section(&addressee)];
    let Ok(state) = get_or_create_state(db, user_id).await else {
        return sections;
    };
    if let Ok(notes) = list_remembered(db, user_id, REMEMBERED_CANDIDATE_LIMIT).await {
        let facts: Vec<String> = notes
            .into_iter()
            .map(|note| ingest::compact_summary(&note.content))
            .filter(|content| !content.is_empty())
            .collect();
        let ranked = rank_remembered(&facts, query, REMEMBERED_PROMPT_LIMIT);
        if let Some(block) = format_remembered_section(&ranked) {
            sections.push(block);
        }
    }
    if let Ok(notes) = list_diary_from_sources(
        db,
        user_id,
        RECENT_SPEAKING_DIARY_SOURCES,
        RECENT_LEDGER_LIMIT,
    )
    .await
    {
        let contents: Vec<String> = notes
            .into_iter()
            .map(|note| ingest::compact_summary(&note.content))
            .filter(|content| !content.is_empty())
            .collect();
        if let Some(block) = format_recent_section(&contents) {
            sections.push(block);
        }
    }
    if let Some(block) = format_activity_section(current_activity(&state)) {
        sections.push(block);
    }
    sections.push(format_mood_section(state.mood, state.arousal));
    sections
}

pub fn speaking_prompt_plain(sections: &[String]) -> String {
    sections.join("\n\n")
}

pub fn has_custom_persona(persona: &agent_persona::Model) -> bool {
    format_persona(persona).is_some()
}

pub use gates::{decide_ingest, is_chatting, is_valuable_event, IngestDecision};
pub use state::{
    apply_mood_hint, apply_task_outcome, apply_user_utterance, clamp_mood, detect_mood_cue,
    effective_activity, is_extremely_low, mood_band, parse_appraisal_hint, Affect, AffectBaseline,
    MoodTransition, ACTIVITY_STALE_SECS, DEFAULT_AROUSAL, DEFAULT_MOOD, MOOD_FLOOR,
    MUSIC_LISTENING_MIN_SECS, ORIGIN,
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
        assert_eq!(super::format_addressee_label(7, None, None), "用户#7");
        assert_eq!(super::format_addressee_label(-12, Some("瞳"), None), "游客");
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
        assert!(named_text.starts_with("你是瞳。"));
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
        assert!(super::refuse_new_task_message(Some(10.0)).is_some());
        assert!(super::refuse_new_task_message(Some(10.1)).is_none());
        assert!(super::refuse_new_task_message(None).is_none());
        assert!(super::speaking_prompts::PERSONA_SPEAKING_CONTRACT.contains("不要念心情"));
        assert!(super::mood_tone_instruction(8.0, 48.0).contains("极低"));
        assert!(super::mood_tone_instruction(30.0, 40.0).contains("偏低"));
        assert!(super::mood_tone_instruction(30.0, 70.0).contains("烦躁"));
        assert!(super::mood_tone_instruction(90.0, 48.0).contains("平常语气"));
        assert!(super::mood_tone_instruction(90.0, 70.0).contains("轻松"));
        let section = super::format_mood_section(72.4, 48.0);
        assert!(!section.contains("72/100"));
        assert!(!section.contains("72.4"));
    }

    #[test]
    fn diary_section_skips_empty_and_compacts() {
        assert!(super::format_recent_section(&[]).is_none());
        let block = super::format_remembered_section(&["今天晚上想打独立游戏".into()]).unwrap();
        assert!(block.contains("## 关于这个人"));
        assert!(block.contains("你留下的事实"));
        assert!(block.contains("- 今天晚上想打独立游戏"));
        assert!(super::format_recent_section(&["Steam 解锁了成就".into()])
            .unwrap()
            .contains("## 最近"));
    }

    #[test]
    fn speaking_recent_omits_event_diary_and_keeps_remembered() {
        assert_eq!(
            super::RECENT_SPEAKING_DIARY_SOURCES,
            &[super::store::DIARY_SOURCE_CHAT]
        );
        assert!(!super::RECENT_SPEAKING_DIARY_SOURCES.contains(&super::store::DIARY_SOURCE_EVENT));
        let remembered = super::format_remembered_section(&["晚上想打独立游戏".into()]).unwrap();
        let event_line = "正在收尾一篇文章，还差最后一段";
        let prompt = super::speaking_prompt_plain(&[remembered]);
        assert!(prompt.contains("## 关于这个人"));
        assert!(prompt.contains("晚上想打独立游戏"));
        assert!(!prompt.contains(event_line));
        let recent_src = include_str!("mod.rs")
            .split("async fn speaking_prompt_from_db")
            .nth(1)
            .and_then(|rest| rest.split("pub fn speaking_prompt_plain").next())
            .unwrap();
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
        let store = include_str!("store.rs");
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
    fn guest_and_addressee_sections_name_the_other_person() {
        assert!(super::guest_speaking_section().contains("游客"));
        assert!(super::addressee_speaking_section("瞳").contains("对瞳说话"));
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
