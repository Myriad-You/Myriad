//! Her nodes on her host (`agent_nodes`): what she keeps about one thing the
//! owner does on a child end, written from what she decided about each moment
//! that end told her (her link's `moment`). Another coding agent's session
//! files under its repository's node; a conversation with another AI under
//! one node of those talks; what a child read of the owner's situation under
//! none.
//!
//! A node holds her own words (what she would have said, what she noted) and
//! references to the events they came from; never the originals. It is only
//! ever the owner's own: nothing here is read for anyone else, and no other
//! audience writes it. Each change is a line in the node's append-only log
//! (`agent_node_events`), written in the same transaction as the change.
//!
//! Her notes on a node come with the next moment about it, so she knows what
//! she already made of it. The owner sees them on the child (`nodes`), and
//! can make her forget a node there: the row and its log are deleted.
//!
//! Nodes are not carried when she moves, as on the Mac: they stay on the end
//! that kept them.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use chrono::{DateTime, Utc};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::sync::broadcast;

use super::gates::{AI_CHAT_EVENT, PEER_AGENT_EVENT};

/// The most notes, references and log-worthy lines a node keeps (as on the Mac).
const NOTES_AT_MOST: usize = 64;
const REFS_AT_MOST: usize = 200;
/// Her notes that come with the next moment about the node, newest last.
const NOTES_TOLD: usize = 6;
/// Characters of each note told with a moment.
const NOTE_CHARS: usize = 320;
/// The key of the node her talks with other AIs file under (the Mac's own).
pub(crate) const CHATS_KEY: &str = "together:chats";

/// What a node is: a repository, or one of the things done together.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Form {
    Repo,
    Together,
}

impl Form {
    fn as_str(self) -> &'static str {
        match self {
            Form::Repo => "repo",
            Form::Together => "together",
        }
    }
}

/// The node an event files under: its form, key and the name she sees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Place {
    pub(crate) form: Form,
    pub(crate) key: String,
    pub(crate) title: String,
}

/// Where a moment of `event_key` with these facts files, if anywhere.
pub(crate) fn place(event_key: &str, facts: &BTreeMap<String, String>) -> Option<Place> {
    match event_key {
        PEER_AGENT_EVENT => {
            let repo = facts.get("repo").map(|repo| repo.trim()).unwrap_or("");
            (!repo.is_empty()).then(|| Place {
                form: Form::Repo,
                key: repo.to_string(),
                title: repo.to_string(),
            })
        }
        // The child names this one in its own language.
        AI_CHAT_EVENT => Some(Place {
            form: Form::Together,
            key: CHATS_KEY.to_string(),
            title: String::new(),
        }),
        _ => None,
    }
}

/// The owner's node of `key`: one per thing, whichever end told her.
pub(crate) fn node_id(user_id: i32, key: &str) -> String {
    let digest = Sha256::digest(format!("agent_node\u{1f}{user_id}\u{1f}{key}").as_bytes());
    format!("node_{}", &hex::encode(digest)[..40])
}

/// One of her notes on a node: what she would have said, or what she noted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Note {
    pub(crate) event: String,
    /// What had happened (`safeFacts.happened`).
    pub(crate) happened: String,
    /// What she decided: speak, ask, remember, propose_work.
    pub(crate) action: String,
    /// In her words.
    pub(crate) line: String,
    pub(crate) at: DateTime<Utc>,
    /// Said to the owner (or, for a note she kept, kept as said); false while
    /// it waits or once it went unsaid.
    pub(crate) said: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Ref {
    event: String,
    at: DateTime<Utc>,
}

/// What she decided about a moment, as it is kept on its node.
#[derive(Debug, Clone)]
pub(crate) struct Decided<'a> {
    pub(crate) event: &'a str,
    pub(crate) happened: &'a str,
    pub(crate) action: &'a str,
    pub(crate) reason: &'a str,
    /// The device that told her.
    pub(crate) origin: &'a str,
    /// Her line or note, when she had one.
    pub(crate) note: Option<Note>,
}

/// A node as the owner sees it on the child.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Shown {
    pub(crate) id: String,
    pub(crate) form: Form,
    pub(crate) key: String,
    pub(crate) title: String,
    pub(crate) origin: String,
    pub(crate) notes: Vec<Note>,
    pub(crate) created_at: i64,
    pub(crate) updated_at: i64,
}

/// The owners whose nodes just changed.
static CHANGED: LazyLock<broadcast::Sender<i32>> = LazyLock::new(|| broadcast::channel(64).0);

/// Hears each owner whose nodes changed (in this process, where she runs).
pub(crate) fn subscribe() -> broadcast::Receiver<i32> {
    CHANGED.subscribe()
}

fn changed(user_id: i32) {
    let _ = CHANGED.send(user_id);
}

fn statement(sql: &str, values: Vec<sea_orm::Value>) -> Statement {
    Statement::from_sql_and_values(DatabaseBackend::Postgres, sql, values)
}

fn json_list<T: for<'de> Deserialize<'de>>(value: Option<Value>) -> Vec<T> {
    value
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default()
}

/// Appends a line to the node's log, after `logged` lines.
async fn log<C: ConnectionTrait>(
    db: &C,
    node: &str,
    seq: i64,
    change: &str,
    fields: Value,
    at: DateTime<Utc>,
) -> Result<(), sea_orm::DbErr> {
    db.execute_raw(statement(
        "INSERT INTO agent_node_events (node_id, seq, change, fields, at) VALUES ($1, $2, $3, $4, $5)",
        vec![
            node.into(),
            seq.into(),
            change.into(),
            fields.into(),
            at.into(),
        ],
    ))
    .await
    .map(|_| ())
}

/// Her notes on the owner's node at `place`, newest last, as told with the
/// next moment about it; None when she has none.
pub(crate) async fn notes_for(
    db: &DatabaseConnection,
    user_id: i32,
    place: &Place,
) -> Option<String> {
    let row = db
        .query_one_raw(statement(
            "SELECT notes FROM agent_nodes WHERE id = $1 AND user_id = $2",
            vec![node_id(user_id, &place.key).into(), user_id.into()],
        ))
        .await
        .ok()
        .flatten()?;
    let notes: Vec<Note> = json_list(row.try_get::<Value>("", "notes").ok());
    told(&notes)
}

/// The notes told with a moment: her own words, each with what became of it.
pub(crate) fn told(notes: &[Note]) -> Option<String> {
    let start = notes.len().saturating_sub(NOTES_TOLD);
    let lines: Vec<String> = notes[start..]
        .iter()
        .map(|note| {
            let what = match (note.action.as_str(), note.said) {
                ("remember", _) => "you noted",
                (_, true) => "you said",
                (_, false) => "you had this to say and did not",
            };
            let line: String = note.line.chars().take(NOTE_CHARS).collect();
            format!(
                "- {} ({}, {}): {line}",
                what,
                note.happened,
                note.at.format("%Y-%m-%d %H:%M UTC")
            )
        })
        .collect();
    (!lines.is_empty()).then(|| {
        format!(
            "What you already made of this before, in your own words (yours, kept on it):\n{}",
            lines.join("\n")
        )
    })
}

/// Files what she decided about a moment on its node: the node made the
/// first time, the event kept as a reference, her line or note kept, each
/// change logged. Only the owner's own.
pub(crate) async fn decided(
    db: &DatabaseConnection,
    user_id: i32,
    place: &Place,
    decided: Decided<'_>,
) -> Result<(), sea_orm::DbErr> {
    let id = node_id(user_id, &place.key);
    let now = crate::clock::now();
    let transaction = db.begin().await?;
    let created = transaction
        .execute_raw(statement(
            "INSERT INTO agent_nodes (id, user_id, form, node_key, title, origin, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $7) ON CONFLICT (id) DO NOTHING",
            vec![
                id.clone().into(),
                user_id.into(),
                place.form.as_str().into(),
                place.key.clone().into(),
                place.title.clone().into(),
                decided.origin.into(),
                now.into(),
            ],
        ))
        .await?
        .rows_affected()
        == 1;
    let Some(row) = transaction
        .query_one_raw(statement(
            "SELECT notes, refs, logged FROM agent_nodes WHERE id = $1 AND user_id = $2 FOR UPDATE",
            vec![id.clone().into(), user_id.into()],
        ))
        .await?
    else {
        // Another owner's key cannot reach here: the id is the owner's.
        transaction.rollback().await?;
        return Ok(());
    };
    let mut notes: Vec<Note> = json_list(row.try_get::<Value>("", "notes").ok());
    let mut refs: Vec<Ref> = json_list(row.try_get::<Value>("", "refs").ok());
    let mut logged: i64 = row.try_get("", "logged").unwrap_or(0);
    // Decided already (a resend that got this far): nothing changes.
    if refs
        .iter()
        .any(|reference| reference.event == decided.event)
    {
        transaction.rollback().await?;
        return Ok(());
    }
    let mut lines: Vec<(&str, Value)> = Vec::new();
    if created {
        lines.push((
            "created",
            json!({ "form": place.form.as_str(), "origin": decided.origin }),
        ));
    }
    lines.push((
        "decided",
        json!({
            "event": decided.event,
            "happened": decided.happened,
            "action": decided.action,
            "reason": decided.reason,
        }),
    ));
    refs.push(Ref {
        event: decided.event.to_string(),
        at: now,
    });
    if let Some(note) = decided.note {
        lines.push(if note.action == "remember" {
            ("noted", json!({ "event": decided.event }))
        } else {
            ("raised", json!({ "event": decided.event }))
        });
        notes.push(note);
    }
    let notes_over = notes.len().saturating_sub(NOTES_AT_MOST);
    notes.drain(..notes_over);
    let refs_over = refs.len().saturating_sub(REFS_AT_MOST);
    refs.drain(..refs_over);
    for (change, fields) in lines {
        logged += 1;
        log(&transaction, &id, logged, change, fields, now).await?;
    }
    transaction
        .execute_raw(statement(
            "UPDATE agent_nodes SET notes = $3, refs = $4, logged = $5, updated_at = $6 \
             WHERE id = $1 AND user_id = $2",
            vec![
                id.into(),
                user_id.into(),
                serde_json::to_value(&notes).unwrap_or_default().into(),
                serde_json::to_value(&refs).unwrap_or_default().into(),
                logged.into(),
                now.into(),
            ],
        ))
        .await?;
    transaction.commit().await?;
    changed(user_id);
    Ok(())
}

/// Her line about `event` was said to the owner: its note on the node says
/// so. Nothing when no node of theirs holds it.
pub(crate) async fn said(
    db: &DatabaseConnection,
    user_id: i32,
    event: &str,
    line: &str,
) -> Result<(), sea_orm::DbErr> {
    let now = crate::clock::now();
    let transaction = db.begin().await?;
    let Some(row) = transaction
        .query_one_raw(statement(
            "SELECT id, notes, logged FROM agent_nodes WHERE user_id = $1 AND notes @> $2 FOR UPDATE",
            vec![user_id.into(), json!([{ "event": event }]).into()],
        ))
        .await?
    else {
        transaction.rollback().await?;
        return Ok(());
    };
    let id: String = row.try_get("", "id")?;
    let mut notes: Vec<Note> = json_list(row.try_get::<Value>("", "notes").ok());
    let logged: i64 = row.try_get::<i64>("", "logged").unwrap_or(0) + 1;
    let Some(note) = notes
        .iter_mut()
        .find(|note| note.event == event && !note.said && note.action != "remember")
    else {
        transaction.rollback().await?;
        return Ok(());
    };
    note.said = true;
    // What she said in the end, in her words.
    if !line.trim().is_empty() {
        note.line = line.chars().take(NOTE_CHARS).collect();
    }
    log(
        &transaction,
        &id,
        logged,
        "said",
        json!({ "event": event }),
        now,
    )
    .await?;
    transaction
        .execute_raw(statement(
            "UPDATE agent_nodes SET notes = $3, logged = $4, updated_at = $5 WHERE id = $1 AND user_id = $2",
            vec![
                id.into(),
                user_id.into(),
                serde_json::to_value(&notes).unwrap_or_default().into(),
                logged.into(),
                now.into(),
            ],
        ))
        .await?;
    transaction.commit().await?;
    changed(user_id);
    Ok(())
}

/// The owner's nodes, the most recently changed first.
pub(crate) async fn list(
    db: &DatabaseConnection,
    user_id: i32,
) -> Result<Vec<Shown>, sea_orm::DbErr> {
    let rows = db
        .query_all_raw(statement(
            "SELECT id, form, node_key, title, origin, notes, created_at, updated_at \
             FROM agent_nodes WHERE user_id = $1 ORDER BY updated_at DESC, id",
            vec![user_id.into()],
        ))
        .await?;
    Ok(rows
        .into_iter()
        .filter_map(|row| {
            let form = match row.try_get::<String>("", "form").ok()?.as_str() {
                "repo" => Form::Repo,
                "together" => Form::Together,
                _ => return None,
            };
            Some(Shown {
                id: row.try_get("", "id").ok()?,
                form,
                key: row.try_get("", "node_key").ok()?,
                title: row.try_get("", "title").ok()?,
                origin: row.try_get("", "origin").ok()?,
                notes: json_list(row.try_get::<Value>("", "notes").ok()),
                created_at: row
                    .try_get::<DateTime<Utc>>("", "created_at")
                    .ok()?
                    .timestamp_millis(),
                updated_at: row
                    .try_get::<DateTime<Utc>>("", "updated_at")
                    .ok()?
                    .timestamp_millis(),
            })
        })
        .collect())
}

/// Makes her forget the owner's node: it and its log are deleted, not
/// marked. False when the owner has no such node.
pub(crate) async fn forget(
    db: &DatabaseConnection,
    user_id: i32,
    id: &str,
) -> Result<bool, sea_orm::DbErr> {
    let gone = db
        .execute_raw(statement(
            "DELETE FROM agent_nodes WHERE id = $1 AND user_id = $2",
            vec![id.into(), user_id.into()],
        ))
        .await?
        .rows_affected()
        == 1;
    if gone {
        changed(user_id);
    }
    Ok(gone)
}

#[cfg(test)]
mod tests {
    use sea_orm::ConnectionTrait;

    use super::*;
    use crate::services::agent::merope::gates::SITUATION_EVENT;

    fn facts(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect()
    }

    #[test]
    fn a_moment_files_under_its_repository_or_the_talks_with_other_ais() {
        let repo = place(PEER_AGENT_EVENT, &facts(&[("repo", "sample-repo")])).unwrap();
        assert_eq!((repo.form, repo.key.as_str()), (Form::Repo, "sample-repo"));
        assert_eq!(repo.title, "sample-repo");
        assert_eq!(place(PEER_AGENT_EVENT, &facts(&[("repo", "  ")])), None);
        assert_eq!(place(PEER_AGENT_EVENT, &facts(&[])), None);
        let chats = place(AI_CHAT_EVENT, &facts(&[("service", "Chat")])).unwrap();
        assert_eq!(
            (chats.form, chats.key.as_str()),
            (Form::Together, CHATS_KEY)
        );
        assert_eq!(
            place(SITUATION_EVENT, &facts(&[("situation", "coding")])),
            None
        );
        // One node per owner and thing, whichever end told her.
        assert_eq!(node_id(7, "sample-repo"), node_id(7, "sample-repo"));
        assert_ne!(node_id(7, "sample-repo"), node_id(8, "sample-repo"));
        // Within her link's ids, for `node.forget` to name it.
        let id = node_id(7, "sample-repo");
        assert!(id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'));
    }

    #[test]
    fn her_notes_come_back_in_her_words_with_what_became_of_them() {
        let note = |n: usize, action: &str, said: bool| Note {
            event: format!("evt_{n}"),
            happened: "waiting".into(),
            action: action.into(),
            line: format!("line {n}"),
            at: chrono::DateTime::from_timestamp(1_790_000_000 + n as i64, 0).unwrap(),
            said,
        };
        assert_eq!(told(&[]), None);
        let notes: Vec<Note> = (0..8)
            .map(|n| match n % 3 {
                0 => note(n, "speak", true),
                1 => note(n, "speak", false),
                _ => note(n, "remember", true),
            })
            .collect();
        let text = told(&notes).unwrap();
        assert!(!text.contains("line 1\n") && text.contains("line 2") && text.contains("line 7"));
        assert!(text.contains("- you noted (waiting"));
        assert!(text.contains("- you said (waiting"));
        assert!(text.contains("- you had this to say and did not (waiting"));
    }

    /// Kept for the owner alone: made once, each decision once, said when it
    /// is, logged line by line, and gone with its log when she forgets it (an
    /// isolated schema; skipped without a test database).
    #[tokio::test]
    async fn a_node_keeps_her_decisions_until_she_forgets_it() {
        let Some(fixture) = crate::federation::test_db::SchemaDb::new_or_media().await else {
            return;
        };
        let db = &fixture.db;
        let (owner, other) = (1_930_901, 1_930_902);
        for user in [owner, other] {
            db.execute_unprepared(&format!(
                "INSERT INTO users (id, username) VALUES ({user}, 'nodes{user}')"
            ))
            .await
            .unwrap();
        }
        let mut heard = subscribe();
        let repo = place(PEER_AGENT_EVENT, &facts(&[("repo", "sample-repo")])).unwrap();
        let line = |event: &str, action: &str, said: bool| Note {
            event: event.into(),
            happened: "waiting".into(),
            action: action.into(),
            line: "它在等你回答。".into(),
            at: crate::clock::now(),
            said,
        };
        let decision = |event: &'static str, action: &'static str, note: Option<Note>| Decided {
            event,
            happened: "waiting",
            action,
            reason: "test",
            origin: "dev_mac",
            note,
        };
        let speak = || decision("evt_a", "speak", Some(line("evt_a", "speak", false)));
        decided(db, owner, &repo, speak()).await.unwrap();
        // Heard by the links of this owner (other tests may be heard first).
        let mine = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while heard.recv().await.ok() != Some(owner) {}
        });
        assert!(mine.await.is_ok());
        // The same event again changes nothing; another without a line keeps only its reference.
        decided(db, owner, &repo, speak()).await.unwrap();
        decided(db, owner, &repo, decision("evt_b", "ignore", None))
            .await
            .unwrap();
        let note = Some(line("evt_c", "remember", true));
        decided(db, owner, &repo, decision("evt_c", "remember", note))
            .await
            .unwrap();
        let shown = list(db, owner).await.unwrap();
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].id, node_id(owner, "sample-repo"));
        assert_eq!(
            (shown[0].form, shown[0].origin.as_str()),
            (Form::Repo, "dev_mac")
        );
        let events: Vec<(&str, bool)> = shown[0]
            .notes
            .iter()
            .map(|note| (note.event.as_str(), note.said))
            .collect();
        assert_eq!(events, [("evt_a", false), ("evt_c", true)]);
        assert!(list(db, other).await.unwrap().is_empty());
        // Said: her words in the end, logged.
        said(db, owner, "evt_a", "它在等你回答要不要改测试。")
            .await
            .unwrap();
        said(db, other, "evt_c", "x").await.unwrap();
        let note = &list(db, owner).await.unwrap()[0].notes[0];
        assert!(note.said && note.line == "它在等你回答要不要改测试。");
        let log = db
            .query_all_raw(statement(
                "SELECT seq, change FROM agent_node_events WHERE node_id = $1 ORDER BY seq",
                vec![node_id(owner, "sample-repo").into()],
            ))
            .await
            .unwrap();
        let log: Vec<(i64, String)> = log
            .iter()
            .map(|row| {
                (
                    row.try_get("", "seq").unwrap(),
                    row.try_get("", "change").unwrap(),
                )
            })
            .collect();
        let changes: Vec<&str> = log.iter().map(|(_, change)| change.as_str()).collect();
        assert_eq!(
            changes,
            [
                "created", "decided", "raised", "decided", "decided", "noted", "said"
            ]
        );
        assert!(
            log.iter()
                .enumerate()
                .all(|(n, (seq, _))| *seq == n as i64 + 1)
        );
        // What she noted comes with the next moment about it.
        assert!(
            notes_for(db, owner, &repo)
                .await
                .unwrap()
                .contains("you noted")
        );
        assert_eq!(notes_for(db, other, &repo).await, None);
        // Only the owner's own is forgotten, and with its log.
        let id = node_id(owner, "sample-repo");
        assert!(!forget(db, other, &id).await.unwrap());
        assert!(forget(db, owner, &id).await.unwrap());
        assert!(list(db, owner).await.unwrap().is_empty());
        let left = db
            .query_one_raw(statement(
                "SELECT count(*) AS n FROM agent_node_events WHERE node_id = $1",
                vec![id.clone().into()],
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(left.try_get::<i64>("", "n").unwrap(), 0);
        assert!(!forget(db, owner, &id).await.unwrap());
        fixture.close().await;
    }
}
