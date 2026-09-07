//! Speak intent in → sentence out. Rechecks sight here. Live and notify are
//! independent channels.

use chrono::Utc;
use sea_orm::DatabaseConnection;

use super::super::gates::{decide_ingest, is_valuable_event};
use super::super::store::{
    affect_from_state, get_or_create_state, get_persona, insert_proactive, latest_open_session,
    recent_proactive, recently_spoke_event, set_activity, touch_proactive,
};
use super::super::{
    addressee_speaking_section, direct_motion, format_mood_section, public_persona_name,
    resolve_addressee_label, resolve_round_motion_style, MoodTransition, MotionContext,
    MotionPhase, PerformanceDirective,
};
use super::{
    compact_summary, current_sight, is_enabled, is_trivial_line, log_skip, SAME_EVENT_MINUTES,
};
use crate::config::ModelTier;
use crate::services::agent::consciousness::{drain_speak_intents, last_live_presence, SpeakIntent};
use crate::services::agent::merope::gates::IngestSight;
use crate::services::agent::notifications::{
    get_notification_manager, LiveSpeech, Notification, NotificationPriority, NotificationType,
};
use crate::services::ai::create_ai_analyzer_for_tier;

fn merope_owns_notify(event_key: &str) -> bool {
    event_key.starts_with("agent.merope.")
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
    let sight = current_sight(intent.user_id, &state).await;
    let decision = decide_ingest(&intent.topic, &sight);
    if !decision.allow_model {
        log_skip(intent.user_id, &intent.topic, decision.reason);
        return Ok(());
    }
    if recently_spoke_event(db, intent.user_id, &intent.topic, SAME_EVENT_MINUTES).await? {
        log_skip(intent.user_id, &intent.topic, "recently_spoke");
        return Ok(());
    }

    let source_intent_id = intent.work_intent_id.as_deref();
    let shown = decision.live
        || (source_intent_id.is_some() && is_valuable_event(&intent.topic))
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
        log_skip(intent.user_id, &intent.topic, "trivial_line");
        return Ok(());
    }
    if let Ok(recent) = recent_proactive(db, intent.user_id, 1).await {
        if recent
            .first()
            .is_some_and(|last| last.content.trim() == spoken.trim())
        {
            log_skip(intent.user_id, &intent.topic, "duplicate_line");
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

    // Merope toasts only what it owns. Task / brew / sync already have a
    // producer; sending ours as well would be two notices for one event.
    let merope_notifies = speech_is_shown(&intent.topic, decision.notify);
    insert_proactive(
        db,
        intent.user_id,
        &spoken,
        Some(&intent.topic),
        merope_notifies,
    )
    .await?;
    let _ = touch_proactive(db, intent.user_id).await;

    if decision.live && shown {
        emit_live_speech(
            intent.user_id,
            &intent.id,
            &intent.topic,
            &spoken,
            performance.as_ref(),
            motion_mood.as_ref(),
            source_intent_id,
        );
    }
    if merope_notifies {
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
    notify && merope_owns_notify(event_key)
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

fn emit_live_speech(
    user_id: i32,
    id: &str,
    event_key: &str,
    spoken: &str,
    performance: Option<&PerformanceDirective>,
    mood: Option<&MoodTransition>,
    source_intent_id: Option<&str>,
) {
    let Some(manager) = get_notification_manager() else {
        return;
    };
    manager.emit_live_speech(
        user_id,
        LiveSpeech {
            id: id.to_string(),
            body: spoken.to_string(),
            event_key: event_key.to_string(),
            performance: performance.and_then(|value| serde_json::to_value(value).ok()),
            merope_state: mood
                .map(|mood| serde_json::json!({ "mood": mood, "activity": "talking" })),
            intention_id: source_intent_id.map(str::to_string),
        },
    );
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
        assert!(redeem.contains("emit_live_speech"));
        assert!(redeem.contains("if shown {"));
        assert!(redeem.contains("direct_motion"));
        assert!(!redeem.contains("enqueue_speak_intent"));
        assert!(!src.contains("consider_event"));
        let notify_at = redeem
            .find("emit_speech_notification")
            .expect("redeem notifies");
        let owns_at = redeem
            .find("speech_is_shown(&intent.topic, decision.notify)")
            .expect("merope notify is owned-event only");
        assert!(
            owns_at < notify_at,
            "existing producers must keep the toast; merope must not add a second"
        );
    }

    #[test]
    fn executing_at_redeem_does_not_compose_a_sentence() {
        let looking = IngestSight {
            on_page: true,
            panel_open: true,
            ..Default::default()
        };
        assert!(
            !decide_ingest(
                "agent.merope.platform_activity",
                &IngestSight {
                    executing: true,
                    ..looking.clone()
                }
            )
            .allow_model
        );
        assert!(decide_ingest("agent.merope.platform_activity", &looking).allow_model);
    }

    #[test]
    fn greeting_is_worth_composing_on_the_page() {
        assert!(
            decide_ingest(
                "agent.merope.greeting",
                &IngestSight {
                    on_page: true,
                    panel_open: true,
                    ..Default::default()
                }
            )
            .allow_model
        );
        assert!(
            decide_ingest(
                "agent.merope.greeting",
                &IngestSight {
                    on_page: true,
                    ..Default::default()
                }
            )
            .allow_model
        );
    }

    #[test]
    fn only_merope_owned_speech_is_worth_a_model_call() {
        assert!(speech_is_shown("agent.merope.platform_activity", true));
        assert!(speech_is_shown("agent.merope.report_ready", true));
        assert!(speech_is_shown("agent.merope.greeting", true));
        // These already have a producer sending the notification.
        assert!(!speech_is_shown("agent.task_failed", true));
        assert!(!speech_is_shown("brew.source_error", true));
        assert!(!speech_is_shown("agent.merope.greeting", false));
    }
}
