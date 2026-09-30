//! Her days with each person and in each group (see
//! `myriad_merope::chat_days`): a line for each day, and looking back
//! through them to the right days before reading what was said.
//!
//! With one person the lines are written here at night, kept with them,
//! heard only in private with them, and apart from ordinary memory: they are
//! the table of days, not things she recalls on her own. A night writes
//! yesterday's and fills in a few older days that have none yet. In a group
//! the table is the group's own days (see `bits`), and looking back reads
//! only that group's talk.

use std::collections::BTreeMap;
use std::time::Duration;

use chrono::{DateTime, FixedOffset, Local, NaiveDate};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::{Value, json};

use super::call::{self, Voice};
use crate::services::agent::memory::unified::{self, Audience};
use myriad_merope::chat_days::{
    DAY_SCHEMA_NAME, DayLine, PICK_SCHEMA_NAME, day_schema, day_system, lines_to_read, parse_day,
    parse_pick, pick_input, pick_schema, pick_system, stand_in,
};
use myriad_merope::remembering::Who;

pub const SOURCE: &str = "day_with";
/// Messages of theirs read back at most, newest first.
const LATEST: i64 = 3000;
/// A day this short has no line of its own; its first words stand in.
const FEW: usize = 4;
/// Days a night writes, per person, and how far back it fills in.
const PER_NIGHT: usize = 5;
const FILL_BACK_DAYS: i64 = 120;
/// People gone over per night.
const PEOPLE: i64 = 20;
const WRITE_TIMEOUT: Duration = Duration::from_secs(40);
/// Turning back is done while she answers: past this, she goes by words.
const PICK_TIMEOUT: Duration = Duration::from_secs(6);
const SHOWN_CHARS: usize = 240;

/// One message: when, whether hers, what (in a group, others' with who
/// said it).
type Said = (DateTime<FixedOffset>, bool, String);

/// Where the talk was: in private with one person, or in one group (its
/// venue as sessions keep it).
#[derive(Debug, Clone, Copy)]
pub enum Place<'a> {
    With(i32),
    In(&'a str),
}

/// The talk there, by the day it was on the host clock, oldest first.
async fn by_day(db: &DatabaseConnection, place: Place<'_>) -> BTreeMap<NaiveDate, Vec<Said>> {
    let (sql, value): (&str, sea_orm::Value) = match place {
        Place::With(user_id) => (
            "SELECT m.role, m.content, m.created_at, '' AS name FROM agent_messages m \
             JOIN agent_sessions s ON s.id = m.session_id \
             WHERE s.user_id = $1 AND s.context->>'mode' = 'chat' \
               AND (s.context->>'venue') IS NULL AND m.role IN ('user', 'assistant') \
             ORDER BY m.created_at DESC LIMIT $2",
            user_id.into(),
        ),
        Place::In(venue) => (
            "SELECT m.role, m.content, m.created_at, \
               coalesce(nullif(u.display_name, ''), u.username, '') AS name \
             FROM agent_messages m JOIN agent_sessions s ON s.id = m.session_id \
             LEFT JOIN users u ON u.id = s.user_id \
             WHERE s.context->>'mode' = 'chat' AND s.context->>'venue' = $1 \
               AND m.role IN ('user', 'assistant') \
             ORDER BY m.created_at DESC LIMIT $2",
            venue.into(),
        ),
    };
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            sql,
            [value, LATEST.into()],
        ))
        .await
        .unwrap_or_default();
    let mut days: BTreeMap<NaiveDate, Vec<Said>> = BTreeMap::new();
    for row in rows.iter().rev() {
        let (Ok(role), Ok(content), Ok(at)) = (
            row.try_get::<String>("", "role"),
            row.try_get::<String>("", "content"),
            row.try_get::<DateTime<FixedOffset>>("", "created_at"),
        ) else {
            continue;
        };
        let text = crate::services::agent::chat_prompt::chat_safe_content(&content);
        if text.trim().is_empty() {
            continue;
        }
        let name: String = row
            .try_get::<String>("", "name")
            .unwrap_or_default()
            .trim()
            .chars()
            .take(24)
            .collect();
        let text = match (&place, role == "assistant") {
            (Place::In(_), false) if !name.is_empty() => format!("{name}：{text}"),
            _ => text,
        };
        days.entry(at.with_timezone(&Local).date_naive())
            .or_default()
            .push((at, role == "assistant", text));
    }
    days
}

/// The lines she wrote for their days, by day.
async fn written(
    db: &DatabaseConnection,
    user_id: i32,
) -> BTreeMap<NaiveDate, crate::models::entities::agent_memories::Model> {
    unified::venue_source_rows(db, Some(user_id), "private", SOURCE, 1000)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter_map(|row| {
            let day = row
                .evidence
                .as_deref()
                .and_then(|evidence| serde_json::from_str::<Value>(evidence).ok())
                .and_then(|evidence| {
                    NaiveDate::parse_from_str(evidence.get("day")?.as_str()?, "%Y-%m-%d").ok()
                })?;
            Some((day, row))
        })
        .collect()
}

/// The line for each day there that has one.
async fn day_lines(db: &DatabaseConnection, place: Place<'_>) -> BTreeMap<NaiveDate, String> {
    match place {
        Place::With(user_id) => written(db, user_id)
            .await
            .into_iter()
            .map(|(day, row)| (day, row.content))
            .collect(),
        Place::In(venue) => super::bits::days_in(db, venue, 1000)
            .await
            .into_iter()
            .filter_map(|(day, text)| {
                Some((NaiveDate::parse_from_str(&day, "%Y-%m-%d").ok()?, text))
            })
            .collect(),
    }
}

/// Every day of talk there, as she sees it turning back.
async fn table(
    db: &DatabaseConnection,
    place: Place<'_>,
) -> (Vec<DayLine>, BTreeMap<NaiveDate, Vec<Said>>) {
    let days = by_day(db, place).await;
    let lines = day_lines(db, place).await;
    let table = days
        .iter()
        .map(|(date, said)| DayLine {
            date: *date,
            messages: said.len(),
            about: lines.get(date).cloned().unwrap_or_else(|| {
                let theirs: Vec<&str> = said
                    .iter()
                    .filter(|(_, hers, _)| !hers)
                    .map(|(_, _, text)| text.as_str())
                    .collect();
                stand_in(&theirs)
            }),
        })
        .collect();
    (table, days)
}

/// Keep `line` as the line for `day` with them, written going over that
/// day at night (see `bits`); nothing when there is none or one is kept.
pub async fn keep_line(db: &DatabaseConnection, user_id: i32, day: NaiveDate, line: &str) {
    let line: String = line
        .trim()
        .chars()
        .take(myriad_merope::chat_days::DAY_CHARS)
        .collect();
    if line.is_empty() || written(db, user_id).await.contains_key(&day) {
        return;
    }
    keep(db, user_id, day, line).await;
}

async fn keep(db: &DatabaseConnection, user_id: i32, date: NaiveDate, line: String) {
    let kept = unified::remember(
        db,
        unified::NewMemory {
            user_id,
            kind: unified::MemoryKind::Fact,
            content: line,
            evidence: Some(json!({ "day": date.to_string() }).to_string()),
            speaker: unified::Speaker::Agent,
            source: SOURCE,
            audience: Audience::private(user_id),
            importance: 0.3,
            concepts: Vec::new(),
        },
    )
    .await;
    if let Err(error) = kept {
        tracing::warn!(%error, user_id, %date, "[Merope] could not keep a day's line");
    }
}

/// Write the line for one day with them.
async fn write_day(
    db: &DatabaseConnection,
    owner: i32,
    user_id: i32,
    date: NaiveDate,
    said: &[Said],
) {
    let conversation: Vec<Value> = said
        .iter()
        .map(|(_, hers, text)| {
            json!({
                "who": if *hers { "you" } else { "they" },
                "text": text.chars().take(300).collect::<String>(),
            })
        })
        .collect();
    let soul = crate::services::agent::identity::get_speaking_soul()
        .await
        .unwrap_or_default();
    let Ok(raw) = call::Ask::new(Voice::Judge, owner, DAY_SCHEMA_NAME)
        .within(WRITE_TIMEOUT)
        .json_raw(
            &day_system(&soul),
            &json!({ "conversation": conversation }).to_string(),
            DAY_SCHEMA_NAME,
            &day_schema(),
        )
        .await
    else {
        tracing::warn!(user_id, %date, "[Merope] could not write a day's line; tried again another night");
        return;
    };
    // A day she had nothing to say about keeps what they said first, so the
    // night does not come back to it again and again.
    let line = parse_day(&raw).unwrap_or_else(|| {
        let theirs: Vec<&str> = said
            .iter()
            .filter(|(_, hers, _)| !hers)
            .map(|(_, _, text)| text.as_str())
            .collect();
        stand_in(&theirs)
    });
    if line.trim().is_empty() {
        return;
    }
    keep(db, user_id, date, line).await;
}

/// People she talked with in private lately.
async fn people(db: &DatabaseConnection) -> Vec<i32> {
    db.query_all_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT s.user_id FROM agent_messages m JOIN agent_sessions s ON s.id = m.session_id \
         WHERE s.user_id > 0 AND s.context->>'mode' = 'chat' AND (s.context->>'venue') IS NULL \
           AND m.role = 'user' AND m.created_at > NOW() - make_interval(days => $1) \
         GROUP BY s.user_id ORDER BY max(m.created_at) DESC LIMIT $2",
        [(FILL_BACK_DAYS as i32).into(), PEOPLE.into()],
    ))
    .await
    .unwrap_or_default()
    .iter()
    .filter_map(|row| row.try_get::<i32>("", "user_id").ok())
    .collect()
}

/// At night: yesterday's line with each person, and a few older days that
/// have none yet, newest first. Today is not over, so never today.
pub async fn go_over(db: &DatabaseConnection, owner: i32) {
    let today = Local::now().date_naive();
    let oldest = today - chrono::Duration::days(FILL_BACK_DAYS);
    for user_id in people(db).await {
        let days = by_day(db, Place::With(user_id)).await;
        let lines = written(db, user_id).await;
        let unwritten: Vec<(&NaiveDate, &Vec<Said>)> = days
            .iter()
            .rev()
            .filter(|(date, said)| {
                **date < today && **date >= oldest && said.len() >= FEW && !lines.contains_key(date)
            })
            .take(PER_NIGHT)
            .collect();
        for (date, said) in unwritten {
            write_day(db, owner, user_id, *date, said).await;
        }
    }
}

/// Looking back through the days there for what `looking_for` names (they
/// just said `asked`): turn to the days worth reading, then read them; a
/// day with nothing in it that answers gives what it was about. Choosing is
/// billed to `payer`. What was found, oldest first: (date, whose, text).
pub async fn turn_back(
    db: &DatabaseConnection,
    place: Place<'_>,
    payer: i32,
    looking_for: &str,
    asked: &str,
) -> Vec<(String, Who, String)> {
    let (table, days) = table(db, place).await;
    let today = Local::now().date_naive();
    // Today's talk is in front of her already.
    let table: Vec<DayLine> = table.into_iter().filter(|day| day.date < today).collect();
    if table.is_empty() {
        return Vec::new();
    }
    // Even among a few days, she turns only to those it could be.
    let Ok(raw) = call::Ask::new(Voice::Judge, payer, PICK_SCHEMA_NAME)
        .within(PICK_TIMEOUT)
        .json_raw(
            &pick_system(),
            &pick_input(asked, looking_for, today, &table),
            PICK_SCHEMA_NAME,
            &pick_schema(),
        )
        .await
    else {
        return Vec::new();
    };
    let picked = parse_pick(&raw, &table).unwrap_or_default();
    let mut found = Vec::new();
    for date in picked {
        let Some(said) = days.get(&date) else {
            continue;
        };
        // The same message kept twice is read once.
        let mut seen = std::collections::HashSet::new();
        let said: Vec<&Said> = said
            .iter()
            .filter(|(_, _, text)| text.trim() != asked.trim())
            .filter(|(_, hers, text)| seen.insert((*hers, text.trim().to_string())))
            .collect();
        let texts: Vec<&str> = said.iter().map(|(_, _, text)| text.as_str()).collect();
        let read = lines_to_read(&texts, looking_for);
        if read.is_empty() {
            if let Some(day) = table.iter().find(|day| day.date == date) {
                found.push((date.to_string(), Who::TheDay, day.about.clone()));
            }
            continue;
        }
        for index in read {
            let (_, hers, text) = said[index];
            found.push((
                date.to_string(),
                if *hers { Who::You } else { Who::They },
                myriad_merope::remembering::excerpt(text, looking_for, SHOWN_CHARS),
            ));
        }
    }
    found.sort_by(|left, right| left.0.cmp(&right.0));
    found
}

#[cfg(test)]
mod live {
    use super::*;

    /// On the site's own chat, read only: the table of days as it stands,
    /// the lines she would write for the last few (printed, never kept),
    /// and what turning back finds for MEROPE_LOOK_FOR (`;`-separated).
    #[tokio::test]
    #[ignore = "reads the site's database and asks its model"]
    async fn turning_back_on_the_site() {
        let db = crate::services::agent::semantic_eval::load_configured_lite().await;
        crate::services::process_db::set_process_database(db.clone());
        let user_id: i32 = std::env::var("MEROPE_LOOK_USER")
            .ok()
            .and_then(|id| id.parse().ok())
            .unwrap_or(1);
        let venue = std::env::var("MEROPE_LOOK_VENUE").ok();
        let place = match venue.as_deref() {
            Some(venue) => Place::In(venue),
            None => Place::With(user_id),
        };
        let (table, days) = table(&db, place).await;
        for day in &table {
            println!("{} {:>3}  {}", day.date, day.messages, day.about);
        }
        for (date, said) in days
            .iter()
            .rev()
            .filter(|(_, said)| said.len() >= FEW)
            .take(3)
        {
            let conversation: Vec<Value> = said
                .iter()
                .map(|(_, hers, text)| json!({ "who": if *hers { "you" } else { "they" }, "text": text }))
                .collect();
            let soul = crate::services::agent::identity::get_speaking_soul()
                .await
                .unwrap_or_default();
            let raw = call::Ask::new(Voice::Judge, user_id, DAY_SCHEMA_NAME)
                .within(WRITE_TIMEOUT)
                .json_raw(
                    &day_system(&soul),
                    &json!({ "conversation": conversation }).to_string(),
                    DAY_SCHEMA_NAME,
                    &day_schema(),
                )
                .await
                .unwrap_or_default();
            println!("line for {date}: {:?}", parse_day(&raw));
        }
        let asked = std::env::var("MEROPE_LOOK_FOR").unwrap_or_default();
        for question in asked.split(';').filter(|q| !q.trim().is_empty()) {
            println!("== {question}");
            for (date, hers, text) in turn_back(&db, place, user_id, question, question).await {
                println!("  by day   [{date}] {:?}: {text}", hers);
            }
            for (date, hers, text) in
                super::super::remembering::look_back(&db, user_id, question, question, 3).await
            {
                println!("  in all   [{date}] {:?}: {text}", hers);
            }
        }
    }
}
