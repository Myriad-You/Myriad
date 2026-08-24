//! Named-event speech: gate → line → hidden proactive → maybe notify.
//!
//! The line is written by Lite only when it will be shown. Everything else keeps
//! the event summary verbatim: the transcript's other reader is this module
//! itself, checking that it does not repeat what it already said.

use chrono::Utc;
use sea_orm::DatabaseConnection;

use super::is_logged_in_addressee;
use crate::config::ModelTier;
use crate::services::agent::notifications::{
    get_notification_manager, Notification, NotificationPriority, NotificationType,
};
use crate::services::agent::run_hub;
use crate::services::ai::create_ai_analyzer_for_tier;

use super::gates::{decide_ingest, is_chatting, is_valuable_event};
use super::store::{
    get_or_create_state, get_persona, insert_diary, insert_proactive, latest_open_session,
    recent_proactive, recently_spoke_event, save_mood, set_activity, touch_proactive,
};
use super::{
    addressee_speaking_section, apply_task_outcome, format_mood_section, is_extremely_low,
    public_persona_name,
};

const SAME_EVENT_MINUTES: i64 = 15;
const MEROPE_OWNED_NOTIFY: &[&str] = &["agent.merope.platform_activity"];

pub async fn is_enabled() -> bool {
    crate::GLOBAL_DYNAMIC_CONFIG
        .read()
        .await
        .merope_enabled_resolved()
}

/// Existing producers keep notifying unless Merope is on and the addressee is mid-conversation.
pub async fn latest_session_id_for(user_id: i32) -> Option<String> {
    let db = crate::services::tapp_registry::database().await.ok()?;
    latest_open_session(&db, user_id)
        .await
        .ok()
        .flatten()
        .map(|(id, _)| id)
}

pub async fn allow_existing_notify(user_id: i32) -> bool {
    if !is_logged_in_addressee(user_id) {
        return true;
    }
    if !is_enabled().await {
        return true;
    }
    let Ok(db) = crate::services::tapp_registry::database().await else {
        return true;
    };
    // Only the live chat window suppresses these — the addressee is already
    // watching the panel. Do-not-disturb means "don't speak up on your own",
    // not "swallow the failures of work this person asked for", so it is
    // deliberately not consulted here; it gates speech in `decide_ingest`.
    !addressee_is_chatting(&db, user_id).await
}

pub fn spawn_diary(user_id: i32, summary: impl Into<String>) {
    let summary = compact_summary(&summary.into());
    if summary.is_empty() {
        return;
    }
    tokio::spawn(async move {
        if !is_logged_in_addressee(user_id) || !is_enabled().await {
            return;
        }
        let Ok(db) = crate::services::tapp_registry::database().await else {
            return;
        };
        if let Err(error) = insert_diary(&db, user_id, &summary, "event").await {
            tracing::debug!(%error, user_id, "[Merope] diary write failed");
        }
    });
}

pub fn spawn_presence(user_id: i32) {
    if !is_logged_in_addressee(user_id) {
        return;
    }
    tokio::spawn(async move {
        if !is_enabled().await {
            return;
        }
        let Ok(db) = crate::services::tapp_registry::database().await else {
            return;
        };
        crate::services::agent::merope::maybe_apply_departure(&db, user_id).await;
        let Ok(state) = get_or_create_state(&db, user_id).await else {
            return;
        };
        let gap_hours = state
            .last_user_message_at
            .map(|at| (Utc::now() - at.with_timezone(&Utc)).num_minutes() as f64 / 60.0)
            .unwrap_or(24.0);
        let first_today = state
            .last_user_message_at
            .is_none_or(|at| at.with_timezone(&Utc).date_naive() != Utc::now().date_naive());
        if !first_today && gap_hours < 12.0 {
            return;
        }
        if recently_spoke_event(&db, user_id, "agent.merope.greeting", 12 * 60)
            .await
            .unwrap_or(true)
        {
            return;
        }
        let summary = if first_today {
            "这个人今天第一次来了"
        } else {
            "这个人隔了很久又来了"
        };
        if let Err(error) = ingest(&db, user_id, "agent.merope.greeting", summary).await {
            tracing::debug!(%error, user_id, "[Merope] presence ingest failed");
        }
    });
}

pub fn spawn(user_id: i32, event_key: impl Into<String>, summary: impl Into<String>) {
    let event_key = event_key.into();
    let summary = summary.into();
    tokio::spawn(async move {
        let Ok(db) = crate::services::tapp_registry::database().await else {
            return;
        };
        if let Err(error) = ingest(&db, user_id, &event_key, &summary).await {
            tracing::warn!(
                %error,
                user_id,
                event_key,
                "[Merope] ingest failed"
            );
        }
    });
}

pub async fn ingest(
    db: &DatabaseConnection,
    user_id: i32,
    event_key: &str,
    summary: &str,
) -> Result<(), anyhow::Error> {
    if !is_logged_in_addressee(user_id) {
        return Ok(());
    }
    if !is_enabled().await {
        return Ok(());
    }
    super::maybe_apply_departure(db, user_id).await;
    let summary = compact_summary(summary);
    if summary.is_empty() {
        return Ok(());
    }

    let state = get_or_create_state(db, user_id).await?;
    let chatting = addressee_is_chatting(db, user_id).await;
    let working = super::current_activity(&state) == "working";
    let decision = decide_ingest(
        event_key,
        super::effective_do_not_disturb(&state),
        chatting,
        working,
    );
    apply_task_mood(db, user_id, event_key, state.mood).await;

    if !decision.allow_model {
        let _ = insert_diary(db, user_id, &summary, "event").await;
        return Ok(());
    }
    if recently_spoke_event(db, user_id, event_key, SAME_EVENT_MINUTES).await? {
        let _ = insert_diary(db, user_id, &summary, "event").await;
        return Ok(());
    }

    // Only a line the addressee will actually read is worth a model call. The rest
    // of the transcript is a ledger with no reader — it exists so the next line
    // does not repeat itself — so the human-readable summary stands in for it.
    let shown = speech_is_shown(event_key, decision.notify);
    let spoken = if shown {
        let _ = set_activity(db, user_id, "thinking").await;
        let line = compose_line(db, user_id, &summary).await;
        let _ = set_activity(db, user_id, "idle").await;
        line
    } else {
        fallback_line(&summary)
    };

    if is_trivial_line(&spoken) {
        let _ = insert_diary(db, user_id, &summary, "event").await;
        return Ok(());
    }
    if let Ok(recent) = recent_proactive(db, user_id, 1).await {
        if recent
            .first()
            .is_some_and(|last| last.content.trim() == spoken.trim())
        {
            let _ = insert_diary(db, user_id, &summary, "event").await;
            return Ok(());
        }
    }

    // Direct motion only after the line has passed every suppression check. This
    // keeps the Lite budget tied to speech the addressee will actually receive.
    let (performance, motion_mood) = if shown {
        match get_or_create_state(db, user_id).await {
            Ok(current) => {
                let band = super::mood_band(current.mood).to_string();
                let mood = super::MoodTransition {
                    before: current.mood,
                    after: current.mood,
                    band_before: band.clone(),
                    band_after: band,
                    delta: 0.0,
                    cause: event_key.to_string(),
                    revision: current.updated_at.with_timezone(&Utc).timestamp_millis(),
                };
                let performance = super::direct_motion(super::MotionContext {
                    user_id,
                    phase: super::MotionPhase::Proactive,
                    mood: mood.clone(),
                    activity: "talking".to_string(),
                    user_text: summary.clone(),
                    response_text: Some(spoken.clone()),
                    task_success: None,
                })
                .await;
                (performance, Some(mood))
            }
            Err(_) => (None, None),
        }
    } else {
        (None, None)
    };

    let _ = insert_diary(db, user_id, &summary, "event").await;
    insert_proactive(db, user_id, &spoken, Some(event_key), shown).await?;
    let _ = touch_proactive(db, user_id).await;

    if shown {
        emit_speech_notification(
            db,
            user_id,
            event_key,
            &spoken,
            performance.as_ref(),
            motion_mood.as_ref(),
        )
        .await;
    }
    Ok(())
}

/// Whether the composed line reaches the addressee at all. Valuable events whose
/// notification an existing producer already owns are excluded: sending our own
/// would mean two notifications for one thing.
fn speech_is_shown(event_key: &str, notify: bool) -> bool {
    notify && MEROPE_OWNED_NOTIFY.contains(&event_key)
}

pub fn fallback_line(summary: &str) -> String {
    match compact_summary(summary).as_str() {
        "这个人今天第一次来了" => "今天又见到你了。".to_string(),
        "这个人隔了很久又来了" => "好久不见。".to_string(),
        "跟这个人的心情掉到了极低" => "我在。".to_string(),
        _ => "刚才有件事，想跟你说一声。".to_string(),
    }
}

pub fn is_trivial_line(text: &str) -> bool {
    let trimmed = text.trim();
    trimmed.chars().count() < 2 || trimmed.starts_with('{')
}

pub fn compact_summary(summary: &str) -> String {
    redact_event_text(summary)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(240)
        .collect()
}

pub fn redact_event_text(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return String::new();
    }
    let mut skip_next = false;
    let mut out = Vec::new();
    for token in trimmed.split_whitespace() {
        if skip_next {
            skip_next = false;
            continue;
        }
        let lower = token.to_ascii_lowercase();
        if lower == "bearer" || lower.starts_with("bearer") {
            skip_next = lower == "bearer";
            continue;
        }
        if let Some(cleaned) = redact_token(token) {
            out.push(cleaned);
        }
    }
    out.join(" ")
}

fn redact_token(token: &str) -> Option<String> {
    let stripped = if let Some(scheme) = token.find("://") {
        let after_scheme = &token[scheme + 3..];
        if let Some(query) = after_scheme.find('?') {
            token[..scheme + 3 + query].to_string()
        } else {
            token.to_string()
        }
    } else {
        token.to_string()
    };
    let lower = stripped.to_ascii_lowercase();
    if lower.contains("api_key=")
        || lower.contains("access_token=")
        || lower.contains("refresh_token=")
        || lower.contains("secret=")
        || lower.contains("password=")
        || lower.starts_with("bearer")
    {
        return None;
    }
    Some(stripped)
}

async fn addressee_is_chatting(db: &DatabaseConnection, user_id: i32) -> bool {
    let last_active = latest_open_session(db, user_id)
        .await
        .ok()
        .flatten()
        .map(|(_, at)| at);
    // A run parked on `waiting_for_input` is the addressee *not* talking: counting
    // it as chatting would suppress the very clarification notice that asks them
    // to come back, so only actively executing runs hold the floor.
    let executing_run = run_hub::user_has_executing_run(user_id).await;
    is_chatting(last_active, executing_run, Utc::now())
}

async fn apply_task_mood(db: &DatabaseConnection, user_id: i32, event_key: &str, mood: f64) {
    let next = match event_key {
        "agent.task_completed" => apply_task_outcome(mood, true),
        "agent.task_failed" | "agent.task_cancelled" => apply_task_outcome(mood, false),
        _ => return,
    };
    let _ = save_mood(db, user_id, next, false).await;
    if !is_extremely_low(mood) && is_extremely_low(next) {
        spawn(user_id, "agent.merope.mood_floor", "跟这个人的心情掉到了极低");
    }
}

async fn compose_line(db: &DatabaseConnection, user_id: i32, summary: &str) -> String {
    let fallback = fallback_line(summary);
    let Some(analyzer) = create_ai_analyzer_for_tier(ModelTier::Lite).await else {
        return fallback;
    };
    let soul = crate::services::agent::identity::get_speaking_soul()
        .await
        .unwrap_or_else(|| "你是 Agent。".to_string());
    let addressee = super::resolve_addressee_label(db, user_id).await;
    let mood_block = match get_or_create_state(db, user_id).await {
        Ok(state) => format!("\n\n{}", format_mood_section(state.mood)),
        Err(_) => String::new(),
    };
    let recent = recent_proactive(db, user_id, 6)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|row| format!("- {}", compact_summary(&row.content)))
        .collect::<Vec<_>>()
        .join("\n");
    let recent_block = if recent.is_empty() {
        "（还没有对这个人说过话）".to_string()
    } else {
        recent
    };
    let system = super::speaking_prompts::compose_proactive_system(
        &soul,
        &addressee_speaking_section(&addressee),
        &mood_block,
        &recent_block,
    );
    let prompt = super::speaking_prompts::compose_proactive_user(summary);
    match crate::services::ai_cost_ledger::with_site_ai_ledger(
        user_id,
        "merope",
        "speak",
        analyzer.analyze_with_system(&system, &prompt),
    )
    .await
    {
        Ok(raw) => {
            let spoken = sanitize_speech(&raw);
            if is_trivial_line(&spoken) {
                fallback
            } else {
                spoken
            }
        }
        Err(error) => {
            tracing::debug!(%error, "[Merope] Lite speech failed, using fallback");
            fallback
        }
    }
}

fn sanitize_speech(raw: &str) -> String {
    let mut text = raw.trim().to_string();
    if text.starts_with("```") {
        text = text
            .lines()
            .skip(1)
            .take_while(|line| !line.trim_start().starts_with("```"))
            .collect::<Vec<_>>()
            .join(" ");
    }
    let text = text
        .trim()
        .trim_matches(|c| c == '"' || c == '“' || c == '”')
        .trim();
    let first = text
        .split_once('\n')
        .map(|(head, _)| head)
        .unwrap_or(text)
        .trim();
    first.chars().take(160).collect()
}

async fn emit_speech_notification(
    db: &DatabaseConnection,
    user_id: i32,
    event_key: &str,
    spoken: &str,
    performance: Option<&super::PerformanceDirective>,
    mood: Option<&super::MoodTransition>,
) {
    let Some(manager) = get_notification_manager() else {
        return;
    };
    if !is_valuable_event(event_key) {
        return;
    }
    let title = display_name(db).await;
    let session_id = latest_open_session(db, user_id)
        .await
        .ok()
        .flatten()
        .map(|(id, _)| id);
    let mut metadata = serde_json::json!({
        "event_key": event_key,
        "action": "open_arael",
        "session_id": session_id,
    });
    if let Some(object) = metadata.as_object_mut() {
        if let Some(performance) = performance {
            object.insert(
                "performance".to_string(),
                serde_json::to_value(performance).unwrap_or_default(),
            );
        }
        if let Some(mood) = mood {
            object.insert(
                "merope_state".to_string(),
                serde_json::json!({ "mood": mood, "activity": "talking" }),
            );
        }
    }
    let notification = Notification::new(
        user_id,
        NotificationType::SystemInfo,
        NotificationPriority::Normal,
        title,
        spoken,
    )
    .with_metadata(metadata);
    manager.notify(notification).await;
}

async fn display_name(db: &DatabaseConnection) -> String {
    let stored = get_persona(db)
        .await
        .ok()
        .flatten()
        .map(|persona| persona.name);
    public_persona_name(true, stored.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_keeps_human_summary() {
        assert_eq!(
            fallback_line("  Steam  解锁了成就  "),
            "刚才有件事，想跟你说一声。"
        );
        assert_eq!(fallback_line("这个人今天第一次来了"), "今天又见到你了。");
        assert_eq!(fallback_line("这个人隔了很久又来了"), "好久不见。");
    }

    #[test]
    fn compact_summary_drops_json_and_secret_shaped_tokens() {
        assert!(compact_summary("{\"token\":\"abc\"}").is_empty());
        assert_eq!(
            compact_summary("Steam 刷新失败 https://api.example/sync?access_token=abcd"),
            "Steam 刷新失败 https://api.example/sync"
        );
        assert_eq!(compact_summary("抓取失败 Bearer eyJhbGciOi"), "抓取失败");
        assert_eq!(compact_summary("  Steam  解锁了成就  "), "Steam 解锁了成就");
    }

    #[test]
    fn only_merope_owned_speech_is_worth_a_model_call() {
        assert!(speech_is_shown("agent.merope.platform_activity", true));
        // These already have a producer sending the notification.
        assert!(!speech_is_shown("agent.task_failed", true));
        assert!(!speech_is_shown("brew.source_error", true));
        // Ambient speech never notifies at all.
        assert!(!speech_is_shown("agent.merope.greeting", false));
    }

    #[test]
    fn json_shaped_speech_is_trivial() {
        assert!(is_trivial_line("{\"line\":\"hi\"}"));
        assert!(!is_trivial_line("刚才那件事做成了。"));
    }
}
