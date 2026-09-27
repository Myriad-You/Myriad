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

pub use local_deliver::deliver_activity_locally;
pub use receive::*;

use axum::{Json, http::StatusCode};

use crate::federation::errors::{is_permanent_federation_error, map_inbox_handler_error};

/// Local side effects that must only run after the inbox transaction that
/// produced them has committed. Dropped on rollback.
///
/// - live-UI broadcasts (FileTransfer progress);
/// - replies addressed to a same-instance inbox (the Accept for a local
///   Follow): the delivery worker refuses localhost / private targets, so they
///   are handed to the in-process path once the triggering activity is durable.
#[derive(Default)]
pub(crate) struct PostCommit {
    notices: Vec<crate::federation::file_transfer::TransferNotice>,
    local_replies: Vec<LocalReply>,
}

struct LocalReply {
    user_id: i32,
    activity: crate::federation::types::Activity,
    target_inbox: String,
}

impl PostCommit {
    pub(crate) fn push(
        &mut self,
        notice: Option<crate::federation::file_transfer::TransferNotice>,
    ) {
        self.notices.extend(notice);
    }

    pub(crate) fn reply_locally(
        &mut self,
        user_id: i32,
        activity: crate::federation::types::Activity,
        target_inbox: String,
    ) {
        self.local_replies.push(LocalReply {
            user_id,
            activity,
            target_inbox,
        });
    }

    pub(crate) async fn run(self, db: &sea_orm::DatabaseConnection) {
        for notice in self.notices {
            notice.broadcast().await;
        }
        for reply in self.local_replies {
            // `enqueue_delivery` records the reply, delivers it in-process and
            // falls back to the HTTP queue itself; the inbound activity is
            // already committed, so a failure here only leaves the reply queued.
            let delivered = local_deliver::enqueue_delivery(
                db,
                reply.user_id,
                &reply.activity,
                &reply.target_inbox,
            )
            .await;
            if let Err((status, body)) = delivered {
                tracing::error!(
                    %status,
                    error = %body.0,
                    activity_type = %reply.activity.activity_type,
                    target = %reply.target_inbox,
                    "Failed to record a same-instance inbox reply"
                );
            }
        }
    }
}

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
