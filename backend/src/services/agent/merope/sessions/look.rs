//! Her careful look at the changes that came together: a session that asks
//! the owner, failed, or finished waits a little with the others that come
//! (until none has come for a while, or the first waited long enough: the
//! contract's `sessions.look`), and then she is shown all of them at once,
//! asked of the model the chat uses with time to think, and her answer held
//! to itself, all by the shared core's rules
//! ([`myriad_nodes::session_look`]): a card for each that is theirs, and at
//! most one thing said, of the one she chooses ([`super::keep`]).
//!
//! One look per question: the same session asking the same again is not
//! looked at twice. A look fails over once (the model was slow or erred,
//! or her grounds named what she was not shown); a second failure keeps
//! only the cards of what waits on the owner, unsaid.

use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use myriad_her_link::now::{Change, Session, WaitingOn, rules};
use myriad_nodes::session_look::{
    self as looks, Answer, Judgment, LookAsk, LookInput, Read, Recalled, Shown, Told, Verdict,
};
use sea_orm::DatabaseConnection;

use super::Picture;
use super::keep;
use crate::services::agent::merope::call::{Ask, Failure, Voice};

/// The look, as the model call and its bill are named.
const LOOK: &str = "session_look";
/// How long the model has for one look.
const LOOK_TIMEOUT: Duration = Duration::from_secs(30);
/// How long before the one try again.
const RETRY_AFTER: Duration = Duration::from_secs(5);
/// What she recalls of the owner for each change.
const MEMORIES: usize = looks::MEMORIES;

/// How long none has come before what came is looked at (shorter in tests).
fn quiet() -> Duration {
    if cfg!(test) {
        Duration::from_millis(300)
    } else {
        Duration::from_millis(rules().look.quiet_ms.max(0) as u64)
    }
}

/// How long after the first they are looked at, at the latest.
fn at_most() -> Duration {
    if cfg!(test) {
        Duration::from_millis(1500)
    } else {
        Duration::from_millis(rules().look.at_most_ms.max(0) as u64)
    }
}

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
    let at = (waiting.last + quiet()).min(waiting.first + at_most());
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

/// What she recalls of the owner for this session.
async fn memories(db: &DatabaseConnection, owner: i32, session: &Session) -> Vec<Recalled> {
    let query = format!(
        "{} {}",
        session.title.as_deref().unwrap_or_default(),
        session.from_agent.as_deref().unwrap_or_default()
    );
    let present = crate::services::agent::memory::unified::Audience::private(owner);
    match crate::services::agent::memory::unified::recall_primed(
        db,
        owner,
        &present,
        Some(query.trim()).filter(|query| !query.is_empty()),
        &crate::services::agent::memory::unified::MemoryKind::ABOUT_PERSON,
        MEMORIES,
        &Default::default(),
        1.0,
    )
    .await
    {
        Ok((recalled, _)) => recalled
            .iter()
            .map(|record| Recalled {
                id: record.id.clone(),
                text: crate::services::agent::merope::store::as_known(record),
            })
            .filter(|recalled| !recalled.text.is_empty())
            .collect(),
        Err(error) => {
            tracing::warn!(%error, "[Merope] no memories for a session's look");
            Vec::new()
        }
    }
}

fn told(picture: &Picture) -> Told {
    Told {
        device: picture.device.clone(),
        session: picture.session.clone(),
    }
}

/// What she is asked of `pictures` together, now.
async fn asking_of(
    db: &DatabaseConnection,
    owner: i32,
    pictures: &[Picture],
) -> Result<LookAsk, anyhow::Error> {
    let others: Vec<Told> = super::pictures(db, owner).await.iter().map(told).collect();
    let mut changes = Vec::new();
    for picture in pictures {
        changes.push(Shown {
            told: told(picture),
            memories: memories(db, owner, &picture.session).await,
            before: keep::before(db, owner, picture).await?,
        });
    }
    let soul = crate::services::agent::merope::resolve_speaking_soul()
        .await
        .unwrap_or_default();
    Ok(looks::look_input(LookInput {
        changes,
        others,
        said: super::said_lately(db, owner).await,
        at: crate::clock::now().timestamp_millis(),
        soul,
    })?)
}

/// Her answer, asked once more when the first did not come or did not
/// hold; none when neither did. Every answer the model gave comes back as
/// it gave it, with how it came.
async fn ask(owner: i32, asking: &LookAsk) -> (Option<Answer>, Vec<String>, Vec<keep::Reply>) {
    let call = Ask::new(Voice::Chat, owner, LOOK)
        .within(LOOK_TIMEOUT)
        .billed_as("sessions")
        .strict();
    let mut prompt = &asking.prompt;
    let mut answers = Vec::new();
    let mut replies = Vec::new();
    for attempt in 0..2 {
        let (result, reported) = crate::services::ai_cost_ledger::reporting(call.json_raw(
            &asking.system,
            prompt,
            LOOK,
            &asking.schema,
        ))
        .await;
        replies.push(keep::Reply::of(reported.as_ref()));
        match result {
            Ok(raw) => {
                let read = looks::read(&raw, &asking.material);
                if let Some(reply) = replies.last_mut() {
                    reply.read = Some(read_word(&raw, &read).to_string());
                }
                answers.push(raw);
                match read {
                    Read::Good(answer) => return (Some(answer), answers, replies),
                    Read::BadGrounds => prompt = &asking.again,
                    Read::Unreadable => {}
                }
            }
            // Not while her mind is elsewhere, nor with no model: no retry.
            Err(Failure::Unavailable) => {
                replies.pop();
                return (None, answers, replies);
            }
            Err(error) => {
                tracing::info!(%error, attempt, "[Merope] a look at sessions did not come");
                answers.push(format!("(no answer: {error})"));
                if attempt == 0 {
                    tokio::time::sleep(RETRY_AFTER).await;
                }
            }
        }
    }
    (None, answers, replies)
}

/// How an answer read, for the ledger: `good`, `bad_grounds`,
/// `unreadable`, or `hollow` (it holds to the schema, but every change in
/// it says nothing of what it is about: the form with nothing in it).
fn read_word(raw: &str, read: &Read) -> &'static str {
    let hollow = serde_json::from_str::<serde_json::Value>(raw)
        .ok()
        .and_then(|answer| answer["changes"].as_array().cloned())
        .is_some_and(|changes| {
            !changes.is_empty()
                && changes.iter().all(|change| {
                    change["about"]
                        .as_str()
                        .is_none_or(|about| about.trim().is_empty())
                })
        });
    match read {
        _ if hollow => "hollow",
        Read::Good(_) => "good",
        Read::BadGrounds => "bad_grounds",
        Read::Unreadable => "unreadable",
    }
}

/// Her card when no answer with grounds came for a change that waits on
/// them, or failed: as the core makes it, but one that waits on the
/// owner's own answer (`waitingOn: owner`, whoever gave the work) is
/// theirs to decide, never a nod. That much the program knows without her.
fn card_without_her(heading: &str, waits: bool, session: &Session) -> Judgment {
    let mut judgment = looks::card_only(heading, waits);
    if session.waiting_on == Some(WaitingOn::Owner) {
        judgment.verdict = Verdict::Decide;
    }
    judgment
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
    let asking = asking_of(db, owner, pictures).await?;
    let (answer, answers, replies) = ask(owner, &asking).await;
    if !answers.is_empty() {
        keep::ledger(db, owner, pictures, &asking.material, answers, replies).await;
    }
    let answer = match answer {
        Some(answer) => looks::settle(answer, &asking.waits),
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
            None if asking.waits[at] || asking.failed[at] => card_without_her(
                &asking.material.changes[at],
                asking.waits[at],
                &picture.session,
            ),
            None => continue,
        };
        if looks::keeps_card(&judgment, asking.failed[at]) {
            kept[at] = keep::card(db, owner, picture, &judgment, &asking.material).await?;
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
        assert_eq!(due(&waiting, start), Some(start + quiet()));
        assert_eq!(due(&waiting, start + quiet()), None);
        // Kept coming: it goes once the first waited long enough.
        let busy = Waiting {
            first: start,
            last: start + at_most(),
            pictures: Vec::new(),
        };
        assert_eq!(due(&busy, start + quiet()), Some(start + at_most()));
        assert_eq!(due(&busy, start + at_most()), None);
    }
}
