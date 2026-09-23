//! HTTP router construction (split by domain).

use super::*;

mod authenticated;
mod base;
mod federation_http;
mod persona_http;
pub(crate) use persona_http::build_persona_router;

pub(crate) use federation_http::build_federation_router;

#[cfg(test)]
pub(crate) fn test_api_router(state: crate::state::AppState) -> Router {
    base::build_base_api_router(state.clone())
        .merge(authenticated::build_authenticated_router(state.clone()))
        .merge(build_federation_router(state.clone()))
        .merge(build_persona_router(state.clone()))
        .with_state(state)
}

fn cors_allowed_methods() -> [axum::http::Method; 6] {
    [
        axum::http::Method::GET,
        axum::http::Method::POST,
        axum::http::Method::PUT,
        axum::http::Method::PATCH,
        axum::http::Method::DELETE,
        axum::http::Method::OPTIONS,
    ]
}

pub(crate) fn http_cors_layer() -> tower_http::cors::CorsLayer {
    use tower_http::cors::AllowOrigin;

    // Custom request headers used by the SPA must be listed for cross-origin preflight.
    let cors_allowed_headers = [
        axum::http::header::CONTENT_TYPE,
        axum::http::header::AUTHORIZATION,
        axum::http::header::ACCEPT,
        axum::http::header::HeaderName::from_static("x-csrf-token"),
        axum::http::header::HeaderName::from_static("x-tapp-runtime-grant"),
        axum::http::header::HeaderName::from_static("x-requested-with"),
        // Setup wizard passphrase + host locale/TZ for Tapp context.
        axum::http::header::HeaderName::from_static("x-setup-secret"),
        axum::http::header::HeaderName::from_static("x-myriad-locale"),
        axum::http::header::HeaderName::from_static("x-myriad-timezone"),
    ];

    tower_http::cors::CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(|origin, _parts| {
            crate::middleware::cors_runtime::origin_is_allowed(origin)
        }))
        .allow_methods(cors_allowed_methods())
        .allow_headers(cors_allowed_headers)
        .allow_credentials(true)
}

/// Final API fallback. A config-mode process only built the setup route graph,
/// so anything it did not register is "finish setup first", not "no such API".
fn unmatched_api_response(config_mode: bool, req: &Request) -> Response {
    if config_mode {
        let mut body = AppError::service_unavailable("Service in configuration mode")
            .with_message("Finish database setup first.")
            .with_hint("After configuration, the service restarts to load the full route table")
            .with_code("configuration_mode")
            .to_json();
        body["configure_endpoint"] = json!("/api/setup/database-config");
        return (StatusCode::SERVICE_UNAVAILABLE, Json(body)).into_response();
    }
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "error": "Not Found",
            "message": format!("No API route for {} {}", req.method(), req.uri().path()),
        })),
    )
        .into_response()
}

async fn installation_claimed(db: &sea_orm::DatabaseConnection) -> anyhow::Result<bool> {
    crate::services::site_owner::installation_has_owner(db)
        .await
        .map_err(|error| anyhow::anyhow!("{error}"))
}

pub(crate) async fn start_unified_server(
    config: AppConfig,
    role: crate::runtime_role::RuntimeRole,
) -> anyhow::Result<()> {
    // Proxy peer allowlist hygiene (TRUST_PROXY_PEERS) — warn when too broad.
    crate::middleware::client_ip::log_proxy_trust_hygiene();

    // Build CORS layer with security-first configuration.
    // Origins live in cors_runtime so site-domain changes can hot-reload without restart.
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

    let cors = http_cors_layer();

    // A FULL_MODE process losing its registered DB handle must not silently
    // degrade into an unauthenticated setup router.
    let db_opt = match services::tapp_registry::database() {
        Ok(db) => Some(db),
        Err(_) if CONFIG_MODE.load(Ordering::Relaxed) => None,
        Err(error) => {
            return Err(anyhow::anyhow!(
                "full-mode router cannot access process database: {error}"
            ));
        }
    };

    let data_dir = &services::data_paths::paths().root;
    if let Some(db) = db_opt.as_ref() {
        if installation_claimed(db).await? {
            api::setup_bootstrap::mark_claimed_on_disk(data_dir)
                .map_err(|error| anyhow::anyhow!("persist claimed setup marker: {error}"))?;
        } else {
            api::setup_bootstrap::init_for_setup(data_dir, true)
                .map_err(|error| anyhow::anyhow!("initialize setup capability: {error}"))?;
        }
    } else {
        api::setup_bootstrap::init_for_setup(data_dir, false)
            .map_err(|error| anyhow::anyhow!("initialize setup capability: {error}"))?;
    }

    // Build the unified API router. When a DB is available, wire `AppState` once
    // so `extract::Db` resolves from state. Config-mode (no DB) is setup + health
    // + system status + local login/me/logout + OAuth bootstrap.
    // The route graph is the only config-mode admission authority: the mode is
    // fixed here by which graph gets built, and the final API fallback reports
    // the configuration error instead of consulting a second path whitelist.
    let config_mode = db_opt.is_none();
    let api_router = if let Some(db) = db_opt {
        let app_state = crate::state::AppState::from_shared(
            db,
            GLOBAL_CONFIG.clone(),
            GLOBAL_DYNAMIC_CONFIG.clone(),
        );
        let routes = base::build_base_api_router(app_state.clone())
            .merge(authenticated::build_authenticated_router(app_state.clone()));
        let routes = if role == crate::runtime_role::RuntimeRole::All {
            routes
                .merge(build_federation_router(app_state.clone()))
                .merge(build_persona_router(app_state.clone()))
        } else {
            routes
        };
        routes
            .route(
                crate::persona::web_control::PATH,
                post(crate::persona::web_control::handle)
                    .layer(axum::extract::DefaultBodyLimit::max(
                        crate::persona::web_control::MAX_REQUEST,
                    ))
                    .layer(axum::middleware::from_fn(
                        crate::persona::web_control::admission,
                    )),
            )
            .with_state(app_state)
    } else {
        // Config-mode router — no extract::Db routes (they require AppState).
        base::build_config_mode_router()
    };

    // Apply middleware and layers
    let api_router = api_router
        // Availability gate: when the startup
        // egress-location probe says this server may not federate, the whole
        // federation path space answers 404 — public AP endpoints included.
        .layer(from_fn(
            middleware::federation_gate::federation_gate_middleware,
        ))
        .layer(from_fn(middleware::csrf::csrf_middleware)) // CSRF 防护
        .layer(from_fn(middleware::rate_limit::rate_limit_middleware)) // Rate limiting
        // Apply security headers after the complete route graph is assembled.
        .layer(from_fn(middleware::security::security_headers_middleware))
        // Global 50 MiB default for most routes. Nested federation uses from_fn
        // live_inbox_body_limit / live_authenticated_body_limit, not DefaultBodyLimit.
        .layer(axum::extract::DefaultBodyLimit::max(50 * 1024 * 1024))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        // JSON/API gzip. DefaultPredicate skips images (except SVG) and SSE, so
        // agent EventSource and /api/proxy/image stay uncompressed.
        .layer(CompressionLayer::new());

    // SPA fallback vs API-only depending on frontend_dist_path.
    let app: Router = if std::path::Path::new(&config.frontend_dist_path).exists() {
        tracing::info!("Serving frontend from: {}", config.frontend_dist_path);
        // SPA fallback: 未匹配的浏览器路由 → index.html（React Router）。
        // 重要：ServeDir 对任何非 GET/HEAD 请求直接返回 405，所以 /api/* 绝不能落到
        // 静态文件服务——未注册的 POST 应是可读的 JSON 404，不是 405。
        let index_html = std::path::Path::new(&config.frontend_dist_path).join("index.html");
        // CompressionLayer 包住 ServeDir。默认谓词跳过图片（除 SVG）和 SSE。
        let serve_dir = tower::Layer::layer(
            &CompressionLayer::new(),
            ServeDir::new(&config.frontend_dist_path).not_found_service(ServeFile::new(index_html)),
        );

        api_router.fallback(move |req: Request| {
            let serve_dir = serve_dir.clone();
            async move {
                let path = req.uri().path();
                if path.starts_with("/api/") || path == "/health" {
                    return unmatched_api_response(config_mode, &req);
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
        api_router.fallback(move |req: Request| async move {
            unmatched_api_response(config_mode, &req)
        })
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
                tracing::info!("🔄 Configuration reload detected");
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
                        let previous_database_url =
                            GLOBAL_CONFIG.read().await.database_url.clone();
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

                        let reconnect =
                            api::system::database_target_changed(
                                &previous_database_url,
                                &new_config.database_url,
                            );
                        if !reconnect {
                            tracing::info!(
                                "♻️ Database target unchanged; skipping reconnect"
                            );
                            if let Ok(db) = services::tapp_registry::database() {
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
                                services::oauth::registry::REGISTRY.reload().await;
                            }
                        } else {
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
                                    services::tapp_registry::set_process_database(db.clone());

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
                                    let was_config_mode = CONFIG_MODE.load(Ordering::Relaxed);
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
                        }
                    }
                    Err(e) => {
                        tracing::error!("❌ Failed to reload configuration: {}", e);
                    }
                }
            }
        }
    });

    // 每 60s 探测数据库与存储，并写回 /health 快照。失败会尝试重连。
    tokio::spawn(async {
        let mut health_check_interval = tokio::time::interval(tokio::time::Duration::from_secs(60));
        loop {
            health_check_interval.tick().await;

            match services::tapp_registry::database() {
                Ok(db) => {
                    if crate::db::health::probe_database(&db).await {
                        tracing::debug!("💚 Database health check passed");
                    } else {
                        tracing::error!("❌ Database health check failed");

                        let config = GLOBAL_CONFIG.read().await;
                        if !config.database_url.is_empty() {
                            tracing::info!("🔄 Attempting to reconnect to database...");
                            match crate::db::connection::establish_connection(&config.database_url)
                                .await
                            {
                                Ok(new_db) => {
                                    services::tapp_registry::set_process_database(new_db.clone());
                                    if crate::db::health::probe_database(&new_db).await {
                                        tracing::info!("✅ Database reconnected successfully");
                                    } else {
                                        tracing::error!(
                                            "❌ Reconnected handle failed the live SELECT 1 probe"
                                        );
                                    }
                                }
                                Err(e) => {
                                    crate::db::health::record_db_probe(false, false);
                                    tracing::error!("❌ Failed to reconnect to database: {}", e);
                                }
                            }
                        }
                    }
                }
                _ => {
                    crate::db::health::record_db_probe(false, false);
                }
            }

            match tokio::task::spawn_blocking(
                crate::services::data_paths::verify_runtime_storage_writable,
            )
            .await
            {
                Ok(Ok(())) => crate::db::health::record_storage_writable(true),
                Ok(Err(e)) => {
                    crate::db::health::record_storage_writable(false);
                    tracing::error!("❌ Storage writability probe failed: {}", e);
                }
                Err(e) => {
                    crate::db::health::record_storage_writable(false);
                    tracing::error!("❌ Storage writability probe join failed: {}", e);
                }
            }
        }
    });

    // Start server with the app (convert to service within start_server)
    start_server(config, app).await
}

#[cfg(test)]
mod cors_method_tests {
    use super::{cors_allowed_methods, http_cors_layer};
    use axum::Router;
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode, header};
    use axum::routing::patch;
    use tower::ServiceExt;

    #[test]
    fn cors_allows_patch() {
        assert!(cors_allowed_methods().contains(&Method::PATCH));
    }

    #[tokio::test]
    async fn preflight_allows_patch_on_user_and_session_routes() {
        crate::middleware::cors_runtime::set_cors_origins(vec!["https://cors-patch.test".into()]);
        let app = Router::new()
            .route("/api/admin/users/{id}", patch(|| async { StatusCode::OK }))
            .route(
                "/api/agent/sessions/{session_id}",
                patch(|| async { StatusCode::OK }),
            )
            .layer(http_cors_layer());

        for path in ["/api/admin/users/1", "/api/agent/sessions/ses_1"] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(Method::OPTIONS)
                        .uri(path)
                        .header(header::ORIGIN, "https://cors-patch.test")
                        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "PATCH")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .expect("preflight");
            assert_eq!(response.status(), StatusCode::OK, "{path}");
            let allow_origin = response
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .and_then(|value| value.to_str().ok())
                .unwrap_or("");
            assert_eq!(allow_origin, "https://cors-patch.test", "{path}");
            let allow_methods = response
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_METHODS)
                .and_then(|value| value.to_str().ok())
                .unwrap_or("");
            assert!(
                allow_methods.to_ascii_uppercase().contains("PATCH"),
                "{path} preflight methods: {allow_methods}"
            );
        }
    }
}

#[cfg(test)]
mod api_compression_tests {
    use super::CompressionLayer;
    use axum::Json;
    use axum::Router;
    use axum::body::Body;
    use axum::http::{Request, StatusCode, header};
    use axum::routing::get;
    use serde_json::json;
    use tower::ServiceExt;

    #[tokio::test]
    async fn json_api_responses_are_gzip_encoded() {
        let padding = "library-item-".repeat(40);
        let app = Router::new()
            .route(
                "/api/ping",
                get({
                    let padding = padding.clone();
                    move || {
                        let padding = padding.clone();
                        async move { Json(json!({ "ok": true, "padding": padding })) }
                    }
                }),
            )
            .layer(CompressionLayer::new());
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/ping")
                    .header(header::ACCEPT_ENCODING, "gzip")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("gzip json");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_ENCODING)
                .and_then(|value| value.to_str().ok()),
            Some("gzip"),
            "JSON API must gzip when the client accepts it"
        );
    }
}

#[cfg(test)]
mod config_mode_fallback_tests {
    use super::{base, unmatched_api_response};
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    fn config_mode_app() -> axum::Router {
        base::build_config_mode_router().fallback(|req: axum::extract::Request| async move {
            unmatched_api_response(true, &req)
        })
    }

    async fn status(uri: &str) -> (StatusCode, Vec<u8>) {
        let res = config_mode_app()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = res.status();
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, bytes.to_vec())
    }

    #[tokio::test]
    async fn config_mode_only_reaches_registered_setup_routes() {
        let (code, bytes) = status("/api/posts").await;
        assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE);
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["code"], "configuration_mode");
        assert_eq!(body["configure_endpoint"], "/api/setup/database-config");

        // A prefix lookalike of a setup route is not admitted by path prefix.
        let (code, _) = status("/api/setup/unknown").await;
        assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE);

        let (code, _) = status("/health").await;
        assert_ne!(code, StatusCode::SERVICE_UNAVAILABLE);
    }

    #[test]
    fn full_mode_unmatched_api_is_not_found() {
        let req = Request::builder()
            .uri("/api/nope")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            unmatched_api_response(false, &req).status(),
            StatusCode::NOT_FOUND
        );
    }
}
