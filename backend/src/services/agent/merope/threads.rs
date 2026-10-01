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
//!
//! A group has its own: after answering someone there she notices, the same
//! way, what she would come back to in that group (someone there has an
//! exam tomorrow). Those are the group's, of no account, kept and heard only
//! there; when one is due she decides, as herself, whether to come back to
//! it there (see `channel_group::come_back`).

use chrono::{DateTime, Utc};
use sea_orm::DatabaseConnection;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::services::agent::memory::unified::{self, Audience, Concept};
use myriad_merope::threads::{MAX_ABOUT_CHARS, MAX_OPEN, MAX_THEN_CHARS};
pub use myriad_merope::threads::{Thread, section};
use std::collections::HashSet;
use std::sync::{LazyLock, Mutex};

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

/// A group's stored venue (`group:telegram:-100123`) from its chat venue.
fn group_venue(venue: &str) -> String {
    Audience::group(venue, 0).venue()
}

fn group_thread_of(row: &crate::models::entities::agent_memories::Model) -> Option<Thread> {
    thread_of(row).filter(|_| row.user_id.is_none())
}

/// What is on her mind in a group (`telegram:-100123`), oldest first.
pub async fn open_in_group(db: &DatabaseConnection, venue: &str) -> Vec<Thread> {
    let mut threads: Vec<Thread> =
        unified::venue_source_rows(db, None, &group_venue(venue), SOURCE, MAX_OPEN as u64 * 2)
            .await
            .unwrap_or_default()
            .iter()
            .filter_map(group_thread_of)
            .collect();
    threads.reverse();
    threads
}

/// Something to come back to in a group. The same subject again replaces
/// the earlier one; past the limit, the oldest goes.
pub async fn keep_in_group(db: &DatabaseConnection, venue: &str, kept: &Kept, now: DateTime<Utc>) {
    let about: String = kept.about.trim().chars().take(MAX_ABOUT_CHARS).collect();
    let then: String = kept.then.trim().chars().take(MAX_THEN_CHARS).collect();
    if about.is_empty() || then.is_empty() {
        return;
    }
    let held = open_in_group(db, venue).await;
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
    close_in_group(db, venue, &retire, "superseded").await;
    let due = kept
        .due_in_hours
        .filter(|hours| (0..=24 * 60).contains(hours))
        .map(|hours| now + chrono::Duration::hours(hours));
    let evidence = json!({ "about": about, "due": due.map(|due| due.to_rfc3339()), "hers": false })
        .to_string();
    let _ = unified::remember_in_venue(db, &group_venue(venue), &then, &evidence, SOURCE).await;
}

/// She took them up in the group, or they no longer matter.
pub async fn close_in_group(db: &DatabaseConnection, venue: &str, ids: &[String], reason: &str) {
    let stored = group_venue(venue);
    for id in ids {
        let _ = unified::retire_unowned(db, &stored, id, reason).await;
    }
}

/// Threads of groups she was already asked about coming back to, in this
/// run: asked once, and if she would not, left to come up in the talk or
/// fade.
static ASKED: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Default::default);

/// Each group's threads that are due: she decides, as herself, whether to
/// come back to one there now (see `channel_group::come_back`). Taken up,
/// it closes. Billed to the site's owner, who hosts her in groups.
pub async fn come_back_in_groups(db: &DatabaseConnection) {
    let now = Utc::now();
    let rows = unified::active_from_source(db, SOURCE)
        .await
        .unwrap_or_default();
    let due: Vec<(String, Thread)> = rows
        .iter()
        .filter_map(|row| {
            let venue = row.venue.strip_prefix("group:")?.to_string();
            let thread = group_thread_of(row)?;
            thread.is_due(now).then_some((venue, thread))
        })
        .filter(|(_, thread)| {
            ASKED
                .lock()
                .map(|mut asked| asked.insert(thread.id.clone()))
                .unwrap_or(false)
        })
        .collect();
    if due.is_empty() {
        return;
    }
    let Ok(owner) = crate::services::site_owner::site_owner_user_id(db).await else {
        return;
    };
    for (venue, thread) in due {
        let what = format!("{}: {}", thread.about, thread.then);
        if crate::services::channel_group::come_back(owner, &venue, &what).await {
            close_in_group(db, &venue, &[thread.id.clone()], "came_back").await;
        }
    }
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
        let stale = match thread_of(&row).and_then(|thread| thread.due) {
            Some(due) => due + PAST_DUE < now,
            None => row.updated_at.with_timezone(&Utc) + UNDATED < now,
        };
        if !stale {
            continue;
        }
        // A group's, of no account; otherwise someone's.
        let retired = match row.user_id {
            Some(user_id) => {
                unified::retire(db, user_id, &[row.id.clone()], "faded")
                    .await
                    .unwrap_or(0)
                    > 0
            }
            None => unified::retire_unowned(db, &row.venue, &row.id, "faded")
                .await
                .unwrap_or(false),
        };
        if retired {
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

    /// A group's threads are its own: kept and read only there, never with
    /// anyone in private; the same subject replaces the earlier one; taken
    /// up, they close; long past due, they fade.
    #[tokio::test]
    async fn a_group_keeps_its_own_threads() {
        use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
        let Ok(url) = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL") else {
            return;
        };
        let isolated = crate::db::IsolatedSchema::migrated(&url, "group_threads").await;
        let db = isolated.db.clone();
        let user_id: i32 = db
            .query_one_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                "INSERT INTO users (username) VALUES ('group-thread-test') RETURNING id",
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get("", "id")
            .unwrap();
        let (here, there) = ("telegram:-1001", "onebot:2002");
        let now = Utc::now();
        let exam = Kept {
            about: "阿明的考试".into(),
            then: "问阿明考得怎么样".into(),
            due_in_hours: Some(20),
        };
        keep_in_group(&db, here, &exam, now).await;
        let open = open_in_group(&db, here).await;
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].then, "问阿明考得怎么样");
        assert!(!open[0].is_due(now) && open[0].is_due(now + chrono::Duration::hours(21)));
        assert!(
            open_in_group(&db, there).await.is_empty(),
            "only that group's"
        );
        assert!(
            super::open(&db, user_id).await.is_empty(),
            "never in private"
        );

        let moved = Kept {
            due_in_hours: Some(1),
            ..exam.clone()
        };
        keep_in_group(&db, here, &moved, now).await;
        let open = open_in_group(&db, here).await;
        assert_eq!(open.len(), 1, "the same subject replaces the earlier one");
        close_in_group(&db, here, &[open[0].id.clone()], "taken_up").await;
        assert!(open_in_group(&db, here).await.is_empty());

        // Due long ago: it fades with the rest.
        keep_in_group(&db, here, &exam, now - chrono::Duration::days(10)).await;
        assert_eq!(open_in_group(&db, here).await.len(), 1);
        let_fade(&db).await;
        assert!(open_in_group(&db, here).await.is_empty());
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
