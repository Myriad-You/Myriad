//! What still stings with each person (see `myriad_merope::sore`).
//!
//! A sore spot is born in her private reflection after an exchange (see
//! `inner`), marked there when they apologize or make it right, and let go
//! of when she goes over a day with them at night (see `bits`). Let go of,
//! it stays among what she remembers of them, as something that happened
//! and is behind them; left untouched for two months, it fades on its own.
//!
//! Hers about them, kept with that person and apart from ordinary memory
//! while it stings. One born in private is heard only in private with them;
//! one born in a group happened in front of others, and is heard in that
//! group when she answers them there, and in private with them.

use chrono::{DateTime, Utc};
use sea_orm::DatabaseConnection;
use serde_json::{Value, json};

use crate::services::agent::memory::unified::{self, Audience};
use myriad_merope::sore::{MAX_OPEN, WHAT_CHARS};
pub use myriad_merope::sore::{Sore, Weight, as_input, carried_section, section};

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
        user_id: row.user_id?,
        what: row.content.clone(),
        // Kept before there were weights: petty or not.
        weight: evidence
            .get("weight")
            .and_then(Value::as_str)
            .and_then(Weight::parse)
            .unwrap_or(
                if evidence.get("petty").and_then(Value::as_bool) == Some(true) {
                    Weight::Petty
                } else {
                    Weight::Hurt
                },
            ),
        since: at("since").unwrap_or_else(|| row.created_at.with_timezone(&Utc)),
        mended: at("mended"),
        venue: row.venue.clone(),
        who: None,
    })
}

/// Where a sore spot of `user_id`'s born at `venue` is kept.
fn audience_of(user_id: i32, venue: &str) -> Audience {
    match venue.strip_prefix("group:") {
        Some(group) => Audience::group(group, user_id),
        None => Audience::private(user_id),
    }
}

/// All of this person's sore spots, wherever they were born, oldest first.
/// How much what still stings with them lowers where her mood toward them
/// settles (see `myriad_merope::sore::mood_weighs`). Runs inside the caller's
/// transaction: a failed read is returned, not taken as "nothing stings",
/// because it has already aborted that transaction.
pub(crate) async fn weighs_on<C: sea_orm::ConnectionTrait>(
    db: &C,
    user_id: i32,
) -> Result<f64, sea_orm::DbErr> {
    let sores: Vec<Sore> = unified::latest_of(db, user_id, SOURCE, MAX_OPEN as u64 * 3)
        .await?
        .iter()
        .filter_map(sore_of)
        .collect();
    Ok(myriad_merope::sore::mood_weighs(&sores))
}

pub async fn open_all(db: &DatabaseConnection, user_id: i32) -> Vec<Sore> {
    let mut sores: Vec<Sore> = unified::latest_of(db, user_id, SOURCE, MAX_OPEN as u64 * 3)
        .await
        .unwrap_or_default()
        .iter()
        .filter_map(sore_of)
        .collect();
    sores.sort_by_key(|sore| sore.since);
    sores
}

/// Sore spots born in a group (`venue` as sessions keep it), oldest first:
/// one person's, or everyone's there, each with who did it.
pub async fn open_in_group(
    db: &DatabaseConnection,
    venue: &str,
    user_id: Option<i32>,
) -> Vec<Sore> {
    let mut sores: Vec<Sore> = unified::venue_source_rows(
        db,
        user_id,
        &format!("group:{venue}"),
        SOURCE,
        MAX_OPEN as u64 * 3,
    )
    .await
    .unwrap_or_default()
    .iter()
    .filter_map(sore_of)
    .collect();
    // Who did it, by the name the group's talk shows them by.
    if user_id.is_none() && !sores.is_empty() {
        let names = crate::services::channel_group::names_here(db, venue).await;
        for sore in &mut sores {
            sore.who = Some(
                names
                    .get(&sore.user_id)
                    .cloned()
                    .unwrap_or_else(|| "someone in the group".to_string()),
            );
        }
    }
    sores.sort_by_key(|sore| sore.since);
    sores
}

#[allow(clippy::too_many_arguments)]
async fn put(
    db: &DatabaseConnection,
    user_id: i32,
    audience: Audience,
    what: &str,
    weight: Weight,
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
                    "weight": weight.as_str(),
                    "since": since.to_rfc3339(),
                    "mended": mended.map(|at| at.to_rfc3339()),
                })
                .to_string(),
            ),
            speaker: unified::Speaker::Agent,
            source: SOURCE,
            audience,
            importance: match weight {
                Weight::Petty => 0.4,
                Weight::Hurt => 0.7,
                Weight::Deep => 0.9,
            },
            concepts: Vec::new(),
        },
    )
    .await;
}

/// Something they did got to her, where it happened (`audience`). Past the
/// limit, the oldest of theirs goes.
pub async fn keep(
    db: &DatabaseConnection,
    user_id: i32,
    audience: Audience,
    what: &str,
    weight: Weight,
) {
    let what: String = what.trim().chars().take(WHAT_CHARS).collect();
    if what.is_empty() {
        return;
    }
    let held = open_all(db, user_id).await;
    if held.len() >= MAX_OPEN {
        let oldest: Vec<String> = held
            .iter()
            .take(held.len() + 1 - MAX_OPEN)
            .map(|sore| sore.id.clone())
            .collect();
        let _ = unified::retire(db, user_id, &oldest, "faded").await;
    }
    put(db, user_id, audience, &what, weight, Utc::now(), None).await;
}

/// They apologized or made it right: it may still sting, and she knows.
pub async fn mend(db: &DatabaseConnection, sores: &[Sore], indexes: &[usize]) {
    for sore in indexes.iter().filter_map(|index| sores.get(*index)) {
        if sore.mended.is_some() {
            continue;
        }
        if unified::retire(db, sore.user_id, &[sore.id.clone()], "superseded")
            .await
            .unwrap_or(0)
            > 0
        {
            put(
                db,
                sore.user_id,
                audience_of(sore.user_id, &sore.venue),
                &sore.what,
                sore.weight,
                sore.since,
                Some(Utc::now()),
            )
            .await;
        }
    }
}

/// She has let these go. What happened stays among what she remembers of
/// them, as behind them now.
pub async fn let_go(db: &DatabaseConnection, sores: &[Sore], indexes: &[usize]) {
    for sore in indexes.iter().filter_map(|index| sores.get(*index)) {
        let user_id = sore.user_id;
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
                audience: audience_of(user_id, &sore.venue),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sore_spot_is_kept_where_it_happened() {
        assert_eq!(audience_of(7, "private").venue(), "private");
        assert_eq!(
            audience_of(7, "group:onebot:123").venue(),
            "group:onebot:123"
        );
    }
}
