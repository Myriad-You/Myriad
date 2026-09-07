//! Myriad backend binary.
//!
//! Re-exports are path-stable on purpose: submodules reach each other through
//! `use super::*`, and several `use` lines only feed `#[cfg(test)]` blocks, so
//! the non-test target reports them unused. Removing them breaks the test
//! target — keep the allow rather than trusting `cargo fix --all-targets`.
#![allow(unused_imports)]
#![allow(private_interfaces)]
// Style fallout from the large modularization split (doc formatting, signature
// shape, structural locals). Behavior is covered by unit/black-box tests;
// tightening these lints is a follow-up hygiene pass, not a security gate.
#![allow(clippy::needless_update)]
#![allow(clippy::doc_lazy_continuation)]
#![allow(clippy::type_complexity)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::redundant_locals)]
#![allow(clippy::empty_line_after_doc_comments)]
#![allow(clippy::unnecessary_sort_by)]
#![allow(clippy::redundant_guards)]

use axum::{
    extract::Request,
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
use tower_http::compression::CompressionLayer;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod api;
mod config;
mod db;
mod error;
mod extract;
mod federation;
mod memory_audit_invariants;
mod middleware;
mod models;
mod oauth_url_builder;
mod router;
mod services;
mod state;

use config::{AppConfig, DynamicConfig};
use sea_orm::ConnectionTrait;
use services::config_service::ConfigService;
use std::sync::atomic::{AtomicBool, Ordering}; // P1: 用于数据库健康检查

// Global flag to indicate if server is running in configuration mode
pub static CONFIG_MODE: AtomicBool = AtomicBool::new(false);

/// True only after this process has applied migrations and completed the
/// schema contract check. Health must not infer this from router selection or
/// from a merely-open database connection.
pub static SCHEMA_READY: AtomicBool = AtomicBool::new(false);

/// Convert every migration/schema failure into a full-mode startup error.
/// Recovery is an explicit operator workflow; no environment override may
/// expose normal routes against an unproven contract.
fn startup_schema_error(stage: &str, err: &impl std::fmt::Display) -> anyhow::Error {
    anyhow::anyhow!(
        "{stage} failed: {err}. Refusing to start full mode because the required database \
         schema was not proven. Repair the database with the explicit migration/recovery \
         workflow; schema-drift overrides never permit normal traffic."
    )
}

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
    // Docker: durable site origin (DATA_DIR/site_public.env) outlives compose-injected CORS.
    api::site_domain::load_durable_site_public_env();

    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "myriad_backend=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Stamp package version + uptime before any log/health that reads build_version().
    // (Must run before the startup banner — otherwise fallback is still v0.0.0-dev.)
    api::init_process_identity();

    print_startup_logo();

    tracing::info!(
        version = api::build_version(),
        commit_sha = ?api::build_commit_sha(),
        "Starting Myriad Backend"
    );

    // Production compose de-roots backend (USER myriad). Warn once if still root.
    warn_if_running_as_root();

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

/// Truecolor half-block logo (`shared/logo-ansi.txt`). Character only — no wordmark.
fn print_startup_logo() {
    const LOGO: &str = include_str!("../../shared/logo-ansi.txt");
    print!("{LOGO}");
    if !LOGO.ends_with('\n') {
        println!();
    }
    println!();
    let _ = std::io::Write::flush(&mut std::io::stdout());
}

/// Cache-Control for statically served frontend assets. `ServeDir` emits only
/// `Last-Modified`, so without this every asset forces a revalidation round-trip
/// per navigation (dozens of unhashed icons ⇒ dozens of conditional GETs). Tiers:
/// - `/assets/*`  content-hashed by Astro → immutable, cache for a year.
/// - media/fonts  unhashed but rarely change → week-long TTL, revalidate in
/// the background while serving the stale copy.
/// - `/sw.js` + HTML  must always revalidate so a new deploy (and its fresh
/// hashed-asset references) lands immediately.
fn static_asset_cache_control(path: &str) -> &'static str {
    if path.starts_with("/assets/") {
        return "public, max-age=31536000, immutable";
    }
    if path == "/sw.js" {
        return "no-cache";
    }
    let is_longlived_static = path.starts_with("/icons/")
        || path.starts_with("/game-logos/")
        || path.starts_with("/fonts/")
        || path.ends_with(".webp")
        || path.ends_with(".png")
        || path.ends_with(".jpg")
        || path.ends_with(".jpeg")
        || path.ends_with(".gif")
        || path.ends_with(".svg")
        || path.ends_with(".avif")
        || path.ends_with(".ico")
        || path.ends_with(".woff")
        || path.ends_with(".woff2");
    if is_longlived_static {
        return "public, max-age=604800, stale-while-revalidate=86400";
    }
    "no-cache"
}

async fn run_server() -> anyhow::Result<()> {
    SCHEMA_READY.store(false, Ordering::Release);
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

    // 验证 JWT 密钥强度
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

    // Memory profile before first DB pool (env can force saver for 1 GiB hosts).
    // Re-applied after dynamic config load with `memory_saver_enabled`.
    services::memory_profile::apply_from_saver_flag(false);

    // Try to initialize database connection if URL is configured.
    // When DATABASE_URL is set (external DB / compose), retry with backoff before
    // falling into CONFIGURATION MODE — a single pool timeout after stack restart
    // must not permanently strand production deploys. Empty URL keeps first-boot setup.
    if !config.database_url.is_empty() {
        let db_target = db::connection::redact_database_url(&config.database_url);
        match db::connection::establish_connection_with_retry(&config.database_url).await {
            Ok(db) => {
                tracing::info!(db_target = %db_target, "✅ Database connection established");

                // Run database migrations automatically on startup (idempotent).
                // Folded 007–015 names are deleted from `seaql_migrations` first
                // so SeaORM does not require no-op files for them; leftover
                // `digital_life_*` experiment tables are dropped in the same
                // step. Any remaining migration failure is fatal to full mode.
                tracing::debug!("Checking for pending database migrations...");
                migration::Migrator::up(&db, None)
                    .await
                    .map_err(|error| startup_schema_error("Database migration", &error))?;
                tracing::info!("✅ Database migrations up to date");

                // Compatibility upgrades are still idempotent, but the check is
                // a hard gate: full routes are impossible after a partial or
                // skipped repair.
                db::schema_check::ensure_schema(&db)
                    .await
                    .map_err(|error| startup_schema_error("Schema contract check", &error))?;
                SCHEMA_READY.store(true, Ordering::Release);

                // 站长画像快照兜底：平台画像 SQL 阶梯够不到，存量库补完列后需要算一次，
                // 否则要等到下次登录/抓取，`/api/auth/me` 与首页信息条会短暂显示两张脸。
                match services::site_owner::site_owner_user_id(&db).await {
                    Ok(owner_id) => {
                        services::avatar::refresh_avatar_snapshot(&db, owner_id).await;
                    }
                    Err(error) => {
                        tracing::debug!(%error, "Skipping avatar snapshot warmup (no site owner yet)")
                    }
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
                        services::memory_profile::apply_from_saver_flag(
                            dynamic_config.memory_saver_enabled,
                        );
                        if dynamic_config.merope_needs_lite() {
                            tracing::warn!(
                                "⚠️  Merope is on without Lite; proactive speech uses a short \
                                 fallback and mood hints stay off (no Standard spend)"
                            );
                        }
                        if dynamic_config.merope_needs_pro() {
                            tracing::warn!(
                                "⚠️  Merope is switched on but the Pro model is not enabled; \
                                 it stays off so onboarding and gated calls do not fall back \
                                 to the standard model"
                            );
                        }
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

                // Load OAuth provider registry (GitHub + future OIDC providers)
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

                // Install governed-text sink before scheduler / declared-API AI builtins run.
                api::tapp_runtime::install_governed_text_executor();
                tracing::info!("✅ Governed text AI executor installed");

                // Install Agent Interaction create sink before Executor handlers run.
                api::tapp_runtime::install_agent_interaction_executor();
                tracing::info!("✅ Agent interaction create executor installed");

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
                // MYR-044: single path authority via DataPaths (DATA_DIR-aware).
                let agent_data_dir = services::data_paths::paths().agent.clone();
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
                api::agent::reclaim_stranded_running_intentions(&db).await;
                {
                    let autonomy_db = db.clone();
                    tokio::spawn(async move {
                        let mut interval =
                            tokio::time::interval(std::time::Duration::from_secs(15));
                        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                        loop {
                            interval.tick().await;
                            // Share the 15s wake, not a call stack: a slow or
                            // panicking autonomy tick must not hold the next opening.
                            let work_db = autonomy_db.clone();
                            tokio::spawn(async move {
                                api::agent::tick_autonomy_work(work_db).await;
                            });
                            let speak_db = autonomy_db.clone();
                            tokio::spawn(async move {
                                crate::services::agent::merope::tick_speak_intents(speak_db).await;
                            });
                        }
                    });
                }
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
                                    let captured_exec_task = std::sync::Arc::new(
                                        tokio::sync::Mutex::new(None::<String>),
                                    );
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

                // Prune private Tapp installs when cleanup mode is "inactivity"
                {
                    let db = db.clone();
                    let dyn_cfg = GLOBAL_DYNAMIC_CONFIG.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_secs(900)).await;
                        let mut interval =
                            tokio::time::interval(std::time::Duration::from_secs(86400));
                        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                        loop {
                            interval.tick().await;
                            let (mode, days) = {
                                let cfg = dyn_cfg.read().await;
                                let mode =
                                    cfg.tapp_private_install_cleanup.trim().to_ascii_lowercase();
                                let days = i64::from(
                                    cfg.tapp_private_install_inactivity_days.clamp(1, 365),
                                );
                                (mode, days)
                            };
                            if mode != "inactivity" {
                                continue;
                            }
                            match api::tapp_store::prune_stale_private_tapps(&db, days).await {
                                Ok(n) if n > 0 => {
                                    tracing::info!(
                                        deleted = n,
                                        days,
                                        "Pruned stale private Tapp installs (inactive users)"
                                    );
                                }
                                Ok(_) => {}
                                Err(e) => {
                                    tracing::warn!(
                                        error = %e,
                                        "Failed to prune stale private Tapp installs"
                                    );
                                }
                            }
                        }
                    });
                    tracing::info!("✅ Private Tapp install cleanup worker started");
                }

                // Initialize Federation delivery worker (MFP Activity delivery queue).
                // Required for createNote/publish fan-out: rows enqueued in
                // fan_out_to_followers are drained here every ~15s.
                federation::delivery::spawn_delivery_worker(db.clone());
                tracing::info!("✅ Federation delivery worker started");

                services::qq_bot::spawn_worker();
                tracing::info!("✅ QQ bot Gateway worker started");

                // 密钥迁移：把存量明文配置与 v0 联邦私钥升级到数据密钥信封。
                //
                // 两者都幂等可重入，中断了下次启动接着做，不需要维护窗口。
                // 联邦私钥必须在这里同步做完 —— 它要随时可用于签名，不能惰性升级。
                services::data_key::log_startup_state();
                match services::data_key::migrate_plaintext_config_values(&db).await {
                    Ok(n) if n > 0 => {
                        tracing::info!("✅ Configuration encryption migration: {n} value(s)")
                    }
                    Ok(_) => {}
                    Err(e) => tracing::error!("Configuration encryption migration failed: {e}"),
                }
                match services::retired_configuration::purge_retired_configuration_keys(&db).await {
                    Ok(n) if n > 0 => {
                        tracing::info!("✅ Retired configuration purge: {n} row(s)")
                    }
                    Ok(_) => {}
                    Err(e) => tracing::error!("Retired configuration purge failed: {e}"),
                }
                let legacy_jwt_secret = {
                    let cfg = GLOBAL_CONFIG.read().await;
                    cfg.jwt_secret.clone()
                };
                match federation::keys::rewrap_legacy_private_keys(&db, &legacy_jwt_secret).await {
                    Ok(n) if n > 0 => {
                        tracing::info!("✅ Federation key rewrap: {n} key(s)")
                    }
                    Ok(_) => {}
                    Err(e) => tracing::error!("Federation key rewrap failed: {e}"),
                }

                tracing::info!("🌐 Starting in FULL MODE - all features available");
                services::tapp_registry::set_process_database(db).await;
                CONFIG_MODE.store(false, Ordering::Relaxed);
            }
            Err(e) => {
                let error_kind = db::connection::classify_connect_error(&e);
                tracing::warn!(
                    db_target = %db_target,
                    error_kind = error_kind.as_str(),
                    error = %e,
                    "⚠️  Database connection failed after retries"
                );
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

/// 路由已挂 `admin_middleware`；`AdminClaims` 把同一个检查写进签名。
async fn update_config(
    extract::AdminClaims(_claims): extract::AdminClaims,
    extract::Db(db): extract::Db,
    Json(payload): Json<api::config::ConfigResponse>,
) -> Response {
    match api::config::update_config(
        axum::extract::State(db),
        axum::extract::State(GLOBAL_DYNAMIC_CONFIG.clone()),
        Json(payload),
    )
    .await
    {
        Ok(json) => json.into_response(),
        Err(err) => err.into_response(),
    }
}

/// 路由已挂 `admin_middleware`；`AdminClaims` 把同一个检查写进签名。
async fn change_site_domain(
    extract::AdminClaims(_claims): extract::AdminClaims,
    extract::Db(db): extract::Db,
    axum::extract::State(dynamic_config): axum::extract::State<
        std::sync::Arc<tokio::sync::RwLock<crate::config::DynamicConfig>>,
    >,
    Json(payload): Json<api::site_domain::ChangeSiteDomainRequest>,
) -> Response {
    let (status, json) = api::site_domain::change_site_domain(
        axum::extract::State(db),
        axum::extract::State(dynamic_config),
        Json(payload),
    )
    .await;
    (status, json).into_response()
}

/// 路由已挂 `admin_middleware`；`AdminClaims` 把同一个检查写进签名，
/// 路由被重挂时也带不走（与 ring 端点保持一致的写法）。
async fn export_settings(
    extract::AdminClaims(claims): extract::AdminClaims,
    extract::Db(db): extract::Db,
) -> Response {
    let Ok(user_id) = claims.sub.parse::<i32>() else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "Invalid authenticated user"})),
        )
            .into_response();
    };

    let (status, json) = api::config::export_settings(axum::extract::State(db), user_id).await;
    let mut response = (status, json).into_response();
    // 导出内容含明文密钥，禁止任何缓存层留存
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

/// 路由已挂 `admin_middleware`；`AdminClaims` 把同一个检查写进签名。
/// 预演对照当前 `configurations` 活键计算保留数，不写库。
async fn preview_settings_restore(
    extract::AdminClaims(_claims): extract::AdminClaims,
    extract::Db(db): extract::Db,
    Json(payload): Json<api::config::SettingsBackup>,
) -> Response {
    let (status, json) =
        api::config::preview_settings_restore(axum::extract::State(db), Json(payload)).await;
    (status, json).into_response()
}

/// 路由已挂 `admin_middleware`；`AdminClaims` 把同一个检查写进签名。
async fn restore_settings(
    extract::AdminClaims(claims): extract::AdminClaims,
    extract::Db(db): extract::Db,
    axum::extract::State(dynamic_config): axum::extract::State<
        std::sync::Arc<tokio::sync::RwLock<crate::config::DynamicConfig>>,
    >,
    Json(payload): Json<api::config::SettingsBackup>,
) -> Response {
    let Ok(user_id) = claims.sub.parse::<i32>() else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "Invalid authenticated user"})),
        )
            .into_response();
    };

    let (status, json) = api::config::restore_settings(
        axum::extract::State(db),
        axum::extract::State(dynamic_config),
        user_id,
        Json(payload),
    )
    .await;
    (status, json).into_response()
}

async fn start_unified_server(config: AppConfig) -> anyhow::Result<()> {
    router::start_unified_server(config).await
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

#[cfg(test)]
mod schema_startup_policy_tests {
    use super::startup_schema_error;

    #[test]
    fn missing_migration_history_is_fatal() {
        let error = startup_schema_error(
            "Database migration",
            &"Migration file of version '009_digital_life_phase_three' is missing",
        );
        let message = error.to_string();
        assert!(message.contains("Refusing to start full mode"));
        assert!(message.contains("009_digital_life_phase_three"));
    }

    #[test]
    fn drift_override_never_turns_contract_failure_into_success() {
        let error = startup_schema_error(
            "Schema contract check",
            &"permission denied for relation users",
        );
        let message = error.to_string();
        assert!(message.contains("schema-drift overrides never permit normal traffic"));
        assert!(message.contains("permission denied"));
    }
}

#[cfg(test)]
mod cache_control_tests {

    use super::static_asset_cache_control;

    #[test]
    fn hashed_assets_are_immutable() {
        assert_eq!(
            static_asset_cache_control("/assets/AnimatedView-xVp24sZE.js"),
            "public, max-age=31536000, immutable"
        );
        // A hashed image under /assets/ is still immutable (hash wins over ext).
        assert_eq!(
            static_asset_cache_control("/assets/logo-abc123.png"),
            "public, max-age=31536000, immutable"
        );
    }

    #[test]
    fn unhashed_media_and_fonts_get_weeklong_ttl() {
        let expected = "public, max-age=604800, stale-while-revalidate=86400";
        assert_eq!(
            static_asset_cache_control("/icons/config/users.png"),
            expected
        );
        assert_eq!(
            static_asset_cache_control("/game-logos/starrail.png"),
            expected
        );
        assert_eq!(static_asset_cache_control("/logo.webp"), expected);
        assert_eq!(static_asset_cache_control("/favicon.webp"), expected);
        assert_eq!(static_asset_cache_control("/fonts/title.woff2"), expected);
    }

    #[test]
    fn sw_and_html_always_revalidate() {
        assert_eq!(static_asset_cache_control("/sw.js"), "no-cache");
        assert_eq!(static_asset_cache_control("/"), "no-cache");
        assert_eq!(static_asset_cache_control("/index.html"), "no-cache");
        // SPA fallback routes resolve to index.html but keep their request path.
        assert_eq!(static_asset_cache_control("/tapp/run/abc"), "no-cache");
    }

    /// 前端产物必须是压缩后下发的。此前 Cargo.toml 开着 compression-gzip/br 却
    /// 从未接过 CompressionLayer，3.2MB 的 JS/CSS 一直裸传；这里守住接线本身。
    mod static_compression {
        use axum::http::{header, Request, StatusCode};
        use std::io::Write;
        use tower::ServiceExt;
        use tower_http::compression::CompressionLayer;
        use tower_http::services::ServeDir;

        /// 建一个临时 dist 目录，放一个足够大且高度可压缩的 JS
        fn make_dist(tag: &str) -> std::path::PathBuf {
            let dir = std::env::temp_dir().join(format!("myriad-compress-test-{tag}"));
            let assets = dir.join("assets");
            std::fs::create_dir_all(&assets).expect("create temp dist");
            let mut f = std::fs::File::create(assets.join("app-deadbeef.js")).expect("create js");
            for _ in 0..400 {
                writeln!(f, "export const answer = 42; // padding padding padding").expect("write");
            }
            dir
        }

        #[tokio::test]
        async fn assets_are_brotli_encoded_and_vary() {
            let dir = make_dist("br");
            // 与 main.rs 里 fallback 的接线完全一致
            let svc = tower::Layer::layer(&CompressionLayer::new(), ServeDir::new(&dir));
            let req = Request::builder()
                .uri("/assets/app-deadbeef.js")
                .header(header::ACCEPT_ENCODING, "br")
                .body(axum::body::Body::empty())
                .unwrap();
            let res = svc.oneshot(req).await.expect("serve");
            assert_eq!(res.status(), StatusCode::OK);
            assert_eq!(
                res.headers()
                    .get(header::CONTENT_ENCODING)
                    .map(|v| v.to_str().unwrap()),
                Some("br"),
                "静态资源必须压缩下发"
            );
            // 与 Cache-Control 分层共存的前提：必须按 Accept-Encoding 分缓存
            let vary = res
                .headers()
                .get_all(header::VARY)
                .iter()
                .map(|v| v.to_str().unwrap().to_ascii_lowercase())
                .collect::<Vec<_>>()
                .join(",");
            assert!(
                vary.contains("accept-encoding"),
                "缺少 Vary: accept-encoding"
            );
            let _ = std::fs::remove_dir_all(&dir);
        }

        #[tokio::test]
        async fn plain_client_still_gets_identity() {
            let dir = make_dist("identity");
            let svc = tower::Layer::layer(&CompressionLayer::new(), ServeDir::new(&dir));
            let req = Request::builder()
                .uri("/assets/app-deadbeef.js")
                .body(axum::body::Body::empty())
                .unwrap();
            let res = svc.oneshot(req).await.expect("serve");
            assert_eq!(res.status(), StatusCode::OK);
            assert!(
                res.headers().get(header::CONTENT_ENCODING).is_none(),
                "未声明 Accept-Encoding 的客户端不应收到压缩体"
            );
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}
