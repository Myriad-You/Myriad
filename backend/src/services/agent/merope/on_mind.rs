//! What stays on her mind (see `myriad_merope::on_mind`): kept, changed when
//! something happens to her, thought further while she is idle, and seen by
//! each person only as theirs to see.
//!
//! - **Changed** by what she says when she finishes a thing of her own
//!   (`doing/finishing.rs`) or after an exchange in private (`inner.rs`):
//!   those calls already happen, and ask her once more.
//! - **Thought further** while she lazes about with something on her mind,
//!   about every half hour ([`think_on`]); nothing on her mind, no call.
//!   Done with it, she lets it go; wanting to tell, whoever may hear it and
//!   is here may (the same live-only decision as a passing thought).
//! - **Seen** in her prompt as a line ([`seen_by`]): about her own things
//!   by anyone, about someone only by them.
//!
//! One record of hers in the runtime registry, so a restart does not end
//! it. It stays on the site for now: what goes with her log waits until
//! every end reads it.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use sea_orm::DatabaseConnection;

pub use myriad_merope::on_mind::{Before, OnMind};
use myriad_merope::on_mind::{
    FROM_TALK, STEP_SCHEMA, read_step, step_input, step_schema, step_system, tell_line,
};

pub const NAMESPACE: &str = "merope_on_mind";
const RECORD: &str = "now";
/// Kept this long past the last time it changed.
const KEEP_DAYS: i64 = 30;
const CALL_TIMEOUT: Duration = Duration::from_secs(45);
/// Her notes on what it came from, at most, for thinking it further.
const NOTES_GIVEN: usize = 2;

/// One step of thinking it further at a time.
static THINKING: AtomicBool = AtomicBool::new(false);

fn identity() -> crate::services::runtime_registry::RegistryIdentity<'static> {
    crate::services::runtime_registry::RegistryIdentity {
        subject_id: None,
        owner_id: None,
        tapp_id: None,
        runtime_id: None,
    }
}

/// What is on her mind, if anything.
pub async fn now(db: &DatabaseConnection) -> Option<OnMind> {
    kept(db).await.ok().flatten()
}

/// [`now`], or why it could not be read.
pub(super) async fn kept(db: &DatabaseConnection) -> anyhow::Result<Option<OnMind>> {
    Ok(crate::services::runtime_registry::get::<OnMind>(db, NAMESPACE, RECORD).await?)
}

pub(super) async fn keep(db: &DatabaseConnection, on_mind: &OnMind) {
    let keep_until = (crate::clock::now() + chrono::Duration::days(KEEP_DAYS)).timestamp();
    if let Err(error) = crate::services::runtime_registry::put(
        db, NAMESPACE, RECORD, identity(), on_mind, keep_until,
    )
    .await
    {
        tracing::warn!(%error, "[Merope] could not keep what is on her mind");
    }
}

pub(super) async fn let_go(db: &DatabaseConnection) {
    if let Err(error) = crate::services::runtime_registry::delete(db, NAMESPACE, RECORD).await {
        tracing::warn!(%error, "[Merope] could not let go of what was on her mind");
    }
}

/// What is on her mind as it may be seen `before` them, as a line for a
/// prompt.
pub async fn seen_by(db: &DatabaseConnection, before: Before) -> Option<String> {
    now(db)
        .await
        .map(|on_mind| on_mind.seen_by(before, crate::clock::now()))
}

/// She said `what` will keep turning over in her head, more than what was
/// on her mind: it takes its place. Nothing said, or the same thing said
/// again, nothing changes.
pub async fn offered(db: &DatabaseConnection, what: Option<String>, about: Option<i32>, from: &str) {
    let Some(what) = what else {
        return;
    };
    if now(db).await.is_some_and(|on_mind| on_mind.is(&what)) {
        return;
    }
    let Some(on_mind) = OnMind::new(&what, about, from, crate::clock::now()) else {
        return;
    };
    tracing::info!(from, about_someone = about.is_some(), "[Merope] something stays on her mind");
    keep(db, &on_mind).await;
}

/// Idle with something on her mind, and it is time: she thinks it one step
/// further, in the background. Otherwise nothing, and no call.
pub fn think_on(db: &DatabaseConnection, owner: i32) {
    if THINKING.swap(true, Ordering::AcqRel) {
        return;
    }
    let db = db.clone();
    super::background::spawn("on_mind_step", async move {
        step(&db, owner).await;
        THINKING.store(false, Ordering::Release);
    });
}

pub(super) async fn step(db: &DatabaseConnection, owner: i32) {
    let now = crate::clock::now();
    let Some(mut on_mind) = self::now(db).await.filter(|on_mind| on_mind.step_due(now)) else {
        return;
    };
    // About someone: what they asked her not to keep first.
    if let Some(about) = on_mind.about
        && (!super::asked::old::done_first(db, about).await
            || self::now(db).await.as_ref() != Some(&on_mind))
    {
        return;
    }
    let soul = crate::services::agent::identity::get_speaking_soul()
        .await
        .unwrap_or_default();
    let input = step_input(&on_mind, &notes_on(db, &on_mind.from).await, now);
    let raw = super::call::Ask::new(super::call::Voice::Hers, owner, "on_mind_step")
        .within(CALL_TIMEOUT)
        .json_raw(&step_system(&soul), &input, STEP_SCHEMA, &step_schema())
        .await;
    // Let go of meanwhile, or another thing came onto it: not brought back.
    if self::now(db).await.as_ref() != Some(&on_mind) {
        return;
    }
    let Some(step) = raw.ok().as_deref().and_then(read_step) else {
        // Not thought on this time: the next try is a step later, not the
        // next minute.
        on_mind.thought_at = now;
        keep(db, &on_mind).await;
        return;
    };
    let on_mind = on_mind.stepped(&step.thought, now);
    if step.settled {
        tracing::info!(steps = on_mind.steps, "[Merope] done with what was on her mind");
        let_go(db).await;
    } else {
        keep(db, &on_mind).await;
    }
    if step.tell && super::timing::asleep_now().is_none() {
        tell_whoever_may_hear(&on_mind);
    }
}

/// Her notes on the thing of her own it came from, most recent first; none
/// for what an exchange left her with (what it is says it).
async fn notes_on(db: &DatabaseConnection, from: &str) -> Vec<String> {
    if from == FROM_TALK {
        return Vec::new();
    }
    crate::services::agent::memory::unified::own_experiences(db, 300)
        .await
        .unwrap_or_default()
        .iter()
        .filter_map(|row| super::doing::note_on(row, from))
        .take(NOTES_GIVEN)
        .collect()
}

/// Whoever is here and may hear what it is: the live-only decision of a
/// passing thought is theirs.
fn tell_whoever_may_hear(on_mind: &OnMind) {
    for user_id in crate::services::agent::consciousness::present_users() {
        if user_id > 0
            && let Some(line) = tell_line(on_mind, Before::Private(user_id))
        {
            super::spawn_ingest(user_id, super::wander::THOUGHT_EVENT, line);
        }
    }
}

#[cfg(test)]
#[path = "on_mind_tests.rs"]
mod tests;
