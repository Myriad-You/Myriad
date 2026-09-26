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

pub const SOURCE: &str = "thread";
/// Open threads kept per person; the oldest go first.
const MAX_OPEN: usize = 8;
const MAX_ABOUT_CHARS: usize = 40;
const MAX_THEN_CHARS: usize = 120;
/// A thread lingers this long past its time, and an undated one this long
/// untouched.
const PAST_DUE: chrono::Duration = chrono::Duration::days(3);
const UNDATED: chrono::Duration = chrono::Duration::days(14);

#[derive(Debug, Clone, PartialEq)]
pub struct Thread {
    pub id: String,
    /// What it is about, a few words.
    pub about: String,
    /// What she would come back with, in her words.
    pub then: String,
    /// When it would be natural to bring it up; none for "whenever".
    pub due: Option<DateTime<Utc>>,
}

impl Thread {
    pub fn is_due(&self, now: DateTime<Utc>) -> bool {
        self.due.is_some_and(|due| due <= now)
    }
}

fn thread_of(row: &crate::models::entities::agent_memories::Model) -> Option<Thread> {
    let evidence: Value = serde_json::from_str(row.evidence.as_deref()?).ok()?;
    let about = evidence.get("about")?.as_str()?.trim().to_string();
    let due = evidence
        .get("due")
        .and_then(Value::as_str)
        .and_then(|due| DateTime::parse_from_rfc3339(due).ok())
        .map(|due| due.with_timezone(&Utc));
    (!about.is_empty()).then(|| Thread {
        id: row.id.clone(),
        about,
        then: row.content.clone(),
        due,
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

/// Something to come back to with them. The same subject again replaces
/// the earlier one; past the limit, the oldest goes.
pub async fn keep(db: &DatabaseConnection, user_id: i32, kept: &Kept, now: DateTime<Utc>) {
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
                json!({ "about": about, "due": due.map(|due| due.to_rfc3339()) }).to_string(),
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

/// What is on her mind about them, for a conversation with them.
pub fn section(threads: &[Thread], now: DateTime<Utc>) -> Option<String> {
    if threads.is_empty() {
        return None;
    }
    let lines: Vec<String> = threads
        .iter()
        .map(|thread| {
            let when = match thread.due {
                Some(due) if due <= now => "now".to_string(),
                Some(due) => format!(
                    "later, around {}",
                    due.with_timezone(&chrono::Local).format("%m-%d %H:%M")
                ),
                None => "whenever it fits".to_string(),
            };
            format!("- {}: {} ({when})", thread.about, thread.then)
        })
        .collect();
    Some(format!(
        "## On your mind about them\nThings you meant to come back to with them. Bring one up when it fits, as yourself; one at a time, and never as a list.\n{}",
        myriad_agent_rules::untrusted_block("on_your_mind", &lines.join("\n"))
    ))
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
    fn a_thread_says_what_and_when() {
        let now: DateTime<Utc> = "2026-09-26T12:00:00Z".parse().unwrap();
        let due = Thread {
            id: "a".into(),
            about: "考试".into(),
            then: "问他考得怎么样".into(),
            due: Some(now - chrono::Duration::hours(1)),
        };
        let later = Thread {
            due: Some(now + chrono::Duration::hours(20)),
            about: "搬家".into(),
            ..due.clone()
        };
        let whenever = Thread {
            due: None,
            about: "那首歌".into(),
            ..due.clone()
        };
        assert!(due.is_due(now) && !later.is_due(now) && !whenever.is_due(now));
        let section = section(&[due, later, whenever], now).unwrap();
        assert!(section.contains("考试: 问他考得怎么样 (now)"));
        assert!(section.contains("搬家") && section.contains("later, around"));
        assert!(section.contains("(whenever it fits)"));
        assert!(section.contains("never as a list"));
        assert!(super::section(&[], now).is_none());
    }

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
