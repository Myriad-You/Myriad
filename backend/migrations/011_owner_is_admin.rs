use sea_orm_migration::prelude::*;

/// Heal: site owner must always be admin.
///
/// After 010, some DBs can have `is_owner = true` with `is_admin = false`
/// (e.g. empty/weird DBs that seeded the lowest-id non-admin as owner). That
/// owner cannot pass `ensure_current_admin` / use admin UI, and non-owners
/// cannot grant admin (only owner can) → site unmanageable via UI.
///
/// This migration is idempotent:
/// 1. `UPDATE users SET is_admin = true WHERE is_owner = true AND is_admin = false`
/// 2. If still zero owners, re-seed with same COALESCE priority as 010, setting
///    **both** `is_owner` and `is_admin` on the chosen row.
/// 3. Zero users → no-op (UPDATE matches nothing).
///
/// Runtime mirror: `schema_check::ensure_single_owner` applies the same heal
/// on every startup (belt-and-suspenders with this durable migration).
///
/// Invariant after heal: `is_owner = true` implies `is_admin = true`.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // Column may be missing on exotic paths; 010 should have added it.
        // IF NOT EXISTS keeps this safe if reordered.
        db.execute_unprepared(
            "ALTER TABLE users ADD COLUMN IF NOT EXISTS is_owner BOOLEAN NOT NULL DEFAULT false",
        )
        .await?;

        // Owner implies admin.
        db.execute_unprepared(
            "UPDATE users SET is_admin = true \
             WHERE is_owner = true AND is_admin = false",
        )
        .await?;

        // If still no owner (and at least one user exists), seed one with both flags.
        // Priority: id=1 if admin → lowest-id admin → lowest-id user.
        db.execute_unprepared(
            "UPDATE users SET is_owner = true, is_admin = true \
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
        // Irreversible heal: do not strip is_admin from owners on down.
        let _ = manager;
        Ok(())
    }
}
