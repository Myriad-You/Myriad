//! Her nodes on the app (`nodes`): what she keeps on her host about the
//! owner's repositories and their talks with other AIs, made from the
//! moments their children told her (`merope::nodes`). A child that takes
//! `nodes` is shown them, kept current, and the owner can make her forget one
//! there (`node.forget`). They stay on the host; the child only shows them.

use serde_json::{Value, json};

use crate::services::agent::merope::nodes::{self, Shown};

use super::connection::Outgoing;
use super::contract::CONTRACT;

/// Room left in a frame for the envelope around the body.
const ENVELOPE_BYTES: usize = 1024;

/// The `nodes` frame: every node, the most recently changed first; when they
/// do not fit in a frame, the least recently changed go first.
pub(super) fn frame(mut shown: Vec<Shown>) -> Outgoing {
    let fits = CONTRACT
        .limits
        .max_frame_bytes
        .saturating_sub(ENVELOPE_BYTES);
    loop {
        let body = json!({ "nodes": shown });
        if shown.is_empty() || body.to_string().len() <= fits {
            return Outgoing::latest("nodes", body);
        }
        shown.pop();
    }
}

/// The owner's nodes as the child is shown them, or nothing when they could
/// not be read (it keeps what it was last shown).
pub(super) async fn current(db: &sea_orm::DatabaseConnection, owner: i32) -> Option<Outgoing> {
    match nodes::list(db, owner).await {
        Ok(shown) => Some(frame(shown)),
        Err(error) => {
            tracing::warn!(%error, "[link] could not read her nodes");
            None
        }
    }
}

/// The node a `node.forget` names: one of hers, by the id she gave it.
pub(super) fn forgotten(body: &Value) -> Option<&str> {
    body.get("nodeId")
        .and_then(Value::as_str)
        .filter(|id| super::contract::valid_id(id))
}
