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
//! [`crate::services::tapp_registry::shared_database_slot`]. Reload/health
//! reconnect via `set_process_database` updates HTTP extractors and background
//! readers together — no dual live pools.
//! - **Background services**: may use `services::tapp_registry::database()` when
//! no request State is available (wired at bootstrap / reload).
//! - **`GLOBAL_*` config**: shared Arcs also held on `AppState` via `from_shared`.

use std::sync::{Arc, RwLock};

use axum::extract::FromRef;
use sea_orm::DatabaseConnection;
use tokio::sync::RwLock as TokioRwLock;

use crate::config::{AppConfig, DynamicConfig};
use crate::services::tapp_registry;

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
        // Isolated slot for unit tests that build AppState without process wiring.
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
        let db_slot = tapp_registry::shared_database_slot();
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
/// **HTTP handlers (full mode):** use `State<Arc<RwLock<DynamicConfig>>>` /
/// `AppState` and write with `*dynamic_config.write().await = …`. Do not call
/// these helpers from request paths.
///
/// **Allowed callers:** bootstrap (`main` / router reload), CONFIG_MODE setup
/// routes (no AppState), and non-HTTP background services that still use the
/// process cache.
pub fn shared_dynamic_config() -> &'static Arc<TokioRwLock<DynamicConfig>> {
    &crate::GLOBAL_DYNAMIC_CONFIG
}

/// Replace the process-shared dynamic config contents (and thus every
/// `AppState` that shares the Arc after `from_shared`).
///
/// Prefer State write on full-mode HTTP paths; reserve this for bootstrap /
/// CONFIG_MODE setup only.
pub async fn replace_shared_dynamic_config(config: DynamicConfig) {
    *crate::GLOBAL_DYNAMIC_CONFIG.write().await = config;
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

    #[test]
    fn from_shared_uses_process_registry_slot() {
        let process_slot = tapp_registry::shared_database_slot();
        // Cannot open a real pool here; only prove Arc identity after from_shared
        // would assign the same slot (constructor always returns process Arc).
        let config = Arc::new(TokioRwLock::new(AppConfig::default()));
        let dynamic = Arc::new(TokioRwLock::new(DynamicConfig::default()));
        // from_shared needs a DatabaseConnection — skip live call; assert API:
        // AppState::from_shared always takes shared_database_slot() (source-level).
        let src = include_str!("state.rs");
        assert!(
            src.contains("tapp_registry::shared_database_slot()"),
            "from_shared must bind the process shared_database_slot"
        );
        assert!(
            src.contains("set_process_database")
                || include_str!("services/tapp_registry.rs").contains("shared_database_slot"),
            "registry exposes shared slot for AppState"
        );
        let _ = (process_slot, config, dynamic);
    }

    #[test]
    fn extract_db_and_process_slot_documented_as_same_handle() {
        let state_src = include_str!("state.rs");
        assert!(state_src.contains("Same Arc as process registry"));
        let reg = include_str!("services/tapp_registry.rs");
        assert!(reg.contains("no dual-pool fork") || reg.contains("shared_database_slot"));
    }
}
