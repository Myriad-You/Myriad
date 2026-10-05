//! Her standing acting: what she does with her body while nobody is talking
//! with her, shown on her face instead of written beside it. The director
//! decides it, as it decides all of her acting, from what she is doing on her
//! own; one direction serves everyone watching for as long as she keeps
//! doing that.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use chrono::{DateTime, Utc};
use myriad_merope::RigStateSummary;
use sea_orm::DatabaseConnection;

use super::store::{affect_from_state, get_or_create_state};
use super::{
    MoodTransition, MotionContext, MotionPhase, PerformanceDirective, doing, mood_band,
    refine_motion, resolve_round_motion_style,
};

/// Directions kept, one per thing she does (and way of watching it).
const KEPT: usize = 8;
const RETRY: chrono::Duration = chrono::Duration::minutes(2);

struct Kept {
    ends: DateTime<Utc>,
    direction: Option<PerformanceDirective>,
}

static KEPT_DIRECTIONS: LazyLock<Mutex<HashMap<String, Kept>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
/// One director call at a time: everyone opening her face at once waits for the same one.
static DIRECTING: LazyLock<tokio::sync::Mutex<()>> = LazyLock::new(|| tokio::sync::Mutex::new(()));

/// How she acts what she is doing now, for someone watching her face; None
/// when she is doing nothing in particular or the director has nothing.
/// `together` is someone listening along to the song she is on.
pub async fn standing_direction(
    db: &DatabaseConnection,
    user_id: i32,
    rig_state: Option<RigStateSummary>,
    together: bool,
) -> Option<PerformanceDirective> {
    let now = Utc::now();
    // Only what she is doing: why she picked it stays hers, and the minutes in
    // are left out so one direction holds for the whole of it.
    let (started, ends, what) = if let Some(current) = doing::current() {
        (
            current.started,
            current.ends,
            format!("{} {}", current.thing.verb(), current.thing.describe()),
        )
    } else {
        let lazing = doing::lazing()?;
        (
            lazing.started,
            lazing.ends,
            super::pace::lazing_line(lazing.kind).to_string(),
        )
    };
    let state = get_or_create_state(db, user_id).await.ok()?;
    let band = mood_band(state.mood, state.arousal);
    let capabilities = rig_state
        .as_ref()
        .map(|rig| rig.capabilities.join(","))
        .unwrap_or_default();
    let key = format!(
        "{}|{together}|{band}|{capabilities}",
        started.timestamp_millis()
    );
    if let Some(kept) = kept(&key, now) {
        return kept;
    }
    let _directing = DIRECTING.lock().await;
    if let Some(kept) = kept(&key, now) {
        return kept;
    }
    let affect = affect_from_state(&state);
    let mood = MoodTransition::from_affect(&affect, &affect, "standing", now.timestamp_millis());
    let motion_style = resolve_round_motion_style(
        rig_state.as_ref(),
        state.mood.round() as i32,
        state.arousal.round() as i32,
    )
    .await;
    let activity = if together {
        format!("{what}, and the person who has her face open is listening along with her")
    } else {
        what
    };
    let direction = refine_motion(MotionContext {
        user_id,
        phase: MotionPhase::Presence,
        mood,
        activity,
        user_text: String::new(),
        response_text: None,
        previous_phrases: Vec::new(),
        task_success: None,
        rig_state,
        motion_style,
    })
    .await
    .filter(|direction| !direction.score.is_empty() || direction.plan.baseline.is_some());
    if let Ok(mut kept) = KEPT_DIRECTIONS.lock() {
        kept.retain(|_, item| item.ends > now);
        while kept.len() >= KEPT {
            let Some(oldest) = kept
                .iter()
                .min_by_key(|(_, item)| item.ends)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            kept.remove(&oldest);
        }
        // A director with nothing to say is asked again soon, not for the whole of it.
        let ends = if direction.is_some() {
            ends
        } else {
            ends.min(now + RETRY)
        };
        kept.insert(
            key,
            Kept {
                ends,
                direction: direction.clone(),
            },
        );
    }
    direction
}

fn kept(key: &str, now: DateTime<Utc>) -> Option<Option<PerformanceDirective>> {
    let kept = KEPT_DIRECTIONS.lock().ok()?;
    kept.get(key)
        .filter(|item| item.ends > now)
        .map(|item| item.direction.clone())
}
