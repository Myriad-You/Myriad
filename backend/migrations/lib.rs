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

#[path = "007_notification_preferences.rs"]
mod notification_preferences;

#[path = "008_tapp_approved_permissions.rs"]
mod tapp_approved_permissions;

#[path = "009_user_presence.rs"]
mod user_presence;

#[path = "010_user_owner.rs"]
mod user_owner;

#[path = "011_owner_is_admin.rs"]
mod owner_is_admin;

pub struct Migrator;

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
            Box::new(notification_preferences::Migration),
            Box::new(tapp_approved_permissions::Migration),
            Box::new(user_presence::Migration),
            Box::new(user_owner::Migration),
            Box::new(owner_is_admin::Migration),
            // 默认平台种子行（含 X）统一由 001 + runtime schema_check::ensure_default_platforms 维护，
            // 不再为单个平台开独立 migration。
        ]
    }
}
