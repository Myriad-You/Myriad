use axum::{
    http::{HeaderMap, StatusCode},
    Json,
};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use sea_orm_migration::MigratorTrait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::env;
use std::fs;
use std::path::PathBuf;
use url::Url;
use crate::error::{status_json_to_http, HttpError};

/// Setup status response
#[derive(Debug, Serialize, Deserialize)]
pub struct SetupStatus {
    pub is_setup_required: bool,
    pub has_database: bool,
    pub has_admin_user: bool,
    pub missing_configs: Vec<String>,
}

/// GET /api/setup/status
/// Check if initial setup is required
pub async fn check_setup_status(
    crate::extract::Db(db): crate::extract::Db,
) -> Result<Json<SetupStatus>, HttpError> {
    tracing::info!("Checking setup status");

    // Check if database tables exist
    let has_database = check_database_tables(&db).await;

    // Check if admin user exists (first user)
    let has_admin_user = if has_database {
        check_admin_user_exists(&db).await
    } else {
        false
    };

    // Collect missing configurations
    let mut missing_configs = Vec::new();

    if !has_database {
        missing_configs.push("Database tables not initialized".to_string());
    }
    if !has_admin_user {
        missing_configs.push("No admin user registered".to_string());
    }

    // Setup is only required if database or admin user is missing
    let is_setup_required = !has_database || !has_admin_user;

    let status = SetupStatus {
        is_setup_required,
        has_database,
        has_admin_user,
        missing_configs,
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
        "database_url_set": env::var("DATABASE_URL").is_ok(),
        "server_host": env::var("SERVER_HOST").unwrap_or_else(|_| "127.0.0.1".to_string()),
        "server_port": env::var("SERVER_PORT").unwrap_or_else(|_| "1103".to_string()),
        "github_oauth": {
            "client_id_set": config_guard.github_client_id.is_some(),
            "client_secret_set": config_guard.github_client_secret.is_some(),
            "redirect_url": config_guard.github_redirect_url.clone(),
        },
        "gemini_api": {
            "api_key_set": config_guard.gemini_api_key.is_some(),
            "model": config_guard.gemini_model.clone(),
        },
        // PR #4: 公开 registration 开关 + OAuth providers 数量给前端
        "allow_local_registration": config_guard.allow_local_registration,
        "oauth_providers_count": config_guard.oauth_providers.iter().filter(|p| p.enabled).count(),
    });

    Ok(Json(config))
}

// Helper functions

/// Check if required database tables exist
async fn check_database_tables(db: &DatabaseConnection) -> bool {
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

    match result {
        Ok(Some(row)) => {
            let platforms_exists: bool = row.try_get("", "platforms_exists").unwrap_or(false);
            let users_exists: bool = row.try_get("", "users_exists").unwrap_or(false);

            // Both tables should exist for complete setup
            let exists = platforms_exists && users_exists;
            tracing::info!(
                "Database tables check - platforms: {}, users: {}, complete: {}",
                platforms_exists,
                users_exists,
                exists
            );
            exists
        }
        Ok(None) => {
            tracing::warn!("No rows returned when checking database tables");
            false
        }
        Err(e) => {
            tracing::error!("Error checking database tables: {:?}", e);
            false
        }
    }
}

/// Check if any current administrator exists, regardless of login provider.
async fn check_admin_user_exists(db: &DatabaseConnection) -> bool {
    let result = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT EXISTS (SELECT 1 FROM users WHERE is_admin = true LIMIT 1) as admin_exists",
            vec![],
        ))
        .await;

    match result {
        Ok(Some(row)) => {
            let exists: bool = row.try_get("", "admin_exists").unwrap_or(false);
            tracing::info!("Admin user exists: {}", exists);
            exists
        }
        Ok(None) => {
            tracing::warn!("No rows returned when checking admin user");
            false
        }
        Err(e) => {
            tracing::error!("Error checking admin user: {:?}", e);
            false
        }
    }
}

/// POST /api/setup/init-database
/// Run non-destructive database migrations during initial configuration.
pub async fn init_database(
    crate::extract::Db(db): crate::extract::Db,
) -> Result<Json<Value>, HttpError> {
    // Setup switches out of CONFIG_MODE as soon as the database can be reached.
    // Keep the recovery migration available until the installation is claimed,
    // then lock it permanently once an administrator exists.
    let admin_exists = check_admin_user_exists(&db).await;

    if admin_exists {
        tracing::error!(
            "🚨 Database initialization REJECTED: Admin user already exists (setup completed)"
        );
        return Err(status_json_to_http((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "Setup already completed",
                "message": "Database has been initialized and an admin user exists. Use the authenticated administration workflow for maintenance."
            })),
        )));
    }

    tracing::info!("Running database migrations");
    let tables_existed = check_database_tables(&db).await;

    // Never drop tables or rewrite migration history from an unauthenticated
    // setup endpoint. Migrator::up is idempotent and applies only pending work;
    // damaged migration state requires explicit operator intervention.
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
                        "message": format!(
                            "Migrations applied but schema check/heals failed: {}",
                            e
                        )
                    })),
                )));
            }
            tracing::info!("✅ Schema check/heals completed after setup migrations");

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
                total_tables, users_exists, platforms_exists, configurations_exists
            );

            let message = if tables_existed {
                "数据库迁移已检查并更新"
            } else {
                "数据库初始化完成"
            };

            Ok(Json(json!({
                "success": true,
                "message": message,
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
                    "message": "数据库迁移失败，请检查数据库连接和权限设置"
                })),
            )))
        }
    }
}

/// POST /api/setup/init-env
/// Initialize .env file from .env.example
///
/// 保护：CONFIG_MODE + 引导令牌（实例此前已配置过时）。
pub async fn initialize_env_file(
    headers: HeaderMap,
) -> Result<Json<Value>, HttpError> {
    crate::api::setup_bootstrap::require_bootstrap(&headers)
        .map_err(HttpError)?;

    // Only allow in CONFIG_MODE
    let config_mode = crate::CONFIG_MODE.load(std::sync::atomic::Ordering::Relaxed);

    if !config_mode {
        tracing::error!(
            "🚨 Environment file initialization REJECTED: Not in CONFIG_MODE (security protection)"
        );
        return Err(status_json_to_http((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "Operation not allowed",
                "message": "Environment file initialization is only allowed in CONFIG_MODE. Please restart the application with CONFIG_MODE=true."
            })),
        )));
    }

    tracing::info!("Initializing .env file from .env.example");

    let env_example_path = get_env_example_path();
    let env_path = get_env_path();

    // Check if .env already exists
    if env_path.exists() {
        return Err(status_json_to_http((
            StatusCode::CONFLICT,
            Json(json!({
                "error": ".env file already exists",
                "message": "Please use the update endpoint to modify existing configuration"
            })),
        )));
    }

    // Check if .env.example exists
    if !env_example_path.exists() {
        return Err(status_json_to_http((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": ".env.example not found",
                "message": "Template file is missing"
            })),
        )));
    }

    // Copy .env.example to .env
    match fs::copy(&env_example_path, &env_path) {
        Ok(_) => {
            tracing::info!(".env file created successfully");
            Ok(Json(json!({
                "success": true,
                "message": ".env file initialized from template"
            })))
        }
        Err(e) => {
            tracing::error!("Failed to create .env file: {:?}", e);
            Err(status_json_to_http((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "Failed to create .env file",
                    "message": "无法创建配置文件，请检查文件系统权限"
                })),
            )))
        }
    }
}

/// Environment configuration update request
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct EnvUpdateRequest {
    // Core configuration (saved to .env)
    pub database_url: Option<String>,
    pub server_host: Option<String>,
    pub server_port: Option<String>,
    pub rust_log: Option<String>,
    pub jwt_secret: Option<String>,
    pub cors_origins: Option<String>,

    // Application configuration (saved to database) - kept for backward compatibility
    // These will be migrated to the database automatically
    pub gemini_api_key: Option<String>,
    pub gemini_model: Option<String>,
    pub openai_api_key: Option<String>,
    pub openai_model: Option<String>,
    pub openai_base_url: Option<String>,
    pub ai_provider: Option<String>,
    pub topic_style: Option<String>,
    pub github_username: Option<String>,
    pub github_token: Option<String>,
    pub bilibili_uid: Option<String>,
    pub steam_api_key: Option<String>,
    pub steam_id: Option<String>,
    pub netease_user_id: Option<String>,
    pub github_client_id: Option<String>,
    pub github_client_secret: Option<String>,
    pub github_redirect_url: Option<String>,
}

/// POST /api/setup/update-env
/// Update .env file with new configuration
///
/// 保护：CONFIG_MODE + 引导令牌（实例此前已配置过时）+ 值语法校验。
pub async fn update_env_file(
    headers: HeaderMap,
    Json(config): Json<EnvUpdateRequest>,
) -> Result<Json<Value>, HttpError> {
    crate::api::setup_bootstrap::require_bootstrap(&headers)
        .map_err(HttpError)?;

    // Only allow in CONFIG_MODE
    let config_mode = crate::CONFIG_MODE.load(std::sync::atomic::Ordering::Relaxed);

    if !config_mode {
        tracing::error!(
            "🚨 Environment file update REJECTED: Not in CONFIG_MODE (security protection)"
        );
        return Err(status_json_to_http((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "Operation not allowed",
                "message": "Environment file updates are only allowed in CONFIG_MODE. Please restart the application with CONFIG_MODE=true to modify configuration."
            })),
        )));
    }

    tracing::info!("Updating .env file configuration");

    let env_path = get_env_path();

    // Check if .env exists
    if !env_path.exists() {
        return Err(status_json_to_http((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": ".env file not found",
                "message": "Please initialize the configuration first"
            })),
        )));
    }

    // Read current .env file
    let content = match fs::read_to_string(&env_path) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("Failed to read .env file: {:?}", e);
            return Err(status_json_to_http((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "Failed to read .env file",
                    "message": "无法读取配置文件，请检查文件是否存在及权限设置"
                })),
            )));
        }
    };

    // Update configuration values
    let mut updated_content = content;

    // Helper macro to update env values (only for core config).
    //
    // 每个值都先过语法校验：`.env` 是逐行 `KEY=VALUE`，值里的 CR/LF 会凭空
    // 生成新的一行，等于注入任意环境变量（例如追加一个 `JWT_SECRET=` 或
    // 攻击者可控的 `RUST_LOG`）。
    macro_rules! update_env_var {
        ($field:expr, $key:expr) => {
            if let Some(value) = $field {
                crate::api::setup_bootstrap::validate_env_value($key, &value).map_err(|msg| {
                    tracing::warn!("🚨 Rejected .env update: {}", msg);
                    (
                        StatusCode::BAD_REQUEST,
                        Json(json!({
                            "error": "Invalid configuration value",
                            "message": msg,
                        })),
                    )
                })?;
                updated_content = update_env_variable(&updated_content, $key, &value);
            }
        };
    }

    // Only update core configuration in .env
    update_env_var!(config.database_url, "DATABASE_URL");
    update_env_var!(config.server_host, "SERVER_HOST");
    update_env_var!(config.server_port, "SERVER_PORT");
    update_env_var!(config.rust_log, "RUST_LOG");
    update_env_var!(config.jwt_secret, "JWT_SECRET");
    update_env_var!(config.cors_origins, "CORS_ORIGINS");

    // Write updated content back to .env
    match fs::write(&env_path, updated_content) {
        Ok(_) => {
            tracing::info!(".env file updated successfully");

            // If database is available, save application configs there
            if let Ok(db) = crate::services::tapp_registry::database().await {
                use crate::services::config_service::ConfigService;
                use serde_json::json;
                use std::collections::HashMap;

                let config_service = ConfigService::new(db);
                let mut db_updates = HashMap::new();

                // Map application configs to database
                if let Some(v) = config.ai_provider {
                    db_updates.insert("ai_provider".to_string(), json!(v));
                }
                if let Some(v) = config.gemini_api_key {
                    db_updates.insert("gemini_api_key".to_string(), json!(v));
                }
                if let Some(v) = config.gemini_model {
                    db_updates.insert("gemini_model".to_string(), json!(v));
                }
                if let Some(v) = config.openai_api_key {
                    db_updates.insert("openai_api_key".to_string(), json!(v));
                }
                if let Some(v) = config.openai_model {
                    db_updates.insert("openai_model".to_string(), json!(v));
                }
                if let Some(v) = config.openai_base_url {
                    db_updates.insert("openai_base_url".to_string(), json!(v));
                }
                if let Some(v) = config.topic_style {
                    db_updates.insert("topic_style".to_string(), json!(v));
                }
                if let Some(v) = config.github_username {
                    db_updates.insert("github_username".to_string(), json!(v));
                }
                if let Some(v) = config.github_token {
                    db_updates.insert("github_token".to_string(), json!(v));
                }
                if let Some(v) = config.bilibili_uid {
                    db_updates.insert("bilibili_uid".to_string(), json!(v));
                }
                if let Some(v) = config.steam_api_key {
                    db_updates.insert("steam_api_key".to_string(), json!(v));
                }
                if let Some(v) = config.steam_id {
                    db_updates.insert("steam_id".to_string(), json!(v));
                }
                if let Some(v) = config.netease_user_id {
                    db_updates.insert("netease_user_id".to_string(), json!(v));
                }
                if let Some(v) = config.github_client_id {
                    db_updates.insert("github_client_id".to_string(), json!(v));
                }
                if let Some(v) = config.github_client_secret {
                    db_updates.insert("github_client_secret".to_string(), json!(v));
                }
                if let Some(v) = config.github_redirect_url {
                    db_updates.insert("github_redirect_url".to_string(), json!(v));
                }

                if !db_updates.is_empty() {
                    if let Err(e) = config_service.update_configs(db_updates).await {
                        tracing::warn!("Failed to update database configs: {}", e);
                    } else {
                        // Reload dynamic config (CONFIG_MODE only — no AppState)
                        if let Ok(new_config) = config_service.load_config().await {
                            // bootstrap-global: no AppState on config-mode setup router
                            crate::state::replace_shared_dynamic_config(new_config).await;
                            crate::services::oauth::registry::REGISTRY.reload().await;
                            tracing::info!("Dynamic configuration reloaded");
                        }
                    }
                }
            } else {
                // Check if we have any app config to save
                let has_app_config = config.ai_provider.is_some()
                    || config.gemini_api_key.is_some()
                    || config.gemini_model.is_some()
                    || config.openai_api_key.is_some()
                    || config.openai_model.is_some()
                    || config.openai_base_url.is_some()
                    || config.topic_style.is_some()
                    || config.github_username.is_some()
                    || config.github_token.is_some()
                    || config.bilibili_uid.is_some()
                    || config.steam_api_key.is_some()
                    || config.steam_id.is_some()
                    || config.netease_user_id.is_some()
                    || config.github_client_id.is_some()
                    || config.github_client_secret.is_some()
                    || config.github_redirect_url.is_some();

                if has_app_config {
                    tracing::warn!("⚠️ Application configuration received but Database is not connected. Settings will NOT be saved.");
                    return Ok(Json(json!({
                        "success": true,
                        "message": "Core configuration updated, but application settings could not be saved because the database is not connected.",
                        "warning": "Application settings (AI keys, etc.) were NOT saved. Please ensure the database is connected and try again.",
                        "path": env_path.display().to_string()
                    })));
                }
            }

            Ok(Json(json!({
                "success": true,
                "message": "Configuration updated successfully.",
                "path": env_path.display().to_string()
            })))
        }
        Err(e) => {
            tracing::error!("Failed to write .env file: {:?}", e);
            Err(status_json_to_http((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "Failed to write .env file",
                    "message": "无法保存配置文件，请检查文件系统权限"
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
    // 已配置过的实例还必须提供 .bootstrap-token / MYRIAD_BOOTSTRAP_TOKEN。
    crate::api::setup_bootstrap::require_bootstrap(&headers)
        .map_err(HttpError)?;

    // P0 安全修复：强制要求 CONFIG_MODE
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
                "message": "数据库配置只能在配置模式下修改。请使用 CONFIG_MODE=true 重启服务。",
                "reason": "Security protection: Database configuration is locked after initial setup",
                "hint": "Restart with CONFIG_MODE=true environment variable if you need to reconfigure the database"
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
                        "message": "无法创建配置文件，请检查文件系统权限"
                    })),
                )));
            }
        } else {
            // Create a minimal .env file with just the database URL
            tracing::info!(".env.example not found, creating minimal .env");
            if let Err(e) = fs::write(&env_path, format!("DATABASE_URL={}\n", database_url)) {
                tracing::error!("Failed to create .env file: {:?}", e);
                return Err(status_json_to_http((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({
                        "error": "Failed to create configuration file",
                        "message": "无法创建配置文件，请检查文件系统权限"
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
                    "message": "无法读取配置文件，请检查文件系统权限"
                })),
            )));
        }
    };

    // Update DATABASE_URL
    let updated_content = update_env_variable(&content, "DATABASE_URL", &database_url);

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
                    "message": "无法保存配置文件，请检查文件系统权限"
                })),
            )))
        }
    }
}

/// Exit so the supervisor cold-starts with the full route table + workers.
/// Used after setup DB save and when CONFIG_MODE reload obtains a DB while
/// still serving the setup-only router.
pub(crate) fn schedule_setup_restart() {
    tracing::info!(
        "🔁 Exiting shortly so the supervisor can restart with the full route table"
    );

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

/// Update a single environment variable in the content.
///
/// 调用方必须先用 [`crate::api::setup_bootstrap::validate_env_value`] 校验。这里
/// 再兜一层：把值里的 CR/LF 换成空格，保证无论调用路径如何，一个 key 永远只
/// 产出一行，不会把 `.env` 撕成多条记录。
fn update_env_variable(content: &str, key: &str, value: &str) -> String {
    let value: String = value
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect();
    let value = value.as_str();

    let lines: Vec<&str> = content.lines().collect();
    let mut result = Vec::new();
    let mut found = false;

    for line in lines {
        let trimmed = line.trim();

        // Skip comments and empty lines
        if trimmed.starts_with('#') || trimmed.is_empty() {
            result.push(line.to_string());
            continue;
        }

        // Check if this line contains our key
        if let Some(eq_pos) = trimmed.find('=') {
            let line_key = trimmed[..eq_pos].trim();
            if line_key == key {
                // Replace the value
                result.push(format!("{}={}", key, value));
                found = true;
                continue;
            }
        }

        result.push(line.to_string());
    }

    // If key wasn't found, append it
    if !found {
        result.push(format!("{}={}", key, value));
    }

    result.join("\n")
}

#[cfg(test)]
mod tests {
    use super::{build_database_url, DatabaseConfigRequest};

    #[test]
    fn database_url_escapes_credentials_and_database_name() {
        let config = DatabaseConfigRequest {
            host: "db.example.com".to_string(),
            port: 5432,
            username: "setup-user".to_string(),
            password: "p@ss:word".to_string(),
            database: "myriad/main".to_string(),
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
        };

        assert!(build_database_url(&config).is_err());
    }
}
