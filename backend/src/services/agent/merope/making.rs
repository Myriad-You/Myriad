//! What she makes on her own time: for now, a turtle soup of her own, from
//! something she took in (the rules are `myriad_merope::making`). Kept as
//! her own row, the truth only in what is kept with it; tried on people as
//! they play it (see `soup`), and how it went stays with it.

use std::time::Duration;

use chrono::Utc;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};

use crate::models::entities::agent_memories;
use crate::services::agent::memory::unified;
use myriad_merope::making::{
    MAKE_SCHEMA, Made, SOURCE, Tried, kept_line, make_input, make_schema, make_system, parse_idea,
};
use myriad_merope::soup::{Ending, Game};

const CALL_TIMEOUT: Duration = Duration::from_secs(60);
/// Puzzles of hers read back, newest first.
const READ_BACK: u64 = 40;
const MATERIAL_CHARS: usize = 2000;
fn made_of(row: &agent_memories::Model) -> Option<Made> {
    let mut made: Made = serde_json::from_str(row.evidence.as_deref()?).ok()?;
    made.id = row.id.clone();
    Some(made)
}

/// Her puzzles, oldest first.
pub(super) async fn all(db: &DatabaseConnection) -> Vec<Made> {
    let mut made: Vec<Made> = unified::own_rows(db, SOURCE, READ_BACK)
        .await
        .unwrap_or_default()
        .iter()
        .filter_map(made_of)
        .collect();
    made.reverse();
    made
}

/// Her oldest puzzle not yet tried at `table` (`soup::Table::record_id`).
pub(super) async fn untried_at(db: &DatabaseConnection, table: &str) -> Option<Made> {
    myriad_merope::making::untried_at(&all(db).await, table).cloned()
}

/// After she took something in that she liked: maybe it gives her an idea
/// for a puzzle of her own, and she makes it. How many she made today and
/// how many no one has played are hers to weigh, not a cap.
pub(super) async fn maybe_make(
    db: &DatabaseConnection,
    owner: i32,
    what: &str,
    took_in: &str,
    material: Option<&str>,
) {
    let made = all(db).await;
    let today = super::clock::local_now().date_naive();
    let made_today = unified::own_rows(db, SOURCE, READ_BACK)
        .await
        .unwrap_or_default()
        .iter()
        .filter(|row| row.created_at.with_timezone(&chrono::Local).date_naive() == today)
        .count();
    let unplayed = made.iter().filter(|made| made.tried.is_empty()).count();
    let soul: String = crate::services::agent::identity::get_speaking_soul()
        .await
        .unwrap_or_default();
    let material: String = material
        .unwrap_or_default()
        .chars()
        .take(MATERIAL_CHARS)
        .collect();
    let recent: Vec<Made> = made.iter().rev().take(8).cloned().collect();
    let raw = super::call::Ask::new(super::call::Voice::HersAtLength, owner, "make_soup")
        .within(CALL_TIMEOUT)
        .json_raw(
            &make_system(&soul, what),
            &make_input(took_in, &material, &recent, made_today, unplayed),
            MAKE_SCHEMA,
            &make_schema(),
        )
        .await;
    let Some(Some(mut puzzle)) = raw.ok().and_then(|raw| parse_idea(&raw)) else {
        return;
    };
    puzzle.from = format!("{what}: {took_in}").chars().take(200).collect();
    let evidence = serde_json::to_string(&puzzle).unwrap_or_default();
    let concepts = vec![unified::Concept {
        name: "海龟汤".into(),
        aliases: vec!["turtle soup".into(), "情境猜谜".into()],
    }];
    match unified::remember_own(db, &kept_line(&puzzle), &evidence, concepts, SOURCE).await {
        Ok(Some(_)) => tracing::info!("[Merope] she made up a turtle soup of her own"),
        Ok(None) => {}
        Err(error) => tracing::warn!(%error, "[Merope] could not keep a puzzle she made"),
    }
}

/// How a game of hers went at `table`, kept with the puzzle.
pub(super) async fn tried(db: &DatabaseConnection, table: &str, game: &Game) {
    let Some(id) = game.made.as_deref() else {
        return;
    };
    let Ok(Some(row)) = agent_memories::Entity::find_by_id(id.to_string())
        .one(db)
        .await
    else {
        return;
    };
    let Some(mut made) = made_of(&row) else {
        return;
    };
    made.tried.push(Tried {
        table: table.to_string(),
        at: Utc::now(),
        ending: match game.ending {
            Some(Ending::Solved) => "solved",
            Some(Ending::GaveUp) => "gave_up",
            None => "left",
        }
        .to_string(),
        asked: game.asked.len(),
        solver: game.solver.clone(),
    });
    let evidence = serde_json::to_string(&made).unwrap_or_default();
    let updated = agent_memories::Entity::update_many()
        .col_expr(
            agent_memories::Column::Evidence,
            sea_orm::sea_query::Expr::value(Some(evidence)),
        )
        .col_expr(
            agent_memories::Column::UpdatedAt,
            sea_orm::sea_query::Expr::value(Utc::now().fixed_offset()),
        )
        .filter(agent_memories::Column::Id.eq(id))
        .exec(db)
        .await;
    if let Err(error) = updated {
        tracing::warn!(%error, "[Merope] could not keep how her puzzle went");
    }
}
