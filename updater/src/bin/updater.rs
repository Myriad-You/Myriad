use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use tracing::{error, info, warn};

use myriad_updater::{
    api,
    config::{Config, DbMode},
    docker::DockerClient,
    log as logging, probe, self_version,
    state::StateDir,
    worker::{RecoveryReport, Worker, WorkerCli},
};

#[derive(Debug, Parser)]
#[command(name = "myriad-updater", version = self_version(), about = "Myriad self-update daemon")]
struct Cli {
    /// Path to state directory (bind-mounted from host).
    #[arg(long, env = "UPDATER_STATE_DIR", default_value = "/state")]
    state_dir: std::path::PathBuf,

    /// Path to the host's compose project directory (mounted at /host/compose).
    #[arg(long, env = "UPDATER_COMPOSE_DIR", default_value = "/host/compose")]
    compose_dir: std::path::PathBuf,

    /// Path to the host's .env file inside the container.
    #[arg(long, env = "UPDATER_ENV_FILE", default_value = "/host/compose/.env")]
    env_file: std::path::PathBuf,

    /// Path to the host's pgdata directory inside the container.
    /// Prefer a path *inside* the compose-dir bind mount (default) so restore can
    /// rename the directory; a dedicated mount point at `/host/pgdata` returns EBUSY.
    #[arg(long, env = "UPDATER_PGDATA", default_value = "/host/compose/pgdata")]
    pgdata: std::path::PathBuf,

    /// Listen address.
    #[arg(long, env = "UPDATER_LISTEN", default_value = "0.0.0.0:1101")]
    listen: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    logging::init();

    info!(version = self_version(), "myriad-updater starting");

    // Phase 1: load config & open state. Failures here are fatal.
    let config = Config::load_from_env().map_err(|e| {
        error!(err = %e, "failed to load updater config from environment");
        e
    })?;
    let state = Arc::new(StateDir::open(&cli.state_dir)?);

    // Security posture (R5/R6): warn only on non-default / insecure choices.
    // Secure defaults (cosign=strict, strong token) stay quiet beyond the boot audit line.
    {
        use myriad_updater::config::{cosign_verify_is_off, UPDATE_TOKEN_WARN_BELOW_LEN};
        use myriad_updater::release::CosignPolicy;

        let policy = CosignPolicy::from_env(Some(&config.cosign_verify));
        let policy_label = match policy {
            CosignPolicy::Strict => "strict",
            CosignPolicy::Soft => "soft",
            CosignPolicy::Off => "off",
        };

        match policy {
            CosignPolicy::Strict => {}
            CosignPolicy::Soft => {
                warn!(
                    cosign = policy_label,
                    "cosign verification is soft: signature failures only warn; prefer strict for production"
                );
            }
            CosignPolicy::Off => {
                // load_from_env already required the dual-key allow flag.
                error!(
                    cosign = policy_label,
                    "cosign verification is OFF (UPDATER_ALLOW_INSECURE_COSIGN accepted) — supply-chain risk"
                );
            }
        }

        let token_len = config.update_token.expose().trim().len();
        if token_len < UPDATE_TOKEN_WARN_BELOW_LEN {
            warn!(
                token_len,
                min = myriad_updater::config::UPDATE_TOKEN_MIN_LEN,
                "UPDATE_TOKEN length is barely above the minimum; prefer a longer random secret"
            );
        }

        // One audit line at boot for dangerous-config decisions (R6). Best-effort.
        let mut audit = format!(
            "audit: boot cosign={policy_label} channel={}",
            config.channel
        );
        if cosign_verify_is_off(&config.cosign_verify) {
            audit.push_str(" insecure_cosign_allowed=true");
        }
        if let Err(e) = state.append_audit(&audit) {
            warn!(err = %e, "failed to write boot audit line");
        }
    }

    // Phase 2: resolve DB mode (process env / mounted .env; default bundled).
    let db_mode = DbMode::resolve(Some(&cli.env_file)).map_err(|e| {
        error!(err = %e, "invalid MYRIAD_DB_MODE");
        e
    })?;
    info!(
        db_mode = %db_mode,
        pgdata_snapshot_enabled = db_mode.pgdata_snapshot_enabled(),
        "database mode resolved"
    );

    // Phase 3: env-probe (compose binary, docker, pgdata fs type, etc.).
    // Any unsupported environment must fail loudly *before* we serve any API.
    let env_probe = probe::run_all(&probe::ProbeInputs {
        state_dir: cli.state_dir.clone(),
        compose_dir: cli.compose_dir.clone(),
        env_file: cli.env_file.clone(),
        pgdata: cli.pgdata.clone(),
        db_mode,
    })
    .await?;
    state.write_env_probe(&env_probe)?;
    if let Some(err) = env_probe.fatal_error() {
        error!(%err, "environment probe failed: refusing to start");
        anyhow::bail!("environment probe failed: {err}");
    }
    for w in env_probe.warnings() {
        warn!(%w, "environment warning");
    }

    // Phase 4: docker client (bollard).
    let docker = Arc::new(DockerClient::connect().await?);

    // Phase 5: recover any in-flight job per §7.1.
    let recovery =
        Worker::recover_or_idle(state.clone(), docker.clone(), Some(cli.env_file.as_path()))
            .await?;
    info!(recovered = ?recovery, "state recovery complete");

    // Phase 6: spawn worker.
    let worker_cli = WorkerCli {
        state_dir: cli.state_dir.clone(),
        compose_dir: cli.compose_dir.clone(),
        env_file: cli.env_file.clone(),
        pgdata: cli.pgdata.clone(),
        listen: cli.listen.clone(),
        db_mode,
    };
    let worker = Arc::new(Worker::new(
        state.clone(),
        docker.clone(),
        config.clone(),
        worker_cli,
    ));
    // Pre-swap crash recovery only cleared maintenance; services may still be stopped.
    if matches!(recovery, RecoveryReport::ClearedPreSwap) {
        match worker.restore_stack_after_pre_swap().await {
            Ok(()) => info!("recovery: pre-swap stack restore completed"),
            Err(e) => {
                error!(
                    err = %e,
                    "recovery: pre-swap stack restore failed; site may stay down until manual compose up"
                );
                let _ =
                    state.append_history(&format!("recovery: pre-swap stack restore failed: {e}"));
            }
        }
    }
    if let Err(e) = worker.reconcile_current_deploy().await {
        warn!(err = %e, "failed to reconcile current deploy identity; continuing with persisted state");
    }
    // Compose may start the updater before backend DNS/health is ready. Retry in the background
    // so a fallback branch tag is replaced by the exact version + SHA embedded in the image.
    let reconcile_worker = worker.clone();
    tokio::spawn(async move {
        for delay_secs in [5, 15, 40] {
            tokio::time::sleep(std::time::Duration::from_secs(delay_secs)).await;
            if let Err(e) = reconcile_worker.reconcile_current_deploy().await {
                warn!(err = %e, "background deploy identity reconciliation failed");
            }
        }
    });
    let worker_handle = worker.clone().spawn();

    // Phase 7: serve API.
    let api_state = api::ApiState {
        worker: worker.clone(),
        state: state.clone(),
        config: config.clone(),
    };
    let app = api::router(api_state);

    let listener = tokio::net::TcpListener::bind(&worker.cli().listen).await?;
    info!(addr = %worker.cli().listen, "HTTP API listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    worker.shutdown().await;
    let _ = worker_handle.await;
    Ok(())
}

/// Wait for Ctrl+C or (on Unix) SIGTERM so Docker/K8s `stop` enters Axum graceful shutdown.
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
            info!("Received Ctrl+C signal");
        },
        _ = terminate => {
            info!("Received terminate signal");
        },
    }
}
