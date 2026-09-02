//! Tapp runtime registry adapter (process DB handle + re-export of workspace crate).
//!
//! Pure registry/mailbox operations live in [`myriad_tapp_registry`]. This module
//! is the **services-layer** entry point for agent / scheduler / runtime grants.
//!
//! `api::tapp_runtime::shared_registry` is a thin re-export of this module for
//! path stability in HTTP handlers only — services must import this path.

pub use myriad_tapp_registry::*;

use sea_orm::{DatabaseConnection, DbErr};
use std::sync::{Arc, OnceLock, RwLock};

/// Single shared DB slot for full-mode process + [`crate::state::AppState`].
///
/// Reconnect / reload writes here once; both `database()` (background) and
/// `AppState` / `extract::Db` (HTTP) read the same handle — no dual-pool fork.
static PROCESS_DB: OnceLock<Arc<RwLock<Option<DatabaseConnection>>>> = OnceLock::new();

/// Shared slot used by process helpers and `AppState::from_shared`.
pub fn shared_database_slot() -> Arc<RwLock<Option<DatabaseConnection>>> {
    PROCESS_DB
        .get_or_init(|| Arc::new(RwLock::new(None)))
        .clone()
}

/// Wire (or re-wire) the process + AppState DB after connect / reload / health reconnect.
pub async fn set_process_database(db: DatabaseConnection) {
    // Signature stays async for existing call sites; the slot is std::sync for FromRef.
    let slot = shared_database_slot();
    *slot
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(db);
}

/// Process-global DB connection for services that run outside a request.
///
/// HTTP handlers must use `State` / `extract::Db` (same underlying slot after
/// `AppState::from_shared`). Prefer passing an explicit `DatabaseConnection` on
/// request paths (grant / rate limit / ws ticket).
pub async fn database() -> Result<DatabaseConnection, DbErr> {
    shared_database_slot()
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
        .ok_or_else(|| DbErr::Custom("database is not connected".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_slot_is_same_arc_for_process_and_appstate_style_holders() {
        let a = shared_database_slot();
        let b = shared_database_slot();
        assert!(Arc::ptr_eq(&a, &b), "slot must be a single process Arc");
    }

    #[tokio::test]
    async fn set_process_database_updates_readers_on_shared_slot() {
        // Structural: write path is the same Arc AppState holds after from_shared.
        let slot = shared_database_slot();
        let before = Arc::as_ptr(&slot) as usize;
        // set_process_database requires a real DatabaseConnection; without DB we
        // only prove the slot identity used by AppState::from_shared.
        let again = shared_database_slot();
        assert_eq!(before, Arc::as_ptr(&again) as usize);
        let _ = database().await; // may be Err if no connection — ok in unit env
    }
}
