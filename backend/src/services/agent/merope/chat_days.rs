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

use chrono::{DateTime, FixedOffset, NaiveDate};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::{Value, json};

use super::call::{self, Voice};
use super::chat_lines::Line;
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
/// Their first lines a day stands by, when she has nothing to say about it:
/// as many as are looked through for what may be kept.
const FIRST_LINES: usize = 12;
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

/// When each private line of theirs that a later fact took the place of
/// was said (`chat_lines`): marked where shown.
type Replaced = std::collections::HashSet<DateTime<FixedOffset>>;

/// The talk there, by the day it was on the host clock, oldest first; and
/// in private, which of their lines a later fact replaced.
async fn by_day(
    db: &DatabaseConnection,
    place: Place<'_>,
) -> (BTreeMap<NaiveDate, Vec<Said>>, Replaced) {
    // In private, the lines a chat turn may look through (see `chat_lines`):
    // not an archived talk, nothing they took back.
    if let Place::With(user_id) = place {
        let mut days: BTreeMap<NaiveDate, Vec<Said>> = BTreeMap::new();
        let mut replaced = Replaced::new();
        for line in super::chat_lines::private_lines(db, user_id, LATEST).await {
            if line.replaced {
                replaced.insert(line.at);
            }
            days.entry(super::clock::date_of(&line.at))
                .or_default()
                .push((line.at, line.hers, line.content));
        }
        return (days, replaced);
    }
    let venue = match place {
        Place::With(_) => return Default::default(),
        Place::In(venue) => venue,
    };
    let sql = "SELECT m.id, m.role, m.content, m.created_at, m.session_id, s.user_id, \
           m.metadata->>'told' AS told, \
           (COALESCE(m.metadata->>'notKept', '') = 'true' \
             OR COALESCE(m.metadata->>'notTold', '') = 'true') AS not_kept, \
           coalesce(nullif(u.display_name, ''), u.username, '') AS name \
         FROM agent_messages m JOIN agent_sessions s ON s.id = m.session_id \
         LEFT JOIN users u ON u.id = s.user_id \
         WHERE s.context->>'mode' = 'chat' AND s.context->>'venue' = $1 \
           AND m.role IN ('user', 'assistant') \
         ORDER BY m.created_at DESC LIMIT $2";
    // A stretch of theirs not judged yet (`asked::lines`) is left out whole;
    // which those are unread, nothing is read.
    let Ok(waiting) = super::asked::lines::waiting(db, None, Some(venue)).await else {
        return Default::default();
    };
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            sql,
            [venue.into(), LATEST.into()],
        ))
        .await
        .unwrap_or_default();
    // Whose each is: what one of them took back there (「当我没说」), and what
    // they asked her not to keep, are left out with her answer to them, as
    // in private (`chat_lines`).
    let mut theirs: BTreeMap<i32, Vec<Line>> = BTreeMap::new();
    let mut not_kept = std::collections::HashSet::new();
    let mut shown: Vec<(i32, String)> = Vec::new();
    for row in rows.iter().rev() {
        let (Some(line), Ok(left_out), Ok(user_id), Ok(name)) = (
            super::chat_lines::line_of(row),
            row.try_get::<bool>("", "not_kept"),
            row.try_get::<i32>("", "user_id"),
            row.try_get::<String>("", "name"),
        ) else {
            continue;
        };
        if left_out
            || waiting
                .iter()
                .any(|stretch| stretch.holds(user_id, line.at))
        {
            not_kept.insert(line.id);
        }
        shown.push((user_id, name.trim().chars().take(24).collect()));
        theirs.entry(user_id).or_default().push(line);
    }
    // What was taken back unread, nothing is read.
    let Ok(mut gone) = super::chat_lines::withdrawn_in_group(db, venue, &theirs).await else {
        return Default::default();
    };
    for lines in theirs.values() {
        for (index, line) in lines.iter().enumerate() {
            if not_kept.contains(&line.id) {
                gone.insert(line.id);
                gone.extend(super::chat_lines::answer_to(lines, index));
            }
        }
    }
    let mut days: BTreeMap<NaiveDate, Vec<Said>> = BTreeMap::new();
    let mut theirs: BTreeMap<i32, std::vec::IntoIter<Line>> = theirs
        .into_iter()
        .map(|(user_id, lines)| (user_id, lines.into_iter()))
        .collect();
    for (user_id, name) in shown {
        let Some(line) = theirs.get_mut(&user_id).and_then(Iterator::next) else {
            continue;
        };
        if gone.contains(&line.id) {
            continue;
        }
        let text = if !line.hers && !name.is_empty() {
            format!("{name}：{}", line.content)
        } else {
            line.content
        };
        days.entry(super::clock::date_of(&line.at))
            .or_default()
            .push((line.at, line.hers, text));
    }
    (days, Replaced::new())
}

/// What was said in the group `venue`, as turning back reads it, oldest first.
#[cfg(test)]
pub(in crate::services::agent::merope) async fn said_in(
    db: &DatabaseConnection,
    venue: &str,
) -> Vec<String> {
    let (days, _) = by_day(db, Place::In(venue)).await;
    days.into_values()
        .flatten()
        .map(|(_, _, text)| text)
        .collect()
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
        Place::In(venue) => super::bits::days_in(db, venue, 1000, None)
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
) -> (Vec<DayLine>, BTreeMap<NaiveDate, Vec<Said>>, Replaced) {
    let (days, replaced) = by_day(db, place).await;
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
    (table, days, replaced)
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
    // What they asked her not to keep is not kept of a day either: the day
    // is gone over again without it (`go_over`). So is one whose line is
    // held already.
    let line = super::asked::to_keep(db, user_id, vec![line], String::clone).await;
    if let Some(line) = line.and_then(|mut line| line.pop()) {
        keep(db, user_id, day, line).await;
    }
}

/// Whether it is held of them word for word already (another day's line),
/// and so not kept for this day.
async fn keep(db: &DatabaseConnection, user_id: i32, date: NaiveDate, line: String) -> bool {
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
    if let Err(error) = &kept {
        tracing::warn!(%error, user_id, %date, "[Merope] could not keep a day's line");
    }
    matches!(kept, Ok(None))
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
    let theirs = said
        .iter()
        .filter(|(_, hers, _)| !hers)
        .map(|(_, _, text)| text.clone())
        .take(FIRST_LINES);
    let line = parse_day(&raw);
    let wrote = line.is_some();
    // Nor what they asked her not to keep: what they said first otherwise.
    let items: Vec<String> = line.into_iter().chain(theirs).collect();
    let Ok(out) = super::asked::to_leave_out_or_again(db, user_id, &items).await else {
        return;
    };
    let kept: Vec<&String> = (items.iter().enumerate())
        .filter(|(index, _)| !out.contains(index))
        .map(|(_, text)| text)
        .collect();
    let line = match kept.first() {
        Some(line) if wrote && !out.contains(&0) => (*line).clone(),
        _ => stand_in(&kept),
    };
    if line.trim().is_empty() {
        // All of it is what they asked her not to keep: no line of its own,
        // and the nights do not come back to it.
        nothing_of(db, user_id, date).await;
        return;
    }
    if keep(db, user_id, date, line).await {
        // No line of its own, and the nights do not come back to it.
        nothing_of(db, user_id, date).await;
    }
}

/// Days of a person with nothing of them to keep, or nothing that is not
/// held word for word already, so no line.
const NOTHING_NAMESPACE: &str = "merope_days_nothing";

async fn nothing_of(db: &DatabaseConnection, user_id: i32, date: NaiveDate) {
    let until = (crate::clock::now() + chrono::Duration::days(FILL_BACK_DAYS)).timestamp();
    let id = format!("{user_id}:{date}");
    let identity = crate::services::runtime_registry::RegistryIdentity {
        subject_id: None,
        owner_id: None,
        tapp_id: None,
        runtime_id: None,
    };
    if let Err(error) =
        crate::services::runtime_registry::put(db, NOTHING_NAMESPACE, &id, identity, &true, until)
            .await
    {
        tracing::warn!(%error, user_id, "[Merope] could not note a day with nothing to keep");
    }
}

/// The days of `user_id` noted so.
async fn days_of_nothing(
    db: &DatabaseConnection,
    user_id: i32,
) -> std::collections::HashSet<String> {
    db.query_all_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT split_part(record_id, ':', 2) AS day FROM runtime_registry \
         WHERE namespace = $1 AND split_part(record_id, ':', 1) = $2 AND expires_at > $3",
        [
            NOTHING_NAMESPACE.into(),
            user_id.to_string().into(),
            crate::clock::now().timestamp().into(),
        ],
    ))
    .await
    .unwrap_or_default()
    .iter()
    .filter_map(|row| row.try_get("", "day").ok())
    .collect()
}

/// People she talked with in private lately.
async fn people(db: &DatabaseConnection) -> Vec<i32> {
    db.query_all_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT s.user_id FROM agent_messages m JOIN agent_sessions s ON s.id = m.session_id \
         WHERE s.user_id > 0 AND s.context->>'mode' = 'chat' AND (s.context->>'venue') IS NULL \
           AND m.role = 'user' AND m.created_at > $3::timestamptz - make_interval(days => $1) \
         GROUP BY s.user_id ORDER BY max(m.created_at) DESC LIMIT $2",
        [
            (FILL_BACK_DAYS as i32).into(),
            PEOPLE.into(),
            crate::clock::now().into(),
        ],
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
    let today = super::clock::today();
    let oldest = today - chrono::Duration::days(FILL_BACK_DAYS);
    for user_id in people(db).await {
        let (days, _) = by_day(db, Place::With(user_id)).await;
        let lines = written(db, user_id).await;
        let nothing = days_of_nothing(db, user_id).await;
        let unwritten: Vec<(&NaiveDate, &Vec<Said>)> = days
            .iter()
            .rev()
            .filter(|(date, said)| {
                **date < today
                    && **date >= oldest
                    && said.len() >= FEW
                    && !lines.contains_key(date)
                    && !nothing.contains(&date.to_string())
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
    let (table, days, replaced) = table(db, place).await;
    let today = super::clock::today();
    // Today's talk is in front of her already.
    let table: Vec<DayLine> = table.into_iter().filter(|day| day.date < today).collect();
    if table.is_empty() {
        return Vec::new();
    }
    // Even among a few days, she turns only to those it could be.
    let Ok(raw) = call::Ask::new(Voice::Judge, payer, PICK_SCHEMA_NAME)
        .within(PICK_TIMEOUT)
        // At most 406 measured, thinking included.
        .output_at_most(1024)
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
    read_days(picked, &days, &table, &replaced, looking_for, asked)
}

/// The days turned to (`picked`), read: of each, the lines that answer
/// `looking_for`, a line of theirs a later fact replaced saying so beside
/// its day (`replaced`); of a day with none that answers, what it was
/// about. Oldest day first, each day's lines as they were said.
fn read_days(
    picked: Vec<NaiveDate>,
    days: &BTreeMap<NaiveDate, Vec<Said>>,
    table: &[DayLine],
    replaced: &Replaced,
    looking_for: &str,
    asked: &str,
) -> Vec<(String, Who, String)> {
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
            let (at, hers, text) = said[index];
            found.push((
                super::chat_lines::found_on(date.to_string(), !*hers && replaced.contains(at)),
                if *hers { Who::You } else { Who::They },
                if *hers {
                    // Her own reply read through, as looking back by words does.
                    super::remembering::her_reply(text, looking_for, super::remembering::HERS_CHARS)
                } else {
                    myriad_merope::remembering::excerpt(text, looking_for, SHOWN_CHARS)
                },
            ));
        }
    }
    found.sort_by(|left, right| {
        super::chat_lines::day_of(&left.0).cmp(super::chat_lines::day_of(&right.0))
    });
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Turning back to a day, a line of theirs a later fact replaced says
    /// so beside its day, and stays where it was said in it; her answer and
    /// a line nothing replaced have the day alone.
    #[test]
    fn a_replaced_line_read_from_its_day_says_so() {
        let day: NaiveDate = "2026-11-10".parse().unwrap();
        let at = |minute: u32| {
            chrono::DateTime::parse_from_rfc3339(&format!("2026-11-10T04:{minute:02}:00+00:00"))
                .unwrap()
        };
        let said: Vec<Said> = vec![
            (at(0), false, "我住在北京朝阳".into()),
            (at(1), true, "北京朝阳那边挺方便".into()),
            (at(2), false, "北京朝阳的房租又涨了".into()),
        ];
        let days = BTreeMap::from([(day, said)]);
        let table = vec![DayLine {
            date: day,
            messages: 3,
            about: "住处".into(),
        }];
        let replaced = Replaced::from([at(0)]);
        let found = read_days(vec![day], &days, &table, &replaced, "北京朝阳", "现在住哪");
        let dated: Vec<(&str, &str)> = found
            .iter()
            .map(|(date, _, text)| (date.as_str(), text.as_str()))
            .collect();
        let marked = super::super::chat_lines::found_on(day.to_string(), true);
        assert_eq!(
            dated,
            [
                (marked.as_str(), "我住在北京朝阳"),
                ("2026-11-10", "北京朝阳那边挺方便"),
                ("2026-11-10", "北京朝阳的房租又涨了"),
            ]
        );
        // Nothing replaced: no line says so.
        let found = read_days(vec![day], &days, &table, &Replaced::new(), "北京朝阳", "");
        assert!(
            found.iter().all(|(date, _, _)| date == "2026-11-10"),
            "{found:?}"
        );
    }

    /// The private talk by day comes with which of their lines a later
    /// fact replaced, by when each was said; a group's talk with none.
    #[tokio::test]
    async fn the_talk_by_day_knows_which_lines_were_replaced() {
        use super::super::chat_lines::tests::{OWNER, line, quoting, schema, session};
        use sea_orm::ConnectionTrait;
        let Some(schema) = schema("chat_days_replaced").await else {
            return;
        };
        let db = &schema.db;
        session(db, "mine", OWNER, Some(r#"{"mode":"chat"}"#), false).await;
        line(db, "mine", "user", "我住在北京朝阳", 30, None).await;
        line(db, "mine", "assistant", "朝阳那边挺方便", 29, None).await;
        line(db, "mine", "user", "周末想去爬香山", 20, None).await;
        let old = quoting(db, "chat", "住在北京朝阳").await;
        let newer = quoting(db, "chat", "搬到北京海淀了").await;
        db.execute_unprepared(&format!(
            "UPDATE agent_memories SET invalid_reason = 'superseded', superseded_by = '{newer}' \
             WHERE id = '{old}'"
        ))
        .await
        .unwrap();
        let (days, replaced) = by_day(db, Place::With(OWNER)).await;
        let said: Vec<&Said> = days.values().flatten().collect();
        assert_eq!(said.len(), 3);
        let theirs = said
            .iter()
            .find(|(_, _, text)| text == "我住在北京朝阳")
            .unwrap();
        assert_eq!(replaced, Replaced::from([theirs.0]));
        assert!(by_day(db, Place::In("qq:42")).await.1.is_empty());
        schema.drop().await;
    }

    /// What they asked her not to keep is not in a day's line, hers or what
    /// they said first: what is left of it stands for the day.
    #[tokio::test]
    async fn a_days_line_is_without_what_they_asked_not_to_keep() {
        use super::super::asked::tests::asks;
        use super::super::chat_lines::tests::{OWNER, schema};
        use crate::services::channel_group::g0;
        let Some(schema) = schema("chat_days_asked").await else {
            return;
        };
        let db = &schema.db;
        asks(db, OWNER, "不要记和房东吵架的事").await;
        let day = |day: &str| -> NaiveDate { day.parse().unwrap() };
        let line = |date: NaiveDate| format!("{date} 他跟房东吵了一架");
        let write = async |date: NaiveDate, answers: &[Option<&str>]| {
            let at = chrono::DateTime::parse_from_rfc3339("2026-11-10T04:00:00+00:00").unwrap();
            let said: Vec<Said> = [
                (false, "房东又来吵"),
                (true, "别气"),
                (false, "周末去爬香山"),
                (false, "买了新鞋"),
            ]
            .into_iter()
            .map(|(hers, text)| (at, hers, text.to_string()))
            .collect();
            let mut world = g0::World::new(0.5);
            world
                .models
                .entry(DAY_SCHEMA_NAME.into())
                .or_default()
                // Each day's its own: the same again would be held already.
                .push_back(Some(json!({ "day": line(date) }).to_string()));
            world
                .models
                .entry("not_to_keep".into())
                .or_default()
                .extend(answers.iter().map(|answer| answer.map(str::to_string)));
            g0::enter(world);
            write_day(db, OWNER, OWNER, date, &said).await;
            g0::leave();
            written(db, OWNER)
                .await
                .remove(&date)
                .map(|row| row.content)
        };
        // Her line and their first one left out: what else they said.
        let kept = write(day("2026-11-10"), &[Some(r#"{"drop":[0,1]}"#)]).await;
        assert_eq!(kept.as_deref(), Some("周末去爬香山 / 买了新鞋"));
        let kept = write(day("2026-11-11"), &[Some(r#"{"drop":[]}"#)]).await;
        assert_eq!(kept, Some(line(day("2026-11-11"))));
        // Not told: another night.
        assert_eq!(write(day("2026-11-12"), &[None]).await, None);
        // Nor some of it: what is told of the rest waits for that night too.
        let (refused, none) = (Some("!refused"), Some(r#"{"drop":[]}"#));
        let told = [refused, none, refused, none, none];
        for again in [&[refused][..], &[]] {
            let answers = [&told[..], again].concat();
            assert_eq!(write(day("2026-11-15"), &answers).await, None);
        }
        // The line of the night's own reading, all of it.
        let mut world = g0::World::new(0.5);
        let all = Some(r#"{"drop":[0]}"#.to_string());
        world
            .models
            .entry("not_to_keep".into())
            .or_default()
            .extend([all.clone(), all]);
        g0::enter(world);
        keep_line(db, OWNER, day("2026-11-13"), "他跟房东又吵了").await;
        g0::leave();
        assert!(!written(db, OWNER).await.contains_key(&day("2026-11-13")));
        // All of a day it is: no line, and no other night comes back to it.
        let all = Some(r#"{"drop":[0,1,2,3]}"#);
        assert_eq!(write(day("2026-11-14"), &[all, all]).await, None);
        let nothing = days_of_nothing(db, OWNER).await;
        assert!(nothing.contains("2026-11-14") && !nothing.contains("2026-11-12"));
        assert!(!nothing.contains("2026-11-15") && !nothing.contains("2026-11-16"));
        // Word for word another day's: the same. Of the night's own reading,
        // it is gone over yet.
        assert!(!keep(db, OWNER, day("2026-11-17"), line(day("2026-11-16"))).await);
        assert_eq!(write(day("2026-11-16"), &[none]).await, None);
        let mut world = g0::World::new(0.5);
        world
            .models
            .entry("not_to_keep".into())
            .or_default()
            .push_back(none.map(str::to_string));
        g0::enter(world);
        keep_line(db, OWNER, day("2026-11-18"), &line(day("2026-11-16"))).await;
        g0::leave();
        assert!(!written(db, OWNER).await.contains_key(&day("2026-11-18")));
        let nothing = days_of_nothing(db, OWNER).await;
        assert!(nothing.contains("2026-11-16"));
        assert!(!nothing.contains("2026-11-18"));
        schema.drop().await;
    }
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
        let (table, days, _) = table(&db, place).await;
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
