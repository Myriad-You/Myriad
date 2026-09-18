//! Trusted persona bootstrap and owned background-driver lifecycle.
//! Started only by the persona worker or explicit development combined mode.
mod drivers;
mod heartbeat;
pub(crate) mod web_control;
pub mod worker;

use crate::{api, services};
use sea_orm::DatabaseConnection;
use services::agent;
use std::{sync::LazyLock, time::Duration};
use tokio::sync::{Mutex, watch};

static RUNTIME: Mutex<Option<drivers::Runtime>> = Mutex::const_new(None);
static FAILURE: LazyLock<watch::Sender<Option<String>>> = LazyLock::new(|| watch::channel(None).0);
static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub async fn start(db: DatabaseConnection) -> anyhow::Result<()> {
    let mut runtime = RUNTIME.lock().await;
    anyhow::ensure!(runtime.is_none(), "persona runtime is already started");
    // Initialize Agent identity system (SOUL.md / USER.md)
    // Agent 数据目录走 DataPaths（DATA_DIR-aware）。
    let agent_data_dir = services::data_paths::paths().agent.clone();
    agent::identity::init_identity(agent_data_dir.clone()).await;
    tracing::info!("✅ Agent identity system initialized");

    // Initialize Agent skill system
    agent::skill::init_skills(agent_data_dir.join("skills")).await;
    tracing::info!("✅ Agent skill system initialized");

    // Initialize Agent skill evolution system
    agent::skill_evolution::init_skill_evolution(agent_data_dir.join("skills")).await;
    tracing::info!("✅ Agent skill evolution system initialized");

    // Initialize Agent memory system
    agent::memory::init_memory(agent_data_dir.join("memory")).await;
    tracing::info!("✅ Agent memory system initialized");

    // Initialize MCP (Model Context Protocol) client
    agent::mcp::init_mcp(&agent_data_dir.join("mcp_servers.json")).await;
    tracing::info!("✅ MCP client initialized");

    // Initialize Agent task store (DB persistence + recovery)
    agent::init_task_store(db.clone()).await;
    tracing::info!("✅ Agent task store initialized");

    // Re-create run hubs + wait-loops for waiting_for_input tasks so
    // answer/subscribe work after process restart.
    api::agent::restore_waiting_runs_after_boot(&db).await;
    api::agent::reclaim_stranded_running_intentions(&db).await;
    agent::heartbeat::init_heartbeat(agent_data_dir.join("HEARTBEAT.md"))
        .await
        .map_err(anyhow::Error::msg)?;
    let mut drivers = drivers::Drivers::new();
    let stop = drivers.stopped();
    drivers.continuous(
        "channel recovery",
        services::channel_work::run_recovery_worker(),
    );
    drivers.continuous("QQ gateway", services::qq_bot::run_worker());
    drivers.continuous("Telegram bot", services::telegram_bot::run_worker());
    drivers.continuous("Discord bot", services::discord_bot::run_worker());
    drivers.continuous("Feishu bot", services::feishu_bot::run_worker());
    let work_db = db.clone();
    drivers.periodic(
        "autonomy",
        Duration::from_secs(15),
        Duration::ZERO,
        move || api::agent::tick_autonomy_work(work_db.clone()),
    );
    // Own recovery admission and shutdown alongside the other persona drivers.
    // Reattach questions only after the lease scan has committed recovery.
    let recovery_db = db.clone();
    drivers.periodic(
        "Work recovery",
        Duration::from_secs(30),
        Duration::from_secs(30),
        move || {
            let db = recovery_db.clone();
            async move {
                if let Err(error) = agent::work_loop::recover(&db).await {
                    tracing::warn!(%error, "Work recovery scan failed");
                }
                api::agent::restore_waiting_runs_after_boot(&db).await;
            }
        },
    );
    let speak_db = db.clone();
    drivers.periodic(
        "speech intents",
        Duration::from_secs(15),
        Duration::ZERO,
        move || agent::merope::tick_speak_intents(speak_db.clone()),
    );
    let expiry_db = db.clone();
    drivers.periodic("TAPP interactions", Duration::from_secs(5), Duration::ZERO, move || {
        let db = expiry_db.clone();
        async move {
            match services::tapp_agent_interaction::expire_due_interactions(&db).await {
                Ok(count) if count > 0 => tracing::info!(count, "[TAPP] Expired Agent interactions resumed"),
                Ok(_) => {},
                Err(error) => tracing::warn!(error = %error.message(), "[TAPP] Agent interaction expiry sweep failed"),
            }
        }
    });
    drivers.periodic(
        "confirmation cleanup",
        Duration::from_secs(300),
        Duration::ZERO,
        agent::cleanup_expired_confirmations,
    );
    let mut ticks = 0u64;
    drivers.periodic(
        "heartbeat",
        Duration::from_secs(60),
        Duration::ZERO,
        move || {
            ticks = ticks.wrapping_add(1);
            heartbeat::tick(db.clone(), stop.clone(), ticks.is_multiple_of(60))
        },
    );
    drivers.periodic(
        "memory and skill pruning",
        Duration::from_secs(86400),
        Duration::from_secs(3600),
        || async {
            if let Some(evolution) = agent::skill_evolution::get_skill_evolution() {
                let pruned = evolution.prune_skills().await;
                if !pruned.is_empty() {
                    tracing::info!(
                        count = pruned.len(),
                        "[SkillEvolution] Pruned low-quality skills"
                    );
                }
            }
            if let Some(memory) = agent::memory::get_memory() {
                memory.cleanup_old_logs(30).await;
            }
        },
    );
    *runtime = Some(drivers.supervise(FAILURE.clone(), Duration::from_secs(30)));
    STARTED.store(true, std::sync::atomic::Ordering::Release);
    tracing::info!("Persona background drivers started under supervision");
    Ok(())
}

pub fn background_status() -> &'static str {
    if FAILURE.borrow().is_some() {
        "failed"
    } else if STARTED.load(std::sync::atomic::Ordering::Acquire) {
        "running"
    } else {
        "stopped"
    }
}

pub(super) async fn request_stop() {
    STARTED.store(false, std::sync::atomic::Ordering::Release);
    if let Some(runtime) = RUNTIME.lock().await.as_ref() {
        runtime.request_stop();
    }
}

pub async fn shutdown() {
    STARTED.store(false, std::sync::atomic::Ordering::Release);
    if let Some(runtime) = RUNTIME.lock().await.take() {
        // Stop admission before draining. Previously the tick/heartbeat loops
        // kept spawning work while the process waited for inflight execution.
        runtime.shutdown().await;
    }
    if let Some(memory) = agent::memory::get_memory() {
        memory.force_flush().await;
    }
    if let Some(evolution) = agent::skill_evolution::get_skill_evolution() {
        evolution.flush().await;
    }
    agent::mcp::shutdown_mcp().await;
}

pub(super) async fn wait_for_failure() -> String {
    let mut receiver = FAILURE.subscribe();
    loop {
        if let Some(error) = receiver.borrow_and_update().clone() {
            return error;
        }
        if receiver.changed().await.is_err() {
            return "persona supervisor unavailable".into();
        }
    }
}
