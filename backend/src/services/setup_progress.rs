//! Whether the site is set up: its tables exist and someone has claimed it.
//! Shared by the setup wizard's HTTP status and the agent's `setup.status`.

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};

/// Shared install-progress flags for HTTP `/api/setup/status` and Agent `setup.status`.
/// AI keys are not part of setup completeness.
pub(crate) struct SetupProgress {
    pub has_database: bool,
    pub has_admin_user: bool,
    pub is_setup_required: bool,
    pub missing_configs: Vec<String>,
}

pub(crate) fn setup_progress_from_flags(has_database: bool, has_admin_user: bool) -> SetupProgress {
    let mut missing_configs = Vec::new();
    if !has_database {
        missing_configs.push("Database tables not initialized".to_string());
    }
    if !has_admin_user {
        missing_configs.push("No admin user registered".to_string());
    }
    SetupProgress {
        has_database,
        has_admin_user,
        is_setup_required: !has_database || !has_admin_user,
        missing_configs,
    }
}

/// A failed read is `Err`: an unreachable database is not an uninitialized one.
pub(crate) async fn inspect_setup_progress(
    db: &DatabaseConnection,
) -> Result<SetupProgress, sea_orm::DbErr> {
    let has_database = check_database_tables(db).await?;
    let has_admin_user = if has_database {
        crate::services::principal::installation_claimed(db).await?
    } else {
        false
    };
    Ok(setup_progress_from_flags(has_database, has_admin_user))
}

/// Check if required database tables exist
pub(crate) async fn check_database_tables(db: &DatabaseConnection) -> Result<bool, sea_orm::DbErr> {
    // Check multiple critical tables to ensure migrations completed
    // Using platforms table since it's the first migration (001)
    let result = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT EXISTS (
                SELECT FROM information_schema.tables 
                WHERE table_name = 'platforms'
            ) as platforms_exists,
            EXISTS (
                SELECT FROM information_schema.tables 
                WHERE table_name = 'users'
            ) as users_exists",
            vec![],
        ))
        .await;

    let row = result?.ok_or_else(|| {
        sea_orm::DbErr::RecordNotFound("database tables check returned no row".to_string())
    })?;
    let platforms_exists: bool = row.try_get("", "platforms_exists")?;
    let users_exists: bool = row.try_get("", "users_exists")?;

    // Both tables should exist for complete setup
    let exists = platforms_exists && users_exists;
    tracing::info!(
        "Database tables check - platforms: {}, users: {}, complete: {}",
        platforms_exists,
        users_exists,
        exists
    );
    Ok(exists)
}
