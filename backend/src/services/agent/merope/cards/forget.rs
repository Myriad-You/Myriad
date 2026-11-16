//! A card goes with what it was about. When what a card stands on is
//! forgotten for good (the owner dropped the grant its sessions came under),
//! the card is forgotten with it: its lines of its own go, and her lines that
//! carried it keep her words and lose the card. Nothing of it is told to her
//! as an answer, and a child kept current reads the talk again. What of it
//! went into her log is forgotten there too, so every side lets it go.

use sea_orm::{ConnectionTrait, DatabaseBackend, DbErr, Statement};

use super::{CARD_ROLE, announce};

/// Forgets every card of the owner's about one of `subjects` (`node:<id>`).
/// How many cards went.
pub(crate) async fn forget_about<C: ConnectionTrait + sea_orm::TransactionTrait>(
    db: &C,
    owner: i32,
    subjects: &[String],
) -> Result<usize, DbErr> {
    if subjects.is_empty() {
        return Ok(0);
    }
    let ids: Vec<String> = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT DISTINCT metadata->>'cardId' AS id FROM agent_proactive_messages \
             WHERE user_id = $1 AND metadata->'card'->'card'->>'subject' = ANY($2)",
            [owner.into(), subjects.to_vec().into()],
        ))
        .await?
        .iter()
        .filter_map(|row| row.try_get::<Option<String>>("", "id").ok().flatten())
        .collect();
    if ids.is_empty() {
        return Ok(0);
    }
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM agent_proactive_messages \
         WHERE user_id = $1 AND role = $2 AND metadata->>'cardId' = ANY($3)",
        [owner.into(), CARD_ROLE.into(), ids.clone().into()],
    ))
    .await?;
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE agent_proactive_messages \
         SET metadata = metadata - 'cardId' - 'card' - 'cardVersions' - 'cardAnswers' \
         WHERE user_id = $1 AND metadata->>'cardId' = ANY($2)",
        [owner.into(), ids.clone().into()],
    ))
    .await?;
    announce(db, owner).await?;
    for card in &ids {
        if let Err(error) = crate::services::her_log::cards::forget(db, owner, card).await {
            tracing::warn!(%error, "[cards] a forgotten card is not forgotten in her log yet");
        }
    }
    Ok(ids.len())
}
