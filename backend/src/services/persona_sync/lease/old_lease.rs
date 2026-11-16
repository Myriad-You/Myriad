//! The one-row host lease from before her lease was the record of which end
//! holds her (`agent_host_lease`), taken apart once at start: its holder
//! and epoch become her lease's grants, the rest her copy and her moves.
//! Then the table goes.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbErr, Statement, TransactionTrait};
use serde_json::Value;

use super::{Done, Move, Phase, SITE, Starter, State, rows};

/// Takes the old row apart, if the old table is still here; run by the
/// schema check after her log's tables are made (the grants live there).
pub async fn take_old_lease(db: &DatabaseConnection) -> Result<(), DbErr> {
    let old = db
        .query_one_raw(Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT to_regclass('agent_host_lease')::text AS name",
        ))
        .await?
        .and_then(|row| row.try_get::<Option<String>>("", "name").ok().flatten());
    if old.is_none() {
        return Ok(());
    }
    let txn = db.begin().await?;
    let row = txn
        .query_one_raw(Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT holder, lineage, epoch, state, move, done FROM agent_host_lease WHERE id = 'her'",
        ))
        .await?;
    if let Some(row) = row
        && rows::copy(&txn, true).await?.is_none()
    {
        let holder: String = row.try_get("", "holder")?;
        let lineage: String = row.try_get("", "lineage")?;
        let epoch: i64 = row.try_get("", "epoch")?;
        let state: String = row.try_get("", "state")?;
        let moving: Option<Value> = row.try_get("", "move")?;
        let done: Option<Value> = row.try_get("", "done")?;
        let copy = match state.as_str() {
            "vacant" => State::Vacant,
            "moved_out" => State::MovedOut,
            _ if holder == SITE => State::Live,
            // Living on a device, or frozen on her way in from one.
            _ => State::Away,
        };
        rows::put_copy(&txn, &lineage, copy).await?;
        // Her lease names the end the row named, at its epoch or later.
        rows::lock(&txn).await?;
        match rows::standing(&txn).await? {
            None => rows::begin_with(&txn, &holder, epoch).await?,
            Some(grant) if grant.held_by(&holder) => {}
            Some(grant) => {
                rows::sign(
                    &txn,
                    myriad_persona_log::lease::Request::Force { to: holder.clone() },
                    Some(epoch.max(grant.epoch as i64) + 1),
                )
                .await?;
            }
        }
        if let Some(done) = done.and_then(|done| serde_json::from_value::<Done>(done).ok()) {
            let now = crate::clock::now();
            rows::put_move(
                &txn,
                &Move {
                    id: done.id,
                    kind: done.kind,
                    from: String::new(),
                    to: holder.clone(),
                    phase: Phase::Committed,
                    by: Starter::Owner,
                    digest: None,
                    started_at: now,
                    confirmed_at: None,
                    agreed: None,
                    import: done.import,
                    via: None,
                    was: None,
                },
            )
            .await?;
            rows::finish(&txn, done.epoch).await?;
        }
        // A move this build cannot read is dropped: the owner starts it again.
        match moving.map(serde_json::from_value::<Move>) {
            Some(Ok(moving)) => {
                rows::put_move(&txn, &moving).await?;
                if state == "frozen" {
                    rows::hold_still(&txn, true, Some(&moving.id)).await?;
                }
            }
            Some(Err(error)) => {
                tracing::warn!(%error, "[host] a move kept on the old lease does not read; dropped");
            }
            None => {}
        }
        tracing::info!(%holder, epoch, %state, "[host] the old host lease was taken apart into her lease, copy and moves");
    }
    txn.execute_unprepared(
        "DROP TABLE agent_host_lease; \
         DROP FUNCTION IF EXISTS agent_host_lease_guard(), agent_host_lease_changed()",
    )
    .await?;
    txn.commit().await?;
    Ok(())
}
