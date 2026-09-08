use sea_orm::{ConnectionTrait, DatabaseBackend, DbErr, Statement};

/// Folded migration names. Structure lives in 001–006; 016–017 are the
/// remaining post-006 permission data migrations. These rows are deleted from
/// `seaql_migrations` before `MigratorTrait::up` so SeaORM does not require
/// a no-op file for each name.
pub const RETIRED_MIGRATION_NAMES: &[&str] = &[
    "007_notification_preferences",
    "007_digital_life",
    "008_tapp_runtime_registry",
    "008_tapp_approved_permissions",
    "008_digital_life_phase_two",
    "009_activity_events",
    "009_user_presence",
    "009_digital_life_phase_three",
    "010_user_owner",
    "010_digital_life_phase_four",
    "011_owner_is_admin",
    "011_digital_life_asset_subjects",
    "012_federation_inbox_receipts",
    "013_federation_inbox_receipts_v2",
    "014_federation_delivery_leases",
    "015_federation_delivery_health",
];

/// Temporary digital_life experiment tables from retired `007_digital_life`
/// and later phase migrations. Local/dev only — never production. Product
/// confirmed they are throwaway; `DROP … CASCADE` is intentional.
///
/// Later phases are not all listed here. Any remaining `public.digital_life_*`
/// table is dropped by prefix scan. Generically named companions
/// (`image_generation_jobs`, `image_assets`) are not in this prefix and stay.
pub const RETIRED_DIGITAL_LIFE_TABLES: &[&str] = &[
    "digital_life_characters",
    "digital_life_dna_evidence",
    "digital_life_events",
    "digital_life_memories",
    "digital_life_worlds",
    "digital_life_world_objects",
    "digital_life_visual_lineages",
    "digital_life_relationships",
    "digital_life_visits",
    "digital_life_intents",
    "digital_life_growth_log",
    "digital_life_model_calls",
    "digital_life_asset_recipes",
    "digital_life_visit_receipts",
    "digital_life_item_catalog",
    "digital_life_inventory",
    "digital_life_journal_entries",
    "digital_life_discovery_cache",
    "digital_life_relationship_milestones",
    "digital_life_arcs",
    "digital_life_goals",
    "digital_life_timeline_entries",
    "digital_life_world_evolution",
    "digital_life_visual_reviews",
    "digital_life_social_blocks",
    "digital_life_social_proposals",
];

pub fn retired_history_params() -> Vec<sea_orm::Value> {
    RETIRED_MIGRATION_NAMES
        .iter()
        .map(|name| String::from(*name).into())
        .collect()
}

/// `DELETE FROM seaql_migrations WHERE version IN ($1,…)` with one
/// placeholder per `RETIRED_MIGRATION_NAMES` entry.
pub fn purge_retired_history_sql() -> String {
    let placeholders = (1..=RETIRED_MIGRATION_NAMES.len())
        .map(|index| format!("${index}"))
        .collect::<Vec<_>>()
        .join(",");
    format!("DELETE FROM seaql_migrations WHERE version IN ({placeholders})")
}

fn is_safe_digital_life_ident(name: &str) -> bool {
    name.starts_with("digital_life_")
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Drop leftover `digital_life_*` experiment tables/types and matching
/// `_schema_versions` marks.
///
/// Explicit list first, then a prefix scan so phase-008–011 tables not in the
/// const are still removed. Identifier interpolation is only the compile-time
/// const; the scan uses `format('%I')`.
pub fn drop_digital_life_experiment_sql() -> String {
    let explicit = RETIRED_DIGITAL_LIFE_TABLES
        .iter()
        .copied()
        .filter(|name| is_safe_digital_life_ident(name))
        .map(|name| format!("DROP TABLE IF EXISTS public.{name} CASCADE;"))
        .collect::<Vec<_>>()
        .join("\n    ");
    format!(
        r#"
DO $$
DECLARE
    r RECORD;
BEGIN
    {explicit}

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
"#
    )
}

/// Remove folded 007–015 names from `seaql_migrations` and drop leftover
/// `digital_life_*` experiment tables.
///
/// History DELETE no-ops when the tracking table does not exist yet
/// (greenfield). Table DROP is `IF EXISTS` / prefix scan and is also
/// idempotent.
pub async fn purge_retired_migration_history(db: &impl ConnectionTrait) -> Result<u64, DbErr> {
    db.execute_unprepared(&drop_digital_life_experiment_sql())
        .await?;

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
        return Ok(0);
    }
    let result = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            purge_retired_history_sql(),
            retired_history_params(),
        ))
        .await?;
    Ok(result.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn purge_sql_is_single_sourced_from_the_name_list() {
        let sql = purge_retired_history_sql();
        assert!(sql.starts_with("DELETE FROM seaql_migrations WHERE version IN ("));
        assert!(sql.ends_with(')'));
        assert_eq!(
            retired_history_params().len(),
            RETIRED_MIGRATION_NAMES.len()
        );
        for index in 1..=RETIRED_MIGRATION_NAMES.len() {
            assert!(
                sql.contains(&format!("${index}")),
                "purge SQL must bind ${index}"
            );
        }
        assert!(!sql.contains(&format!("${}", RETIRED_MIGRATION_NAMES.len() + 1)));
    }

    #[test]
    fn retired_names_do_not_include_greenfield_or_grant_clear() {
        for kept in [
            "001_initial_schema",
            "002_tapp_system",
            "003_brew_system",
            "004_agent_system",
            "005_federation",
            "006_oauth_identities",
            "016_tapp_legacy_grant_clear",
        ] {
            assert!(
                !RETIRED_MIGRATION_NAMES.contains(&kept),
                "{kept} must stay in the migrator"
            );
        }
    }

    #[test]
    fn digital_life_drop_sql_only_targets_the_experiment_prefix() {
        assert!(
            RETIRED_DIGITAL_LIFE_TABLES
                .iter()
                .all(|name| is_safe_digital_life_ident(name)),
            "explicit drop list must be digital_life_* identifiers"
        );
        let sql = drop_digital_life_experiment_sql();
        for table in RETIRED_DIGITAL_LIFE_TABLES {
            assert!(
                sql.contains(&format!("DROP TABLE IF EXISTS public.{table} CASCADE;")),
                "explicit drop missing {table}"
            );
        }
        assert!(sql.contains("LIKE 'digital_life!_%' ESCAPE '!'"));
        assert!(sql.contains("DROP TABLE IF EXISTS public.%I CASCADE"));
        assert!(sql.contains("DROP TYPE IF EXISTS public.%I CASCADE"));
        assert!(!sql.contains("agent_persona"));
        assert!(!sql.contains("image_generation_jobs"));
        assert!(!sql.contains("image_assets"));
    }
}
