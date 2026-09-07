//! Federation inbox: signed Activity receive paths and activity handlers.
//!
//! Real submodules, each owning its imports:
//! - [`receipt`] — durable inbound receipts
//! - [`receive`] — HTTP `post_inbox` / `post_shared_inbox` and dispatch
//! - [`signature`] — pre-parse gate and HTTP Signature verification
//! - [`activities`] — Follow / Accept / Undo / content / Move handlers
//! - [`local_deliver`] — in-process local delivery and outbound enqueue
//! - [`mfp`] — Myriad Federation Protocol activity handlers

mod activities;
mod local_deliver;
mod mfp;
mod receipt;
mod receive;
mod signature;

pub use activities::{
    extract_accept_object_id, extract_activity_actor_id, resolve_follow_accept_target,
};
pub use local_deliver::deliver_activity_locally;
pub use receive::*;

use axum::{http::StatusCode, Json};
use serde_json::json;

use crate::federation::errors::{is_permanent_federation_error, map_inbox_handler_error};

/// Map handler errors to HTTP status; permanent peer-state mismatches → 4xx.
fn inbox_err(context: &str, e: String) -> (StatusCode, Json<serde_json::Value>) {
    if is_permanent_federation_error(&e) {
        tracing::warn!("{}: {}", context, e);
    } else {
        tracing::error!("{}: {}", context, e);
    }
    let (status, body) = map_inbox_handler_error(e);
    (status, body)
}
