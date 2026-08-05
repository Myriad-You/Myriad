use sea_orm_migration::prelude::*;

/// Durable cross-process/restart idempotency for signed federation inbox
/// activities.  The `(signer, activity_id)` key is authoritative; the raw
/// body digest prevents an attacker or a buggy peer from reusing an id for
/// different bytes.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                r#"
CREATE TABLE IF NOT EXISTS federation_inbox_receipts (
    id BIGSERIAL PRIMARY KEY,
    signer TEXT NOT NULL,
    activity_id TEXT NOT NULL,
    body_digest CHAR(64) NOT NULL,
    status VARCHAR(16) NOT NULL DEFAULT 'processing',
    attempts INTEGER NOT NULL DEFAULT 1,
    lease_until TIMESTAMPTZ,
    outcome_status SMALLINT,
    error_message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    accepted_at TIMESTAMPTZ,
    CONSTRAINT federation_inbox_receipts_status_check
        CHECK (status IN ('processing', 'accepted', 'failed', 'rejected')),
    CONSTRAINT federation_inbox_receipts_digest_check
        CHECK (body_digest ~ '^[0-9a-f]{64}$'),
    CONSTRAINT federation_inbox_receipts_identity_unique
        UNIQUE (signer, activity_id)
);
CREATE INDEX IF NOT EXISTS idx_federation_inbox_receipts_status_lease
    ON federation_inbox_receipts (status, lease_until);
CREATE INDEX IF NOT EXISTS idx_federation_inbox_receipts_updated
    ON federation_inbox_receipts (updated_at DESC);
"#,
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("DROP TABLE IF EXISTS federation_inbox_receipts;")
            .await?;
        Ok(())
    }
}
