//! Her undertone and her own gaps (see `myriad_merope::undertone`).
//!
//! What landed hard in her own time (it moved her, a guess held or failed,
//! she found something out or found she had thought it wrong, a want came
//! true or she let it go) pushes how today has left her, as what happens
//! with people does (`self_state`). Once a night the days not yet in are
//! folded into her undertone, from her own rows alone (her experiences,
//! how her wants ended, the minutes she spent and lazed): none of it is
//! about anyone. It is kept in the runtime registry; once every end reads
//! it, it goes with her log as one of her records
//! (`persona_sync::snapshot`). Every place her mood settles reads it
//! (`store::addressee::baseline_toward`).

use chrono::{DateTime, FixedOffset, NaiveDate, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection};

use crate::services::agent::memory::unified;
pub use myriad_merope::undertone::Undertone;
use myriad_merope::{elsewhere::Push, undertone::Day};

pub const NAMESPACE: &str = "merope_undertone";
pub const RECORD: &str = "undertone";
/// Kept this long past the last night it was folded.
const KEEP_DAYS: i64 = 400;
/// Nights a missed one can still be folded for.
const BACKFILL_DAYS: u64 = 3;
/// Most of her own rows of one kind read for a day, or since she got up.
const READ_AT_MOST: u64 = 3000;

fn identity() -> crate::services::runtime_registry::RegistryIdentity<'static> {
    crate::services::runtime_registry::RegistryIdentity {
        subject_id: None,
        owner_id: None,
        tapp_id: None,
        runtime_id: None,
    }
}

/// Her undertone as kept; none until a night has folded a day she lived.
pub async fn kept<C: ConnectionTrait>(db: &C) -> Option<Undertone> {
    crate::services::runtime_registry::get::<Undertone>(db, NAMESPACE, RECORD)
        .await
        .ok()
        .flatten()
        .filter(Undertone::holds)
}

async fn put(db: &DatabaseConnection, undertone: &Undertone) {
    let keep_until = (crate::clock::now() + chrono::Duration::days(KEEP_DAYS)).timestamp();
    if let Err(error) = crate::services::runtime_registry::put(
        db,
        NAMESPACE,
        RECORD,
        identity(),
        undertone,
        keep_until,
    )
    .await
    {
        tracing::warn!(%error, "[Merope] could not keep her undertone");
    }
}

/// Each thing of her own that landed hard between `since` and `until`,
/// as a push of how today has left her (`myriad_merope::undertone::gap`,
/// `want_ended`), and how many things of her own she did then.
pub async fn gaps_between(
    db: &DatabaseConnection,
    since: DateTime<FixedOffset>,
    until: DateTime<FixedOffset>,
) -> (Vec<Push>, u32) {
    let mut pushes = Vec::new();
    let mut things = 0;
    let within = |row: &crate::models::entities::agent_memories::Model| row.created_at < until;
    let push = |row: &crate::models::entities::agent_memories::Model, valence: f64| Push {
        at: row.created_at.with_timezone(&Utc),
        valence,
    };
    if let Ok(rows) =
        unified::own_rows_since(db, unified::OWN_EXPERIENCE, since, READ_AT_MOST).await
    {
        for row in rows.iter().filter(|row| within(row)) {
            let Some(gap) = super::doing::gap_of(row) else {
                continue;
            };
            things += 1;
            if let Some(valence) = gap {
                pushes.push(push(row, valence));
            }
        }
    }
    if let Ok(rows) = unified::own_rows_since(db, super::wants::ENDED, since, READ_AT_MOST).await
    {
        for row in rows.iter().filter(|row| within(row)) {
            if let Some(came_true) = super::wants::came_true(row) {
                pushes.push(push(
                    row,
                    myriad_merope::undertone::want_ended(came_true),
                ));
            }
        }
    }
    (pushes, things)
}

/// One of her days as her undertone counts it, from her own rows alone.
pub async fn day_of(db: &DatabaseConnection, day: NaiveDate) -> Option<Day> {
    let (start, end) = super::pace::bounds(day)?;
    let (gaps, things) = gaps_between(db, start, end).await;
    Some(Day {
        things,
        gaps: u32::try_from(gaps.len()).unwrap_or(u32::MAX),
        lift: gaps.iter().map(|gap| gap.valence).sum(),
        spent_minutes: super::pace::spent_on(db, day).await.unwrap_or(0.0),
        lazed_minutes: super::pace::lazed_on(db, day).await,
    })
}

/// At night: the days not yet in her undertone (yesterday, and a few a
/// missed night left), oldest first. Folding a day twice changes nothing.
pub async fn go_over(db: &DatabaseConnection) {
    let today = super::clock::local_now().date_naive();
    let mut undertone = kept(db).await;
    let before = undertone;
    for back in (1..=BACKFILL_DAYS).rev() {
        let Some(day) = today.checked_sub_days(chrono::Days::new(back)) else {
            continue;
        };
        if undertone.is_some_and(|kept| kept.day >= day) {
            continue;
        }
        let Some(lived) = day_of(db, day).await else {
            continue;
        };
        undertone = Undertone::after(undertone.as_ref(), day, &lived);
    }
    if let Some(undertone) = undertone.filter(|now| Some(*now) != before) {
        put(db, &undertone).await;
        tracing::info!(
            day = %undertone.day,
            days = undertone.days,
            "[Merope] her undertone, with another day of her own"
        );
    }
}

#[cfg(test)]
#[path = "undertone_tests.rs"]
mod tests;
