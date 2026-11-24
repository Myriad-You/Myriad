//! An upkeep whose writing could not be held to what they asked her not to
//! keep (`upkeep_not_kept`). None of it is written, and it is noted which
//! node it was of (ids only, and when). Not to be asked about now, the talk
//! is gone over no more that night: each of its nodes would be paid for and
//! not told either; the next night that node comes last, for it may be what
//! it says. For what it says, it is tried again each night and, given up on
//! as anything never told is (`memory_jobs::GIVES_UP_AFTER_SECS`), with
//! nothing new on it meanwhile and other judgments answered, the node is
//! marked gone over with nothing of it written, till there is something new.

use sea_orm::DatabaseConnection;
use serde_json::json;

use super::{
    RowIn, upkeep_not_kept,
    write::{Beside, write_with},
};
use crate::services::agent::merope::call;
use crate::services::agent::merope::memory_jobs::GIVES_UP_AFTER_SECS;
use crate::services::her_log::pen::Thought;
use crate::services::runtime_registry as registry;

/// Where a node whose upkeep was not told is kept, as what is cut off is
/// (`upkeep_cut_off`).
pub(super) const NAMESPACE: &str = "merope_memory_node_upkeep_not_told";
/// How long it is kept at all after it was last tried.
const KEPT: i64 = 7 * 86_400;
/// Not told for what it says no more often than this, it is counted anew.
const LAPSES: i64 = 2 * 86_400;
/// The mark of a node given up on.
const NOT_TOLD: &str = "not_told";

/// A node whose upkeep was not told.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(super) struct NotTold {
    /// The id of what was newest on it then.
    seen: String,
    /// When it was last tried, Unix seconds.
    tried: i64,
    /// Since when, and when last, it was not told for what it says.
    untold: Option<(i64, i64)>,
}

impl NotTold {
    /// Not told once more `now`, for what it says or (`says`) not.
    fn again(kept: Option<Self>, seen: &str, says: bool, now: i64) -> Self {
        let untold = kept
            .filter(|kept| kept.seen == seen)
            .and_then(|kept| kept.untold)
            .filter(|(_, last)| now - last <= LAPSES);
        Self {
            seen: seen.to_string(),
            tried: now,
            untold: match says {
                true => Some((untold.map_or(now, |(since, _)| since), now)),
                false => untold,
            },
        }
    }

    /// Whether it is given up on.
    fn gives_up(&self, now: i64) -> bool {
        self.untold.is_some_and(|(since, _)| {
            now - since >= GIVES_UP_AFTER_SECS && call::judged_since(since)
        })
    }
}

/// What an upkeep's rows came to.
pub(super) enum Told {
    /// These are written.
    Write(Vec<RowIn>),
    /// None of them is: on with the talk's next node.
    Next,
    /// Nor another node of the talk tonight.
    Stop,
}

pub(super) fn key(user_id: i32, audience: &str, node: &str) -> String {
    format!("{user_id}:{audience}:{node}")
}

/// `rows` of the upkeep of `node` (the inbox's, none) in `user_id`'s
/// `audience`, `seen` newest on it, held to what they asked.
pub(super) async fn told(
    db: &DatabaseConnection,
    (user_id, audience, node): (i32, &str, Option<&str>),
    seen: &str,
    rows: Vec<RowIn>,
    pen: &Thought,
) -> Told {
    let mark = rows.last().filter(|row| row.kind == "node.upkept").cloned();
    let told = upkeep_not_kept::leave_out(db, user_id, audience, rows).await;
    let (Some(node), Some(mark)) = (node, mark) else {
        return told.map_or(Told::Stop, Told::Write);
    };
    let key = key(user_id, audience, node);
    let kept = match registry::get::<NotTold>(db, NAMESPACE, &key).await {
        Ok(kept) => kept,
        // Unread, it is as though never noted.
        Err(error) => {
            tracing::warn!(%error, "[memory-nodes] what upkeep was not told of could not be read");
            None
        }
    };
    let failure = match told {
        Ok(rows) => {
            if kept.is_some() {
                let_go(db, &key).await;
            }
            return Told::Write(rows);
        }
        Err(failure) => failure,
    };
    let now = crate::clock::now().timestamp();
    let not_told = NotTold::again(kept, seen, failure.counted().is_some(), now);
    if failure.untold() && not_told.gives_up(now) {
        let mut mark = mark;
        mark.body["fallback"] = json!(NOT_TOLD);
        if let Some(body) = mark.body.as_object_mut() {
            body.remove("wrote");
        }
        let beside = Beside::default();
        match write_with(db, user_id, audience, &[mark], Some(pen), &beside).await {
            Ok(_) => {
                tracing::warn!(
                    %failure,
                    "[memory-nodes] what an upkeep wrote was never told of: nothing of it is written"
                );
                let_go(db, &key).await;
                return Told::Next;
            }
            Err(error) => {
                tracing::warn!(%error, "[memory-nodes] an upkeep never told of could not be marked")
            }
        }
    }
    keep(db, user_id, &key, &not_told, now).await;
    tracing::info!(%failure, "[memory-nodes] upkeep could not be told of: asked again later");
    if failure.untold() {
        Told::Next
    } else {
        Told::Stop
    }
}

/// `nodes` due in `user_id`'s `audience`, those not told of last, the longest
/// ago first.
pub(super) async fn not_told_last(
    db: &DatabaseConnection,
    user_id: i32,
    audience: &str,
    nodes: &mut Vec<String>,
) {
    let mut by = Vec::new();
    for node in nodes.drain(..) {
        let kept = registry::get::<NotTold>(db, NAMESPACE, &key(user_id, audience, &node)).await;
        by.push((kept.ok().flatten().map(|kept| kept.tried), node));
    }
    by.sort_by_key(|(tried, _)| *tried);
    nodes.extend(by.into_iter().map(|(_, node)| node));
}

async fn keep(db: &DatabaseConnection, user_id: i32, key: &str, not_told: &NotTold, now: i64) {
    let identity = registry::RegistryIdentity {
        subject_id: Some(user_id),
        owner_id: Some(user_id),
        tapp_id: None,
        runtime_id: None,
    };
    let put = registry::put(db, NAMESPACE, key, identity, not_told, now + KEPT).await;
    if let Err(error) = put {
        tracing::warn!(%error, "[memory-nodes] upkeep not told of, and that could not be kept");
    }
}

async fn let_go(db: &DatabaseConnection, key: &str) {
    if let Err(error) = registry::delete(db, NAMESPACE, key).await {
        tracing::warn!(%error, "[memory-nodes] upkeep told of, what was not before not let go");
    }
}

#[cfg(test)]
#[path = "upkeep_not_told_tests.rs"]
mod tests;
