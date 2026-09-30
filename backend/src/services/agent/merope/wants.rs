//! What she wants (see `myriad_merope::wants`): kept, gone over at night,
//! and put before her when she chooses what to do and when she talks.
//!
//! A want is hers: it names no one and is heard wherever she is. At night
//! she goes over what she did and heard lately with what she already wants:
//! new wants grow from those records, others move along, come true, or she
//! lets them go. One that came true, or that she let go of, stays among her
//! own memories as part of her life; one nothing moved in six weeks fades.

use chrono::{DateTime, Utc};
use sea_orm::DatabaseConnection;
use serde_json::{Value, json};

use super::call::{self, Voice};
use crate::services::agent::memory::unified;
use myriad_merope::wants::{MAX_OPEN, Reach, SCHEMA_NAME, parse, schema, system};
pub use myriad_merope::wants::{Want, as_input, section};

pub const SOURCE: &str = "want";
/// A want that came true or that she let go of, as part of her life.
pub const ENDED: &str = "want_ended";
/// Untouched this long, a want fades.
const FADES_AFTER: chrono::Duration = chrono::Duration::days(42);
const CALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

fn want_of(row: &crate::models::entities::agent_memories::Model) -> Option<Want> {
    let evidence: Value = serde_json::from_str(row.evidence.as_deref()?).ok()?;
    let at = |value: &Value| {
        value
            .as_str()
            .and_then(|at| DateTime::parse_from_rfc3339(at).ok())
            .map(|at| at.with_timezone(&Utc))
    };
    let notes = evidence
        .get("notes")
        .and_then(Value::as_array)
        .map(|notes| {
            notes
                .iter()
                .filter_map(|note| {
                    Some((
                        at(note.get("at")?)?,
                        note.get("note")?.as_str()?.to_string(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    Some(Want {
        id: row.id.clone(),
        want: row.content.clone(),
        why: evidence
            .get("why")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        reach: evidence
            .get("reach")
            .and_then(Value::as_str)
            .and_then(Reach::parse)?,
        longing: evidence
            .get("longing")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        since: evidence
            .get("since")
            .and_then(at)
            .unwrap_or_else(|| row.created_at.with_timezone(&Utc)),
        notes,
    })
}

/// What she wants now, oldest first.
pub async fn open(db: &DatabaseConnection) -> Vec<Want> {
    let mut wants: Vec<Want> = unified::own_rows(db, SOURCE, MAX_OPEN as u64 * 2)
        .await
        .unwrap_or_default()
        .iter()
        .filter_map(want_of)
        .collect();
    wants.sort_by_key(|want| want.since);
    wants
}

async fn put(db: &DatabaseConnection, want: &Want, grew_from: &[String]) {
    let evidence = json!({
        "why": want.why,
        "reach": want.reach.as_str(),
        "longing": want.longing,
        "since": want.since.to_rfc3339(),
        "notes": want.notes.iter().map(|(at, note)| json!({ "at": at.to_rfc3339(), "note": note })).collect::<Vec<_>>(),
        "grewFrom": grew_from,
    });
    let _ = unified::remember_own(db, &want.want, &evidence.to_string(), Vec::new(), SOURCE).await;
}

/// How a want ended, kept as part of her life.
async fn ended(db: &DatabaseConnection, want: &Want, how: &str, came_true: bool) {
    let reason = if came_true { "came_true" } else { "let_go" };
    if unified::retire_own(db, &want.id, reason).await.is_err() {
        return;
    }
    let evidence = json!({
        "want": want.want,
        "since": want.since.to_rfc3339(),
        "ended": reason,
        "how": how,
    });
    let line = if came_true {
        format!("想要的「{}」实现了：{how}", want.want.trim())
    } else {
        format!("不再想要「{}」了：{how}", want.want.trim())
    };
    let _ = unified::remember_own(db, &line, &evidence.to_string(), Vec::new(), ENDED).await;
}

/// At night: what she wants, from what she did and heard this past week.
pub async fn go_over(db: &DatabaseConnection, owner: i32) {
    let now = Utc::now();
    let records = super::explore::lately(db, now.fixed_offset() - chrono::Duration::days(7)).await;
    let held = open(db).await;
    if records.is_empty() && held.is_empty() {
        return;
    }
    let soul = crate::services::agent::identity::get_speaking_soul()
        .await
        .unwrap_or_default();
    let input = json!({
        "records": records.iter().map(|record| json!({ "id": record.id, "what": record.line })).collect::<Vec<_>>(),
        "wants": as_input(&held, now),
        "whoYouHaveBeen": super::self_story::current(db).await,
    })
    .to_string();
    let Ok(raw) = call::Ask::new(Voice::Hers, owner, "wants")
        .within(CALL_TIMEOUT)
        .json_raw(&system(&soul), &input, SCHEMA_NAME, &schema())
        .await
    else {
        return;
    };
    let ids: Vec<String> = records.iter().map(|record| record.id.clone()).collect();
    let Some(night) = parse(&raw, &ids, &held) else {
        return;
    };
    let mut done: Vec<usize> = Vec::new();
    for came in &night.came_true {
        if let Some(want) = held.get(came.i).filter(|_| !done.contains(&came.i)) {
            ended(db, want, &came.how, true).await;
            done.push(came.i);
        }
    }
    for gone in &night.let_go {
        if let Some(want) = held.get(gone.i).filter(|_| !done.contains(&gone.i)) {
            ended(db, want, &gone.how, false).await;
            done.push(gone.i);
        }
    }
    for moved in &night.moved {
        let Some(want) = held.get(moved.i).filter(|_| !done.contains(&moved.i)) else {
            continue;
        };
        if unified::retire_own(db, &want.id, "superseded")
            .await
            .is_err()
        {
            continue;
        }
        let mut want = want.clone();
        want.notes.push((now, moved.note.clone()));
        put(db, &want, &[]).await;
        done.push(moved.i);
    }
    // Past the limit, the oldest she still holds goes to make room.
    let still = held.len() - done.len().min(held.len());
    let over = (still + night.new.len()).saturating_sub(MAX_OPEN);
    for want in held
        .iter()
        .enumerate()
        .filter(|(index, _)| !done.contains(index))
        .map(|(_, want)| want)
        .take(over)
    {
        let _ = unified::retire_own(db, &want.id, "faded").await;
    }
    for new in &night.new {
        let grew_from: Vec<String> = new
            .cites
            .iter()
            .filter_map(|cite| records.iter().find(|record| &record.id == cite))
            .map(|record| record.row.clone())
            .collect();
        let want = Want {
            id: String::new(),
            want: new.want.clone(),
            why: new.why.clone(),
            reach: new.reach,
            longing: new.longing,
            since: now,
            notes: Vec::new(),
        };
        put(db, &want, &grew_from).await;
    }
    if !night.new.is_empty() || !done.is_empty() {
        tracing::info!(
            new = night.new.len(),
            changed = done.len(),
            "[Merope] what she wants"
        );
    }
}

/// Wants nothing moved in six weeks fade.
pub async fn let_fade(db: &DatabaseConnection) {
    match unified::fade_source(db, SOURCE, FADES_AFTER).await {
        Ok(faded) if faded > 0 => tracing::info!(faded, "[Merope] wants faded"),
        Ok(_) => {}
        Err(error) => tracing::warn!(%error, "[Merope] could not let old wants fade"),
    }
}

/// The wants as a choice of what to do sees them: a line each.
/// Her wants for choosing what to do: longings are not among them.
pub fn lines(wants: &[Want]) -> Vec<String> {
    wants
        .iter()
        .filter(|want| !want.longing)
        .map(|want| match want.notes.last() {
            Some((_, note)) => format!("{} (lately: {note})", want.want),
            None => want.want.clone(),
        })
        .collect()
}
