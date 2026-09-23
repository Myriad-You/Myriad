//! Bounded maintenance, owned by the process cleanup loop on every worker.
use super::{MediaError, MediaService};
use crate::models::entities::media_assets;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, QuerySelect};

pub async fn maintain(db: &DatabaseConnection) -> Result<(), MediaError> {
    let service = MediaService::from_data_paths(crate::services::data_paths::paths());
    let recovery = service.recover_expired(db, 16).await;
    let deletions = retry_deletions(&service, db, 16).await;
    recovery.map(|_| ()).and(deletions)
}

pub(super) async fn retry_deletions(
    service: &MediaService,
    db: &DatabaseConnection,
    limit: u64,
) -> Result<(), MediaError> {
    let rows = media_assets::Entity::find()
        .filter(media_assets::Column::State.eq("deleting"))
        .order_by_asc(media_assets::Column::UpdatedAt)
        .limit(limit.clamp(1, 32))
        .all(db)
        .await?;
    for row in rows {
        // Touch before retry so a permanently failing file cannot starve others.
        use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE media_assets SET updated_at = NOW() WHERE id = $1 AND state = 'deleting'",
            [row.id.into()],
        ))
        .await?;
        if let Err(error) = service.delete(db, row.id).await {
            tracing::warn!(asset_id = row.id, %error, "media deletion retry failed");
        }
    }
    Ok(())
}

/// Owned by the process lifecycle; never awaited by database/schema startup.
pub fn start_upgrade_worker() -> tokio::task::JoinHandle<()> {
    tokio::spawn(async {
        loop {
            // Give initialization time to connect the DB. No elapsed-time test
            // marks the migration complete; its durable cursor is authoritative.
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            let Ok(db) = crate::services::tapp_registry::database() else {
                continue;
            };
            let service = MediaService::from_data_paths(crate::services::data_paths::paths());
            let legacy = super::LegacyPaths::from_data_paths(crate::services::data_paths::paths());
            let origins = super::upgrade::configured_origins().await;
            match super::upgrade::automatic_step(
                &db,
                service.store(),
                &legacy,
                &origins,
                chrono::Utc::now().timestamp(),
            )
            .await
            {
                Ok(Some(progress)) if progress.complete => {
                    tracing::info!(
                        scanned = progress.scanned,
                        unresolved = progress.unresolved,
                        "media upgrade completed"
                    )
                }
                Ok(Some(progress)) if progress.error.is_some() => tracing::warn!(
                    error = ?progress.error, source = ?progress.error_source, pending_failures = progress.pending_failures, next_retry_at = ?progress.next_retry_at,
                    "media upgrade has deferred records; normal records continue before retry"),
                Ok(_) => {}
                Err(error) => {
                    // Disconnected/uninitialized DB cannot persist its backoff yet.
                    tracing::warn!(%error, "media upgrade unavailable; retrying later");
                    tokio::time::sleep(std::time::Duration::from_secs(55)).await;
                }
            }
        }
    })
}
