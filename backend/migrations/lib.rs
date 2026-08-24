pub use sea_orm_migration::prelude::*;

// 数据库结构定义
#[path = "001_initial_schema.rs"]
mod initial_schema;

#[path = "002_tapp_system.rs"]
mod tapp_system;

#[path = "003_brew_system.rs"]
mod brew_system;

#[path = "004_agent_system.rs"]
mod agent_system;

#[path = "005_federation.rs"]
mod federation;

#[path = "006_oauth_identities.rs"]
mod oauth_identities;

mod retired_history;

// 016 is data cleanup (#336). Schema columns live in 001–006; this migration
// still has to run on existing installs that carry retired permission strings.
#[path = "016_tapp_legacy_grant_clear.rs"]
mod tapp_legacy_grant_clear;

pub use retired_history::{purge_retired_migration_history, RETIRED_MIGRATION_NAMES};

pub struct Migrator;

impl Migrator {
    /// Strip folded 007–015 names from `seaql_migrations`, drop leftover
    /// `digital_life_*` experiment tables, then apply 001–006 + 016.
    ///
    /// SeaORM rejects applied versions that have no file *before* any `up()`
    /// body runs, so 016 cannot delete those rows itself. This wrapper is the
    /// only `Migrator::up` call path.
    pub async fn up<'c, C>(db: C, steps: Option<u32>) -> Result<(), DbErr>
    where
        C: IntoSchemaManagerConnection<'c>,
    {
        let executor = db.into_database_executor();
        purge_retired_migration_history(&executor).await?;
        <Self as MigratorTrait>::up(executor, steps).await
    }
}

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(initial_schema::Migration),
            Box::new(tapp_system::Migration),
            Box::new(brew_system::Migration),
            Box::new(agent_system::Migration),
            Box::new(federation::Migration),
            Box::new(oauth_identities::Migration),
            Box::new(tapp_legacy_grant_clear::Migration),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn migrator_contains_only_greenfield_and_grant_clear() {
        let migrations = Migrator::migrations();
        let names: Vec<&str> = migrations
            .iter()
            .map(|migration| migration.name())
            .collect();
        let unique: HashSet<&str> = names.iter().copied().collect();
        assert_eq!(unique.len(), names.len(), "migration names must be unique");
        assert_eq!(
            names,
            [
                "001_initial_schema",
                "002_tapp_system",
                "003_brew_system",
                "004_agent_system",
                "005_federation",
                "006_oauth_identities",
                "016_tapp_legacy_grant_clear",
            ]
        );
        for retired in RETIRED_MIGRATION_NAMES {
            assert!(
                !unique.contains(retired),
                "folded history entry {retired} must not stay in the migrator"
            );
        }
    }
}
