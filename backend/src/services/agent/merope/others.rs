//! How others took her (see `myriad_merope::others`): what is kept of it,
//! and the records she looks back over when she writes who she has been.
//!
//! Writing to someone first is kept with what she wrote (`store`); whether
//! they answered is read from whether they said anything after. Speaking up
//! unasked in a group is kept here, once it is known whether anyone took it
//! up, in the group it happened in and apart from ordinary memory; the group
//! reads it back after a restart. What people told her about herself and
//! what hurt her are kept where they were said (`making_sense`, `sore`).

use chrono::{DateTime, FixedOffset, Utc};
use sea_orm::DatabaseConnection;
use serde_json::{Value, json};

use crate::services::agent::memory::unified::{self, Audience};
use myriad_merope::others::{self as rules, ANSWERED_WITHIN_HOURS};
use myriad_merope::sore::Weight;

pub const SPOKE_UP: &str = "spoke_up";
/// Speaking up unasked is remembered this long.
const FADE_AFTER: chrono::Duration = chrono::Duration::days(30);
/// Speaking up unasked, as records: the ones nobody took up and the ones
/// someone did, at most this many each.
const SPOKE_UP_RECORDS: usize = 3;
const TOLD_RECORDS: usize = 6;
const ROWS: u64 = 200;

fn group_venue(venue: &str) -> String {
    Audience::group(venue, 0).venue()
}

/// She spoke up unasked in the group at `venue` (as sessions keep it),
/// saying `line`; whether anyone took it up.
pub async fn spoke_up(db: &DatabaseConnection, venue: &str, line: &str, taken: bool) {
    let evidence = json!({ "taken": taken }).to_string();
    if let Err(error) =
        unified::remember_in_venue(db, &group_venue(venue), line, &evidence, SPOKE_UP).await
    {
        tracing::warn!(%error, "[Merope] could not keep how speaking up went");
    }
}

fn taken(row: &crate::models::entities::agent_memories::Model) -> bool {
    row.evidence
        .as_deref()
        .and_then(|evidence| serde_json::from_str::<Value>(evidence).ok())
        .and_then(|evidence| evidence.get("taken").and_then(Value::as_bool))
        .unwrap_or(false)
}

/// Her speaking up unasked in the group at `venue` lately, oldest first:
/// when, and whether anyone took it up.
pub async fn spoke_up_lately(
    db: &DatabaseConnection,
    venue: &str,
    limit: u64,
) -> Vec<(DateTime<Utc>, bool)> {
    let mut lately: Vec<(DateTime<Utc>, bool)> =
        unified::latest_in_venue(db, &group_venue(venue), SPOKE_UP, limit)
            .await
            .unwrap_or_default()
            .iter()
            .map(|row| (row.created_at.with_timezone(&Utc), taken(row)))
            .collect();
    lately.reverse();
    lately
}

pub async fn let_fade(db: &DatabaseConnection) {
    let _ = unified::fade_source(db, SPOKE_UP, FADE_AFTER).await;
}

/// A record for her story: when, the row it rests on, the line, and whether
/// it did not go well.
pub(super) type Found = (DateTime<FixedOffset>, String, String, bool);

/// How others took her since `since`, as records, and the counts.
pub(super) async fn records(
    db: &DatabaseConnection,
    since: DateTime<FixedOffset>,
) -> (Vec<Found>, Value) {
    let mut found: Vec<Found> = Vec::new();

    // Writing first: counted, never who.
    let first = super::store::first_words(
        db,
        super::reach::EVENT_KEY,
        None,
        since.with_timezone(&Utc),
        ANSWERED_WITHIN_HOURS,
        ROWS as i64,
    )
    .await
    .unwrap_or_default();
    let settled: Vec<bool> = first
        .iter()
        .filter_map(|(_, _, answered)| *answered)
        .collect();
    let answered = settled.iter().filter(|answered| **answered).count();
    if let (Some((line, missed)), Some((_, at, _))) =
        (rules::wrote_first(settled.len(), answered), first.first())
    {
        found.push((at.fixed_offset(), "first_words".to_string(), line, missed));
    }

    // Speaking up unasked: counted, with a few of what she said.
    let rows: Vec<_> = unified::with_history(db, &[SPOKE_UP], ROWS)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|row| row.created_at >= since)
        .collect();
    let taken_up = rows.iter().filter(|row| taken(row)).count();
    for took in [false, true] {
        for row in rows
            .iter()
            .rev()
            .filter(|row| taken(row) == took)
            .take(SPOKE_UP_RECORDS)
        {
            let (line, missed) = rules::spoke_up(&row.content, took);
            found.push((row.created_at, row.id.clone(), line, missed));
        }
    }

    // What people told her about herself.
    let told: Vec<_> = unified::with_history(db, &[super::making_sense::SOURCE], ROWS)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|row| row.created_at >= since)
        .collect();
    for row in told.iter().rev().take(TOLD_RECORDS) {
        found.push((
            row.created_at,
            row.id.clone(),
            rules::told(&row.content),
            false,
        ));
    }

    // Being hurt, and letting go: counted, never what or who.
    let sores = unified::with_history(db, &[super::sore::SOURCE], ROWS)
        .await
        .unwrap_or_default();
    let mut weights = [0usize; 3];
    let mut let_go = 0;
    let mut latest: Option<(DateTime<FixedOffset>, String)> = None;
    for row in &sores {
        let evidence = row
            .evidence
            .as_deref()
            .and_then(|evidence| serde_json::from_str::<Value>(evidence).ok())
            .unwrap_or_default();
        let began = evidence
            .get("since")
            .and_then(Value::as_str)
            .and_then(|at| DateTime::parse_from_rfc3339(at).ok());
        // Mending keeps it as a new row with the same beginning: the first
        // of those is when it happened.
        if began.is_some_and(|began| began >= since)
            && row
                .created_at
                .signed_duration_since(began.unwrap_or(row.created_at))
                < chrono::Duration::minutes(1)
        {
            let weight = evidence
                .get("weight")
                .and_then(Value::as_str)
                .and_then(Weight::parse);
            match weight {
                Some(Weight::Petty) => weights[0] += 1,
                Some(Weight::Hurt) => weights[1] += 1,
                Some(Weight::Deep) => weights[2] += 1,
                None => continue,
            }
            latest = Some((row.created_at, row.id.clone()));
        }
        if row.invalid_reason.as_deref() == Some("forgiven")
            && row.invalid_at.is_some_and(|at| at >= since)
        {
            let_go += 1;
            latest = Some((row.invalid_at.unwrap_or(row.created_at), row.id.clone()));
        }
    }
    if let (Some((line, missed)), Some((at, row))) = (
        rules::hurt(weights[0], weights[1], weights[2], let_go),
        latest,
    ) {
        found.push((at, row, line, missed));
    }

    let tally = json!({
        "wroteFirst": { "times": settled.len(), "answered": answered },
        "spokeUpUnasked": { "times": rows.len(), "takenUp": taken_up },
        "toldAboutYourself": told.len(),
        "hurt": { "small": weights[0], "hurt": weights[1], "deep": weights[2], "letGo": let_go },
    });
    (found, tally)
}
