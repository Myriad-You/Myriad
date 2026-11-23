//! The calls she asked a device and is waiting on (contract `calls`).
//!
//! Only she asks, and only a device linked now that was told it is granted
//! the action: one that is not linked is answered `call_unavailable` at
//! once, and nothing is queued for it. `deadlineAt` is on the host's clock;
//! the host counts a call timed out a minute after it. A call sent on a link
//! that went away is sent again, same callId and deadline, once the device
//! links again and was told what it is granted. When the owner takes an
//! action back, its calls never sent end `call_not_granted`, and so does a
//! short look already sent (`calls.seen`), the device linked or not; a long
//! one sent is cancelled, and whatever comes of any other sent one is what
//! happened. One that ends `unknown` is never done again by itself. Every call is one row
//! (`calls_store`); a result that comes after the call was counted timed
//! out changes that row. Nothing of what was asked, what came back or what
//! the device said is kept or logged: an outcome and a code.

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use sea_orm::DatabaseConnection;
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};

use super::calls::Action;
use super::calls_store::{self, Outcome, Record};
use super::connection::{Outgoing, frame};

/// After `deadlineAt` the host waits this much longer before it counts a
/// call timed out (calls).
const GRACE: Duration = Duration::from_secs(60);
/// A call counted timed out is remembered this long, for its result.
const LATE_KEPT: Duration = Duration::from_secs(3600);
/// The codes a failed call may end with (`calls.failCodes`).
const FAIL_CODES: [&str; 10] = [
    "call_unknown",
    "version_unsupported",
    "call_not_granted",
    "wrong_audience",
    "declined",
    "not_now",
    "call_timeout",
    "call_cancelled",
    "call_unavailable",
    "internal_error",
];
/// The kinds of `for` (`callAudiences`).
const AUDIENCES: [&str; 4] = ["owner", "own", "group", "other"];

thread_local! {
    /// This test's own times, in milliseconds: the most a call is given,
    /// the grace after its deadline, and how long a long one waits for
    /// `call.accepted` (each test has its own thread).
    static TIMES_IN_TEST: Cell<Option<(u64, u64, u64)>> = const { Cell::new(None) };
}

#[cfg(test)]
pub(super) fn times_in_test(most_ms: u64, grace_ms: u64, accept_ms: u64) {
    TIMES_IN_TEST.with(|cell| cell.set(Some((most_ms, grace_ms, accept_ms))));
}

fn most_ms(action: &str) -> u64 {
    TIMES_IN_TEST.with(Cell::get).map_or_else(
        || myriad_her_link::call_most_ms(action),
        |(most, _, _)| most,
    )
}

fn grace() -> Duration {
    TIMES_IN_TEST
        .with(Cell::get)
        .map_or(GRACE, |(_, grace, _)| Duration::from_millis(grace))
}

fn accept_within() -> Duration {
    TIMES_IN_TEST.with(Cell::get).map_or_else(
        || Duration::from_millis(super::contract::CONTRACT.timing.call_accept_ms),
        |(_, _, accept)| Duration::from_millis(accept),
    )
}

/// How a call she made came out: its outcome, and for one done, its result.
/// A failure says only its code, never the device's words.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Answer {
    pub outcome: Outcome,
    pub result: Option<Value>,
}

impl Answer {
    pub(crate) fn failed(code: &'static str) -> Self {
        Self {
            outcome: Outcome::Failed(code),
            result: None,
        }
    }
}

struct Pending {
    owner: i32,
    device_id: String,
    action: String,
    version: u64,
    long: bool,
    /// Its `deadlineAt` on the host's clock: past it, it is not sent again.
    deadline_at: i64,
    /// The ask as it is sent, every time.
    body: Value,
    /// The link it was last sent on; `None` while never sent.
    link: Option<u64>,
    accepted: bool,
    cancelled: bool,
    answer: Option<oneshot::Sender<Answer>>,
}

/// The calls waiting on their answer, by callId.
static PENDING: LazyLock<Mutex<HashMap<String, Pending>>> = LazyLock::new(Mutex::default);
/// Calls counted timed out, for a result that comes after all.
static TIMED_OUT: LazyLock<Mutex<HashMap<String, TimedOut>>> = LazyLock::new(Mutex::default);

/// A call counted timed out: its row, from when, and whether that row is
/// written yet. A result that comes before it is, is held in it (`late`)
/// and written right after, so the row ends as what happened.
struct TimedOut {
    record: Record,
    at: Instant,
    kept: bool,
}

fn pending<T>(f: impl FnOnce(&mut HashMap<String, Pending>) -> T) -> T {
    match PENDING.lock() {
        Ok(mut all) => f(&mut all),
        Err(poisoned) => f(&mut poisoned.into_inner()),
    }
}

/// How many of her calls to `device_id` still wait on their answer.
#[cfg(test)]
pub(super) fn waiting_on(device_id: &str) -> usize {
    pending(|all| {
        all.values()
            .filter(|one| one.device_id == device_id)
            .count()
    })
}

fn timed_out<T>(f: impl FnOnce(&mut HashMap<String, TimedOut>) -> T) -> T {
    match TIMED_OUT.lock() {
        Ok(mut all) => f(&mut all),
        Err(poisoned) => f(&mut poisoned.into_inner()),
    }
}

/// `for` as a row keeps it: its kind (one of `callAudiences`, else other)
/// and a group's id.
fn audience(of: &Value) -> (String, Option<String>) {
    let kind = of["kind"]
        .as_str()
        .filter(|kind| AUDIENCES.contains(kind))
        .unwrap_or("other");
    let group = of["groupId"]
        .as_str()
        .map(|group| group.chars().take(128).collect());
    (kind.to_string(), group)
}

/// She asks the owner's device `device_id` to do `action` for `of`
/// (`call.ask`'s `for`) with `args`, and waits for how it came out; `most_ms`
/// gives it less time than the contract's most. The call runs on by itself:
/// a caller that stops waiting (its step timed out, or was cancelled) leaves
/// it to end as calls do, its one row kept and counted on her status page,
/// and a long one is cancelled.
pub(crate) async fn ask(
    db: &DatabaseConnection,
    owner: i32,
    device_id: &str,
    action: &str,
    of: &Value,
    args: Value,
    most_ms: Option<u64>,
) -> Answer {
    let (answer, answered) = oneshot::channel();
    tokio::spawn(run(
        db.clone(),
        owner,
        device_id.to_string(),
        action.to_string(),
        of.clone(),
        args,
        most_ms,
        answer,
    ));
    answered
        .await
        .unwrap_or_else(|_| Answer::failed("internal_error"))
}

#[allow(clippy::too_many_arguments)]
async fn run(
    db: DatabaseConnection,
    owner: i32,
    device_id: String,
    action: String,
    of: Value,
    args: Value,
    most_ms: Option<u64>,
    mut caller: oneshot::Sender<Answer>,
) {
    let call_id = format!("call_{}", uuid::Uuid::new_v4().simple());
    let asked_at = crate::clock::now();
    let spec = myriad_her_link::call_spec(&action);
    let call = Call {
        owner,
        device_id: &device_id,
        call_id: &call_id,
        action: &action,
    };
    let answer = match spec {
        Some(spec) => asking(&call, spec, &of, args, asked_at, most_ms, &mut caller).await,
        None => Answer::failed("call_unknown"),
    };
    let (for_kind, for_group) = audience(&of);
    let record = Record {
        call_id: call_id.clone(),
        owner,
        device_id: device_id.clone(),
        action: action.clone(),
        version: spec.map_or(0, |spec| spec.version),
        for_kind,
        for_group,
        outcome: answer.outcome,
        asked_at,
        ended_at: crate::clock::now(),
        late: false,
    };
    if answer.outcome == Outcome::Failed("call_timeout") {
        keep_timed_out(&db, &record).await;
    } else if let Err(error) = calls_store::record(&db, &record).await {
        tracing::warn!(%error, device = %device_id, action, "[link] her call to a device is not kept");
    }
    let outcome = answer.outcome.name();
    let code = answer.outcome.code().unwrap_or_default();
    crate::services::agent::alive::called(
        super::calls::kind(&action),
        answer.outcome.code().unwrap_or(outcome),
    );
    tracing::info!(device = %device_id, action, outcome, code, "[link] her call to a device ended");
    let _ = caller.send(answer);
}

/// Keeps a call counted timed out: first where a result that still comes
/// finds it, then its row; what came meanwhile is written over it.
pub(super) async fn keep_timed_out(db: &DatabaseConnection, record: &Record) {
    timed_out(|all| {
        all.retain(|_, one| one.at.elapsed() < LATE_KEPT);
        let one = TimedOut {
            record: record.clone(),
            at: Instant::now(),
            kept: false,
        };
        all.insert(record.call_id.clone(), one);
    });
    #[cfg(test)]
    if let Some(row_written) = KEEPING.with(|keeping| keeping.borrow_mut().take()) {
        let _ = row_written.await;
    }
    if let Err(error) = calls_store::record(db, record).await {
        tracing::warn!(%error, device = %record.device_id, action = %record.action, "[link] her call to a device is not kept");
    }
    let came = timed_out(|all| match all.get_mut(&record.call_id) {
        Some(one) if one.record.late => all.remove(&record.call_id).map(|one| one.record),
        Some(one) => {
            one.kept = true;
            None
        }
        None => None,
    });
    if let Some(came) = came
        && let Err(error) = calls_store::record(db, &came).await
    {
        tracing::warn!(%error, device = %record.device_id, "[link] what came of her call after all is not kept");
    }
}

#[cfg(test)]
thread_local! {
    /// A test's hold on the next timed-out row being written: released when
    /// the test says so, as a slow write would be.
    static KEEPING: std::cell::RefCell<Option<oneshot::Receiver<()>>> =
        const { std::cell::RefCell::new(None) };
}

/// The next timed-out row on this thread is written only once `go` is sent.
#[cfg(test)]
pub(super) fn hold_next_timed_out_row() -> oneshot::Sender<()> {
    let (go, row_written) = oneshot::channel();
    KEEPING.with(|keeping| *keeping.borrow_mut() = Some(row_written));
    go
}

/// Whether call `call_id` is counted timed out and its row not written yet.
#[cfg(test)]
pub(super) fn timed_out_unkept(call_id: &str) -> bool {
    timed_out(|all| all.get(call_id).is_some_and(|one| !one.kept))
}

/// Who asks what of which device, under which callId.
struct Call<'a> {
    owner: i32,
    device_id: &'a str,
    call_id: &'a str,
    action: &'a str,
}

async fn asking(
    call: &Call<'_>,
    spec: &myriad_her_link::CallSpec,
    of: &Value,
    args: Value,
    asked_at: chrono::DateTime<chrono::Utc>,
    shorter: Option<u64>,
    caller: &mut oneshot::Sender<Answer>,
) -> Answer {
    let Call {
        owner,
        device_id,
        call_id,
        action,
    } = *call;
    // Whom it is for, before anything is asked.
    if let Err(code) = myriad_her_link::call_allowed(action, of) {
        return Answer::failed(code);
    }
    let most = shorter.map_or_else(|| most_ms(action), |shorter| shorter.min(most_ms(action)));
    let deadline_at = asked_at.timestamp_millis() + most as i64;
    let body = json!({
        "callId": call_id,
        "action": action,
        "version": spec.version,
        "args": args,
        "for": of,
        "deadlineAt": deadline_at,
    });
    let (answer, answered) = oneshot::channel();
    pending(|all| {
        all.insert(
            call_id.to_string(),
            Pending {
                owner,
                device_id: device_id.to_string(),
                action: action.to_string(),
                version: spec.version,
                long: spec.long,
                deadline_at,
                body: body.clone(),
                link: None,
                accepted: false,
                cancelled: false,
                answer: Some(answer),
            },
        )
    });
    let ask = Outgoing::reliable("call.ask", call_id.to_string(), body);
    if let Err(code) = super::send_call(owner, device_id, call_id, action, ask) {
        pending(|all| all.remove(call_id));
        return Answer::failed(code);
    }
    let now = tokio::time::Instant::now();
    let deadline = now + Duration::from_millis(most) + grace();
    let accept_by = now + accept_within();
    let mut waits_accepted = spec.long;
    let mut caller_here = spec.long;
    tokio::pin!(answered);
    loop {
        tokio::select! {
            answer = &mut answered => return answer.unwrap_or_else(|_| Answer::failed("internal_error")),
            // No one waits for a long one any more: it is cancelled, and what
            // comes of it is still kept.
            () = caller.closed(), if caller_here => {
                caller_here = false;
                let cancel_now = pending(|all| {
                    all.get_mut(call_id)
                        .filter(|one| !one.cancelled)
                        .map(|one| one.cancelled = true)
                        .is_some()
                });
                if cancel_now {
                    super::send_calls_frame(device_id, cancel(call_id));
                }
            }
            () = tokio::time::sleep_until(accept_by), if waits_accepted => {
                waits_accepted = false;
                let unaccepted = pending(|all| {
                    let accepted = all.get(call_id).is_none_or(|one| one.accepted);
                    if accepted { None } else { all.remove(call_id) }
                });
                if unaccepted.is_some() {
                    super::send_calls_frame(device_id, cancel(call_id));
                    return Answer::failed("call_timeout");
                }
            }
            () = tokio::time::sleep_until(deadline) => {
                if pending(|all| all.remove(call_id)).is_some() {
                    return Answer::failed("call_timeout");
                }
                // Its answer came just now.
                return answered.await.unwrap_or_else(|_| Answer::failed("internal_error"));
            }
        }
    }
}

fn cancel(call_id: &str) -> Outgoing {
    frame("call.cancel", json!({ "callId": call_id }))
}

/// The call was sent on link `link` (under the registry's lock, so a
/// withdrawal never takes it for one never sent).
pub(super) fn sent(call_id: &str, link: u64) {
    pending(|all| {
        if let Some(one) = all.get_mut(call_id) {
            one.link = Some(link);
        }
    });
}

/// How a `call.result` says the call came out. One that names another
/// action or version than was asked is the receiver's own failure; so is a
/// look at the screen not of the shape `screenFields` lets it have, and one
/// that is, keeps only what it lets it say.
fn answer_of(action: &str, version: u64, body: &Value) -> Answer {
    if body["action"].as_str() != Some(action) || body["version"].as_u64() != Some(version) {
        return Answer::failed("internal_error");
    }
    match body["outcome"].as_str() {
        Some("done") => {
            let result = body.get("result").cloned().unwrap_or_else(|| json!({}));
            let result = if action == "screen.look" {
                match super::senses::look_result(&result) {
                    Some(kept) => kept,
                    None => return Answer::failed("internal_error"),
                }
            } else {
                result
            };
            Answer {
                outcome: Outcome::Done,
                result: Some(result),
            }
        }
        Some("failed") => {
            let given = body["error"]["code"].as_str();
            let code = FAIL_CODES
                .iter()
                .copied()
                .find(|code| Some(*code) == given)
                .unwrap_or("internal_error");
            Answer::failed(code)
        }
        _ => Answer {
            outcome: Outcome::Unknown,
            result: None,
        },
    }
}

/// Whether a result says what a device saw (contract `calls.seen`: the
/// screen, a picture, a recording).
fn saw(action: &str, body: &Value) -> bool {
    body["outcome"].as_str() == Some("done") && myriad_her_link::call_seen(action)
}

/// What a result that came on the device's link comes to, with what that
/// link is granted now: what a device saw of an action taken back is
/// `call_not_granted`, and what it carries is dropped unread; any other is
/// read as it says.
fn read(device_id: &str, granted: &[Action], action: &str, version: u64, body: &Value) -> Answer {
    let grants = || {
        granted
            .iter()
            .any(|one| one.name == action && one.version == version)
    };
    if saw(action, body) && !grants() {
        tracing::info!(device = %device_id, %action, "[link] what a device saw came after the owner took it back: not kept");
        return Answer::failed("call_not_granted");
    }
    answer_of(action, version, body)
}

/// What became of a `call.result`.
enum Came {
    /// What the device saw, on a link that is not its link any more: what it
    /// is granted there says nothing of what it is granted now. Left as if
    /// it had not come: it comes again on the device's next link, once that
    /// is told what it is granted (contract `calls.seen`).
    Left,
    /// The answer of a call that waited.
    Answered(Option<oneshot::Sender<Answer>>, Answer),
    /// What came of one counted timed out; its row, when that is to be
    /// written now.
    Late(Option<Record>),
    /// Of no call that waits.
    Nothing,
}

/// A `call.result` for `call_id` from the device, on a link `granted` so much
/// now, or (`None`) that is not its link any more (under the registry's lock,
/// so what is read is of what is granted as it is taken).
fn came(device_id: &str, call_id: &str, granted: Option<&[Action]>, body: &Value) -> Came {
    let left = |action: &str| granted.is_none() && saw(action, body);
    let granted = granted.unwrap_or_default();
    let waiting = pending(|all| {
        let one = all.get(call_id).filter(|one| one.device_id == device_id)?;
        if left(&one.action) {
            return Some(Came::Left);
        }
        let mut one = all.remove(call_id)?;
        let answer = read(device_id, granted, &one.action, one.version, body);
        Some(Came::Answered(one.answer.take(), answer))
    });
    if let Some(waiting) = waiting {
        return waiting;
    }
    // Its timed-out row not written yet: held for when it is.
    timed_out(|all| {
        let Some(one) = all
            .get_mut(call_id)
            .filter(|one| one.record.device_id == device_id && !one.record.late)
        else {
            return Came::Nothing;
        };
        if left(&one.record.action) {
            return Came::Left;
        }
        let answer = read(
            device_id,
            granted,
            &one.record.action,
            one.record.version,
            body,
        );
        one.record.outcome = answer.outcome;
        one.record.ended_at = crate::clock::now();
        one.record.late = true;
        tracing::info!(device = %device_id, action = %one.record.action, outcome = answer.outcome.name(), "[link] her call to a device came back after it was counted timed out");
        Came::Late(if one.kept {
            all.remove(call_id).map(|one| one.record)
        } else {
            None
        })
    })
}

/// A device answered one of her calls (`call.result`) on its link `link`.
/// Only the device it was asked of answers it; one that comes after the call
/// was counted timed out changes its row to what happened. `false` when it
/// was left unread ([`Came::Left`]): it is not to be acked either, so it
/// comes again.
pub(super) async fn result(
    db: &DatabaseConnection,
    device_id: &str,
    link: u64,
    body: &Value,
) -> bool {
    let Some(call_id) = body["callId"].as_str() else {
        return true;
    };
    let came = super::granted_on(device_id, link, |granted| {
        came(device_id, call_id, granted, body)
    });
    match came {
        Came::Left => {
            tracing::info!(device = %device_id, "[link] what a device saw came on a link it has left: waits for its next");
            return false;
        }
        Came::Answered(waits, answer) => {
            if let Some(waits) = waits {
                let _ = waits.send(answer);
            }
        }
        Came::Late(Some(record)) => {
            if let Err(error) = calls_store::record(db, &record).await {
                tracing::warn!(%error, device = %device_id, "[link] what came of her call after all is not kept");
            }
        }
        Came::Late(None) | Came::Nothing => {}
    }
    true
}

/// A device has one of her long calls (`call.accepted`).
pub(super) fn accepted(device_id: &str, body: &Value) {
    let Some(call_id) = body["callId"].as_str() else {
        return;
    };
    pending(|all| {
        if let Some(one) = all
            .get_mut(call_id)
            .filter(|one| one.device_id == device_id)
        {
            one.accepted = true;
        }
    });
}

/// Link `link` of the device was just told it is granted `granted`, through
/// `sender` (under the registry's lock, right after `call.granted`). Its
/// calls of an action not granted now and never sent end `call_not_granted`;
/// so does a short one sent of a seen action (`calls.seen`), whose result
/// would not be kept now (`read`); a long one sent is cancelled. Those
/// sent on an earlier link and granted now go again, same callId and
/// deadline, unless their deadline has passed.
pub(super) fn regranted(
    device_id: &str,
    link: u64,
    granted: &[Action],
    sender: &mpsc::Sender<Outgoing>,
) {
    let mut dropped = Vec::new();
    let now_ms = crate::clock::now().timestamp_millis();
    pending(|all| {
        all.retain(|call_id, one| {
            if one.device_id != device_id {
                return true;
            }
            if !granted.iter().any(|action| action.name == one.action) {
                if one.link.is_none() || (!one.long && myriad_her_link::call_seen(&one.action)) {
                    dropped.extend(one.answer.take());
                    return false;
                }
                if one.long && !one.cancelled && sender.try_send(cancel(call_id)).is_ok() {
                    one.cancelled = true;
                }
                return true;
            }
            if one.link.is_some_and(|sent_on| sent_on != link) && now_ms <= one.deadline_at {
                let again = Outgoing::reliable("call.ask", call_id.clone(), one.body.clone());
                if sender.try_send(again).is_ok() {
                    one.link = Some(link);
                }
            }
            true
        })
    });
    for waits in dropped {
        let _ = waits.send(Answer::failed("call_not_granted"));
    }
}

/// The owner takes `group` back for their device, linked or not (under the
/// registry's lock): its short looks already sent end now, `call_not_granted`.
/// What is granted later is not for what was asked before, whether the
/// device was away meanwhile or it is granted again at once.
pub(super) fn withdrawn(owner: i32, device_id: &str, group: &str) {
    let mut ended = Vec::new();
    pending(|all| {
        all.retain(|_, one| {
            let ends = one.owner == owner
                && one.device_id == device_id
                && one.link.is_some()
                && !one.long
                && myriad_her_link::call_seen(&one.action)
                && myriad_her_link::call_spec(&one.action).is_some_and(|spec| spec.group == group);
            if ends {
                ended.extend(one.answer.take());
            }
            !ends
        });
    });
    for waits in ended {
        let _ = waits.send(Answer::failed("call_not_granted"));
    }
}

/// The device is not one any more (under the registry's lock, as its link
/// is let go): every call of hers to it ends now, its row kept. One never
/// sent was not granted; one sent may have happened (unknown), and a result
/// that still comes is not read.
pub(super) fn forget_device(device_id: &str) {
    let ended: Vec<_> = pending(|all| {
        let ours: Vec<String> = all
            .iter()
            .filter(|(_, one)| one.device_id == device_id)
            .map(|(call_id, _)| call_id.clone())
            .collect();
        ours.iter()
            .filter_map(|call_id| all.remove(call_id))
            .filter_map(|mut one| {
                let sent = one.link.is_some();
                one.answer.take().map(|waits| (waits, sent))
            })
            .collect()
    });
    for (waits, sent) in ended {
        let answer = if sent {
            Answer {
                outcome: Outcome::Unknown,
                result: None,
            }
        } else {
            Answer::failed("call_not_granted")
        };
        let _ = waits.send(answer);
    }
}

/// When the rows of her calls were last let go of.
static PRUNED_AT: Mutex<Option<Instant>> = Mutex::new(None);

/// Lets go of the rows of her calls past what is kept, at most hourly.
pub(crate) async fn sweep(db: &DatabaseConnection) {
    let due = PRUNED_AT.lock().is_ok_and(|mut at| {
        let due = at.is_none_or(|at| at.elapsed() >= Duration::from_secs(3600));
        if due {
            *at = Some(Instant::now());
        }
        due
    });
    if !due {
        return;
    }
    match calls_store::prune(db, crate::clock::now()).await {
        Ok(0) => {}
        Ok(gone) => tracing::info!(gone, "[link] rows of her calls past 30 days let go"),
        Err(error) => tracing::warn!(%error, "[link] rows of her calls could not be let go of"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fail_codes_and_audiences_are_the_contracts() {
        let contract: Value = serde_json::from_str(super::super::contract::CONTRACT_JSON).unwrap();
        assert_eq!(contract["calls"]["failCodes"], json!(FAIL_CODES));
        assert_eq!(contract["callAudiences"], json!(AUDIENCES));
    }

    fn said(body: Value) -> Value {
        let mut said = json!({ "callId": "c", "action": "screen.look", "version": 1 });
        said.as_object_mut()
            .unwrap()
            .extend(body.as_object().unwrap().clone());
        said
    }

    #[test]
    fn a_result_says_only_an_outcome_and_a_code_and_a_look_only_what_it_may() {
        let failed = answer_of(
            "screen.look",
            1,
            &said(json!({ "outcome": "failed", "error": { "code": "not_now" } })),
        );
        assert_eq!(failed, Answer::failed("not_now"));
        // A code the contract does not name is the receiver's own failure.
        let odd = answer_of(
            "screen.look",
            1,
            &said(
                json!({ "outcome": "failed", "error": { "code": "a long story of what went wrong" } }),
            ),
        );
        assert_eq!(odd, Answer::failed("internal_error"));
        assert_eq!(
            answer_of("screen.look", 1, &said(json!({ "outcome": "unknown" }))).outcome,
            Outcome::Unknown
        );
        let look = answer_of(
            "screen.look",
            1,
            &said(json!({ "outcome": "done", "result": {
                "facts": { "app": "WeChat", "who": "Ann", "screen_text": "what Ann wrote", "clipboard": "x" },
                "summary": "Talking with Ann about dinner\nAnn: see you at 7",
            }})),
        );
        assert_eq!(
            look.result,
            Some(json!({
                "facts": { "app": "WeChat", "who": "Ann" },
                "summary": "Talking with Ann about dinner",
            }))
        );
        let keys = super::super::counts::screen_keys();
        assert!(
            keys.iter()
                .any(|(t, key, _)| *t == "call.result" && key == "clipboard")
        );
    }

    #[test]
    fn a_result_of_another_action_or_version_or_shape_is_not_read() {
        // Another version than was asked, or another action.
        let other_version = json!({ "callId": "c", "action": "screen.look", "version": 2,
                                    "outcome": "done", "result": { "facts": { "app": "Mail" } } });
        assert_eq!(
            answer_of("screen.look", 1, &other_version),
            Answer::failed("internal_error")
        );
        let other_action = json!({ "callId": "c", "action": "voice.speak", "version": 1,
                                   "outcome": "done", "result": { "delivered": true } });
        assert_eq!(
            answer_of("screen.look", 1, &other_action),
            Answer::failed("internal_error")
        );
        // Facts that are not an object: nothing of it is read.
        let text_facts = said(json!({ "outcome": "done",
            "result": { "facts": "Ann: see you at 7", "summary": "Mail" } }));
        assert_eq!(
            answer_of("screen.look", 1, &text_facts),
            Answer::failed("internal_error")
        );
        // Keys beside facts and summary, values that are not plain, and a
        // summary's further lines are left out and counted.
        let wide = said(json!({ "outcome": "done", "result": {
            "facts": { "app": "Mail", "topic": ["Ann", "see you at 7"], "private": false },
            "summary": "Mail\nAnn: see you at 7",
            "screen_text": "Ann: see you at 7",
        }}));
        assert_eq!(
            answer_of("screen.look", 1, &wide).result,
            Some(json!({ "facts": { "app": "Mail", "private": false }, "summary": "Mail" }))
        );
        let keys = super::super::counts::screen_keys();
        for key in ["screen_text", "topic", "summary"] {
            assert!(
                keys.iter()
                    .any(|(t, one, _)| *t == "call.result" && one == key),
                "{key}"
            );
        }
    }

    fn waiting(call_id: &str, device_id: &str, link: Option<u64>) -> oneshot::Receiver<Answer> {
        let (answer, answered) = oneshot::channel();
        pending(|all| {
            all.insert(
                call_id.into(),
                Pending {
                    owner: 1,
                    device_id: device_id.into(),
                    action: "screen.look".into(),
                    version: 1,
                    long: false,
                    deadline_at: crate::clock::now().timestamp_millis() + 60_000,
                    body: json!({}),
                    link,
                    accepted: false,
                    cancelled: false,
                    answer: Some(answer),
                },
            )
        });
        answered
    }

    #[tokio::test]
    async fn a_call_never_sent_ends_not_granted_and_a_removed_devices_calls_end_at_once() {
        let (sender, mut sent) = mpsc::channel(4);
        let never = waiting("call_never_sent", "dev_withdrawn", None);
        let look = waiting("call_sent_look", "dev_withdrawn", Some(7));
        let sent_one = waiting("call_sent_short", "dev_withdrawn", Some(7));
        pending(|all| {
            if let Some(one) = all.get_mut("call_sent_short") {
                one.action = "notify.post".into();
            }
        });
        regranted("dev_withdrawn", 7, &[], &sender);
        assert_eq!(never.await.unwrap(), Answer::failed("call_not_granted"));
        // A look sent ends not granted: what it saw would not be kept now.
        assert_eq!(look.await.unwrap(), Answer::failed("call_not_granted"));
        // Any other one sent is not dropped, and a short one is not cancelled.
        assert!(pending(|all| all.contains_key("call_sent_short")));
        assert!(sent.try_recv().is_err());
        pending(|all| all.remove("call_sent_short"));
        drop(sent_one);

        // A call sent on an earlier link whose deadline passed is not sent again.
        let (answer, _answered) = oneshot::channel();
        pending(|all| {
            all.insert(
                "call_expired".into(),
                Pending {
                    owner: 1,
                    device_id: "dev_expired".into(),
                    action: "screen.look".into(),
                    version: 1,
                    long: false,
                    deadline_at: crate::clock::now().timestamp_millis() - 1,
                    body: json!({}),
                    link: Some(1),
                    accepted: false,
                    cancelled: false,
                    answer: Some(answer),
                },
            )
        });
        let granted = [super::super::calls::Action {
            name: "screen.look".into(),
            version: 1,
        }];
        regranted("dev_expired", 2, &granted, &sender);
        assert!(sent.try_recv().is_err());
        pending(|all| all.remove("call_expired"));

        // A removed device: nothing of it waits on.
        let never = waiting("call_never_sent_2", "dev_removed", None);
        let sent_one = waiting("call_sent_2", "dev_removed", Some(3));
        forget_device("dev_removed");
        assert_eq!(never.await.unwrap(), Answer::failed("call_not_granted"));
        assert_eq!(sent_one.await.unwrap().outcome, Outcome::Unknown);
        assert!(pending(|all| !all
            .values()
            .any(|one| one.device_id == "dev_removed")));
    }

    fn granting(name: &str) -> [Action; 1] {
        [Action {
            name: name.into(),
            version: 1,
        }]
    }

    #[test]
    fn what_a_device_saw_is_kept_only_while_it_is_granted_there() {
        let done = said(json!({ "outcome": "done", "result": { "facts": { "app": "Mail" } } }));
        let of_look = |granted: &[Action]| read("dev_look", granted, "screen.look", 1, &done);
        assert_eq!(of_look(&[]), Answer::failed("call_not_granted"));
        assert_eq!(
            of_look(&granting("notify.post")),
            Answer::failed("call_not_granted")
        );
        assert_eq!(
            of_look(&granting("screen.look")).result,
            Some(json!({ "facts": { "app": "Mail" } }))
        );
        // What happened is read as it says: a notification shown, or a look
        // that did not see anything.
        let shown = json!({ "action": "notify.post", "version": 1, "outcome": "done", "result": { "shown": true } });
        assert_eq!(
            read("dev_look", &[], "notify.post", 1, &shown).outcome,
            Outcome::Done
        );
        let failed = said(json!({ "outcome": "failed", "error": { "code": "not_now" } }));
        assert_eq!(
            read("dev_look", &[], "screen.look", 1, &failed),
            Answer::failed("not_now")
        );
    }

    #[tokio::test]
    async fn what_a_device_saw_on_a_link_it_left_waits_for_its_next() {
        let done = said(json!({ "outcome": "done", "result": { "facts": { "app": "Mail" } } }));
        let mut look = waiting("call_left", "dev_left", Some(1));
        // Not its link any more: not taken, so not answered wrongly either.
        assert!(matches!(
            came("dev_left", "call_left", None, &done),
            Came::Left
        ));
        assert!(look.try_recv().is_err());
        assert_eq!(waiting_on("dev_left"), 1);
        // Not this device's: nothing of it.
        assert!(matches!(
            came("dev_other", "call_left", None, &done),
            Came::Nothing
        ));
        // On its next link, granted there: read.
        let next = granting("screen.look");
        let Came::Answered(_, answer) = came("dev_left", "call_left", Some(&next), &done) else {
            panic!("it is answered");
        };
        assert_eq!(answer.outcome, Outcome::Done);
        // What did not see anything is what happened, whichever link it came on.
        let _declined = waiting("call_left_2", "dev_left", Some(1));
        let failed = said(json!({ "outcome": "failed", "error": { "code": "declined" } }));
        assert!(matches!(
            came("dev_left", "call_left_2", None, &failed),
            Came::Answered(_, answer) if answer == Answer::failed("declined")
        ));
        assert_eq!(waiting_on("dev_left"), 0);
    }

    #[tokio::test]
    async fn a_group_taken_back_ends_its_short_looks_sent_and_nothing_else() {
        let look = waiting("call_taken_back", "dev_away", Some(1));
        let others = [
            ("call_never_sent_3", "dev_away", None),
            ("call_other_device", "dev_here", Some(1)),
        ]
        .map(|(id, device, link)| waiting(id, device, link));
        let notice = waiting("call_notice", "dev_away", Some(1));
        let theirs = waiting("call_theirs", "dev_away", Some(1));
        pending(|all| {
            all.get_mut("call_notice").unwrap().action = "notify.post".into();
            all.get_mut("call_theirs").unwrap().owner = 2;
        });
        // Its link gone or not, and whatever is granted next.
        withdrawn(1, "dev_away", "notify");
        assert!(pending(|all| all.contains_key("call_taken_back")));
        withdrawn(1, "dev_away", "screen");
        assert_eq!(look.await.unwrap(), Answer::failed("call_not_granted"));
        for id in [
            "call_never_sent_3",
            "call_other_device",
            "call_notice",
            "call_theirs",
        ] {
            assert!(pending(|all| all.remove(id).is_some()), "{id}");
        }
        drop((others, notice, theirs));
    }

    #[test]
    fn whom_a_call_was_for_is_kept_as_one_of_the_contracts_kinds() {
        assert_eq!(
            audience(&json!({ "kind": "owner" })),
            ("owner".into(), None)
        );
        assert_eq!(
            audience(&json!({ "kind": "group", "groupId": "telegram:-100" })),
            ("group".into(), Some("telegram:-100".into()))
        );
        assert_eq!(
            audience(&json!({ "kind": "everyone" })),
            ("other".into(), None)
        );
    }
}
