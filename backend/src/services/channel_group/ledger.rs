//! The group's ledger: how it types and how long she takes to answer, as numbers only.

use super::*;

/// Runtime-registry namespace of each group's talk ledger (see `Ledger`).
pub(super) const LEDGER_NAMESPACE: &str = "group_talk_ledger";
pub(super) const LEDGER_MESSAGES: usize = 2000;
pub(super) const LEDGER_WAITS: usize = 500;
pub(super) const LEDGER_DAYS: i64 = 30;
/// Messages gathered before the ledger is written; her reply writes it too.
pub(super) const LEDGER_EVERY: usize = 10;
/// Who she is in the ledger.
pub(super) const HER: &str = "her";

/// How a group types, and how long she took there to answer whoever
/// called her, as numbers only: what the process layer is checked against
/// (see `myriad_merope::talk_shape::Typed`). Who is a hash, never an id;
/// nobody's words are kept.
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct Ledger {
    pub(super) messages: Vec<myriad_merope::talk_shape::Typed>,
    pub(super) her_waits: Vec<f64>,
    /// Counts of the pieces their messages and hers are made of (see
    /// `myriad_merope::contrast`), never the messages.
    #[serde(default)]
    pub(super) theirs: myriad_merope::contrast::Counts,
    #[serde(default)]
    pub(super) hers: myriad_merope::contrast::Counts,
}

/// Messages of each side the piece counts stay within (older ones weigh
/// less once past it).
pub(super) const LEDGER_COUNTED: u32 = 2000;

/// A member as the ledger knows them: the same token for the same person,
/// never their id.
pub(super) fn ledger_who(from: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    from.hash(&mut hasher);
    format!("{:012x}", hasher.finish() & 0xffff_ffff_ffff)
}

/// Note a message typed there (counted into its pieces for whoever said
/// it), and how long she took if it was her first answer to someone who
/// called her; whether the ledger is due a write.
pub(super) fn note_said(
    venue: &str,
    typed: Option<myriad_merope::talk_shape::Typed>,
    waited: Option<f64>,
    said: Option<&str>,
) -> bool {
    with_group(venue, |group| {
        if let (Some(typed), Some(said)) = (&typed, said) {
            if typed.by == HER {
                group.ledger_hers.add(said);
            } else {
                group.ledger_theirs.add(said);
            }
        }
        group.ledger.extend(typed);
        group.ledger_waits.extend(waited);
        group.ledger.len() >= LEDGER_EVERY || !group.ledger_waits.is_empty()
    })
    .unwrap_or(false)
}

/// Write what was noted to the group's ledger, keeping the latest.
pub(super) async fn keep_ledger(venue: &str) {
    let Ok(db) = crate::services::process_db::database() else {
        return;
    };
    let Some((messages, waits, theirs, hers)) = with_group(venue, |group| {
        (
            std::mem::take(&mut group.ledger),
            std::mem::take(&mut group.ledger_waits),
            std::mem::take(&mut group.ledger_theirs),
            std::mem::take(&mut group.ledger_hers),
        )
    }) else {
        return;
    };
    if messages.is_empty() && waits.is_empty() {
        return;
    }
    let mut ledger = crate::services::runtime_registry::get::<Ledger>(&db, LEDGER_NAMESPACE, venue)
        .await
        .ok()
        .flatten()
        .unwrap_or_default();
    ledger.messages.extend(messages);
    let over = ledger.messages.len().saturating_sub(LEDGER_MESSAGES);
    ledger.messages.drain(..over);
    ledger.her_waits.extend(waits);
    let over = ledger.her_waits.len().saturating_sub(LEDGER_WAITS);
    ledger.her_waits.drain(..over);
    ledger.theirs.merge(&theirs);
    ledger.theirs.keep_within(LEDGER_COUNTED);
    ledger.hers.merge(&hers);
    ledger.hers.keep_within(LEDGER_COUNTED);
    let keep_until = (chrono::Utc::now() + chrono::Duration::days(LEDGER_DAYS)).timestamp();
    if let Err(error) = crate::services::runtime_registry::put(
        &db,
        LEDGER_NAMESPACE,
        venue,
        crate::services::runtime_registry::RegistryIdentity {
            subject_id: None,
            owner_id: None,
            tapp_id: None,
            runtime_id: None,
        },
        &ledger,
        keep_until,
    )
    .await
    {
        warn!(%error, %venue, "[Group] could not keep the talk ledger");
    }
}

/// How her lines differ from the people's here (see
/// `myriad_merope::contrast`): from the ledger and what is not yet written
/// to it; while the ledger has too little of theirs, from the lines kept.
pub(super) async fn how_she_differs(venue: &str) -> Option<String> {
    use myriad_merope::contrast::{Counts, describe, overused};
    let (mut theirs, mut hers) = with_group(venue, |group| {
        (group.ledger_theirs.clone(), group.ledger_hers.clone())
    })?;
    if let Ok(db) = crate::services::process_db::database()
        && let Ok(Some(ledger)) =
            crate::services::runtime_registry::get::<Ledger>(&db, LEDGER_NAMESPACE, venue).await
    {
        theirs.merge(&ledger.theirs);
        hers.merge(&ledger.hers);
    }
    if theirs.messages < 20 {
        (theirs, hers) = with_group(venue, |group| {
            let kept = |hers: bool| {
                Counts::of(
                    group
                        .lines
                        .iter()
                        .filter(|line| line.hers == hers)
                        .flat_map(|line| line.text.lines()),
                )
            };
            (kept(false), kept(true))
        })?;
    }
    describe(&overused(&hers, &theirs))
}
