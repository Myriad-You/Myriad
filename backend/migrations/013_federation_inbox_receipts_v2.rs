use sea_orm_migration::prelude::*;

/// Compatibility repair for databases that ran the short-lived first shape
/// of migration 012 from this PR branch.
///
/// Changing 012 itself cannot repair those databases because SeaORM will not
/// rerun a migration name already present in `seaql_migrations`. The old shape
/// had no inbox scope, so its rows cannot be mapped without reintroducing the
/// cross-inbox suppression bug; rebuild is the only truthful conversion.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                r#"
DO $$
BEGIN
    IF to_regclass('federation_inbox_receipts') IS NOT NULL
       AND NOT EXISTS (
           SELECT 1
           FROM information_schema.columns
           WHERE table_schema = current_schema()
             AND table_name = 'federation_inbox_receipts'
             AND column_name = 'inbox_scope'
       )
    THEN
        DROP TABLE federation_inbox_receipts;
    END IF;
END $$;

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

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // 012 owns the current table. This compatibility step has no safe way
        // to reconstruct scope-less receipts and therefore has no standalone
        // reverse transformation.
        Ok(())
    }
}
