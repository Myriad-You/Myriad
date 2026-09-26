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

#[path = "007_local_music.rs"]
mod local_music;

#[path = "008_local_music_playlists.rs"]
mod local_music_playlists;

mod ai_cost_ledger_rename;
mod ai_quota_usage_rename;
mod phantasi_legacy_rename;
mod runtime_registry_rename;

pub use ai_cost_ledger_rename::rename_ai_cost_ledger_if_needed;
pub use ai_quota_usage_rename::rename_ai_quota_usage_if_needed;
pub use phantasi_legacy_rename::rename_brew_to_phantasi_if_needed;
pub use runtime_registry_rename::rename_runtime_registry_if_needed;

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

pub struct Migrator;

impl Migrator {
    /// Drop leftover `digital_life_*` experiment tables, keep only versions
    /// that still have files in `seaql_migrations`, then apply 001–006.
    ///
    /// SeaORM rejects applied versions that have no file *before* any `up()`
    /// body runs, so this wrapper is the only `Migrator::up` call path.
    pub async fn up<'c, C>(db: C, steps: Option<u32>) -> Result<(), DbErr>
    where
        C: IntoSchemaManagerConnection<'c>,
    {
        let executor = db.into_database_executor();
        rename_brew_to_phantasi_if_needed(&executor).await?;
        rename_runtime_registry_if_needed(&executor).await?;
        rename_ai_cost_ledger_if_needed(&executor).await?;
        rename_ai_quota_usage_if_needed(&executor).await?;
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
            Box::new(local_music::Migration),
            Box::new(local_music_playlists::Migration),
        ]
    }
}

/// Prefix-scan drop for local/dev `digital_life_*` leftovers. No table catalog.
const DROP_DIGITAL_LIFE_SQL: &str = r#"
DO $$
DECLARE
    r RECORD;
BEGIN
    FOR r IN
        SELECT tablename
          FROM pg_tables
         WHERE schemaname = 'public'
           AND tablename LIKE 'digital_life!_%' ESCAPE '!'
    LOOP
        EXECUTE format('DROP TABLE IF EXISTS public.%I CASCADE', r.tablename);
    END LOOP;

    FOR r IN
        SELECT t.typname
          FROM pg_type t
          JOIN pg_namespace n ON n.oid = t.typnamespace
         WHERE n.nspname = 'public'
           AND t.typname LIKE 'digital_life!_%' ESCAPE '!'
           AND t.typtype IN ('e', 'd', 'c')
    LOOP
        EXECUTE format('DROP TYPE IF EXISTS public.%I CASCADE', r.typname);
    END LOOP;

    IF to_regclass('public._schema_versions') IS NOT NULL THEN
        DELETE FROM _schema_versions
         WHERE version LIKE 'digital_life%'
            OR version LIKE '%digital_life%';
    END IF;
END $$;
"#;

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

/// `digital_life_*` tables must not remain. `seaql_migrations` may only
/// record versions that still have files — otherwise SeaORM demands a no-op.
async fn discard_unknown_migration_history(db: &impl ConnectionTrait) -> Result<(), DbErr> {
    db.execute_unprepared(DROP_DIGITAL_LIFE_SQL).await?;

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
                "007_local_music",
                "008_local_music_playlists",
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
        assert_eq!(keep.len(), 8);
        for index in 1..=keep.len() {
            assert!(sql.contains(&format!("${index}")));
        }
        assert!(!sql.contains(&format!("${}", keep.len() + 1)));
        assert!(!keep.iter().any(|name| name.contains("digital_life")));
    }

    #[test]
    fn digital_life_drop_sql_is_prefix_only() {
        assert!(DROP_DIGITAL_LIFE_SQL.contains("LIKE 'digital_life!_%' ESCAPE '!'"));
        assert!(DROP_DIGITAL_LIFE_SQL.contains("DROP TABLE IF EXISTS public.%I CASCADE"));
        assert!(DROP_DIGITAL_LIFE_SQL.contains("DROP TYPE IF EXISTS public.%I CASCADE"));
        assert!(!DROP_DIGITAL_LIFE_SQL.contains("agent_persona"));
        assert!(!DROP_DIGITAL_LIFE_SQL.contains("digital_life_characters"));
    }
}
