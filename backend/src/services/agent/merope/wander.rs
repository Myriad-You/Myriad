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
//!
//! The rules are the core's (`myriad_merope::wander`: when the mind drifts
//! and what the thought says; `recall::wander`: where it drifts); who is
//! present, the record and handing the thought on are here.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use chrono::{DateTime, Utc};
use myriad_agent_rules::memory::recall::NOT_AGAIN_SECS;
use myriad_merope::wander::{quiet_enough, thought};
use sea_orm::DatabaseConnection;

use crate::services::agent::memory::unified;

pub use myriad_merope::wander::THOUGHT_EVENT;
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

/// Her last thought about them, and when she last thought of each memory
/// still within the core's not-again window.
fn recently_thought(
    user_id: i32,
    now: DateTime<Utc>,
) -> (Option<DateTime<Utc>>, HashMap<String, DateTime<Utc>>) {
    let Ok(mut held) = WANDERING.lock() else {
        return (None, HashMap::new());
    };
    let entry = held.entry(user_id).or_default();
    entry
        .thought
        .retain(|(at, _)| (now - *at).num_seconds() < NOT_AGAIN_SECS);
    let mut thought = HashMap::new();
    for (at, id) in &entry.thought {
        thought.insert(id.clone(), *at);
    }
    (entry.last, thought)
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

/// What the thought is, for the event decision (the core's words).
fn thought_summary(wandered: &unified::Wandered) -> String {
    let gap = wandered
        .gap
        .as_ref()
        .map(|(gap, known)| (gap.as_str(), *known));
    thought(&wandered.memory.content, gap)
}

/// One minute of her idle mind across everyone present.
pub async fn tick(db: DatabaseConnection) {
    // Asleep, her mind does not wander out loud.
    if !super::is_enabled().await || super::timing::asleep_now().is_some() {
        return;
    }
    let myself = super::self_state::current(&db).await;
    let now = crate::clock::now();
    for user_id in crate::services::agent::consciousness::present_users() {
        if user_id <= 0 || !super::is_logged_in_addressee(user_id) {
            continue;
        }
        // Her face where they are using her: a page's, or the app's.
        let live = crate::services::agent::consciousness::face_presence(user_id);
        if !crate::services::agent::consciousness::can_speak_to(&live) || live.speaking {
            continue;
        }
        let Ok(state) = super::get_or_create_state(&db, user_id).await else {
            continue;
        };
        if super::effective_do_not_disturb(&state) || super::current_activity(&state) != "idle" {
            continue;
        }
        let (last_thought, thought_at) = recently_thought(user_id, now);
        let utc =
            |at: Option<chrono::DateTime<chrono::FixedOffset>>| at.map(|at| at.with_timezone(&Utc));
        let spoke_up = crate::services::agent::speech::ledger::last_unasked_at(
            &db,
            &crate::services::agent::speech::private(user_id),
        )
        .await;
        if !quiet_enough(
            utc(state.last_user_message_at),
            spoke_up,
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
            &thought_at,
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
    fn a_thought_is_not_had_again_for_days() {
        let user = -93_001;
        let now = crate::clock::now();
        note_thought(user, "mem_cat".into(), now - chrono::Duration::days(1));
        note_thought(user, "mem_tea".into(), now - chrono::Duration::days(4));
        let (last, thought_at) = recently_thought(user, now);
        assert!(last.is_some());
        assert!(thought_at.contains_key("mem_cat"));
        assert!(!thought_at.contains_key("mem_tea"), "long enough ago");
    }
}
