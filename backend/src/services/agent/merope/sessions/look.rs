//! Her careful look at the changes that came together: a session that asks
//! the owner, failed, or finished waits a little with the others that come
//! (until none has come for [`QUIET`], or [`AT_MOST`] after the first),
//! and then she is shown all of them at once, numbered ([`super::shown`]),
//! asked of the model the chat uses with time to think, and her answer held
//! to itself ([`super::judge`]): a card for each that is theirs, and at most
//! one thing said, of the one she chooses ([`super::keep`]).
//!
//! One look per question: the same session asking the same again is not
//! looked at twice. A look fails over once (the model was slow or erred,
//! or her grounds named what she was not shown); a second failure keeps
//! only the cards of what waits on the owner, unsaid.

use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use myriad_her_link::now::Change;
use sea_orm::DatabaseConnection;

use super::Picture;
use super::judge::{self, Answer, Material, Read};
use super::keep;
use super::shown::{self, Shown};
use crate::services::agent::merope::call::{Ask, Failure, Voice};

/// The look, as the model call and its bill are named.
const LOOK: &str = "session_look";
/// How long the model has for one look.
const LOOK_TIMEOUT: Duration = Duration::from_secs(30);
/// How long before the one try again.
const RETRY_AFTER: Duration = Duration::from_secs(5);
/// How long none has come before what came is looked at.
const QUIET: Duration = if cfg!(test) {
    Duration::from_millis(300)
} else {
    Duration::from_secs(20)
};
/// How long after the first they are looked at, at the latest.
const AT_MOST: Duration = if cfg!(test) {
    Duration::from_millis(1500)
} else {
    Duration::from_secs(60)
};

type Question = (i32, String, String, String, String);

/// The questions under way or kept, by owner, device, grant, session and
/// what it asked (a digest): one look each. One whose card was not kept is
/// let go, and a session that moved on lets go of its own.
static LOOKED: LazyLock<Mutex<HashSet<Question>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

/// Looks a dropped grant must not finish, by owner, device and grant.
static DROPPED: LazyLock<Mutex<HashSet<(i32, String, String)>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

struct Waiting {
    first: Instant,
    last: Instant,
    pictures: Vec<Picture>,
}

/// What came and waits to be looked at, by owner (and, in tests, by the
/// test's thread: tests beside each other share owners, not databases).
static WAITING: LazyLock<Mutex<HashMap<(i32, String), Waiting>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn waiting_key(owner: i32) -> (i32, String) {
    let scope = if cfg!(test) {
        format!("{:?}", std::thread::current().id())
    } else {
        String::new()
    };
    (owner, scope)
}

/// When what waits is looked at: none came for a while, or the first came
/// long enough ago.
fn due(waiting: &Waiting, now: Instant) -> Option<Instant> {
    let at = (waiting.last + QUIET).min(waiting.first + AT_MOST);
    (at > now).then_some(at)
}

fn question(owner: i32, picture: &Picture) -> Question {
    (
        owner,
        picture.device.clone(),
        picture.session.grant.clone(),
        picture.session.session.clone(),
        keep::asking(&picture.session),
    )
}

fn same_session(one: &Picture, other: &Picture) -> bool {
    one.device == other.device
        && one.session.grant == other.session.grant
        && one.session.session == other.session.session
}

/// Puts a change with the others that come with it, to be looked at
/// together; a question already looked at is not again.
pub(super) fn add(db: DatabaseConnection, owner: i32, picture: Picture) {
    if matches!(picture.session.change, Change::Asked | Change::Failed)
        && !LOOKED
            .lock()
            .map(|mut looked| looked.insert(question(owner, &picture)))
            .unwrap_or(false)
    {
        return;
    }
    let now = Instant::now();
    let key = waiting_key(owner);
    let first = {
        let Ok(mut waiting) = WAITING.lock() else {
            return;
        };
        match waiting.get_mut(&key) {
            Some(held) => {
                held.last = now;
                // The session's newest change stands for it.
                held.pictures.retain(|kept| !same_session(kept, &picture));
                held.pictures.push(picture);
                false
            }
            None => {
                waiting.insert(
                    key.clone(),
                    Waiting {
                        first: now,
                        last: now,
                        pictures: vec![picture],
                    },
                );
                true
            }
        }
    };
    if !first {
        return;
    }
    crate::services::agent::merope::background::spawn("a look at sessions", async move {
        loop {
            let next = WAITING
                .lock()
                .ok()
                .and_then(|waiting| waiting.get(&key).map(|held| due(held, Instant::now())));
            match next {
                // Dropped meanwhile.
                None => return,
                Some(Some(at)) => tokio::time::sleep_until(at.into()).await,
                Some(None) => break,
            }
        }
        let Some(held) = WAITING
            .lock()
            .ok()
            .and_then(|mut waiting| waiting.remove(&key))
        else {
            return;
        };
        let pictures = held.pictures;
        let kept = match look(&db, owner, &pictures).await {
            Ok(kept) => kept,
            Err(error) => {
                tracing::warn!(%error, "[Merope] a look at sessions failed");
                vec![false; pictures.len()]
            }
        };
        // A question whose card was not kept may be looked at when it
        // comes again.
        if let Ok(mut looked) = LOOKED.lock() {
            for (picture, kept) in pictures.iter().zip(kept) {
                if !kept {
                    looked.remove(&question(owner, picture));
                }
            }
        }
    });
}

/// The session moved on from what it asked: what it asks next is looked at
/// afresh, even the same again.
pub(super) fn moved_on(owner: i32, device: &str, grant: &str, session: &str) {
    if let Ok(mut looked) = LOOKED.lock() {
        looked.retain(|(who, from, under, which, _)| {
            !(*who == owner && from == device && under == grant && which == session)
        });
    }
}

/// Nothing more of a dropped grant is looked at or kept.
pub(super) fn cancel(owner: i32, device: &str, grant: &str) {
    if let Ok(mut looked) = LOOKED.lock() {
        looked.retain(|(who, from, under, _, _)| {
            !(*who == owner && from == device && under == grant)
        });
    }
    if let Ok(mut waiting) = WAITING.lock() {
        for ((who, _), held) in waiting.iter_mut() {
            if *who == owner {
                held.pictures.retain(|picture| {
                    !(picture.device == device && picture.session.grant == grant)
                });
            }
        }
        waiting.retain(|_, held| !held.pictures.is_empty());
    }
    if let Ok(mut dropped) = DROPPED.lock() {
        dropped.insert((owner, device.to_string(), grant.to_string()));
    }
}

/// Whether the owner dropped `picture`'s grant.
pub(super) fn dropped(owner: i32, picture: &Picture) -> bool {
    grant_dropped(owner, &picture.device, &picture.session.grant)
}

/// As a restart leaves it: nothing dropped in memory.
#[cfg(test)]
pub(super) fn restarted() {
    if let Ok(mut dropped) = DROPPED.lock() {
        dropped.clear();
    }
    if let Ok(mut looked) = LOOKED.lock() {
        looked.clear();
    }
}

/// Whether the owner dropped `device`'s `grant` (in this process).
pub(super) fn grant_dropped(owner: i32, device: &str, grant: &str) -> bool {
    DROPPED
        .lock()
        .is_ok_and(|dropped| dropped.contains(&(owner, device.to_string(), grant.to_string())))
}

/// Her answer, asked once more when the first did not come or did not
/// hold; none when neither did. Every answer the model gave comes back as
/// it gave it.
async fn ask(owner: i32, soul: &str, material: &Material) -> (Option<Answer>, Vec<String>) {
    let call = Ask::new(Voice::Chat, owner, LOOK)
        .within(LOOK_TIMEOUT)
        .billed_as("sessions");
    let system = judge::system(soul);
    let schema = judge::schema();
    let mut prompt = judge::prompt(material);
    let mut answers = Vec::new();
    for attempt in 0..2 {
        match call.json_raw(&system, &prompt, LOOK, &schema).await {
            Ok(raw) => {
                let read = judge::read(&raw, material);
                answers.push(raw);
                match read {
                    Read::Good(answer) => return (Some(answer), answers),
                    Read::BadGrounds => prompt = judge::again(material),
                    Read::Unreadable => {}
                }
            }
            // Not while her mind is elsewhere, nor with no model: no retry.
            Err(Failure::Unavailable) => return (None, answers),
            Err(error) => {
                tracing::info!(%error, attempt, "[Merope] a look at sessions did not come");
                answers.push(format!("(no answer: {error})"));
                if attempt == 0 {
                    tokio::time::sleep(RETRY_AFTER).await;
                }
            }
        }
    }
    (None, answers)
}

/// Looks at `pictures` together; for each, whether what she made of it
/// was kept.
async fn look(
    db: &DatabaseConnection,
    owner: i32,
    pictures: &[Picture],
) -> Result<Vec<bool>, anyhow::Error> {
    let mut kept = vec![false; pictures.len()];
    if pictures.is_empty() || crate::services::persona_sync::lease::paused("a look at sessions") {
        return Ok(kept);
    }
    let others = super::pictures(db, owner).await;
    let mut shown = Vec::new();
    for picture in pictures {
        shown.push(Shown {
            picture,
            others: &others,
            memories: shown::memories(db, owner, &picture.session).await,
            before: keep::before(db, owner, picture).await?,
        });
    }
    let said = super::said_lately(db, owner).await;
    let material = shown::material(&shown, &said);
    let waits: Vec<bool> = pictures
        .iter()
        .map(|picture| keep::waits_on_owner(&picture.session))
        .collect();
    let soul = crate::services::agent::merope::resolve_speaking_soul()
        .await
        .unwrap_or_default();
    let (answer, answers) = ask(owner, &soul, &material).await;
    if !answers.is_empty() {
        keep::ledger(db, owner, pictures, &material, answers).await;
    }
    let answer = match answer {
        Some(answer) => judge::settle(answer, &waits),
        None => Answer {
            judgments: vec![None; pictures.len()],
            line: None,
        },
    };
    for (at, picture) in pictures.iter().enumerate() {
        if dropped(owner, picture) {
            continue;
        }
        // Nothing with grounds for one that waits on them: it still has
        // its card.
        let judgment = match &answer.judgments[at] {
            Some(judgment) => judgment.clone(),
            None if waits[at] => judge::card_only(&shown::now_line(&picture.session)),
            None => continue,
        };
        if judge::has_card(&judgment) || picture.session.change == Change::Failed {
            kept[at] = keep::card(db, owner, picture, &judgment, &material).await?;
            // Dropped while it was written: it goes with the rest of the grant.
            if dropped(owner, picture) {
                super::forget_grant(db, owner, &picture.device, &picture.session.grant).await?;
            }
        } else {
            // Nothing for them: looked at, nothing to keep.
            kept[at] = true;
        }
    }
    if let Some(line) = &answer.line
        && let Some(judgment) = &answer.judgments[line.change - 1]
    {
        let picture = &pictures[line.change - 1];
        if kept[line.change - 1] && !dropped(owner, picture) {
            keep::say(db, owner, picture, &line.text, judgment);
            // Dropped as it was made: taken back, wherever it is by now.
            if dropped(owner, picture) {
                crate::services::agent::consciousness::withdraw_speak_intents(
                    owner,
                    &myriad_nodes::sessions::grant_ref(&picture.device, &picture.session.grant),
                );
            }
        }
    }
    Ok(kept)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_came_goes_once_quiet_or_once_held_long_enough() {
        let start = Instant::now();
        let waiting = Waiting {
            first: start,
            last: start,
            pictures: Vec::new(),
        };
        assert_eq!(due(&waiting, start), Some(start + QUIET));
        assert_eq!(due(&waiting, start + QUIET), None);
        // Kept coming: it goes once the first waited long enough.
        let busy = Waiting {
            first: start,
            last: start + AT_MOST,
            pictures: Vec::new(),
        };
        assert_eq!(due(&busy, start + QUIET), Some(start + AT_MOST));
        assert_eq!(due(&busy, start + AT_MOST), None);
    }
}
