//! What still stings with each person (see `myriad_merope::sore`).
//!
//! A sore spot is born in her private reflection after an exchange (see
//! `inner`), marked there when they apologize or make it right, and let go
//! of when she goes over a day with them at night (see `bits`). Let go of,
//! it stays among what she remembers of them, as something that happened
//! and is behind them; left untouched for two months, it fades on its own.
//!
//! Hers about them, kept with that person, heard only in private with them,
//! and apart from ordinary memory while it stings.

use chrono::{DateTime, Utc};
use sea_orm::DatabaseConnection;
use serde_json::{Value, json};

use crate::services::agent::memory::unified::{self, Audience};
use myriad_merope::sore::{MAX_OPEN, WHAT_CHARS};
pub use myriad_merope::sore::{Sore, as_input, section};

pub const SOURCE: &str = "sore";
/// Untouched this long, a sore spot fades.
const FADES_AFTER: chrono::Duration = chrono::Duration::days(60);

fn sore_of(row: &crate::models::entities::agent_memories::Model) -> Option<Sore> {
    let evidence: Value = serde_json::from_str(row.evidence.as_deref()?).ok()?;
    let at = |key: &str| {
        evidence
            .get(key)
            .and_then(Value::as_str)
            .and_then(|at| DateTime::parse_from_rfc3339(at).ok())
            .map(|at| at.with_timezone(&Utc))
    };
    Some(Sore {
        id: row.id.clone(),
        what: row.content.clone(),
        petty: evidence
            .get("petty")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        since: at("since").unwrap_or_else(|| row.created_at.with_timezone(&Utc)),
        mended: at("mended"),
    })
}

/// What still stings with this person, oldest first.
pub async fn open(db: &DatabaseConnection, user_id: i32) -> Vec<Sore> {
    let mut sores: Vec<Sore> = unified::venue_source_rows(
        db,
        Some(user_id),
        &Audience::private(user_id).venue(),
        SOURCE,
        MAX_OPEN as u64 * 2,
    )
    .await
    .unwrap_or_default()
    .iter()
    .filter_map(sore_of)
    .collect();
    sores.sort_by_key(|sore| sore.since);
    sores
}

async fn put(
    db: &DatabaseConnection,
    user_id: i32,
    what: &str,
    petty: bool,
    since: DateTime<Utc>,
    mended: Option<DateTime<Utc>>,
) {
    let _ = unified::remember(
        db,
        unified::NewMemory {
            user_id,
            kind: unified::MemoryKind::Fact,
            content: what.to_string(),
            evidence: Some(
                json!({
                    "petty": petty,
                    "since": since.to_rfc3339(),
                    "mended": mended.map(|at| at.to_rfc3339()),
                })
                .to_string(),
            ),
            speaker: unified::Speaker::Agent,
            source: SOURCE,
            audience: Audience::private(user_id),
            importance: if petty { 0.4 } else { 0.7 },
            concepts: Vec::new(),
        },
    )
    .await;
}

/// Something they did got to her. Past the limit, the oldest goes.
pub async fn keep(db: &DatabaseConnection, user_id: i32, what: &str, petty: bool) {
    let what: String = what.trim().chars().take(WHAT_CHARS).collect();
    if what.is_empty() {
        return;
    }
    let held = open(db, user_id).await;
    if held.len() >= MAX_OPEN {
        let oldest: Vec<String> = held
            .iter()
            .take(held.len() + 1 - MAX_OPEN)
            .map(|sore| sore.id.clone())
            .collect();
        let _ = unified::retire(db, user_id, &oldest, "faded").await;
    }
    put(db, user_id, &what, petty, Utc::now(), None).await;
}

/// They apologized or made it right: it may still sting, and she knows.
pub async fn mend(db: &DatabaseConnection, user_id: i32, sores: &[Sore], indexes: &[usize]) {
    for sore in indexes.iter().filter_map(|index| sores.get(*index)) {
        if sore.mended.is_some() {
            continue;
        }
        if unified::retire(db, user_id, &[sore.id.clone()], "superseded")
            .await
            .unwrap_or(0)
            > 0
        {
            put(
                db,
                user_id,
                &sore.what,
                sore.petty,
                sore.since,
                Some(Utc::now()),
            )
            .await;
        }
    }
}

/// She has let these go. What happened stays among what she remembers of
/// them, as behind them now.
pub async fn let_go(db: &DatabaseConnection, user_id: i32, sores: &[Sore], indexes: &[usize]) {
    for sore in indexes.iter().filter_map(|index| sores.get(*index)) {
        if unified::retire(db, user_id, &[sore.id.clone()], "forgiven")
            .await
            .unwrap_or(0)
            == 0
        {
            continue;
        }
        let _ = unified::remember(
            db,
            unified::NewMemory {
                user_id,
                kind: unified::MemoryKind::Fact,
                content: format!("{}（后来放下了）", sore.what.trim()),
                evidence: Some(
                    json!({ "since": sore.since.to_rfc3339(), "letGo": Utc::now().to_rfc3339() })
                        .to_string(),
                ),
                speaker: unified::Speaker::Agent,
                source: "chat",
                audience: Audience::private(user_id),
                importance: 0.3,
                concepts: Vec::new(),
            },
        )
        .await;
        tracing::info!(user_id, "[Merope] she let something go");
    }
}

/// Sore spots nobody touched in two months fade.
pub async fn let_fade(db: &DatabaseConnection) {
    match unified::fade_source(db, SOURCE, FADES_AFTER).await {
        Ok(faded) if faded > 0 => tracing::info!(faded, "[Merope] sore spots faded"),
        Ok(_) => {}
        Err(error) => tracing::warn!(%error, "[Merope] could not let old sore spots fade"),
    }
}
