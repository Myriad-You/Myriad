use sea_orm_migration::prelude::*;

/// Durable site owner flag (`users.is_owner`).
///
/// Previously privilege gates used a hard-coded primary admin heuristic (`id = 1`).
/// This migration introduces a boolean column and seeds exactly one owner:
/// 1. user id=1 if it exists and is admin
/// 2. else lowest-id admin
/// 3. else lowest-id user
///
/// Idempotent: if any owner already exists, only collapses multiples to the lowest id.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            "ALTER TABLE users ADD COLUMN IF NOT EXISTS is_owner BOOLEAN NOT NULL DEFAULT false",
        )
        .await?;

        // At most one owner (partial unique index on a constant expression).
        db.execute_unprepared(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_users_single_owner \
             ON users ((true)) WHERE is_owner = true",
        )
        .await?;

        // Collapse multiple owners → keep lowest id.
        db.execute_unprepared(
            "UPDATE users SET is_owner = false \
             WHERE is_owner = true \
               AND id <> (SELECT MIN(id) FROM users WHERE is_owner = true)",
        )
        .await?;

        // If none, seed one (id=1 admin → lowest admin → lowest user).
        db.execute_unprepared(
            "UPDATE users SET is_owner = true \
             WHERE id = COALESCE( \
               (SELECT id FROM users WHERE id = 1 AND is_admin = true LIMIT 1), \
               (SELECT id FROM users WHERE is_admin = true ORDER BY id ASC LIMIT 1), \
               (SELECT id FROM users ORDER BY id ASC LIMIT 1) \
             ) \
             AND NOT EXISTS (SELECT 1 FROM users WHERE is_owner = true)",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP INDEX IF EXISTS idx_users_single_owner")
            .await?;
        db.execute_unprepared("ALTER TABLE users DROP COLUMN IF EXISTS is_owner")
            .await?;
        Ok(())
    }
}
