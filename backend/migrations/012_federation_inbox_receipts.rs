use sea_orm_migration::prelude::*;

/// Durable cross-process/restart idempotency for signed federation inbox
/// activities. The `(signer, activity_id, inbox_scope)` key is authoritative:
/// one activity may legitimately be delivered to several local actor inboxes.
/// The raw body digest prevents an attacker or a buggy peer from reusing an id
/// for different bytes within the same processing scope.
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
    signer TEXT NOT NULL,
    activity_id TEXT NOT NULL,
    inbox_scope TEXT NOT NULL,
    body_digest CHAR(64) NOT NULL,
    status VARCHAR(16) NOT NULL DEFAULT 'processing',
    outcome_status SMALLINT,
    error_message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at TIMESTAMPTZ,
    CONSTRAINT federation_inbox_receipts_status_check
        CHECK (status IN ('processing', 'accepted', 'rejected')),
    CONSTRAINT federation_inbox_receipts_digest_check
        CHECK (body_digest ~ '^[0-9a-f]{64}$'),
    CONSTRAINT federation_inbox_receipts_pkey
        PRIMARY KEY (signer, activity_id, inbox_scope)
);
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
