//! What she keeps of what the site owner does on a child end
//! (`merope::nodes`, `agent_nodes`) on her nodes too: the site owner's
//! private talk, a topic per thing. Another coding agent's session files
//! under its repository's node (`repo:<host>/<path>`, the core's one key
//! for a remote, with the repository as its object); a conversation with
//! another AI under one node of those talks (`minted:together-chats`). What
//! she would have said or noted about a moment is a span of her own words
//! there, resting on the event it came from (`event:<id>`); the event
//! itself is never kept.
//!
//! The old rows stay as they are, and stay what the child shows. When the
//! owner has her forget a node there, its node here is forgotten with it:
//! the owner's forget of the whole node, every row of it a stub.
//!
//! Only with her pen (forgetting: always); never failing the write it follows.

use myriad_nodes::{BODY_VERSION, entries::row_id_from};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};
use serde_json::json;

use super::{Author, Error, RowIn, audience, forget, hanging, heads, homes, read, write};
use crate::services::agent::merope::nodes::{Form, Note, Place};

/// The name of the node her talks with other AIs file under.
const CHATS_NAME: &str = "和其他 AI 的对话";
/// Its key on her nodes.
pub(crate) const CHATS_KEY: &str = "minted:together-chats";

/// The key of `form`'s node of `key` on her nodes; none for a repository
/// that is not a remote (a local path names no thing on every device).
pub(crate) fn key_of(form: Form, key: &str) -> Option<String> {
    match form {
        Form::Repo => myriad_nodes::key::repo_key(key).ok(),
        Form::Together => Some(CHATS_KEY.to_string()),
    }
}

/// She noted, or had this to say, about `event` filed at `place`, for the
/// site owner's `user_id`.
pub(crate) async fn noted(
    db: &DatabaseConnection,
    user_id: i32,
    place: &Place,
    event: &str,
    note: &Note,
) {
    if user_id <= 0 {
        return;
    }
    let site_owner = hanging::site_owner(db).await;
    if let Err(error) = put(db, user_id, site_owner, place, event, note).await {
        tracing::warn!(%error, "[memory-nodes] what she made of a child's moment not on her nodes");
    }
}

/// [`noted`]: whether a span was written.
pub(crate) async fn put(
    db: &DatabaseConnection,
    user_id: i32,
    site_owner: Option<i32>,
    place: &Place,
    event: &str,
    note: &Note,
) -> Result<bool, Error> {
    let text: String = homes::clean(&note.line)
        .chars()
        .take(myriad_nodes::limits::TEXT_CHARS)
        .collect();
    let (Some(key), false) = (key_of(place.form, &place.key), text.is_empty()) else {
        return Ok(false);
    };
    let Some(pen) = crate::services::her_log::pen::think() else {
        tracing::info!("[memory-nodes] a child's moment not on her nodes: her mind is elsewhere");
        return Ok(false);
    };
    let talk = audience::private_talk(user_id, site_owner);
    let node = myriad_nodes::node_id(&talk, &key)?;
    let cite = format!("event:{}", homes::clean(event));
    let span = row_id_from("node.span", &node, &cite, "");
    let there = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT 1 AS one FROM agent_node_rows WHERE user_id = $1 AND row_id = $2",
            [user_id.into(), span.clone().into()],
        ))
        .await?
        .is_some();
    if there {
        return Ok(false);
    }
    let mut rows = Vec::new();
    if read::node(db, user_id, &talk, &node).await?.is_none() {
        let name: String = match place.form {
            Form::Repo if !place.title.trim().is_empty() => homes::clean(&place.title),
            Form::Repo => key.trim_start_matches("repo:").to_string(),
            Form::Together => CHATS_NAME.to_string(),
        }
        .chars()
        .take(myriad_nodes::limits::NAME_CHARS)
        .collect();
        let mut created = json!({
            "version": BODY_VERSION, "sync": "shared", "saidBy": ["her"],
            "kind": "topic", "key": key, "name": name,
        });
        if place.form == Form::Repo {
            created["object"] = json!({"kind": "repo", "name": name});
        }
        rows.push(RowIn {
            id: row_id_from("node.created", &node, &key, ""),
            node: node.clone(),
            kind: "node.created".into(),
            body: created,
            author: Author::Her,
        });
    }
    let at = note.at.timestamp_micros();
    rows.push(RowIn {
        id: span,
        node,
        kind: "node.span".into(),
        body: json!({
            "version": BODY_VERSION, "sync": "shared", "saidBy": ["her"], "audience": talk,
            "text": text, "refs": [cite], "from": at, "to": at,
        }),
        author: Author::Her,
    });
    write::write(db, user_id, &talk, &rows, Some(&pen)).await?;
    Ok(true)
}

/// The site owner had her forget their node of `form` and `key` on a child:
/// its node on her nodes goes too.
pub(crate) async fn forgotten(db: &DatabaseConnection, user_id: i32, form: Form, key: &str) {
    let site_owner = hanging::site_owner(db).await;
    if let Err(error) = forget_node(db, user_id, site_owner, form, key).await {
        tracing::warn!(%error, "[memory-nodes] a child's node not forgotten on her nodes");
    }
}

/// [`forgotten`]: whether there was such a node.
pub(crate) async fn forget_node(
    db: &DatabaseConnection,
    user_id: i32,
    site_owner: Option<i32>,
    form: Form,
    key: &str,
) -> Result<bool, Error> {
    let Some(key) = key_of(form, key) else {
        return Ok(false);
    };
    let talk = audience::private_talk(user_id, site_owner);
    let node = myriad_nodes::node_id(&talk, &key)?;
    let txn = db.begin().await?;
    write::lock(&txn, user_id, &talk).await?;
    let rows: Vec<_> = heads::rows_of(&txn, user_id, &talk)
        .await?
        .into_iter()
        .filter(|kept| forget::row_node(&kept.row) == node)
        .map(|kept| kept.row)
        .collect();
    if rows.is_empty() {
        txn.commit().await?;
        return Ok(false);
    }
    forget::forget_one(&txn, user_id, &talk, &node, forget::Forgets::Node).await?;
    forget::stub(&txn, user_id, &rows).await?;
    let folded = heads::fold_rows(&heads::rows_of(&txn, user_id, &talk).await?)?;
    heads::refresh(&txn, user_id, &talk, &folded).await?;
    txn.commit().await?;
    Ok(true)
}
