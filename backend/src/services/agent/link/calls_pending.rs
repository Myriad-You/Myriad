//! The calls she asked a device and is waiting on (contract `calls`).
//!
//! Only she asks, and only a device linked now that was told it is granted
//! the action: one that is not linked is answered `call_unavailable` at
//! once, and nothing is queued for it. `deadlineAt` is on the host's clock;
//! the host counts a call timed out a minute after it. A call sent on a link
//! that went away is sent again, same callId and deadline, once the device
//! links again and was told what it is granted. When the owner takes an
//! action back, its calls never sent end `call_not_granted`; a long one sent
//! is cancelled, and whatever comes of a sent one is what happened. One that
//! ends `unknown` is never done again by itself. Every call is one row
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
    device_id: String,
    action: String,
    long: bool,
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
static TIMED_OUT: LazyLock<Mutex<HashMap<String, (Record, Instant)>>> =
    LazyLock::new(Mutex::default);

fn pending<T>(f: impl FnOnce(&mut HashMap<String, Pending>) -> T) -> T {
    match PENDING.lock() {
        Ok(mut all) => f(&mut all),
        Err(poisoned) => f(&mut poisoned.into_inner()),
    }
}

fn timed_out<T>(f: impl FnOnce(&mut HashMap<String, (Record, Instant)>) -> T) -> T {
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
/// (`call.ask`'s `for`) with `args`, and waits for how it came out. The one
/// row of the call is kept, and it is counted on her status page.
pub(crate) async fn ask(
    db: &DatabaseConnection,
    owner: i32,
    device_id: &str,
    action: &str,
    of: &Value,
    args: Value,
) -> Answer {
    let call_id = format!("call_{}", uuid::Uuid::new_v4().simple());
    let asked_at = crate::clock::now();
    let spec = myriad_her_link::call_spec(action);
    let answer = match spec {
        Some(spec) => asking(owner, device_id, &call_id, action, spec, of, args, asked_at).await,
        None => Answer::failed("call_unknown"),
    };
    let (for_kind, for_group) = audience(of);
    let record = Record {
        call_id,
        owner,
        device_id: device_id.to_string(),
        action: action.to_string(),
        version: spec.map_or(0, |spec| spec.version),
        for_kind,
        for_group,
        outcome: answer.outcome,
        asked_at,
        ended_at: crate::clock::now(),
        late: false,
    };
    if let Err(error) = calls_store::record(db, &record).await {
        tracing::warn!(%error, device = %device_id, action, "[link] her call to a device is not kept");
    }
    if answer.outcome == Outcome::Failed("call_timeout") {
        timed_out(|all| {
            all.retain(|_, (_, at)| at.elapsed() < LATE_KEPT);
            all.insert(record.call_id.clone(), (record, Instant::now()));
        });
    }
    let outcome = answer.outcome.name();
    let code = answer.outcome.code().unwrap_or_default();
    crate::services::agent::alive::called(
        super::calls::kind(action),
        answer.outcome.code().unwrap_or(outcome),
    );
    tracing::info!(device = %device_id, action, outcome, code, "[link] her call to a device ended");
    answer
}

#[allow(clippy::too_many_arguments)]
async fn asking(
    owner: i32,
    device_id: &str,
    call_id: &str,
    action: &str,
    spec: &myriad_her_link::CallSpec,
    of: &Value,
    args: Value,
    asked_at: chrono::DateTime<chrono::Utc>,
) -> Answer {
    // Whom it is for, before anything is asked.
    if let Err(code) = myriad_her_link::call_allowed(action, of) {
        return Answer::failed(code);
    }
    let most = most_ms(action);
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
                device_id: device_id.to_string(),
                action: action.to_string(),
                long: spec.long,
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
    tokio::pin!(answered);
    loop {
        tokio::select! {
            answer = &mut answered => return answer.unwrap_or_else(|_| Answer::failed("internal_error")),
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

/// How a `call.result` says the call came out, the result of a look at the
/// screen kept to what `screenFields` lets it say.
fn answer_of(action: &str, body: &Value) -> Answer {
    match body["outcome"].as_str() {
        Some("done") => {
            let mut result = body.get("result").cloned().unwrap_or_else(|| json!({}));
            if action == "screen.look" {
                super::senses::keep_look("call.result", "facts", &mut result);
            }
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

/// A device answered one of her calls (`call.result`). Only the device it
/// was asked of answers it; one that comes after the call was counted timed
/// out changes its row to what happened.
pub(super) async fn result(db: &DatabaseConnection, device_id: &str, body: &Value) {
    let Some(call_id) = body["callId"].as_str() else {
        return;
    };
    let waiting = pending(|all| {
        let ours = all
            .get(call_id)
            .is_some_and(|one| one.device_id == device_id);
        if ours { all.remove(call_id) } else { None }
    });
    if let Some(mut one) = waiting {
        let answer = answer_of(&one.action, body);
        if let Some(waits) = one.answer.take() {
            let _ = waits.send(answer);
        }
        return;
    }
    let late = timed_out(|all| {
        let ours = all
            .get(call_id)
            .is_some_and(|(record, _)| record.device_id == device_id);
        if ours { all.remove(call_id) } else { None }
    });
    let Some((mut record, _)) = late else {
        return;
    };
    let answer = answer_of(&record.action, body);
    record.outcome = answer.outcome;
    record.ended_at = crate::clock::now();
    record.late = true;
    tracing::info!(device = %device_id, action = %record.action, outcome = answer.outcome.name(), "[link] her call to a device came back after it was counted timed out");
    if let Err(error) = calls_store::record(db, &record).await {
        tracing::warn!(%error, device = %device_id, "[link] what came of her call after all is not kept");
    }
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
/// a long one sent is cancelled. Those sent on an earlier link and granted
/// now go again, same callId and deadline.
pub(super) fn regranted(
    device_id: &str,
    link: u64,
    granted: &[super::calls::Action],
    sender: &mpsc::Sender<Outgoing>,
) {
    let mut dropped = Vec::new();
    pending(|all| {
        all.retain(|call_id, one| {
            if one.device_id != device_id {
                return true;
            }
            if !granted.iter().any(|action| action.name == one.action) {
                if one.link.is_none() {
                    dropped.extend(one.answer.take());
                    return false;
                }
                if one.long && !one.cancelled && sender.try_send(cancel(call_id)).is_ok() {
                    one.cancelled = true;
                }
                return true;
            }
            if one.link.is_some_and(|sent_on| sent_on != link) {
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

/// The device is not one any more: its calls never sent end not granted;
/// one sent ends by its deadline.
pub(super) fn forget_device(device_id: &str) {
    let dropped: Vec<_> = pending(|all| {
        let never_sent: Vec<String> = all
            .iter()
            .filter(|(_, one)| one.device_id == device_id && one.link.is_none())
            .map(|(call_id, _)| call_id.clone())
            .collect();
        never_sent
            .iter()
            .filter_map(|call_id| all.remove(call_id))
            .filter_map(|mut one| one.answer.take())
            .collect()
    });
    for waits in dropped {
        let _ = waits.send(Answer::failed("call_not_granted"));
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

    #[test]
    fn a_result_says_only_an_outcome_and_a_code_and_a_look_only_what_it_may() {
        let failed = answer_of(
            "screen.look",
            &json!({ "callId": "c", "action": "screen.look", "version": 1,
                     "outcome": "failed", "error": { "code": "not_now" } }),
        );
        assert_eq!(failed, Answer::failed("not_now"));
        // A code the contract does not name is the receiver's own failure.
        let odd = answer_of(
            "screen.look",
            &json!({ "outcome": "failed", "error": { "code": "a long story of what went wrong" } }),
        );
        assert_eq!(odd, Answer::failed("internal_error"));
        assert_eq!(
            answer_of("screen.look", &json!({ "outcome": "unknown" })).outcome,
            Outcome::Unknown
        );
        let look = answer_of(
            "screen.look",
            &json!({ "outcome": "done", "result": {
                "facts": { "app": "WeChat", "who": "Ann", "screen_text": "what Ann wrote", "clipboard": "x" },
                "summary": "Talking with Ann about dinner\nAnn: see you at 7",
            }}),
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

    fn waiting(call_id: &str, device_id: &str, link: Option<u64>) -> oneshot::Receiver<Answer> {
        let (answer, answered) = oneshot::channel();
        pending(|all| {
            all.insert(
                call_id.into(),
                Pending {
                    device_id: device_id.into(),
                    action: "screen.look".into(),
                    long: false,
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
    async fn a_call_never_sent_ends_not_granted_when_its_action_or_device_goes() {
        let (sender, mut sent) = mpsc::channel(4);
        let never = waiting("call_never_sent", "dev_withdrawn", None);
        let sent_one = waiting("call_sent_short", "dev_withdrawn", Some(7));
        regranted("dev_withdrawn", 7, &[], &sender);
        assert_eq!(never.await.unwrap(), Answer::failed("call_not_granted"));
        // One sent is not dropped, and a short one is not cancelled.
        assert!(pending(|all| all.contains_key("call_sent_short")));
        assert!(sent.try_recv().is_err());
        pending(|all| all.remove("call_sent_short"));
        drop(sent_one);

        let never = waiting("call_never_sent_2", "dev_removed", None);
        let _sent_one = waiting("call_sent_2", "dev_removed", Some(3));
        forget_device("dev_removed");
        assert_eq!(never.await.unwrap(), Answer::failed("call_not_granted"));
        assert!(pending(|all| all.remove("call_sent_2")).is_some());
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
