//! What she meant to bring up that came due while they asked to be left
//! alone (`left_alone`). It is not said as it comes due, nor forgotten: past
//! the end, what is left of it is asked of her at once, whether she would
//! still write, and said in one message if so.
//!
//! Nothing is settled before it is known: she is asleep, they are talking or
//! on do-not-disturb, the judgment fails or is not answered, the message does
//! not go out: it stays waiting, and is looked at again ([`ready`]); what
//! comes due then meanwhile does not wait with it. Said, it is settled; that
//! she would not is too. An answer that does not come is asked for again
//! every [`AGAIN_AFTER`] as long as the intent waits: it is only let go of
//! when she says she would not, or [`TRIES_AT_MOST`] messages were sent and
//! no way out took one (one that was not written, or that another line of
//! hers went before, is none).
//!
//! The message is one thing in the ledger, [`SAID`] and a mark of each of
//! those it was about ([`mark`]). Said and not settled (the run ends between),
//! what it was about is settled and not said twice ([`told`]), whichever of
//! them are left; what was not among them is asked about like any other,
//! also when it was due before (it came since, from another end of hers).
//!
//! Which came due meanwhile is told from when it was meant for, so a restart
//! loses none; what waits on a node, and has no time, is remembered here, and
//! a restart takes it as anything else of hers.

use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use myriad_merope::reach::{NOT_MID_TALK, Reason};
use myriad_merope::speech::Refusal;
use myriad_nodes::body::{IntentStatus, Origin};
use sea_orm::DatabaseConnection;
use sha2::{Digest, Sha256};

use super::{
    intent_store::{self, Held},
    intents_due::settle,
};
use crate::services::agent::merope::{
    clock,
    reach::{self, Unsaid},
    threads::Thread,
    timing,
};
use crate::services::agent::speech::{
    self, Admission, Source,
    admit::{admit, facts, goes_on},
};

/// How long she waits to be asked again when no answer came, or nothing went
/// out.
const AGAIN_AFTER: Duration = Duration::from_secs(10 * 60);
/// How often a message may be sent and not go out before it is let go: each
/// costs a line of hers written.
const TRIES_AT_MOST: u32 = 6;

/// What its message is about, in the ledger, before their marks: hers
/// (`left_alone::OWN`).
pub(super) const SAID: &str = "own|late|";
/// The most of them asked about at once; the rest, the next time.
const AT_ONCE: usize = 60;

/// Those that came due while they were to be left alone.
static WAITED: LazyLock<Mutex<HashSet<(i32, String)>>> = LazyLock::new(Default::default);
/// When `user_id`'s are asked about again, and how often nothing went out.
static TRIED: LazyLock<Mutex<HashMap<i32, (Instant, u32)>>> = LazyLock::new(Default::default);

pub(super) fn wait(user_id: i32, id: &str) {
    if let Ok(mut waited) = WAITED.lock() {
        waited.insert((user_id, id.to_string()));
    }
}

fn waited(user_id: i32, id: &str) -> bool {
    WAITED
        .lock()
        .is_ok_and(|waited| waited.contains(&(user_id, id.to_string())))
}

/// When `held` is due, when it has a time.
fn due(held: &Held) -> Option<i64> {
    held.trigger.as_ref()?["at"].as_i64()
}

/// Whether `held`, hers to `user_id`, came due before `ended`, when they were
/// to be left alone.
pub(super) fn is(user_id: i32, held: &Held, ended: Option<DateTime<Utc>>) -> bool {
    let before = due(held)
        .zip(ended)
        .is_some_and(|(at, ended)| at < ended.timestamp_micros());
    before || waited(user_id, &held.id)
}

/// Whether anything of hers to `user_id` waits that came due before `ended`.
/// It cannot be told: as if it did.
pub(in crate::services::agent::merope) async fn any_left(
    db: &DatabaseConnection,
    user_id: i32,
    ended: DateTime<Utc>,
) -> bool {
    let Ok(audiences) = intent_store::waiting_audiences(db).await else {
        return true;
    };
    for (_, audience) in audiences.iter().filter(|(user, _)| *user == user_id) {
        let Ok(held) = intent_store::held(db, user_id, audience).await else {
            return true;
        };
        if held.iter().any(|one| {
            one.waiting() && one.origin != Some(Origin::Partner) && is(user_id, one, Some(ended))
        }) {
            return true;
        }
    }
    false
}

/// Whether she may be asked about them now: she is awake, they are not in the
/// middle of a talk, and she was not just asked in vain.
pub(super) async fn ready(db: &DatabaseConnection, user_id: i32) -> bool {
    let again = TRIED.lock().is_ok_and(|tried| {
        tried
            .get(&user_id)
            .is_some_and(|(at, _)| at.elapsed() < AGAIN_AFTER)
    });
    !again
        && timing::asleep_now().is_none()
        && reach::last_talk(db, user_id)
            .await
            .is_none_or(|last| clock::now() - last >= NOT_MID_TALK)
}

/// Asked again after a while; how often nothing has gone out, this one too
/// when `failed`.
fn later(user_id: i32, failed: bool) -> u32 {
    let Ok(mut tried) = TRIED.lock() else {
        return 0;
    };
    let before = tried.get(&user_id).map_or(0, |(_, tries)| *tries);
    let tries = before + u32::from(failed);
    tried.insert(user_id, (Instant::now(), tries));
    tries
}

/// What stands for the intent `id` in the name of the message about it.
pub(super) fn mark(id: &str) -> String {
    hex::encode(&Sha256::digest(id.as_bytes())[..8])
}

/// Those of `held` (hers) that a message of these was about, settled as said:
/// it went out, and the run ended before they were.
pub(super) async fn told(
    db: &DatabaseConnection,
    user_id: i32,
    audience: &str,
    held: &[&Held],
) -> HashSet<String> {
    let marks: HashSet<String> = speech::ledger::bearing(db, &speech::private(user_id), None)
        .await
        .iter()
        .filter(|said| said.reached())
        .filter_map(|said| said.thing.as_deref()?.strip_prefix(SAID))
        .flat_map(|marks| marks.split(','))
        .map(str::to_string)
        .collect();
    let told: Vec<&Held> = held
        .iter()
        .copied()
        .filter(|held| marks.contains(&mark(&held.id)))
        .collect();
    done(db, user_id, audience, &told, IntentStatus::Said).await;
    told.iter().map(|held| held.id.clone()).collect()
}

/// What is left of `held` (hers, come due), asked of her at once.
pub(super) async fn bring_up(
    db: &DatabaseConnection,
    user_id: i32,
    audience: &str,
    mut held: Vec<&Held>,
) {
    held.sort_by_key(|held| (due(held).unwrap_or(i64::MAX), held.at, held.id.clone()));
    held.truncate(AT_ONCE);
    let marks: Vec<String> = held.iter().map(|held| mark(&held.id)).collect();
    let thing = format!("{SAID}{}", marks.join(","));
    let to = speech::private(user_id);
    let mut wish = speech::wish(to.clone(), Source::Reach, thing.clone(), to);
    wish.thing = Some(thing.clone());
    let admission = admit(db, &wish).await;
    if let Admission::Refused(Refusal::SameThing { .. }) = &admission {
        // Said, longer ago than [`told`] looks back.
        done(db, user_id, audience, &held, IntentStatus::Said).await;
        return;
    }
    if !goes_on(&admission) {
        return;
    }
    let reason = Reason {
        due: Vec::new(),
        wished: held
            .iter()
            .map(|held| Thread {
                id: held.id.clone(),
                about: held.text.clone(),
                then: String::new(),
                due: None,
                hers: true,
            })
            .collect(),
        days_since: reach::last_talk(db, user_id)
            .await
            .map(|last| (clock::now() - last).num_days()),
        to_tell: Vec::new(),
        to_try: None,
    };
    let talk = reach::recent_talk(db, user_id).await;
    let judged = reach::would_write(db, user_id, &reason, &talk, facts(&admission)).await;
    let Some(about) = judged else {
        later(user_id, false);
        return;
    };
    let Some(about) = about else {
        tracing::info!(user_id, "[Merope] what she meant to say earlier is let be");
        done(db, user_id, audience, &held, IntentStatus::Dropped).await;
        return;
    };
    // She sees all of them as she writes: one left out is one she left out.
    let all: Vec<&str> = held.iter().map(|held| held.text.as_str()).collect();
    let about = format!(
        "{about} (all you meant to bring up while they asked to be left alone: {})",
        all.join("; ")
    );
    match reach::say_now(db, user_id, Source::Reach, &thing, &about).await {
        Ok(_) => done(db, user_id, audience, &held, IntentStatus::Said).await,
        Err(why) => {
            if later(user_id, why == Unsaid::NotDelivered) >= TRIES_AT_MOST {
                done(db, user_id, audience, &held, IntentStatus::Dropped).await;
            }
        }
    }
}

async fn done(
    db: &DatabaseConnection,
    user_id: i32,
    audience: &str,
    held: &[&Held],
    status: IntentStatus,
) {
    if held.is_empty() {
        return;
    }
    for held in held {
        settle(db, user_id, audience, held, status).await;
        if let Ok(mut waited) = WAITED.lock() {
            waited.remove(&(user_id, held.id.clone()));
        }
    }
    if let Ok(mut tried) = TRIED.lock() {
        tried.remove(&user_id);
    }
}

/// What the process remembers of `user_id` is forgotten, as a restart does.
#[cfg(test)]
pub(super) fn restart(user_id: i32) {
    if let Ok(mut waited) = WAITED.lock() {
        waited.retain(|(user, _)| *user != user_id);
    }
    if let Ok(mut tried) = TRIED.lock() {
        tried.remove(&user_id);
    }
}

/// As if it was a while since `user_id` was last asked about; how often a
/// message did not go out stays.
#[cfg(test)]
pub(super) fn again(user_id: i32) {
    if let Ok(mut tried) = TRIED.lock()
        && let Some((at, _)) = tried.get_mut(&user_id)
        && let Some(before) = at.checked_sub(AGAIN_AFTER)
    {
        *at = before;
    }
}
