//! Application state injected into Axum as the primary router `State`.
//!
//! Process-wide Lazy caches (rate limits, regex, circuit breakers) stay
//! global by design. Core request dependencies (DB, config) live here so handlers
//! and extractors can use `State<AppState>` / `FromRef`.
//!
//! # Globals policy
//!
//! - **HTTP handlers**: obtain DB via `extract::Db` or `State<DatabaseConnection>`
//! (FromRef from `AppState`). Do not call process DB helpers on request paths
//! when a connection is already available from State.
//! - **`extract::Db` on `()`**: always 503 — no process-DB fallback.
//! - **Shared DB slot**: [`AppState::db_slot`] is the **same** `Arc` as
//! [`crate::services::process_db::shared_database_slot`]. Reload/health
//! reconnect via `set_process_database` updates HTTP extractors and background
//! readers together — no dual live pools.
//! - **Background services**: may use `services::process_db::database()` when
//! no request State is available (wired at bootstrap / reload).
//! - **`GLOBAL_*` config**: shared Arcs also held on `AppState` via `from_shared`.

use std::sync::{Arc, RwLock};

use axum::extract::FromRef;
use sea_orm::DatabaseConnection;
use tokio::sync::RwLock as TokioRwLock;

use crate::config::{AppConfig, DynamicConfig};
use crate::services::process_db;

/// Shared application state for the Axum router.
#[derive(Clone)]
pub struct AppState {
    /// Same Arc as process registry — reconnect updates this slot in place.
    pub db_slot: Arc<RwLock<Option<DatabaseConnection>>>,
    pub config: Arc<TokioRwLock<AppConfig>>,
    pub dynamic_config: Arc<TokioRwLock<DynamicConfig>>,
}

impl AppState {
    pub fn new(db: DatabaseConnection, config: AppConfig, dynamic_config: DynamicConfig) -> Self {
        // Private slot; production uses `from_shared` (process registry Arc).
        Self {
            db_slot: Arc::new(RwLock::new(Some(db))),
            config: Arc::new(TokioRwLock::new(config)),
            dynamic_config: Arc::new(TokioRwLock::new(dynamic_config)),
        }
    }

    /// Share process config Arcs and the **process DB slot** (reconnect-safe).
    pub fn from_shared(
        db: DatabaseConnection,
        config: Arc<TokioRwLock<AppConfig>>,
        dynamic_config: Arc<TokioRwLock<DynamicConfig>>,
    ) -> Self {
        let db_slot = process_db::shared_database_slot();
        *db_slot
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(db);
        Self {
            db_slot,
            config,
            dynamic_config,
        }
    }

    /// Current DB handle from the shared slot (if connected).
    pub fn db(&self) -> Option<DatabaseConnection> {
        self.db_slot
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

impl FromRef<AppState> for DatabaseConnection {
    fn from_ref(state: &AppState) -> Self {
        state
            .db()
            .expect("AppState has no database connection (full-mode routes require a live DB)")
    }
}

impl FromRef<AppState> for Arc<TokioRwLock<DynamicConfig>> {
    fn from_ref(state: &AppState) -> Self {
        state.dynamic_config.clone()
    }
}

/// Process-shared dynamic config Arc (same handle as [`AppState::dynamic_config`]
/// after [`AppState::from_shared`]).
///
/// **HTTP handlers (full mode):** use `State<Arc<tokio::sync::RwLock<DynamicConfig>>>` /
/// `AppState` and pass that handle to `config_service::install_dynamic_config`.
/// Do not call this accessor from request paths.
///
/// **Allowed callers:** bootstrap (`main` / router reload), CONFIG_MODE setup
/// routes (no AppState), and non-HTTP background services that still use the
/// process cache.
pub fn shared_dynamic_config() -> &'static Arc<TokioRwLock<DynamicConfig>> {
    &crate::GLOBAL_DYNAMIC_CONFIG
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::FromRef;

    #[test]
    fn from_ref_yields_db_clone_handle() {
        fn _assert_from_ref<T: FromRef<AppState>>() {}
        _assert_from_ref::<DatabaseConnection>();
        _assert_from_ref::<Arc<TokioRwLock<DynamicConfig>>>();
    }
}
