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

#[path = "012_federation_inbox_receipts.rs"]
mod federation_inbox_receipts;

#[path = "013_federation_inbox_receipts_v2.rs"]
mod federation_inbox_receipts_v2;

#[path = "014_federation_delivery_leases.rs"]
mod federation_delivery_leases;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        let mut migrations: Vec<Box<dyn MigrationTrait>> = vec![
            Box::new(initial_schema::Migration),
            Box::new(tapp_system::Migration),
            Box::new(brew_system::Migration),
            Box::new(agent_system::Migration),
            Box::new(federation::Migration),
            Box::new(oauth_identities::Migration),
        ];
        // Applied migration names are an immutable compatibility contract.
        // These historical implementations were folded into the complete
        // greenfield schema, but their names must remain so startup never has
        // to rewrite `seaql_migrations` to make history appear valid.
        migrations.extend(retired_history::migrations());
        migrations.push(Box::new(federation_inbox_receipts::Migration));
        migrations.push(Box::new(federation_inbox_receipts_v2::Migration));
        migrations.push(Box::new(federation_delivery_leases::Migration));
        migrations
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn migration_names_are_unique_and_retain_published_history() {
        let migrations = Migrator::migrations();
        let names: Vec<&str> = migrations
            .iter()
            .map(|migration| migration.name())
            .collect();
        let unique: HashSet<&str> = names.iter().copied().collect();
        assert_eq!(unique.len(), names.len(), "migration names must be unique");

        for retired in retired_history::RETIRED_MIGRATION_NAMES {
            assert!(
                unique.contains(retired),
                "published migration history entry {retired} must never be removed"
            );
        }
        assert!(
            unique.contains("012_federation_inbox_receipts"),
            "durable federation receipt migration must remain registered"
        );
        assert!(
            unique.contains("013_federation_inbox_receipts_v2"),
            "legacy receipt-shape repair must remain registered"
        );
        assert!(
            unique.contains("014_federation_delivery_leases"),
            "delivery lease ownership migration must remain registered"
        );
    }
}
