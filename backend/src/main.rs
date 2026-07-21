use axum::{
    extract::{FromRequest, Request},
    http::StatusCode,
    middleware::{from_fn, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
    Json, Router,
};
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod api;
mod config;
mod db;
mod federation;
mod middleware;
mod models;
mod oauth_url_builder;
mod services;
mod util;

use config::{AppConfig, DynamicConfig};
use sea_orm::ConnectionTrait;
use services::config_service::ConfigService;
use std::sync::atomic::{AtomicBool, Ordering}; // P1: 用于数据库健康检查

// Global flag to indicate if server is running in configuration mode
pub static CONFIG_MODE: AtomicBool = AtomicBool::new(false);

/// Fail-soft: production images run as uid 1000 (`myriad`); root is a hygiene warning only.
fn warn_if_running_as_root() {
    #[cfg(unix)]
    {
        // Avoid a libc crate dep: libc geteuid is ubiquitous on Unix.
        extern "C" {
            fn geteuid() -> u32;
        }
        // SAFETY: geteuid is a pure syscall with no arguments.
        let uid = unsafe { geteuid() };
        if uid == 0 {
            tracing::warn!(
                "backend is running as root (uid 0); production compose should use non-root USER myriad (de-root)"
            );
        }
    }
}

// Global database connection (None in config mode, Some in full mode)
pub static DB_CONNECTION: once_cell::sync::Lazy<Arc<RwLock<Option<sea_orm::DatabaseConnection>>>> =
    once_cell::sync::Lazy::new(|| Arc::new(RwLock::new(None)));

// Global core configuration (hot-reloadable)
pub static GLOBAL_CONFIG: once_cell::sync::Lazy<Arc<RwLock<AppConfig>>> =
    once_cell::sync::Lazy::new(|| Arc::new(RwLock::new(AppConfig::default())));

// Global dynamic configuration from database (hot-reloadable)
pub static GLOBAL_DYNAMIC_CONFIG: once_cell::sync::Lazy<Arc<RwLock<DynamicConfig>>> =
    once_cell::sync::Lazy::new(|| Arc::new(RwLock::new(DynamicConfig::default())));

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load .env before any component reads environment variables.
    dotenvy::dotenv().ok();

    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "myriad_backend=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!(
        version = api::build_version(),
        commit_sha = ?api::build_commit_sha(),
        "🚀 Starting Myriad Backend"
    );

    // Production compose de-roots backend (USER myriad). Warn once if still root.
    warn_if_running_as_root();

    // Record process start time for /health.uptime_seconds.
    api::mark_startup();

    // Initialise the updater proxy client. None if env not set; routes still register
    // and return a clean 503.
    let updater_client = services::updater_client::UpdaterClient::from_env();
    if let Some(c) = &updater_client {
        tracing::info!(
            base_url = %c.base_url(),
            can_mutate = c.can_mutate(),
            "updater client configured"
        );
        // Best-effort reachability probe. Don't block startup — the updater container may
        // still be coming up, and admin routes return 503 cleanly when unreachable.
        let probe = c.clone();
        tokio::spawn(async move {
            match probe.ping().await {
                Ok(_) => tracing::info!("updater /healthz: reachable"),
                Err(e) => {
                    tracing::warn!(err = %e, "updater /healthz: unreachable on startup (will retry on demand)")
                }
            }
        });
    } else {
        tracing::info!(
            "no updater client configured (set MYRIAD_UPDATER_URL + UPDATER_GATEWAY_SECRET to enable)"
        );
    }
    api::updater_admin::init(updater_client);

    run_server().await?;

    tracing::info!("👋 Backend shutdown complete");
    Ok(())
}

async fn run_server() -> anyhow::Result<()> {
    // Load configuration
    let config = AppConfig::from_env()?;

    services::data_paths::verify_runtime_storage_writable().map_err(|error| {
        anyhow::anyhow!(
            "backend storage preflight failed; repair /app/data and /app/cache ownership/permissions for uid 1000: {error}"
        )
    })?;
    tracing::info!(
        data_dir = %services::data_paths::paths().root.display(),
        cache_dir = %services::data_paths::paths().cache.display(),
        "backend storage write preflight passed"
    );

    // Initialize global config
    *GLOBAL_CONFIG.write().await = config.clone();
    tracing::info!("✅ Configuration loaded and cached globally");

    // ✅ 安全修复: 验证 JWT 密钥强度
    match std::env::var("JWT_SECRET") {
        Ok(secret) => {
            if secret.len() < 32 {
                tracing::error!(
                    "🚨 JWT_SECRET is too weak ({} chars). Minimum 32 characters required for security.",
                    secret.len()
                );
                if std::env::var("ENVIRONMENT").unwrap_or_default() == "production" {
                    anyhow::bail!(
                        "JWT_SECRET must be at least 32 characters in production environment"
                    );
                } else {
                    tracing::warn!("⚠️  Continuing with weak JWT_SECRET in development mode. DO NOT use in production!");
                }
            } else {
                tracing::info!("✅ JWT_SECRET strength validated ({} chars)", secret.len());
            }
        }
        Err(_) => {
            tracing::warn!("⚠️  JWT_SECRET not configured. Authentication features will not work.");
        }
    }

    // Try to initialize database connection if URL is configured
    if !config.database_url.is_empty() {
        match db::connection::establish_connection(&config.database_url).await {
            Ok(db) => {
                tracing::info!("✅ Database connection established");

                // Retired migration files have been folded into the base schema.
                // Remove only their known history rows before SeaORM validates
                // migration-file/history parity; schema_check owns the backfill.
                if let Err(e) = db::schema_check::reconcile_retired_migration_history(&db).await {
                    tracing::warn!("Failed to reconcile retired migration history: {}", e);
                }

                // Run database migrations automatically on startup (idempotent - skips already applied migrations)
                use sea_orm_migration::MigratorTrait;
                tracing::debug!("Checking for pending database migrations...");
                match migration::Migrator::up(&db, None).await {
                    Ok(_) => {
                        tracing::info!("✅ Database migrations up to date");
                    }
                    Err(e) => {
                        // Log the error but don't stop the service
                        // Migrations might fail if tables already exist from manual setup
                        tracing::warn!("⚠️  Database migration check failed: {}", e);
                        tracing::info!("Continuing with existing database schema...");
                    }
                }

                // Auto-complete missing schema fields (safe, idempotent operation)
                if let Err(e) = db::schema_check::ensure_schema(&db).await {
                    tracing::warn!("⚠️  Schema check failed: {}", e);
                    tracing::info!("Continuing with existing schema...");
                }

                match api::tapp_store::recover_tapp_filesystem_state(&db).await {
                    Ok(0) => {}
                    Ok(count) => {
                        tracing::warn!(count, "Recovered interrupted Tapp filesystem transactions")
                    }
                    Err(error) => tracing::error!(
                        %error,
                        "Failed to inspect Tapp filesystem transaction state"
                    ),
                }

                // Load dynamic configuration from database
                let config_service = ConfigService::new(db.clone());

                // Load the merged configuration
                match config_service.load_config().await {
                    Ok(dynamic_config) => {
                        *GLOBAL_DYNAMIC_CONFIG.write().await = dynamic_config;
                        tracing::info!("✅ Dynamic configuration loaded from database");
                    }
                    Err(e) => {
                        tracing::warn!("⚠️  Failed to load dynamic config: {}", e);
                        tracing::info!("Using default configuration");
                    }
                }

                // Validate GitHub OAuth configuration (after database config is loaded)
                // This is informational only - OAuth will work if configured in database
                use oauth_url_builder::OAuthUrlBuilder;
                if let Err(e) = OAuthUrlBuilder::validate_github_oauth_config().await {
                    tracing::debug!("ℹ️  GitHub OAuth status: {}", e);
                }

                // 🔐 Load OAuth provider registry (GitHub + future OIDC providers)
                services::oauth::registry::init().await;
                tracing::info!(
                    "✅ OAuth providers loaded: {}",
                    services::oauth::registry::REGISTRY.list().await.len()
                );

                // 通知中心必须先于任何后台调度器启动；interval 首次 tick 会立即执行，
                // 否则启动阶段的 Tapp/Brew/MCP 事件会静默丢失。
                services::agent::notifications::init_notifications(db.clone()).await;
                api::updater_admin::resume_pending_job_notifications().await;
                tracing::info!("✅ Agent notification system initialized");

                // Initialize Tapp scheduler engine
                api::tapp_scheduler::init_scheduler(db.clone()).await;
                tracing::info!("✅ Tapp scheduler engine initialized");

                // Reconcile Myriad Core platform refresh jobs after the shared
                // scheduler is ready. Failure does not block startup; the admin
                // settings save path will retry and report the error directly.
                match api::config::reconcile_platform_auto_refresh(&db).await {
                    Ok(summary) => tracing::info!(
                        enabled_tasks = summary.enabled_tasks,
                        disabled_tasks = summary.disabled_tasks,
                        interval_hours = summary.interval_hours,
                        "✅ Core platform auto-refresh tasks reconciled"
                    ),
                    Err(error) => tracing::warn!(
                        "Failed to reconcile Core platform auto-refresh tasks: {}",
                        error
                    ),
                }

                // Initialize Brew scheduler engine (RSS/Atom feed updates)
                services::brew_scheduler::init_brew_scheduler(db.clone()).await;
                tracing::info!("✅ Brew scheduler engine initialized");

                // Initialize Agent identity system (SOUL.md / USER.md)
                let agent_data_dir = std::path::PathBuf::from("data/agent");
                services::agent::identity::init_identity(agent_data_dir.clone()).await;
                tracing::info!("✅ Agent identity system initialized");

                // Initialize Agent skill system
                services::agent::skill::init_skills(agent_data_dir.join("skills")).await;
                tracing::info!("✅ Agent skill system initialized");

                // Initialize Agent skill evolution system
                services::agent::skill_evolution::init_skill_evolution(
                    agent_data_dir.join("skills"),
                )
                .await;
                tracing::info!("✅ Agent skill evolution system initialized");

                // Initialize Agent memory system
                services::agent::memory::init_memory(agent_data_dir.join("memory")).await;
                tracing::info!("✅ Agent memory system initialized");

                // Initialize MCP (Model Context Protocol) client
                services::agent::mcp::init_mcp(&agent_data_dir.join("mcp_servers.json")).await;
                tracing::info!("✅ MCP client initialized");

                // Initialize Agent task store (DB persistence + recovery)
                services::agent::init_task_store(db.clone()).await;
                tracing::info!("✅ Agent task store initialized");

                // Re-create run hubs + wait-loops for waiting_for_input tasks so
                // answer/subscribe work after process restart.
                api::agent::restore_waiting_runs_after_boot().await;
                tracing::info!("✅ Agent waiting-task run hubs restored");

                // Expire persisted Tapp Agent interactions and resume their
                // waiting Executor tasks. Every replica runs this; DB CAS
                // ensures a single terminal transition.
                api::tapp_runtime::spawn_agent_interaction_expiry_worker(db.clone());
                tracing::info!("✅ Tapp Agent interaction expiry worker started");

                // Initialize Agent heartbeat system
                services::agent::heartbeat::init_heartbeat(agent_data_dir.join("HEARTBEAT.md"))
                    .await;
                tracing::info!("✅ Agent heartbeat system initialized");

                // Spawn confirmation cleanup background worker
                tokio::spawn(async {
                    let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
                    loop {
                        interval.tick().await;
                        services::agent::cleanup_expired_confirmations().await;
                    }
                });
                tracing::info!("✅ Agent confirmation cleanup worker started");

                // Spawn heartbeat background worker
                {
                    let heartbeat_db = db.clone();
                    tokio::spawn(async move {
                        // Heartbeat 独立 Semaphore（上限 2，防止风暴）
                        let semaphore = Arc::new(tokio::sync::Semaphore::new(2));
                        let mut interval =
                            tokio::time::interval(std::time::Duration::from_secs(60));
                        // 系统休眠恢复后跳过积压的 tick，避免同一分钟内连续触发
                        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                        let mut tick_count: u64 = 0;

                        loop {
                            interval.tick().await;
                            tick_count = tick_count.wrapping_add(1);

                            // 每小时清理过期认领桶（保留 48h）
                            if tick_count.is_multiple_of(60) {
                                services::agent::heartbeat::HeartbeatManager::cleanup_old_claims(
                                    &heartbeat_db,
                                    48,
                                )
                                .await;
                            }

                            let hb = match services::agent::heartbeat::get_heartbeat() {
                                Some(hb) => hb,
                                None => continue,
                            };

                            let due_tasks = hb.check_due_tasks().await;
                            for task in due_tasks {
                                let task_db = heartbeat_db.clone();
                                let hb_ref = hb.clone();
                                let task_semaphore = semaphore.clone();
                                tokio::spawn(async move {
                                    // Due tasks have already been reserved by the scheduler. Queue
                                    // them behind the semaphore instead of dropping them when busy.
                                    let _permit = match task_semaphore.acquire_owned().await {
                                        Ok(permit) => permit,
                                        Err(error) => {
                                            tracing::error!(
                                                task_id = %task.id,
                                                error = %error,
                                                "[Heartbeat] Execution semaphore closed"
                                            );
                                            return;
                                        }
                                    };

                                    let minute_bucket =
                                        services::agent::heartbeat::HeartbeatManager::current_minute_bucket();
                                    // 多副本 CAS：未抢到则跳过（另一实例已执行或已完成）
                                    if !services::agent::heartbeat::HeartbeatManager::try_claim_execution(
                                        &task_db,
                                        &task.id,
                                        minute_bucket,
                                    )
                                    .await
                                    {
                                        return;
                                    }

                                    let _inflight =
                                        services::agent::heartbeat::HeartbeatInflightGuard::enter();

                                    tracing::info!(
                                        task_id = %task.id,
                                        "[Heartbeat] Executing due task: {}",
                                        task.name
                                    );

                                    let request = services::agent::UserRequest {
                                        raw_input: task.action.clone(),
                                        timestamp: chrono::Utc::now(),
                                        user_id: services::agent::SYSTEM_USER_ID,
                                        context: None,
                                    };

                                    let agent = services::agent::Agent::new(task_db.clone()).await;
                                    let task_name = task.name.clone();
                                    let timeout = std::time::Duration::from_secs(
                                        services::agent::heartbeat::HEARTBEAT_TASK_TIMEOUT_SECS,
                                    );

                                    // 捕获 TaskCreated 的 executor task_id，超时后协作取消
                                    let (progress_tx, mut progress_rx) =
                                        tokio::sync::mpsc::channel::<
                                            services::agent::types::AgentProgressEvent,
                                        >(64);
                                    let captured_exec_task =
                                        std::sync::Arc::new(tokio::sync::Mutex::new(None::<String>));
                                    let captured_for_fwd = captured_exec_task.clone();
                                    tokio::spawn(async move {
                                        while let Some(event) = progress_rx.recv().await {
                                            if let services::agent::types::AgentProgressEvent::TaskCreated {
                                                task_id,
                                                ..
                                            } = &event
                                            {
                                                *captured_for_fwd.lock().await = Some(task_id.clone());
                                            }
                                        }
                                    });

                                    let outcome = tokio::time::timeout(
                                        timeout,
                                        agent.process_with_progress(request, progress_tx),
                                    )
                                    .await;

                                    let mut claim_status = "done";
                                    match outcome {
                                        Ok(Ok(response)) => {
                                            let succeeded = response.is_successful_outcome();
                                            let response_summary = response
                                                .message
                                                .chars()
                                                .take(200)
                                                .collect::<String>();
                                            let result_summary = if succeeded {
                                                response_summary
                                            } else {
                                                claim_status = "failed";
                                                format!("ERROR: {}", response_summary)
                                            };
                                            hb_ref.record_result(&task.id, &result_summary).await;
                                            let full_body = response
                                                .message
                                                .chars()
                                                .take(4000)
                                                .collect::<String>();
                                            if let Some(nm) = services::agent::notifications::get_notification_manager() {
                                                nm.notify_heartbeat_result(&task_name, &full_body, succeeded).await;
                                            }
                                            if succeeded {
                                                tracing::info!(
                                                    task_id = %task.id,
                                                    "[Heartbeat] Task completed: {}",
                                                    result_summary
                                                );
                                            } else {
                                                tracing::warn!(
                                                    task_id = %task.id,
                                                    "[Heartbeat] Task returned a non-success outcome: {}",
                                                    result_summary
                                                );
                                            }
                                        }
                                        Ok(Err(e)) => {
                                            claim_status = "failed";
                                            let err_msg = format!("ERROR: {}", e);
                                            hb_ref.record_result(&task.id, &err_msg).await;
                                            if let Some(nm) = services::agent::notifications::get_notification_manager() {
                                                nm.notify_heartbeat_result(&task_name, &err_msg, false).await;
                                            }
                                            tracing::warn!(
                                                task_id = %task.id,
                                                error = %e,
                                                "[Heartbeat] Task failed"
                                            );
                                        }
                                        Err(_elapsed) => {
                                            claim_status = "failed";
                                            // 硬取消：协作式 is_cancelled，打断 executor 步骤环
                                            if let Some(exec_tid) =
                                                captured_exec_task.lock().await.clone()
                                            {
                                                services::agent::executor::request_cancel(
                                                    &exec_tid,
                                                    &format!(
                                                        "heartbeat timed out after {}s",
                                                        services::agent::heartbeat::HEARTBEAT_TASK_TIMEOUT_SECS
                                                    ),
                                                )
                                                .await;
                                            }
                                            let err_msg = format!(
                                                "ERROR: heartbeat task timed out after {}s",
                                                services::agent::heartbeat::HEARTBEAT_TASK_TIMEOUT_SECS
                                            );
                                            hb_ref.record_result(&task.id, &err_msg).await;
                                            if let Some(nm) = services::agent::notifications::get_notification_manager() {
                                                nm.notify_heartbeat_result(&task_name, &err_msg, false).await;
                                            }
                                            tracing::warn!(
                                                task_id = %task.id,
                                                timeout_secs = services::agent::heartbeat::HEARTBEAT_TASK_TIMEOUT_SECS,
                                                "[Heartbeat] Task timed out; cancel requested"
                                            );
                                        }
                                    }
                                    services::agent::heartbeat::HeartbeatManager::complete_claim(
                                        &task_db,
                                        &task.id,
                                        minute_bucket,
                                        claim_status,
                                    )
                                    .await;
                                });
                            }
                        }
                    });
                    tracing::info!("✅ Heartbeat background worker started");
                }

                // Spawn skill evolution pruning worker (daily)
                tokio::spawn(async move {
                    // 初始延迟 1 小时，避免启动时负担
                    tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
                    let mut interval = tokio::time::interval(std::time::Duration::from_secs(86400));
                    loop {
                        interval.tick().await;
                        // 清理过期 Skill
                        if let Some(evolution) =
                            services::agent::skill_evolution::get_skill_evolution()
                        {
                            let pruned = evolution.prune_skills().await;
                            if !pruned.is_empty() {
                                tracing::info!(
                                    "[SkillEvolution] Pruned {} low-quality skills: {:?}",
                                    pruned.len(),
                                    pruned
                                );
                            }
                        }
                        // 清理过期记忆日志（保留 30 天）
                        if let Some(mem) = services::agent::memory::get_memory() {
                            mem.cleanup_old_logs(30).await;
                        }
                    }
                });
                tracing::info!("✅ Skill evolution pruning worker started");

                // Initialize Federation delivery worker (MFP Activity delivery queue).
                // Required for createNote/publish fan-out: rows enqueued in
                // fan_out_to_followers are drained here every ~15s.
                federation::delivery::spawn_delivery_worker(db.clone());
                tracing::info!("✅ Federation delivery worker started");

                tracing::info!("🌐 Starting in FULL MODE - all features available");
                *DB_CONNECTION.write().await = Some(db);
                CONFIG_MODE.store(false, Ordering::Relaxed);
            }
            Err(e) => {
                tracing::warn!("⚠️  Database connection failed: {}", e);
                tracing::info!("🔧 Starting in CONFIGURATION MODE");
                tracing::info!("📝 Only setup/status/bootstrap auth endpoints are available");
                tracing::info!(
                    "💡 Configure database via POST /api/setup/database-config; the service will restart to load the full route table"
                );
                CONFIG_MODE.store(true, Ordering::Relaxed);
            }
        }
    } else {
        tracing::warn!("⚠️  No database URL configured");
        tracing::info!("🔧 Starting in CONFIGURATION MODE");
        tracing::info!("📝 Only setup/status/bootstrap auth endpoints are available");
        tracing::info!(
            "💡 Configure database via POST /api/setup/database-config; the service will restart to load the full route table"
        );
        CONFIG_MODE.store(true, Ordering::Relaxed);
    }

    // Start the unified server. If this process booted without a DB, setup writes
    // DATABASE_URL and exits so the supervisor can restart with the full route table.
    start_unified_server(config).await
}

/// Middleware to check if route is allowed in configuration mode
async fn config_mode_middleware(req: Request, next: Next) -> Response {
    let path = req.uri().path();

    // Whitelist of paths that are allowed in configuration mode
    let allowed_paths = [
        "/health",
        "/api/setup/config",
        "/api/setup/status",
        "/api/setup/init-env",
        "/api/setup/update-env",
        "/api/setup/database-config", // ✅ 允许配置数据库（有内部认证检查）
        "/api/setup/init-database",
        "/api/setup/create-admin",
        "/api/system/status",
        "/api/auth/login",           // Allow login endpoint
        "/api/auth/me",              // Allow user info endpoint (for login state check)
        "/api/auth/logout",          // Allow logout endpoint
        "/api/auth/change-password", // Allow change password endpoint
        "/api/auth/register",        // PR #4: 公开注册（自身有 allow_local_registration 检查）
        "/api/auth/oauth/providers", // PR #2: 公开列出 OAuth providers
    ];

    // If in config mode and path is not whitelisted, return 503
    if CONFIG_MODE.load(Ordering::Relaxed) && !allowed_paths.iter().any(|p| path.starts_with(p)) {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "Service in configuration mode",
                "message": "服务器正在配置模式，请先完成数据库配置和初始化",
                "configure_endpoint": "/api/setup/database-config",
                "hint": "After configuration, the service restarts to load the full route table"
            })),
        )
            .into_response();
    }

    next.run(req).await
}

/// Wrapper for check_setup_status that gets DB from global state
async fn check_setup_status_wrapper() -> Response {
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => match api::setup::check_setup_status(axum::extract::State(db.clone())).await {
            Ok(response) => response.into_response(),
            Err((status, json)) => (status, json).into_response(),
        },
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "Database not connected",
                "message": "Database connection not available. Please configure database first."
            })),
        )
            .into_response(),
    }
}

/// Wrapper for init_database that gets DB from global state
async fn init_database_wrapper() -> Response {
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => match api::setup::init_database(axum::extract::State(db.clone())).await {
            Ok(response) => response.into_response(),
            Err((status, json)) => (status, json).into_response(),
        },
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "Database not connected",
                "message": "Database connection not available. Please configure database first."
            })),
        )
            .into_response(),
    }
}

/// Wrapper for create_admin that gets DB from global state
async fn create_admin_wrapper(
    Json(payload): Json<api::auth_local::CreateAdminRequest>,
) -> Response {
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            match api::auth_local::create_admin(axum::extract::State(db.clone()), Json(payload))
                .await
            {
                Ok(response) => response.into_response(),
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "Database not connected",
                "message": "Database connection not available. Please configure database first."
            })),
        )
            .into_response(),
    }
}

/// Wrapper for register that gets DB from global state (PR #4)
async fn register_wrapper(Json(payload): Json<api::auth_local::RegisterRequest>) -> Response {
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            match api::auth_local::register(axum::extract::State(db.clone()), Json(payload)).await {
                Ok(response) => response.into_response(),
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "Database not connected",
                "message": "数据库未连接，请先完成初始配置"
            })),
        )
            .into_response(),
    }
}

/// Wrapper for set_password (PR #4)
async fn set_password_wrapper(
    headers: axum::http::HeaderMap,
    Json(payload): Json<api::auth_local::SetPasswordRequest>,
) -> Response {
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => match api::auth_local::set_password(
            axum::extract::State(db.clone()),
            headers,
            Json(payload),
        )
        .await
        {
            Ok(response) => response.into_response(),
            Err((status, json)) => (status, json).into_response(),
        },
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

/// PR #6: Wrapper for admin_create_user
async fn admin_create_user_wrapper(
    headers: axum::http::HeaderMap,
    Json(payload): Json<api::auth_local::AdminCreateUserRequest>,
) -> Response {
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => match api::auth_local::admin_create_user(
            axum::extract::State(db.clone()),
            headers,
            Json(payload),
        )
        .await
        {
            Ok(response) => response.into_response(),
            Err((status, json)) => (status, json).into_response(),
        },
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

/// 设置页用户管理：列表（api::admin_users 取代 PR #6 的旧版列表）
async fn admin_list_users_wrapper(headers: axum::http::HeaderMap) -> Response {
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            match api::admin_users::list_users(axum::extract::State(db.clone()), headers).await {
                Ok(response) => response.into_response(),
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

/// 设置页用户管理：单用户详情
async fn admin_get_user_wrapper(
    axum::extract::Path(user_id): axum::extract::Path<i32>,
    headers: axum::http::HeaderMap,
) -> Response {
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => match api::admin_users::get_user(
            axum::extract::State(db.clone()),
            axum::extract::Path(user_id),
            headers,
        )
        .await
        {
            Ok(response) => response.into_response(),
            Err((status, json)) => (status, json).into_response(),
        },
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

/// 设置页用户管理：更新用户
async fn admin_update_user_wrapper(
    axum::extract::Path(user_id): axum::extract::Path<i32>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<api::admin_users::UpdateUserRequest>,
) -> Response {
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => match api::admin_users::update_user(
            axum::extract::State(db.clone()),
            axum::extract::Path(user_id),
            headers,
            Json(payload),
        )
        .await
        {
            Ok(response) => response.into_response(),
            Err((status, json)) => (status, json).into_response(),
        },
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

/// 设置页用户管理：解绑用户的 OAuth identity
async fn admin_unlink_identity_wrapper(
    axum::extract::Path(path): axum::extract::Path<(i32, i32)>,
    headers: axum::http::HeaderMap,
) -> Response {
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => match api::admin_users::unlink_identity(
            axum::extract::State(db.clone()),
            axum::extract::Path(path),
            headers,
        )
        .await
        {
            Ok(response) => response.into_response(),
            Err((status, json)) => (status, json).into_response(),
        },
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

/// 设置页用户管理：删除用户
async fn admin_delete_user_wrapper(
    axum::extract::Path(user_id): axum::extract::Path<i32>,
    headers: axum::http::HeaderMap,
) -> Response {
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => match api::admin_users::delete_user(
            axum::extract::State(db.clone()),
            axum::extract::Path(user_id),
            headers,
        )
        .await
        {
            Ok(response) => response.into_response(),
            Err((status, json)) => (status, json).into_response(),
        },
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

/// Wrapper for toggle_local_login (PR #4)
async fn toggle_local_login_wrapper(
    headers: axum::http::HeaderMap,
    Json(payload): Json<api::auth_local::LocalLoginToggleRequest>,
) -> Response {
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => match api::auth_local::toggle_local_login(
            axum::extract::State(db.clone()),
            headers,
            Json(payload),
        )
        .await
        {
            Ok(response) => response.into_response(),
            Err((status, json)) => (status, json).into_response(),
        },
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

/// Wrapper for local_login that gets DB from global state
async fn local_login_wrapper(Json(payload): Json<api::auth_local::LocalLoginRequest>) -> Response {
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            match api::auth_local::local_login(axum::extract::State(db.clone()), Json(payload))
                .await
            {
                Ok(response) => response.into_response(),
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "Database not connected",
                "message": "数据库未连接，请先完成初始配置"
            })),
        )
            .into_response(),
    }
}

/// Wrapper for change_password that gets DB from global state
async fn change_password_wrapper(
    headers: axum::http::HeaderMap,
    Json(payload): Json<api::auth_local::ChangePasswordRequest>,
) -> Response {
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            match api::auth_local::change_password(
                axum::extract::State(db.clone()),
                headers,
                Json(payload),
            )
            .await
            {
                Ok(response) => response.into_response(),
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "Database not connected",
                "message": "数据库未连接，请先完成初始配置"
            })),
        )
            .into_response(),
    }
}

/// Wrapper for get_site_metadata that gets DB from global state
async fn get_site_metadata_wrapper() -> Response {
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let (status, json) =
                api::config::get_site_metadata(axum::extract::State(db.clone())).await;
            (status, json).into_response()
        }
        None => {
            // 返回默认元数据，不需要数据库连接
            (
                StatusCode::OK,
                Json(json!({
                    "site_title": "Myriad - A myriad of lights, in one place.",
                    "site_description": "A myriad of lights, in one place.",
                    "site_favicon": "/favicon.webp"
                })),
            )
                .into_response()
        }
    }
}

/// Wrapper for get_public_config that gets DB from global state
/// 🔓 公开端点 - 返回脱敏的平台配置（仅用于社交链接显示）
async fn get_public_config_wrapper() -> Response {
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let (status, json) =
                api::config::get_public_config(axum::extract::State(db.clone())).await;
            (status, json).into_response()
        }
        None => {
            // 没有数据库连接时返回空配置
            (
                StatusCode::OK,
                Json(json!({
                    "platforms": []
                })),
            )
                .into_response()
        }
    }
}

/// Wrapper for get_public_ui_config that gets DB from global state
/// 🔓 公开端点 - 返回公开的UI配置（萌宠、壁纸等）
async fn get_public_ui_config_wrapper() -> Response {
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let (status, json) =
                api::config::get_public_ui_config(axum::extract::State(db.clone())).await;
            (status, json).into_response()
        }
        None => {
            // 没有数据库连接时返回默认配置
            (
                StatusCode::OK,
                Json(json!({
                    "pet_enabled": true,
                    "pet_image_url": "",
                    "wallpaper_url": "",
                    "wallpaper_blur": 3
                })),
            )
                .into_response()
        }
    }
}

/// Wrapper for get_config that gets DB from global state
async fn get_config_wrapper() -> Response {
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let (status, json) = api::config::get_config(axum::extract::State(db.clone())).await;
            (status, json).into_response()
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "Database not connected",
                "message": "数据库未连接，配置功能暂不可用"
            })),
        )
            .into_response(),
    }
}

/// Wrapper for update_config that gets DB from global state
async fn update_config_wrapper(
    headers: axum::http::HeaderMap,
    Json(payload): Json<api::config::ConfigResponse>,
) -> Response {
    if let Err((status, json)) = middleware::auth::verify_current_admin_from_headers(&headers).await
    {
        return (status, json).into_response();
    }

    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let (status, json) =
                api::config::update_config(axum::extract::State(db.clone()), Json(payload)).await;
            (status, json).into_response()
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "Database not connected",
                "message": "数据库未连接，配置功能暂不可用"
            })),
        )
            .into_response(),
    }
}

/// Site public domain change: rewrite BASE_URL / FRONTEND_URL / CORS_ORIGINS (not federation Move).
async fn change_site_domain_wrapper(
    headers: axum::http::HeaderMap,
    Json(payload): Json<api::site_domain::ChangeSiteDomainRequest>,
) -> Response {
    if let Err((status, json)) = middleware::auth::verify_current_admin_from_headers(&headers).await
    {
        return (status, json).into_response();
    }

    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let (status, json) =
                api::site_domain::change_site_domain(axum::extract::State(db.clone()), Json(payload))
                    .await;
            (status, json).into_response()
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "Database not connected",
                "message": "数据库未连接，域名变更功能暂不可用"
            })),
        )
            .into_response(),
    }
}

/// Export every persisted setting plus the current administrator's user preferences.
async fn export_settings_wrapper(headers: axum::http::HeaderMap) -> Response {
    let claims = match middleware::auth::verify_current_admin_from_headers(&headers).await {
        Ok(claims) => claims,
        Err((status, json)) => return (status, json).into_response(),
    };
    let user_id = match claims.sub.parse::<i32>() {
        Ok(user_id) => user_id,
        Err(_) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Invalid authenticated user"})),
            )
                .into_response();
        }
    };

    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let (status, json) =
                api::config::export_settings(axum::extract::State(db.clone()), user_id).await;
            let mut response = (status, json).into_response();
            response.headers_mut().insert(
                axum::http::header::CACHE_CONTROL,
                axum::http::HeaderValue::from_static("no-store, private"),
            );
            response.headers_mut().insert(
                axum::http::header::PRAGMA,
                axum::http::HeaderValue::from_static("no-cache"),
            );
            response
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

/// Preview an adaptive settings restore without changing persisted state.
async fn preview_settings_restore_wrapper(
    headers: axum::http::HeaderMap,
    Json(payload): Json<api::config::SettingsBackup>,
) -> Response {
    if let Err((status, json)) = middleware::auth::verify_current_admin_from_headers(&headers).await
    {
        return (status, json).into_response();
    }

    let (status, json) = api::config::preview_settings_restore(Json(payload)).await;
    (status, json).into_response()
}

/// Atomically restore a versioned settings backup.
async fn restore_settings_wrapper(
    headers: axum::http::HeaderMap,
    Json(payload): Json<api::config::SettingsBackup>,
) -> Response {
    let claims = match middleware::auth::verify_current_admin_from_headers(&headers).await {
        Ok(claims) => claims,
        Err((status, json)) => return (status, json).into_response(),
    };
    let user_id = match claims.sub.parse::<i32>() {
        Ok(user_id) => user_id,
        Err(_) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Invalid authenticated user"})),
            )
                .into_response();
        }
    };

    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let (status, json) = api::config::restore_settings(
                axum::extract::State(db.clone()),
                user_id,
                Json(payload),
            )
            .await;
            (status, json).into_response()
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

/// Wrapper for update_dashboard_config that gets DB from global state
async fn update_dashboard_config_wrapper(
    Json(payload): Json<api::config::DashboardConfigPayload>,
) -> Response {
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let (status, json) = api::config::update_dashboard_config(
                axum::extract::State(db.clone()),
                Json(payload),
            )
            .await;
            (status, json).into_response()
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "Database not connected",
                "message": "数据库未连接，配置功能暂不可用"
            })),
        )
            .into_response(),
    }
}

/// Wrapper for update_control_panel_config that gets DB from global state
async fn update_control_panel_config_wrapper(
    Json(payload): Json<api::config::ControlPanelConfigPayload>,
) -> Response {
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let (status, json) = api::config::update_control_panel_config(
                axum::extract::State(db.clone()),
                Json(payload),
            )
            .await;
            (status, json).into_response()
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "Database not connected",
                "message": "数据库未连接，配置功能暂不可用"
            })),
        )
            .into_response(),
    }
}

/// Wrapper for update_tapp_window_schemes that gets DB from global state
async fn update_tapp_window_schemes_wrapper(
    Json(payload): Json<api::config::TappWindowSchemesPayload>,
) -> Response {
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let (status, json) = api::config::update_tapp_window_schemes(
                axum::extract::State(db.clone()),
                Json(payload),
            )
            .await;
            (status, json).into_response()
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "Database not connected",
                "message": "数据库未连接，配置功能暂不可用"
            })),
        )
            .into_response(),
    }
}

/// Wrapper for get_module_visibility_preferences that gets DB from global state
async fn get_module_visibility_preferences_wrapper() -> Response {
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let (status, json) =
                api::config::get_module_visibility_preferences(axum::extract::State(db.clone()))
                    .await;
            (status, json).into_response()
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "Database not connected",
                "message": "数据库未连接，模块可见性功能暂不可用"
            })),
        )
            .into_response(),
    }
}

/// Wrapper for update_module_visibility_preferences that gets DB from global state
async fn update_module_visibility_preferences_wrapper(
    Json(payload): Json<api::config::ModuleVisibilityPreferences>,
) -> Response {
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let (status, json) = api::config::update_module_visibility_preferences(
                axum::extract::State(db.clone()),
                Json(payload),
            )
            .await;
            (status, json).into_response()
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "Database not connected",
                "message": "数据库未连接，模块可见性功能暂不可用"
            })),
        )
            .into_response(),
    }
}

/// Wrapper for get_hitokoto_config that gets DB from global state
async fn get_hitokoto_config_wrapper() -> Response {
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let (status, json) =
                api::config::get_hitokoto_config(axum::extract::State(db.clone())).await;
            (status, json).into_response()
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "Database not connected",
                "message": "数据库未连接，一言配置功能暂不可用"
            })),
        )
            .into_response(),
    }
}

/// Wrapper for update_hitokoto_config that gets DB from global state
async fn update_hitokoto_config_wrapper(
    Json(payload): Json<api::config::HitokotoConfig>,
) -> Response {
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let (status, json) = api::config::update_hitokoto_config(
                axum::extract::State(db.clone()),
                Json(payload),
            )
            .await;
            (status, json).into_response()
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "Database not connected",
                "message": "数据库未连接，一言配置功能暂不可用"
            })),
        )
            .into_response(),
    }
}

/// Wrapper for get_report_settings that gets DB from global state
async fn get_report_settings_wrapper() -> Response {
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let (status, json) =
                api::config::get_report_settings(axum::extract::State(db.clone())).await;
            (status, json).into_response()
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "Database not connected",
                "message": "数据库未连接，报告设置功能暂不可用"
            })),
        )
            .into_response(),
    }
}

/// Wrapper for update_report_settings that gets DB from global state
async fn update_report_settings_wrapper(
    Json(payload): Json<api::config::ReportSettings>,
) -> Response {
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let (status, json) = api::config::update_report_settings(
                axum::extract::State(db.clone()),
                Json(payload),
            )
            .await;
            (status, json).into_response()
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "Database not connected",
                "message": "数据库未连接，报告设置功能暂不可用"
            })),
        )
            .into_response(),
    }
}

/// Wrapper for get_permissions that gets DB from global state
async fn get_permissions_wrapper(headers: axum::http::HeaderMap) -> Response {
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let (status, json) =
                api::config::get_permissions(axum::extract::State(db.clone()), headers).await;
            (status, json).into_response()
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "Database not connected",
                "message": "数据库未连接，权限功能暂不可用"
            })),
        )
            .into_response(),
    }
}

/// Wrapper for update_permissions that gets DB from global state
async fn update_permissions_wrapper(
    Json(payload): Json<api::config::UpdatePermissionsPayload>,
) -> Response {
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let (status, json) =
                api::config::update_permissions(axum::extract::State(db.clone()), Json(payload))
                    .await;
            (status, json).into_response()
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "Database not connected",
                "message": "数据库未连接，权限功能暂不可用"
            })),
        )
            .into_response(),
    }
}

/// PR #6: Wrapper for get_oauth_providers (admin)
async fn get_oauth_providers_wrapper() -> Response {
    let (status, json) = api::config::get_oauth_providers().await;
    (status, json).into_response()
}

/// PR #6: Wrapper for update_oauth_providers (admin)
async fn update_oauth_providers_wrapper(
    Json(payload): Json<api::config::UpdateOAuthProvidersPayload>,
) -> Response {
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let (status, json) = api::config::update_oauth_providers(
                axum::extract::State(db.clone()),
                Json(payload),
            )
            .await;
            (status, json).into_response()
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

/// Wrapper for test_platform that gets DB from global state
async fn test_platform_wrapper(Json(payload): Json<serde_json::Value>) -> Response {
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let (status, json) =
                api::config::test_platform(axum::extract::State(db.clone()), Json(payload)).await;
            (status, json).into_response()
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "Database not connected",
                "message": "数据库未连接，配置功能暂不可用"
            })),
        )
            .into_response(),
    }
}

/// Wrapper for get_current_user that gets DB from global state
async fn get_current_user_wrapper(headers: axum::http::HeaderMap) -> Response {
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            match api::auth::get_current_user(axum::extract::State(db.clone()), headers).await {
                Ok(response) => response.into_response(),
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "Database not connected",
                "message": "数据库未连接，认证功能暂不可用"
            })),
        )
            .into_response(),
    }
}

/// Wrapper for logout - no auth required, just clear cookie
async fn logout_wrapper() -> Response {
    api::auth::logout().await.into_response()
}

/// Wrapper for OAuth callback that gets DB from global state.
///
/// Keep the route registered even when DB is temporarily unavailable, so the
/// login surface gets a clear 503 instead of a route-table 404.
async fn oauth_provider_callback_wrapper(
    axum::extract::Path(slug): axum::extract::Path<String>,
    axum::extract::Query(params): axum::extract::Query<api::oauth::CallbackQuery>,
) -> Response {
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => match api::oauth::provider_callback(
            axum::extract::Path(slug),
            axum::extract::Query(params),
            axum::extract::State(db.clone()),
        )
        .await
        {
            Ok(response) => response,
            Err((status, json)) => (status, json).into_response(),
        },
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "Database not connected",
                "message": "数据库未连接，OAuth 回调暂不可用"
            })),
        )
            .into_response(),
    }
}

/// Discord 数据平台一键授权 callback（需要 DB 写配置）
async fn discord_platform_oauth_callback_wrapper(
    axum::extract::Query(params): axum::extract::Query<api::discord::OAuthCallbackQuery>,
) -> Response {
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => match api::discord::oauth_callback(
            axum::extract::State(db.clone()),
            axum::extract::Query(params),
        )
        .await
        {
            Ok(response) => response,
            Err((status, json)) => (status, json).into_response(),
        },
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "Database not connected",
                "message": "数据库未连接，Discord 数据授权回调暂不可用"
            })),
        )
            .into_response(),
    }
}

/// Wrapper for OAuth identity unlink that gets DB from global state.
async fn oauth_provider_unlink_wrapper(
    axum::extract::Path((slug, identity_id)): axum::extract::Path<(String, i32)>,
    headers: axum::http::HeaderMap,
) -> Response {
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => match api::oauth::provider_unlink(
            axum::extract::Path((slug, identity_id)),
            axum::extract::State(db.clone()),
            headers,
        )
        .await
        {
            Ok(json) => json.into_response(),
            Err((status, json)) => (status, json).into_response(),
        },
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "Database not connected",
                "message": "数据库未连接，OAuth 身份解绑暂不可用"
            })),
        )
            .into_response(),
    }
}

/// Wrapper for listing the current user's linked OAuth identities.
async fn oauth_list_my_identities_wrapper(headers: axum::http::HeaderMap) -> Response {
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            match api::oauth::list_my_identities(axum::extract::State(db.clone()), headers).await {
                Ok(json) => json.into_response(),
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "Database not connected",
                "message": "数据库未连接，OAuth 身份列表暂不可用"
            })),
        )
            .into_response(),
    }
}

/// Wrapper for get_user_info that gets DB from global state
async fn get_user_info_wrapper() -> Response {
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let (status, json) =
                api::profile::get_user_info(axum::extract::State(db.clone())).await;
            (status, json).into_response()
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "Database not connected",
                "message": "数据库未连接"
            })),
        )
            .into_response(),
    }
}

/// Wrapper for get_batch_user_info that gets DB from global state
/// 批量获取用户信息 - 性能优化版本，减少多次API调用
async fn get_batch_user_info_wrapper() -> Response {
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let (status, json) =
                api::profile::get_batch_user_info(axum::extract::State(db.clone())).await;
            (status, json).into_response()
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "Database not connected",
                "message": "数据库未连接"
            })),
        )
            .into_response(),
    }
}

/// Wrapper for get_raw_metadata that gets DB from global state
async fn get_raw_metadata_wrapper() -> Response {
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let (status, json) =
                api::profile::get_raw_metadata(axum::extract::State(db.clone())).await;
            (status, json).into_response()
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "Database not connected",
                "message": "数据库未连接"
            })),
        )
            .into_response(),
    }
}

/// Wrapper for get_latest_report that gets DB from global state
async fn get_latest_report_wrapper(headers: axum::http::HeaderMap) -> Response {
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            match api::reports::get_latest_report(axum::extract::State(db.clone()), headers).await {
                Ok(json) => (StatusCode::OK, json).into_response(),
                Err(status) => {
                    (status, Json(json!({ "error": "Failed to get report" }))).into_response()
                }
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "Database not connected",
                "message": "数据库未连接，无法获取报告"
            })),
        )
            .into_response(),
    }
}

/// Wrapper for get_comprehensive_reports_list that gets DB from global state
async fn get_comprehensive_reports_list_wrapper(headers: axum::http::HeaderMap) -> Response {
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            match api::reports::get_comprehensive_reports_list(
                axum::extract::State(db.clone()),
                headers,
            )
            .await
            {
                Ok(json) => (StatusCode::OK, json).into_response(),
                Err(status) => (
                    status,
                    Json(json!({ "error": "Failed to get comprehensive reports" })),
                )
                    .into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "Database not connected",
                "message": "数据库未连接，无法获取综合报告列表"
            })),
        )
            .into_response(),
    }
}

/// Wrapper for get_comprehensive_report_by_id that gets DB from global state
async fn get_comprehensive_report_by_id_wrapper(
    headers: axum::http::HeaderMap,
    axum::extract::Path(report_id): axum::extract::Path<i32>,
) -> Response {
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            match api::reports::get_comprehensive_report_by_id(
                axum::extract::State(db.clone()),
                headers,
                axum::extract::Path(report_id),
            )
            .await
            {
                Ok(json) => (StatusCode::OK, json).into_response(),
                Err(status) => (
                    status,
                    Json(json!({ "error": "Failed to get comprehensive report" })),
                )
                    .into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "Database not connected",
                "message": "数据库未连接，无法获取综合报告详情"
            })),
        )
            .into_response(),
    }
}

// ==================== Federation Wrappers ====================

async fn federation_admin_required(claims: &middleware::auth::Claims) -> Option<Response> {
    match middleware::auth::ensure_current_admin(claims).await {
        Ok(()) => None,
        Err((status, body)) => Some((status, body).into_response()),
    }
}

/// GET /api/federation/identity — 获取当前登录用户的联邦地址
async fn federation_identity_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };

    let identity = federation::actor::get_local_identity(&claims.username).await;
    (StatusCode::OK, Json(identity)).into_response()
}

/// POST /api/federation/follow — 关注远程用户
async fn federation_follow_wrapper(req: axum::extract::Request) -> Response {
    let claims = req.extensions().get::<middleware::auth::Claims>().cloned();
    let claims = match claims {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let body_bytes = match axum::body::Bytes::from_request(req, &()).await {
        Ok(b) => b,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid body"})),
            )
                .into_response()
        }
    };
    let payload: federation::follow::FollowRequest = match serde_json::from_slice(&body_bytes) {
        Ok(p) => p,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid JSON"})),
            )
                .into_response()
        }
    };
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let user_id: i32 = claims.sub.parse().unwrap_or(0);
            match federation::follow::follow_remote(user_id, &claims.username, db, &payload.target)
                .await
            {
                Ok(resp) => {
                    (StatusCode::OK, Json(serde_json::to_value(resp).unwrap())).into_response()
                }
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

/// POST /api/federation/unfollow — 取消关注远程用户
async fn federation_unfollow_wrapper(req: axum::extract::Request) -> Response {
    let claims = req.extensions().get::<middleware::auth::Claims>().cloned();
    let claims = match claims {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let body_bytes = match axum::body::Bytes::from_request(req, &()).await {
        Ok(b) => b,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid body"})),
            )
                .into_response()
        }
    };
    let payload: federation::follow::FollowRequest = match serde_json::from_slice(&body_bytes) {
        Ok(p) => p,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid JSON"})),
            )
                .into_response()
        }
    };
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let user_id: i32 = claims.sub.parse().unwrap_or(0);
            match federation::follow::unfollow_remote(
                user_id,
                &claims.username,
                db,
                &payload.target,
            )
            .await
            {
                Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

/// GET /api/federation/following — 获取我关注的远程用户列表
async fn federation_following_list_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let user_id: i32 = claims.sub.parse().unwrap_or(0);
            match get_follow_list(db, user_id, "outgoing").await {
                Ok(list) => (StatusCode::OK, Json(list)).into_response(),
                Err(e) => {
                    (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))).into_response()
                }
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

/// GET /api/federation/followers — 获取关注我的远程用户列表
async fn federation_followers_list_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let user_id: i32 = claims.sub.parse().unwrap_or(0);
            match get_follow_list(db, user_id, "incoming").await {
                Ok(list) => (StatusCode::OK, Json(list)).into_response(),
                Err(e) => {
                    (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))).into_response()
                }
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

/// GET /api/federation/timeline — 获取联邦时间线
async fn federation_timeline_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let user_id: i32 = claims.sub.parse().unwrap_or(0);
            match get_federation_timeline(db, user_id).await {
                Ok(timeline) => (StatusCode::OK, Json(timeline)).into_response(),
                Err(e) => {
                    (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))).into_response()
                }
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

// ==================== Phase 2: Content Publishing Wrappers ====================

/// POST /api/federation/publish — 发布内容到联邦网络
async fn federation_publish_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let body_bytes = match axum::body::Bytes::from_request(req, &()).await {
        Ok(b) => b,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid body"})),
            )
                .into_response()
        }
    };
    let payload: federation::content::PublishRequest = match serde_json::from_slice(&body_bytes) {
        Ok(p) => p,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid JSON"})),
            )
                .into_response()
        }
    };
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let user_id: i32 = claims.sub.parse().unwrap_or(0);
            match federation::content::publish_content(user_id, &claims.username, db, &payload)
                .await
            {
                Ok(resp) => {
                    (StatusCode::OK, Json(serde_json::to_value(resp).unwrap())).into_response()
                }
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

/// Shared body parse helper for object-id interaction endpoints.
async fn federation_object_id_from_body(
    req: axum::extract::Request,
) -> Result<(middleware::auth::Claims, federation::interactions::ObjectIdRequest), Response> {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response())
        }
    };
    let body_bytes = match axum::body::Bytes::from_request(req, &()).await {
        Ok(b) => b,
        Err(_) => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid body"})),
            )
                .into_response())
        }
    };
    let payload: federation::interactions::ObjectIdRequest =
        match serde_json::from_slice(&body_bytes) {
            Ok(p) => p,
            Err(_) => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": "Invalid JSON (expect { object_id })"})),
                )
                    .into_response())
            }
        };
    Ok((claims, payload))
}

async fn federation_like_wrapper(req: axum::extract::Request) -> Response {
    let (claims, payload) = match federation_object_id_from_body(req).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let user_id: i32 = claims.sub.parse().unwrap_or(0);
            match federation::interactions::like_object(
                user_id,
                &claims.username,
                db,
                &payload.object_id,
            )
            .await
            {
                Ok(resp) => (StatusCode::OK, Json(serde_json::to_value(resp).unwrap())).into_response(),
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

async fn federation_unlike_wrapper(req: axum::extract::Request) -> Response {
    let (claims, payload) = match federation_object_id_from_body(req).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let user_id: i32 = claims.sub.parse().unwrap_or(0);
            match federation::interactions::unlike_object(
                user_id,
                &claims.username,
                db,
                &payload.object_id,
            )
            .await
            {
                Ok(resp) => (StatusCode::OK, Json(serde_json::to_value(resp).unwrap())).into_response(),
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

async fn federation_bookmark_wrapper(req: axum::extract::Request) -> Response {
    let (claims, payload) = match federation_object_id_from_body(req).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let user_id: i32 = claims.sub.parse().unwrap_or(0);
            match federation::interactions::bookmark_object(user_id, db, &payload.object_id).await {
                Ok(resp) => (StatusCode::OK, Json(serde_json::to_value(resp).unwrap())).into_response(),
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

async fn federation_unbookmark_wrapper(req: axum::extract::Request) -> Response {
    let (claims, payload) = match federation_object_id_from_body(req).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let user_id: i32 = claims.sub.parse().unwrap_or(0);
            match federation::interactions::unbookmark_object(user_id, db, &payload.object_id).await
            {
                Ok(resp) => (StatusCode::OK, Json(serde_json::to_value(resp).unwrap())).into_response(),
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

async fn federation_bookmarks_list_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let user_id: i32 = claims.sub.parse().unwrap_or(0);
            match federation::interactions::list_bookmarks(user_id, db).await {
                Ok(resp) => (StatusCode::OK, Json(serde_json::to_value(resp).unwrap())).into_response(),
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

async fn federation_announce_from_body(
    req: axum::extract::Request,
) -> Result<(middleware::auth::Claims, federation::interactions::AnnounceRequest), Response> {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response())
        }
    };
    let body_bytes = match axum::body::Bytes::from_request(req, &()).await {
        Ok(b) => b,
        Err(_) => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid body"})),
            )
                .into_response())
        }
    };
    let payload: federation::interactions::AnnounceRequest =
        match serde_json::from_slice(&body_bytes) {
            Ok(p) => p,
            Err(_) => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": "Invalid JSON (expect { object_id, content })"})),
                )
                    .into_response())
            }
        };
    Ok((claims, payload))
}

async fn federation_announce_wrapper(req: axum::extract::Request) -> Response {
    let (claims, payload) = match federation_announce_from_body(req).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let user_id: i32 = claims.sub.parse().unwrap_or(0);
            let content = payload.content.as_deref().unwrap_or("");
            match federation::interactions::announce_object(
                user_id,
                &claims.username,
                db,
                &payload.object_id,
                content,
            )
            .await
            {
                Ok(resp) => (StatusCode::OK, Json(serde_json::to_value(resp).unwrap())).into_response(),
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

async fn federation_unannounce_wrapper(req: axum::extract::Request) -> Response {
    let (claims, payload) = match federation_object_id_from_body(req).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let user_id: i32 = claims.sub.parse().unwrap_or(0);
            match federation::interactions::unannounce_object(
                user_id,
                &claims.username,
                db,
                &payload.object_id,
            )
            .await
            {
                Ok(resp) => (StatusCode::OK, Json(serde_json::to_value(resp).unwrap())).into_response(),
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

/// POST /api/federation/notes — 创建 freeform Note（文本 + 附件）
async fn federation_create_note_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let body_bytes = match axum::body::Bytes::from_request(req, &()).await {
        Ok(b) => b,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid body"})),
            )
                .into_response()
        }
    };
    let payload: federation::content::CreateNoteRequest = match serde_json::from_slice(&body_bytes) {
        Ok(p) => p,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid JSON"})),
            )
                .into_response()
        }
    };
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let user_id: i32 = claims.sub.parse().unwrap_or(0);
            match federation::content::create_note(user_id, &claims.username, db, &payload).await {
                Ok(resp) => {
                    (StatusCode::OK, Json(serde_json::to_value(resp).unwrap())).into_response()
                }
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

/// POST /api/federation/media — multipart 上传图片/视频，返回公开 URL
async fn federation_media_upload_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    let mut multipart = match axum::extract::Multipart::from_request(req, &()).await {
        Ok(m) => m,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Expected multipart/form-data"})),
            )
                .into_response()
        }
    };

    let mut file_bytes: Option<Vec<u8>> = None;
    let mut filename = "upload.bin".to_string();
    let mut mime = "application/octet-stream".to_string();

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        if name != "file" {
            continue;
        }
        if let Some(fname) = field.file_name() {
            filename = fname.to_string();
        }
        if let Some(ct) = field.content_type() {
            mime = ct.to_string();
        }
        match field.bytes().await {
            Ok(b) => file_bytes = Some(b.to_vec()),
            Err(_) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": "Failed to read file field"})),
                )
                    .into_response()
            }
        }
        break;
    }

    let Some(bytes) = file_bytes else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Missing multipart field 'file'"})),
        )
            .into_response();
    };

    match federation::content::store_federation_media(user_id, &filename, &mime, &bytes).await {
        Ok(resp) => (StatusCode::OK, Json(serde_json::to_value(resp).unwrap())).into_response(),
        Err((status, json)) => (status, json).into_response(),
    }
}

/// POST /api/federation/unpublish — 取消发布
async fn federation_unpublish_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let body_bytes = match axum::body::Bytes::from_request(req, &()).await {
        Ok(b) => b,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid body"})),
            )
                .into_response()
        }
    };
    let payload: serde_json::Value = match serde_json::from_slice(&body_bytes) {
        Ok(p) => p,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid JSON"})),
            )
                .into_response()
        }
    };
    let content_type = payload["content_type"].as_str().unwrap_or("").trim();
    let content_id = payload["content_id"].as_str().unwrap_or("").trim();
    let activity_id = payload["activity_id"].as_str().unwrap_or("").trim();
    let has_activity = !activity_id.is_empty();
    let has_content = !content_id.is_empty(); // content_type optional when activity_id or inferable
    if !has_activity && !has_content {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Provide activity_id, or content_type + content_id"
            })),
        )
            .into_response();
    }
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let user_id: i32 = claims.sub.parse().unwrap_or(0);
            match federation::content::unpublish_content(
                user_id,
                &claims.username,
                db,
                if content_type.is_empty() { None } else { Some(content_type) },
                if content_id.is_empty() { None } else { Some(content_id) },
                if activity_id.is_empty() { None } else { Some(activity_id) },
            )
            .await
            {
                Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

/// GET /api/federation/published — 获取已发布内容列表
async fn federation_published_list_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let user_id: i32 = claims.sub.parse().unwrap_or(0);
            match federation::content::list_published(user_id, db).await {
                Ok(items) => (
                    StatusCode::OK,
                    Json(json!({"items": items, "total": items.len()})),
                )
                    .into_response(),
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

// ==================== Phase 3: Channel Wrapper Functions ====================

/// 创建 Channel
async fn federation_create_channel_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let body_bytes = match axum::body::Bytes::from_request(req, &()).await {
        Ok(b) => b,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid body"})),
            )
                .into_response()
        }
    };
    let payload: federation::channel::CreateChannelRequest =
        match serde_json::from_slice(&body_bytes) {
            Ok(p) => p,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": format!("Invalid JSON: {}", e)})),
                )
                    .into_response()
            }
        };
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let user_id: i32 = claims.sub.parse().unwrap_or(0);
            match federation::channel::create_channel(user_id, &claims.username, db, &payload).await
            {
                Ok(resp) => {
                    (StatusCode::OK, Json(serde_json::to_value(resp).unwrap())).into_response()
                }
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

/// Channel 列表
async fn federation_list_channels_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let user_id: i32 = claims.sub.parse().unwrap_or(0);
            match federation::channel::list_channels(user_id, &claims.username, db).await {
                Ok(channels) => (
                    StatusCode::OK,
                    Json(json!({"channels": channels, "total": channels.len()})),
                )
                    .into_response(),
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

/// Channel 详情
async fn federation_get_channel_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let channel_id = req
        .uri()
        .path()
        .strip_prefix("/api/federation/channels/")
        .unwrap_or("")
        .split('/')
        .next()
        .unwrap_or("")
        .to_string();
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let user_id: i32 = claims.sub.parse().unwrap_or(0);
            match federation::channel::get_channel(user_id, &channel_id, db).await {
                Ok(detail) => {
                    (StatusCode::OK, Json(serde_json::to_value(detail).unwrap())).into_response()
                }
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

/// 关闭 Channel
async fn federation_close_channel_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let channel_id = req
        .uri()
        .path()
        .strip_prefix("/api/federation/channels/")
        .unwrap_or("")
        .strip_suffix("/close")
        .unwrap_or("")
        .to_string();
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let user_id: i32 = claims.sub.parse().unwrap_or(0);
            match federation::channel::close_channel(user_id, &claims.username, &channel_id, db)
                .await
            {
                Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

/// 删除已关闭的 Channel（本地硬删除）
async fn federation_delete_channel_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let channel_id = req
        .uri()
        .path()
        .strip_prefix("/api/federation/channels/")
        .unwrap_or("")
        .split('/')
        .next()
        .unwrap_or("")
        .to_string();
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let user_id: i32 = claims.sub.parse().unwrap_or(0);
            match federation::channel::delete_channel(user_id, &channel_id, db).await {
                Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

/// 接受 Channel
async fn federation_accept_channel_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let channel_id = req
        .uri()
        .path()
        .strip_prefix("/api/federation/channels/")
        .unwrap_or("")
        .strip_suffix("/accept")
        .unwrap_or("")
        .to_string();
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let user_id: i32 = claims.sub.parse().unwrap_or(0);
            match federation::channel::accept_channel(user_id, &claims.username, &channel_id, db)
                .await
            {
                Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

/// 发起 Channel E2E 密钥交换
async fn federation_e2e_key_exchange_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let channel_id = req
        .uri()
        .path()
        .strip_prefix("/api/federation/channels/")
        .unwrap_or("")
        .strip_suffix("/e2e/key-exchange")
        .unwrap_or("")
        .to_string();
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let user_id: i32 = claims.sub.parse().unwrap_or(0);
            match federation::channel::initiate_e2e_key_exchange(
                user_id,
                &claims.username,
                &channel_id,
                db,
            )
            .await
            {
                Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

/// 发送消息
async fn federation_send_message_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let channel_id = req
        .uri()
        .path()
        .strip_prefix("/api/federation/channels/")
        .unwrap_or("")
        .strip_suffix("/messages")
        .unwrap_or("")
        .to_string();
    let body_bytes = match axum::body::Bytes::from_request(req, &()).await {
        Ok(b) => b,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid body"})),
            )
                .into_response()
        }
    };
    let payload: federation::channel::SendMessageRequest = match serde_json::from_slice(&body_bytes)
    {
        Ok(p) => p,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": format!("Invalid JSON: {}", e)})),
            )
                .into_response()
        }
    };
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let user_id: i32 = claims.sub.parse().unwrap_or(0);
            match federation::channel::send_message(
                user_id,
                &claims.username,
                &channel_id,
                db,
                &payload,
            )
            .await
            {
                Ok(resp) => {
                    (StatusCode::OK, Json(serde_json::to_value(resp).unwrap())).into_response()
                }
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

/// 获取消息历史
async fn federation_get_messages_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let uri = req.uri().clone();
    let path = uri.path();
    let channel_id = path
        .strip_prefix("/api/federation/channels/")
        .unwrap_or("")
        .strip_suffix("/messages")
        .unwrap_or("")
        .to_string();
    // 解析查询参数
    let query_str = uri.query().unwrap_or("");
    let params: std::collections::HashMap<String, String> =
        url::form_urlencoded::parse(query_str.as_bytes())
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
    let before = params.get("before").map(|s| s.as_str());
    let limit = params.get("limit").and_then(|s| s.parse::<i64>().ok());

    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let user_id: i32 = claims.sub.parse().unwrap_or(0);
            match federation::channel::get_messages(user_id, &channel_id, db, before, limit).await {
                Ok(messages) => (
                    StatusCode::OK,
                    Json(json!({"messages": messages, "total": messages.len()})),
                )
                    .into_response(),
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

// ==================== Phase 4: Room 多方通信 Wrapper ====================

async fn federation_create_room_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let body = match axum::body::to_bytes(req.into_body(), 1024 * 64).await {
        Ok(b) => b,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid body"})),
            )
                .into_response()
        }
    };
    let parsed: federation::room::CreateRoomRequest = match serde_json::from_slice(&body) {
        Ok(p) => p,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid JSON"})),
            )
                .into_response()
        }
    };
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let user_id: i32 = claims.sub.parse().unwrap_or(0);
            match federation::room::create_room(user_id, &claims.username, db, &parsed).await {
                Ok(detail) => (StatusCode::OK, Json(json!(detail))).into_response(),
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

async fn federation_list_rooms_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let user_id: i32 = claims.sub.parse().unwrap_or(0);
            match federation::room::list_rooms(user_id, &claims.username, db).await {
                Ok(rooms) => (
                    StatusCode::OK,
                    Json(json!({"rooms": rooms, "total": rooms.len()})),
                )
                    .into_response(),
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

async fn federation_get_room_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let path = req.uri().path().to_string();
    let room_id = path
        .strip_prefix("/api/federation/rooms/")
        .unwrap_or("")
        .to_string();
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let user_id: i32 = claims.sub.parse().unwrap_or(0);
            match federation::room::get_room(user_id, &claims.username, &room_id, db).await {
                Ok(detail) => (StatusCode::OK, Json(json!(detail))).into_response(),
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

async fn federation_update_room_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let path = req.uri().path().to_string();
    let room_id = path
        .strip_prefix("/api/federation/rooms/")
        .unwrap_or("")
        .to_string();
    let body = match axum::body::to_bytes(req.into_body(), 1024 * 64).await {
        Ok(b) => b,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid body"})),
            )
                .into_response()
        }
    };
    let parsed: federation::room::UpdateRoomRequest = match serde_json::from_slice(&body) {
        Ok(p) => p,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid JSON"})),
            )
                .into_response()
        }
    };
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let user_id: i32 = claims.sub.parse().unwrap_or(0);
            match federation::room::update_room(user_id, &claims.username, &room_id, db, &parsed)
                .await
            {
                Ok(detail) => (StatusCode::OK, Json(json!(detail))).into_response(),
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

async fn federation_delete_room_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let path = req.uri().path().to_string();
    let room_id = path
        .strip_prefix("/api/federation/rooms/")
        .unwrap_or("")
        .to_string();
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let user_id: i32 = claims.sub.parse().unwrap_or(0);
            match federation::room::delete_room(user_id, &claims.username, &room_id, db).await {
                Ok(result) => (StatusCode::OK, Json(result)).into_response(),
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

async fn federation_get_room_members_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let path = req.uri().path().to_string();
    let room_id = path
        .strip_prefix("/api/federation/rooms/")
        .unwrap_or("")
        .strip_suffix("/members")
        .unwrap_or("")
        .to_string();
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let user_id: i32 = claims.sub.parse().unwrap_or(0);
            match federation::room::get_members(user_id, &claims.username, &room_id, db).await {
                Ok(members) => (
                    StatusCode::OK,
                    Json(json!({"members": members, "total": members.len()})),
                )
                    .into_response(),
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

async fn federation_invite_room_member_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let path = req.uri().path().to_string();
    let room_id = path
        .strip_prefix("/api/federation/rooms/")
        .unwrap_or("")
        .strip_suffix("/invite")
        .unwrap_or("")
        .to_string();
    let body = match axum::body::to_bytes(req.into_body(), 1024 * 64).await {
        Ok(b) => b,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid body"})),
            )
                .into_response()
        }
    };
    let parsed: federation::room::InviteMemberRequest = match serde_json::from_slice(&body) {
        Ok(p) => p,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid JSON"})),
            )
                .into_response()
        }
    };
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let user_id: i32 = claims.sub.parse().unwrap_or(0);
            match federation::room::invite_member(user_id, &claims.username, &room_id, db, &parsed)
                .await
            {
                Ok(result) => (StatusCode::OK, Json(result)).into_response(),
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

async fn federation_remove_room_member_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let path = req.uri().path().to_string();
    // /api/federation/rooms/{room_id}/members/{actor} — actor is URL-encoded
    let after_rooms = path.strip_prefix("/api/federation/rooms/").unwrap_or("");
    let parts: Vec<&str> = after_rooms.splitn(2, "/members/").collect();
    let room_id = parts.first().copied().unwrap_or("").to_string();
    let target_actor = parts.get(1).copied().unwrap_or("");
    let target_actor_decoded = urlencoding::decode(target_actor)
        .unwrap_or_default()
        .to_string();
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let user_id: i32 = claims.sub.parse().unwrap_or(0);
            match federation::room::remove_member(
                user_id,
                &claims.username,
                &room_id,
                &target_actor_decoded,
                db,
            )
            .await
            {
                Ok(result) => (StatusCode::OK, Json(result)).into_response(),
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

async fn federation_leave_room_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let path = req.uri().path().to_string();
    let room_id = path
        .strip_prefix("/api/federation/rooms/")
        .unwrap_or("")
        .strip_suffix("/leave")
        .unwrap_or("")
        .to_string();
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let user_id: i32 = claims.sub.parse().unwrap_or(0);
            match federation::room::leave_room(user_id, &claims.username, &room_id, db).await {
                Ok(result) => (StatusCode::OK, Json(result)).into_response(),
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

/// POST /api/federation/rooms/{room_id}/accept — accept pending room invite
async fn federation_accept_room_invite_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let path = req.uri().path().to_string();
    let room_id = path
        .strip_prefix("/api/federation/rooms/")
        .unwrap_or("")
        .strip_suffix("/accept")
        .unwrap_or("")
        .to_string();
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let user_id: i32 = claims.sub.parse().unwrap_or(0);
            match federation::room::accept_room_invite(user_id, &claims.username, &room_id, db)
                .await
            {
                Ok(result) => (StatusCode::OK, Json(result)).into_response(),
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

/// POST /api/federation/rooms/{room_id}/reject — reject pending room invite
async fn federation_reject_room_invite_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let path = req.uri().path().to_string();
    let room_id = path
        .strip_prefix("/api/federation/rooms/")
        .unwrap_or("")
        .strip_suffix("/reject")
        .unwrap_or("")
        .to_string();
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let user_id: i32 = claims.sub.parse().unwrap_or(0);
            match federation::room::reject_room_invite(user_id, &claims.username, &room_id, db)
                .await
            {
                Ok(result) => (StatusCode::OK, Json(result)).into_response(),
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

/// POST /api/federation/rooms/{room_id}/transfer-ownership
async fn federation_transfer_room_ownership_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let path = req.uri().path().to_string();
    let room_id = path
        .strip_prefix("/api/federation/rooms/")
        .unwrap_or("")
        .strip_suffix("/transfer-ownership")
        .unwrap_or("")
        .to_string();
    let body = match axum::body::to_bytes(req.into_body(), 64 * 1024).await {
        Ok(b) => b,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid body"})),
            )
                .into_response()
        }
    };
    let parsed: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid JSON"})),
            )
                .into_response()
        }
    };
    let new_owner = parsed
        .get("new_owner")
        .or_else(|| parsed.get("actor"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let user_id: i32 = claims.sub.parse().unwrap_or(0);
            match federation::room::transfer_room_ownership(
                user_id,
                &claims.username,
                &room_id,
                &new_owner,
                db,
            )
            .await
            {
                Ok(result) => (StatusCode::OK, Json(result)).into_response(),
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

/// 发起 Room E2E 密钥发布
async fn federation_room_e2e_key_exchange_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let room_id = req
        .uri()
        .path()
        .strip_prefix("/api/federation/rooms/")
        .unwrap_or("")
        .strip_suffix("/e2e/key-exchange")
        .unwrap_or("")
        .to_string();
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let user_id: i32 = claims.sub.parse().unwrap_or(0);
            match federation::room::initiate_e2e_key_exchange(
                user_id,
                &claims.username,
                &room_id,
                db,
            )
            .await
            {
                Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

async fn federation_send_room_message_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let path = req.uri().path().to_string();
    let room_id = path
        .strip_prefix("/api/federation/rooms/")
        .unwrap_or("")
        .strip_suffix("/messages")
        .unwrap_or("")
        .to_string();
    let body = match axum::body::to_bytes(req.into_body(), 1024 * 64).await {
        Ok(b) => b,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid body"})),
            )
                .into_response()
        }
    };
    let parsed: federation::room::SendRoomMessageRequest = match serde_json::from_slice(&body) {
        Ok(p) => p,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid JSON"})),
            )
                .into_response()
        }
    };
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let user_id: i32 = claims.sub.parse().unwrap_or(0);
            match federation::room::send_room_message(
                user_id,
                &claims.username,
                &room_id,
                db,
                &parsed,
            )
            .await
            {
                Ok(resp) => (StatusCode::OK, Json(json!(resp))).into_response(),
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

async fn federation_get_room_messages_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let uri = req.uri().clone();
    let path = uri.path();
    let room_id = path
        .strip_prefix("/api/federation/rooms/")
        .unwrap_or("")
        .strip_suffix("/messages")
        .unwrap_or("")
        .to_string();
    let query_str = uri.query().unwrap_or("");
    let params: std::collections::HashMap<String, String> =
        url::form_urlencoded::parse(query_str.as_bytes())
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
    let before = params.get("before").map(|s| s.as_str());
    let limit = params.get("limit").and_then(|s| s.parse::<i64>().ok());
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let user_id: i32 = claims.sub.parse().unwrap_or(0);
            match federation::room::get_room_messages(
                user_id,
                &claims.username,
                &room_id,
                db,
                before,
                limit,
            )
            .await
            {
                Ok(messages) => (
                    StatusCode::OK,
                    Json(json!({"messages": messages, "total": messages.len()})),
                )
                    .into_response(),
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

async fn federation_pin_room_message_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let path = req.uri().path().to_string();
    let after_rooms = path.strip_prefix("/api/federation/rooms/").unwrap_or("");
    let parts: Vec<&str> = after_rooms.splitn(2, "/messages/").collect();
    let room_id = parts.first().copied().unwrap_or("").to_string();
    let message_encoded = parts
        .get(1)
        .copied()
        .unwrap_or("")
        .strip_suffix("/pin")
        .unwrap_or("");
    let message_id = urlencoding::decode(message_encoded)
        .unwrap_or_default()
        .to_string();
    let body = match axum::body::to_bytes(req.into_body(), 1024 * 16).await {
        Ok(b) => b,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid body"})),
            )
                .into_response()
        }
    };
    let parsed: federation::room::PinRoomMessageRequest = match serde_json::from_slice(&body) {
        Ok(p) => p,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid JSON"})),
            )
                .into_response()
        }
    };
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let user_id: i32 = claims.sub.parse().unwrap_or(0);
            match federation::room::pin_room_message(
                user_id,
                &claims.username,
                &room_id,
                &message_id,
                db,
                &parsed,
            )
            .await
            {
                Ok(result) => (StatusCode::OK, Json(result)).into_response(),
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

// ==================== Phase 5: Ring 去中心化环网 ====================

async fn federation_create_ring_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    if let Some(resp) = federation_admin_required(&claims).await {
        return resp;
    }
    let body = match axum::body::to_bytes(req.into_body(), 65536).await {
        Ok(b) => b,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid body"})),
            )
                .into_response()
        }
    };
    let create_req: federation::ring::CreateRingRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": format!("Invalid JSON: {}", e)})),
            )
                .into_response()
        }
    };
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let user_id: i32 = claims.sub.parse().unwrap_or(0);
            match federation::ring::create_ring(user_id, db, &create_req).await {
                Ok(ring) => (StatusCode::CREATED, Json(json!(ring))).into_response(),
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

async fn federation_list_rings_wrapper(_req: axum::extract::Request) -> Response {
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => match federation::ring::list_rings(db).await {
            Ok(rings) => (
                StatusCode::OK,
                Json(json!({"rings": rings, "total": rings.len()})),
            )
                .into_response(),
            Err((status, json)) => (status, json).into_response(),
        },
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

async fn federation_get_ring_wrapper(req: axum::extract::Request) -> Response {
    let path = req.uri().path().to_string();
    let ring_id = path
        .strip_prefix("/api/federation/rings/")
        .unwrap_or("")
        .split('/')
        .next()
        .unwrap_or("")
        .to_string();
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => match federation::ring::get_ring(&ring_id, db).await {
            Ok(ring) => (StatusCode::OK, Json(json!(ring))).into_response(),
            Err((status, json)) => (status, json).into_response(),
        },
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

async fn federation_leave_ring_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    if let Some(resp) = federation_admin_required(&claims).await {
        return resp;
    }
    let path = req.uri().path().to_string();
    let ring_id = path
        .strip_prefix("/api/federation/rings/")
        .unwrap_or("")
        .strip_suffix("/leave")
        .unwrap_or("")
        .to_string();
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => match federation::ring::leave_ring(&ring_id, &claims.username, db).await {
            Ok(v) => (StatusCode::OK, Json(v)).into_response(),
            Err((status, json)) => (status, json).into_response(),
        },
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

async fn federation_get_ring_peers_wrapper(req: axum::extract::Request) -> Response {
    let path = req.uri().path().to_string();
    let ring_id = path
        .strip_prefix("/api/federation/rings/")
        .unwrap_or("")
        .strip_suffix("/peers")
        .unwrap_or("")
        .to_string();
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => match federation::ring::get_peers(&ring_id, db).await {
            Ok(peers) => (
                StatusCode::OK,
                Json(json!({"peers": peers, "total": peers.len()})),
            )
                .into_response(),
            Err((status, json)) => (status, json).into_response(),
        },
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

async fn federation_add_ring_peer_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    if let Some(resp) = federation_admin_required(&claims).await {
        return resp;
    }
    let path = req.uri().path().to_string();
    let ring_id = path
        .strip_prefix("/api/federation/rings/")
        .unwrap_or("")
        .strip_suffix("/peers")
        .unwrap_or("")
        .to_string();
    let body = match axum::body::to_bytes(req.into_body(), 65536).await {
        Ok(b) => b,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid body"})),
            )
                .into_response()
        }
    };
    let add_req: federation::ring::AddPeerRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": format!("Invalid JSON: {}", e)})),
            )
                .into_response()
        }
    };
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            match federation::ring::add_peer(&ring_id, &claims.username, db, &add_req).await {
                Ok(v) => (StatusCode::OK, Json(v)).into_response(),
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

async fn federation_remove_ring_peer_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    if let Some(resp) = federation_admin_required(&claims).await {
        return resp;
    }
    let path = req.uri().path().to_string();
    let rest = path.strip_prefix("/api/federation/rings/").unwrap_or("");
    let parts: Vec<&str> = rest.splitn(3, '/').collect();
    let ring_id = parts.first().unwrap_or(&"").to_string();
    let peer_encoded = parts.get(2).unwrap_or(&"").to_string();
    let peer_url = urlencoding::decode(&peer_encoded)
        .unwrap_or_default()
        .to_string();
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => match federation::ring::remove_peer(
            &ring_id,
            &peer_url,
            &claims.username,
            db,
        )
        .await
        {
            Ok(v) => (StatusCode::OK, Json(v)).into_response(),
            Err((status, json)) => (status, json).into_response(),
        },
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

async fn federation_trigger_ring_sync_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    if let Some(resp) = federation_admin_required(&claims).await {
        return resp;
    }
    let path = req.uri().path().to_string();
    let ring_id = path
        .strip_prefix("/api/federation/rings/")
        .unwrap_or("")
        .strip_suffix("/sync")
        .unwrap_or("")
        .to_string();
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => match federation::ring::trigger_sync(&ring_id, &claims.username, db).await {
            Ok(v) => (StatusCode::OK, Json(v)).into_response(),
            Err((status, json)) => (status, json).into_response(),
        },
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

// ==================== Phase 5 补全: Trust 策略管理 ====================

/// GET /api/federation/delivery/stats — user delivery queue counters
async fn federation_delivery_stats_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => match federation::delivery::delivery_stats_for_user(db, user_id).await {
            Ok(v) => (StatusCode::OK, Json(v)).into_response(),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e})),
            )
                .into_response(),
        },
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

/// POST /api/federation/delivery/{id}/retry — requeue a dead/stuck item
async fn federation_retry_delivery_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    let path = req.uri().path().to_string();
    // /api/federation/delivery/{id}/retry
    let id_str = path
        .strip_prefix("/api/federation/delivery/")
        .unwrap_or("")
        .strip_suffix("/retry")
        .unwrap_or("");
    let queue_id: i32 = match id_str.parse() {
        Ok(i) => i,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid delivery id"})),
            )
                .into_response()
        }
    };
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => match federation::delivery::retry_delivery_item(db, user_id, queue_id).await {
            Ok(v) => (StatusCode::OK, Json(v)).into_response(),
            Err((status, v)) => (status, Json(v)).into_response(),
        },
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

/// POST /api/federation/delivery/{id}/cancel — cancel pending/delivering item
async fn federation_cancel_delivery_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    let path = req.uri().path().to_string();
    let id_str = path
        .strip_prefix("/api/federation/delivery/")
        .unwrap_or("")
        .strip_suffix("/cancel")
        .unwrap_or("");
    let queue_id: i32 = match id_str.parse() {
        Ok(i) => i,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid delivery id"})),
            )
                .into_response()
        }
    };
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => match federation::delivery::cancel_delivery_item(db, user_id, queue_id).await {
            Ok(v) => (StatusCode::OK, Json(v)).into_response(),
            Err((status, v)) => (status, Json(v)).into_response(),
        },
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

/// POST /api/federation/delivery/retry-dead — requeue all dead items for user
async fn federation_retry_all_dead_delivery_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    let limit = req
        .uri()
        .query()
        .and_then(|q| {
            q.split('&')
                .find_map(|p| p.strip_prefix("limit=").and_then(|v| v.parse::<i64>().ok()))
        })
        .unwrap_or(50);
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => match federation::delivery::retry_all_dead_for_user(db, user_id, limit).await
        {
            Ok(v) => (StatusCode::OK, Json(v)).into_response(),
            Err((status, v)) => (status, Json(v)).into_response(),
        },
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

/// POST /api/federation/delivery/cancel-pending — cancel all pending/delivering items for user
async fn federation_cancel_all_pending_delivery_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    let limit = req
        .uri()
        .query()
        .and_then(|q| {
            q.split('&')
                .find_map(|p| p.strip_prefix("limit=").and_then(|v| v.parse::<i64>().ok()))
        })
        .unwrap_or(100);
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            match federation::delivery::cancel_all_pending_for_user(db, user_id, limit).await {
                Ok(v) => (StatusCode::OK, Json(v)).into_response(),
                Err((status, v)) => (status, Json(v)).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

/// POST /api/federation/rooms/{room_id}/join — self-join open rooms
async fn federation_join_room_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let path = req.uri().path().to_string();
    let room_id = path
        .strip_prefix("/api/federation/rooms/")
        .unwrap_or("")
        .strip_suffix("/join")
        .unwrap_or("")
        .to_string();
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let user_id: i32 = claims.sub.parse().unwrap_or(0);
            match federation::room::join_room(user_id, &claims.username, &room_id, db).await {
                Ok(result) => (StatusCode::OK, Json(result)).into_response(),
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

/// GET /api/federation/delivery?limit= — recent queue rows (dead first)
async fn federation_list_delivery_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let user_id: i32 = claims.sub.parse().unwrap_or(0);
    let query_str = req.uri().query().unwrap_or("");
    let params: std::collections::HashMap<String, String> =
        url::form_urlencoded::parse(query_str.as_bytes())
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
    let limit = params
        .get("limit")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(30);
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => match federation::delivery::list_delivery_for_user(db, user_id, limit).await {
            Ok(v) => (StatusCode::OK, Json(v)).into_response(),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e})),
            )
                .into_response(),
        },
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

async fn federation_get_trust_policy_wrapper(_req: axum::extract::Request) -> Response {
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => match federation::trust::get_policy(db).await {
            Ok(v) => (StatusCode::OK, Json(v)).into_response(),
            Err((status, v)) => (status, Json(v)).into_response(),
        },
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

/// PUT /api/federation/trust/policy — update allowlist / min_trust / auto_discover (admin)
async fn federation_update_trust_policy_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    if let Some(resp) = federation_admin_required(&claims).await {
        return resp;
    }
    let body = match axum::body::to_bytes(req.into_body(), 65536).await {
        Ok(b) => b,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid body"})),
            )
                .into_response()
        }
    };
    let payload: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid JSON"})),
            )
                .into_response()
        }
    };
    let min_trust = payload
        .get("min_trust_level")
        .and_then(|v| v.as_i64())
        .map(|n| n as i16);
    let allowed_domains = payload.get("allowed_domains").and_then(|v| {
        v.as_array().map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect::<Vec<_>>()
        })
    });
    let auto_discover = payload.get("auto_discover").and_then(|v| v.as_bool());
    // Prefer nested rate_limit { max_requests_per_window, window_seconds, trusted_multiplier }
    // with flat keys as fallback for older clients.
    let rate_obj = payload.get("rate_limit");
    let rate_max = rate_obj
        .and_then(|r| r.get("max_requests_per_window"))
        .or_else(|| payload.get("rate_max_requests"))
        .and_then(|v| v.as_i64());
    let rate_window = rate_obj
        .and_then(|r| r.get("window_seconds"))
        .or_else(|| payload.get("rate_window_seconds"))
        .and_then(|v| v.as_i64());
    let rate_mul = rate_obj
        .and_then(|r| r.get("trusted_multiplier"))
        .or_else(|| payload.get("rate_trusted_multiplier"))
        .and_then(|v| v.as_i64());
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            match federation::trust::update_policy(
                db,
                min_trust,
                allowed_domains,
                auto_discover,
                rate_max,
                rate_window,
                rate_mul,
            )
                .await
            {
                Ok(v) => (StatusCode::OK, Json(v)).into_response(),
                Err((status, v)) => (status, Json(v)).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

async fn federation_list_instances_wrapper(_req: axum::extract::Request) -> Response {
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => match federation::trust::list_instances(db).await {
            Ok(v) => (StatusCode::OK, Json(v)).into_response(),
            Err((status, v)) => (status, Json(v)).into_response(),
        },
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

async fn federation_update_instance_trust_wrapper(req: axum::extract::Request) -> Response {
    let body = match axum::body::to_bytes(req.into_body(), 65536).await {
        Ok(b) => b,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid body"})),
            )
                .into_response()
        }
    };
    let payload: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid JSON"})),
            )
                .into_response()
        }
    };
    let domain = match payload.get("domain").and_then(|v| v.as_str()) {
        Some(d) => d.to_string(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "domain required"})),
            )
                .into_response()
        }
    };
    let level = payload
        .get("trust_level")
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as i16;
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => match federation::trust::update_instance_trust(db, &domain, level).await {
            Ok(v) => (StatusCode::OK, Json(v)).into_response(),
            Err((status, v)) => (status, Json(v)).into_response(),
        },
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

async fn federation_list_content_filters_wrapper(_req: axum::extract::Request) -> Response {
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => match federation::trust::list_content_filters(db).await {
            Ok(v) => (StatusCode::OK, Json(v)).into_response(),
            Err((status, v)) => (status, Json(v)).into_response(),
        },
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

async fn federation_create_content_filter_wrapper(req: axum::extract::Request) -> Response {
    let body = match axum::body::to_bytes(req.into_body(), 65536).await {
        Ok(b) => b,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid body"})),
            )
                .into_response()
        }
    };
    let payload: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid JSON"})),
            )
                .into_response()
        }
    };
    let name = payload
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let filter_type = payload
        .get("filter_type")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let value = payload
        .get("value")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let enabled = payload
        .get("enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            match federation::trust::create_content_filter(db, &name, &filter_type, &value, enabled)
                .await
            {
                Ok(v) => (StatusCode::OK, Json(v)).into_response(),
                Err((status, v)) => (status, Json(v)).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

async fn federation_update_content_filter_wrapper(req: axum::extract::Request) -> Response {
    let path = req.uri().path().to_string();
    let id_str = path
        .strip_prefix("/api/federation/trust/filters/")
        .unwrap_or("")
        .to_string();
    let id: i32 = match id_str.parse() {
        Ok(i) => i,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid filter id"})),
            )
                .into_response()
        }
    };
    let body = match axum::body::to_bytes(req.into_body(), 65536).await {
        Ok(b) => b,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid body"})),
            )
                .into_response()
        }
    };
    let payload: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid JSON"})),
            )
                .into_response()
        }
    };
    let name = payload.get("name").and_then(|v| v.as_str());
    let filter_type = payload.get("filter_type").and_then(|v| v.as_str());
    let value = payload.get("value").and_then(|v| v.as_str());
    let enabled = payload.get("enabled").and_then(|v| v.as_bool());
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            match federation::trust::update_content_filter(
                db,
                id,
                name,
                filter_type,
                value,
                enabled,
            )
            .await
            {
                Ok(v) => (StatusCode::OK, Json(v)).into_response(),
                Err((status, v)) => (status, Json(v)).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

async fn federation_delete_content_filter_wrapper(req: axum::extract::Request) -> Response {
    let path = req.uri().path().to_string();
    let id_str = path
        .strip_prefix("/api/federation/trust/filters/")
        .unwrap_or("")
        .to_string();
    let id: i32 = match id_str.parse() {
        Ok(i) => i,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid filter id"})),
            )
                .into_response()
        }
    };
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => match federation::trust::delete_content_filter(db, id).await {
            Ok(v) => (StatusCode::OK, Json(v)).into_response(),
            Err((status, v)) => (status, Json(v)).into_response(),
        },
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

async fn federation_toggle_instance_block_wrapper(req: axum::extract::Request) -> Response {
    let body = match axum::body::to_bytes(req.into_body(), 65536).await {
        Ok(b) => b,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid body"})),
            )
                .into_response()
        }
    };
    let payload: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid JSON"})),
            )
                .into_response()
        }
    };
    let domain = match payload.get("domain").and_then(|v| v.as_str()) {
        Some(d) => d.to_string(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "domain required"})),
            )
                .into_response()
        }
    };
    let block = payload
        .get("block")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => match federation::trust::toggle_instance_block(db, &domain, block).await {
            Ok(v) => (StatusCode::OK, Json(v)).into_response(),
            Err((status, v)) => (status, Json(v)).into_response(),
        },
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

// ==================== Phase 5 补全: 文件传输 ====================

async fn federation_initiate_transfer_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let path = req.uri().path().to_string();
    let channel_id = path
        .strip_prefix("/api/federation/channels/")
        .unwrap_or("")
        .strip_suffix("/transfers")
        .unwrap_or("")
        .to_string();
    let body = match axum::body::to_bytes(req.into_body(), 65536).await {
        Ok(b) => b,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid body"})),
            )
                .into_response()
        }
    };
    let transfer_req: federation::file_transfer::InitTransferRequest =
        match serde_json::from_slice(&body) {
            Ok(r) => r,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": format!("Invalid JSON: {}", e)})),
                )
                    .into_response()
            }
        };
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let user_id: i32 = claims.sub.parse().unwrap_or(0);
            match federation::file_transfer::initiate_transfer(
                user_id,
                &claims.username,
                &channel_id,
                db,
                &transfer_req,
            )
            .await
            {
                Ok(t) => (StatusCode::CREATED, Json(json!(t))).into_response(),
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

async fn federation_list_transfers_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let path = req.uri().path().to_string();
    let channel_id = path
        .strip_prefix("/api/federation/channels/")
        .unwrap_or("")
        .strip_suffix("/transfers")
        .unwrap_or("")
        .to_string();
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => match federation::file_transfer::list_transfers(
            &channel_id,
            claims.sub.parse().unwrap_or(0),
            db,
        )
        .await
        {
            Ok(transfers) => (
                StatusCode::OK,
                Json(json!({"transfers": transfers, "total": transfers.len()})),
            )
                .into_response(),
            Err((status, json)) => (status, json).into_response(),
        },
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

async fn federation_initiate_room_transfer_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let path = req.uri().path().to_string();
    let room_id = path
        .strip_prefix("/api/federation/rooms/")
        .unwrap_or("")
        .strip_suffix("/transfers")
        .unwrap_or("")
        .to_string();
    let body = match axum::body::to_bytes(req.into_body(), 65536).await {
        Ok(b) => b,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid body"})),
            )
                .into_response()
        }
    };
    let transfer_req: federation::file_transfer::InitTransferRequest =
        match serde_json::from_slice(&body) {
            Ok(r) => r,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": format!("Invalid JSON: {}", e)})),
                )
                    .into_response()
            }
        };
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let user_id: i32 = claims.sub.parse().unwrap_or(0);
            match federation::file_transfer::initiate_room_transfer(
                user_id,
                &claims.username,
                &room_id,
                db,
                &transfer_req,
            )
            .await
            {
                Ok(t) => (StatusCode::CREATED, Json(json!(t))).into_response(),
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

async fn federation_list_room_transfers_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let path = req.uri().path().to_string();
    let room_id = path
        .strip_prefix("/api/federation/rooms/")
        .unwrap_or("")
        .strip_suffix("/transfers")
        .unwrap_or("")
        .to_string();
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => match federation::file_transfer::list_room_transfers(
            &room_id,
            claims.sub.parse().unwrap_or(0),
            &claims.username,
            db,
        )
        .await
        {
            Ok(transfers) => (
                StatusCode::OK,
                Json(json!({"transfers": transfers, "total": transfers.len()})),
            )
                .into_response(),
            Err((status, json)) => (status, json).into_response(),
        },
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

/// GET /api/federation/rooms/{room_id}/files — group attachment library index
async fn federation_list_room_files_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let uri = req.uri().clone();
    let path = uri.path();
    let room_id = path
        .strip_prefix("/api/federation/rooms/")
        .unwrap_or("")
        .strip_suffix("/files")
        .unwrap_or("")
        .to_string();
    let query_str = uri.query().unwrap_or("");
    let params: std::collections::HashMap<String, String> =
        url::form_urlencoded::parse(query_str.as_bytes())
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
    let before = params.get("before").map(|s| s.as_str());
    let limit = params.get("limit").and_then(|s| s.parse::<i64>().ok());
    let filter = params.get("filter").map(|s| s.as_str());
    let q = params.get("q").map(|s| s.as_str());
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let user_id: i32 = claims.sub.parse().unwrap_or(0);
            match federation::room::list_room_files(
                user_id,
                &claims.username,
                &room_id,
                db,
                before,
                limit,
                filter,
                q,
            )
            .await
            {
                Ok(result) => (StatusCode::OK, Json(json!(result))).into_response(),
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

async fn federation_get_transfer_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let path = req.uri().path().to_string();
    let transfer_id = path
        .strip_prefix("/api/federation/transfers/")
        .unwrap_or("")
        .split('/')
        .next()
        .unwrap_or("")
        .to_string();
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => match federation::file_transfer::get_transfer(
            &transfer_id,
            claims.sub.parse().unwrap_or(0),
            &claims.username,
            db,
        )
        .await
        {
            Ok(t) => (StatusCode::OK, Json(json!(t))).into_response(),
            Err((status, json)) => (status, json).into_response(),
        },
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

/// Stream completed transfer bytes for browser / Tapp host download.
async fn federation_download_transfer_wrapper(req: axum::extract::Request) -> Response {
    use axum::body::Body;
    use axum::http::header::{CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE, HeaderValue};
    use tokio::io::AsyncReadExt;
    use tokio_stream::wrappers::ReceiverStream;

    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let path = req.uri().path().to_string();
    // /api/federation/transfers/{id}/content
    let transfer_id = path
        .strip_prefix("/api/federation/transfers/")
        .unwrap_or("")
        .strip_suffix("/content")
        .unwrap_or("")
        .to_string();
    if transfer_id.is_empty() || transfer_id.contains('/') {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Invalid transfer id"})),
        )
            .into_response();
    }

    let db_opt = DB_CONNECTION.read().await;
    let file = match db_opt.as_ref() {
        Some(db) => {
            match federation::file_transfer::open_transfer_file(
                &transfer_id,
                claims.sub.parse().unwrap_or(0),
                &claims.username,
                db,
            )
            .await
            {
                Ok(f) => f,
                Err((status, json)) => return (status, json).into_response(),
            }
        }
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error": "Database not connected"})),
            )
                .into_response()
        }
    };

    // RFC 5987 filename* for non-ASCII; ASCII fallback for legacy clients.
    let ascii_name: String = file
        .filename
        .chars()
        .map(|c| if c.is_ascii() && c != '"' && c != '\\' { c } else { '_' })
        .collect();
    let ascii_name = if ascii_name.trim_matches('_').is_empty() {
        "download".to_string()
    } else {
        ascii_name
    };
    let encoded = urlencoding::encode(&file.filename);
    let disposition = format!(
        "attachment; filename=\"{}\"; filename*=UTF-8''{}",
        ascii_name, encoded
    );

    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Vec<u8>, std::io::Error>>(4);
    let disk_path = file.path.clone();
    tokio::spawn(async move {
        let mut f = match tokio::fs::File::open(&disk_path).await {
            Ok(f) => f,
            Err(e) => {
                let _ = tx.send(Err(e)).await;
                return;
            }
        };
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            match f.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => {
                    if tx.send(Ok(buf[..n].to_vec())).await.is_err() {
                        break;
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(e)).await;
                    break;
                }
            }
        }
    });

    let body = Body::from_stream(ReceiverStream::new(rx));
    let mut res = Response::new(body);
    *res.status_mut() = StatusCode::OK;
    let headers = res.headers_mut();
    if let Ok(v) = HeaderValue::from_str(&file.mime_type) {
        headers.insert(CONTENT_TYPE, v);
    } else {
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/octet-stream"),
        );
    }
    if let Ok(v) = HeaderValue::from_str(&disposition) {
        headers.insert(CONTENT_DISPOSITION, v);
    }
    if let Ok(v) = HeaderValue::from_str(&file.file_size.to_string()) {
        headers.insert(CONTENT_LENGTH, v);
    }
    res
}

async fn federation_upload_chunk_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let path = req.uri().path().to_string();
    let transfer_id = path
        .strip_prefix("/api/federation/transfers/")
        .unwrap_or("")
        .strip_suffix("/chunks")
        .unwrap_or("")
        .to_string();
    // Chunk payload is base64 of up to 1 MiB raw (~1.37 MiB) plus JSON keys.
    // 1 MiB cap was truncating real uploads and broke file-meta entirely.
    let body = match axum::body::to_bytes(req.into_body(), 3 * 1024 * 1024).await {
        Ok(b) => b,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid body or chunk too large"})),
            )
                .into_response()
        }
    };
    let chunk_req: federation::file_transfer::UploadChunkRequest =
        match serde_json::from_slice(&body) {
            Ok(r) => r,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": format!("Invalid JSON: {}", e)})),
                )
                    .into_response()
            }
        };
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let user_id: i32 = claims.sub.parse().unwrap_or(0);
            match federation::file_transfer::upload_chunk(
                user_id,
                &claims.username,
                &transfer_id,
                db,
                &chunk_req,
            )
            .await
            {
                Ok(v) => (StatusCode::OK, Json(v)).into_response(),
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

async fn federation_cancel_transfer_wrapper(req: axum::extract::Request) -> Response {
    let claims = match req.extensions().get::<middleware::auth::Claims>().cloned() {
        Some(c) => c,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Not authenticated"})),
            )
                .into_response()
        }
    };
    let path = req.uri().path().to_string();
    let transfer_id = path
        .strip_prefix("/api/federation/transfers/")
        .unwrap_or("")
        .strip_suffix("/cancel")
        .unwrap_or("")
        .to_string();
    let db_opt = DB_CONNECTION.read().await;
    match db_opt.as_ref() {
        Some(db) => {
            let user_id: i32 = claims.sub.parse().unwrap_or(0);
            match federation::file_transfer::cancel_transfer(
                user_id,
                &claims.username,
                &transfer_id,
                db,
            )
            .await
            {
                Ok(v) => (StatusCode::OK, Json(v)).into_response(),
                Err((status, json)) => (status, json).into_response(),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Database not connected"})),
        )
            .into_response(),
    }
}

async fn get_follow_list(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    direction: &str,
) -> Result<serde_json::Value, String> {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT ra.actor_url, ra.username, ra.domain, ra.display_name,
                      ra.avatar_url, f.status, f.created_at
               FROM federation_follows f
               JOIN federation_remote_actors ra ON ra.id = f.remote_actor_id
               WHERE f.user_id = $1 AND f.direction = $2
               ORDER BY f.created_at DESC"#,
            [user_id.into(), direction.into()],
        ))
        .await
        .map_err(|e| format!("DB error: {}", e))?;

    let list: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            json!({
                "actor_url": r.try_get::<String>("", "actor_url").unwrap_or_default(),
                // Nullable columns must use Option — try_get::<String> fails on NULL
                "username": r.try_get::<Option<String>>("", "username").ok().flatten(),
                "domain": r.try_get::<String>("", "domain").unwrap_or_default(),
                "display_name": r.try_get::<Option<String>>("", "display_name").ok().flatten(),
                "avatar_url": r.try_get::<Option<String>>("", "avatar_url").ok().flatten(),
                "status": r.try_get::<String>("", "status").unwrap_or_default(),
            })
        })
        .collect();

    Ok(json!({"items": list, "total": list.len()}))
}

/// 查询联邦时间线
async fn get_federation_timeline(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
) -> Result<serde_json::Value, String> {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    let base_url = federation::types::get_base_url().await;
    let base = base_url.trim_end_matches('/');
    let local_domain = federation::types::extract_domain(&base_url).unwrap_or_default();
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT t.activity_id, t.activity_type, t.object_type,
                      t.content_preview, t.content_json, t.is_read, t.received_at,
                      ra.actor_url AS remote_actor_url,
                      ra.username AS remote_username,
                      ra.domain AS remote_domain,
                      ra.display_name AS remote_display_name,
                      ra.avatar_url AS remote_avatar_url,
                      author.username AS author_username,
                      author.display_name AS author_display_name,
                      peer.username AS peer_username,
                      peer.display_name AS peer_display_name,
                      CASE
                          WHEN ra.id IS NOT NULL THEN
                              CASE
                                  WHEN peer.username IS NOT NULL
                                       AND (
                                           (peer.avatar_url IS NOT NULL AND peer.avatar_url <> ''
                                            AND peer.avatar_url NOT LIKE 'https://ui-avatars.com/%'
                                            AND peer.avatar_url NOT LIKE 'http://ui-avatars.com/%')
                                           OR EXISTS (
                                               SELECT 1 FROM user_identities ui
                                               WHERE ui.user_id = peer.id
                                                 AND ui.avatar_url IS NOT NULL AND ui.avatar_url <> ''
                                           )
                                       )
                                  THEN $2 || '/users/' || peer.username || '/avatar'
                                  ELSE NULL
                              END
                          ELSE
                              CASE
                                  WHEN author.username IS NOT NULL
                                       AND (
                                           (author.avatar_url IS NOT NULL AND author.avatar_url <> ''
                                            AND author.avatar_url NOT LIKE 'https://ui-avatars.com/%'
                                            AND author.avatar_url NOT LIKE 'http://ui-avatars.com/%')
                                           OR EXISTS (
                                               SELECT 1 FROM user_identities ui
                                               WHERE ui.user_id = author.id
                                                 AND ui.avatar_url IS NOT NULL AND ui.avatar_url <> ''
                                           )
                                       )
                                  THEN $2 || '/users/' || author.username || '/avatar'
                                  ELSE NULL
                              END
                      END AS local_avatar_proxy
               FROM federation_timeline t
               LEFT JOIN federation_remote_actors ra ON ra.id = t.remote_actor_id
               -- Self-authored rows only — do NOT join viewer profile onto remote posts.
               LEFT JOIN users author ON ra.id IS NULL AND author.id = t.user_id
               -- Same-instance remote_actor stubs → local user profile enrichment.
               LEFT JOIN users peer ON ra.id IS NOT NULL
                   AND ra.username IS NOT NULL
                   AND peer.username = ra.username
                   AND (
                       ra.actor_url LIKE ($2 || '/users/%')
                       OR ra.domain = $3
                   )
               WHERE t.user_id = $1
                 AND (t.activity_type IS NULL OR t.activity_type <> 'Like')
               ORDER BY t.received_at DESC
               LIMIT 50"#,
            [user_id.into(), base.into(), local_domain.clone().into()],
        ))
        .await
        .map_err(|e| format!("DB error: {}", e))?;

    let mut items: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            let received_at = r
                .try_get::<chrono::DateTime<chrono::FixedOffset>>("", "received_at")
                .ok()
                .map(|t| t.to_rfc3339());
            let remote_actor_url = r
                .try_get::<Option<String>>("", "remote_actor_url")
                .ok()
                .flatten()
                .filter(|s| !s.is_empty());
            let content_json = r
                .try_get::<Option<serde_json::Value>>("", "content_json")
                .ok()
                .flatten();
            let local_avatar_proxy = r
                .try_get::<Option<String>>("", "local_avatar_proxy")
                .ok()
                .flatten()
                .filter(|s| !s.is_empty());
            // Prefer the post author's remote_actor; never the viewer's profile.
            let actor = if let Some(url) = remote_actor_url {
                let remote_display = r
                    .try_get::<Option<String>>("", "remote_display_name")
                    .ok()
                    .flatten()
                    .filter(|s| !s.is_empty());
                let peer_display = r
                    .try_get::<Option<String>>("", "peer_display_name")
                    .ok()
                    .flatten()
                    .filter(|s| !s.is_empty());
                let remote_username = r
                    .try_get::<Option<String>>("", "remote_username")
                    .ok()
                    .flatten();
                let peer_username = r
                    .try_get::<Option<String>>("", "peer_username")
                    .ok()
                    .flatten();
                let remote_avatar = r
                    .try_get::<Option<String>>("", "remote_avatar_url")
                    .ok()
                    .flatten()
                    .filter(|s| !s.is_empty());
                json!({
                    "actor_url": url,
                    "username": remote_username.clone().or(peer_username.clone()),
                    "domain": r.try_get::<Option<String>>("", "remote_domain").ok().flatten(),
                    "display_name": remote_display
                        .or(peer_display)
                        .or(remote_username)
                        .or(peer_username),
                    "avatar_url": remote_avatar.or(local_avatar_proxy),
                })
            } else {
                let local_username = r
                    .try_get::<Option<String>>("", "author_username")
                    .ok()
                    .flatten()
                    .unwrap_or_default();
                let local_display = r
                    .try_get::<Option<String>>("", "author_display_name")
                    .ok()
                    .flatten()
                    .filter(|s| !s.is_empty());
                let actor_url = if local_username.is_empty() {
                    String::new()
                } else {
                    federation::types::actor_url(&base_url, &local_username)
                };
                let domain = federation::types::extract_domain(&base_url);
                json!({
                    "actor_url": actor_url,
                    "username": local_username,
                    "domain": domain,
                    "display_name": local_display,
                    "avatar_url": local_avatar_proxy,
                    "is_local": true,
                })
            };
            let object_id = content_json
                .as_ref()
                .and_then(federation::interactions::extract_object_id);
            json!({
                "activity_id": r.try_get::<String>("", "activity_id").unwrap_or_default(),
                "activity_type": r.try_get::<Option<String>>("", "activity_type").ok().flatten(),
                "object_type": r.try_get::<Option<String>>("", "object_type").ok().flatten(),
                "content_preview": r.try_get::<Option<String>>("", "content_preview").ok().flatten(),
                "content_json": content_json,
                "object_id": object_id,
                "is_read": r.try_get::<bool>("", "is_read").unwrap_or(false),
                // Frontend (Aro) expects created_at / timestamp for timeAgo()
                "created_at": received_at.clone(),
                "received_at": received_at,
                "actor": actor,
            })
        })
        .collect();

    // Enrich like/bookmark/announce/reply counts and me-flags
    let object_ids: Vec<String> = items
        .iter()
        .filter_map(|it| {
            it.get("object_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .collect();
    if let Ok(stats_map) =
        federation::interactions::interaction_stats_for_objects(db, user_id, &object_ids).await
    {
        for item in &mut items {
            if let Some(oid) = item
                .get("object_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
            {
                if let Some(st) = stats_map.get(&oid) {
                    if let Some(obj) = item.as_object_mut() {
                        obj.insert("liked_by_me".into(), json!(st.liked_by_me));
                        obj.insert("bookmarked_by_me".into(), json!(st.bookmarked_by_me));
                        obj.insert("announced_by_me".into(), json!(st.announced_by_me));
                        obj.insert("like_count".into(), json!(st.like_count));
                        obj.insert("bookmark_count".into(), json!(st.bookmark_count));
                        obj.insert("announce_count".into(), json!(st.announce_count));
                        obj.insert("reply_count".into(), json!(st.reply_count));
                        obj.insert("is_bookmarked".into(), json!(st.bookmarked_by_me));
                    }
                }
            }
        }
    }

    Ok(json!({"items": items, "total": items.len()}))
}

/// Start unified server with all routes (middleware controls access based on mode)
/// Authenticated `/api/federation/*` API surface.
///
/// Tapp host attribution and authentication are applied once at router level
/// instead of per route: `auth_middleware` runs first (outermost), then
/// `federation_host_attribution`, so grant-bearing requests are enforced on
/// every federation route — including ones added later — instead of silently
/// executing with host identity when a per-route layer is forgotten
/// (fail-closed). Admin-only trust management keeps its extra admin gate.
fn federation_api_router() -> Router {
    let main_router = Router::new()
        .route("/api/federation/identity", get(federation_identity_wrapper))
        .route("/api/federation/follow", post(federation_follow_wrapper))
        .route("/api/federation/unfollow", post(federation_unfollow_wrapper))
        .route(
            "/api/federation/following",
            get(federation_following_list_wrapper),
        )
        .route(
            "/api/federation/followers",
            get(federation_followers_list_wrapper),
        )
        .route("/api/federation/timeline", get(federation_timeline_wrapper))
        .route("/api/federation/publish", post(federation_publish_wrapper))
        .route("/api/federation/notes", post(federation_create_note_wrapper))
        .route("/api/federation/like", post(federation_like_wrapper))
        .route("/api/federation/unlike", post(federation_unlike_wrapper))
        .route("/api/federation/bookmark", post(federation_bookmark_wrapper))
        .route(
            "/api/federation/unbookmark",
            post(federation_unbookmark_wrapper),
        )
        .route(
            "/api/federation/bookmarks",
            get(federation_bookmarks_list_wrapper),
        )
        .route("/api/federation/announce", post(federation_announce_wrapper))
        .route(
            "/api/federation/unannounce",
            post(federation_unannounce_wrapper),
        )
        .route(
            "/api/federation/unpublish",
            post(federation_unpublish_wrapper),
        )
        .route(
            "/api/federation/published",
            get(federation_published_list_wrapper),
        )
        .route(
            "/api/federation/channels",
            get(federation_list_channels_wrapper).post(federation_create_channel_wrapper),
        )
        .route(
            "/api/federation/channels/{channel_id}",
            get(federation_get_channel_wrapper).delete(federation_delete_channel_wrapper),
        )
        .route(
            "/api/federation/channels/{channel_id}/close",
            post(federation_close_channel_wrapper),
        )
        .route(
            "/api/federation/channels/{channel_id}/accept",
            post(federation_accept_channel_wrapper),
        )
        .route(
            "/api/federation/channels/{channel_id}/e2e/key-exchange",
            post(federation_e2e_key_exchange_wrapper),
        )
        .route(
            "/api/federation/channels/{channel_id}/messages",
            get(federation_get_messages_wrapper).post(federation_send_message_wrapper),
        )
        .route(
            "/api/federation/channels/{channel_id}/ws-ticket",
            post(api::tapp_runtime::mint_channel_ws_ticket),
        )
        .route(
            "/api/federation/channels/{channel_id}/ws",
            get(federation::ws_gateway::channel_websocket),
        )
        .route(
            "/api/federation/rooms",
            get(federation_list_rooms_wrapper).post(federation_create_room_wrapper),
        )
        .route(
            "/api/federation/rooms/{room_id}",
            get(federation_get_room_wrapper)
                .put(federation_update_room_wrapper)
                .delete(federation_delete_room_wrapper),
        )
        .route(
            "/api/federation/rooms/{room_id}/members",
            get(federation_get_room_members_wrapper),
        )
        .route(
            "/api/federation/rooms/{room_id}/invite",
            post(federation_invite_room_member_wrapper),
        )
        .route(
            "/api/federation/rooms/{room_id}/accept",
            post(federation_accept_room_invite_wrapper),
        )
        .route(
            "/api/federation/rooms/{room_id}/reject",
            post(federation_reject_room_invite_wrapper),
        )
        .route(
            "/api/federation/rooms/{room_id}/join",
            post(federation_join_room_wrapper),
        )
        .route(
            "/api/federation/rooms/{room_id}/members/{actor}",
            delete(federation_remove_room_member_wrapper),
        )
        .route(
            "/api/federation/rooms/{room_id}/leave",
            post(federation_leave_room_wrapper),
        )
        .route(
            "/api/federation/rooms/{room_id}/transfer-ownership",
            post(federation_transfer_room_ownership_wrapper),
        )
        .route(
            "/api/federation/rooms/{room_id}/messages",
            get(federation_get_room_messages_wrapper).post(federation_send_room_message_wrapper),
        )
        .route(
            "/api/federation/rooms/{room_id}/e2e/key-exchange",
            post(federation_room_e2e_key_exchange_wrapper),
        )
        .route(
            "/api/federation/rooms/{room_id}/messages/{message_id}/pin",
            post(federation_pin_room_message_wrapper),
        )
        .route(
            "/api/federation/rooms/{room_id}/ws-ticket",
            post(api::tapp_runtime::mint_room_ws_ticket),
        )
        .route(
            "/api/federation/rooms/{room_id}/ws",
            get(federation::ws_gateway::room_websocket),
        )
        .route(
            "/api/federation/rings",
            get(federation_list_rings_wrapper).post(federation_create_ring_wrapper),
        )
        .route(
            "/api/federation/rings/{ring_id}",
            get(federation_get_ring_wrapper),
        )
        .route(
            "/api/federation/rings/{ring_id}/leave",
            post(federation_leave_ring_wrapper),
        )
        .route(
            "/api/federation/rings/{ring_id}/peers",
            get(federation_get_ring_peers_wrapper).post(federation_add_ring_peer_wrapper),
        )
        .route(
            "/api/federation/rings/{ring_id}/peers/{peer}",
            delete(federation_remove_ring_peer_wrapper),
        )
        .route(
            "/api/federation/rings/{ring_id}/sync",
            post(federation_trigger_ring_sync_wrapper),
        )
        .route(
            "/api/federation/delivery/stats",
            get(federation_delivery_stats_wrapper),
        )
        .route(
            "/api/federation/delivery",
            get(federation_list_delivery_wrapper),
        )
        .route(
            "/api/federation/delivery/retry-dead",
            post(federation_retry_all_dead_delivery_wrapper),
        )
        .route(
            "/api/federation/delivery/cancel-pending",
            post(federation_cancel_all_pending_delivery_wrapper),
        )
        .route(
            "/api/federation/delivery/{id}/retry",
            post(federation_retry_delivery_wrapper),
        )
        .route(
            "/api/federation/delivery/{id}/cancel",
            post(federation_cancel_delivery_wrapper),
        )
        .route(
            "/api/federation/trust/policy",
            get(federation_get_trust_policy_wrapper).put(federation_update_trust_policy_wrapper),
        )
        .route(
            "/api/federation/trust/instances",
            get(federation_list_instances_wrapper),
        )
        .route(
            "/api/federation/trust/update",
            post(federation_update_instance_trust_wrapper)
                .route_layer(from_fn(middleware::auth::admin_middleware)),
        )
        .route(
            "/api/federation/trust/block",
            post(federation_toggle_instance_block_wrapper)
                .route_layer(from_fn(middleware::auth::admin_middleware)),
        )
        .route(
            "/api/federation/trust/filters",
            get(federation_list_content_filters_wrapper)
                .post(federation_create_content_filter_wrapper)
                .route_layer(from_fn(middleware::auth::admin_middleware)),
        )
        .route(
            "/api/federation/trust/filters/{id}",
            axum::routing::put(federation_update_content_filter_wrapper)
                .delete(federation_delete_content_filter_wrapper)
                .route_layer(from_fn(middleware::auth::admin_middleware)),
        )
        .route(
            "/api/federation/channels/{channel_id}/transfers",
            get(federation_list_transfers_wrapper).post(federation_initiate_transfer_wrapper),
        )
        .route(
            "/api/federation/rooms/{room_id}/transfers",
            get(federation_list_room_transfers_wrapper)
                .post(federation_initiate_room_transfer_wrapper),
        )
        .route(
            "/api/federation/rooms/{room_id}/files",
            get(federation_list_room_files_wrapper),
        )
        .route(
            "/api/federation/transfers/{transfer_id}",
            get(federation_get_transfer_wrapper),
        )
        .route(
            "/api/federation/transfers/{transfer_id}/content",
            get(federation_download_transfer_wrapper),
        )
        .route(
            "/api/federation/transfers/{transfer_id}/chunks",
            post(federation_upload_chunk_wrapper),
        )
        .route(
            "/api/federation/transfers/{transfer_id}/cancel",
            post(federation_cancel_transfer_wrapper),
        )
        // Federation control-plane writes are JSON (follow/channel/room/message)
        // plus base64 file-transfer chunks / optional Tapp package snapshots.
        // 40 MiB covers MAX_MESSAGE_PAYLOAD (32 MiB) with headroom and
        // DEFAULT_CHUNK_SIZE = 1 MiB raw (~1.37 MiB base64). Global 50MB stays
        // for media uploads outside this router — do not lower the global limit.
        .layer(axum::extract::DefaultBodyLimit::max(40 * 1024 * 1024))
        .route_layer(from_fn(api::tapp_runtime::federation_host_attribution))
        .route_layer(from_fn(middleware::auth::auth_middleware));

    // Freeform Note media: images ≤10MB, video ≤50MB — dedicated limit + same auth/attribution.
    let media_router = Router::new()
        .route(
            "/api/federation/media",
            post(federation_media_upload_wrapper),
        )
        .layer(axum::extract::DefaultBodyLimit::max(55 * 1024 * 1024))
        .route_layer(from_fn(api::tapp_runtime::federation_host_attribution))
        .route_layer(from_fn(middleware::auth::auth_middleware));

    main_router.merge(media_router)
}

async fn start_unified_server(config: AppConfig) -> anyhow::Result<()> {
    // Build CORS layer with security-first configuration
    use tower_http::cors::AllowOrigin;

    // Parse allowed origins from config
    let allowed_origins: Vec<axum::http::HeaderValue> = config
        .cors_origins
        .iter()
        .filter_map(|origin| origin.parse().ok())
        .collect();

    // Custom request headers used by the SPA must be listed for cross-origin preflight.
    let cors_allowed_headers = [
        axum::http::header::CONTENT_TYPE,
        axum::http::header::AUTHORIZATION,
        axum::http::header::ACCEPT,
        axum::http::header::HeaderName::from_static("x-csrf-token"),
        axum::http::header::HeaderName::from_static("x-tapp-runtime-grant"),
        axum::http::header::HeaderName::from_static("x-requested-with"),
    ];

    let cors = if allowed_origins.is_empty() {
        // Check if in production mode
        let is_production = std::env::var("ENVIRONMENT")
            .unwrap_or_else(|_| "development".to_string())
            == "production";

        if is_production {
            tracing::error!("🚨 SECURITY ERROR: CORS_ORIGINS must be configured in production!");
            tracing::error!("Set CORS_ORIGINS environment variable to your frontend domain(s)");
            tracing::error!(
                "Example: CORS_ORIGINS=https://yourdomain.com,https://www.yourdomain.com"
            );
            panic!("CORS_ORIGINS is required in production mode for security");
        }

        tracing::warn!("⚠️ No CORS origins configured, using localhost-only for development");
        // Development mode: restrict to localhost
        let dev_origins = vec![
            "http://localhost:1102"
                .parse::<axum::http::HeaderValue>()
                .unwrap(),
            "http://localhost:1103"
                .parse::<axum::http::HeaderValue>()
                .unwrap(),
            "http://127.0.0.1:1102"
                .parse::<axum::http::HeaderValue>()
                .unwrap(),
            "http://127.0.0.1:1103"
                .parse::<axum::http::HeaderValue>()
                .unwrap(),
        ];
        CorsLayer::new()
            .allow_origin(AllowOrigin::list(dev_origins))
            .allow_methods([
                axum::http::Method::GET,
                axum::http::Method::POST,
                axum::http::Method::PUT,
                axum::http::Method::DELETE,
                axum::http::Method::OPTIONS,
            ])
            .allow_headers(cors_allowed_headers)
            .allow_credentials(true)
    } else {
        tracing::info!("✅ CORS configured for origins: {:?}", config.cors_origins);
        CorsLayer::new()
            .allow_origin(AllowOrigin::list(allowed_origins))
            .allow_methods([
                axum::http::Method::GET,
                axum::http::Method::POST,
                axum::http::Method::PUT,
                axum::http::Method::DELETE,
                axum::http::Method::OPTIONS,
            ])
            .allow_headers(cors_allowed_headers)
            .allow_credentials(true)
    };

    // Get database connection (might be None in config mode)
    let db_opt = DB_CONNECTION.read().await.clone();

    // Build the unified API router. Core setup/auth/config routes are always
    // registered through wrappers. Larger DB route groups are added on full-mode
    // startup, so setup-mode database changes restart the process.
    let api_router = Router::new()
        .route("/health", get(api::health))
        // Setup routes (always available)
        .route("/api/setup/config", get(api::setup::get_setup_config))
        .route("/api/setup/status", get(check_setup_status_wrapper))
        .route("/api/setup/init-env", post(api::setup::initialize_env_file))
        .route("/api/setup/update-env", post(api::setup::update_env_file))
        .route(
            "/api/setup/database-config",
            post(api::setup::save_database_config),
        )
        .route("/api/setup/init-database", post(init_database_wrapper))
        .route("/api/setup/create-admin", post(create_admin_wrapper))
        // System management routes
        // ⚠️ P2: system/status 暴露了一些系统信息，但为了监控保持公开（考虑移除敏感字段）
        .route("/api/system/status", get(api::system::system_status))
        .route(
            "/api/system/reload-config",
            post(api::system::reload_config)
                // ✅ P1 修复：配置重载应该只有 admin 可以触发
                .route_layer(from_fn(middleware::auth::admin_middleware)),
        )
        // ✅ P2: 系统监控指标端点（内存、任务、连接等）- 🔒 需要管理员权限
        .route(
            "/api/metrics",
            get(api::metrics::get_metrics).route_layer(from_fn(middleware::auth::admin_middleware)),
        )
        // Authentication routes (use wrapper for dynamic DB access)
        .route("/api/auth/login", post(local_login_wrapper))
        .route("/api/auth/me", get(get_current_user_wrapper))
        .route(
            "/api/auth/logout",
            post(logout_wrapper), // 不需要认证中间件
        )
        // OAuth routes stay registered even if DB is temporarily unavailable,
        // keeping login/setup surfaces on 503 responses instead of 404s.
        .route("/api/auth/oauth/providers", get(api::oauth::list_providers))
        .route(
            "/api/auth/oauth/{slug}/login",
            get(api::oauth::provider_login),
        )
        .route(
            "/api/auth/oauth/{slug}/callback",
            get(oauth_provider_callback_wrapper),
        )
        .route(
            "/api/auth/oauth/{slug}/link",
            get(api::oauth::provider_link).route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        .route(
            "/api/auth/oauth/{slug}/unlink/{identity_id}",
            axum::routing::delete(oauth_provider_unlink_wrapper)
                .route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        .route(
            "/api/auth/identities",
            get(oauth_list_my_identities_wrapper)
                .route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        .route(
            "/api/auth/change-password",
            post(change_password_wrapper).route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        // 设置页用户管理（列表/创建/详情/更新/解绑 identity）— 仅管理员
        .route(
            "/api/admin/users",
            get(admin_list_users_wrapper)
                .post(admin_create_user_wrapper)
                .route_layer(from_fn(middleware::auth::admin_middleware)),
        )
        .route(
            "/api/admin/users/{id}",
            get(admin_get_user_wrapper)
                .patch(admin_update_user_wrapper)
                .delete(admin_delete_user_wrapper)
                .route_layer(from_fn(middleware::auth::admin_middleware)),
        )
        .route(
            "/api/admin/users/{id}/identities/{identity_id}",
            axum::routing::delete(admin_unlink_identity_wrapper)
                .route_layer(from_fn(middleware::auth::admin_middleware)),
        )
        // Site public domain (BASE_URL / FRONTEND_URL / CORS) — not federation Move
        .route(
            "/api/admin/site/domain",
            post(change_site_domain_wrapper)
                .route_layer(from_fn(middleware::auth::admin_middleware)),
        )
        // PR #4: 公开注册（开关受 allow_local_registration 控制） + 后补密码 + 本地登录开关
        .route("/api/auth/register", post(register_wrapper))
        .route(
            "/api/auth/me/set-password",
            post(set_password_wrapper).route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        .route(
            "/api/auth/me/local-login",
            axum::routing::patch(toggle_local_login_wrapper)
                .route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        // Configuration routes (use wrapper for dynamic DB access)
        .route(
            "/api/config",
            get(get_config_wrapper).route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        .route(
            "/api/config",
            post(update_config_wrapper).route_layer(from_fn(middleware::auth::admin_middleware)),
        )
        .route(
            "/api/config/settings-backup",
            get(export_settings_wrapper)
                .post(restore_settings_wrapper)
                .route_layer(from_fn(middleware::auth::admin_middleware)),
        )
        .route(
            "/api/config/settings-backup/preview",
            post(preview_settings_restore_wrapper)
                .route_layer(from_fn(middleware::auth::admin_middleware)),
        )
        .route(
            "/api/config/dashboard",
            post(update_dashboard_config_wrapper)
                .route_layer(from_fn(middleware::auth::admin_middleware)),
        )
        .route(
            "/api/config/control-panel",
            post(update_control_panel_config_wrapper)
                .route_layer(from_fn(middleware::auth::admin_middleware)),
        )
        .route(
            "/api/config/tapp-window-schemes",
            post(update_tapp_window_schemes_wrapper)
                .route_layer(from_fn(middleware::auth::auth_middleware)), // 登录用户可保存
        )
        .route(
            "/api/config/module-visibility",
            get(get_module_visibility_preferences_wrapper),
        )
        .route(
            "/api/config/module-visibility",
            put(update_module_visibility_preferences_wrapper)
                .route_layer(from_fn(middleware::auth::admin_middleware)),
        )
        // 一言配置读取公开；全局写入仅管理员
        .route("/api/config/hitokoto", get(get_hitokoto_config_wrapper))
        .route(
            "/api/config/hitokoto",
            axum::routing::put(update_hitokoto_config_wrapper)
                .route_layer(from_fn(middleware::auth::admin_middleware)),
        )
        // 报告过期设置：读取公开（读取路径需要）；写入仅管理员
        .route(
            "/api/config/report-settings",
            get(get_report_settings_wrapper),
        )
        .route(
            "/api/config/report-settings",
            axum::routing::put(update_report_settings_wrapper)
                .route_layer(from_fn(middleware::auth::admin_middleware)),
        )
        // 权限配置 API
        .route("/api/config/permissions", get(get_permissions_wrapper)) // 🔓 公开端点：获取当前用户权限
        .route(
            "/api/config/permissions",
            post(update_permissions_wrapper)
                .route_layer(from_fn(middleware::auth::admin_middleware)), // 🔒 仅管理员
        )
        // PR #6: OAuth providers + 本地注册开关（仅管理员可读写）
        .route(
            "/api/config/oauth-providers",
            get(get_oauth_providers_wrapper)
                .put(update_oauth_providers_wrapper)
                .route_layer(from_fn(middleware::auth::admin_middleware)),
        )
        .route(
            "/api/config/test",
            post(test_platform_wrapper).route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        .route("/api/config/metadata", get(get_site_metadata_wrapper)) // 🔓 公开端点：网站元数据
        .route("/api/config/public", get(get_public_config_wrapper)) // 🔓 公开端点：平台公开信息（用于社交链接）
        .route("/api/config/ui", get(get_public_ui_config_wrapper)) // 🔓 公开端点：UI配置（萌宠、壁纸等）
        // ✅ 安全修复 P0: CSRF Token 获取端点
        .route("/api/csrf-token", get(middleware::csrf::get_csrf_token))
        // AI推荐API - 🔓 公开端点：图标推荐服务
        .route(
            "/api/ai/recommend-icon",
            post(api::ai_recommend::recommend_icon),
        )
        // Profile routes (use wrapper for dynamic DB access) - ALWAYS REGISTERED
        .route("/api/profile/user-info", get(get_user_info_wrapper))
        .route("/api/profile/batch", get(get_batch_user_info_wrapper)); // 🚀 性能优化：批量API

    let mut api_router = api_router
        .route("/api/profile/metadata", get(get_raw_metadata_wrapper))
        // ==================== Federation (MFP) 公开端点 ====================
        // Layer 1: 发现（无需认证）
        .route(
            "/.well-known/webfinger",
            get(federation::discovery::webfinger),
        )
        .route(
            "/.well-known/nodeinfo",
            get(federation::discovery::nodeinfo_wellknown),
        )
        .route("/nodeinfo/2.1", get(federation::discovery::nodeinfo))
        // Layer 2: Actor + Outbox + Collections（无需认证，AP 标准端点）
        .route("/users/{username}", get(federation::actor::get_actor))
        .route(
            "/users/{username}/avatar",
            get(federation::actor::get_avatar),
        )
        .nest_service(
            "/api/federation/avatar-cache",
            tower::ServiceBuilder::new()
                .layer(SetResponseHeaderLayer::if_not_present(
                    axum::http::header::CACHE_CONTROL,
                    axum::http::HeaderValue::from_static("public, max-age=604800, immutable"),
                ))
                .service(ServeDir::new(&services::data_paths::paths().cache_images)),
        )
        // Public federation media (Note attachments Image/Video) — URLs embedded in AP.
        // Intentionally unauthenticated GET so remote instances can fetch media during
        // federation. Must stay outside session/auth middleware (see delivery.rs docs).
        .nest_service(
            "/media/federation",
            tower::ServiceBuilder::new()
                .layer(SetResponseHeaderLayer::if_not_present(
                    axum::http::header::CACHE_CONTROL,
                    axum::http::HeaderValue::from_static("public, max-age=604800"),
                ))
                .service(ServeDir::new(
                    federation::content::federation_media_root(),
                )),
        )
        .route(
            "/users/{username}/outbox",
            get(federation::outbox::get_outbox),
        )
        .route(
            "/users/{username}/followers",
            get(federation::actor::get_followers),
        )
        .route(
            "/users/{username}/following",
            get(federation::actor::get_following),
        )
        // Layer 2: Inbox（远程实例投递，通过 HTTP Signature 验证）
        // 40 MiB: channel/room messages (up to 32 MiB payload, Tapp package share)
        // and FileChunk activities. Still under the global 50MB DefaultBodyLimit
        // used for media/avatar uploads — do not lower that global ceiling here.
        .merge(
            Router::new()
                .route(
                    "/users/{username}/inbox",
                    post(federation::inbox::post_inbox),
                )
                .route("/inbox", post(federation::inbox::post_shared_inbox))
                .layer(axum::extract::DefaultBodyLimit::max(40 * 1024 * 1024)),
        )
        // ==================== Federation API（需认证）====================
        // Tapp 宿主归因与认证在 federation_api_router() 内按 Router 级统一挂载。
        .merge(federation_api_router());

    // Add DB-dependent routes if we have a connection
    // These routes require more complex state handling so keep them conditional for now
    if let Some(db) = db_opt {
        let db_router = Router::new()
            // 双层报告系统API - 🔒 REQUIRE AUTHENTICATION
            .route(
                "/api/reports/platform",
                post(api::reports::generate_platform_reports)
                    .route_layer(from_fn(middleware::auth::admin_middleware)),
            )
            .route(
                "/api/reports/comprehensive",
                post(api::reports::generate_comprehensive_report)
                    .route_layer(from_fn(middleware::auth::admin_middleware)),
            )
            .route(
                "/api/reports/generate-all",
                post(api::reports::generate_all_reports)
                    .route_layer(from_fn(middleware::auth::admin_middleware)),
            )
            // Note: /api/reports/latest, /api/reports/comprehensive/list, /api/reports/comprehensive/{id}
            // are now registered above with wrappers in the main api_router
            .route(
                "/api/reports/comprehensive/{id}/delete",
                delete(api::reports::delete_comprehensive_report)
                    .route_layer(from_fn(middleware::auth::admin_middleware)),
            )
            // Note: /api/auth/me and /api/auth/logout are now registered above with wrappers
            // Note: /api/config routes are now registered above with wrappers, not here
            // Note: /api/profile/user-info, metadata now registered above with wrappers
            .route("/api/platforms", get(api::platforms::list_platforms))
            .route("/api/profiles", get(api::platforms::get_profiles))
            .route(
                "/api/fetch",
                post(api::platforms::trigger_fetch)
                    .route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            .route(
                "/api/analysis",
                get(api::analysis::get_analysis)
                    .post(api::analysis::trigger_analysis)
                    .route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            // Prompt generation - 🔒 REQUIRE AUTHENTICATION
            .route(
                "/api/prompt/generate",
                post(api::prompt::generate_prompt)
                    .route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            // 后台任务管理 API - 🔒 REQUIRE AUTHENTICATION
            .route(
                "/api/tasks",
                post(api::tasks::submit_task)
                    .get(api::tasks::list_tasks)
                    .route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            .route(
                "/api/tasks/{task_id}",
                get(api::tasks::get_task_status)
                    .route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            .route(
                "/api/tasks/platform/{platform}",
                get(api::tasks::get_platform_task)
                    .route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            // 缓存管理 API - 🔒 REQUIRE AUTHENTICATION
            .route(
                "/api/cache/status",
                get(api::cache::get_cache_status)
                    .route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            .route(
                "/api/cache/status/{platform}",
                get(api::cache::get_platform_cache_status)
                    .route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            .route(
                "/api/cache/{platform}",
                delete(api::cache::clear_platform_cache)
                    .route_layer(from_fn(middleware::auth::admin_middleware)),
            )
            .route(
                "/api/cache/clear",
                post(api::cache::clear_caches)
                    .route_layer(from_fn(middleware::auth::admin_middleware)),
            )
            .route(
                "/api/cache/all",
                delete(api::cache::clear_all_caches)
                    .route_layer(from_fn(middleware::auth::admin_middleware)),
            )
            // Profile report routes (complex ones still conditional) - 🔒 REQUIRE AUTHENTICATION
            .route(
                "/api/profile/fetch-all",
                post(api::profile::fetch_all_data)
                    .route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            .route(
                "/api/profile/fetch-platform",
                post(api::profile::fetch_single_platform_data)
                    .route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            .route(
                "/api/profile/refresh",
                post(api::profile::refresh_platform_data)
                    .route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            .route(
                "/api/profile/cache",
                delete(api::profile::delete_platform_cache)
                    .route_layer(from_fn(middleware::auth::admin_middleware)),
            )
            // Library data route (公开访问 - 单用户系统)
            .route("/api/library", get(api::profile::get_library_data))
            .route(
                "/api/library/preferences",
                get(api::profile::get_library_source_preferences),
            )
            .route(
                "/api/library/preferences",
                axum::routing::put(api::profile::update_library_source_preferences)
                    .route_layer(from_fn(middleware::auth::admin_middleware)),
            )
            // Recent activities route (公开访问 - 单用户系统)
            .route("/api/activities", get(api::profile::get_recent_activities))
            // Reports routes (读取端点公开访问，支持未认证用户)
            .route("/api/reports/latest", get(get_latest_report_wrapper))
            .route(
                "/api/reports/list",
                get(api::tapp_runtime::list_reports)
                    .route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            .route(
                "/api/reports/comprehensive/list",
                get(get_comprehensive_reports_list_wrapper),
            )
            .route(
                "/api/reports/comprehensive/{id}",
                get(get_comprehensive_report_by_id_wrapper),
            )
            // ============ Tapp 应用管理 API ============
            // 部分公开访问（游客可查看管理员的 Tapp），部分需要认证（在路由内部处理）
            .nest("/api/tapps", api::tapp_store::create_tapp_routes())
            // ============ Tapp Playground（管理员 + Pro 模型）============
            .nest(
                "/api/tapp-playground",
                api::tapp_playground::create_playground_routes(),
            )
            // ============ Agent AI 任务编排 API ============
            // 自然语言任务分解、执行和监控
            .nest("/api/agent", api::agent::create_agent_routes())
            // ============ Brew 阅读 API ============
            // RSS/Atom 订阅管理、文章获取、阅读状态同步
            .nest("/api/brew", api::brew::create_brew_routes())
            // ============ Brewlia AI 增强 API ============
            // AI 词汇注释、内容摘要等增强阅读功能
            .nest("/api/brewlia", api::brewlia::create_brewlia_routes())
            // ============ 语音服务 API ============
            // 腾讯云 TTS 文本转语音、ASR 语音转文本
            .nest("/api/speech", api::speech::create_speech_routes())
            // ============ Tapp API ============
            // Platform data API - 🔒 REQUIRE AUTHENTICATION
            .route(
                "/api/tapp/platform/{platform}/data",
                get(api::tapp_runtime::get_platform_data)
                    .route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            .route(
                "/api/tapp/platform/{platform}/stats",
                get(api::tapp_runtime::get_platform_stats)
                    .route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            .route(
                "/api/tapp/platform/{platform}/distribution/{dimension}",
                get(api::tapp_runtime::get_platform_distribution)
                    .route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            .route(
                "/api/tapp/platform/items",
                post(api::tapp_runtime::add_platform_item)
                    .route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            .route(
                "/api/tapp/platform/items/batch",
                post(api::tapp_runtime::add_platform_items_batch)
                    .route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            // AI Task API - 支持权限下放（使用 optional_auth）
            .route(
                "/api/tapp/ai/v2/tasks",
                post(api::tapp_runtime::create_ai_task)
                    .route_layer(from_fn(middleware::auth::optional_auth_middleware)),
            )
            .route(
                "/api/tapp/ai/v2/tasks/{task_id}",
                get(api::tapp_runtime::get_ai_task)
                    .delete(api::tapp_runtime::cancel_ai_task)
                    .route_layer(from_fn(middleware::auth::optional_auth_middleware)),
            )
            .route(
                "/api/tapp/ai/v2/tasks/{task_id}/events",
                get(api::tapp_runtime::stream_ai_task_events)
                    .route_layer(from_fn(middleware::auth::optional_auth_middleware)),
            )
            .route(
                "/api/tapp/ai/v2/usage",
                get(api::tapp_runtime::ai_usage)
                    .route_layer(from_fn(middleware::auth::optional_auth_middleware)),
            )
            // 独立 AI 费用账本（宿主 UI 专用，读取本人逐次调用流水）
            .route(
                "/api/tapp/ai/v2/ledger",
                get(api::tapp_runtime::ai_cost_ledger)
                    .route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            // ============ Tapp P0 扩展 API ============
            // Data Processing: inline transforms support guests; platform/storage
            // inputs and outputs are still denied without their Runtime Grant permissions.
            .route(
                "/api/tapp/data/transform",
                post(api::tapp_runtime::data_transform)
                    .route_layer(from_fn(middleware::auth::optional_auth_middleware)),
            )
            // Cross-Tapp data remains private until a visible one-shot host
            // authorization has produced a consumable Data Access Grant.
            .route(
                "/api/tapp/data-exchange/requests",
                post(api::tapp_runtime::prepare_data_exchange)
                    .route_layer(from_fn(middleware::auth::optional_auth_middleware)),
            )
            .route(
                "/api/tapp/data-exchange/requests/{request_id}/authorize",
                post(api::tapp_runtime::authorize_data_exchange)
                    .route_layer(from_fn(middleware::auth::optional_auth_middleware)),
            )
            .route(
                "/api/tapp/data-exchange/requests/{request_id}",
                delete(api::tapp_runtime::cancel_data_exchange)
                    .route_layer(from_fn(middleware::auth::optional_auth_middleware)),
            )
            .route(
                "/api/tapp/data-exchange/consume",
                post(api::tapp_runtime::consume_data_exchange)
                    .route_layer(from_fn(middleware::auth::optional_auth_middleware)),
            )
            // Context API - 🔓 支持权限下放（公开信息）
            .route(
                "/api/tapp/context/app",
                get(api::tapp_runtime::get_context_app)
                    .route_layer(from_fn(middleware::auth::optional_auth_middleware)),
            )
            .route(
                "/api/tapp/context/user",
                get(api::tapp_runtime::get_context_user)
                    .route_layer(from_fn(middleware::auth::optional_auth_middleware)),
            )
            .route(
                "/api/tapp/context/player",
                get(api::tapp_runtime::get_context_player)
                    .route_layer(from_fn(middleware::auth::optional_auth_middleware)),
            )
            .route(
                "/api/tapp/context/navigation",
                get(api::tapp_runtime::get_context_navigation)
                    .route_layer(from_fn(middleware::auth::optional_auth_middleware)),
            )
            .route(
                "/api/tapp/context/system",
                get(api::tapp_runtime::get_context_system)
                    .route_layer(from_fn(middleware::auth::optional_auth_middleware)),
            )
            // Federation feed: guests see public items; users see public + personal items.
            .route(
                "/api/tapp/federation/feed",
                get(api::tapp_runtime::get_federation_feed)
                    .route_layer(from_fn(middleware::auth::optional_auth_middleware)),
            )
            // ============ Tapp P1 扩展 API ============
            // Report CRUD - 🔒 REQUIRE AUTHENTICATION
            .route(
                "/api/tapp/reports",
                post(api::tapp_runtime::create_report)
                    .route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            .route(
                "/api/tapp/report-catalog",
                get(api::tapp_runtime::list_runtime_reports)
                    .route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            .route(
                "/api/tapp/report-catalog/{report_id}",
                get(api::tapp_runtime::get_runtime_report)
                    .route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            .route(
                "/api/tapp/report-catalog/platform/{platform}",
                get(api::tapp_runtime::get_runtime_platform_report)
                    .route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            .route(
                "/api/tapp/reports/tapp/{tapp_id}",
                get(api::tapp_runtime::list_tapp_reports)
                    .route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            .route(
                "/api/tapp/reports/{tapp_id}/{report_id}",
                get(api::tapp_runtime::get_tapp_report)
                    .put(api::tapp_runtime::update_tapp_report)
                    .delete(api::tapp_runtime::delete_tapp_report)
                    .route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            // Media Control - 🔓 支持权限下放
            .route(
                "/api/tapp/media/control",
                post(api::tapp_runtime::media_control)
                    .route_layer(from_fn(middleware::auth::optional_auth_middleware)),
            )
            .route(
                "/api/tapp/media/status",
                get(api::tapp_runtime::media_status)
                    .route_layer(from_fn(middleware::auth::optional_auth_middleware)),
            )
            // Tapp notifications - unified notification pipeline
            .route(
                "/api/tapp/notifications",
                post(api::tapp_runtime::create_tapp_notification)
                    .route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            // P2: Component Registration
            .route(
                "/api/tapp/components/register",
                post(api::tapp_runtime::register_component)
                    .route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            .route(
                "/api/tapp/components/{tapp_id}/{component_type}/{component_id}",
                delete(api::tapp_runtime::unregister_component)
                    .route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            .route(
                "/api/tapp/components/{tapp_id}",
                get(api::tapp_runtime::list_components)
                    .route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            .route(
                "/api/tapp/components/all/{component_type}",
                get(api::tapp_runtime::list_all_components_by_type)
                    .route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            // P2: Shortcut Registration
            .route(
                "/api/tapp/shortcuts/register",
                post(api::tapp_runtime::register_shortcut)
                    .route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            .route(
                "/api/tapp/shortcuts/{tapp_id}/{shortcut_id}",
                delete(api::tapp_runtime::unregister_shortcut)
                    .route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            .route(
                "/api/tapp/shortcuts",
                get(api::tapp_runtime::list_shortcuts)
                    .route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            // Manifest-scoped Event Broker
            .route(
                "/api/tapp/events/publish",
                post(api::tapp_runtime::publish_event)
                    .route_layer(from_fn(middleware::auth::optional_auth_middleware)),
            )
            .route(
                "/api/tapp/events/stream",
                get(api::tapp_runtime::stream_events)
                    .route_layer(from_fn(middleware::auth::optional_auth_middleware)),
            )
            .route(
                "/api/tapp/agent/v2/interactions/stream",
                get(api::tapp_runtime::stream_agent_interactions)
                    .route_layer(from_fn(middleware::auth::optional_auth_middleware)),
            )
            .route(
                "/api/tapp/agent/v2/interactions/{interaction_id}",
                get(api::tapp_runtime::get_agent_interaction)
                    .route_layer(from_fn(middleware::auth::optional_auth_middleware)),
            )
            .route(
                "/api/tapp/agent/v2/interactions/{interaction_id}/accept",
                post(api::tapp_runtime::accept_agent_interaction)
                    .route_layer(from_fn(middleware::auth::optional_auth_middleware)),
            )
            .route(
                "/api/tapp/agent/v2/interactions/{interaction_id}/result",
                post(api::tapp_runtime::submit_agent_interaction_result)
                    .route_layer(from_fn(middleware::auth::optional_auth_middleware)),
            )
            .route(
                "/api/tapp/agent/v2/interactions/{interaction_id}/reject",
                post(api::tapp_runtime::reject_agent_interaction)
                    .route_layer(from_fn(middleware::auth::optional_auth_middleware)),
            )
            .route(
                "/api/tapp/agent/v2/interactions/{interaction_id}/intents",
                post(api::tapp_runtime::request_agent_intent)
                    .route_layer(from_fn(middleware::auth::optional_auth_middleware)),
            )
            // Metrics & Rate Limit
            .route(
                "/api/tapp/metrics",
                get(api::tapp_runtime::get_tapp_metrics)
                    .route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            .route(
                "/api/tapp/rate-limit/{tapp_id}",
                get(api::tapp_runtime::get_rate_limit_status)
                    .route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            // ============ Tapp 定时任务 API ============
            // Scheduler - 🔒 REQUIRE AUTHENTICATION
            .route(
                "/api/tapp/scheduler/tasks",
                get(api::tapp_scheduler::list_tasks)
                    .post(api::tapp_scheduler::register_task)
                    .route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            .route(
                "/api/tapp/scheduler/{tapp_id}/tasks",
                get(api::tapp_scheduler::list_tapp_tasks)
                    .route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            .route(
                "/api/tapp/scheduler/{tapp_id}/tasks/{task_id}",
                get(api::tapp_scheduler::get_task)
                    .delete(api::tapp_scheduler::unregister_task)
                    .route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            .route(
                "/api/tapp/scheduler/{tapp_id}/tasks/{task_id}/enable",
                post(api::tapp_scheduler::enable_task)
                    .route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            .route(
                "/api/tapp/scheduler/{tapp_id}/tasks/{task_id}/disable",
                post(api::tapp_scheduler::disable_task)
                    .route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            .route(
                "/api/tapp/scheduler/{tapp_id}/tasks/{task_id}/trigger",
                post(api::tapp_scheduler::trigger_task)
                    .route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            .route(
                "/api/tapp/scheduler/ws",
                get(api::tapp_scheduler::scheduler_websocket)
                    .route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            // ============ Tapp API 声明系统 ============
            // API Execute - 支持 public 和 protected 两级权限
            .route(
                "/api/tapp/{tapp_id}/api/{api_name}",
                post(api::tapp_runtime::execute_tapp_api)
                    .route_layer(from_fn(middleware::auth::optional_auth_middleware)),
            )
            // API List - 列出 Tapp 可用的 API
            .route(
                "/api/tapp/{tapp_id}/apis",
                get(api::tapp_runtime::list_tapp_apis)
                    .route_layer(from_fn(middleware::auth::optional_auth_middleware)),
            )
            // Context Geo - 公开 API，获取客户端地理位置
            .route(
                "/api/tapp/context/geo",
                get(api::tapp_runtime::get_context_geo)
                    .route_layer(from_fn(middleware::auth::optional_auth_middleware)),
            )
            // Image proxy route
            .route("/api/proxy/image", get(api::proxy::proxy_image))
            // Client geo location route
            .route("/api/proxy/client-geo", get(api::proxy::get_client_geo))
            // Hitokoto proxy route
            .route("/api/proxy/hitokoto", get(api::proxy::proxy_hitokoto))
            // Web content fetch proxy (for reading list from web search)
            .route(
                "/api/proxy/fetch-content",
                get(api::proxy::fetch_web_content),
            )
            // Music proxy routes
            .route(
                "/api/proxy/music/netease/playlist/{id}",
                get(api::proxy::proxy_netease_playlist),
            )
            .route(
                "/api/proxy/music/netease/lyrics/{id}",
                get(api::proxy::proxy_netease_lyrics),
            )
            .route(
                "/api/proxy/music/netease/lyrics-verbatim/{id}",
                get(api::proxy::proxy_netease_lyrics_verbatim),
            )
            .route(
                "/api/proxy/music/netease/song/{id}",
                get(api::proxy::proxy_netease_song),
            )
            .route(
                "/api/proxy/music/netease/audio/{id}",
                get(api::proxy::proxy_netease_audio),
            )
            .route(
                "/api/proxy/music/qq/playlist/{id}",
                get(api::proxy::proxy_qq_playlist),
            )
            .route(
                "/api/proxy/music/qq/audio/{id}",
                get(api::proxy::proxy_qq_audio),
            )
            .route(
                "/api/proxy/music/qq/lyrics/{id}",
                get(api::proxy::proxy_qq_lyrics),
            )
            .route(
                "/api/proxy/music/kugou/lyrics-verbatim",
                get(api::proxy::proxy_kugou_lyrics_verbatim),
            )
            // Bilibili API routes
            .route("/api/bilibili/user", get(api::bilibili::get_bilibili_user))
            .route(
                "/api/bilibili/user/{uid}",
                get(api::bilibili::get_bilibili_user_info),
            )
            .route(
                "/api/bilibili/favorites/{uid}",
                get(api::bilibili::get_bilibili_favorites),
            )
            .route(
                "/api/bilibili/bangumi/{uid}",
                get(api::bilibili::get_bilibili_bangumi),
            )
            .route(
                "/api/bilibili/bangumi/all/{uid}",
                get(api::bilibili::get_all_bilibili_bangumi),
            )
            // Bangumi API routes
            .route("/api/bangumi/user", get(api::bangumi::get_bangumi_user))
            .route(
                "/api/bangumi/user/{username}",
                get(api::bangumi::get_bangumi_user_info),
            )
            .route("/api/bangumi/me", get(api::bangumi::get_bangumi_me))
            .route(
                "/api/bangumi/collections/{username}",
                get(api::bangumi::get_bangumi_collections),
            )
            // Steam API routes
            .route("/api/steam/presence", get(api::steam::get_steam_presence))
            .route("/api/steam/user", get(api::steam::get_steam_user))
            .route("/api/steam/user/info", get(api::steam::get_steam_user_info))
            .route("/api/steam/games", get(api::steam::get_steam_games))
            // 游戏公开状态小组件（UID / Gamertag / Online ID，无用户 Cookie）
            .route(
                "/api/game/presence",
                get(api::game_presence::get_game_presence),
            )
            .route(
                "/api/game/presence/capabilities",
                get(api::game_presence::get_game_presence_capabilities),
            )
            .route(
                "/api/steam/wishlist/{steam_id}",
                get(api::steam::get_steam_wishlist),
            )
            .route("/api/steam/stats", get(api::steam::get_steam_stats))
            .route(
                "/api/steam/game/{app_id}",
                get(api::steam::get_steam_game_details),
            )
            // X (Twitter) — 直连调试接口会带 bearer query，必须登录；正式同步走配置 + profile fetch
            .route(
                "/api/x/user",
                get(api::x::get_x_user).route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            .route(
                "/api/x/user/info",
                get(api::x::get_x_user_info)
                    .route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            // 分享到 X：仅生成 Intent 链接（不代发帖、不 OAuth）
            .route(
                "/api/x/share/status",
                get(api::x::share_status).route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            .route(
                "/api/x/share",
                post(api::x::share_to_x).route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            // Discord — 调试接口带 access_token query，必须登录；正式同步走配置 + profile fetch
            .route(
                "/api/discord/status",
                get(api::discord::discord_status)
                    .route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            .route(
                "/api/discord/me",
                get(api::discord::get_discord_me)
                    .route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            .route(
                "/api/discord/profile",
                get(api::discord::get_discord_profile)
                    .route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            // Discord 数据平台一键授权（start 需 admin cookie；callback 公开 + state CSRF）
            .route(
                "/api/platforms/discord/oauth/start",
                get(api::discord::oauth_start)
                    .route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            .route(
                "/api/platforms/discord/oauth/callback",
                get(discord_platform_oauth_callback_wrapper),
            )
            // MyAnimeList — 调试接口：username 必填，client_id 可选；正式同步走配置 + profile fetch
            .route(
                "/api/mal/user",
                get(api::mal::get_mal_user).route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            .route(
                "/api/mal/user/{username}",
                get(api::mal::get_mal_user_info)
                    .route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            .route(
                "/api/mal/anime/{username}",
                get(api::mal::get_mal_anime_list)
                    .route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            .route(
                "/api/mal/manga/{username}",
                get(api::mal::get_mal_manga_list)
                    .route_layer(from_fn(middleware::auth::auth_middleware)),
            )
            .with_state(db);

        // Merge with base router
        api_router = api_router.merge(db_router);
    }

    // 🚀 Updater admin proxy routes (admin-only). Backend forwards to the updater container
    // and injects UPDATE_TOKEN server-side, so the browser never sees the secret.
    // See docs/updater-spec.md §13.
    {
        use middleware::auth::admin_middleware;
        api_router = api_router
            .route(
                "/api/admin/updater/status",
                get(api::updater_admin::status).route_layer(from_fn(admin_middleware)),
            )
            .route(
                "/api/admin/updater/available",
                get(api::updater_admin::available).route_layer(from_fn(admin_middleware)),
            )
            .route(
                "/api/admin/updater/jobs",
                get(api::updater_admin::jobs).route_layer(from_fn(admin_middleware)),
            )
            .route(
                "/api/admin/updater/jobs/{id}",
                get(api::updater_admin::job).route_layer(from_fn(admin_middleware)),
            )
            .route(
                "/api/admin/updater/snapshots",
                get(api::updater_admin::snapshots).route_layer(from_fn(admin_middleware)),
            )
            .route(
                "/api/admin/updater/snapshots/{id}",
                axum::routing::delete(api::updater_admin::delete_snapshot)
                    .route_layer(from_fn(admin_middleware)),
            )
            .route(
                "/api/admin/updater/commits",
                get(api::updater_admin::commits).route_layer(from_fn(admin_middleware)),
            )
            .route(
                "/api/admin/updater/builds",
                get(api::updater_admin::builds).route_layer(from_fn(admin_middleware)),
            )
            .route(
                "/api/admin/updater/releases",
                get(api::updater_admin::releases).route_layer(from_fn(admin_middleware)),
            )
            .route(
                "/api/admin/updater/compare",
                get(api::updater_admin::compare).route_layer(from_fn(admin_middleware)),
            )
            .route(
                "/api/admin/updater/update",
                post(api::updater_admin::trigger_update).route_layer(from_fn(admin_middleware)),
            )
            .route(
                "/api/admin/updater/prefs",
                post(api::updater_admin::set_prefs).route_layer(from_fn(admin_middleware)),
            )
            .route(
                "/api/admin/updater/rollback",
                post(api::updater_admin::rollback).route_layer(from_fn(admin_middleware)),
            )
            .route(
                "/api/admin/updater/diagnostics",
                get(api::updater_admin::diagnostics).route_layer(from_fn(admin_middleware)),
            )
            .route(
                "/api/admin/updater/rescue/exit-maintenance",
                post(api::updater_admin::exit_maintenance).route_layer(from_fn(admin_middleware)),
            )
            .route(
                "/api/admin/updater/rescue/forget-current",
                post(api::updater_admin::forget_current).route_layer(from_fn(admin_middleware)),
            )
            .route(
                "/api/admin/updater/rescue/continue",
                post(api::updater_admin::rescue_continue).route_layer(from_fn(admin_middleware)),
            )
            .route(
                "/api/admin/updater/self-update",
                post(api::updater_admin::self_update).route_layer(from_fn(admin_middleware)),
            )
            .route(
                "/api/admin/updater/proxy-update",
                post(api::updater_admin::proxy_update).route_layer(from_fn(admin_middleware)),
            )
            .route(
                "/api/admin/updater/self-update/last",
                get(api::updater_admin::self_update_last).route_layer(from_fn(admin_middleware)),
            );
    }

    // Apply middleware and layers
    let api_router = api_router
        .layer(from_fn(config_mode_middleware))
        .layer(from_fn(middleware::csrf::csrf_middleware)) // ✅ 安全修复 P0: CSRF 防护
        .layer(from_fn(middleware::rate_limit::rate_limit_middleware)) // Rate limiting
        // Apply security headers after the complete route graph is assembled.
        .layer(from_fn(middleware::security::security_headers_middleware))
        // Global 50MB: media/avatar uploads need a large ceiling. Federation public
        // inbox + federation_api_router apply a stricter 40 MiB DefaultBodyLimit
        // on their own routers (nested limits still apply under this outer layer).
        .layer(axum::extract::DefaultBodyLimit::max(50 * 1024 * 1024)) // 🛡️ 防止OOM: 限制请求体最大50MB
        .layer(cors)
        .layer(TraceLayer::new_for_http());

    // Now the type is unified, convert to Router<()> by applying route matching
    let app: Router = if std::path::Path::new(&config.frontend_dist_path).exists() {
        tracing::info!("Serving frontend from: {}", config.frontend_dist_path);
        // SPA fallback: 未匹配的浏览器路由 → index.html（React Router）。
        // 重要：ServeDir 对任何非 GET/HEAD 请求直接返回 405，所以 /api/* 绝不能落到
        // 静态文件服务——否则未注册的 POST（例如旧 backend 进程缺 /prefs）会误报 405
        // 而不是可读的 JSON 404。
        let index_html = std::path::Path::new(&config.frontend_dist_path).join("index.html");
        let serve_dir =
            ServeDir::new(&config.frontend_dist_path).not_found_service(ServeFile::new(index_html));
        api_router.fallback(move |req: Request| {
            let serve_dir = serve_dir.clone();
            async move {
                let path = req.uri().path();
                if path.starts_with("/api/") || path == "/health" {
                    return (
                        StatusCode::NOT_FOUND,
                        Json(json!({
                            "error": "Not Found",
                            "message": format!(
                                "No API route for {} {}",
                                req.method(),
                                path
                            ),
                        })),
                    )
                        .into_response();
                }
                use tower::ServiceExt;
                match serve_dir.oneshot(req).await {
                    Ok(res) => res.into_response(),
                    Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
                }
            }
        })
    } else {
        tracing::warn!("Frontend dist path not found, serving API only");
        api_router
    };

    // Spawn background task to clean up old tasks (防止内存泄漏)
    tokio::spawn(async {
        let mut cleanup_interval = tokio::time::interval(tokio::time::Duration::from_secs(300)); // 每5分钟
        loop {
            cleanup_interval.tick().await;
            services::background_processor::BACKGROUND_PROCESSOR
                .cleanup_old_tasks()
                .await;
            tracing::info!("🧹 Background task cleanup completed");
        }
    });

    // Spawn background task to monitor for config reload
    tokio::spawn(async move {
        let mut check_interval = tokio::time::interval(tokio::time::Duration::from_secs(2));
        loop {
            check_interval.tick().await;

            if api::system::is_config_reload_requested() {
                tracing::info!(
                    "🔄 Configuration reload detected - attempting to reconnect database"
                );
                api::system::reset_config_reload_flag();

                // Reload .env file with override to ensure latest values
                let env_path = std::path::PathBuf::from(".env");
                if let Err(e) = dotenvy::from_path_override(&env_path) {
                    tracing::warn!("⚠️ Failed to reload .env file: {}", e);
                } else {
                    tracing::info!("♻️ Environment variables reloaded from .env");
                }

                match AppConfig::from_env() {
                    Ok(new_config) => {
                        // Update global config FIRST for hot-reload
                        *GLOBAL_CONFIG.write().await = new_config.clone();
                        tracing::info!(
                            "♻️ Global configuration updated - AI API settings now live!"
                        );

                        if !new_config.database_url.is_empty() {
                            match db::connection::establish_connection(&new_config.database_url)
                                .await
                            {
                                Ok(db) => {
                                    tracing::info!("✅ Database connection established!");

                                    match api::tapp_store::recover_tapp_filesystem_state(&db).await
                                    {
                                        Ok(0) => {}
                                        Ok(count) => tracing::warn!(
                                            count,
                                            "Recovered interrupted Tapp filesystem transactions"
                                        ),
                                        Err(error) => tracing::error!(
                                            %error,
                                            "Failed to inspect Tapp filesystem transaction state"
                                        ),
                                    }

                                    // Update global database connection
                                    *DB_CONNECTION.write().await = Some(db.clone());

                                    // Reload dynamic configuration from database
                                    let config_service = ConfigService::new(db);
                                    match config_service.load_config().await {
                                        Ok(dynamic_config) => {
                                            *GLOBAL_DYNAMIC_CONFIG.write().await = dynamic_config;
                                            tracing::info!(
                                                "✅ Dynamic configuration reloaded from database"
                                            );
                                        }
                                        Err(e) => {
                                            tracing::warn!(
                                                "⚠️  Failed to reload dynamic config: {}",
                                                e
                                            );
                                        }
                                    }

                                    // 🔐 Reload OAuth provider registry from new dynamic config
                                    services::oauth::registry::REGISTRY.reload().await;

                                    // Switch to full mode FIRST before logging
                                    CONFIG_MODE.store(false, Ordering::Relaxed);

                                    tracing::info!(
                                        "🎉 Switched from CONFIGURATION MODE to FULL MODE"
                                    );
                                    tracing::info!("✨ All API endpoints are now available!");
                                    tracing::info!(
                                        "🔓 Login endpoint is now accessible at /api/auth/login"
                                    );
                                }
                                Err(e) => {
                                    tracing::error!(
                                        "❌ Database connection failed after reload: {}",
                                        e
                                    );
                                    tracing::info!("🔧 Staying in configuration mode");
                                }
                            }
                        } else {
                            tracing::warn!("⚠️  DATABASE_URL still empty after reload");
                        }
                    }
                    Err(e) => {
                        tracing::error!("❌ Failed to reload configuration: {}", e);
                    }
                }
            }
        }
    });

    // Spawn database health check task (P1优化：定期健康检查和自动重连)
    tokio::spawn(async {
        let mut health_check_interval = tokio::time::interval(tokio::time::Duration::from_secs(60)); // 每分钟检查一次
        loop {
            health_check_interval.tick().await;

            let db_opt = DB_CONNECTION.read().await;
            if let Some(db) = db_opt.as_ref() {
                // 执行简单查询测试连接
                match db
                    .execute(sea_orm::Statement::from_string(
                        sea_orm::DatabaseBackend::Postgres,
                        "SELECT 1".to_owned(),
                    ))
                    .await
                {
                    Ok(_) => {
                        tracing::debug!("💚 Database health check passed");
                    }
                    Err(e) => {
                        tracing::error!("❌ Database health check failed: {}", e);

                        // 尝试重新连接
                        drop(db_opt); // 释放读锁

                        let config = GLOBAL_CONFIG.read().await;
                        if !config.database_url.is_empty() {
                            tracing::info!("🔄 Attempting to reconnect to database...");
                            match crate::db::connection::establish_connection(&config.database_url)
                                .await
                            {
                                Ok(new_db) => {
                                    *DB_CONNECTION.write().await = Some(new_db);
                                    tracing::info!("✅ Database reconnected successfully");
                                }
                                Err(e) => {
                                    tracing::error!("❌ Failed to reconnect to database: {}", e);
                                }
                            }
                        }
                    }
                }
            }
        }
    });

    // Start server with the app (convert to service within start_server)
    start_server(config, app).await
}

/// Common server startup logic
async fn start_server(config: AppConfig, app: Router) -> anyhow::Result<()> {
    let host = config
        .server_host
        .parse::<std::net::IpAddr>()
        .unwrap_or_else(|_| std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1)));
    let addr = SocketAddr::new(host, config.server_port);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("🚀 Server listening on http://{}", addr);

    // Use graceful shutdown with ConnectInfo support
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;

    Ok(())
}

async fn shutdown_signal() {
    use tokio::signal;

    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            tracing::info!("Received Ctrl+C signal");
        },
        _ = terminate => {
            tracing::info!("Received terminate signal");
        },
    }

    tracing::info!("Starting graceful shutdown...");

    // 停止调度器引擎
    api::tapp_scheduler::shutdown_scheduler().await;
    services::brew_scheduler::shutdown_brew_scheduler().await;

    // 等待进行中的 heartbeat（最长 30s），减少杀进程时半途副作用
    services::agent::heartbeat::wait_inflight_drain(std::time::Duration::from_secs(30)).await;

    // Agent 状态落盘 + MCP 子进程回收（滚动更新不丢最近记忆/技能统计）
    if let Some(memory) = services::agent::memory::get_memory() {
        memory.force_flush().await;
        tracing::info!("[Shutdown] Agent memory flushed");
    }
    if let Some(evolution) = services::agent::skill_evolution::get_skill_evolution() {
        evolution.flush().await;
        tracing::info!("[Shutdown] Skill evolution stats flushed");
    }
    services::agent::mcp::shutdown_mcp().await;
}
