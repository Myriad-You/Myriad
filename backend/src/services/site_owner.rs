//! Site owner resolution for public/dashboard surfaces.
//!
//! Prefer durable `users.is_owner`; fall back to lowest admin id for pre-is_owner DBs.
//! Lives in services so reports/config/scheduler do not reach through HTTP profile handlers.

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};

pub async fn site_owner_user_id(db: &DatabaseConnection) -> Result<i32, String> {
    // 1) Durable site owner flag
    if let Ok(Some(row)) = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT id FROM users WHERE is_owner = true ORDER BY id ASC LIMIT 1".to_string(),
        ))
        .await
    {
        if let Ok(id) = row.try_get::<i32>("", "id") {
            return Ok(id);
        }
    }

    // 2) Legacy: first admin (pre-is_owner installs / column missing)
    let row = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT id FROM users WHERE is_admin = true ORDER BY id ASC LIMIT 1".to_string(),
        ))
        .await
        .map_err(|error| format!("Failed to resolve site owner: {error}"))?;
    row.and_then(|row| row.try_get::<i32>("", "id").ok())
        .ok_or_else(|| "No administrator is configured as the site owner".to_string())
}
