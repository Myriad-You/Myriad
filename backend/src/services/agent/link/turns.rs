//! Which run each of a device's turns started: the idempotency record for
//! `input` and the key `run.resume` finds a run by. Kept in memory and in the
//! runtime registry for as long as runs are (a day), so a resent input does
//! not start a second run even across a restart, and a device can only ever
//! resume runs its own turns started.

use std::collections::{HashMap, VecDeque};
use std::sync::{LazyLock, Mutex};

use sea_orm::ConnectionTrait;
use serde::{Deserialize, Serialize};

use crate::services::runtime_registry::{self as registry, RegistryIdentity};

const NAMESPACE: &str = "her_link_turn";
/// The same as the run hub keeps runs.
const RETENTION_SECS: i64 = 24 * 60 * 60;
/// Turns kept in memory per device; older ones are still in the registry.
const KEPT_PER_DEVICE: usize = 64;
/// Turns told back in `welcome`.
pub(super) const TOLD_IN_WELCOME: usize = 16;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Turn {
    pub turn_id: String,
    pub run_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

static TURNS: LazyLock<Mutex<HashMap<String, VecDeque<Turn>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn record_id(device_id: &str, turn_id: &str) -> String {
    format!("{device_id}:{turn_id}")
}

fn keep(device_id: &str, turn: Turn) {
    let Ok(mut turns) = TURNS.lock() else {
        return;
    };
    let kept = turns.entry(device_id.to_string()).or_default();
    kept.retain(|old| old.turn_id != turn.turn_id);
    kept.push_front(turn);
    kept.truncate(KEPT_PER_DEVICE);
}

/// The run this device's turn started, if it started one.
pub(crate) async fn find<C: ConnectionTrait>(
    db: &C,
    device_id: &str,
    turn_id: &str,
) -> Option<Turn> {
    let kept = TURNS.lock().ok().and_then(|turns| {
        turns
            .get(device_id)?
            .iter()
            .find(|turn| turn.turn_id == turn_id)
            .cloned()
    });
    if kept.is_some() {
        return kept;
    }
    match registry::get::<Turn>(db, NAMESPACE, &record_id(device_id, turn_id)).await {
        Ok(Some(turn)) if turn.turn_id == turn_id => {
            keep(device_id, turn.clone());
            Some(turn)
        }
        Ok(_) => None,
        Err(error) => {
            tracing::warn!(%error, device = %device_id, "[link] could not read a turn record");
            None
        }
    }
}

/// Records that this device's turn started `turn.run_id`.
pub(crate) async fn remember<C: ConnectionTrait>(db: &C, device_id: &str, owner: i32, turn: Turn) {
    keep(device_id, turn.clone());
    let expires_at = crate::clock::now().timestamp() + RETENTION_SECS;
    if let Err(error) = registry::put(
        db,
        NAMESPACE,
        &record_id(device_id, &turn.turn_id),
        RegistryIdentity {
            subject_id: Some(owner),
            owner_id: Some(owner),
            tapp_id: None,
            runtime_id: Some(device_id),
        },
        &turn,
        expires_at,
    )
    .await
    {
        // The turn still runs; only a resend after a restart could start it again.
        tracing::warn!(%error, device = %device_id, "[link] could not record a turn");
    }
}

/// The device's turn that started `run_id`, among those this process has.
pub(crate) fn by_run(device_id: &str, run_id: &str) -> Option<Turn> {
    TURNS.lock().ok().and_then(|turns| {
        turns
            .get(device_id)?
            .iter()
            .find(|turn| turn.run_id == run_id)
            .cloned()
    })
}

/// The device's latest turns in this process, newest first.
pub(crate) fn recent(device_id: &str, limit: usize) -> Vec<Turn> {
    TURNS
        .lock()
        .ok()
        .and_then(|turns| {
            turns
                .get(device_id)
                .map(|kept| kept.iter().take(limit).cloned().collect())
        })
        .unwrap_or_default()
}

/// Drops what this process remembers about the device's turns, leaving the
/// registry: as after a restart.
#[cfg(test)]
pub(crate) fn forget_in_memory(device_id: &str) {
    if let Ok(mut turns) = TURNS.lock() {
        turns.remove(device_id);
    }
}
