//! What is going on on a child end, told as it changes (the `now` family):
//! the owner's coding-agent sessions there. A `now.session` is one
//! session's whole picture when it changed; a `now.drop` ends the grant the
//! owner gave the device to watch their sessions, and what was kept of it
//! goes for good (`merope::sessions`).
//!
//! The body is read by the core's rules (`myriad_her_link::now`), so the
//! site and the child refuse the same bodies. It is written, then answered
//! `now.taken`; a record of each taken id (two days, longer than a child
//! keeps one) answers a resend without taking it in twice.

use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::services::runtime_registry::{self as registry, RegistryIdentity};

use super::connection::Outgoing;

const NAMESPACE: &str = "her_link_now";
/// How long a taken id is remembered: a child keeps one a day at most.
const RETENTION_SECS: i64 = 2 * 24 * 60 * 60;

#[derive(Serialize, Deserialize)]
struct Taken {
    id: String,
}

fn record_id(device_id: &str, id: &str) -> String {
    format!("{device_id}:{id}")
}

pub(super) fn taken_frame(id: &str) -> Outgoing {
    Outgoing::latest("now.taken", json!({ "id": id }))
}

/// Whether this device's `now` frame `id` was already written.
pub(super) async fn was_taken(db: &DatabaseConnection, device_id: &str, id: &str) -> bool {
    match registry::get::<Taken>(db, NAMESPACE, &record_id(device_id, id)).await {
        Ok(found) => found.is_some_and(|taken| taken.id == id),
        Err(error) => {
            tracing::warn!(%error, device = %device_id, "[link] could not read a now record");
            false
        }
    }
}

/// Records the frame as taken.
pub(super) async fn record(
    db: &DatabaseConnection,
    owner: i32,
    device_id: &str,
    id: &str,
) -> Result<(), sea_orm::DbErr> {
    registry::put(
        db,
        NAMESPACE,
        &record_id(device_id, id),
        RegistryIdentity {
            subject_id: Some(owner),
            owner_id: Some(owner),
            tapp_id: None,
            runtime_id: Some(device_id),
        },
        &Taken { id: id.to_string() },
        crate::clock::now().timestamp() + RETENTION_SECS,
    )
    .await
}
