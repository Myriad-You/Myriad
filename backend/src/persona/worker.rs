//! Persona HTTP and background runtime. Web owns migrations and platform schedulers.
use std::{sync::Arc, time::Duration};

use axum::{Json, Router, extract::State, http::StatusCode, routing::get};
use sea_orm::{ConnectOptions, ConnectionTrait, Database, DatabaseConnection};
use serde_json::json;
use tokio::sync::watch;

use crate::{config::AppConfig, services::config_service::ConfigService};

#[derive(Clone)]
struct HealthState {
    db: DatabaseConnection,
    configured: Arc<std::sync::atomic::AtomicBool>,
}

fn connection_options(url: &str) -> ConnectOptions {
    let mut options = ConnectOptions::new(url.to_owned());
    options
        .min_connections(1)
        .max_connections(8)
        .connect_timeout(Duration::from_secs(10))
        .acquire_timeout(Duration::from_secs(5))
        .idle_timeout(Duration::from_secs(60))
        .sqlx_logging(false)
        .map_sqlx_postgres_opts(|opts| {
            opts.application_name("myriad-persona-worker").options([
                ("statement_timeout", "30000"),
                ("lock_timeout", "3000"),
                ("idle_in_transaction_session_timeout", "10000"),
                ("transaction_timeout", "35000"),
                ("work_mem", "4MB"),
                ("max_parallel_workers_per_gather", "0"),
            ])
        });
    options
}

pub async fn run() -> anyhow::Result<()> {
    use std::sync::atomic::{AtomicBool, Ordering};
    crate::services::data_key::init_existing()?;
    let config = AppConfig::from_env()?;
    anyhow::ensure!(
        !config.database_url.is_empty(),
        "persona-worker requires DATABASE_URL"
    );
    *crate::GLOBAL_CONFIG.write().await = config.clone();
    let db = Database::connect(connection_options(&config.database_url)).await?;
    crate::db::worker_policy::verify(&db, crate::db::worker_policy::WorkerKind::Persona).await?;
    // The web process owns migrations. Starting a worker against an incomplete
    // deployment fails before the first claim; the supervisor can retry later.
    let drift = crate::db::schema_check::report_schema_drift(&db).await?;
    anyhow::ensure!(
        drift.is_empty(),
        "persona-worker requires the current schema: {}",
        drift.summary()
    );
    crate::SCHEMA_READY.store(true, Ordering::Release);
    crate::services::process_db::set_process_database(db.clone());
    *crate::GLOBAL_DYNAMIC_CONFIG.write().await =
        ConfigService::new(db.clone()).load_config().await?;
    crate::services::agent::notifications::init_notifications(db.clone()).await;
    super::start(db.clone()).await?;

    let configured = Arc::new(AtomicBool::new(true));
    crate::middleware::cors_runtime::set_cors_origins(config.cors_origins.clone());
    let app_state = crate::state::AppState::from_shared(
        db.clone(),
        crate::GLOBAL_CONFIG.clone(),
        crate::GLOBAL_DYNAMIC_CONFIG.clone(),
    );
    let domain = crate::router::build_persona_router(app_state.clone())
        .with_state(app_state)
        .layer(axum::middleware::from_fn(
            crate::middleware::csrf::csrf_middleware,
        ))
        .layer(axum::middleware::from_fn(
            crate::middleware::rate_limit::rate_limit_middleware,
        ))
        .layer(axum::middleware::from_fn(
            crate::middleware::security::security_headers_middleware,
        ))
        .layer(axum::extract::DefaultBodyLimit::max(50 * 1024 * 1024))
        .layer(crate::router::http_cors_layer())
        .layer(axum::middleware::from_fn_with_state(
            Budgets::default(),
            limit_http_work,
        ));
    let state = HealthState {
        db: db.clone(),
        configured: configured.clone(),
    };
    let address = format!("{}:{}", config.server_host, config.server_port);
    let listener = tokio::net::TcpListener::bind(&address).await?;
    let (shutdown, mut stopped) = watch::channel(false);
    let app = Router::new()
        .route("/health", get(health))
        .with_state(state)
        .merge(domain);
    let mut http = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .with_graceful_shutdown(async move {
            let _ = stopped.changed().await;
        })
        .await
    });
    let mut refresh = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(15));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            // Public domain changes persist in the shared read-only data mount.
            // Reload both origin and DB settings independently of the web process.
            crate::services::site_public_env::load_durable_site_public_env();
            let core = AppConfig::from_env()?;
            crate::middleware::cors_runtime::set_cors_origins(core.cors_origins.clone());
            *crate::GLOBAL_CONFIG.write().await = core;
            match ConfigService::new(db.clone()).load_config().await {
                Ok(config) => {
                    *crate::GLOBAL_DYNAMIC_CONFIG.write().await = config;
                    configured.store(true, Ordering::Release);
                }
                Err(error) => {
                    configured.store(false, Ordering::Release);
                    // Stop claims instead of continuing indefinitely with stale policy.
                    return Err::<(), anyhow::Error>(error);
                }
            }
        }
    });
    tracing::info!(%address, "Persona HTTP and background process ready");
    let result = tokio::select! {
        signal = termination_signal() => signal,
        error = super::wait_for_failure() => Err(anyhow::anyhow!(error)),
        result = &mut http => Err(anyhow::anyhow!("persona HTTP server stopped: {result:?}")),
        result = &mut refresh => Err(anyhow::anyhow!("persona config refresh stopped: {result:?}")),
    };
    super::request_stop().await;
    let _ = shutdown.send(true);
    refresh.abort();
    if !http.is_finished() {
        if tokio::time::timeout(Duration::from_secs(5), &mut http)
            .await
            .is_err()
        {
            http.abort();
        }
    }
    super::shutdown().await;
    crate::services::jobs::shutdown(crate::services::jobs::SHUTDOWN_DRAIN).await;
    result
}

/// Admission is immediate and capacity stays held until the response stream is
/// consumed/dropped. Slow uploads/downloads cannot leave an unbounded waiter list.
async fn limit_http_work(
    State(budgets): State<Budgets>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    use futures::StreamExt;
    let path = request.uri().path();
    let passive_stream = request.method() == axum::http::Method::GET
        && (path.ends_with("/stream") || path == "/api/speech/convo/events");
    let control = path == "/api/agent/presence"
        || path.ends_with("/cancel")
        || matches!(
            path,
            "/api/speech/convo/stop" | "/api/speech/convo/interrupt"
        );
    let budget = if passive_stream {
        budgets.streams
    } else if control {
        budgets.controls
    } else {
        budgets.requests
    };
    let Ok(permit) = budget.try_acquire_owned() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let response = match tokio::time::timeout(Duration::from_secs(600), next.run(request)).await {
        Ok(response) => response,
        Err(_) => return StatusCode::GATEWAY_TIMEOUT.into_response(),
    };
    let (parts, body) = response.into_parts();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3600);
    let inner = futures::stream::unfold(Some(body.into_data_stream()), move |body| async move {
        let mut body = body?;
        let next = tokio::time::timeout_at(deadline, body.next()).await;
        return match next {
            Ok(Some(item)) => Some((item.map_err(std::io::Error::other), Some(body))),
            Ok(None) => None,
            Err(_) => Some((
                Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "persona response deadline",
                )),
                None,
            )),
        };
    });
    let stream = crate::held_stream::HeldStream::new(inner, permit);
    axum::response::Response::from_parts(parts, axum::body::Body::from_stream(stream))
}

async fn health(State(state): State<HealthState>) -> (StatusCode, Json<serde_json::Value>) {
    use std::sync::atomic::Ordering;
    let database = matches!(
        tokio::time::timeout(Duration::from_secs(2), state.db.ping()).await,
        Ok(Ok(()))
    );
    let ready = database
        && state.configured.load(Ordering::Acquire)
        && super::background_status() == "running";
    (
        if ready {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        },
        Json(json!({
            "role": "persona-worker", "ready": ready, "database": database,
            "version": myriad_process_info::build_version(), "commit_sha": myriad_process_info::build_commit_sha(),
            "persona_background": super::background_status(),
        })),
    )
}

async fn termination_signal() -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! { result = tokio::signal::ctrl_c() => result?, _ = term.recv() => {} }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c().await?;
    Ok(())
}

#[derive(Clone)]
struct Budgets {
    requests: Arc<tokio::sync::Semaphore>,
    streams: Arc<tokio::sync::Semaphore>,
    controls: Arc<tokio::sync::Semaphore>,
}
impl Default for Budgets {
    fn default() -> Self {
        Self {
            requests: Arc::new(tokio::sync::Semaphore::new(8)),
            streams: Arc::new(tokio::sync::Semaphore::new(32)),
            controls: Arc::new(tokio::sync::Semaphore::new(8)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, Bytes},
        extract::Request,
    };
    use tower::ServiceExt;
    #[tokio::test]
    async fn stalled_work_and_sse_leave_cancel_admission_available() {
        let budget = Budgets {
            requests: Arc::new(tokio::sync::Semaphore::new(1)),
            streams: Arc::new(tokio::sync::Semaphore::new(1)),
            controls: Arc::new(tokio::sync::Semaphore::new(1)),
        };
        let app = Router::new()
            .route(
                "/api/agent/process",
                axum::routing::post(|| async {
                    Body::from_stream(futures::stream::pending::<Result<Bytes, std::io::Error>>())
                }),
            )
            .route(
                "/api/agent/notifications/stream",
                get(|| async {
                    Body::from_stream(futures::stream::pending::<Result<Bytes, std::io::Error>>())
                }),
            )
            .route(
                "/api/agent/tasks/id/cancel",
                axum::routing::post(|| async { StatusCode::OK }),
            )
            .layer(axum::middleware::from_fn_with_state(
                budget.clone(),
                limit_http_work,
            ));
        let request = |method, path| {
            Request::builder()
                .method(method)
                .uri(path)
                .body(Body::empty())
                .unwrap()
        };
        let work = app
            .clone()
            .oneshot(request("POST", "/api/agent/process"))
            .await
            .unwrap();
        let stream = app
            .clone()
            .oneshot(request("GET", "/api/agent/notifications/stream"))
            .await
            .unwrap();
        assert_eq!(budget.requests.available_permits(), 0);
        assert_eq!(budget.streams.available_permits(), 0);
        assert_eq!(
            app.clone()
                .oneshot(request("POST", "/api/agent/process"))
                .await
                .unwrap()
                .status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(
            app.clone()
                .oneshot(request("POST", "/api/agent/tasks/id/cancel"))
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
        drop(work);
        drop(stream);
        assert_eq!(budget.requests.available_permits(), 1);
        assert_eq!(budget.streams.available_permits(), 1);
    }
}
