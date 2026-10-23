//! What people told her about herself in private, and letting old tellings
//! fade (see `myriad_merope::making_sense`). In a group her reading of the
//! talk, and what the group told her, is her group mind's
//! (`myriad_merope::group`).

use sea_orm::DatabaseConnection;

use crate::services::agent::memory::unified;

pub const SOURCE: &str = "about_me";
/// What she was told stays with her this long unless it comes up again.
const FADE_AFTER: chrono::Duration = chrono::Duration::days(60);
/// The same thing, still on the screen, is read again every turn: once in
/// this long is being told once.
const TOLD_ONCE_WITHIN: chrono::Duration = chrono::Duration::minutes(30);
const TOLD_SHOWN: u64 = 3;

/// Keep what someone told her about herself in private, unless they told
/// her something just now.
pub async fn remember_told_by(db: &DatabaseConnection, user_id: i32, told: &str) {
    let told: String = told.chars().take(160).collect();
    let lately = unified::latest_of(db, user_id, SOURCE, 1)
        .await
        .unwrap_or_default()
        .into_iter()
        .any(|row| crate::clock::now().fixed_offset() - row.created_at < TOLD_ONCE_WITHIN);
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
    let now = crate::clock::now();
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
