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

use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use sea_orm::DatabaseConnection;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::services::agent::consciousness::{
    AcceptSource, IntentRecord, IntentStatus, IntentStore,
};

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

fn pickup() -> Duration {
    Duration::from_millis(CONTRACT.timing.act_pickup_ms)
}

fn max_run() -> Duration {
    Duration::from_millis(CONTRACT.timing.act_max_run_ms)
}

/// The owner's Macs that take requests now, by name.
pub(crate) fn macs(owner: i32) -> Vec<Mac> {
    let mut macs: Vec<Mac> = super::act_links(owner)
        .into_iter()
        .map(|(device_id, name, _)| Mac { device_id, name })
        .collect();
    macs.sort_by(|a, b| a.name.cmp(&b.name).then(a.device_id.cmp(&b.device_id)));
    macs
}

/// The Mac a new proposal for a Mac is for: the one the owner is using her on
/// now, else the first.
pub(crate) fn mac_for_work(owner: i32) -> Option<Mac> {
    let macs = macs(owner);
    macs.iter()
        .find(|mac| {
            crate::services::agent::consciousness::native_presence(owner, &mac.device_id)
                .is_some_and(|live| live.page_visible)
        })
        .cloned()
        .or_else(|| macs.into_iter().next())
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
    ask(db, proposal, cap, false).await
}

/// The owner accepted on the site her proposal for their Mac, still open:
/// the Mac is asked to run it, and only then is it accepted and running. A
/// Mac that is busy or gone leaves the proposal as it was (D38); the Err is
/// the code why, as [`request`]'s.
pub(crate) async fn request_accepting(
    db: &DatabaseConnection,
    proposal: &IntentRecord,
) -> Result<String, &'static str> {
    ask(db, proposal, None, true).await
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
    let resting = RESTING.lock().map(|set| set.clone()).unwrap_or_default();
    let wanted = proposal.proposal.device_id.as_deref();
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
        if acts.values().any(|act| act.device_id == device_id) {
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
