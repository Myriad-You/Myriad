//! A rule's rows and `node.standing`: written on the site's side only
//! (`Sync::Local`) while a side of hers would refuse them, as shared once
//! every side reads them.
//!
//! A core below ABI 3 refuses both, and a side gets nothing while one is to
//! go to it (`myriad_nodes::wire`), so they go only once every device of the
//! owner's that syncs her log last synced with `log3`
//! ([`relay::every_side_reads`]). Then what the site kept of them on its
//! side is written again as shared ([`share_kept`]): taken out and written
//! in one batch, in the order it was written, each under its own id. A
//! local row never left the site and took no count, so no side misses it
//! and none has it twice. Her rows among them (her cards on a session)
//! need her pen: while her mind is on another end they stay local until
//! the next time.

use myriad_nodes::Sync;
use myriad_persona_log::Row;
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::{Value, json};

use super::{
    Error,
    prepare::{Author, RowIn},
    write::{self, Beside},
};
use crate::services::her_log::{NODE, pen, relay};

/// The core ABI that reads a rule's rows and `node.standing`.
pub(crate) const READS: u32 = 3;

/// How a rule's row or a `node.standing` of `owner`'s is written now.
pub(crate) async fn sync_for<C: ConnectionTrait>(db: &C, owner: i32) -> Sync {
    match relay::every_side_reads(db, owner, READS).await {
        Ok(true) => Sync::Shared,
        Ok(false) => Sync::Local(NODE.to_string()),
        Err(error) => {
            tracing::warn!(%error, "[memory-nodes] which cores her sides read is not known; kept on the site's side");
            Sync::Local(NODE.to_string())
        }
    }
}

/// One local row to be written again.
struct Kept {
    audience: String,
    row: Row,
    node: String,
    kind: String,
    body: String,
    author: Author,
}

/// Writes again as shared what the site kept on its side only until every
/// side read it: every `node.standing`, and every row of a node a rule
/// made (a repository, a session) or a rule wrote. Nothing when a side
/// still reads less. Returns how many rows went.
pub(crate) async fn share_kept(db: &DatabaseConnection, owner: i32) -> Result<usize, Error> {
    if sync_for(db, owner).await != Sync::Shared {
        return Ok(0);
    }
    let pen = pen::think();
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT r.audience, r.node_id, r.kind, r.body, r.author, r.envelope \
             FROM agent_node_rows r \
             WHERE r.user_id = $1 AND r.dseq IS NULL AND r.origin = $2 \
             AND (r.kind = 'node.standing' OR r.author = 'rule' OR EXISTS ( \
                 SELECT 1 FROM agent_node_rows c WHERE c.user_id = r.user_id \
                 AND c.audience = r.audience AND c.node_id = r.node_id \
                 AND c.kind = 'node.created' AND c.author = 'rule'))",
            [owner.into(), NODE.into()],
        ))
        .await?;
    let mut kept = Vec::new();
    for result in rows {
        let envelope: Value = result.try_get("", "envelope")?;
        let Ok(row) = serde_json::from_value::<Row>(envelope) else {
            continue;
        };
        let author = match result.try_get::<String>("", "author")?.as_str() {
            "rule" => Author::Rule,
            "her" if pen.is_some() => Author::Her,
            _ => continue,
        };
        if row.forgotten {
            continue;
        }
        kept.push(Kept {
            audience: result.try_get("", "audience")?,
            node: result.try_get("", "node_id")?,
            kind: result.try_get("", "kind")?,
            body: result.try_get("", "body")?,
            author,
            row,
        });
    }
    kept.sort_by(|a, b| (&a.audience, &a.row.hlc).cmp(&(&b.audience, &b.row.hlc)));
    let mut shared = 0;
    let mut at = 0;
    while at < kept.len() {
        let audience = kept[at].audience.clone();
        let mut rows = Vec::new();
        let mut ids = Vec::new();
        while at < kept.len() && kept[at].audience == audience {
            let one = &kept[at];
            at += 1;
            let Ok(mut body) = serde_json::from_str::<Value>(&one.body) else {
                continue;
            };
            if body.get("sync") != Some(&json!(Sync::Local(NODE.to_string()))) {
                continue;
            }
            body["sync"] = json!(Sync::Shared);
            ids.push(one.row.id.clone());
            rows.push(RowIn {
                id: one.row.id.clone(),
                node: one.node.clone(),
                kind: one.kind.clone(),
                body,
                author: one.author,
            });
        }
        if rows.is_empty() {
            continue;
        }
        let beside = Beside {
            unshared: ids,
            ..Beside::default()
        };
        match write::write_with(db, owner, &audience, &rows, pen.as_ref(), &beside).await {
            Ok(written) => shared += written.rows.len(),
            Err(error) => {
                tracing::warn!(%error, "[memory-nodes] what was kept on the site's side is not shared yet")
            }
        }
    }
    Ok(shared)
}
