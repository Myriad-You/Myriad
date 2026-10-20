//! Where a line she says in person goes (the app they are using her on, or
//! her face on the site) and saying it on the app.

use chrono::Utc;
use sea_orm::DatabaseConnection;

use super::super::store::{affect_from_state, get_or_create_state, latest_open_session};
use super::super::{
    MoodTransition, MotionContext, MotionPhase, PerformanceDirective, local_directive,
    resolve_round_motion_style,
};
use crate::services::agent::consciousness::last_live_presence;

const GREETING_EVENT: &str = "agent.merope.greeting";

/// Where a line said in person goes now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InPerson {
    /// The native app they are using her on, whose link takes `say`.
    App(String),
    /// Her face on a page of the site.
    Face,
    /// Neither is in front of them.
    Nowhere,
}

/// The app when that is where they are using her (of the surfaces they are
/// on, the one they came to or used last) and its link takes `say`;
/// otherwise the site's face, when a page is visible. A touch is answered on
/// the face it came from.
pub(crate) fn in_person(user_id: i32, touch: bool) -> InPerson {
    use crate::services::agent::consciousness::{Front, LiveSurface, front_presence};
    if touch {
        if let Some(device) = super::super::touch::touched_app(user_id) {
            return if crate::services::agent::link::can_say(user_id, &device) {
                InPerson::App(device)
            } else {
                InPerson::Nowhere
            };
        }
    } else {
        if let Some(Front {
            surface: LiveSurface::Native,
            device_id: Some(device),
            ..
        }) = front_presence(user_id)
        {
            if crate::services::agent::link::can_say(user_id, &device) {
                return InPerson::App(device);
            }
        }
    }
    if last_live_presence(user_id).page_visible {
        InPerson::Face
    } else {
        InPerson::Nowhere
    }
}

/// Says the line on the app, with how she acts it.
pub(crate) async fn say_on_app(
    db: &DatabaseConnection,
    user_id: i32,
    device: &str,
    id: &str,
    event_key: &str,
    spoken: &str,
    performance: Option<&PerformanceDirective>,
) -> bool {
    use crate::services::agent::link::SayKind;
    let kind = if event_key == GREETING_EVENT {
        SayKind::Greeting
    } else {
        SayKind::FaceToFace
    };
    say_line(
        db,
        user_id,
        device,
        id,
        kind,
        event_key,
        spoken,
        performance,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn say_line(
    db: &DatabaseConnection,
    user_id: i32,
    device: &str,
    id: &str,
    kind: crate::services::agent::link::SayKind,
    event_key: &str,
    spoken: &str,
    performance: Option<&PerformanceDirective>,
) -> bool {
    use crate::services::agent::link::{Say, say};
    let session_id = latest_open_session(db, user_id)
        .await
        .ok()
        .flatten()
        .map(|(id, _)| id);
    say(
        user_id,
        device,
        Say {
            id: id.to_string(),
            kind,
            text: spoken.to_string(),
            session_id,
            perform: performance.and_then(|value| serde_json::to_value(value).ok()),
            event_key: Some(event_key.to_string()),
        },
    )
}

/// Her first words, on the app `device` they are using her on: acted at once
/// with her local base motion, then as her director refines it.
/// Her first words said on the app `device`: the `say` id they went out as,
/// or None when that link does not take them.
pub(crate) async fn say_first_on_app(
    db: &DatabaseConnection,
    user_id: i32,
    device: String,
    event_key: &str,
    line: &str,
) -> Option<String> {
    let id = format!("say_{}", uuid::Uuid::new_v4().simple());
    let context = line_context(db, user_id, event_key, line).await;
    let floor = context.as_ref().and_then(local_directive);
    let said = say_line(
        db,
        user_id,
        &device,
        &id,
        crate::services::agent::link::SayKind::Proactive,
        event_key,
        line,
        floor.as_ref(),
    )
    .await;
    if !said {
        return None;
    }
    if let Some(mut context) = context {
        context.phase = MotionPhase::Delivery;
        super::redeem::spawn_say_refinement(db.clone(), None, device, id.clone(), context);
    }
    Some(id)
}

/// How she acts a line she says on her own, as her mood stands: the context
/// her base motion and her director take, in the proactive phase.
async fn line_context(
    db: &DatabaseConnection,
    user_id: i32,
    event_key: &str,
    spoken: &str,
) -> Option<MotionContext> {
    let current = get_or_create_state(db, user_id).await.ok()?;
    let affect = affect_from_state(&current);
    let mood = MoodTransition::from_affect(
        &affect,
        &affect,
        event_key,
        current.updated_at.with_timezone(&Utc).timestamp_millis(),
    );
    let motion_style = resolve_round_motion_style(
        None,
        current.mood.round() as i32,
        current.arousal.round() as i32,
    )
    .await;
    Some(MotionContext {
        user_id,
        phase: MotionPhase::Proactive,
        mood,
        activity: "talking".to_string(),
        // Nobody said anything to her: she speaks up.
        user_text: String::new(),
        response_text: Some(spoken.to_string()),
        previous_phrases: Vec::new(),
        task_success: None,
        rig_state: crate::services::agent::consciousness::acting_rig_state(user_id),
        motion_style,
    })
}
