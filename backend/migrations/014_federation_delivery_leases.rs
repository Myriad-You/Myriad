use sea_orm_migration::prelude::*;

/// Give each outbound delivery claim an explicit owner and expiry.
///
/// Existing `delivering` rows are intentionally left untouched. They retain a
/// NULL token and continue through the legacy ten-minute recovery predicate;
/// this avoids rewriting live queue state during a rolling deployment.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                r#"
ALTER TABLE federation_delivery_queue
    ADD COLUMN IF NOT EXISTS lease_token UUID,
    ADD COLUMN IF NOT EXISTS lease_expires_at TIMESTAMPTZ;

CREATE INDEX IF NOT EXISTS idx_delivery_lease_expiry
    ON federation_delivery_queue (status, lease_expires_at);
"#,
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                r#"
DROP INDEX IF EXISTS idx_delivery_lease_expiry;
ALTER TABLE federation_delivery_queue
    DROP COLUMN IF EXISTS lease_expires_at,
    DROP COLUMN IF EXISTS lease_token;
"#,
            )
            .await?;
        Ok(())
    }
}
