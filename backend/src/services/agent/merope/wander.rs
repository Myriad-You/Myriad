//! Mind-wandering while someone is here and quiet.
//!
//! When a person has the site open, can see her, and has not said anything
//! for a while, her mind may drift over what she knows of them (a random walk
//! along association, no model involved) and settle on something. That
//! thought goes through the same event decision as anything else she might
//! bring up — do-not-disturb, work in progress, cooldown, her own energy —
//! and is usually let go. It is only ever spoken live: it never becomes a
//! notification or a diary entry.
//!
//! Wandering needs someone present so the thought has somewhere to go.
//! Whether she is up to saying it, tired or not, is the event decision's,
//! which sees her energy. What she thought of lately is not thought of again
//! for a few days. That record is attention, not memory, and lives in
//! process memory only.

use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex};

use chrono::{DateTime, Utc};
use sea_orm::DatabaseConnection;

use crate::services::agent::memory::unified;

pub const THOUGHT_EVENT: &str = "agent.merope.thought";
/// Quiet this long before the mind starts to drift.
const QUIET_SECS: i64 = 3 * 60;
/// Between two thoughts about the same person.
const BETWEEN_THOUGHTS_SECS: i64 = 10 * 60;
/// A thought is not had again about the same memory for this long.
const NOT_AGAIN_SECS: i64 = 3 * 24 * 3600;
const THOUGHTS_KEPT: usize = 64;

#[derive(Default)]
struct Wandering {
    last: Option<DateTime<Utc>>,
    thought: Vec<(DateTime<Utc>, String)>,
}

static WANDERING: LazyLock<Mutex<HashMap<i32, Wandering>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// A new persona has had no thoughts yet.
pub(super) fn forget() {
    if let Ok(mut wandering) = WANDERING.lock() {
        wandering.clear();
    }
}

/// Whether it has been quiet long enough, since they spoke, since she last
/// spoke up and since her last thought, for her mind to drift to them.
fn quiet_enough(
    last_user_message_at: Option<DateTime<Utc>>,
    last_proactive_at: Option<DateTime<Utc>>,
    last_thought_at: Option<DateTime<Utc>>,
    cooldown_secs: i64,
    now: DateTime<Utc>,
) -> bool {
    let since = |at: Option<DateTime<Utc>>| at.map(|at| (now - at).num_seconds());
    since(last_user_message_at).is_none_or(|secs| secs >= QUIET_SECS)
        && since(last_proactive_at).is_none_or(|secs| secs >= cooldown_secs)
        && since(last_thought_at).is_none_or(|secs| secs >= BETWEEN_THOUGHTS_SECS)
}

fn recently_thought(user_id: i32, now: DateTime<Utc>) -> (Option<DateTime<Utc>>, HashSet<String>) {
    let Ok(mut held) = WANDERING.lock() else {
        return (None, HashSet::new());
    };
    let entry = held.entry(user_id).or_default();
    entry
        .thought
        .retain(|(at, _)| (now - *at).num_seconds() < NOT_AGAIN_SECS);
    (
        entry.last,
        entry.thought.iter().map(|(_, id)| id.clone()).collect(),
    )
}

fn note_thought(user_id: i32, memory_id: String, now: DateTime<Utc>) {
    if let Ok(mut held) = WANDERING.lock() {
        let entry = held.entry(user_id).or_default();
        entry.last = Some(now);
        entry.thought.push((now, memory_id));
        if entry.thought.len() > THOUGHTS_KEPT {
            entry.thought.remove(0);
        }
    }
}

/// What the thought is, for the event decision. Where it lands on something
/// she knows only a little about, that fact comes with it; whether it makes
/// her want to know more is the decision's to judge.
fn thought_summary(wandered: &unified::Wandered) -> String {
    match &wandered.gap {
        Some((gap, known)) => format!(
            "你忽然想起关于对方的一件事：{}。关于「{gap}」，你只记得{}。",
            wandered.memory.content,
            if *known <= 1 {
                "这一件事"
            } else {
                "两件事"
            }
        ),
        None => format!("你忽然想起关于对方的一件事：{}", wandered.memory.content),
    }
}

/// One minute of her idle mind across everyone present.
pub async fn tick(db: DatabaseConnection) {
    // Asleep, her mind does not wander out loud.
    if !super::is_enabled().await || super::timing::asleep_now().is_some() {
        return;
    }
    let myself = super::self_state::current(&db).await;
    let now = Utc::now();
    for user_id in crate::services::agent::consciousness::present_users() {
        if user_id <= 0 || !super::is_logged_in_addressee(user_id) {
            continue;
        }
        let live = crate::services::agent::consciousness::last_live_presence(user_id);
        if !live.page_visible || !live.face_visible || live.speaking {
            continue;
        }
        let Ok(state) = super::get_or_create_state(&db, user_id).await else {
            continue;
        };
        if super::effective_do_not_disturb(&state) || super::current_activity(&state) != "idle" {
            continue;
        }
        let (last_thought, avoid) = recently_thought(user_id, now);
        let utc =
            |at: Option<chrono::DateTime<chrono::FixedOffset>>| at.map(|at| at.with_timezone(&Utc));
        if !quiet_enough(
            utc(state.last_user_message_at),
            utc(state.last_proactive_at),
            last_thought,
            myself.proactive_cooldown_secs(),
            now,
        ) {
            continue;
        }
        let mut roll = rand::random::<f64>;
        let landed = unified::wander(
            &db,
            user_id,
            &unified::Audience::private(user_id),
            &super::priming::current(user_id),
            &avoid,
            &mut roll,
        )
        .await;
        let Ok(Some(wandered)) = landed else {
            continue;
        };
        note_thought(user_id, wandered.memory.id.clone(), now);
        tracing::info!(
            user_id,
            curious = wandered.gap.is_some(),
            "[Merope] a thought came to mind"
        );
        super::spawn_ingest(user_id, THOUGHT_EVENT, thought_summary(&wandered));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mind_drifts_only_after_quiet_and_not_too_often() {
        let now = Utc::now();
        let ago = |secs: i64| Some(now - chrono::Duration::seconds(secs));
        assert!(quiet_enough(ago(600), None, None, 180, now));
        assert!(!quiet_enough(ago(30), None, None, 180, now), "just spoke");
        assert!(
            !quiet_enough(ago(600), ago(100), None, 180, now),
            "she spoke up a moment ago"
        );
        assert!(
            !quiet_enough(ago(600), ago(300), None, 600, now),
            "a tired cooldown is longer"
        );
        assert!(
            !quiet_enough(ago(600), None, ago(60), 180, now),
            "just had a thought"
        );
        assert!(quiet_enough(None, None, None, 180, now));
    }

    #[test]
    fn a_thought_is_not_had_again_for_days() {
        let user = -93_001;
        let now = Utc::now();
        note_thought(user, "mem_cat".into(), now - chrono::Duration::days(1));
        note_thought(user, "mem_tea".into(), now - chrono::Duration::days(4));
        let (last, avoid) = recently_thought(user, now);
        assert!(last.is_some());
        assert!(avoid.contains("mem_cat"));
        assert!(!avoid.contains("mem_tea"), "long enough ago");
    }
}
