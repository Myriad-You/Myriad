//! A node with something on it and no 「现在」 of hers, built at upkeep:
//! one kept before 「现在」 was written, or one no stretch has put a fact
//! on since, so [`super::standing::after_settled`] never built it. Upkeep
//! builds each through the same path as a stretch does
//! ([`super::standing::build_waiting`]): the same lock, read again before
//! it is written.
//!
//! A node built to nothing (nothing on it stands, or the answer read as
//! nothing, or nothing on it held to go by) is still never built, so it
//! would be asked again at every upkeep. Which entries it held then is
//! kept ([`NAMESPACE`], one record per node: a digest of their row ids,
//! nothing of their words): it is asked again once they change, an entry
//! put on it or brought by a merge. An answer that never came (the provider) is
//! asked again at the next upkeep. The record goes with the node:
//! forgotten, taken back, folded into another, left empty or gone
//! ([`went`]).
//!
//! Judged here, not in the core's `build_due`: upkeep runs only on the
//! host her mind is on, never on a Mac (always a child), so no other end
//! has to judge the same; in the core every end would have to move its
//! pointer for it.

use myriad_nodes::stands::{self, BuildDueInput};
use sea_orm::{ConnectionTrait, DatabaseConnection};
use sha2::{Digest, Sha256};

use super::{Error, read};
use crate::services::runtime_registry as registry;

/// Where which entries a node built to nothing held is kept, by node.
pub(crate) const NAMESPACE: &str = "merope_standing_built_empty";
/// How long it is kept: past that, the node is asked once more.
const KEPT: i64 = 365 * 86_400;

fn key(user_id: i32, audience: &str, node: &str) -> String {
    format!("{user_id}:{audience}:{node}")
}

/// Which entries `folded` (a node as folded here) holds: a digest of
/// their row ids, those a merge brought in too; what a build reads. Not
/// upkeep's mark or her 「现在」: those change nothing a build goes by.
pub(super) fn seen(folded: &serde_json::Value) -> String {
    let mut rows: Vec<String> = read::entries_of(folded)
        .into_iter()
        .map(|entry| entry.row)
        .collect();
    rows.sort();
    hex::encode(&Sha256::digest(rows.join("\n").as_bytes())[..12])
}

/// Keeps that `node` was built to nothing holding the entries `seen`. What fails
/// is logged: the node is asked again at the next upkeep.
pub(super) async fn built_empty(
    db: &DatabaseConnection,
    user_id: i32,
    audience: &str,
    node: &str,
    seen: &str,
) {
    let kept = registry::put(
        db,
        NAMESPACE,
        &key(user_id, audience, node),
        registry::RegistryIdentity {
            subject_id: Some(user_id),
            owner_id: Some(user_id),
            tapp_id: None,
            runtime_id: None,
        },
        &seen,
        crate::clock::now().timestamp() + KEPT,
    )
    .await;
    if let Err(error) = kept {
        tracing::warn!(%error, "[memory-nodes] a node built to nothing could not be kept so");
    }
}

/// Lets go of what was kept of `node` built to nothing: it is forgotten,
/// or folded into another.
pub(super) async fn went<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
    node: &str,
) -> Result<(), Error> {
    registry::delete(db, NAMESPACE, &key(user_id, audience, node)).await?;
    Ok(())
}

/// Each topic or person of `user_id`'s `audience` that holds no 「现在」
/// of hers (the core's `build_due`), and is not one built to nothing with
/// nothing new on it since.
pub(super) async fn waiting(
    db: &DatabaseConnection,
    user_id: i32,
    audience: &str,
) -> Result<Vec<String>, Error> {
    let nodes: Vec<String> = read::heads(db, user_id, audience, None)
        .await?
        .into_iter()
        .filter(|head| matches!(head.kind.as_deref(), Some("topic") | Some("person")))
        // Not one gone, nor a rule's bare name holding nothing; any other,
        // as a stretch builds any it puts a fact on.
        .filter(|head| {
            !matches!(
                head.status.as_str(),
                "forgotten" | "retracted" | "absorbed" | "dormant" | "set_aside"
            )
        })
        .map(|head| head.node_id)
        .collect();
    if nodes.is_empty() {
        return Ok(Vec::new());
    }
    let due = stands::build_due(&BuildDueInput {
        nodes: super::standing::standing_on(db, user_id, audience, &nodes).await?,
    });
    let mut waiting = Vec::new();
    for node in due.nodes {
        let kept: Option<String> = match registry::get(
            db,
            NAMESPACE,
            &key(user_id, audience, &node),
        )
        .await
        {
            Ok(kept) => kept,
            // Unread: not asked this time rather than asked hour after
            // hour while it cannot be read.
            Err(error) => {
                tracing::warn!(%error, "[memory-nodes] a node built to nothing could not be read");
                continue;
            }
        };
        let Some(kept) = kept else {
            waiting.push(node);
            continue;
        };
        let now = read::node(db, user_id, audience, &node).await?;
        if now.is_some_and(|folded| seen(&folded) != kept) {
            waiting.push(node);
        }
    }
    Ok(waiting)
}
