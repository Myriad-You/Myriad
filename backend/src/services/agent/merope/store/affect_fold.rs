//! Folding her recorded affect changes into where replay starts: site
//! upkeep (see `upkeep`), which runs whichever end is her parent.

use super::addressee::lock_addressee;
use super::*;

/// How long her recorded changes toward the site's owner are kept before
/// they are folded into the anchor (spec 1.4.3: about 4.7 of her mood's
/// settling time, so what is folded no longer shows).
pub const OWNER_CHANGES_KEPT: chrono::Duration = chrono::Duration::days(14);
/// How long changes toward anyone else are kept, for looking back on.
pub const OTHERS_CHANGES_KEPT: chrono::Duration = chrono::Duration::days(30);

/// Fold each person's recorded changes older than they are kept into their
/// anchor, then drop what the anchor holds: replay from the moved anchor
/// gives the same affect. The owner's are kept [`OWNER_CHANGES_KEPT`],
/// everyone else's [`OTHERS_CHANGES_KEPT`]. Returns the changes folded.
pub async fn fold_old_affect_changes(
    db: &DatabaseConnection,
    owner: Option<i32>,
) -> Result<u64, anyhow::Error> {
    let now = super::super::clock::now();
    let people: Vec<i32> = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT user_id FROM agent_affect_anchor ORDER BY user_id",
        ))
        .await?
        .iter()
        .filter_map(|row| row.try_get("", "user_id").ok())
        .collect();
    let mut folded = 0;
    for user_id in people {
        let kept = if Some(user_id) == owner {
            OWNER_CHANGES_KEPT
        } else {
            OTHERS_CHANGES_KEPT
        };
        let upto = now - kept;
        let transaction = db.begin().await?;
        lock_addressee(&transaction, user_id).await?;
        let Some((anchor, changes)) = affect_log_on(&transaction, user_id, Some(upto)).await?
        else {
            transaction.commit().await?;
            continue;
        };
        let moved = myriad_persona_sync::affect::fold(&anchor, &changes, upto.timestamp_micros())
            .map_err(|error| anyhow::anyhow!("folding her affect: {error}"))?;
        if moved != anchor {
            let at = chrono::DateTime::from_timestamp_micros(moved.at)
                .ok_or_else(|| anyhow::anyhow!("anchor time out of range"))?
                .fixed_offset();
            let rev = myriad_persona_sync::hlc::legacy(now.timestamp_micros(), None)
                .map_err(|error| anyhow::anyhow!("anchor revision: {error}"))?
                .to_string();
            transaction
                .execute_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "UPDATE agent_affect_anchor SET mood = $2, arousal = $3, emotion = $4, \
                     emotion_arousal = $5, at = $6, through_event = $7, rev = $8 \
                     WHERE user_id = $1",
                    vec![
                        user_id.into(),
                        moved.affect.mood.into(),
                        moved.affect.arousal.into(),
                        moved.affect.emotion.into(),
                        moved.affect.emotion_arousal.into(),
                        at.into(),
                        moved.through_event.clone().into(),
                        rev.into(),
                    ],
                ))
                .await?;
        }
        // What the anchor holds no longer needs keeping.
        if let Some(through) = &moved.through_event {
            let at = chrono::DateTime::from_timestamp_micros(moved.at)
                .ok_or_else(|| anyhow::anyhow!("anchor time out of range"))?
                .fixed_offset();
            folded += transaction
                .execute_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "DELETE FROM agent_affect_events WHERE user_id = $1 \
                     AND (at < $2 OR (at = $2 AND id COLLATE \"C\" <= $3))",
                    vec![user_id.into(), at.into(), through.clone().into()],
                ))
                .await?
                .rows_affected();
        }
        transaction.commit().await?;
    }
    Ok(folded)
}
