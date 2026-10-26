//! What a child end saw for her (`moment`): while she lives on the host,
//! the child's own senses keep running (another coding agent's session on
//! that machine, the owner's conversation with another AI there, its own
//! reading of what they are doing) and tell her host each event she would
//! have had living there.
//!
//! The host takes it as an event of her own for the owner alone, on their
//! latest private chat, kept as from that device: said in person or let go
//! (`merope::spawn_told`). What she decides to remember of it becomes a
//! memory only for the kinds the contract lets keep one, and only when the
//! child says so. What it says is data about what happened, never anything
//! she is told to do.
//!
//! The child keeps a moment until `moment.taken` names it, which goes once
//! the moment is written here. A record of each taken id (two days, longer
//! than a child keeps one) answers a resend without taking it in twice.

use std::collections::{BTreeMap, HashSet};
use std::sync::{LazyLock, Mutex};

use chrono::{DateTime, Utc};
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::services::agent::consciousness::EventUrgency;
use crate::services::runtime_registry::{self as registry, RegistryIdentity};

use super::connection::Outgoing;
use super::contract::CONTRACT;

const NAMESPACE: &str = "her_link_moment";
/// How long a taken id is remembered: a child keeps a moment a day at most.
const RETENTION_SECS: i64 = 2 * 24 * 60 * 60;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum Urgency {
    Soon,
    #[default]
    Normal,
}

/// One `moment` body.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Moment {
    pub(super) kind: String,
    pub(super) occurred_at: i64,
    pub(super) headline: String,
    pub(super) summary: String,
    #[serde(default)]
    pub(super) safe_facts: BTreeMap<String, String>,
    #[serde(default)]
    pub(super) urgency: Urgency,
    #[serde(default)]
    pub(super) keeps_memory: bool,
}

/// A field name as the child's sources may use one: lowercase, no part of
/// it a word that would carry someone's words or tell her what to do.
pub(super) fn fact_key(key: &str) -> bool {
    let rules = &CONTRACT.moments.fact_key;
    (1..=rules.max_chars).contains(&key.len())
        && key.as_bytes()[0].is_ascii_lowercase()
        && key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        && key
            .split('_')
            .all(|part| !rules.refused_parts.iter().any(|refused| refused == part))
}

impl Moment {
    /// A kind a child tells, within the contract's limits.
    pub(super) fn readable(&self) -> bool {
        let rules = &CONTRACT.moments;
        rules.kinds.contains(&self.kind)
            && self.occurred_at > 0
            && self.headline.chars().count() <= rules.headline_chars
            && !self.summary.trim().is_empty()
            && self.summary.chars().count() <= CONTRACT.limits.max_text_chars
            && self.safe_facts.len() <= rules.facts_at_most
            && self
                .safe_facts
                .iter()
                .all(|(key, value)| fact_key(key) && value.chars().count() <= rules.fact_chars)
    }

    /// What she decides to remember of it may become a memory: only for a
    /// kind that may keep one, whatever the child says of the rest.
    pub(super) fn keeps_memory(&self) -> bool {
        self.keeps_memory && CONTRACT.moments.keeps_memory.contains(&self.kind)
    }

    /// Her event, as from `device_id`, where it is the moment `id`.
    pub(super) fn told(&self, device_id: &str, id: &str) -> crate::services::agent::merope::Told {
        crate::services::agent::merope::Told {
            id: id.to_string(),
            headline: self.headline.clone(),
            safe_facts: self.safe_facts.clone(),
            urgency: match self.urgency {
                Urgency::Soon => EventUrgency::Soon,
                Urgency::Normal => EventUrgency::Normal,
            },
            occurred_at: DateTime::<Utc>::from_timestamp_millis(self.occurred_at)
                .unwrap_or_else(crate::clock::now),
            origin: device_id.to_string(),
            keeps_memory: self.keeps_memory(),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Taken {
    id: String,
}

fn record_id(device_id: &str, id: &str) -> String {
    format!("{device_id}:{id}")
}

/// Moments being written now, by device and id: a copy on a link that
/// replaced the first is let be, and is answered when it comes again.
static WRITING: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

pub(super) fn taken_frame(id: &str) -> Outgoing {
    Outgoing::latest("moment.taken", json!({ "id": id }))
}

/// Whether this device's moment `id` was already written.
pub(super) async fn was_taken(db: &DatabaseConnection, device_id: &str, id: &str) -> bool {
    match registry::get::<Taken>(db, NAMESPACE, &record_id(device_id, id)).await {
        Ok(found) => found.is_some_and(|taken| taken.id == id),
        Err(error) => {
            tracing::warn!(%error, device = %device_id, "[link] could not read a moment record");
            false
        }
    }
}

/// Starts writing the moment unless it is being written already.
pub(super) fn begin(device_id: &str, id: &str) -> bool {
    WRITING
        .lock()
        .map(|mut writing| writing.insert(record_id(device_id, id)))
        .unwrap_or(false)
}

pub(super) fn end(device_id: &str, id: &str) {
    if let Ok(mut writing) = WRITING.lock() {
        writing.remove(&record_id(device_id, id));
    }
}

/// Records the moment as taken. Once it is, it reaches her: the caller
/// hands it over and answers `moment.taken`.
pub(super) async fn write(
    db: &DatabaseConnection,
    owner: i32,
    device_id: &str,
    id: &str,
) -> Result<(), sea_orm::DbErr> {
    registry::put(
        db,
        NAMESPACE,
        &record_id(device_id, id),
        RegistryIdentity {
            subject_id: Some(owner),
            owner_id: Some(owner),
            tapp_id: None,
            runtime_id: Some(device_id),
        },
        &Taken { id: id.to_string() },
        crate::clock::now().timestamp() + RETENTION_SECS,
    )
    .await
}

/// Hands the taken moment to her, for the owner, as from `device_id`.
pub(super) fn hand_over(owner: i32, device_id: &str, id: &str, moment: Moment) {
    let told = moment.told(device_id, id);
    #[cfg(test)]
    if let Ok(mut handed) = HANDED.lock() {
        handed.push((moment.kind.clone(), moment.summary.clone(), told.clone()));
    }
    crate::services::agent::merope::spawn_told(owner, moment.kind, moment.summary, told);
}

/// What was handed to her, in order: (kind, summary, as told).
#[cfg(test)]
pub(super) static HANDED: Mutex<Vec<(String, String, crate::services::agent::merope::Told)>> =
    Mutex::new(Vec::new());
