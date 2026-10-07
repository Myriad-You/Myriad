//! Looking into her, for the site's owner: who she is lately, what she wants
//! and thinks, what she holds about each person and each group, and how
//! each of those changed. Presence over weeks is what she is for; this is
//! where it can be watched, a little each week, rather than tested in a
//! moment. Read only; nothing here is said to anyone.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Duration, FixedOffset, Utc};
use sea_orm::DatabaseConnection;
use serde_json::{Value, json};

use crate::models::entities::agent_memories::Model as Row;
use crate::services::agent::memory::unified;

/// Rows looked through, newest kept.
const HISTORY: u64 = 5000;
/// Weeks her voice is followed over, and lines a week needs to say
/// anything.
const VOICE_WEEKS: i64 = 8;
const VOICE_LINES: usize = 30;

/// Her own voice week by week: how much her way of typing moved from the
/// week before, how much people's own did over the same weeks (to read hers
/// against), and how far hers was from theirs (see `myriad_merope::style`).
/// A voice of her own is steady and within people's range; one drifting a
/// lot, or far from everyone, is not.
async fn voice(db: &DatabaseConnection, now: DateTime<Utc>) -> Vec<Value> {
    use chrono::Datelike;
    use myriad_merope::style::{distance, profile};
    let since = (now - Duration::weeks(VOICE_WEEKS)).fixed_offset();
    let lines = super::store::chat_lines_since(db, since, 60_000)
        .await
        .unwrap_or_default();
    // Weeks from Monday, local.
    let week_of = |at: &DateTime<FixedOffset>| {
        let day = at.with_timezone(&chrono::Local).date_naive();
        day - chrono::Days::new(u64::from(day.weekday().num_days_from_monday()))
    };
    let mut weeks: BTreeMap<chrono::NaiveDate, (Vec<String>, Vec<String>)> = BTreeMap::new();
    for (at, line, hers) in lines {
        let week = weeks.entry(week_of(&at)).or_default();
        if hers {
            week.0.push(line);
        } else {
            week.1.push(line);
        }
    }
    let mut out = Vec::new();
    let (mut her_before, mut people_before) = (None, None);
    for (week, (hers, theirs)) in weeks {
        let enough = |lines: &[String]| lines.len() >= VOICE_LINES;
        let her = enough(&hers).then(|| profile(&hers));
        let people = enough(&theirs).then(|| profile(&theirs));
        let apart = |left: &Option<myriad_merope::style::Profile>,
                     right: &Option<myriad_merope::style::Profile>| {
            match (left, right) {
                (Some(left), Some(right)) => Some(distance(left, right)),
                _ => None,
            }
        };
        out.push(json!({
            "week": week.to_string(),
            "lines": hers.len(),
            "peopleLines": theirs.len(),
            "drift": apart(&her, &her_before),
            "peopleDrift": apart(&people, &people_before),
            "fromPeople": apart(&her, &people),
        }));
        if her.is_some() {
            her_before = her;
        }
        if people.is_some() {
            people_before = people;
        }
    }
    out
}

/// How far back closed threads and her own time are shown.
const LATELY: Duration = Duration::days(30);
const THIS_WEEK: Duration = Duration::days(7);

fn when(at: &DateTime<FixedOffset>) -> String {
    at.to_rfc3339()
}

fn evidence(row: &Row) -> Value {
    row.evidence
        .as_deref()
        .and_then(|evidence| serde_json::from_str(evidence).ok())
        .unwrap_or(Value::Null)
}

/// A row as a line in its history: what it said, when, and whether it
/// still holds (and if not, when and why it stopped).
fn entry(row: &Row) -> Value {
    json!({
        "text": row.content,
        "at": when(&row.created_at),
        "current": row.invalid_at.is_none(),
        "endedAt": row.invalid_at.as_ref().map(when),
        "endedWhy": row.invalid_reason,
    })
}

/// A replacement made only to update a row (a note added, marked mended):
/// not a change worth showing.
fn replaced(row: &Row) -> bool {
    row.invalid_reason.as_deref() == Some("superseded")
}

fn sore_entry(row: &Row, who: Option<&str>) -> Value {
    let evidence = evidence(row);
    let weight = evidence
        .get("weight")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| {
            if evidence.get("petty").and_then(Value::as_bool) == Some(true) {
                "petty".into()
            } else {
                "hurt".into()
            }
        });
    json!({
        "what": row.content,
        "weight": weight,
        "since": evidence.get("since").cloned().unwrap_or_else(|| json!(when(&row.created_at))),
        "mended": evidence.get("mended").cloned().unwrap_or(Value::Null),
        "where": if row.venue.starts_with("group:") { "group" } else { "private" },
        "who": who,
        "status": match row.invalid_reason.as_deref() {
            None => "open",
            Some("forgiven") => "let_go",
            Some(other) => other,
        },
        "endedAt": row.invalid_at.as_ref().map(when),
    })
}

pub async fn snapshot(db: &DatabaseConnection) -> Value {
    let now = Utc::now();
    let rows = unified::with_history(
        db,
        &[
            super::self_story::SOURCE,
            unified::OWN_VIEW,
            super::explore::QUESTION,
            super::wants::SOURCE,
            super::wants::ENDED,
            super::self_story::CORRECTED,
            "narrative",
            super::bits::US_SOURCE,
            super::sore::SOURCE,
            super::threads::SOURCE,
            super::bits::SOURCE,
            super::bits::DAY_SOURCE,
            super::bits::LANDS_SOURCE,
            super::chat_days::SOURCE,
            super::recognizing::SOURCE,
        ],
        HISTORY,
    )
    .await
    .unwrap_or_default();
    let of = |source: &'static str| rows.iter().filter(move |row| row.source == source);
    let recent = |row: &&Row| {
        row.invalid_at
            .is_none_or(|at| now - at.with_timezone(&Utc) < LATELY)
    };

    // --- her -----------------------------------------------------------------
    let wants: Vec<Value> = super::wants::open(db)
        .await
        .iter()
        .map(|want| {
            json!({
                "want": want.want,
                "why": want.why,
                "reach": want.reach.as_str(),
                "longing": want.longing,
                "since": want.since.to_rfc3339(),
                "notes": want.notes.iter().map(|(at, note)| json!({ "at": at.to_rfc3339(), "note": note })).collect::<Vec<_>>(),
            })
        })
        .collect();
    let doing: Vec<Value> = unified::own_experiences(db, 200)
        .await
        .unwrap_or_default()
        .iter()
        .filter(|row| now - row.created_at.with_timezone(&Utc) < THIS_WEEK)
        .filter_map(|row| {
            let mut view = super::doing::experience_view(row)?;
            view["at"] = json!(when(&row.created_at));
            view["text"] = json!(row.content);
            Some(view)
        })
        .collect();
    let her = json!({
        "selfStory": of(super::self_story::SOURCE).map(entry).collect::<Vec<_>>(),
        "wants": wants,
        "wantsEnded": of(super::wants::ENDED).map(entry).collect::<Vec<_>>(),
        // A view she changed her mind about stays, as what she used to think.
        "views": of(unified::OWN_VIEW).map(entry).collect::<Vec<_>>(),
        "questions": of(super::explore::QUESTION).filter(recent).map(entry).collect::<Vec<_>>(),
        "corrected": of(super::self_story::CORRECTED).map(entry).collect::<Vec<_>>(),
        "days": of("narrative").filter(|row| row.user_id.is_none()).map(entry).collect::<Vec<_>>(),
        "doingThisWeek": doing,
        "voice": voice(db, now).await,
        "pace": super::pace::week_view(db).await,
        "taste": super::doing::taste_view(db).await,
        "vitals": super::vitals::week(db).await,
    });

    // --- each person -------------------------------------------------------------
    let about_people = [
        super::bits::US_SOURCE,
        super::sore::SOURCE,
        super::threads::SOURCE,
    ];
    let mut people: BTreeSet<i32> = rows
        .iter()
        .filter(|row| about_people.contains(&row.source.as_str()))
        .filter_map(|row| row.user_id)
        .collect();
    people.extend(
        of(super::bits::SOURCE)
            .filter(|row| row.venue == "private")
            .filter_map(|row| row.user_id),
    );
    let mut names: BTreeMap<i32, String> = BTreeMap::new();
    for user_id in &people {
        names.insert(*user_id, super::resolve_addressee_label(db, *user_id).await);
    }
    let mut out_people: Vec<(DateTime<FixedOffset>, Value)> = Vec::new();
    for user_id in &people {
        let theirs = |row: &&Row| row.user_id == Some(*user_id);
        let latest = rows
            .iter()
            .filter(theirs)
            .map(|row| row.updated_at)
            .max()
            .unwrap_or_default();
        let (first, days) = super::speaking_context::acquaintance(db, *user_id)
            .await
            .unwrap_or((None, 0));
        let threads: Vec<Value> = of(super::threads::SOURCE)
            .filter(theirs)
            .filter(|row| !replaced(row))
            .filter(recent)
            .map(|row| {
                let evidence = evidence(row);
                json!({
                    "about": evidence.get("about"),
                    "then": row.content,
                    "due": evidence.get("due"),
                    "at": when(&row.created_at),
                    "current": row.invalid_at.is_none(),
                    "endedWhy": row.invalid_reason,
                })
            })
            .collect();
        let bits: Vec<Value> = of(super::bits::SOURCE)
            .filter(theirs)
            .filter(|row| row.venue == "private")
            .map(|row| {
                json!({
                    "handle": evidence(row).get("handle"),
                    "how": row.content,
                    "at": when(&row.created_at),
                    "current": row.invalid_at.is_none(),
                })
            })
            .collect();
        out_people.push((
            latest,
            json!({
                "id": user_id,
                "name": names.get(user_id),
                "firstTalked": first.map(|at| at.to_rfc3339()),
                "daysTalked": days,
                "us": of(super::bits::US_SOURCE).filter(theirs).map(entry).collect::<Vec<_>>(),
                "lands": of(super::bits::LANDS_SOURCE).filter(theirs).filter(|row| row.venue == "private").map(entry).collect::<Vec<_>>(),
                "chatDays": of(super::chat_days::SOURCE).filter(theirs).map(|row| {
                    json!({ "day": evidence(row).get("day"), "text": row.content, "current": row.invalid_at.is_none() })
                }).collect::<Vec<_>>(),
                "sore": of(super::sore::SOURCE).filter(theirs).filter(|row| !replaced(row)).map(|row| sore_entry(row, None)).collect::<Vec<_>>(),
                "threads": threads,
                "bits": bits,
            }),
        ));
    }
    out_people.sort_by(|left, right| right.0.cmp(&left.0));

    // --- each group --------------------------------------------------------------
    let venues: BTreeSet<String> = rows
        .iter()
        .filter(|row| {
            [
                super::bits::DAY_SOURCE,
                super::bits::SOURCE,
                super::bits::LANDS_SOURCE,
                super::sore::SOURCE,
                super::recognizing::SOURCE,
            ]
            .contains(&row.source.as_str())
        })
        .filter_map(|row| row.venue.strip_prefix("group:").map(str::to_string))
        .collect();
    let mut groups: Vec<(DateTime<FixedOffset>, Value)> = Vec::new();
    for venue in venues {
        let here = format!("group:{venue}");
        let in_it = |row: &&Row| row.venue == here;
        let latest = rows
            .iter()
            .filter(in_it)
            .map(|row| row.updated_at)
            .max()
            .unwrap_or_default();
        let mut sores = Vec::new();
        for row in of(super::sore::SOURCE)
            .filter(in_it)
            .filter(|row| !replaced(row))
        {
            let who = match row.user_id {
                Some(user_id) => match names.get(&user_id) {
                    Some(name) => name.clone(),
                    None => super::resolve_addressee_label(db, user_id).await,
                },
                None => String::new(),
            };
            sores.push(sore_entry(row, Some(&who)));
        }
        let mut guesses = Vec::new();
        for row in of(super::recognizing::SOURCE)
            .filter(in_it)
            .filter(|row| row.invalid_at.is_none())
        {
            let evidence = evidence(row);
            let candidate = match evidence
                .get("userId")
                .and_then(Value::as_i64)
                .and_then(|id| i32::try_from(id).ok())
            {
                Some(user_id) => super::resolve_addressee_label(db, user_id).await,
                None => String::new(),
            };
            guesses.push(json!({
                "stranger": evidence.get("name"),
                "candidate": candidate,
                "sure": evidence.get("sure"),
                "why": row.content,
                "at": when(&row.created_at),
            }));
        }
        groups.push((
            latest,
            json!({
                "venue": venue,
                "guesses": guesses,
                "lands": of(super::bits::LANDS_SOURCE).filter(in_it).map(entry).collect::<Vec<_>>(),
                "days": of(super::bits::DAY_SOURCE).filter(in_it).map(|row| {
                    json!({ "day": evidence(row).get("day"), "text": row.content, "current": row.invalid_at.is_none() })
                }).collect::<Vec<_>>(),
                "bits": of(super::bits::SOURCE).filter(in_it).map(|row| {
                    json!({ "handle": evidence(row).get("handle"), "how": row.content, "at": when(&row.created_at), "current": row.invalid_at.is_none() })
                }).collect::<Vec<_>>(),
                "sore": sores,
            }),
        ));
    }
    groups.sort_by(|left, right| right.0.cmp(&left.0));

    json!({
        "generatedAt": now.to_rfc3339(),
        "her": her,
        "people": out_people.into_iter().map(|(_, person)| person).collect::<Vec<_>>(),
        "groups": groups.into_iter().map(|(_, group)| group).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod live {
    /// The snapshot from the site's own database, over a read-only
    /// connection: `cargo test … she_can_be_looked_into -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore = "reads the site's database"]
    async fn she_can_be_looked_into() {
        let db = crate::services::agent::semantic_eval::load_configured_lite().await;
        let snapshot = super::snapshot(&db).await;
        let count = |path: &str| {
            snapshot
                .pointer(path)
                .and_then(serde_json::Value::as_array)
                .map_or(0, Vec::len)
        };
        println!(
            "her: selfStory {} wants {} views {} questions {} days {} doing {}; people {}; groups {}",
            count("/her/selfStory"),
            count("/her/wants"),
            count("/her/views"),
            count("/her/questions"),
            count("/her/days"),
            count("/her/doingThisWeek"),
            count("/people"),
            count("/groups"),
        );
        assert!(snapshot.get("her").is_some());
        if let Ok(path) = std::env::var("MEROPE_MIND_DUMP") {
            std::fs::write(path, serde_json::to_string_pretty(&snapshot).unwrap()).unwrap();
        }
    }
}
