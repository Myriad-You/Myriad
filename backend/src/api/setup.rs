use crate::error::{HttpError, status_json_to_http};
use axum::{
    Json,
    http::{HeaderMap, StatusCode},
};
use myriad_error::AppError;
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use sea_orm_migration::MigratorTrait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::env;
use std::fs;
use std::path::PathBuf;
use url::Url;

/// Setup status response
#[derive(Debug, Serialize, Deserialize)]
pub struct SetupStatus {
    pub is_setup_required: bool,
    pub has_database: bool,
    pub has_admin_user: bool,
    /// 编排 / deploy 预置了 `MYRIAD_SETUP_SECRET` 时，安装写操作必须对上。
    pub setup_secret_required: bool,
    pub missing_configs: Vec<String>,
}

/// GET /api/setup/status
/// Check if initial setup is required
pub async fn check_setup_status(
    crate::extract::Db(db): crate::extract::Db,
) -> Result<Json<SetupStatus>, HttpError> {
    tracing::info!("Checking setup status");

    let progress = inspect_setup_progress(&db).await.map_err(|error| {
        tracing::error!(%error, "setup status cannot be read");
        setup_state_unavailable()
    })?;

    let status = SetupStatus {
        is_setup_required: progress.is_setup_required,
        has_database: progress.has_database,
        has_admin_user: progress.has_admin_user,
        setup_secret_required: crate::api::setup_bootstrap::setup_secret_is_configured(),
        missing_configs: progress.missing_configs,
    };

    tracing::info!("Setup status: {:?}", status);

    Ok(Json(status))
}

/// GET /api/setup/config
/// Get safe configuration info (no secrets)
///
/// Registered on both config-mode (no `AppState`) and full-mode routers, so this
/// reads the process-shared Arc. Same handle as `AppState.dynamic_config` after
/// `from_shared`.
pub async fn get_setup_config() -> Result<Json<Value>, HttpError> {
    // bootstrap-global: no AppState on config-mode setup router
    let config_guard = crate::state::shared_dynamic_config().read().await;

    let config = json!({
        "allow_local_registration": config_guard.allow_local_registration,
        "setup_secret_required": crate::api::setup_bootstrap::setup_secret_is_configured(),
        "setup_window_open": crate::api::setup_bootstrap::setup_window_is_open(),
    });

    Ok(Json(config))
}

// Helper functions

pub(crate) use crate::services::setup_progress::{
    SetupProgress, check_database_tables, inspect_setup_progress, setup_progress_from_flags,
};

/// 500, not 503: the wizard reads 503 as "tables not initialized".
fn setup_state_unavailable() -> HttpError {
    status_json_to_http((
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({
            "error": "Setup state cannot be verified",
            "code": "setup_state_unavailable"
        })),
    ))
}

/// Optional JSON body so `setup_secret` can travel with header-less clients.
#[derive(Debug, Default, Deserialize)]
pub struct SetupSecretBody {
    #[serde(default)]
    pub setup_secret: Option<String>,
}

fn body_setup_secret(body: &Option<Json<SetupSecretBody>>) -> Option<&str> {
    body.as_ref()
        .and_then(|Json(value)| value.setup_secret.as_deref())
}

/// POST /api/setup/init-database
/// Run non-destructive database migrations during initial configuration.
pub async fn init_database(
    crate::extract::Db(db): crate::extract::Db,
    headers: HeaderMap,
    body: Option<Json<SetupSecretBody>>,
) -> Result<Json<Value>, HttpError> {
    crate::api::setup_bootstrap::require_setup_secret(&headers, body_setup_secret(&body))
        .map_err(HttpError)?;
    // Lock this endpoint once an administrator exists. CONFIG_MODE is not
    // flipped here; a live setup-only process needs schedule_setup_restart.
    // An unreadable claim fails closed: never migrate a database whose owner
    // we cannot rule out.
    let admin_exists = crate::services::principal::installation_claimed(&db)
        .await
        .map_err(|error| {
            tracing::error!(%error, "database initialization refused: claim cannot be read");
            setup_state_unavailable()
        })?;

    if admin_exists {
        tracing::error!(
            "🚨 Database initialization REJECTED: Admin user already exists (setup completed)"
        );
        return Err(status_json_to_http((
            StatusCode::FORBIDDEN,
            Json(
                AppError::forbidden("Setup already completed").with_message(
                    "Database has been initialized and an admin user exists. Use the authenticated administration workflow for maintenance.",
                ).to_json(),
            ),
        )));
    }

    tracing::info!("Running database migrations");
    let tables_existed = check_database_tables(&db).await.map_err(|error| {
        tracing::error!(%error, "database initialization refused: tables cannot be read");
        setup_state_unavailable()
    })?;

    // Never drop feature tables from an unauthenticated setup endpoint.
    // Migrator::up drops leftover `digital_life_*` tables, keeps only
    // versions that still have files, then applies 001–006; other damaged
    // migration state requires explicit operator intervention.
    // Import the migrator from migrations module
    use crate::db::Migrator;

    let migrations = <Migrator as MigratorTrait>::migrations();
    tracing::info!(
        "🚀 Starting database migrations with {} migrations",
        migrations.len()
    );

    // Log each migration name
    for (i, migration) in migrations.iter().enumerate() {
        tracing::info!("  Migration {}: {}", i + 1, migration.name());
    }

    // Check existing migrations in seaql_migrations table
    let existing_migrations = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT version FROM seaql_migrations ORDER BY version",
            vec![],
        ))
        .await;

    if let Ok(rows) = existing_migrations {
        let versions: Vec<String> = rows
            .iter()
            .filter_map(|row| row.try_get::<String>("", "version").ok())
            .collect();
        if !versions.is_empty() {
            tracing::info!("📝 Existing migrations in seaql_migrations: {:?}", versions);
        } else {
            tracing::info!("📝 No existing migrations found, will run all migrations");
        }
    }

    match Migrator::up(&db, None).await {
        Ok(_) => {
            tracing::info!("✅ Database migrations completed successfully");

            // Migrator = greenfield CREATE SoT; schema_check heals seeds / owner /
            // recent columns so first-boot does not wait for a process restart.
            if let Err(e) = crate::db::schema_check::ensure_schema(&db).await {
                tracing::error!("Schema ensure after Migrator::up failed: {}", e);
                return Err(status_json_to_http((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({
                        "error": "Schema ensure failed",
                        "code": "schema_ensure_failed"
                    })),
                )));
            }
            crate::SCHEMA_READY.store(true, std::sync::atomic::Ordering::Release);
            tracing::info!("✅ Schema check/heals completed after setup migrations");

            // A legacy database may already contain users. `ensure_schema` can
            // promote its durable owner during migration, which claims the
            // installation just as surely as create-admin does.
            // Unreadable here means "unknown", not "unclaimed": report it and
            // leave the window to the next request's fail-closed gate.
            let installation_claimed = crate::services::principal::installation_claimed(&db)
                .await
                .map_err(|error| {
                    tracing::error!(%error, "claim after database initialization cannot be read");
                    setup_state_unavailable()
                })?;
            if installation_claimed {
                if let Err(error) = crate::api::setup_bootstrap::consume_setup() {
                    tracing::error!(%error, "database initialization claimed installation but setup cleanup failed");
                    let _ = crate::api::setup_bootstrap::invalidate_setup_in_memory();
                    return Err(status_json_to_http((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({
                            "error": "Setup window cleanup failed",
                            "code": "setup_cleanup_failed"
                        })),
                    )));
                }
            }

            // List all tables in the database
            let tables_result = db
                .query_all_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "SELECT table_name FROM information_schema.tables WHERE table_schema = 'public' ORDER BY table_name",
                    vec![],
                ))
                .await;

            if let Ok(rows) = tables_result {
                let table_names: Vec<String> = rows
                    .iter()
                    .filter_map(|row| row.try_get::<String>("", "table_name").ok())
                    .collect();
                tracing::info!("📋 Tables in database: {:?}", table_names);
            }

            // Verify tables were created
            let verify_result = db
                .query_one_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "SELECT 
                        (SELECT COUNT(*) FROM information_schema.tables WHERE table_schema = 'public') as total_tables,
                        EXISTS (SELECT FROM information_schema.tables WHERE table_name = 'users') as users_exists,
                        EXISTS (SELECT FROM information_schema.tables WHERE table_name = 'platforms') as platforms_exists,
                        EXISTS (SELECT FROM information_schema.tables WHERE table_name = 'configurations') as configurations_exists",
                    vec![],
                ))
                .await;

            let (total_tables, users_exists, platforms_exists, configurations_exists) =
                if let Ok(Some(row)) = verify_result {
                    (
                        row.try_get::<i64>("", "total_tables").unwrap_or(0),
                        row.try_get::<bool>("", "users_exists").unwrap_or(false),
                        row.try_get::<bool>("", "platforms_exists").unwrap_or(false),
                        row.try_get::<bool>("", "configurations_exists")
                            .unwrap_or(false),
                    )
                } else {
                    (0, false, false, false)
                };

            tracing::info!(
                "📊 Database verification - Total tables: {}, Users: {}, Platforms: {}, Configurations: {}",
                total_tables,
                users_exists,
                platforms_exists,
                configurations_exists
            );

            let kind = if tables_existed {
                "migrated"
            } else {
                "initialized"
            };

            Ok(Json(json!({
                "success": true,
                "kind": kind,
                "message": if tables_existed {
                    "Database migration checked"
                } else {
                    "Database initialized"
                },
                "recreated": false,
                "verification": {
                    "total_tables": total_tables,
                    "users_table": users_exists,
                    "platforms_table": platforms_exists,
                    "configurations_table": configurations_exists
                }
            })))
        }
        Err(e) => {
            tracing::error!("❌ Database migration failed: {:?}", e);
            Err(status_json_to_http((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "Database migration failed",
                    "code": "db_migration_failed"
                })),
            )))
        }
    }
}

/// Database configuration request
#[derive(Debug, Deserialize)]
pub struct DatabaseConfigRequest {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub database: String,
    /// 与 `.env` 里 `MYRIAD_SETUP_SECRET` 对暗号。也可改走 `X-Setup-Secret`。
    #[serde(default)]
    pub setup_secret: Option<String>,
}

fn build_database_url(config: &DatabaseConfigRequest) -> Result<String, String> {
    let host = config.host.trim();
    let username = config.username.trim();
    let database = config.database.trim();

    if host.is_empty() || username.is_empty() || database.is_empty() {
        return Err("Host, username, and database are required".to_string());
    }
    if config.port == 0 {
        return Err("Database port must be between 1 and 65535".to_string());
    }

    let mut database_url =
        Url::parse("postgres://localhost").map_err(|_| "Invalid database URL".to_string())?;
    database_url
        .set_host(Some(host))
        .map_err(|_| "Invalid database host".to_string())?;
    database_url
        .set_port(Some(config.port))
        .map_err(|_| "Invalid database port".to_string())?;
    database_url
        .set_username(username)
        .map_err(|_| "Invalid database username".to_string())?;
    database_url
        .set_password(Some(&config.password))
        .map_err(|_| "Invalid database password".to_string())?;
    database_url
        .path_segments_mut()
        .map_err(|_| "Invalid database name".to_string())?
        .clear()
        .push(database);

    Ok(database_url.into())
}

/// POST /api/setup/database-config
/// Save database configuration to .env file (专门用于配置数据库)
/// 安全保护：只能在 CONFIG_MODE 下修改数据库配置
pub async fn save_database_config(
    headers: HeaderMap,
    Json(config): Json<DatabaseConfigRequest>,
) -> Result<Json<Value>, HttpError> {
    // 这是最危险的端点：它能把实例重新指向任意 PostgreSQL。
    // CONFIG_MODE 本身会因为数据库故障自动开启，所以它不足以作为唯一门槛 ——
    // 编排预置了安装暗号时必须对上。
    crate::api::setup_bootstrap::require_setup_window().map_err(HttpError)?;
    crate::api::setup_bootstrap::require_setup_secret(&headers, config.setup_secret.as_deref())
        .map_err(HttpError)?;

    // 必须 `CONFIG_MODE=true`。
    let config_mode = crate::CONFIG_MODE.load(std::sync::atomic::Ordering::Relaxed);

    if !config_mode {
        tracing::error!(
            "🚨 SECURITY: Database config change REJECTED - not in CONFIG_MODE. \n\
             This is a critical security protection. Database configuration can only be \n\
             modified during initial setup with CONFIG_MODE=true."
        );
        return Err(status_json_to_http((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "Operation not allowed",
                "code": "config_mode_required",
                "hint": "Restart with CONFIG_MODE=true if you need to reconfigure the database"
            })),
        )));
    }

    tracing::info!("Saving database configuration (CONFIG_MODE verified)");

    // Construct a properly escaped URL so credentials containing characters
    // such as `@`, `:` or `/` do not corrupt the connection string.
    let database_url = build_database_url(&config).map_err(|message| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Invalid database configuration",
                "message": message
            })),
        )
    })?;

    tracing::info!("Database URL constructed (password masked)");

    if let Err(message) =
        crate::api::setup_bootstrap::validate_env_value("DATABASE_URL", &database_url)
    {
        return Err(status_json_to_http((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Invalid database configuration",
                "message": message
            })),
        )));
    }

    let env_path = get_env_path();

    // If .env doesn't exist, create it from .env.example
    if !env_path.exists() {
        let env_example_path = get_env_example_path();
        if env_example_path.exists() {
            tracing::info!(".env not found, creating from .env.example");
            if let Err(e) = fs::copy(&env_example_path, &env_path) {
                tracing::error!("Failed to create .env from template: {:?}", e);
                return Err(status_json_to_http((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({
                        "error": "Failed to create configuration file",
                        "code": "config_file_permission"
                    })),
                )));
            }
        } else {
            // Create a minimal .env file with just the database URL
            tracing::info!(".env.example not found, creating minimal .env");
            let seed = match crate::api::config::update_env_var("", "DATABASE_URL", &database_url) {
                Ok(content) => content,
                Err(message) => {
                    return Err(status_json_to_http((
                        StatusCode::BAD_REQUEST,
                        Json(json!({
                            "error": "Invalid database configuration",
                            "message": message
                        })),
                    )));
                }
            };
            if let Err(e) = fs::write(&env_path, seed) {
                tracing::error!("Failed to create .env file: {:?}", e);
                return Err(status_json_to_http((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({
                        "error": "Failed to create configuration file",
                        "code": "config_file_permission"
                    })),
                )));
            }

            schedule_setup_restart();

            return Ok(Json(json!({
                "success": true,
                "message": "Database configuration saved successfully. Backend will restart to apply the full route table.",
                "path": env_path.display().to_string(),
                "database_url_set": true,
                "restart_triggered": true,
                "reload_triggered": false,
                "note": "The service must restart because routes are built at process startup."
            })));
        }
    }

    // Read and update existing .env file
    let content = match fs::read_to_string(&env_path) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("Failed to read .env file: {:?}", e);
            return Err(status_json_to_http((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "Failed to read configuration file",
                    "code": "config_file_permission"
                })),
            )));
        }
    };

    // Update DATABASE_URL through the shared writer (CR/LF/NUL fail closed).
    let updated_content =
        crate::api::config::update_env_var(&content, "DATABASE_URL", &database_url).map_err(
            |message| {
                status_json_to_http((
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": "Invalid database configuration",
                        "message": message
                    })),
                ))
            },
        )?;

    // Write back to file
    match fs::write(&env_path, updated_content) {
        Ok(_) => {
            tracing::info!("✅ Database configuration saved successfully");

            schedule_setup_restart();

            Ok(Json(json!({
                "success": true,
                "message": "Database configuration saved successfully. Backend will restart to apply the full route table.",
                "path": env_path.display().to_string(),
                "database_url_set": true,
                "restart_triggered": true,
                "reload_triggered": false,
                "note": "The service must restart because routes are built at process startup."
            })))
        }
        Err(e) => {
            tracing::error!("Failed to write .env file: {:?}", e);
            Err(status_json_to_http((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "Failed to save configuration",
                    "code": "config_file_permission"
                })),
            )))
        }
    }
}

/// Exit so the supervisor cold-starts with the full route table + workers.
/// Used after setup DB save and when CONFIG_MODE reload obtains a DB while
/// still serving the setup-only router.
pub(crate) fn schedule_setup_restart() {
    tracing::info!("🔁 Exiting shortly so the supervisor can restart with the full route table");

    tokio::spawn(async {
        tokio::time::sleep(std::time::Duration::from_millis(750)).await;
        tracing::info!("🔁 Exiting for full-mode restart");
        std::process::exit(0);
    });
}

// Helper functions

/// Get the path to .env.example
fn get_env_example_path() -> PathBuf {
    let mut path = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    path.push(".env.example");
    path
}

/// Get the path to .env
fn get_env_path() -> PathBuf {
    let mut path = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    path.push(".env");
    path
}

#[cfg(test)]
mod tests {
    use super::{DatabaseConfigRequest, build_database_url};

    #[test]
    fn database_url_escapes_credentials_and_database_name() {
        let config = DatabaseConfigRequest {
            host: "db.example.com".to_string(),
            port: 5432,
            username: "setup-user".to_string(),
            password: "p@ss:word".to_string(),
            database: "myriad/main".to_string(),
            setup_secret: None,
        };

        let url = build_database_url(&config).expect("database URL should be valid");

        assert_eq!(
            url,
            "postgres://setup-user:p%40ss%3Aword@db.example.com:5432/myriad%2Fmain"
        );
    }

    #[test]
    fn database_url_rejects_empty_required_fields() {
        let config = DatabaseConfigRequest {
            host: " ".to_string(),
            port: 5432,
            username: "postgres".to_string(),
            password: "password".to_string(),
            database: "myriad".to_string(),
            setup_secret: None,
        };

        assert!(build_database_url(&config).is_err());
    }

    #[test]
    fn database_config_request_accepts_setup_secret() {
        let parsed: DatabaseConfigRequest = serde_json::from_value(serde_json::json!({
            "host": "db.example.com",
            "port": 5432,
            "username": "postgres",
            "password": "password",
            "database": "myriad",
            "setup_secret": "phrase-from-env"
        }))
        .expect("request should deserialize");
        assert_eq!(parsed.setup_secret.as_deref(), Some("phrase-from-env"));
        assert!(build_database_url(&parsed).is_ok());
    }

    #[test]
    fn setup_progress_requires_database_and_admin_not_ai_keys() {
        let missing_admin = super::setup_progress_from_flags(true, false);
        assert!(missing_admin.is_setup_required);
        assert_eq!(
            missing_admin.missing_configs,
            vec!["No admin user registered".to_string()]
        );

        let ready = super::setup_progress_from_flags(true, true);
        assert!(!ready.is_setup_required);
        assert!(ready.missing_configs.is_empty());
    }

    /// The init gate and the post-migration check both stop on an unreadable
    /// claim; neither may degrade it to "no admin yet".
    #[test]
    fn init_database_claim_checks_fail_closed() {
        let source = include_str!("setup.rs");
        let init = source
            .split("pub async fn init_database(")
            .nth(1)
            .and_then(|rest| rest.split("/// Database configuration request").next())
            .unwrap();
        let gates: Vec<&str> = init
            .split("principal::installation_claimed(&db)")
            .skip(1)
            .collect();
        assert_eq!(gates.len(), 2);
        for gate in gates {
            let head = gate.trim_start();
            assert!(head.starts_with(".await") && head[6..].trim_start().starts_with(".map_err("));
        }
    }

    #[tokio::test]
    async fn setup_progress_surfaces_unreadable_claims() {
        use sea_orm::{ConnectOptions, ConnectionTrait, Database, DatabaseBackend, Statement};
        let Ok(url) = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL") else {
            return;
        };
        let mut options = ConnectOptions::new(url);
        options.max_connections(1).min_connections(1);
        let db = Database::connect(options).await.unwrap();
        let exec = |sql: &'static str| {
            let db = db.clone();
            async move {
                db.execute_raw(Statement::from_string(DatabaseBackend::Postgres, sql))
                    .await
                    .unwrap();
            }
        };
        exec("CREATE TEMP TABLE platforms (id INT)").await;
        exec("CREATE TEMP TABLE users (id INT, is_admin TEXT, is_owner BOOLEAN)").await;
        exec("INSERT INTO users VALUES (1, 'yes', false)").await;
        assert!(super::inspect_setup_progress(&db).await.is_err());

        exec("ALTER TABLE users ALTER COLUMN is_admin TYPE BOOLEAN USING false").await;
        let progress = super::inspect_setup_progress(&db).await.unwrap();
        assert!(progress.has_database && !progress.has_admin_user);
        exec("UPDATE users SET is_admin = true").await;
        assert!(
            super::inspect_setup_progress(&db)
                .await
                .unwrap()
                .has_admin_user
        );
        db.close().await.unwrap();
    }
}
