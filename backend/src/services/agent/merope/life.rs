//! Her nights: once a day, while the site sleeps, she looks back.
//!
//! - **Her own day.** A few first-person lines about yesterday, written from
//!   material that names no one: how many people she talked with, how the
//!   work she did went, how often she spoke up on her own. They become her
//!   own memory, shared by every conversation, so her life has a past that
//!   grew out of what actually happened rather than a backstory. Because
//!   every audience hears it, nothing about any one person goes in.
//! - **Her views.** She goes over what she did on her own lately and lets
//!   views of her own grow or change (see `views`).
//! - **What she wonders.** From what she did this past week, the questions
//!   she would like to go and find out (see `explore`).
//! - **Who she has been.** Once a week she looks back over what she did and
//!   writes who she has been lately, from those records alone (see
//!   `self_story`).
//! - **Filling in old memories.** Memories kept before concepts existed get
//!   their concepts, one person at a time, so association can reach them.
//!   One person's memories never share a model call with another's.
//!
//! Model calls are billed to the site owner; without one, the night passes.

use chrono::{Datelike, Duration, NaiveDate, TimeZone, Timelike};
use sea_orm::DatabaseConnection;
use serde_json::json;

use crate::services::agent::memory::unified;
use myriad_merope::life::{
    CONCEPTS_SCHEMA, CONCEPTS_SYSTEM, DAY_SCHEMA, DayFacts, Filled, MAX_DAY_CHARS, concepts_schema,
    day_label, own_day_prompt,
};

/// Her night, on the host clock like the do-not-disturb window.
const NIGHT: std::ops::RangeInclusive<u32> = 3..=5;
/// Memories filled in per person per night, and people per night.
const FILL_PER_PERSON: u64 = 20;
const FILL_PEOPLE: u64 = 10;
/// The day whose bits were last gone over, so a night does it once.
static BITS_DONE: std::sync::LazyLock<std::sync::Mutex<Option<NaiveDate>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(None));

/// Days a missed night can still be written for.
const BACKFILL_DAYS: u64 = 3;

pub async fn tick(db: DatabaseConnection) {
    let now = chrono::Local::now();
    if !NIGHT.contains(&now.hour()) || !super::is_enabled().await {
        return;
    }
    let Some(owner) = super::call::site_owner().await else {
        return;
    };
    // Yesterday, and any day just before it a missed night left unwritten
    // between days she did write (never days before she had any).
    for back in (1..=BACKFILL_DAYS).rev() {
        let Some(day) = now.date_naive().checked_sub_days(chrono::Days::new(back)) else {
            continue;
        };
        let Ok((written, before)) = unified::own_day_written(&db, day).await else {
            continue;
        };
        if !written && (back == 1 || before) {
            write_yesterday(&db, owner, day).await;
        }
    }
    super::views::go_over(&db, owner).await;
    super::self_story::look_back(&db, owner).await;
    super::explore::wonder(&db, owner).await;
    // Yesterday with each person, once a night.
    if let Some((start, end)) = now.date_naive().pred_opt().and_then(day_bounds) {
        if BITS_DONE
            .lock()
            .is_ok_and(|done| *done != Some(start.date_naive()))
        {
            if let Ok(mut done) = BITS_DONE.lock() {
                *done = Some(start.date_naive());
            }
            super::bits::go_over(&db, owner, start, end).await;
            // A group's joke that keeps coming back may become its sticker.
            super::stickers::for_group_jokes(&db, owner).await;
        }
    }
    super::views::let_fade(&db).await;
    super::strangers::let_fade(&db).await;
    super::threads::let_fade(&db).await;
    super::sore::let_fade(&db).await;
    super::making_sense::let_fade(&db).await;
    fill_old_concepts(&db, owner).await;
}

fn day_bounds(
    day: NaiveDate,
) -> Option<(
    chrono::DateTime<chrono::FixedOffset>,
    chrono::DateTime<chrono::FixedOffset>,
)> {
    let start = chrono::Local
        .from_local_datetime(&day.and_hms_opt(0, 0, 0)?)
        .earliest()?;
    let end = start + Duration::days(1);
    Some((start.fixed_offset(), end.fixed_offset()))
}

async fn day_facts(db: &DatabaseConnection, day: NaiveDate) -> Option<DayFacts> {
    let (start, end) = day_bounds(day)?;
    // Whoever spoke to her last on that day; people who came back later
    // count on the later day. An undercount, never a name.
    let people_talked_with = super::store::people_last_talked_between(db, start, end)
        .await
        .ok()?;
    let work_done = super::store::work_ended_between(db, "completed", start, end)
        .await
        .ok()?;
    let work_failed = super::store::work_ended_between(db, "failed", start, end)
        .await
        .ok()?;
    let spoke_up_unprompted = super::store::spoke_up_between(db, start, end).await.ok()?;
    Some(DayFacts {
        people_talked_with,
        work_done,
        work_failed,
        spoke_up_unprompted,
    })
}

async fn write_yesterday(db: &DatabaseConnection, owner: i32, day: NaiveDate) {
    let Some(facts) = day_facts(db, day).await else {
        return;
    };
    let Ok(model) = super::call::Ask::new(super::call::Voice::HersAtLength, owner, DAY_SCHEMA)
        .within(std::time::Duration::from_secs(60))
        .model()
        .await
    else {
        return;
    };
    let soul = crate::services::agent::identity::get_speaking_soul()
        .await
        .unwrap_or_default();
    // Oldest first, as she would reread them.
    let earlier: Vec<String> = unified::own_days(db, 3)
        .await
        .unwrap_or_default()
        .into_iter()
        .rev()
        .map(|entry| entry.content)
        .collect();
    let on_your_own = match day_bounds(day) {
        Some((start, end)) => super::doing::during(db, start, end, 8).await,
        None => Vec::new(),
    };
    let input = json!({
        "day": day.weekday().to_string(),
        "dayFacts": facts,
        "onYourOwn": on_your_own,
        "earlierEntries": earlier,
    })
    .to_string();
    let written = model.text(&own_day_prompt(&soul), &input).await;
    let Ok(text) = written else {
        return;
    };
    let text: String = super::ingest::compact_summary(&text)
        .chars()
        .take(MAX_DAY_CHARS)
        .collect();
    match unified::write_own_day(db, day, &text).await {
        Ok(true) => tracing::info!(%day, "[Merope] kept a day of her own"),
        Ok(false) => {}
        Err(error) => tracing::warn!(%error, "[Merope] could not keep her day"),
    }
}

async fn fill_old_concepts(db: &DatabaseConnection, owner: i32) {
    let Ok(people) = unified::people_without_concepts(db, FILL_PEOPLE).await else {
        return;
    };
    for user_id in people {
        let Ok(memories) = unified::without_concepts(db, user_id, FILL_PER_PERSON).await else {
            continue;
        };
        if memories.is_empty() {
            continue;
        }
        let Ok(model) = super::call::Ask::new(super::call::Voice::Judge, owner, CONCEPTS_SCHEMA)
            .within(std::time::Duration::from_secs(60))
            .model()
            .await
        else {
            return;
        };
        let input = json!({
            "facts": memories
                .iter()
                .map(|memory| json!({ "id": memory.id, "fact": memory.content }))
                .collect::<Vec<_>>()
        })
        .to_string();
        let schema = concepts_schema();
        let raw = model
            .json(CONCEPTS_SYSTEM, &input, CONCEPTS_SCHEMA, &schema)
            .await;
        let Some(filled) = raw.ok().and_then(|raw| super::call::parse::<Filled>(&raw)) else {
            continue;
        };
        let asked: std::collections::HashSet<&str> =
            memories.iter().map(|memory| memory.id.as_str()).collect();
        let mut count = 0;
        for memory in filled.memories {
            // Only the ids we asked about, and only this person's.
            if asked.contains(memory.id.as_str())
                && unified::fill_concepts(db, user_id, &memory.id, memory.concepts)
                    .await
                    .unwrap_or(false)
            {
                count += 1;
            }
        }
        tracing::info!(user_id, count, "[Merope] filled concepts into old memories");
    }
}

/// The diary call as production sends it, for the semantic suite.
#[cfg(test)]
pub(crate) fn own_day_probe_contract(soul: &str) -> String {
    own_day_prompt(soul)
}

/// Her latest days for the speaking prompt, oldest first.
/// Each line says which day it was: undated lines read as one blur, and in
/// testing the model told an older day as the latest.
pub async fn recent_days(db: &DatabaseConnection, limit: u64) -> Vec<String> {
    let today = chrono::Local::now().date_naive();
    let mut days: Vec<String> = unified::own_days(db, limit)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|day| {
            let ago = (today - day.created_at.date_naive()).num_days();
            format!("{}: {}", day_label(ago), day.content)
        })
        .collect();
    days.reverse();
    days
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_day_runs_midnight_to_midnight() {
        let day = NaiveDate::from_ymd_opt(2026, 9, 24).unwrap();
        let (start, end) = day_bounds(day).unwrap();
        assert_eq!(end - start, Duration::days(1));
    }
}
