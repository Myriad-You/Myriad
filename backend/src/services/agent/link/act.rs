//! Her host asking a Mac to act: `act.request` down, `act.result` up, and
//! `act.cancel` (contract `act`).
//!
//! Only a child that said it takes `act` is asked, and a Mac says so only
//! while its owner has switched on, there, letting the site ask it to act;
//! turning that off there stops the run. What is asked is one Work task: the
//! instruction of one of her proposals for that Mac (place `mac`), accepted
//! on the site or by her own autonomy. It runs there through the Mac's own
//! Work, under the Mac's own grants, which the host can only narrow (`cap`);
//! never a single computer-use step, never a scheduled task. A proposal the
//! owner accepts on the Mac itself runs there without being asked
//! (`proposals`).
//!
//! Requests live here, in memory, beside the link: kept for the device until
//! it acks, sent again when it links again, and closed by its result or a
//! time limit (`actPickupMs` to start, `actMaxRunMs` and a minute to end). A
//! device removed meanwhile fails its request. The proposal follows: running
//! while asked, accepted again when refused or not picked up, and completed,
//! failed or closed when the run ends. What the run did comes up as an
//! experience (`started: host`), as any Work there does.
//!
//! A Mac that lists `calls` and was granted a whole run (`work.run`, the
//! owner's `work` group) is asked it as a call instead (`calls_pending`):
//! taken within `callAcceptMs`, given `longCallMaxMs` with the owner's
//! confirming, cancelled when the proposal is dismissed. Its result says only
//! how the run ended; one the contract says was not done takes the proposal
//! back to accepted, anything else ends it, and one that may have happened is
//! never asked again by itself.

use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use sea_orm::DatabaseConnection;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::services::agent::consciousness::{
    AcceptSource, IntentRecord, IntentStatus, IntentStore,
};

use super::calls_pending::Answer;
use super::calls_store::Outcome;
use super::connection::Outgoing;
use super::contract::{CONTRACT, valid_id};

/// After `actMaxRunMs` the Mac stops the run itself; the host waits this much
/// longer for its result before it counts the run timed out.
const GRACE: Duration = Duration::from_secs(60);
/// What of the run's last words the proposal keeps.
const SUMMARY_CHARS: usize = 160;

/// A Mac of the owner's whose link takes requests now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Mac {
    pub device_id: String,
    pub name: String,
}

#[derive(Debug, Clone)]
struct Act {
    owner: i32,
    device_id: String,
    proposal_id: String,
    body: Value,
    asked_at: Instant,
    started_at: Option<Instant>,
    acked: bool,
    revoked: bool,
}

/// Open requests, by act id.
static ACTS: LazyLock<Mutex<HashMap<String, Act>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
/// Devices that refused a request or did not pick one up: not asked again
/// until they link again.
static RESTING: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

/// A whole run asked of a Mac as a call (`calls`).
const RUN: &str = "work.run";

/// A proposal running on a Mac as a call: the Mac, and what stops waiting
/// for it (which cancels it there).
struct Run {
    owner: i32,
    device_id: String,
    stop: Option<tokio::task::AbortHandle>,
}

/// Proposals running on a Mac as a call, by proposal id.
static RUNS: LazyLock<Mutex<HashMap<String, Run>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// Whether the Mac runs one of her proposals now, either way.
fn busy(acts: &HashMap<String, Act>, device_id: &str) -> bool {
    acts.values().any(|act| act.device_id == device_id)
        || RUNS
            .lock()
            .is_ok_and(|runs| runs.values().any(|run| run.device_id == device_id))
}

fn pickup() -> Duration {
    Duration::from_millis(CONTRACT.timing.act_pickup_ms)
}

fn max_run() -> Duration {
    Duration::from_millis(CONTRACT.timing.act_max_run_ms)
}

/// The owner's Macs that take requests now, or a whole run as a call, by
/// name.
pub(crate) fn macs(owner: i32) -> Vec<Mac> {
    let mut macs: Vec<Mac> = super::act_links(owner)
        .into_iter()
        .map(|(device_id, name, _)| Mac { device_id, name })
        .collect();
    for (device_id, name) in super::granted_named(owner, RUN) {
        if !macs.iter().any(|mac| mac.device_id == device_id) {
            macs.push(Mac { device_id, name });
        }
    }
    macs.sort_by(|a, b| a.name.cmp(&b.name).then(a.device_id.cmp(&b.device_id)));
    macs
}

/// The Mac a new proposal for a Mac is for, by the one rule for picking an
/// end ([`super::pick::device`]): the one the owner is using her on now,
/// else the one they used last, else the first by name.
pub(crate) fn mac_for_work(owner: i32) -> Option<Mac> {
    let macs = macs(owner);
    let ids: Vec<String> = macs.iter().map(|mac| mac.device_id.clone()).collect();
    let picked = super::pick::device(owner, &ids)?.clone();
    macs.into_iter().find(|mac| mac.device_id == picked)
}

/// Asks the Mac the accepted proposal is for to run it, and marks it running.
/// The Err is the code why nothing was asked: `act_unavailable` (no linked
/// Mac takes requests), `act_busy` (it runs another), `proposal_unknown` (the
/// proposal is not accepted and waiting any more).
pub(crate) async fn request(
    db: &DatabaseConnection,
    proposal: &IntentRecord,
    cap: Option<Vec<String>>,
) -> Result<String, &'static str> {
    counted(ask(db, proposal, cap, false).await)
}

/// The owner accepted on the site her proposal for their Mac, still open:
/// the Mac is asked to run it, and only then is it accepted and running. A
/// Mac that is busy or gone leaves the proposal as it was (D38); the Err is
/// the code why, as [`request`]'s.
pub(crate) async fn request_accepting(
    db: &DatabaseConnection,
    proposal: &IntentRecord,
) -> Result<String, &'static str> {
    counted(ask(db, proposal, None, true).await)
}

/// The call between ends this is, as `alive` counts them.
const CALL: &str = "act.request";

/// Counts a request as asked, or as the code why it was not. A run asked
/// as a call is counted as calls are.
fn counted(asked: Result<String, &'static str>) -> Result<String, &'static str> {
    if !asked.as_ref().is_ok_and(|id| id.starts_with("run_")) {
        crate::services::agent::alive::called(
            CALL,
            asked.as_ref().map_or_else(|code| *code, |_| "asked"),
        );
    }
    asked
}

async fn ask(
    db: &DatabaseConnection,
    proposal: &IntentRecord,
    cap: Option<Vec<String>>,
    accept: bool,
) -> Result<String, &'static str> {
    // While she is not here (moving, or living elsewhere) nothing is asked
    // in her name.
    match crate::services::persona_sync::lease::place() {
        crate::services::persona_sync::lease::Where::Here => {}
        crate::services::persona_sync::lease::Where::Moving => return Err("host_frozen"),
        _ => return Err("host_moved"),
    }
    let owner = proposal.user_id;
    let wanted = proposal.proposal.device_id.as_deref();
    // A Mac granted a whole run as a call takes it as one; `act.request` is
    // for one that does not list calls.
    let called: Vec<String> = super::granted_devices(owner, RUN)
        .into_iter()
        .filter(|device| wanted.is_none_or(|wanted| wanted == device))
        .collect();
    if let Some(device_id) = super::pick::device(owner, &called).cloned() {
        return run_as_call(db, proposal, cap, accept, device_id).await;
    }
    let resting = RESTING.lock().map(|set| set.clone()).unwrap_or_default();
    let Some((device_id, _, sender)) = super::act_links(owner)
        .into_iter()
        .filter(|(device, _, _)| !resting.contains(device))
        .find(|(device, _, _)| wanted.is_none_or(|wanted| wanted == device))
    else {
        return Err("act_unavailable");
    };
    let id = format!("act_{}", uuid::Uuid::new_v4().simple());
    let mut body = json!({
        "proposalId": proposal.id,
        "title": proposal.proposal.title,
        "instruction": proposal.proposal.instruction,
        "pickupBy": crate::clock::now().timestamp_millis() + CONTRACT.timing.act_pickup_ms as i64,
        "maxRunMs": CONTRACT.timing.act_max_run_ms,
    });
    if !proposal.proposal.expected_outcome.trim().is_empty() {
        body["expectedOutcome"] = proposal.proposal.expected_outcome.clone().into();
    }
    // Only names a Mac knows (`capNames`); the rest stand for nothing there.
    if let Some(cap) = cap {
        body["cap"] = super::contract::cap_for_mac(cap).into();
    }
    // One request at a time per Mac: kept before the proposal moves, so a
    // second one meanwhile finds it busy.
    {
        let Ok(mut acts) = ACTS.lock() else {
            return Err("act_unavailable");
        };
        if busy(&acts, &device_id) {
            return Err("act_busy");
        }
        acts.insert(
            id.clone(),
            Act {
                owner,
                device_id: device_id.clone(),
                proposal_id: proposal.id.clone(),
                body: body.clone(),
                asked_at: Instant::now(),
                started_at: None,
                acked: false,
                revoked: false,
            },
        );
    }
    if !running(db, proposal, accept).await {
        if let Ok(mut acts) = ACTS.lock() {
            acts.remove(&id);
        }
        return Err("proposal_unknown");
    }
    // A full queue leaves it to the next heartbeat; it is kept either way.
    let _ = sender.try_send(Outgoing::reliable("act.request", id.clone(), body));
    tracing::info!(device = %device_id, proposal = %proposal.id, act = %id, "[link] asked the Mac to act");
    Ok(id)
}

/// The proposal accepted (when `accept`) and running; false when it moved
/// on meanwhile.
async fn running(db: &DatabaseConnection, proposal: &IntentRecord, accept: bool) -> bool {
    let owner = proposal.user_id;
    let store = IntentStore::new(db.clone());
    let accepted = if accept {
        store
            .mark_accepted(&proposal.id, owner, AcceptSource::User)
            .await
            .map(|_| ())
    } else {
        Ok(())
    };
    let moved = match accepted {
        Ok(()) => store
            .transition(&proposal.id, owner, IntentStatus::Running, None, None, None)
            .await
            .map(|_| ()),
        Err(error) => Err(error),
    };
    if let Err(error) = moved {
        tracing::info!(%error, proposal = %proposal.id, "[link] proposal moved before the Mac was asked");
        return false;
    }
    true
}

/// Asks Mac `device_id` to run the proposal as a call (`work.run`), and
/// marks it running; a task waits for how it ends and moves it on. The Ok
/// is the run's id here.
async fn run_as_call(
    db: &DatabaseConnection,
    proposal: &IntentRecord,
    cap: Option<Vec<String>>,
    accept: bool,
    device_id: String,
) -> Result<String, &'static str> {
    let owner = proposal.user_id;
    let mut args = json!({
        "proposalId": proposal.id,
        "title": proposal.proposal.title,
        "instruction": proposal.proposal.instruction,
    });
    if !proposal.proposal.expected_outcome.trim().is_empty() {
        args["expectedOutcome"] = proposal.proposal.expected_outcome.clone().into();
    }
    if let Some(cap) = cap {
        args["cap"] = super::contract::cap_for_mac(cap).into();
    }
    // One run at a time per Mac, either way it is asked.
    {
        let Ok(acts) = ACTS.lock() else {
            return Err("act_unavailable");
        };
        if busy(&acts, &device_id) {
            return Err("act_busy");
        }
        let Ok(mut runs) = RUNS.lock() else {
            return Err("act_unavailable");
        };
        runs.insert(
            proposal.id.clone(),
            Run {
                owner,
                device_id: device_id.clone(),
                stop: None,
            },
        );
    }
    if !running(db, proposal, accept).await {
        if let Ok(mut runs) = RUNS.lock() {
            runs.remove(&proposal.id);
        }
        return Err("proposal_unknown");
    }
    let waiting = {
        let db = db.clone();
        let device_id = device_id.clone();
        let proposal_id = proposal.id.clone();
        tokio::spawn(async move {
            // For the owner: her proposals for their Mac are theirs.
            let answer = super::calls_pending::ask(
                &db,
                owner,
                &device_id,
                RUN,
                &json!({ "kind": "owner" }),
                args,
                None,
            )
            .await;
            let ours = RUNS
                .lock()
                .ok()
                .and_then(|mut runs| runs.remove(&proposal_id))
                .is_some();
            if ours {
                ended(&db, owner, &proposal_id, &answer).await;
            }
        })
    };
    // Dismissed meanwhile (or over already): no one waits for it.
    let kept = RUNS.lock().is_ok_and(|mut runs| {
        runs.get_mut(&proposal.id)
            .map(|run| run.stop = Some(waiting.abort_handle()))
            .is_some()
    });
    if !kept {
        waiting.abort();
    }
    tracing::info!(device = %device_id, proposal = %proposal.id, "[link] asked the Mac to run it as a call");
    Ok(format!("run_{}", proposal.id))
}

/// How a run asked as a call leaves her proposal: with no word of the
/// device's, only how it ended (`act_*` as a request's, `declined`,
/// `outcome_unknown`). None takes it back to accepted: the contract says
/// it was not done.
pub(super) fn run_ended(answer: &Answer) -> Option<(IntentStatus, Option<String>)> {
    let said = |code: &str| Some(code.to_string());
    match answer.outcome {
        Outcome::Done => match answer
            .result
            .as_ref()
            .and_then(|result| result["status"].as_str())
        {
            Some("completed") => Some((IntentStatus::Completed, None)),
            _ => Some((IntentStatus::Failed, None)),
        },
        Outcome::Failed("call_cancelled") => Some((IntentStatus::Abandoned, said("act_cancelled"))),
        Outcome::Failed("declined") => Some((IntentStatus::Abandoned, said("declined"))),
        Outcome::Failed("call_timeout") => Some((IntentStatus::Failed, said("act_timeout"))),
        Outcome::Failed("internal_error") | Outcome::Unknown => {
            Some((IntentStatus::Failed, said("outcome_unknown")))
        }
        Outcome::Failed(_) => None,
    }
}

async fn ended(db: &DatabaseConnection, owner: i32, proposal_id: &str, answer: &Answer) {
    let store = IntentStore::new(db.clone());
    match run_ended(answer) {
        Some((status, said)) => {
            tracing::info!(proposal = %proposal_id, ?status, "[link] the Mac's run ended");
            if let Err(error) = store
                .transition(proposal_id, owner, status, None, None, said)
                .await
            {
                // Dismissed on the site meanwhile.
                tracing::info!(%error, proposal = %proposal_id, "[link] the proposal had moved on");
            }
        }
        None => {
            let _ = store.reclaim_running_to_accepted(proposal_id, owner).await;
        }
    }
}

/// The device's requests it has not acked, to send again.
pub(super) fn pending(device_id: &str) -> Vec<(String, Value)> {
    ACTS.lock()
        .map(|acts| {
            acts.iter()
                .filter(|(_, act)| act.device_id == device_id && !act.acked && !act.revoked)
                .map(|(id, act)| (id.clone(), act.body.clone()))
                .collect()
        })
        .unwrap_or_default()
}

/// The device has the request `id`.
pub(super) fn acked(device_id: &str, id: &str) {
    if let Ok(mut acts) = ACTS.lock() {
        if let Some(act) = acts.get_mut(id).filter(|act| act.device_id == device_id) {
            act.acked = true;
        }
    }
}

/// The device linked again saying it takes requests: it may be asked again.
pub(super) fn linked(device_id: &str) {
    if let Ok(mut resting) = RESTING.lock() {
        resting.remove(device_id);
    }
}

/// The device is not one any more: its open request fails at the next sweep.
pub(super) fn forget_device(device_id: &str) {
    if let Ok(mut acts) = ACTS.lock() {
        for act in acts.values_mut().filter(|act| act.device_id == device_id) {
            act.revoked = true;
        }
    }
    linked(device_id);
}

/// One `act.result` body.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ActResult {
    pub(super) act_id: String,
    pub(super) phase: String,
    #[serde(default)]
    pub(super) status: Option<String>,
    #[serde(default)]
    pub(super) code: Option<String>,
    #[serde(default)]
    pub(super) summary: Option<String>,
}

/// How a finished run leaves her proposal.
pub(super) fn outcome(
    status: &str,
    summary: Option<&str>,
) -> Option<(IntentStatus, Option<String>)> {
    let said = summary
        .map(str::trim)
        .filter(|summary| !summary.is_empty())
        .map(|summary| summary.chars().take(SUMMARY_CHARS).collect::<String>());
    match status {
        "completed" => Some((IntentStatus::Completed, said)),
        "failed" => Some((IntentStatus::Failed, said)),
        "cancelled" => Some((IntentStatus::Abandoned, Some("act_cancelled".into()))),
        "timeout" => Some((IntentStatus::Failed, Some("act_timeout".into()))),
        _ => None,
    }
}

/// `act.result` from the device: what became of its request. The Err is a
/// frame error code.
pub(super) async fn result(
    db: &DatabaseConnection,
    owner: i32,
    device_id: &str,
    result: ActResult,
) -> Result<(), &'static str> {
    if !valid_id(&result.act_id) {
        return Err("bad_id");
    }
    let phase = result.phase.as_str();
    if !matches!(phase, "started" | "refused" | "finished") {
        return Err("bad_frame");
    }
    let finished = match phase {
        "finished" => Some(
            outcome(
                result.status.as_deref().unwrap_or(""),
                result.summary.as_deref(),
            )
            .ok_or("bad_frame")?,
        ),
        _ => None,
    };
    let act = {
        let Ok(mut acts) = ACTS.lock() else {
            return Ok(());
        };
        let ours = acts
            .get(&result.act_id)
            .is_some_and(|act| act.device_id == device_id && act.owner == owner);
        if !ours {
            // Over already (timed out, or a resend): nothing more to do.
            return Ok(());
        }
        if phase == "started" {
            if let Some(act) = acts.get_mut(&result.act_id) {
                act.acked = true;
                act.started_at.get_or_insert_with(Instant::now);
            }
            tracing::info!(device = %device_id, act = %result.act_id, "[link] the Mac started what it was asked");
            return Ok(());
        }
        acts.remove(&result.act_id)
    };
    let Some(act) = act else {
        return Ok(());
    };
    let store = IntentStore::new(db.clone());
    if let Some((status, summary)) = finished {
        tracing::info!(device = %device_id, act = %result.act_id, ?status, "[link] the Mac finished what it was asked");
        // One of the four words `outcome` took.
        crate::services::agent::alive::called(CALL, result.status.as_deref().unwrap_or("finished"));
        if let Err(error) = store
            .transition(&act.proposal_id, owner, status, None, None, summary)
            .await
        {
            // Dismissed on the site meanwhile, or timed out here first.
            tracing::info!(%error, act = %result.act_id, "[link] the proposal had moved on");
        }
        return Ok(());
    }
    let code = result.code.as_deref().unwrap_or("act_unavailable");
    tracing::info!(device = %device_id, act = %result.act_id, code, "[link] the Mac refused what it was asked");
    // The device's own word, of the contract's codes or not.
    crate::services::agent::alive::called(
        CALL,
        match code {
            "act_off" | "act_busy" | "act_unavailable" | "act_timeout" => code,
            _ => "refused",
        },
    );
    if code != "act_busy" {
        if let Ok(mut resting) = RESTING.lock() {
            resting.insert(device_id.to_string());
        }
    }
    let _ = store
        .reclaim_running_to_accepted(&act.proposal_id, owner)
        .await;
    Ok(())
}

/// Stops the request running the proposal, when the owner dismissed it on
/// the site; false when no Mac runs it.
pub(crate) fn cancel_for_proposal(owner: i32, proposal_id: &str) -> bool {
    // Asked as a call: no one waits for it any more, so it is cancelled
    // there (`calls_pending`).
    let run = RUNS.lock().ok().and_then(|mut runs| {
        runs.get(proposal_id)
            .is_some_and(|run| run.owner == owner)
            .then(|| runs.remove(proposal_id))
            .flatten()
    });
    if let Some(run) = run {
        if let Some(stop) = run.stop {
            stop.abort();
        }
        return true;
    }
    let found = ACTS.lock().ok().and_then(|acts| {
        acts.iter()
            .find(|(_, act)| act.owner == owner && act.proposal_id == proposal_id)
            .map(|(id, act)| (id.clone(), act.device_id.clone()))
    });
    let Some((id, device_id)) = found else {
        return false;
    };
    send_cancel(owner, &device_id, &id, "act_cancelled")
}

fn send_cancel(owner: i32, device_id: &str, id: &str, code: &str) -> bool {
    let Some((_, _, sender)) = super::act_links(owner)
        .into_iter()
        .find(|(device, _, _)| device == device_id)
    else {
        return false;
    };
    sender
        .try_send(Outgoing::latest(
            "act.cancel",
            json!({ "actId": id, "code": code }),
        ))
        .is_ok()
}

/// What a sweep found of one request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Due {
    /// Not started within `actPickupMs`.
    NotPickedUp,
    /// Started, and no result within `actMaxRunMs` and the grace.
    RanOver,
    /// The device was removed.
    Revoked,
}

fn due(act: &Act, now: Instant, pickup: Duration, max_run: Duration) -> Option<Due> {
    if act.revoked {
        return Some(Due::Revoked);
    }
    match act.started_at {
        None if now.duration_since(act.asked_at) >= pickup => Some(Due::NotPickedUp),
        Some(started) if now.duration_since(started) >= max_run + GRACE => Some(Due::RanOver),
        _ => None,
    }
}

/// Closes what ran out of time or lost its device, and moves the proposals
/// on: one not picked up is accepted again (and that Mac not asked again
/// until it links again); one that ran over, or whose Mac was removed, failed.
pub(crate) async fn sweep(db: &DatabaseConnection) {
    let now = Instant::now();
    let closed: Vec<(String, Act, Due)> = {
        let Ok(mut acts) = ACTS.lock() else {
            return;
        };
        let ids: Vec<(String, Due)> = acts
            .iter()
            .filter_map(|(id, act)| due(act, now, pickup(), max_run()).map(|due| (id.clone(), due)))
            .collect();
        ids.into_iter()
            .filter_map(|(id, due)| acts.remove(&id).map(|act| (id, act, due)))
            .collect()
    };
    let store = IntentStore::new(db.clone());
    for (id, act, due) in closed {
        tracing::info!(device = %act.device_id, act = %id, ?due, "[link] a request to the Mac is over");
        crate::services::agent::alive::called(
            CALL,
            match due {
                Due::NotPickedUp => "not_picked_up",
                Due::RanOver => "ran_over",
                Due::Revoked => "revoked",
            },
        );
        match due {
            Due::NotPickedUp => {
                if let Ok(mut resting) = RESTING.lock() {
                    resting.insert(act.device_id.clone());
                }
                let _ = store
                    .reclaim_running_to_accepted(&act.proposal_id, act.owner)
                    .await;
            }
            Due::RanOver => {
                send_cancel(act.owner, &act.device_id, &id, "act_timeout");
                let _ = store
                    .transition(
                        &act.proposal_id,
                        act.owner,
                        IntentStatus::Failed,
                        None,
                        None,
                        Some("act_timeout".into()),
                    )
                    .await;
            }
            Due::Revoked => {
                let _ = store
                    .transition(
                        &act.proposal_id,
                        act.owner,
                        IntentStatus::Failed,
                        None,
                        None,
                        Some("act_revoked".into()),
                    )
                    .await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn act(started: Option<Instant>, revoked: bool, asked: Instant) -> Act {
        Act {
            owner: 1,
            device_id: "dev_1".into(),
            proposal_id: "int_1".into(),
            body: json!({}),
            asked_at: asked,
            started_at: started,
            acked: false,
            revoked,
        }
    }

    #[test]
    fn a_request_is_over_when_not_picked_up_ran_over_or_its_mac_went() {
        let asked = Instant::now();
        let (pickup, max_run) = (Duration::from_secs(600), Duration::from_secs(1800));
        let soon = asked + Duration::from_secs(599);
        assert_eq!(due(&act(None, false, asked), soon, pickup, max_run), None);
        assert_eq!(
            due(&act(None, false, asked), asked + pickup, pickup, max_run),
            Some(Due::NotPickedUp)
        );
        // Started: the pickup no longer counts, the run's own limit and a minute do.
        let started = asked + Duration::from_secs(10);
        let running = act(Some(started), false, asked);
        assert_eq!(due(&running, started + max_run, pickup, max_run), None);
        assert_eq!(
            due(&running, started + max_run + GRACE, pickup, max_run),
            Some(Due::RanOver)
        );
        assert_eq!(
            due(&act(Some(started), true, asked), started, pickup, max_run),
            Some(Due::Revoked)
        );
    }

    #[test]
    fn a_finished_run_leaves_the_proposal_as_it_ended() {
        assert_eq!(
            outcome("completed", Some(" 都移好了 ")),
            Some((IntentStatus::Completed, Some("都移好了".into())))
        );
        assert_eq!(outcome("failed", None), Some((IntentStatus::Failed, None)));
        assert_eq!(
            outcome("cancelled", Some("停下了")),
            Some((IntentStatus::Abandoned, Some("act_cancelled".into())))
        );
        assert_eq!(
            outcome("timeout", None),
            Some((IntentStatus::Failed, Some("act_timeout".into())))
        );
        assert_eq!(outcome("interrupted", None), None);
        let long = "字".repeat(400);
        let kept = outcome("completed", Some(&long)).unwrap().1.unwrap();
        assert_eq!(kept.chars().count(), SUMMARY_CHARS);
        // The contract's statuses are exactly these.
        for status in &CONTRACT.act_statuses {
            assert!(outcome(status, None).is_some(), "{status}");
        }
        assert_eq!(CONTRACT.timing.act_pickup_ms, 600_000);
        assert_eq!(CONTRACT.timing.act_max_run_ms, 1_800_000);
    }
}
