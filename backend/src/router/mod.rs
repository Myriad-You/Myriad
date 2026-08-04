//! HTTP router construction (split by domain).

use super::*;

mod base;
mod authenticated;

pub(crate) async fn start_unified_server(config: AppConfig) -> anyhow::Result<()> {
    // Proxy peer allowlist hygiene (TRUST_PROXY_PEERS) — warn when too broad.
    crate::middleware::client_ip::log_proxy_trust_hygiene();

    // Build CORS layer with security-first configuration.
    // Origins live in cors_runtime so site-domain changes can hot-reload without restart.
    use tower_http::cors::AllowOrigin;

    let mut initial_origins = config.cors_origins.clone();
    if initial_origins.is_empty() {
        let is_production = AppConfig::is_production_environment();
        if is_production {
            tracing::error!("🚨 SECURITY ERROR: CORS_ORIGINS must be configured in production!");
            tracing::error!("Set CORS_ORIGINS environment variable to your frontend domain(s)");
            tracing::error!(
                "Example: CORS_ORIGINS=https://yourdomain.com,https://www.yourdomain.com"
            );
            panic!("CORS_ORIGINS is required in production mode for security");
        }
        tracing::warn!("⚠️ No CORS origins configured, using localhost-only for development");
        initial_origins = vec![
            "http://localhost:1102".into(),
            "http://localhost:1103".into(),
            "http://127.0.0.1:1102".into(),
            "http://127.0.0.1:1103".into(),
        ];
    }
    crate::middleware::cors_runtime::set_cors_origins(initial_origins.clone());
    tracing::info!("✅ CORS configured for origins: {:?}", initial_origins);

    // Custom request headers used by the SPA must be listed for cross-origin preflight.
    let cors_allowed_headers = [
        axum::http::header::CONTENT_TYPE,
        axum::http::header::AUTHORIZATION,
        axum::http::header::ACCEPT,
        axum::http::header::HeaderName::from_static("x-csrf-token"),
        axum::http::header::HeaderName::from_static("x-tapp-runtime-grant"),
        axum::http::header::HeaderName::from_static("x-requested-with"),
        // Setup wizard (already-configured instance re-init) + host locale/TZ for Tapp context.
        axum::http::header::HeaderName::from_static("x-bootstrap-token"),
        axum::http::header::HeaderName::from_static("x-myriad-locale"),
        axum::http::header::HeaderName::from_static("x-myriad-timezone"),
    ];

    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(|origin, _parts| {
            crate::middleware::cors_runtime::origin_is_allowed(origin)
        }))
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::PUT,
            axum::http::Method::DELETE,
            axum::http::Method::OPTIONS,
        ])
        .allow_headers(cors_allowed_headers)
        .allow_credentials(true);

    // Get database connection (might be None in config mode)
    let db_opt = services::tapp_registry::database().await.ok();

    // Build the unified API router. When a DB is available, wire `AppState` once
    // so `extract::Db` resolves from state. Config-mode (no DB) keeps a
    // state-less setup surface only.
    let api_router = if let Some(db) = db_opt {
        let app_state = crate::state::AppState::from_shared(
            db,
            GLOBAL_CONFIG.clone(),
            GLOBAL_DYNAMIC_CONFIG.clone(),
        );
        base::build_base_api_router(app_state.clone())
            .merge(authenticated::build_authenticated_router(app_state.clone()))
            .with_state(app_state)
    } else {
        // Setup / health only — no extract::Db routes (they require AppState).
        base::build_config_mode_router()
    };



    // Apply middleware and layers
    let api_router = api_router
        .layer(from_fn(config_mode_middleware))
        .layer(from_fn(middleware::csrf::csrf_middleware)) // CSRF 防护
        .layer(from_fn(middleware::rate_limit::rate_limit_middleware)) // Rate limiting
        // Apply security headers after the complete route graph is assembled.
        .layer(from_fn(middleware::security::security_headers_middleware))
        // Global 50 MiB default for most routes. Nested federation routers set
        // their own DefaultBodyLimit from `federation::limits` (inbox 64 MiB,
        // authenticated 80 MiB — MYR-002) which **may exceed** this outer layer —
        // see `federation::limits` tests. Do not assume 50 MiB caps public inbox.
        .layer(axum::extract::DefaultBodyLimit::max(50 * 1024 * 1024))
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
        // 前端产物此前是**未压缩**直传的（Cargo.toml 早已开了 compression-gzip/br 两个
        // feature，但从没接过 CompressionLayer）。实测 dist/assets 的 JS+CSS 合计
        // 3290KB → gzip 906KB / brotli 687KB，首屏传输量少约八成。
        // 用默认谓词即可：DefaultPredicate = SizeAbove ∧ 非 gRPC ∧ 非图片 ∧ 非 SSE，
        // 所以 text/event-stream（agent 流式）和已压缩的图片/字体不会被二次处理；
        // 压缩响应还会自动补 `Vary: accept-encoding`，与下面的 Cache-Control 分层共存。
        let serve_dir = tower::Layer::layer(
            &CompressionLayer::new(),
            ServeDir::new(&config.frontend_dist_path).not_found_service(ServeFile::new(index_html)),
        );

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
                // Resolve the cache tier before `req` is consumed by oneshot.
                let cache_control = static_asset_cache_control(path);
                use tower::ServiceExt;
                match serve_dir.oneshot(req).await {
                    Ok(res) => {
                        let mut res = res.into_response();
                        res.headers_mut().insert(
                            axum::http::header::CACHE_CONTROL,
                            axum::http::HeaderValue::from_static(cache_control),
                        );
                        res
                    }
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

                // Reload .env (cwd), then durable DATA_DIR/site_public.env last so
                // Docker volume site domain / CORS outlives compose-injected env.
                // Without this, CONFIG_RELOAD would re-read compose .env and wipe
                // the hot CORS allowlist written by site_domain.
                let env_path = std::path::PathBuf::from(".env");
                if let Err(e) = dotenvy::from_path_override(&env_path) {
                    tracing::warn!("⚠️ Failed to reload .env file: {}", e);
                } else {
                    tracing::info!("♻️ Environment variables reloaded from .env");
                }
                api::site_domain::load_durable_site_public_env();

                match AppConfig::from_env() {
                    Ok(new_config) => {
                        // Update global config FIRST for hot-reload
                        *GLOBAL_CONFIG.write().await = new_config.clone();
                        tracing::info!(
                            "♻️ Global configuration updated - AI API settings now live!"
                        );
                        // Re-sync HTTP CorsLayer from env after durable overlay
                        // (load_durable already set cors_runtime when file exists).
                        if let Ok(cors) = std::env::var("CORS_ORIGINS") {
                            crate::middleware::cors_runtime::set_cors_origins_csv(&cors);
                        }

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

                                    // Update process DB for health checks + background services
                                    services::tapp_registry::set_process_database(db.clone()).await;

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

                                    // Reload OAuth provider registry from new dynamic config
                                    services::oauth::registry::REGISTRY.reload().await;

                                    // Route table + full-mode workers are built only at process
                                    // start. Clearing CONFIG_MODE without rebuild claims "full
                                    // APIs" on a setup-only Router — exit for supervisor restart.
                                    let was_config_mode =
                                        CONFIG_MODE.load(Ordering::Relaxed);
                                    if was_config_mode {
                                        tracing::info!(
                                            "🔁 Database became available while serving the CONFIG_MODE route table; scheduling process restart for full routes"
                                        );
                                        api::setup::schedule_setup_restart();
                                    } else {
                                        tracing::info!(
                                            "♻️ Runtime configuration / database handle reloaded (route table unchanged)"
                                        );
                                    }
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

            if let Ok(db) = services::tapp_registry::database().await {
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

                        let config = GLOBAL_CONFIG.read().await;
                        if !config.database_url.is_empty() {
                            tracing::info!("🔄 Attempting to reconnect to database...");
                            match crate::db::connection::establish_connection(&config.database_url)
                                .await
                            {
                                Ok(new_db) => {
                                    services::tapp_registry::set_process_database(new_db).await;
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

