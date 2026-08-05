use sea_orm_migration::prelude::*;

/// Historical migration names that shipped before their changes were folded
/// into the current greenfield migrations.
///
/// Removing a migration implementation after it has been recorded makes
/// SeaORM reject the whole history.  Keep an immutable no-op entry instead of
/// deleting rows from `seaql_migrations` at process startup.  Existing
/// databases skip names they already applied; new databases record the no-op
/// after the complete 001-006 schema has been created.
pub struct RetiredMigration(&'static str);

impl RetiredMigration {
    pub const fn new(name: &'static str) -> Self {
        Self(name)
    }
}

impl MigrationName for RetiredMigration {
    fn name(&self) -> &str {
        self.0
    }
}

#[async_trait::async_trait]
impl MigrationTrait for RetiredMigration {
    async fn up(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}

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
];

pub fn migrations() -> impl Iterator<Item = Box<dyn MigrationTrait>> {
    RETIRED_MIGRATION_NAMES
        .iter()
        .copied()
        .map(|name| Box::new(RetiredMigration::new(name)) as Box<dyn MigrationTrait>)
}
