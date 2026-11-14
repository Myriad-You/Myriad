//! A host core that holds her lease but never catches up: the server takes
//! her back.
//!
//! A new holder thinks only once it has every row the previous holder wrote
//! up to the handover (`lease::behind`), so a holder that cannot fetch them
//! holds her and does nothing: she answers no one anywhere, and nothing else
//! moves her (a takeover waits for the holder to be out of reach, and it is
//! not). What it holds is read from its last `log.vector` of each audience
//! against the grant's `through`, whether that vector came before the grant
//! or after ([`heard`], [`check`]): a holder stuck from the start may never
//! say anything again. Once [`AFTER_MICROS`] have passed with it behind and
//! no step forward, the server takes her back here by a force ([`check`]),
//! as when the owner turns syncing off, and tells every link. A holder the
//! server has heard no vector from in this process is not judged.
//!
//! The mark stays after that until the device's vector covers what it was
//! handed: until then it is not handed her again for a talk or a place
//! ([`held_back`]), or she would go there and stop again. The owner's own
//! "come here" still moves her.
//!
//! The marks and vectors are kept in the process: a restart forgets them,
//! and the vector a device sends when its link opens again tells them anew.

use std::collections::{BTreeMap, HashMap};
use std::sync::{LazyLock, Mutex};

use myriad_persona_log::Vector;
use myriad_persona_log::lease::{Grant, Request};
use sea_orm::DatabaseConnection;

use super::lease::{self, Outcome};
use super::{Error, NODE, Settings};

/// How long a holder may stay behind without a step forward: five minutes,
/// shorter than a first-sync link lives (`full::VALID_FOR_MICROS`).
pub const AFTER_MICROS: i64 = 5 * 60 * 1_000_000;

/// One device behind on what it was handed.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Mark {
    /// Whose rows it waits for: the holder it was handed her from.
    from: String,
    /// Per audience still short: (what it was handed, what it has).
    short: BTreeMap<String, (u64, u64)>,
    /// When it was first seen behind.
    since: i64,
    /// When it last took a step forward (or `since`).
    moved: i64,
}

/// What the server knows of the host cores' holdings.
#[derive(Debug, Default)]
struct Known {
    marks: HashMap<String, Mark>,
    /// Each device's last vector, per audience.
    last: HashMap<String, BTreeMap<String, Vector>>,
}

static KNOWN: LazyLock<Mutex<Known>> = LazyLock::new(Default::default);

// Tests run side by side in one process, each on its own thread.
#[cfg(test)]
thread_local! {
    static TEST_KNOWN: std::cell::RefCell<Known> = Default::default();
}

fn with_known<T>(act: impl FnOnce(&mut Known) -> T) -> T {
    #[cfg(test)]
    {
        let _ = &KNOWN;
        TEST_KNOWN.with(|known| act(&mut known.borrow_mut()))
    }
    #[cfg(not(test))]
    match KNOWN.lock() {
        Ok(mut known) => act(&mut known),
        Err(poisoned) => act(&mut poisoned.into_inner()),
    }
}

/// Where `grant`'s holder is short of what it was handed, by the vectors
/// `last` known of it: none when it is not, or nothing is known. An
/// audience it has said nothing of yet is one it has none of (its vectors
/// come one audience at a time, in no set order).
fn short_of(grant: &Grant, last: Option<&BTreeMap<String, Vector>>) -> Option<Mark> {
    let (through, last) = (grant.through.as_ref()?, last?);
    let from = grant.from.clone().unwrap_or_else(|| NODE.to_string());
    let short: BTreeMap<String, (u64, u64)> = through
        .iter()
        .filter_map(|(audience, &want)| {
            let have = last
                .get(audience)
                .and_then(|vector| vector.get(&from))
                .copied()
                .unwrap_or(0);
            (have < want).then(|| (audience.clone(), (want, have)))
        })
        .collect();
    (!short.is_empty()).then_some(Mark {
        from,
        short,
        since: 0,
        moved: 0,
    })
}

/// `device`, a host core, said it holds `theirs` of `audience` at the
/// server's time `now`. `standing` is the grant that stands when `device`
/// holds it: what it was handed is read from it. A device already marked
/// is read against its mark, holder or not, so it is cleared once it
/// catches up.
pub fn heard(device: &str, standing: Option<&Grant>, audience: &str, theirs: &Vector, now: i64) {
    with_known(|known| {
        known
            .last
            .entry(device.to_string())
            .or_default()
            .insert(audience.to_string(), theirs.clone());
        if let Some(mark) = known.marks.get_mut(device) {
            let Some(&(want, had)) = mark.short.get(audience) else {
                return;
            };
            let have = theirs.get(&mark.from).copied().unwrap_or(0);
            if have >= want {
                mark.short.remove(audience);
            } else {
                if have > had {
                    mark.moved = now;
                }
                mark.short.insert(audience.to_string(), (want, have));
            }
            if mark.short.is_empty() {
                let since = mark.since;
                known.marks.remove(device);
                tracing::info!(
                    device,
                    behind_secs = (now - since) / 1_000_000,
                    "[her-log] a device caught up on what it was handed"
                );
            }
            return;
        }
        let Some(grant) = standing.filter(|grant| grant.held_by(device)) else {
            return;
        };
        if let Some(mark) = short_of(grant, known.last.get(device)) {
            known.marks.insert(
                device.to_string(),
                Mark {
                    since: now,
                    moved: now,
                    ..mark
                },
            );
        }
    });
}

/// Whether `device` is behind on what it was handed her with: then it is
/// not handed her again by asking for a talk or a place.
pub fn held_back(device: &str) -> bool {
    with_known(|known| known.marks.contains_key(device))
}

/// The holder, behind with no step forward for [`AFTER_MICROS`] at `now`:
/// her mind taken back here by a force, and every link of the owner told.
/// The grant when it moved.
pub async fn check(
    db: &DatabaseConnection,
    settings: &Settings,
    now: i64,
) -> Result<Option<Grant>, Error> {
    let Some(standing) = lease::standing(db).await? else {
        return Ok(None);
    };
    let Some(holder) = standing.holder.clone().filter(|holder| holder != NODE) else {
        return Ok(None);
    };
    // A holder with no mark yet is read from its last vectors, those from
    // before the grant too: the five minutes start now.
    let Some(mark) = with_known(|known| {
        if !known.marks.contains_key(&holder)
            && let Some(mark) = short_of(&standing, known.last.get(&holder))
        {
            known.marks.insert(
                holder.clone(),
                Mark {
                    since: now,
                    moved: now,
                    ..mark
                },
            );
        }
        known.marks.get(&holder).cloned()
    }) else {
        return Ok(None);
    };
    if now - mark.moved < AFTER_MICROS {
        return Ok(None);
    }
    let Outcome::Granted(grant) = lease::request(
        db,
        settings,
        Request::Force {
            to: NODE.to_string(),
        },
        now,
    )
    .await?
    else {
        return Ok(None);
    };
    // Counts and audiences only: what it lacks, never what is in it.
    let short: Vec<String> = mark
        .short
        .iter()
        .map(|(audience, (want, have))| format!("{audience} {have}/{want}"))
        .collect();
    tracing::warn!(
        device = %holder,
        behind_secs = (now - mark.since) / 1_000_000,
        still_secs = (now - mark.moved) / 1_000_000,
        short = ?short,
        "[her-log] her mind was taken back from a device that never caught up"
    );
    let owner = crate::services::principal::site_owner_id(db).await?;
    super::pen::settle_for(db, settings, owner).await?;
    if let Some(owner) = owner {
        crate::services::agent::link::tell_log_links(
            owner,
            "lease.epoch",
            lease::epoch_body(&grant, settings),
        );
    }
    Ok(Some(grant))
}
