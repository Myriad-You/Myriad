//! What they asked her not to keep (`unified::asked_not_to_keep`), held to
//! wherever something of theirs is about to be kept: once it is written out,
//! one small ask says which of it says any of that, and those are left out.
//! What writes it never sees what they asked, so nothing of that can end up
//! in it; this alone does, and answers only which. No one who asked nothing
//! is asked about.
//!
//! Going over a private talk does not come here: it is shown what they asked
//! itself (`chat_remember::settle_raw`), for it is also where they ask it.

pub(super) mod lines;
pub(super) mod old;
mod round;
pub(super) mod unread;

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use myriad_merope::chat_remember::Asked;
use myriad_merope::not_to_keep::{
    NOT_ASKED_SCHEMA_NAME, NOT_TO_KEEP_SCHEMA_NAME, not_asked_input, not_asked_schema,
    not_asked_system, not_to_keep_input, not_to_keep_schema, not_to_keep_system, parse_not_asked,
    parse_not_to_keep,
};

use super::call::{Ask, Failure as CallFailure, Voice};
use super::chat_remember::ChatMemoryUpdates;
use super::memory_jobs::Failure;

const TIMEOUT: Duration = Duration::from_secs(30);
/// An ask unanswered, or whose answer is not one, is asked again this many
/// times in all.
const TRIES: u32 = 3;
/// With less time than this left, it is not asked at all.
const LEAST: Duration = Duration::from_secs(2);

static ASKS: AtomicU64 = AtomicU64::new(0);
static LOOKED_AGAIN: AtomicU64 = AtomicU64::new(0);
static ALL_LEFT_OUT: AtomicU64 = AtomicU64::new(0);
static LEFT_OUT: AtomicU64 = AtomicU64::new(0);
static UNANSWERED: AtomicU64 = AtomicU64::new(0);
static DIARY_NOT_TOLD: AtomicU64 = AtomicU64::new(0);
static NOT_ASKED: AtomicU64 = AtomicU64::new(0);
static LET_BE: AtomicU64 = AtomicU64::new(0);

/// How often it was asked since the site started, how often everything was
/// to be left out and was looked at again, how often that stood and all of
/// it was, how much was left out, and how much was not written for want of
/// an answer; lines of a talk marked; lines of her diary not written for
/// want of room to ask; what a line in a group was read as asking and does
/// not; what she held from before that was closed, and how often something
/// of it was not said for not being judged yet; what was never told of and
/// left as it was; group talk let go of unkept for the same; and lines let
/// go for want of room where they waited. Counts only.
pub(crate) fn counts() -> serde_json::Value {
    let of = |count: &AtomicU64| count.load(Ordering::Relaxed);
    serde_json::json!({
        "asks": of(&ASKS),
        "lookedAgain": of(&LOOKED_AGAIN),
        "allLeftOut": of(&ALL_LEFT_OUT),
        "leftOut": of(&LEFT_OUT),
        "unanswered": of(&UNANSWERED),
        "diaryNotTold": of(&DIARY_NOT_TOLD),
        "notAsked": of(&NOT_ASKED),
        "linesMarked": of(&lines::MARKED),
        "oldClosed": of(&old::CLOSED),
        "oldHeldBack": of(&old::HELD_BACK),
        "letBe": of(&LET_BE),
        "notWritten": of(&NOT_WRITTEN),
        "groupLetGo": of(&GROUP_LET_GO),
        "foldedAway": super::memory_jobs::folded_away(),
    })
}

/// New, and not told of while others beside it were: not written.
static NOT_WRITTEN: AtomicU64 = AtomicU64::new(0);

static GROUP_LET_GO: AtomicU64 = AtomicU64::new(0);

/// A stretch of a group's talk let go of, none of it kept: what those in it
/// asked not to keep was not told while its words were at hand.
pub(crate) fn group_let_go() {
    GROUP_LET_GO.fetch_add(1, Ordering::Relaxed);
}

/// A line of her diary not written, for there was no room to ask about it.
pub(super) fn diary_not_told() {
    DIARY_NOT_TOLD.fetch_add(1, Ordering::Relaxed);
}

/// Which of `items`, about to be kept of `user_id`, to leave out. Nothing
/// asked about when they asked nothing. An `Err` is no answer: none of the
/// items may be written then, only tried again later.
pub(super) async fn to_leave_out(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    items: &[String],
) -> Result<BTreeSet<usize>, Failure> {
    left_out(db, user_id, &By::default(), items).await
}

/// [`to_leave_out`], of items others are in too: what is said of `who` in
/// them alone. Told by `until`, or not at all.
pub(super) async fn to_leave_out_of(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    who: Option<&str>,
    items: &[String],
    until: Instant,
) -> Result<BTreeSet<usize>, Failure> {
    let by = By {
        who,
        now: &[],
        until: Some(until),
    };
    left_out(db, user_id, &by, items).await
}

/// What items are judged by, beside what is kept of it.
#[derive(Default)]
struct By<'a> {
    /// Of items others are in too: what is said of them alone.
    who: Option<&'a str>,
    /// What they ask in what the items come from, not kept yet.
    now: &'a [Asked],
    /// Answered by then, or not at all.
    until: Option<Instant>,
}

async fn left_out(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    by: &By<'_>,
    items: &[String],
) -> Result<BTreeSet<usize>, Failure> {
    let told = told_apart(db, user_id, by, items).await?;
    Ok(told.out.into_iter().chain(told.untold).collect())
}

/// What is told of `items`, new: one by one where they are not told
/// together, and what is not told so, beside others that were, is not kept
/// (it is what it says, not who asks; counted). An `Err`: none of them told,
/// or one that could not be asked about; by what was shown, when any failed
/// so, for that is given up on in the end.
async fn told_apart(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    by: &By<'_>,
    items: &[String],
) -> Result<Told, Failure> {
    let asked = what_they_asked(db, user_id, by).await?;
    if items.is_empty() || asked.is_empty() {
        return Ok(Told::default());
    }
    let mut round = round::Round::of(user_id, &asked, by, items);
    round.gone_over().await;
    let failed = round.failed();
    let lets_go = failed.is_none_or(CallFailure::of_what_was_shown) && round.any_told();
    let mut told = round.told();
    if let Some(failure) = failed.filter(|_| !lets_go) {
        return Err(told.shown.unwrap_or(failure).into());
    }
    told.failure = None;
    NOT_WRITTEN.fetch_add(told.untold.len() as u64, Ordering::Relaxed);
    Ok(told)
}

/// What `user_id` asked her not to keep, and asks now; nothing of no one's.
async fn what_they_asked(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    by: &By<'_>,
) -> Result<Vec<String>, Failure> {
    if user_id <= 0 {
        return Ok(Vec::new());
    }
    let mut asked: Vec<String> =
        crate::services::agent::memory::unified::asked_not_to_keep(db, user_id)
            .await
            .map_err(|_| Failure::Storage)?
            .into_iter()
            .map(|row| super::ingest::compact_summary(&row.content))
            .filter(|held| !by.now.iter().any(|one| one.replaces.contains(held)))
            .collect();
    for one in by.now {
        if !asked.contains(&one.fact) {
            asked.push(one.fact.clone());
        }
    }
    Ok(asked)
}

/// Of `items`, those that say something, each with its place: what says
/// nothing is of nothing.
fn saying_something(items: &[String]) -> (Vec<usize>, Vec<String>) {
    items
        .iter()
        .enumerate()
        .filter(|(_, item)| !item.trim().is_empty())
        .map(|(place, item)| (place, item.clone()))
        .unzip()
}

/// What was told of some items: which are to be left out, and which were
/// not told, and why.
#[derive(Debug, Default)]
pub(super) struct Told {
    pub(super) out: BTreeSet<usize>,
    pub(super) untold: BTreeSet<usize>,
    pub(super) failure: Option<Failure>,
    /// Of what was shown, the first such failure of them, whatever came
    /// after ([`Unanswered::shown`]).
    shown: Option<CallFailure>,
}

/// An ask with no answer that will do: how it ended, and whether any try of
/// it failed for what was shown.
struct Unanswered {
    failure: CallFailure,
    shown: Option<CallFailure>,
}

impl Told {
    /// How an ask ended, noted.
    fn unanswered(&mut self, unanswered: Unanswered) -> CallFailure {
        self.shown = self.shown.or(unanswered.shown);
        unanswered.failure
    }

    /// Of what she already held: what was not told stays as it is, to be
    /// asked again (`Err`), or, with `gives_up`, let be and counted.
    pub(super) fn let_be(self, gives_up: bool) -> Result<(), Failure> {
        match self.failure {
            None => Ok(()),
            Some(failure) if gives_up && failure.untold() => {
                LET_BE.fetch_add(self.untold.len() as u64, Ordering::Relaxed);
                tracing::warn!(
                    untold = self.untold.len(),
                    %failure,
                    "[Merope] whether they asked not to keep it was never told: left as it is"
                );
                Ok(())
            }
            Some(failure) => Err(failure),
        }
    }
}

/// Which of `items`, that she already holds, are to go ([`to_leave_out_of`]),
/// told by `until`: what is not told together is asked one by one, and
/// nothing not told is of it.
pub(super) async fn told_of(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    who: Option<&str>,
    items: &[String],
    until: Instant,
) -> Told {
    let by = By {
        who,
        now: &[],
        until: Some(until),
    };
    told_by(db, user_id, &by, items).await
}

async fn told_by(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    by: &By<'_>,
    items: &[String],
) -> Told {
    let asked = match what_they_asked(db, user_id, by).await {
        Ok(asked) if items.is_empty() || asked.is_empty() => return Told::default(),
        Ok(asked) => asked,
        Err(failure) => {
            return Told {
                untold: (0..items.len()).collect(),
                failure: Some(failure),
                ..Told::default()
            };
        }
    };
    let mut round = round::Round::of(user_id, &asked, by, items);
    round.gone_over().await;
    round.told()
}

/// Whether `user_id` asked her not to keep anything; as though they had when
/// that cannot be read.
pub(in crate::services::agent) async fn anything(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
) -> bool {
    crate::services::agent::memory::unified::asked_not_to_keep(db, user_id)
        .await
        .map_or(true, |asked| !asked.is_empty())
}

/// Of `items`, about to be kept of `user_id`, those to keep, by what each
/// says (`text`); `None` when that could not be told, and none may be kept.
pub(in crate::services::agent) async fn to_keep<T>(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    items: Vec<T>,
    text: impl Fn(&T) -> String,
) -> Option<Vec<T>> {
    let texts: Vec<String> = items.iter().map(text).collect();
    let out = to_leave_out(db, user_id, &texts).await.ok()?;
    Some(
        items
            .into_iter()
            .enumerate()
            .filter(|(index, _)| !out.contains(index))
            .map(|(_, item)| item)
            .collect(),
    )
}

/// One ask's answer. All of it to be left out is looked at once more: only
/// what both say is, and all of it when both do (counted, `allLeftOut`).
async fn judged(
    user_id: i32,
    asked: &[String],
    by: &By<'_>,
    items: &[String],
) -> Result<Vec<usize>, Unanswered> {
    let drop = answered(user_id, asked, by, items).await?;
    if drop.len() < items.len() {
        return Ok(drop);
    }
    LOOKED_AGAIN.fetch_add(1, Ordering::Relaxed);
    let again = answered(user_id, asked, by, items).await?;
    let both: Vec<usize> = drop
        .into_iter()
        .filter(|index| again.contains(index))
        .collect();
    if both.len() == items.len() {
        ALL_LEFT_OUT.fetch_add(1, Ordering::Relaxed);
    }
    Ok(both)
}

async fn answered(
    user_id: i32,
    asked: &[String],
    by: &By<'_>,
    items: &[String],
) -> Result<Vec<usize>, Unanswered> {
    ASKS.fetch_add(1, Ordering::Relaxed);
    tried(
        user_id,
        &not_to_keep_system(),
        &not_to_keep_input(asked, by.who, items),
        NOT_TO_KEEP_SCHEMA_NAME,
        &not_to_keep_schema(),
        by.until,
        |raw| parse_not_to_keep(raw, items.len()),
    )
    .await
}

/// Of what `line`, said in a group, was read as asking her not to keep,
/// what it does ask. An `Err` is no answer: the line is gone over again
/// later, nothing of it kept till then.
pub(super) async fn really_asked(
    user_id: i32,
    line: &str,
    asked: Vec<Asked>,
    until: Option<Instant>,
) -> Result<Vec<Asked>, CallFailure> {
    let facts: Vec<String> = asked.iter().map(|one| one.fact.clone()).collect();
    let not_asked = tried(
        user_id,
        &not_asked_system(),
        &not_asked_input(line, &facts),
        NOT_ASKED_SCHEMA_NAME,
        &not_asked_schema(),
        until,
        |raw| parse_not_asked(raw, facts.len()),
    )
    .await
    .map_err(|unanswered| unanswered.failure)?;
    NOT_ASKED.fetch_add(not_asked.len() as u64, Ordering::Relaxed);
    Ok(asked
        .into_iter()
        .enumerate()
        .filter(|(index, _)| !not_asked.contains(index))
        .map(|(_, one)| one)
        .collect())
}

/// One ask of the judge, again while it is unanswered or unreadable and
/// there is time. Out of time before that many asks, it got no answer in
/// time, whatever the last of them came to.
async fn tried(
    user_id: i32,
    system: &str,
    input: &str,
    name: &str,
    schema: &serde_json::Value,
    until: Option<Instant>,
    read: impl Fn(&str) -> Option<Vec<usize>>,
) -> Result<Vec<usize>, Unanswered> {
    let mut failure = CallFailure::Timeout;
    let mut shown = None;
    for tried in 1..=TRIES {
        let within = until.map_or(TIMEOUT, |until| {
            until.saturating_duration_since(Instant::now()).min(TIMEOUT)
        });
        if within < LEAST {
            failure = CallFailure::Timeout;
            break;
        }
        let ask = Ask::new(Voice::Judge, user_id, "not_to_keep")
            .within(within)
            .output_at_most(2048)
            .read_by_asker();
        let answer = ask
            .json_raw(system, input, name, schema)
            .await
            .and_then(|raw| read(&raw).ok_or(CallFailure::InvalidOutput));
        failure = match answer {
            Ok(indexes) => {
                ask.judged();
                return Ok(indexes);
            }
            Err(failure) => failure,
        };
        if failure.of_what_was_shown() {
            shown.get_or_insert(failure);
        }
        if !failure.retryable() && failure != CallFailure::InvalidOutput {
            break;
        }
        if tried < TRIES && !cfg!(test) {
            tokio::time::sleep(Duration::from_secs(2 * u64::from(tried))).await;
        }
    }
    Err(Unanswered { failure, shown })
}

/// What a line or a stretch comes to, without what they asked not to keep:
/// a fact of it goes with what it would have replaced and where it would
/// have gone. What they ask in it (`now`) holds for it too, and is not
/// itself judged by what they asked before. Whatever they take back goes as
/// it would. Told by `until`, or not at all; whether a fact of it goes
/// though it was not told of it ([`told_apart`]).
pub(super) async fn leave_out_of(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    now: &[Asked],
    updates: &mut ChatMemoryUpdates,
    until: Option<Instant>,
) -> Result<bool, Failure> {
    enum At {
        Fact(usize),
        Said(usize),
        PutOnto(usize),
    }
    let mut at = Vec::new();
    let mut items = Vec::new();
    for (index, update) in updates.updates.iter().enumerate() {
        if let Some(fact) = &update.fact
            && !updates.asked.contains(&index)
        {
            at.push(At::Fact(index));
            items.push(fact.clone());
        }
    }
    for (index, said) in updates.said.iter().enumerate() {
        at.push(At::Said(index));
        items.push(said.said.clone());
    }
    for (index, thing) in updates.put_onto.iter().enumerate() {
        at.push(At::PutOnto(index));
        items.push(thing.clone());
    }
    let by = By {
        who: None,
        now,
        until,
    };
    let told = told_apart(db, user_id, &by, &items).await?;
    let untold = (told.untold.iter()).any(|&index| matches!(at[index], At::Fact(_)));
    let drop = told.out.into_iter().chain(told.untold);
    let (mut said, mut put_onto) = (BTreeSet::new(), BTreeSet::new());
    for index in drop {
        match at[index] {
            // Kept in its place, with nothing in it: the others are known
            // by theirs.
            At::Fact(index) => {
                let update = &mut updates.updates[index];
                update.fact = None;
                update.supersedes.clear();
                update.evidence = None;
                update.concepts.clear();
                update.cues.clear();
                update.topic = None;
            }
            At::Said(index) => {
                said.insert(index);
            }
            At::PutOnto(index) => {
                put_onto.insert(index);
            }
        }
    }
    let mut index = 0..;
    updates
        .said
        .retain(|_| !said.contains(&index.next().unwrap_or(0)));
    let mut index = 0..;
    updates
        .put_onto
        .retain(|_| !put_onto.contains(&index.next().unwrap_or(0)));
    Ok(untold)
}

#[cfg(test)]
#[path = "asked_tests.rs"]
pub(super) mod tests;
