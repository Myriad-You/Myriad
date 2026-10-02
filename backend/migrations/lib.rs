use sea_orm::{DatabaseBackend, Statement};
pub use sea_orm_migration::prelude::*;

// 数据库结构定义
#[path = "001_initial_schema.rs"]
mod initial_schema;

#[path = "002_tapp_system.rs"]
mod tapp_system;

#[path = "003_phantasi_system.rs"]
mod phantasi_system;

#[path = "004_agent_system.rs"]
mod agent_system;

#[path = "005_federation.rs"]
mod federation;

#[path = "006_oauth_identities.rs"]
mod oauth_identities;

/// Channel on which every change to a room membership row is announced, so
/// live room sockets can re-check whether their member still belongs.
pub const ROOM_MEMBERSHIP_CHANNEL: &str = "myriad_room_membership";

/// Trigger announcing membership changes on [`ROOM_MEMBERSHIP_CHANNEL`].
/// Shared by migration 005 and the runtime schema heal.
pub const ROOM_MEMBERSHIP_NOTIFY_SQL: &str = r#"
CREATE OR REPLACE FUNCTION federation_room_membership_notify() RETURNS trigger AS $$
BEGIN
    PERFORM pg_notify('myriad_room_membership', OLD.room_id);
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;
DROP TRIGGER IF EXISTS federation_room_membership_notify ON federation_room_members;
CREATE TRIGGER federation_room_membership_notify
    AFTER UPDATE OR DELETE ON federation_room_members
    FOR EACH ROW EXECUTE FUNCTION federation_room_membership_notify();
"#;

pub const SOURCE_RECENT_INDEX_SQL: &str = "CREATE INDEX IF NOT EXISTS idx_phantasi_items_source_recent ON phantasi_items (source_id, published_at DESC NULLS LAST, id DESC)";

/// The schema mark 0.6.1 writes once its startup heals have all run
/// (`schema_check::SCHEMA_VERSION` at that release). Marks are dated, so
/// they compare as text.
pub const SUPPORT_FLOOR_SCHEMA_MARK: &str = "2026.09.29.1";

/// An existing database must have finished a 0.6.1 (or later) startup:
/// the renames and heals for anything older are gone, and skipping them
/// would fail later, less clearly, or not at all. A new database passes.
const REFUSE_BELOW_SUPPORT_FLOOR_SQL: &str = r#"
DO $$
BEGIN
    -- Two statements: PL/pgSQL plans each one when it first runs, and a
    -- single OR would name seaql_migrations before a new database has it.
    IF to_regclass('seaql_migrations') IS NULL THEN
        RETURN;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM seaql_migrations) THEN
        RETURN;
    END IF;
    IF to_regclass('_schema_versions') IS NULL THEN
        RAISE EXCEPTION 'this database predates Myriad 0.6.1: upgrade to 0.6.1 first, start it once, then upgrade to this release';
    END IF;
    IF NOT EXISTS (SELECT 1 FROM _schema_versions WHERE version >= '@FLOOR@') THEN
        RAISE EXCEPTION 'this database predates Myriad 0.6.1: upgrade to 0.6.1 first, start it once, then upgrade to this release';
    END IF;
END $$;
"#;

pub struct Migrator;

impl Migrator {
    /// Refuse a database older than the support floor (0.6.1), keep only
    /// versions that still have files in `seaql_migrations`, then apply
    /// 001–006.
    ///
    /// SeaORM rejects applied versions that have no file *before* any `up()`
    /// body runs, so this wrapper is the only `Migrator::up` call path.
    pub async fn up<'c, C>(db: C, steps: Option<u32>) -> Result<(), DbErr>
    where
        C: IntoSchemaManagerConnection<'c>,
    {
        let executor = db.into_database_executor();
        executor
            .execute_unprepared(
                &REFUSE_BELOW_SUPPORT_FLOOR_SQL.replace("@FLOOR@", SUPPORT_FLOOR_SCHEMA_MARK),
            )
            .await?;
        discard_unknown_migration_history(&executor).await?;
        <Self as MigratorTrait>::up(executor, steps).await
    }
}

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(initial_schema::Migration),
            Box::new(tapp_system::Migration),
            Box::new(phantasi_system::Migration),
            Box::new(agent_system::Migration),
            Box::new(federation::Migration),
            Box::new(oauth_identities::Migration),
        ]
    }
}

fn keep_migration_versions() -> Vec<String> {
    Migrator::migrations()
        .iter()
        .map(|migration| migration.name().to_string())
        .collect()
}

fn discard_unknown_history_sql(keep_count: usize) -> String {
    let placeholders = (1..=keep_count)
        .map(|index| format!("${index}"))
        .collect::<Vec<_>>()
        .join(",");
    format!("DELETE FROM seaql_migrations WHERE version NOT IN ({placeholders})")
}

/// `seaql_migrations` may only record versions that still have files —
/// otherwise SeaORM demands a no-op. Structure folded into 001–006 leaves
/// such rows behind on older databases.
async fn discard_unknown_migration_history(db: &impl ConnectionTrait) -> Result<(), DbErr> {
    let present = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT 1
               FROM information_schema.tables
              WHERE table_schema = 'public'
                AND table_name = 'seaql_migrations'"
                .to_string(),
        ))
        .await?;
    if present.is_empty() {
        return Ok(());
    }

    let keep = keep_migration_versions();
    if keep.is_empty() {
        return Err(DbErr::Custom(
            "migrator file list is empty; refusing to discard seaql_migrations".into(),
        ));
    }
    let keep_len = keep.len();
    let params: Vec<sea_orm::Value> = keep.into_iter().map(Into::into).collect();
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        discard_unknown_history_sql(keep_len),
        params,
    ))
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn migrator_contains_greenfield_schema() {
        let migrations = Migrator::migrations();
        let names: Vec<String> = migrations
            .iter()
            .map(|migration| migration.name().to_string())
            .collect();
        let unique: HashSet<&str> = names.iter().map(String::as_str).collect();
        assert_eq!(unique.len(), names.len(), "migration names must be unique");
        assert_eq!(
            names,
            [
                "001_initial_schema",
                "002_tapp_system",
                "003_phantasi_system",
                "004_agent_system",
                "005_federation",
                "006_oauth_identities",
            ]
        );
        assert!(
            names
                .iter()
                .all(|name| { name.starts_with("00") && name.as_bytes()[2].is_ascii_digit() }),
            "migration versions stay zero-padded numeric"
        );
    }

    #[test]
    fn unknown_history_sql_keeps_only_migrator_files() {
        let keep = keep_migration_versions();
        let sql = discard_unknown_history_sql(keep.len());
        assert!(sql.starts_with("DELETE FROM seaql_migrations WHERE version NOT IN ("));
        assert_eq!(keep.len(), 6);
        for index in 1..=keep.len() {
            assert!(sql.contains(&format!("${index}")));
        }
        assert!(!sql.contains(&format!("${}", keep.len() + 1)));
    }

    #[test]
    fn the_floor_mark_is_a_dated_schema_mark() {
        let parts: Vec<&str> = SUPPORT_FLOOR_SCHEMA_MARK.split('.').collect();
        assert_eq!(parts.len(), 4, "YYYY.MM.DD.N compares as text");
        assert!(REFUSE_BELOW_SUPPORT_FLOOR_SQL.contains("@FLOOR@"));
        assert!(REFUSE_BELOW_SUPPORT_FLOOR_SQL.contains("upgrade to 0.6.1 first"));
    }
}
