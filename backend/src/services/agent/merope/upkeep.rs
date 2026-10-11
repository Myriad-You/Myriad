//! Upkeep of what the site keeps of her, apart from her own life: her life
//! (`life`) is her parent end's, and pauses while that end is a device or
//! offline (Q6); this goes on regardless, since it only keeps the site's
//! records in shape.
//!
//! - **Her affect changes.** Once a day, each person's recorded changes
//!   older than they are kept are folded into where replay starts and then
//!   dropped (`store::fold_old_affect_changes`): the owner's after
//!   [`OWNER_CHANGES_KEPT`](super::store::OWNER_CHANGES_KEPT), everyone
//!   else's after [`OTHERS_CHANGES_KEPT`](super::store::OTHERS_CHANGES_KEPT).

use std::sync::{LazyLock, Mutex};

use chrono::NaiveDate;
use sea_orm::DatabaseConnection;

/// The day her affect changes were last folded, so a day folds once.
static FOLDED_ON: LazyLock<Mutex<Option<NaiveDate>>> = LazyLock::new(|| Mutex::new(None));

pub async fn tick(db: DatabaseConnection) {
    let today = super::clock::today();
    if FOLDED_ON.lock().is_ok_and(|folded| *folded == Some(today)) {
        return;
    }
    // The owner's are kept apart from everyone else's; without knowing who
    // that is, nothing is folded until it is known.
    let Some(owner) = super::call::site_owner().await else {
        return;
    };
    match super::store::fold_old_affect_changes(&db, Some(owner)).await {
        Ok(folded) => {
            if folded > 0 {
                tracing::info!(folded, "[Merope] old affect changes folded");
            }
            if let Ok(mut done) = FOLDED_ON.lock() {
                *done = Some(today);
            }
        }
        // Tried again at the next tick.
        Err(error) => tracing::warn!(%error, "[Merope] could not fold old affect changes"),
    }
}
