//! Her other memory sources put on her nodes where they are written
//! (decision D1): each place that keeps a row of one of them hands its id
//! here once the row is kept, and the row goes where [`super::homes`] says,
//! through the one way in ([`super::write`]). The row is written as it
//! always was first; what hangs on a node cites it (`memory:<id>`), so
//! hanging it again writes nothing, and forgetting it takes what hangs
//! ([`super::forget`]).
//!
//! - **An entry**: the core's own rows (`entries::fact_rows`), the node
//!   started the first time.
//! - **A span**: one of a day, from its start to the next in her zone.
//! - **A view**: only when the row counts as evidence for one
//!   (`trust::view_may_change`); else an entry, as [`super::homes`] says.
//! - **Something not wrapped up**: a sore spot, with each later version of
//!   it (mended, let go of) written on the same item.
//!
//! Where each row went is kept with the others' ([`super::placements`]),
//! but for the row that lets a sore go: that one is also a memory of them
//! like any other, for sorting to put where it goes.
//!
//! Only while her nodes are written, only with her pen. Nothing here ever
//! fails the write it follows: what cannot be put on a node is said in the
//! log and left. No model is called.

use std::collections::BTreeSet;

use chrono::{NaiveDate, TimeZone};
use myriad_nodes::{
    BODY_VERSION, SaidBy,
    body::MEMORY_REF,
    entries::{self, FactRowsInput, Start, row_id_from},
    key::Key,
    trust::{Known, view_may_change},
};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, EntityTrait, Statement};
use serde_json::{Value, json};

use super::{
    Author, Error, RowIn,
    homes::{self, Before, Hang, Home, Named},
    placements::{self, How},
    read,
    write::{Beside, write_with},
};
use crate::models::entities::agent_memories;
use crate::services::agent::memory::unified::OWN_VENUE;

/// The site owner, with whom groups and her own are kept.
pub(crate) async fn site_owner(db: &DatabaseConnection) -> Option<i32> {
    // Tests each keep a database of their own: the process's cached owner
    // may be another's.
    #[cfg(test)]
    {
        db.query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            crate::services::principal::SITE_OWNER_ID_SQL.to_string(),
        ))
        .await
        .ok()
        .flatten()
        .and_then(|row| row.try_get::<i32>("", "id").ok())
    }
    #[cfg(not(test))]
    {
        crate::services::principal::site_owner_id(db)
            .await
            .ok()
            .flatten()
    }
}

/// The memory row `memory_id` was just kept: put it on her nodes, if it
/// goes there. Never fails the write it follows.
pub(crate) async fn kept(db: &DatabaseConnection, memory_id: &str) {
    if !super::mode().writes() {
        return;
    }
    let site_owner = site_owner(db).await;
    if let Err(error) = hang(db, site_owner, memory_id).await {
        tracing::warn!(%error, "[memory-nodes] a memory not put on her nodes");
    }
}

/// The row `id` took the place of: a sore spot first, else the latest.
async fn before<C: ConnectionTrait>(db: &C, id: &str) -> Result<Option<Before>, Error> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id, source, invalid_reason FROM agent_memories WHERE superseded_by = $1 \
             ORDER BY (source = 'sore') DESC, updated_at DESC, id LIMIT 1",
            [id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    Ok(Some(Before {
        id: row.try_get("", "id")?,
        source: row.try_get("", "source")?,
        reason: row.try_get("", "invalid_reason")?,
    }))
}

async fn exists<C: ConnectionTrait>(db: &C, user_id: i32, row_id: &str) -> Result<bool, Error> {
    Ok(db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT 1 AS one FROM agent_node_rows WHERE user_id = $1 AND row_id = $2",
            [user_id.into(), row_id.into()],
        ))
        .await?
        .is_some())
}

fn as_rows(made: entries::FactRows) -> Vec<RowIn> {
    made.rows
        .into_iter()
        .map(|row| RowIn {
            id: row.id,
            node: made.node.clone(),
            kind: row.kind,
            body: row.body,
            author: Author::Her,
        })
        .collect()
}

/// The rows that start the node `home` goes on.
async fn start<C: ConnectionTrait>(db: &C, home: &Home) -> Result<Vec<RowIn>, Error> {
    let name = match &home.named {
        Named::Person(user_id) => super::facts::person_name(db, *user_id).await?,
        Named::Group(group) => group
            .chars()
            .take(myriad_nodes::limits::NAME_CHARS)
            .collect(),
        Named::Her => homes::HER_NAME.to_string(),
    };
    Ok(as_rows(entries::fact_rows(FactRowsInput {
        audience: home.audience.clone(),
        sync: myriad_nodes::Sync::Shared,
        start: Some(Start {
            key: Some(home.key.clone()),
            name,
            aliases: Vec::new(),
        }),
        key: None,
        facts: Vec::new(),
    })?))
}

/// The entry of `memory` on `home`'s node, said by `said_by`.
fn entry(home: &Home, memory: &str, said_by: &[SaidBy]) -> Result<Vec<RowIn>, Error> {
    Ok(as_rows(entries::fact_rows(FactRowsInput {
        audience: home.audience.clone(),
        sync: myriad_nodes::Sync::Shared,
        start: None,
        key: Some(home.key.clone()),
        facts: vec![entries::Fact {
            memory: memory.to_string(),
            text: home.text.clone(),
            said_by: said_by.to_vec(),
            refs: Vec::new(),
            revision: String::new(),
            audience: None,
        }],
    })?))
}

fn sorted(said_by: &[SaidBy]) -> Vec<SaidBy> {
    let set: BTreeSet<SaidBy> = said_by.iter().copied().collect();
    set.into_iter().collect()
}

/// `day` from its start to the next in her zone, Unix microseconds.
fn day_bounds(day: NaiveDate) -> Option<(i64, i64)> {
    let zone = crate::services::agent::merope::clock::zone();
    let at = |day: NaiveDate| {
        zone.from_local_datetime(&day.and_hms_opt(0, 0, 0)?)
            .earliest()
            .map(|at| at.timestamp_micros())
    };
    Some((at(day)?, at(day.succ_opt()?)?))
}

/// The `base` of the next write to the item `id` of `field` on `head`.
fn base_of(head: Option<&Value>, field: &str, id: &str) -> Value {
    head.and_then(|head| head[field][id].get("base").cloned())
        .unwrap_or_else(|| json!([]))
}

/// The item on `head` an earlier row `follows` is kept on: the one that
/// cites it, else one of its own.
fn item_of(head: Option<&Value>, follows: Option<&str>, memory: &str) -> String {
    if let Some(follows) = follows {
        let cite = Value::from(format!("{MEMORY_REF}{follows}"));
        let found = head
            .and_then(|head| head["open"].as_object())
            .and_then(|open| {
                open.iter().find(|(_, item)| {
                    item["value"]["because"]
                        .as_array()
                        .is_some_and(|because| because.contains(&cite))
                })
            })
            .map(|(id, _)| id.clone());
        if let Some(found) = found {
            return found;
        }
    }
    let of: String = follows
        .unwrap_or(memory)
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_')
        .collect();
    format!("sore-{of}").chars().take(64).collect()
}

/// What `row`, cited as `cite`, is as evidence: her own words in the one
/// person's private talk, or in her own, count; anything else is seen.
fn known_for(row: &agent_memories::Model, cite: &str) -> Known {
    let hers_there = row.speaker == "agent" && (row.venue == "private" || row.venue == OWN_VENUE);
    let mut known = Known::default();
    if hers_there {
        known.her.insert(cite.to_string());
    }
    known
}

/// Puts the memory row `memory_id` on her nodes ([module docs](self));
/// whether anything was written.
pub(crate) async fn hang(
    db: &DatabaseConnection,
    site_owner: Option<i32>,
    memory_id: &str,
) -> Result<bool, Error> {
    let Some(row) = agent_memories::Entity::find_by_id(memory_id.to_string())
        .one(db)
        .await?
    else {
        return Ok(false);
    };
    let before = before(db, &row.id).await?;
    let Some(home) = homes::home(&row, before.as_ref(), site_owner) else {
        return Ok(false);
    };
    let Some(pen) = crate::services::her_log::pen::think() else {
        tracing::info!("[memory-nodes] a memory not put on her nodes: her mind is elsewhere");
        return Ok(false);
    };
    let node = myriad_nodes::node_id(&home.audience, &home.key)?;
    let head = read::node(db, home.user_id, &home.audience, &node).await?;
    let cite = format!("{MEMORY_REF}{}", row.id);
    let rows: Vec<RowIn> = match &home.hang {
        Hang::Entry { said_by } => entry(&home, &row.id, said_by)?,
        Hang::Span { said_by, day } => {
            let mut body = json!({
                "version": BODY_VERSION, "sync": "shared", "saidBy": sorted(said_by),
                "audience": home.audience, "text": home.text, "refs": [cite],
            });
            if let Some((from, to)) = day.and_then(day_bounds) {
                body["from"] = json!(from);
                body["to"] = json!(to);
            }
            vec![RowIn {
                id: row_id_from("node.span", &node, &cite, ""),
                node: node.clone(),
                kind: "node.span".into(),
                body,
                author: Author::Her,
            }]
        }
        Hang::View { view, instead } => {
            if view_may_change(std::slice::from_ref(&cite), &known_for(&row, &cite)) {
                vec![RowIn {
                    id: row_id_from("node.view_changed", &node, &cite, ""),
                    node: node.clone(),
                    kind: "node.view_changed".into(),
                    body: json!({
                        "version": BODY_VERSION, "sync": "shared", "saidBy": ["her"],
                        "base": base_of(head.as_ref(), "views", view), "view": view,
                        "text": home.text, "because": [cite],
                    }),
                    author: Author::Her,
                }]
            } else {
                entry(&home, &row.id, instead)?
            }
        }
        Hang::Open { follows, closed } => {
            let item = item_of(head.as_ref(), follows.as_deref(), &row.id);
            let mut body = json!({
                "version": BODY_VERSION, "sync": "shared", "saidBy": ["her"],
                "base": base_of(head.as_ref(), "open", &item), "item": item,
                "text": home.text, "because": [cite],
            });
            if *closed {
                body["closed"] = json!(true);
            }
            vec![RowIn {
                id: row_id_from("node.open_item", &node, &cite, ""),
                node: node.clone(),
                kind: "node.open_item".into(),
                body,
                author: Author::Her,
            }]
        }
    };
    // Hung already: the same row again writes nothing.
    let Some(last) = rows.last() else {
        return Ok(false);
    };
    if exists(db, home.user_id, &last.id).await? {
        return Ok(false);
    }
    let mut batch = if head.is_some() {
        Vec::new()
    } else {
        start(db, &home).await?
    };
    batch.extend(rows);
    let letting_go = matches!(home.hang, Hang::Open { closed: true, .. });
    // 「关于你」 as going over a stretch names it (`super::facts`).
    let about = home.key == super::audience::person_key(&home.audience, home.user_id)
        && matches!(Key::parse(&home.key)?, Key::Person(_))
        && home.audience.starts_with("personal:");
    let how = if about { How::About } else { How::Node };
    let beside = Beside {
        placements: (!letting_go)
            .then(|| placements::Placement {
                memory_id: row.id.clone(),
                user_id: home.user_id,
                audience: home.audience.clone(),
                how,
                node_id: Some(node.clone()),
            })
            .into_iter()
            .collect(),
        ..Beside::default()
    };
    write_with(
        db,
        home.user_id,
        &home.audience,
        &batch,
        Some(&pen),
        &beside,
    )
    .await?;
    Ok(true)
}
