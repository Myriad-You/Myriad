//! What is on her mind about each person: things to come back to with them.
//!
//! After an exchange in private she notices, as part of how the exchange
//! left her (see `inner`), what she would want to come back to later:
//! something they are about to do or face ("an exam tomorrow"), with when it
//! would be natural to ask; something left unfinished between them. When
//! her reply takes one up, or it stops mattering, she lets it go.
//!
//! These are hers about them, kept with that person, heard only in private
//! with them, and apart from ordinary memory. They are what she brings up
//! when they talk, what she opens with when they come back, and a reason to
//! write to them first when they are away (see `reach`). A thread past its
//! time by a few days, or undated and untouched for two weeks, fades.

use chrono::{DateTime, Utc};
use sea_orm::DatabaseConnection;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::services::agent::memory::unified::{self, Audience, Concept};
use myriad_merope::threads::{MAX_ABOUT_CHARS, MAX_OPEN, MAX_THEN_CHARS};
pub use myriad_merope::threads::{Thread, section};

pub const SOURCE: &str = "thread";
/// A thread lingers this long past its time, and an undated one this long
/// untouched.
const PAST_DUE: chrono::Duration = chrono::Duration::days(3);
const UNDATED: chrono::Duration = chrono::Duration::days(14);

fn thread_of(row: &crate::models::entities::agent_memories::Model) -> Option<Thread> {
    let evidence: Value = serde_json::from_str(row.evidence.as_deref()?).ok()?;
    let about = evidence.get("about")?.as_str()?.trim().to_string();
    let due = evidence
        .get("due")
        .and_then(Value::as_str)
        .and_then(|due| DateTime::parse_from_rfc3339(due).ok())
        .map(|due| due.with_timezone(&Utc));
    let hers = evidence
        .get("hers")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    (!about.is_empty()).then(|| Thread {
        id: row.id.clone(),
        about,
        then: row.content.clone(),
        due,
        hers,
    })
}

/// What is on her mind about this person, oldest first.
pub async fn open(db: &DatabaseConnection, user_id: i32) -> Vec<Thread> {
    let mut threads: Vec<Thread> = unified::venue_source_rows(
        db,
        Some(user_id),
        &Audience::private(user_id).venue(),
        SOURCE,
        MAX_OPEN as u64 * 2,
    )
    .await
    .unwrap_or_default()
    .iter()
    .filter_map(thread_of)
    .collect();
    threads.reverse();
    threads
}

/// Something of hers toward them (from going over a day with them at night):
/// kept like any thread, whenever it fits.
pub async fn wish(db: &DatabaseConnection, user_id: i32, about: &str, then: &str) {
    let kept = Kept {
        about: about.to_string(),
        then: then.to_string(),
        due_in_hours: None,
    };
    put(db, user_id, &kept, Utc::now(), true).await;
}

/// Something to come back to with them. The same subject again replaces
/// the earlier one; past the limit, the oldest goes.
pub async fn keep(db: &DatabaseConnection, user_id: i32, kept: &Kept, now: DateTime<Utc>) {
    put(db, user_id, kept, now, false).await;
}

async fn put(db: &DatabaseConnection, user_id: i32, kept: &Kept, now: DateTime<Utc>, hers: bool) {
    let about: String = kept.about.trim().chars().take(MAX_ABOUT_CHARS).collect();
    let then: String = kept.then.trim().chars().take(MAX_THEN_CHARS).collect();
    if about.is_empty() || then.is_empty() {
        return;
    }
    let held = open(db, user_id).await;
    let mut retire: Vec<String> = held
        .iter()
        .filter(|thread| thread.about == about)
        .map(|thread| thread.id.clone())
        .collect();
    let others = held.len() - retire.len();
    if others + 1 > MAX_OPEN {
        retire.extend(
            held.iter()
                .filter(|thread| thread.about != about)
                .take(others + 1 - MAX_OPEN)
                .map(|thread| thread.id.clone()),
        );
    }
    if !retire.is_empty() {
        let _ = unified::retire(db, user_id, &retire, "superseded").await;
    }
    let due = kept
        .due_in_hours
        .filter(|hours| (0..=24 * 60).contains(hours))
        .map(|hours| now + chrono::Duration::hours(hours));
    let _ = unified::remember(
        db,
        unified::NewMemory {
            user_id,
            kind: unified::MemoryKind::Fact,
            content: then,
            evidence: Some(
                json!({ "about": about, "due": due.map(|due| due.to_rfc3339()), "hers": hers })
                    .to_string(),
            ),
            speaker: unified::Speaker::Agent,
            source: SOURCE,
            audience: Audience::private(user_id),
            importance: 0.5,
            concepts: vec![Concept {
                name: about,
                aliases: Vec::new(),
            }],
        },
    )
    .await;
}

/// She took it up, or it no longer matters.
pub async fn close(db: &DatabaseConnection, user_id: i32, ids: &[String], reason: &str) {
    if !ids.is_empty() {
        let _ = unified::retire(db, user_id, ids, reason).await;
    }
}

/// A thread as her reflection after an exchange reports it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Kept {
    pub about: String,
    pub then: String,
    /// Hours from now until it would be natural to bring it up; none for
    /// whenever it fits.
    pub due_in_hours: Option<i64>,
}

/// The open threads as her reflection sees them, numbered for `done`.
pub fn as_input(threads: &[Thread]) -> Vec<Value> {
    threads
        .iter()
        .enumerate()
        .map(|(index, thread)| json!({ "i": index, "about": thread.about, "then": thread.then }))
        .collect()
}

/// Threads past their time by a few days, or undated and untouched for
/// two weeks, fade.
pub async fn let_fade(db: &DatabaseConnection) {
    let now = Utc::now();
    let rows = unified::active_from_source(db, SOURCE)
        .await
        .unwrap_or_default();
    let mut faded = 0;
    for row in rows {
        let Some(user_id) = row.user_id else {
            continue;
        };
        let stale = match thread_of(&row).and_then(|thread| thread.due) {
            Some(due) => due + PAST_DUE < now,
            None => row.updated_at.with_timezone(&Utc) + UNDATED < now,
        };
        if stale
            && unified::retire(db, user_id, &[row.id.clone()], "faded")
                .await
                .unwrap_or(0)
                > 0
        {
            faded += 1;
        }
    }
    if faded > 0 {
        tracing::info!(faded, "[Merope] threads faded");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn her_reflection_reports_threads_in_a_fixed_shape() {
        let kept: Kept =
            serde_json::from_str(r#"{"about":"考试","then":"问他考得怎么样","dueInHours":30}"#)
                .unwrap();
        assert_eq!(kept.due_in_hours, Some(30));
        assert!(
            serde_json::from_str::<Kept>(r#"{"about":"x","then":"y","dueInHours":1,"extra":1}"#)
                .is_err()
        );
    }
}
