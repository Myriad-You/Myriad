//! Agent Life: site persona, per-addressee state, hidden proactive speech.

pub mod gates;
pub mod ingest;
pub mod onboarding_ai;
pub mod onboarding_prompts;
pub mod report_dna;
pub mod speaking_prompts;
pub mod state;
pub mod store;

pub use ingest::{
    allow_existing_notify, life_enabled, spawn as spawn_ingest, spawn_diary, spawn_presence,
};
pub use store::{
    clear_persona, get_or_create_state, get_persona, insert_diary, insert_proactive, latest_diary,
    list_diary, normalize_persona_fields, recent_proactive, save_departure_mood, save_mood,
    set_activity, set_do_not_disturb, upsert_persona, PortraitUpdate,
};

/// Logged-in users only. Guests use negative ids; heartbeat is `SYSTEM_USER_ID` (0).
pub fn is_logged_in_addressee(user_id: i32) -> bool {
    user_id > 0
}

pub async fn mark_activity(db: &sea_orm::DatabaseConnection, user_id: i32, activity: &str) {
    if !is_logged_in_addressee(user_id) {
        return;
    }
    if !life_enabled().await {
        return;
    }
    let _ = set_activity(db, user_id, activity).await;
}

use crate::models::entities::agent_persona;

/// Soul text for user-facing speech: site persona when the flag is on, else SOUL.md.
pub async fn resolve_speaking_soul() -> Option<String> {
    let enabled = crate::GLOBAL_DYNAMIC_CONFIG
        .read()
        .await
        .agent_life_enabled_resolved();
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
/// Life + Lite → Lite. Otherwise Standard so chat still answers.
pub async fn create_speaking_analyzer() -> Option<crate::services::analyzer::AiAnalyzer> {
    if life_enabled().await {
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
        .agent_life_enabled_resolved()
    {
        return None;
    }
    let state = get_or_create_state(db, user_id).await.ok()?;
    refuse_new_task_message(Some(state.mood))
}

/// Returns the mood before this utterance, if life applied.
pub async fn note_user_turn(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    text: &str,
    utterance_index: u32,
) -> Option<f64> {
    if !is_logged_in_addressee(user_id) {
        return None;
    }
    if !crate::GLOBAL_DYNAMIC_CONFIG
        .read()
        .await
        .agent_life_enabled_resolved()
    {
        return None;
    }
    let state = get_or_create_state(db, user_id).await.ok()?;
    let previous = state.mood;
    let gap_hours = state
        .last_user_message_at
        .map(|at| (chrono::Utc::now() - at.with_timezone(&chrono::Utc)).num_minutes() as f64 / 60.0)
        .unwrap_or(24.0);
    let first_today = state.last_user_message_at.is_none_or(|at| {
        at.with_timezone(&chrono::Utc).date_naive() != chrono::Utc::now().date_naive()
    });
    let (praised, scolded) = detect_mood_cue(text);
    let next = apply_user_utterance(
        state.mood,
        utterance_index,
        praised,
        scolded,
        first_today,
        gap_hours,
    );
    let _ = save_mood(db, user_id, next, true).await;
    if !praised && !scolded && text.chars().count() >= CHAT_DIARY_MIN_CHARS {
        spawn_mood_hint(user_id, text);
    }
    if !is_extremely_low(state.mood) && is_extremely_low(next) {
        spawn_ingest(user_id, "agent.life.mood_floor", "跟这个人的心情掉到了极低");
    }
    Some(previous)
}

/// After planning, so this turn is not already sitting in the diary the model just read.
pub async fn note_chat_diary(db: &sea_orm::DatabaseConnection, user_id: i32, text: &str) {
    if !is_logged_in_addressee(user_id) {
        return;
    }
    if !life_enabled().await {
        return;
    }
    maybe_write_chat_diary(db, user_id, text).await;
}

pub async fn maybe_apply_departure(db: &sea_orm::DatabaseConnection, user_id: i32) {
    if !is_logged_in_addressee(user_id) {
        return;
    }
    if !life_enabled().await {
        return;
    }
    let Ok(state) = get_or_create_state(db, user_id).await else {
        return;
    };
    let Some(last) = state.last_user_message_at else {
        return;
    };
    let now = chrono::Utc::now();
    let last = last.with_timezone(&chrono::Utc);
    let silent = (now - last).num_seconds();
    let since_departure = state
        .last_departure_at
        .map(|at| (at.with_timezone(&chrono::Utc) - last).num_seconds());
    if !should_apply_departure(Some(silent), since_departure) {
        return;
    }
    let next = apply_departure(state.mood);
    let _ = save_departure_mood(db, user_id, next).await;
}

fn spawn_mood_hint(user_id: i32, text: impl Into<String>) {
    let text = text.into();
    tokio::spawn(async move {
        if !is_logged_in_addressee(user_id) || !life_enabled().await {
            return;
        }
        let Some(analyzer) =
            crate::services::ai::create_ai_analyzer_for_tier(crate::config::ModelTier::Lite).await
        else {
            return;
        };
        let Ok(raw) = analyzer
            .analyze_with_system(
                "只输出一个 -2 到 2 的整数，表示这句话对心情的微调。不要解释，不要输出别的字。",
                &text,
            )
            .await
        else {
            return;
        };
        let Some(hint) = parse_mood_hint(&raw) else {
            return;
        };
        if hint == 0.0 {
            return;
        }
        let Ok(db) = crate::services::tapp_registry::database().await else {
            return;
        };
        let Ok(state) = get_or_create_state(&db, user_id).await else {
            return;
        };
        let next = apply_mood_hint(state.mood, hint);
        let _ = save_mood(&db, user_id, next, false).await;
    });
}

const CHAT_DIARY_MIN_CHARS: usize = 8;
const CHAT_DIARY_GAP_MINUTES: i64 = 20;

pub fn should_write_chat_diary(text: &str, last_chat_age_minutes: Option<i64>) -> bool {
    let summary = crate::services::agent::life::ingest::compact_summary(text);
    if summary.chars().count() < CHAT_DIARY_MIN_CHARS {
        return false;
    }
    last_chat_age_minutes.is_none_or(|age| age >= CHAT_DIARY_GAP_MINUTES)
}

async fn maybe_write_chat_diary(db: &sea_orm::DatabaseConnection, user_id: i32, text: &str) {
    let last_age = match latest_diary(db, user_id, Some("chat")).await {
        Ok(Some(last)) => {
            Some((chrono::Utc::now() - last.created_at.with_timezone(&chrono::Utc)).num_minutes())
        }
        Ok(None) => None,
        Err(_) => return,
    };
    if !should_write_chat_diary(text, last_age) {
        return;
    }
    let summary = crate::services::agent::life::ingest::compact_summary(text);
    let _ = insert_diary(db, user_id, &summary, "chat").await;
}

/// Public face of the site persona. Life off or empty name → Arael.
pub fn public_persona_name(life_enabled: bool, stored_name: Option<&str>) -> String {
    if !life_enabled {
        return "Arael".to_string();
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
    addressee_speaking_section, format_activity_section, format_diary_section, format_mood_section,
    format_persona, guest_speaking_section, mood_tone_instruction,
};

/// Prompt sections for whoever this turn is speaking to. Empty when life is off.
pub async fn speaking_prompt(user_id: i32) -> Vec<String> {
    if !life_enabled().await {
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
    speaking_prompt_from_db(&db, user_id).await
}

async fn speaking_prompt_from_db(db: &sea_orm::DatabaseConnection, user_id: i32) -> Vec<String> {
    let addressee = resolve_addressee_label(db, user_id).await;
    let mut sections = vec![addressee_speaking_section(&addressee)];
    let Ok(state) = get_or_create_state(db, user_id).await else {
        return sections;
    };
    if let Ok(notes) = list_diary(db, user_id, 8).await {
        let contents: Vec<String> = notes
            .into_iter()
            .map(|note| ingest::compact_summary(&note.content))
            .filter(|content| !content.is_empty())
            .collect();
        if let Some(block) = format_diary_section(&contents) {
            sections.push(block);
        }
    }
    if let Some(block) = format_activity_section(current_activity(&state)) {
        sections.push(block);
    }
    sections.push(format_mood_section(state.mood));
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
    apply_departure, apply_mood_hint, apply_task_outcome, apply_user_utterance, clamp_mood,
    detect_mood_cue, effective_activity, is_extremely_low, parse_mood_hint, should_apply_departure,
    ACTIVITY_STALE_SECS, MOOD_FLOOR,
};

/// The activity to act on, with a stale one read as idle.
pub fn current_activity(state: &crate::models::entities::agent_addressee_state::Model) -> &str {
    let age = (chrono::Utc::now() - state.updated_at.with_timezone(&chrono::Utc)).num_seconds();
    effective_activity(&state.activity, age)
}

#[cfg(test)]
mod tests {
    use super::should_write_chat_diary;

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
        assert_eq!(super::public_persona_name(false, Some("瞳")), "Arael");
        assert_eq!(super::public_persona_name(true, Some("  瞳  ")), "瞳");
        assert_eq!(super::public_persona_name(true, Some("   ")), "Arael");
        assert_eq!(super::public_persona_name(true, None), "Arael");
        assert!(!super::is_logged_in_addressee(0));
        assert!(!super::is_logged_in_addressee(-1));
        assert!(super::is_logged_in_addressee(1));
    }

    #[test]
    fn empty_persona_is_not_custom() {
        let blank = crate::models::entities::agent_persona::Model {
            id: "site".into(),
            name: "  ".into(),
            personality: String::new(),
            portrait_asset_id: None,
            updated_by: None,
            updated_at: chrono::Utc::now().into(),
        };
        assert!(super::format_persona(&blank).is_none());
        assert!(!super::has_custom_persona(&blank));
        let named = crate::models::entities::agent_persona::Model {
            name: "瞳".into(),
            ..blank.clone()
        };
        assert_eq!(super::format_persona(&named).as_deref(), Some("你是瞳。"));
        assert!(super::has_custom_persona(&named));
    }

    #[test]
    fn persona_fields_trim_whitespace() {
        let (name, personality) =
            crate::services::agent::life::store::normalize_persona_fields("  瞳  ", "  认真  ");
        assert_eq!(name, "瞳");
        assert_eq!(personality, "认真");
        let (empty, _) = crate::services::agent::life::store::normalize_persona_fields(" \t ", "");
        assert!(empty.is_empty());
    }

    #[test]
    fn mood_tone_stays_quiet_about_the_number() {
        assert!(super::refuse_new_task_message(Some(10.0)).is_some());
        assert!(super::refuse_new_task_message(Some(10.1)).is_none());
        assert!(super::refuse_new_task_message(None).is_none());
        assert!(super::mood_tone_instruction(70.0).contains("不要念出心情数字"));
        assert!(super::mood_tone_instruction(8.0).contains("极低"));
        assert!(super::mood_tone_instruction(30.0).contains("偏低"));
        assert!(super::mood_tone_instruction(90.0).contains("轻松"));
        let section = super::format_mood_section(72.4);
        assert!(!section.contains("72/100"));
        assert!(section.contains("不要念出心情数字"));
    }

    #[test]
    fn diary_section_skips_empty_and_compacts() {
        assert!(super::format_diary_section(&[]).is_none());
        let block = super::format_diary_section(&["今天晚上想打会独立游戏".into()]).unwrap();
        assert!(block.contains("## 关于这个人的日记"));
        assert!(block.contains("不要当众报流水账"));
        assert!(block.contains("- 今天晚上想打会独立游戏"));
    }

    #[test]
    fn guest_and_addressee_sections_name_the_other_person() {
        assert!(super::guest_speaking_section().contains("游客"));
        assert!(super::addressee_speaking_section("瞳").contains("对瞳说话"));
        assert_eq!(
            super::speaking_prompt_plain(&[
                super::addressee_speaking_section("瞳"),
                super::format_mood_section(70.0)
            ]),
            format!(
                "{}\n\n{}",
                super::addressee_speaking_section("瞳"),
                super::format_mood_section(70.0)
            )
        );
    }
}
