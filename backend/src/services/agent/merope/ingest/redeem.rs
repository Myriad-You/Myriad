//! Speak intent in → sentence out. Rechecks chatting / dnd / working here.

use chrono::Utc;
use sea_orm::DatabaseConnection;

use super::super::gates::{decide_ingest, is_valuable_event};
use super::super::store::{
    affect_from_state, get_or_create_state, get_persona, insert_proactive, latest_open_session,
    recent_proactive, recently_spoke_event, set_activity, touch_proactive,
};
use super::super::{
    activity_is_busy, addressee_speaking_section, current_activity, direct_motion,
    effective_do_not_disturb, format_mood_section, public_persona_name, resolve_addressee_label,
    resolve_round_motion_style, MoodTransition, MotionContext, MotionPhase, PerformanceDirective,
};
use super::{
    addressee_is_chatting, compact_summary, is_enabled, is_trivial_line, SAME_EVENT_MINUTES,
};
use crate::config::ModelTier;
use crate::services::agent::consciousness::{drain_speak_intents, last_live_presence, SpeakIntent};
use crate::services::agent::notifications::{
    get_notification_manager, Notification, NotificationPriority, NotificationType,
};
use crate::services::ai::create_ai_analyzer_for_tier;

const MEROPE_OWNED_NOTIFY: &[&str] = &[
    "agent.merope.platform_activity",
    "agent.merope.report_ready",
];

/// Re-check chatting / dnd / working at redeem time. Produce-time gates
/// are stale after the 15s autonomy loop.
pub fn may_redeem_speech(event_key: &str, dnd: bool, chatting: bool, working: bool) -> bool {
    decide_ingest(event_key, dnd, chatting, working).allow_model
}

pub async fn tick_speak_intents(db: DatabaseConnection) {
    if !is_enabled().await {
        return;
    }
    let now = Utc::now();
    for intent in drain_speak_intents(now) {
        if let Err(error) = redeem_speak_intent(&db, intent).await {
            tracing::warn!(%error, "[Merope] redeem speak intent failed");
        }
    }
}

async fn redeem_speak_intent(
    db: &DatabaseConnection,
    intent: SpeakIntent,
) -> Result<(), anyhow::Error> {
    if intent.expires_at <= Utc::now() {
        return Ok(());
    }
    let state = get_or_create_state(db, intent.user_id).await?;
    let chatting = addressee_is_chatting(db, intent.user_id).await;
    let working = activity_is_busy(current_activity(&state));
    let dnd = effective_do_not_disturb(&state);
    if !may_redeem_speech(&intent.topic, dnd, chatting, working) {
        return Ok(());
    }
    let decision = decide_ingest(&intent.topic, dnd, chatting, working);
    if recently_spoke_event(db, intent.user_id, &intent.topic, SAME_EVENT_MINUTES).await? {
        return Ok(());
    }

    let source_intent_id = intent.work_intent_id.as_deref();
    let shown = (source_intent_id.is_some() && is_valuable_event(&intent.topic))
        || speech_is_shown(&intent.topic, decision.notify);
    let spoken = if shown {
        let _ = set_activity(db, intent.user_id, "thinking").await;
        let line = compose_line(db, intent.user_id, &intent.gist).await;
        let _ = set_activity(db, intent.user_id, "idle").await;
        line
    } else {
        fallback_line(&intent.gist)
    };

    if is_trivial_line(&spoken) {
        return Ok(());
    }
    if let Ok(recent) = recent_proactive(db, intent.user_id, 1).await {
        if recent
            .first()
            .is_some_and(|last| last.content.trim() == spoken.trim())
        {
            return Ok(());
        }
    }

    // Direct motion only after the line has passed every suppression check. This
    // keeps the Lite budget tied to speech the addressee will actually receive.
    let (performance, motion_mood) = if shown {
        match get_or_create_state(db, intent.user_id).await {
            Ok(current) => {
                let affect = affect_from_state(&current);
                let mood = MoodTransition::from_affect(
                    &affect,
                    &affect,
                    &intent.topic,
                    current.updated_at.with_timezone(&Utc).timestamp_millis(),
                );
                let motion_style = resolve_round_motion_style(
                    None,
                    current.mood.round() as i32,
                    current.arousal.round() as i32,
                )
                .await;
                let performance = direct_motion(MotionContext {
                    user_id: intent.user_id,
                    phase: MotionPhase::Proactive,
                    mood: mood.clone(),
                    activity: "talking".to_string(),
                    user_text: intent.gist.clone(),
                    response_text: Some(spoken.clone()),
                    task_success: None,
                    rig_state: last_live_presence(intent.user_id).rig_state,
                    motion_style,
                })
                .await;
                (performance, Some(mood))
            }
            Err(_) => (None, None),
        }
    } else {
        (None, None)
    };

    insert_proactive(db, intent.user_id, &spoken, Some(&intent.topic), shown).await?;
    let _ = touch_proactive(db, intent.user_id).await;

    if shown {
        emit_speech_notification(
            db,
            intent.user_id,
            &intent.topic,
            &spoken,
            performance.as_ref(),
            motion_mood.as_ref(),
            source_intent_id,
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

async fn compose_line(db: &DatabaseConnection, user_id: i32, summary: &str) -> String {
    let fallback = fallback_line(summary);
    let Some(analyzer) = create_ai_analyzer_for_tier(ModelTier::Lite).await else {
        return fallback;
    };
    let soul = crate::services::agent::identity::get_speaking_soul()
        .await
        .unwrap_or_else(|| "你是 Agent。".to_string());
    let addressee = resolve_addressee_label(db, user_id).await;
    let mood_block = match get_or_create_state(db, user_id).await {
        Ok(state) => format!("\n\n{}", format_mood_section(state.mood, state.arousal)),
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
    let system = super::super::speaking_prompts::compose_proactive_system(
        &soul,
        &addressee_speaking_section(&addressee),
        &mood_block,
        &recent_block,
    );
    let prompt = super::super::speaking_prompts::compose_proactive_user(summary);
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
    performance: Option<&PerformanceDirective>,
    mood: Option<&MoodTransition>,
    source_intent_id: Option<&str>,
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
        "action": "open_agent",
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
        if let Some(intent_id) = source_intent_id {
            object.insert(
                "intention_id".to_string(),
                serde_json::Value::String(intent_id.to_string()),
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
    fn redeem_speaks_without_producing() {
        let src = include_str!("redeem.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        let tick = src
            .split("pub async fn tick_speak_intents")
            .nth(1)
            .and_then(|rest| rest.split("async fn redeem_speak_intent").next())
            .expect("tick path");
        let enabled_at = tick
            .find("is_enabled")
            .expect("tick gates on merope switch");
        let drain_at = tick
            .find("drain_speak_intents")
            .expect("tick drains after the gate");
        assert!(
            enabled_at < drain_at,
            "disabled tick must not drain speak intents"
        );
        let redeem = src
            .split("async fn redeem_speak_intent")
            .nth(1)
            .expect("redeem path");
        assert!(redeem.contains("if is_trivial_line(&spoken)"));
        assert!(redeem.contains("last.content.trim() == spoken.trim()"));
        assert!(redeem.contains("insert_proactive"));
        assert!(redeem.contains("emit_speech_notification"));
        assert!(redeem.contains("if shown {"));
        assert!(redeem.contains("direct_motion"));
        assert!(!redeem.contains("enqueue_speak_intent"));
        assert!(!src.contains("consider_event"));
    }

    #[test]
    fn chatting_at_redeem_does_not_compose_a_sentence() {
        assert!(!may_redeem_speech(
            "agent.merope.platform_activity",
            false,
            true,
            false
        ));
        assert!(may_redeem_speech(
            "agent.merope.platform_activity",
            false,
            false,
            false
        ));
    }

    #[test]
    fn only_merope_owned_speech_is_worth_a_model_call() {
        assert!(speech_is_shown("agent.merope.platform_activity", true));
        assert!(speech_is_shown("agent.merope.report_ready", true));
        // These already have a producer sending the notification.
        assert!(!speech_is_shown("agent.task_failed", true));
        assert!(!speech_is_shown("brew.source_error", true));
        // Ambient speech never notifies at all.
        assert!(!speech_is_shown("agent.merope.greeting", false));
    }
}
