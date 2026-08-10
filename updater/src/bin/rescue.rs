//! `myriad-rescue` CLI. Does *not* depend on the updater HTTP API being alive — operates on the
//! configured state directory and Docker connection directly. See docs/updater-spec.md §16.3.

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

use myriad_updater::{config::DbMode, log as logging, rescue, self_version, state::StateDir};

#[derive(Debug, Parser)]
#[command(name = "myriad-rescue", version = self_version(), about = "Offline rescue tool for Myriad updates")]
struct Cli {
    #[arg(long, env = "UPDATER_STATE_DIR", default_value = "/state")]
    state_dir: PathBuf,

    #[arg(long, env = "UPDATER_COMPOSE_DIR", default_value = "/host/compose")]
    compose_dir: PathBuf,

    #[arg(long, env = "UPDATER_ENV_FILE", default_value = "/host/compose/.env")]
    env_file: PathBuf,

    #[arg(long, env = "UPDATER_PGDATA", default_value = "/host/compose/pgdata")]
    pgdata: PathBuf,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    /// Print current state summary.
    Status,
    /// Force-clear maintenance state. Use when updater is wedged.
    ExitMaintenance {
        #[arg(long)]
        force: bool,
    },
    /// Restore pgdata from a snapshot. Snapshots are listed by `status`.
    Rollback {
        #[arg(long)]
        snapshot: String,
    },
    /// Bundle diagnostics into a tar.gz.
    Diagnose {
        #[arg(long, default_value = "myriad-diagnostics.tar.gz")]
        output: PathBuf,
    },
    /// Forget the current in-flight job and reset to idle. Dangerous.
    ForgetJob,
    /// Remove old snapshots, keeping N most recent (default 3).
    CleanSnapshots {
        #[arg(long, default_value_t = 3)]
        keep: usize,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    logging::init();
    let cli = Cli::parse();
    // Rescue uses a lock-less StateDir so it can run while the updater container is up
    // (e.g. for `status` / `diagnose`). Destructive subcommands like `rollback` assume the
    // operator has already stopped the updater container — documented in §16.3.
    let state = StateDir::open_readonly(&cli.state_dir)?;
    let db_mode = DbMode::resolve(Some(&cli.env_file))?;
    let ctx = rescue::Context {
        state,
        compose_dir: cli.compose_dir,
        env_file: cli.env_file,
        pgdata: cli.pgdata,
        db_mode,
    };
    match cli.cmd {
        Cmd::Status => rescue::status(&ctx).await,
        Cmd::ExitMaintenance { force } => rescue::exit_maintenance(&ctx, force).await,
        Cmd::Rollback { snapshot } => rescue::rollback(&ctx, &snapshot).await,
        Cmd::Diagnose { output } => rescue::diagnose(&ctx, &output).await,
        Cmd::ForgetJob => rescue::forget_job(&ctx).await,
        Cmd::CleanSnapshots { keep } => rescue::clean_snapshots(&ctx, keep).await,
    }
}
