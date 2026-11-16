//! What the site owner may change on the page of her nodes (decision N9),
//! and nothing else: delete a node or one entry on it, rename a node,
//! cancel one of her intents, answer what was heard about them.
//!
//! - **Delete** is the person's forget, as everywhere on the site: an
//!   entry's memory row is forgotten truly ([`manage::forget_any`]: the row,
//!   its earlier versions, the words it came from), which forgets its entry
//!   with it ([`super::forget`]); an entry naming no memory still held is
//!   forgotten by a forget of its row and becomes the core's stub. A node:
//!   every memory on it so, then a forget of the node, its rows stubs and
//!   the heads folded again; what the site kept of an instruction on it
//!   goes too. A person's own node (「关于你」, 「她自己」, someone in a
//!   group) is not deleted whole: it is where they are kept, and is made
//!   again the next time; its entries are deleted one by one.
//! - **Rename** is the owner's row (`Author::Owner`, said by them), written
//!   against the name as it stands. Her upkeep never changes it back: the
//!   core only revises a node's status and summary, and holds anything that
//!   would change or merge away what the partner set (`partnerSet`).
//! - **Cancel** settles a waiting intent as dropped, her row with her pen,
//!   as their words calling it off do (`intending::call_off`), recorded as
//!   `cancelled`; a thread's or a want's row goes with it.
//! - **Heard**: 「不是」 deletes it as an entry is deleted (void, never taken
//!   again). 「是这样」 needs where an entry came from, which the site does
//!   not keep yet: refused. Nothing is heard on the site until it does.
//!
//! Each is refused while she is not on the site (the caller asks), and for
//! an audience not on the owner's page ([`page::on_the_page`]).

use myriad_nodes::{Kind, entries::row_id_from};
use myriad_persona_log::Row;
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};
use serde_json::{Value, json};

use super::forget::{self, Forgets};
use super::{
    Author, Error, RowIn, heads, home_intents, intending, intent_store, page, read, write,
};
use crate::services::agent::memory::manage;

/// Why an edit was not made. Never says with what.
#[derive(Debug)]
pub(crate) enum Refusal {
    /// Not on the owner's page, or not there.
    NotFound,
    /// Not allowed as asked: a code the page words.
    No(&'static str),
    Failed(Error),
}

impl From<Error> for Refusal {
    fn from(error: Error) -> Self {
        Self::Failed(error)
    }
}

impl From<sea_orm::DbErr> for Refusal {
    fn from(error: sea_orm::DbErr) -> Self {
        Self::Failed(Error::Db(error))
    }
}

/// The live head of `node` on the owner's page.
async fn live(
    db: &DatabaseConnection,
    owner: i32,
    audience: &str,
    node: &str,
) -> Result<Value, Refusal> {
    if !page::on_the_page(audience) {
        return Err(Refusal::NotFound);
    }
    let head = read::node(db, owner, audience, node)
        .await?
        .ok_or(Refusal::NotFound)?;
    let gone = head["forgotten"].as_bool().unwrap_or(false)
        || head["absorbedInto"].is_object()
        || head["retracted"]
            .as_array()
            .is_some_and(|retracted| !retracted.is_empty());
    if gone {
        return Err(Refusal::NotFound);
    }
    Ok(head)
}

/// Renames `node` to `name`, the owner's row.
pub(crate) async fn rename(
    db: &DatabaseConnection,
    owner: i32,
    audience: &str,
    node: &str,
    name: &str,
) -> Result<(), Refusal> {
    let head = live(db, owner, audience, node).await?;
    let name = name.trim();
    if name.is_empty() || name.chars().count() > myriad_nodes::limits::NAME_CHARS {
        return Err(Refusal::No("bad_name"));
    }
    if head["name"]["value"].as_str() == Some(name) {
        return Ok(());
    }
    let base: Vec<String> =
        serde_json::from_value(head["name"]["base"].clone()).unwrap_or_default();
    let sync = match &head["name"]["sync"] {
        Value::Null => json!("shared"),
        sync => sync.clone(),
    };
    let row = RowIn {
        id: row_id_from(
            "node.revised",
            node,
            &format!("owner-name:{}", base.join(",")),
            name,
        ),
        node: node.to_string(),
        kind: "node.revised".into(),
        body: json!({
            "version": myriad_nodes::BODY_VERSION, "sync": sync, "saidBy": ["partner"],
            "base": base, "field": "name", "value": name,
        }),
        author: Author::Owner,
    };
    match write::write(db, owner, audience, &[row], None).await {
        Ok(_) => Ok(()),
        Err(Error::Refused(_)) => Err(Refusal::No("bad_name")),
        Err(error) => Err(error.into()),
    }
}

/// The memory rows an entry names, without `memory:`.
fn memories_of(entry: &Value) -> Vec<String> {
    super::shown::cited(&entry["refs"])
}

/// Forgets the rows `ids` of the owner's `audience` by forgets of their own
/// (each on the node it was written on), as stubs, and folds the heads
/// again: in one transaction under the audience's lock.
async fn forget_rows(
    db: &DatabaseConnection,
    owner: i32,
    audience: &str,
    ids: &[String],
) -> Result<(), Error> {
    let txn = db.begin().await?;
    write::lock(&txn, owner, audience).await?;
    let doomed: Vec<Row> = heads::rows_of(&txn, owner, audience)
        .await?
        .into_iter()
        .filter(|kept| ids.contains(&kept.row.id) && !kept.row.forgotten)
        .map(|kept| kept.row)
        .collect();
    for row in &doomed {
        forget::forget_one(
            &txn,
            owner,
            audience,
            &forget::row_node(row),
            Forgets::Row(&row.id),
        )
        .await?;
    }
    forget::stub(&txn, owner, &doomed).await?;
    let folded = heads::fold_rows(&heads::rows_of(&txn, owner, audience).await?)?;
    heads::refresh(&txn, owner, audience, &folded).await?;
    txn.commit().await?;
    Ok(())
}

/// Forgets every memory `entries` name, truly.
async fn forget_memories(db: &DatabaseConnection, entries: &[&Value]) -> Result<(), Refusal> {
    for memory in entries.iter().flat_map(|entry| memories_of(entry)) {
        manage::forget_any(db, &memory).await?;
    }
    Ok(())
}

/// Deletes the entry `row` of `node`.
pub(crate) async fn forget_entry(
    db: &DatabaseConnection,
    owner: i32,
    audience: &str,
    node: &str,
    row: &str,
) -> Result<(), Refusal> {
    let head = live(db, owner, audience, node).await?;
    let Some(entry) = head["entries"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|entry| entry["row"] == row)
    else {
        return Err(Refusal::NotFound);
    };
    forget_memories(db, &[entry]).await?;
    // Named no memory still held: the row itself.
    let still = read::node(db, owner, audience, node)
        .await?
        .is_some_and(|head| {
            head["entries"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|entry| entry["row"] == row)
        });
    if still {
        forget_rows(db, owner, audience, &[row.to_string()]).await?;
    }
    Ok(())
}

/// Deletes `node`, with every memory on it.
pub(crate) async fn forget_node(
    db: &DatabaseConnection,
    owner: i32,
    audience: &str,
    node: &str,
) -> Result<(), Refusal> {
    let head = live(db, owner, audience, node).await?;
    if head["kind"]["value"]["kind"] == json!(Kind::Person) {
        return Err(Refusal::No("person_node"));
    }
    let entries: Vec<&Value> = head["entries"].as_array().into_iter().flatten().collect();
    forget_memories(db, &entries).await?;
    let txn = db.begin().await?;
    write::lock(&txn, owner, audience).await?;
    forget::forget_one(&txn, owner, audience, node, Forgets::Node).await?;
    let rows = heads::rows_of(&txn, owner, audience).await?;
    let folded = heads::fold_rows(&rows)?;
    let doomed: Vec<Row> = rows
        .into_iter()
        .filter(|kept| folded.stubbed.contains(&kept.row.id))
        .map(|kept| kept.row)
        .collect();
    forget::stub(&txn, owner, &doomed).await?;
    let folded = heads::fold_rows(&heads::rows_of(&txn, owner, audience).await?)?;
    heads::refresh(&txn, owner, audience, &folded).await?;
    // What the site kept of an instruction on it goes with its intents.
    txn.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM agent_node_instructions WHERE user_id = $1 AND audience = $2 AND node_id = $3",
        [owner.into(), audience.into(), node.into()],
    ))
    .await?;
    txn.commit().await?;
    Ok(())
}

/// Cancels her waiting intent `intent` on `node`.
pub(crate) async fn cancel_intent(
    db: &DatabaseConnection,
    owner: i32,
    audience: &str,
    node: &str,
    intent: &str,
) -> Result<(), Refusal> {
    live(db, owner, audience, node).await?;
    let Some(pen) = crate::services::her_log::pen::think() else {
        return Err(Refusal::No("mind_away"));
    };
    let Some(held) = intent_store::held(db, owner, audience)
        .await?
        .into_iter()
        .find(|held| held.node == node && held.id == intent && held.waiting())
    else {
        return Err(Refusal::NotFound);
    };
    intending::call_off(db, owner, audience, &held, &pen).await?;
    drop(pen);
    if held.id.starts_with(home_intents::THREAD) {
        home_intents::in_step(db, owner).await;
    } else if held.id.starts_with(home_intents::WANT) {
        home_intents::in_step_own(db).await;
    }
    Ok(())
}

/// The owner's answer to what was heard about them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Answer {
    Confirmed,
    Denied,
}

/// Answers the heard entry `row` of `node`.
pub(crate) async fn answer_heard(
    db: &DatabaseConnection,
    owner: i32,
    audience: &str,
    node: &str,
    row: &str,
    answer: Answer,
) -> Result<(), Refusal> {
    let shown = page::node(db, owner, audience, node)
        .await?
        .ok_or(Refusal::NotFound)?;
    if !shown.heard.iter().any(|heard| heard.row == row) {
        return Err(Refusal::NotFound);
    }
    match answer {
        Answer::Denied => forget_entry(db, owner, audience, node, row).await,
        Answer::Confirmed => Err(Refusal::No("heard_unkept")),
    }
}
