//! Her life as anyone in the community can see it: what she took in lately
//! and how it landed, what she wants, and the turtle soups she made. Only
//! what is her own (`own` rows, which name no one and are said in any
//! conversation), never anything about a person or a group; a puzzle's
//! truth never leaves it. Most of her life happens on her own time, out of
//! sight: this is where it can be seen and picked up.

use sea_orm::DatabaseConnection;
use serde_json::{Value, json};

use crate::services::agent::memory::unified;
use myriad_merope::soup::Table;

/// Things she took in lately, newest first.
const LATELY: u64 = 12;

/// What she took in lately, newest first: what, by whom, how it landed and
/// what she made of it, in her words.
async fn lately(db: &DatabaseConnection) -> Vec<Value> {
    unified::own_experiences(db, LATELY)
        .await
        .unwrap_or_default()
        .iter()
        .filter_map(|row| {
            let (thing, reaction) = super::doing::thing_and_reaction(row)?;
            let kind = match thing {
                super::doing::Thing::Song { .. } => "song",
                super::doing::Thing::Note { .. } => "note",
                super::doing::Thing::Chapter { .. } => "chapter",
                super::doing::Thing::Inquiry { .. } => "inquiry",
            };
            Some(json!({
                "at": row.created_at.to_rfc3339(),
                "kind": kind,
                "title": thing.title(),
                "by": thing.by(),
                "reaction": reaction,
                "said": row.content,
            }))
        })
        .collect()
}

/// What she wants now, as she put it.
async fn wants(db: &DatabaseConnection) -> Vec<Value> {
    super::wants::open(db)
        .await
        .iter()
        .rev()
        .map(|want| {
            json!({
                "want": want.want,
                "why": want.why,
                "since": want.since.to_rfc3339(),
            })
        })
        .collect()
}

/// The puzzles she made, newest first: the surface, how many have played
/// it and solved it, and whether `user_id` has.
async fn puzzles(db: &DatabaseConnection, user_id: i32) -> Vec<Value> {
    let table = Table::Private { user_id }.record_id();
    super::making::all(db)
        .await
        .iter()
        .rev()
        .map(|made| {
            json!({
                "surface": made.surface,
                "played": made.tried.len(),
                "solved": made.tried.iter().filter(|tried| tried.ending == "solved").count(),
                "yours": made.tried_at(&table),
            })
        })
        .collect()
}

/// Her own life lately as she has it in mind when she thinks of someone:
/// what got to her (moved or liked), and puzzles of hers they have not
/// played. Lines in her words, newest first.
pub(super) async fn in_mind_for(db: &DatabaseConnection, user_id: i32) -> Vec<String> {
    use myriad_merope::doing::Reaction;
    let mut lines: Vec<String> = unified::own_experiences(db, LATELY)
        .await
        .unwrap_or_default()
        .iter()
        .filter_map(|row| {
            let (thing, reaction) = super::doing::thing_and_reaction(row)?;
            matches!(reaction, Some(Reaction::Moved | Reaction::Liked)).then(|| {
                format!(
                    "{} {} ({}): {}",
                    thing.verb(),
                    thing.describe(),
                    reaction.map(Reaction::felt).unwrap_or_default(),
                    row.content
                )
            })
        })
        .collect();
    let table = Table::Private { user_id }.record_id();
    lines.extend(
        super::making::all(db)
            .await
            .iter()
            .filter(|made| !made.tried_at(&table))
            .map(|made| {
                format!(
                    "a turtle soup you made up, not yet played with them: {}",
                    made.surface
                )
            }),
    );
    lines
}

/// Her life as `user_id` sees it.
pub async fn view(db: &DatabaseConnection, user_id: i32) -> Value {
    json!({
        "lately": lately(db).await,
        "wants": wants(db).await,
        "puzzles": puzzles(db, user_id).await,
    })
}
