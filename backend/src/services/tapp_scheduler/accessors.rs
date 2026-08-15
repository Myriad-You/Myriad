// Process-wide scheduler engine handle.

use super::types_frontend::TappSchedulerEngine;

static SCHEDULER_ENGINE: once_cell::sync::OnceCell<
    std::sync::Arc<tokio::sync::RwLock<TappSchedulerEngine>>,
> = once_cell::sync::OnceCell::new();

/// Process-wide scheduler engine handle.
pub fn try_scheduler_engine() -> Option<std::sync::Arc<tokio::sync::RwLock<TappSchedulerEngine>>> {
    SCHEDULER_ENGINE.get().cloned()
}

/// Process-wide scheduler engine handle, or error string if not started.
pub fn scheduler_engine() -> Result<std::sync::Arc<tokio::sync::RwLock<TappSchedulerEngine>>, String>
{
    try_scheduler_engine().ok_or_else(|| "Scheduler not initialized".to_string())
}

/// Initialize the process-wide scheduler engine.
pub async fn init_scheduler(db: sea_orm::DatabaseConnection) {
    let engine = TappSchedulerEngine::new(db);
    engine.start().await;
    let _ = SCHEDULER_ENGINE.set(std::sync::Arc::new(tokio::sync::RwLock::new(engine)));
    tracing::info!("[TappScheduler] Scheduler initialized");
}

/// Shut down the process-wide scheduler engine.
pub async fn shutdown_scheduler() {
    if let Some(engine) = SCHEDULER_ENGINE.get() {
        engine.write().await.stop().await;
        tracing::info!("[TappScheduler] Scheduler shut down");
    }
}
