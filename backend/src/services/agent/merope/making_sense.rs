//! Making sense of a group chat before answering in it, and carrying what
//! people there told her about herself (see `myriad_merope::making_sense`).

use std::time::Duration;

use sea_orm::DatabaseConnection;

use super::call::{self, Voice};
use crate::services::agent::memory::unified;
use myriad_merope::making_sense::{SCHEMA_NAME, Sense, input, parse, schema, system};

pub const SOURCE: &str = "about_me";
/// Reading is done while she would be reading anyway; past this she answers
/// without it.
const READ_WITHIN: Duration = Duration::from_secs(8);
/// What she was told stays with her this long unless it comes up again.
const FADE_AFTER: chrono::Duration = chrono::Duration::days(60);
/// The same thing, still on the screen, is read again every turn: once in
/// this long is being told once.
const TOLD_ONCE_WITHIN: chrono::Duration = chrono::Duration::minutes(30);
const TOLD_SHOWN: u64 = 3;

/// The words a judgment's answer says she is not sure of.
fn unsure_in(raw: &str) -> Vec<String> {
    myriad_agent_rules::extract_json_object_from_ai_response(raw.trim())
        .and_then(|json| serde_json::from_str::<serde_json::Value>(&json).ok())
        .map(|value| myriad_merope::memes::unsure_terms(&value))
        .unwrap_or_default()
}

/// What she makes of a group's talk: `conversation` is its recent lines,
/// hers as `you：…`, the one she answers last. Billed to `owner`.
pub async fn read(owner: i32, conversation: &[String]) -> Option<Sense> {
    let soul = crate::services::agent::identity::get_speaking_soul()
        .await
        .unwrap_or_default();
    let raw = call::Ask::new(Voice::Judge, owner, "group_making_sense")
        .within(READ_WITHIN)
        .json_raw(&system(&soul), &input(conversation), SCHEMA_NAME, &schema())
        .await
        .ok()?;
    // Words or memes in the talk she is not sure of: hers to look up.
    super::memes::learn(owner, unsure_in(&raw), conversation);
    parse(&raw)
}

/// Keep what people in the group at `venue` (stored form, `group:…`) told
/// her about herself, unless she was told something there just now.
pub async fn remember_told(db: &DatabaseConnection, venue: &str, told: &str, line: &str) {
    let lately = unified::latest_in_venue(db, venue, SOURCE, 1)
        .await
        .unwrap_or_default()
        .into_iter()
        .any(|row| chrono::Utc::now().fixed_offset() - row.created_at < TOLD_ONCE_WITHIN);
    if lately {
        return;
    }
    let evidence = serde_json::json!({ "line": line }).to_string();
    match unified::remember_in_venue(db, venue, told, &evidence, SOURCE).await {
        Ok(Some(_)) => {
            tracing::info!(%venue, "[Merope] kept what the group told her about herself")
        }
        Ok(None) => {}
        Err(error) => tracing::warn!(%error, "[Merope] could not keep what she was told"),
    }
}

/// What people in the group told her about herself, latest first, as a
/// section, if anything.
pub async fn told_section(db: &DatabaseConnection, venue: &str) -> Option<String> {
    let rows = unified::latest_in_venue(db, venue, SOURCE, TOLD_SHOWN)
        .await
        .ok()?;
    let now = chrono::Utc::now();
    let told: Vec<(String, String)> = rows
        .into_iter()
        .map(|row| {
            (
                row.content,
                myriad_merope::doing::ago_text(now - row.created_at.with_timezone(&chrono::Utc)),
            )
        })
        .collect();
    myriad_merope::making_sense::told_section(&told)
}

/// Keep what someone told her about herself in private, unless they told
/// her something just now.
pub async fn remember_told_by(db: &DatabaseConnection, user_id: i32, told: &str) {
    let told: String = told.chars().take(160).collect();
    let lately = unified::latest_of(db, user_id, SOURCE, 1)
        .await
        .unwrap_or_default()
        .into_iter()
        .any(|row| chrono::Utc::now().fixed_offset() - row.created_at < TOLD_ONCE_WITHIN);
    if lately || told.trim().is_empty() {
        return;
    }
    let kept = unified::remember(
        db,
        unified::NewMemory {
            user_id,
            kind: unified::MemoryKind::Fact,
            content: told,
            evidence: None,
            speaker: unified::Speaker::Agent,
            source: SOURCE,
            audience: unified::Audience::private(user_id),
            importance: 0.5,
            concepts: Vec::new(),
        },
    )
    .await;
    if let Err(error) = kept {
        tracing::warn!(%error, "[Merope] could not keep what she was told");
    }
}

/// What this person told her about herself, latest first, as a section.
pub async fn told_by_section(db: &DatabaseConnection, user_id: i32) -> Option<String> {
    let rows = unified::latest_of(db, user_id, SOURCE, TOLD_SHOWN)
        .await
        .ok()?;
    let now = chrono::Utc::now();
    let told: Vec<(String, String)> = rows
        .into_iter()
        .filter(|row| !row.venue.starts_with("group:"))
        .map(|row| {
            (
                row.content,
                myriad_merope::doing::ago_text(now - row.created_at.with_timezone(&chrono::Utc)),
            )
        })
        .collect();
    myriad_merope::making_sense::told_section_titled(
        &told,
        "What they have told you about yourself",
    )
}

pub async fn let_fade(db: &DatabaseConnection) {
    if let Err(error) = unified::fade_source(db, SOURCE, FADE_AFTER).await {
        tracing::warn!(%error, "[Merope] could not let what she was told fade");
    }
}
