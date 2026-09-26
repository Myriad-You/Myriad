//! Speak intent in → sentence out. Rechecks sight here. Live and notify are
//! independent channels.

use super::delivery_claim::DeliveryCoordinator;
use chrono::{DateTime, FixedOffset, Utc};
use futures::{StreamExt, stream};
use once_cell::sync::Lazy;
use sea_orm::DatabaseConnection;
use std::collections::HashMap;

use super::super::gates::{IngestDecision, decide_ingest, is_task_outcome, is_valuable_event};
use super::super::store::{
    affect_from_state, get_or_create_state, get_persona, insert_proactive, latest_open_session,
    recent_proactive, recently_spoke_event, touch_proactive,
};
use super::super::{
    MoodTransition, MotionContext, MotionPhase, PerformanceDirective, addressee_speaking_section,
    direct_motion, format_mood_section, local_directive, public_persona_name, refine_motion,
    resolve_addressee_label, resolve_round_motion_style,
};
use super::{
    SAME_EVENT_MINUTES, compact_summary, current_sight, is_enabled, is_trivial_line, log_skip,
};
use crate::services::agent::consciousness::{SpeakIntent, drain_speak_intents, last_live_presence};
use crate::services::agent::merope::gates::IngestSight;
use crate::services::agent::notification_preferences::{ACTION_OPEN_AGENT, NotificationEventKey};
use crate::services::agent::notifications::{
    LiveSpeech, Notification, NotificationPriority, NotificationType, get_notification_manager,
};
use crate::services::ai::create_strict_lite_ai_analyzer_with_timeout;

static DELIVERY: Lazy<DeliveryCoordinator> = Lazy::new(DeliveryCoordinator::default);

fn merope_owns_notify(event_key: &str) -> bool {
    event_key.starts_with("agent.merope.")
}

pub async fn tick_speak_intents(db: DatabaseConnection) {
    if !is_enabled().await {
        return;
    }
    let now = Utc::now();
    let mut users: HashMap<i32, Vec<SpeakIntent>> = HashMap::new();
    for intent in drain_speak_intents(now) {
        users.entry(intent.user_id).or_default().push(intent);
    }
    // Preserve each addressee's queue order without making one slow model
    // stall every other addressee taken by this drain.
    stream::iter(users.into_values())
        .for_each_concurrent(8, |intents| {
            let db = &db;
            async move {
                for intent in intents {
                    if let Err(error) = redeem_speak_intent(db, intent).await {
                        tracing::warn!(%error, "[Merope] redeem speak intent failed");
                    }
                }
            }
        })
        .await;
}

async fn redeem_speak_intent(
    db: &DatabaseConnection,
    intent: SpeakIntent,
) -> Result<(), anyhow::Error> {
    if intent.expires_at <= Utc::now() {
        return Ok(());
    }
    let Some(mut claim) = DELIVERY.claim(&intent).await else {
        log_skip(
            intent.user_id,
            &intent.topic,
            "delivery_already_claimed_or_expired",
        );
        return Ok(());
    };
    let touch = intent.topic == "agent.merope.touch";
    let state = get_or_create_state(db, intent.user_id).await?;
    let input_at = state.last_user_message_at;
    let Some(decision) = current_delivery(db, &intent, input_at, false).await else {
        return Ok(());
    };
    let repeat_minutes = if touch { 1 } else { SAME_EVENT_MINUTES };
    if recently_spoke_event(db, intent.user_id, &intent.topic, repeat_minutes).await? {
        log_skip(intent.user_id, &intent.topic, "recently_spoke");
        return Ok(());
    }
    let last_proactive = state
        .last_proactive_at
        .map(|value| value.with_timezone(&Utc));
    let cooldown = crate::services::agent::merope::self_state::current(db)
        .await
        .proactive_cooldown_secs();
    if within_proactive_cooldown(&intent.topic, last_proactive, Utc::now(), cooldown) {
        log_skip(intent.user_id, &intent.topic, "proactive_cooldown");
        return Ok(());
    }

    let source_intent_id = intent.work_intent_id.as_deref();
    let shown = decision.live
        || (source_intent_id.is_some() && is_valuable_event(&intent.topic))
        || speech_is_shown(&intent.topic, decision.notify);
    let spoken = if touch {
        // The consciousness decision already contains the in-person sentence.
        // Do not paraphrase it in a second model call or invent a fallback.
        sanitize_speech(&intent.gist)
    } else if shown {
        // Background composition does not own the foreground activity. A late
        // completion must not reset a newer Chat/Work task to idle.
        // Nothing composed, nothing said: she never speaks a stock line.
        match compose_line(db, intent.user_id, &intent.gist).await {
            Some(line) => line,
            None => {
                log_skip(intent.user_id, &intent.topic, "not_composed");
                return Ok(());
            }
        }
    } else {
        // Seen by no one: nothing to say, and nothing to pretend was said.
        log_skip(intent.user_id, &intent.topic, "not_shown");
        return Ok(());
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

    let Some(decision) = current_delivery(db, &intent, input_at, false).await else {
        return Ok(());
    };
    // `direct_motion` is the non-live `shown` branch. Compose may already spend
    // Lite; a later `current_delivery` re-read can still drop the line.
    let mut pending_motion = None;
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
                let mut context = MotionContext {
                    user_id: intent.user_id,
                    phase: MotionPhase::Proactive,
                    mood: mood.clone(),
                    activity: "talking".to_string(),
                    user_text: intent
                        .observation
                        .clone()
                        .unwrap_or_else(|| intent.gist.clone()),
                    response_text: Some(spoken.clone()),
                    previous_phrases: Vec::new(),
                    task_success: None,
                    rig_state: last_live_presence(intent.user_id).rig_state,
                    motion_style,
                };
                let performance = if decision.live {
                    // Touch already has a local embodied response; do not reset its face.
                    let floor = if touch {
                        None
                    } else {
                        local_directive(&context)
                    };
                    context.phase = MotionPhase::Delivery;
                    pending_motion = Some(context);
                    floor
                } else {
                    direct_motion(context).await
                };
                (performance, Some(mood))
            }
            Err(_) => (None, None),
        }
    } else {
        (None, None)
    };

    // Re-read after every model operation, before persistence and delivery.
    // Applies to every event, not just touch, and remembers a new user input
    // even if its Chat run already finished while composition was in flight.
    let Some(decision) = current_delivery(db, &intent, input_at, false).await else {
        return Ok(());
    };
    // Merope toasts only what it owns. Task / phantasi / sync already have a
    // producer; sending ours as well would be two notices for one event.
    let mut delivered = false;
    if decision.live && shown {
        delivered = emit_live_speech(
            intent.user_id,
            &intent.id,
            &intent.topic,
            &spoken,
            performance.as_ref(),
            motion_mood.as_ref(),
            source_intent_id,
        );
        if delivered {
            claim.delivered(&intent, repeat_minutes);
        }
        if let Some(context) = pending_motion.take().filter(|_| delivered) {
            let user_id = intent.user_id;
            let id = intent.id.clone();
            let motion_db = db.clone();
            let motion_intent = intent.clone();
            tokio::spawn(async move {
                let Some(performance) = refine_motion(context).await else {
                    return;
                };
                if !current_delivery(&motion_db, &motion_intent, input_at, true)
                    .await
                    .is_some_and(|decision| decision.live)
                {
                    return;
                }
                // The client additionally requires this exact line to still be playing.
                if let (Some(manager), Ok(value)) = (
                    get_notification_manager(),
                    serde_json::to_value(performance),
                ) {
                    manager.emit_live_speech_motion(user_id, id, value);
                }
            });
        }
    }
    let notified = if speech_is_shown(&intent.topic, decision.notify) {
        emit_speech_notification(
            db,
            &intent,
            input_at,
            &spoken,
            performance.as_ref(),
            motion_mood.as_ref(),
            source_intent_id,
        )
        .await
    } else {
        false
    };
    // The transcript and cooldown describe accepted delivery, not composition.
    // Suppression or an unavailable transport must not consume either.
    if delivered || notified {
        claim.delivered(&intent, repeat_minutes);
        insert_proactive(db, intent.user_id, &spoken, Some(&intent.topic), notified).await?;
        let _ = touch_proactive(db, intent.user_id).await;
    }
    Ok(())
}

async fn current_delivery(
    db: &DatabaseConnection,
    intent: &SpeakIntent,
    input_at: Option<DateTime<FixedOffset>>,
    refinement: bool,
) -> Option<IngestDecision> {
    let state = get_or_create_state(db, intent.user_id).await.ok()?;
    let sight = current_sight(intent.user_id, &state).await;
    let decision = delivery_decision(
        intent,
        input_at,
        state.last_user_message_at,
        &sight,
        is_enabled().await,
        Utc::now(),
    )?;
    let live = last_live_presence(intent.user_id);
    if (intent.topic == "agent.merope.touch"
        && (!live.page_visible || !live.face_visible || (!refinement && live.speaking)))
        || (refinement && !live.face_visible)
    {
        return None;
    }
    Some(decision)
}

fn delivery_decision(
    intent: &SpeakIntent,
    input_at: Option<DateTime<FixedOffset>>,
    current_input_at: Option<DateTime<FixedOffset>>,
    sight: &IngestSight,
    enabled: bool,
    now: DateTime<Utc>,
) -> Option<IngestDecision> {
    if !enabled || intent.expires_at <= now || input_at != current_input_at {
        return None;
    }
    let decision = decide_ingest(&intent.topic, sight);
    decision.allow_model.then_some(decision)
}

/// Merope toast gate: `notify && merope_owns_notify`. Live speech can still
/// reach the face when this is false.
fn speech_is_shown(event_key: &str, notify: bool) -> bool {
    notify && merope_owns_notify(event_key)
}

async fn compose_line(db: &DatabaseConnection, user_id: i32, summary: &str) -> Option<String> {
    let Some(analyzer) =
        create_strict_lite_ai_analyzer_with_timeout(Some(std::time::Duration::from_secs(30)))
            .await
            .map(crate::services::analyzer::AiAnalyzer::with_light_thinking)
    else {
        return None;
    };
    let soul = crate::services::agent::identity::get_speaking_soul()
        .await
        .unwrap_or_else(|| "You are Agent.".to_string());
    let mut sections =
        crate::services::agent::merope::speaking_prompt_for_event(db, user_id, summary).await;
    // How the last exchange left her, if it was a moment ago.
    sections.extend(
        super::super::inner::current(
            user_id,
            &crate::services::agent::memory::unified::Audience::private(user_id),
        )
        .as_deref()
        .and_then(crate::services::agent::merope::format_inner_moment_ago_section),
    );
    let mind = crate::services::agent::merope::speaking_prompt_plain(&sections);
    let recent = recent_proactive(db, user_id, 6)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|row| format!("- {}", compact_summary(&row.content)))
        .collect::<Vec<_>>()
        .join("\n");
    let recent_block = if recent.is_empty() {
        "(no prior lines to this person)".to_string()
    } else {
        recent
    };
    let system =
        super::super::speaking_prompts::compose_proactive_system(&soul, &mind, &recent_block);
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
            (!is_trivial_line(&spoken)).then_some(spoken)
        }
        Err(error) => {
            tracing::debug!(%error, "[Merope] Lite speech failed; saying nothing");
            None
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
) -> bool {
    let Some(manager) = get_notification_manager() else {
        return false;
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
    )
}

async fn emit_speech_notification(
    db: &DatabaseConnection,
    intent: &SpeakIntent,
    input_at: Option<DateTime<FixedOffset>>,
    spoken: &str,
    performance: Option<&PerformanceDirective>,
    mood: Option<&MoodTransition>,
    source_intent_id: Option<&str>,
) -> bool {
    let user_id = intent.user_id;
    let event_key = &intent.topic;
    let Some(manager) = get_notification_manager() else {
        return false;
    };
    // Only catalogued, interrupt-worthy events may become a notification.
    let Some(event) =
        NotificationEventKey::from_key(event_key).filter(|event| event.interrupts_when_away())
    else {
        return false;
    };
    let title = display_name(db).await;
    let session_id = latest_open_session(db, user_id)
        .await
        .ok()
        .flatten()
        .map(|(id, _)| id);
    let mut metadata = serde_json::json!({
        "action": ACTION_OPEN_AGENT,
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
    .with_event(event, metadata);
    // Name/session lookup above may also yield. Decide the notification channel
    // from the current panel/DND state, not the original routing decision.
    if !current_delivery(db, intent, input_at, false)
        .await
        .is_some_and(|decision| speech_is_shown(event_key, decision.notify))
    {
        return false;
    }
    manager.notify(notification).await
}

async fn display_name(db: &DatabaseConnection) -> String {
    let stored = get_persona(db)
        .await
        .ok()
        .flatten()
        .map(|persona| persona.name);
    public_persona_name(true, stored.as_deref())
}

/// 刚主动说过话就先不再开口。间隔跨事件计算（同一事件的重复另有
/// `SAME_EVENT_MINUTES` 管），由她自己的精力决定，最短
/// `BASE_PROACTIVE_COOLDOWN_SECS`，累了就等更久（见 `self_state`）。
/// 触摸是对当下动作的回应，任务结果是对方在等的事，这两类不受间隔限制。
fn within_proactive_cooldown(
    topic: &str,
    last_proactive_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    cooldown_secs: i64,
) -> bool {
    if topic == "agent.merope.touch" || is_task_outcome(topic) {
        return false;
    }
    let cooldown =
        cooldown_secs.max(crate::services::agent::merope::self_state::BASE_PROACTIVE_COOLDOWN_SECS);
    last_proactive_at
        .is_some_and(|last| now.signed_duration_since(last) < chrono::Duration::seconds(cooldown))
}

#[cfg(test)]
mod tests {
    #[test]
    fn proactive_speech_waits_between_events_but_not_for_touch_or_outcomes() {
        let now = chrono::Utc::now();
        let recent = Some(now - chrono::Duration::seconds(30));
        let old = Some(
            now - chrono::Duration::seconds(
                crate::services::agent::merope::self_state::BASE_PROACTIVE_COOLDOWN_SECS + 1,
            ),
        );
        assert!(super::within_proactive_cooldown(
            "phantasi.digest",
            recent,
            now,
            180
        ));
        assert!(!super::within_proactive_cooldown(
            "phantasi.digest",
            old,
            now,
            180
        ));
        assert!(!super::within_proactive_cooldown(
            "phantasi.digest",
            None,
            now,
            180
        ));
        assert!(!super::within_proactive_cooldown(
            "agent.merope.touch",
            recent,
            now,
            180
        ));
        assert!(!super::within_proactive_cooldown(
            "agent.task_completed",
            recent,
            now,
            180
        ));
        // A tired persona waits longer; nothing makes the wait shorter.
        assert!(super::within_proactive_cooldown(
            "phantasi.digest",
            old,
            now,
            600
        ));
        assert!(super::within_proactive_cooldown(
            "phantasi.digest",
            Some(now - chrono::Duration::seconds(100)),
            now,
            10
        ));
    }
    #[test]
    fn every_event_rechecks_input_expiry_switch_dnd_and_delivery_surface() {
        let now = Utc::now();
        let input = Some(now.fixed_offset());
        let here = IngestSight {
            on_page: true,
            panel_open: true,
            ..Default::default()
        };
        for topic in [
            "agent.merope.touch",
            "agent.merope.greeting",
            "agent.merope.platform_activity",
            "agent.task_completed",
        ] {
            let intent = crate::services::agent::consciousness::new_speak_intent(
                1,
                "event".into(),
                topic.into(),
                "hello".into(),
                Default::default(),
                None,
            );
            let decide = |current, sight: &IngestSight, enabled, at| {
                delivery_decision(&intent, input, current, sight, enabled, at)
            };
            let allowed = decide(input, &here, true, now).unwrap();
            assert!(allowed.live && !allowed.notify);
            assert!(
                decide(input, &here, false, now).is_none(),
                "{topic}: disabled"
            );
            assert!(
                decide(input, &here, true, intent.expires_at).is_none(),
                "{topic}: expired"
            );
            assert!(
                decide(
                    Some((now + chrono::Duration::milliseconds(1)).fixed_offset()),
                    &here,
                    true,
                    now
                )
                .is_none(),
                "{topic}: new input even if already idle"
            );
            assert!(
                decide(
                    input,
                    &IngestSight {
                        do_not_disturb: true,
                        ..here.clone()
                    },
                    true,
                    now
                )
                .is_none(),
                "{topic}: DND"
            );
            let away = decide(input, &IngestSight::default(), true, now);
            if topic == "agent.merope.touch" {
                assert!(away.is_none());
            } else {
                let away = away.unwrap();
                assert!(!away.live && away.notify, "{topic}: recompute routing");
            }
            if topic != "agent.task_completed" {
                assert!(
                    decide(
                        input,
                        &IngestSight {
                            executing: true,
                            ..here.clone()
                        },
                        true,
                        now
                    )
                    .is_none()
                );
            }
        }
    }

    #[test]
    fn background_composition_cannot_reset_foreground_activity_and_outputs_are_guarded() {
        let src = include_str!("redeem.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        assert!(!src.contains("set_activity("));
        let before_write = src.split("insert_proactive(").next().unwrap();
        assert!(before_write.contains("if delivered || notified"));
        assert!(before_write.contains("delivered = emit_live_speech("));
        assert!(before_write.contains("let notified = if"));
        assert!(
            before_write.rfind("current_delivery(").unwrap()
                > before_write.rfind("direct_motion(context).await").unwrap()
        );
        let notify = src
            .split("async fn emit_speech_notification(")
            .nth(1)
            .unwrap();
        assert!(
            notify.find("current_delivery(").unwrap()
                > notify.find("latest_open_session(").unwrap()
        );
        assert!(
            notify.find("current_delivery(").unwrap() < notify.find("manager.notify(").unwrap()
        );
    }

    #[test]
    fn live_speech_is_published_before_background_refinement() {
        let source = include_str!("redeem.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        let live = source.split("if decision.live && shown").nth(1).unwrap();
        assert!(live.find("emit_live_speech(").unwrap() < live.find("tokio::spawn").unwrap());
        assert!(live.contains("refine_motion(context).await"));
        assert!(live.contains("emit_live_speech_motion(user_id, id, value)"));
        let selection = source
            .split("let performance = if decision.live {")
            .nth(1)
            .unwrap()
            .split("pending_motion =")
            .next()
            .unwrap();
        assert!(!selection.contains(".await"));
        assert!(selection.contains("MotionPhase::Delivery"));
    }
    use super::*;

    /// She never speaks a stock line: a line nobody will see, or one that
    /// could not be composed, is not said at all.
    #[test]
    fn nothing_composed_is_nothing_said() {
        let src = include_str!("redeem.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        assert!(!src.contains("There you are."));
        assert!(!src.contains("Something came up"));
        assert!(src.contains("\"not_composed\""));
        assert!(src.contains("\"not_shown\""));
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
        // Other producers own these toasts.
        assert!(!speech_is_shown("agent.task_failed", true));
        assert!(!speech_is_shown("phantasi.source_error", true));
        // Merope-owned but `notify` is false.
        assert!(!speech_is_shown("agent.merope.greeting", false));
    }
}
