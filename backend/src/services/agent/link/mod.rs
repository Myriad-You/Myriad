//! Her link: the one WebSocket a child end dials to her host,
//! `GET /api/agent/link` with a device token. Its frames, codes and timings
//! are `shared/her_link_contract.json`, which children build against.
//!
//! Here the site is her host and the child is a native app on the site
//! owner's device. A child sends what the person says and whether they are
//! there; the host runs her as it always does and sends the run, her acting
//! and her mood down. Nothing she is — persona, memory, mood, decisions,
//! model keys — goes to the child, and no frame or error carries a secret.
//! Runs start for the site owner through the same entry and gate as every
//! other channel, so what a turn may do is the owner's granted permissions
//! as they stand at that moment.
//!
//! One link per device: a new connection replaces the old one. Removing the
//! device, or the site getting another owner, closes it.
//!
//! What she says unprompted goes to the app when that is where the person
//! is using her ([`say`]); see `merope::ingest` and `merope::reach`.
//!
//! Her body on the app ([`body`]): a child that says it takes `playback`
//! gets her director's refinements as the reply is being said, and reports
//! how far saying it got; one that takes `touch` sends contact with her
//! figure up, and she answers it there. One that takes `wardrobe` wears its
//! own set of her wardrobe ([`outfit`]).
//!
//! Her senses on the app ([`senses`]): what a child reports seeing and
//! hearing for her, as far as it said it senses it, is what she senses of
//! the person while the app is where they are using her.
//!
//! Her experience on the app ([`experience`]): Work a child that takes
//! `experience` runs starts from her Work memories, asked for over the
//! link, and what it did comes back up to become part of her.
//!
//! Her life on the app beside the chat: a child that takes `conversations`
//! is shown her recent chats with the owner ([`conversations`]); one that
//! takes `notify` is told the site's notifications about her ([`notices`]);
//! one that takes `proposals` shows her Work proposals as cards the owner
//! can answer there ([`proposals`]).
//!
//! Her host asking the app to act ([`act`]): a child that takes `act` (the
//! owner switched it on there) is asked to run one Work task of hers, a
//! proposal for that Mac accepted on the site or by her; it runs there under
//! the Mac's own grants, and comes back as its result and her experience.

mod act;
mod body;
mod connection;
mod contract;
mod conversations;
mod experience;
mod notices;
mod outfit;
mod proposals;
mod say;
mod senses;
mod turns;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use tokio::sync::{mpsc, oneshot};

pub(crate) use act::{
    cancel_for_proposal, mac_for_work, macs, request as act_request,
    request_accepting as act_request_accepting, sweep as sweep_acts,
};
pub(crate) use body::perform_say;
pub(crate) use connection::serve;
pub(crate) use contract::{Close, Timing};
pub(crate) use conversations::{
    heard_change as conversations_changed, heard_nothing_for_a_while as conversations_unheard,
};
pub(crate) use say::{Say, SayKind, can_say, say};

/// The largest frame a child may send.
pub(crate) fn max_frame_bytes() -> usize {
    contract::CONTRACT.limits.max_frame_bytes
}

struct Open {
    id: u64,
    owner: i32,
    close: oneshot::Sender<Close>,
    /// Where her `say` frames go, once the child said it takes them.
    says: Option<mpsc::Sender<connection::Outgoing>>,
    /// The child takes `perform` refinements of what it is saying.
    playback: bool,
    /// Where `experience.taken` goes, once the child said it sends `experience`.
    experience: Option<mpsc::Sender<connection::Outgoing>>,
    /// Where `act.request` goes, and the device's name, once the child said
    /// it takes `act`.
    acts: Option<(mpsc::Sender<connection::Outgoing>, String)>,
    /// The device she lives on dialled in as her host (H3e-7): where it is
    /// told she is frozen there or thawed, and the move it was last told of.
    host: Option<(mpsc::Sender<connection::Outgoing>, Option<String>)>,
}

/// The open link of each device, by device id.
static OPEN: LazyLock<Mutex<HashMap<String, Open>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Registers a new link for the device, closing the one it had.
fn open(device_id: &str, owner: i32) -> (u64, oneshot::Receiver<Close>) {
    let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let (close, closed) = oneshot::channel();
    if let Ok(mut open) = OPEN.lock() {
        let link = Open {
            id,
            owner,
            close,
            says: None,
            playback: false,
            experience: None,
            acts: None,
            host: None,
        };
        if let Some(previous) = open.insert(device_id.to_string(), link) {
            let _ = previous.close.send(Close::Replaced);
        }
    }
    (id, closed)
}

/// The link `id` takes `say` frames, sent through `sender`, and with
/// `playback` the refinements of how she acts them.
fn takes_say(device_id: &str, id: u64, sender: mpsc::Sender<connection::Outgoing>, playback: bool) {
    if let Ok(mut open) = OPEN.lock() {
        if let Some(link) = open.get_mut(device_id).filter(|link| link.id == id) {
            link.says = Some(sender);
            link.playback = playback;
        }
    }
}

/// The link `id` sends `experience`: what is written of it is answered
/// through `sender`, whichever link carried it.
fn takes_experience(device_id: &str, id: u64, sender: mpsc::Sender<connection::Outgoing>) {
    if let Ok(mut open) = OPEN.lock() {
        if let Some(link) = open.get_mut(device_id).filter(|link| link.id == id) {
            link.experience = Some(sender);
        }
    }
}

/// The link `id` takes `act.request`, sent through `sender`; `name` is the
/// device as the owner named it.
fn takes_acts(device_id: &str, id: u64, sender: mpsc::Sender<connection::Outgoing>, name: String) {
    if let Ok(mut open) = OPEN.lock() {
        if let Some(link) = open.get_mut(device_id).filter(|link| link.id == id) {
            link.acts = Some((sender, name));
        }
    }
}

/// The link `id` is the device she lives on, as her host: lease changes are
/// told through `sender`; `frozen` is the move it was told it is frozen for.
fn takes_host(
    device_id: &str,
    id: u64,
    sender: mpsc::Sender<connection::Outgoing>,
    frozen: Option<String>,
) {
    if let Ok(mut open) = OPEN.lock() {
        if let Some(link) = open.get_mut(device_id).filter(|link| link.id == id) {
            link.host = Some((sender, frozen));
        }
    }
}

/// Where she lives changed: each child is told she is not here (frozen or
/// moved) and reconnects to the refusal; the device she lives on is told on
/// its own link that she is frozen there for a move, or thawed, and a device
/// that is not her host any more is told she moved (H3e-7).
pub(crate) fn lease_changed(lease: &crate::services::persona_sync::lease::Lease) {
    use crate::services::persona_sync::lease::{State, Where};
    let place = lease.place();
    let mut closing: Vec<(Open, Close)> = Vec::new();
    if let Ok(mut open) = OPEN.lock() {
        let devices: Vec<String> = open.keys().cloned().collect();
        for device in devices {
            let Some(link) = open.get_mut(&device) else {
                continue;
            };
            if let Some((sender, told)) = link.host.as_mut() {
                let holds =
                    lease.holder == device && matches!(lease.state, State::Live | State::Frozen);
                if !holds {
                    let _ = sender.try_send(connection::Outgoing::latest(
                        "host.moved",
                        serde_json::json!({ "epoch": lease.epoch, "holder": lease.holder }),
                    ));
                    if let Some(link) = open.remove(&device) {
                        closing.push((link, Close::HostMoved));
                    }
                    continue;
                }
                let frozen_for = lease
                    .moving
                    .as_ref()
                    .filter(|moving| lease.state == State::Frozen && moving.from == device);
                match frozen_for {
                    Some(moving) if told.as_deref() != Some(moving.id.as_str()) => {
                        let _ = sender.try_send(connection::Outgoing::latest(
                            "host.freeze",
                            connection::freeze_body(moving),
                        ));
                        *told = Some(moving.id.clone());
                    }
                    Some(_) => {}
                    None => {
                        if let Some(id) = told.take() {
                            let _ = sender.try_send(connection::Outgoing::latest(
                                "host.thaw",
                                serde_json::json!({ "moveId": id }),
                            ));
                        }
                    }
                }
                continue;
            }
            let why = match place {
                Where::Here => continue,
                // No end has her: every child is told, and reconnects to the refusal.
                Where::Moving => Close::HostFrozen,
                Where::Elsewhere(_) | Where::Vacant | Where::MovedOut => Close::HostMoved,
            };
            if let Some(link) = open.remove(&device) {
                closing.push((link, why));
            }
        }
    }
    for (link, why) in closing {
        let _ = link.close.send(why);
    }
}

/// The owner's devices whose links take `act` now: (device id, name, sender).
fn act_links(owner: i32) -> Vec<(String, String, mpsc::Sender<connection::Outgoing>)> {
    OPEN.lock()
        .map(|open| {
            open.iter()
                .filter(|(_, link)| link.owner == owner)
                .filter_map(|(device, link)| {
                    link.acts
                        .as_ref()
                        .map(|(sender, name)| (device.clone(), name.clone(), sender.clone()))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Where the owner's device is told an experience was written, when its
/// link is open and sends them.
fn experience_sender(owner: i32, device_id: &str) -> Option<mpsc::Sender<connection::Outgoing>> {
    OPEN.lock().ok().and_then(|open| {
        open.get(device_id)
            .filter(|link| link.owner == owner)
            .and_then(|link| link.experience.clone())
    })
}

/// Where the owner's device takes `say` frames, when its link is open and does.
fn say_sender(owner: i32, device_id: &str) -> Option<mpsc::Sender<connection::Outgoing>> {
    OPEN.lock().ok().and_then(|open| {
        open.get(device_id)
            .filter(|link| link.owner == owner)
            .and_then(|link| link.says.clone())
    })
}

/// Where the owner's device takes refinements of her `say` lines, when its
/// link is open and does.
fn playback_sender(owner: i32, device_id: &str) -> Option<mpsc::Sender<connection::Outgoing>> {
    OPEN.lock().ok().and_then(|open| {
        open.get(device_id)
            .filter(|link| link.owner == owner && link.playback)
            .and_then(|link| link.says.clone())
    })
}

/// The device has a link open now: what the owner's device list shows as
/// connected.
pub(crate) fn is_open(device_id: &str) -> bool {
    OPEN.lock()
        .map(|open| open.contains_key(device_id))
        .unwrap_or(false)
}

/// Forgets the link `id` when it is still the device's.
fn forget(device_id: &str, id: u64) {
    if let Ok(mut open) = OPEN.lock() {
        if open.get(device_id).is_some_and(|open| open.id == id) {
            open.remove(device_id);
        }
    }
}

/// Closes the device's link, if it has one open, saying why. A device that
/// is not one any more keeps nothing waiting for it.
pub(crate) fn disconnect(device_id: &str, why: Close) {
    if matches!(why, Close::Revoked | Close::OwnerChanged) {
        say::forget_device(device_id);
        act::forget_device(device_id);
    }
    let open = OPEN.lock().ok().and_then(|mut open| open.remove(device_id));
    if let Some(open) = open {
        let _ = open.close.send(why);
    }
}

/// Closes every open link: the host is stopping.
pub(crate) fn disconnect_all(why: Close) {
    let open: Vec<Open> = OPEN
        .lock()
        .map(|mut open| open.drain().map(|(_, open)| open).collect())
        .unwrap_or_default();
    for open in open {
        let _ = open.close.send(why);
    }
}
