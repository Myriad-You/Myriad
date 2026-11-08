//! Her memories as rows of her log (protocol 2, b2). Same as her records
//! ([`super::state`]): the rows are written by comparing. Each of the site
//! owner's memories that her log keeps (said in their private talk, or her
//! own; never a group's) is compared with the digest the log last has of
//! `memory:<id>` (`agent_her_state_rows`):
//!
//! - new, or its words changed: `memory.write`, with what it took the place
//!   of (`replaces`) and the words it is found by as the site made them
//!   (`cues`, its concepts);
//! - retired since: `memory.retire`, why (`superseded`, `deleted`, `faded`,
//!   as the site says it) and the memory that holds it now (`by`);
//! - in use again: `memory.restore`;
//! - gone from the site's table altogether: retired as `deleted`.
//!
//! Only while the site holds the pen: her memories are hers, and while her
//! mind is on another end that end writes them. Comparing again writes
//! nothing new.

use std::collections::BTreeMap;

use myriad_persona_log::payloads::{MemoryRestore, MemoryRetire, MemoryWrite};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseTransaction, Statement};
use serde_json::Value;

use super::{Error, Owner, keys, state, store};
use crate::services::agent::memory::unified::OWN_VENUE;

/// What a comparison of her memories wrote.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Compared {
    pub written: usize,
    pub retired: usize,
    pub restored: usize,
    /// Memories whose words the log does not take (an empty text…): left
    /// out, as on any other side.
    pub skipped: usize,
}

/// One memory as the site has it.
struct Memory {
    id: String,
    audience: String,
    kind: String,
    text: String,
    cues: Vec<String>,
    retired: Option<(i64, String, Option<String>)>,
    written_at: i64,
}

impl Memory {
    fn object(&self) -> String {
        object(&self.id)
    }

    /// What the log should have of it: its words, and whether it is in use.
    fn digest(&self) -> String {
        let words = myriad_persona_sync::digest(&serde_json::json!({
            "kind": self.kind,
            "text": self.text,
            "cues": self.cues,
        }));
        match self.retired {
            None => format!("memory:{words}"),
            Some(_) => format!("retired:{words}"),
        }
    }
}

fn object(id: &str) -> String {
    format!("memory:{id}")
}

/// The site owner's memories her log keeps.
async fn memories<C: ConnectionTrait>(db: &C, owner: &Owner) -> Result<Vec<Memory>, Error> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id, kind, content, venue, concepts, invalid_at, invalid_reason, superseded_by, \
             created_at FROM agent_memories \
             WHERE user_id = $1 AND venue NOT LIKE 'group:%' ORDER BY created_at, id",
            [owner.user.into()],
        ))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let venue: String = row.try_get("", "venue")?;
        let concepts: Value = row.try_get("", "concepts")?;
        let invalid_at: Option<chrono::DateTime<chrono::FixedOffset>> =
            row.try_get("", "invalid_at")?;
        let created_at: chrono::DateTime<chrono::FixedOffset> = row.try_get("", "created_at")?;
        let reason: Option<String> = row.try_get("", "invalid_reason")?;
        let by: Option<String> = row.try_get("", "superseded_by")?;
        let content: String = row.try_get("", "content")?;
        out.push(Memory {
            id: row.try_get("", "id")?,
            audience: if venue == OWN_VENUE {
                Owner::OWN.to_string()
            } else {
                owner.personal()
            },
            kind: row.try_get("", "kind")?,
            text: content.trim().to_string(),
            cues: concepts
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|cue| !cue.is_empty())
                .map(str::to_string)
                .collect(),
            retired: invalid_at.map(|at| {
                (
                    at.timestamp_millis(),
                    reason
                        .filter(|why| !why.is_empty())
                        .unwrap_or_else(|| "faded".into()),
                    by,
                )
            }),
            written_at: created_at.timestamp_millis(),
        });
    }
    Ok(out)
}

/// Compares her memories with her log in `txn` and writes what differs, as
/// her rows (`epoch`, the server's). `known` is what the log last has of
/// each object, by digest.
pub(super) async fn compare(
    txn: &DatabaseTransaction,
    root: &dyn keys::Root,
    owner: &Owner,
    epoch: u64,
    known: &BTreeMap<String, String>,
    now: i64,
) -> Result<Compared, Error> {
    let memories = memories(txn, owner).await?;
    let mut replaced: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for memory in &memories {
        if let Some((_, _, Some(by))) = &memory.retired {
            replaced
                .entry(by.as_str())
                .or_default()
                .push(object(&memory.id));
        }
    }
    let her = (myriad_persona_log::Author::Her, Some(epoch));
    let mut compared = Compared::default();
    let mut here = std::collections::BTreeSet::new();
    for memory in &memories {
        let object = memory.object();
        here.insert(object.clone());
        let digest = memory.digest();
        let had = known.get(&object);
        if had == Some(&digest) {
            continue;
        }
        let words_known = had.is_some_and(|had| {
            had.split_once(':').map(|(_, words)| words) == digest.split_once(':').map(|(_, w)| w)
        });
        let mut rows: Vec<(&'static str, Value)> = Vec::new();
        if !words_known {
            rows.push((
                "memory.write",
                serde_json::to_value(MemoryWrite {
                    at: memory.written_at,
                    text: memory.text.clone(),
                    kind: memory.kind.clone(),
                    replaces: replaced
                        .get(memory.id.as_str())
                        .cloned()
                        .unwrap_or_default(),
                    cues: memory.cues.clone(),
                })
                .unwrap_or_default(),
            ));
        }
        let was_retired = had.is_some_and(|had| had.starts_with("retired:"));
        match &memory.retired {
            // Retired since, or written again while retired.
            Some((at, why, by)) if !was_retired || !words_known => rows.push((
                "memory.retire",
                serde_json::to_value(MemoryRetire {
                    at: *at,
                    why: why.clone(),
                    by: by.as_deref().map(self::object),
                })
                .unwrap_or_default(),
            )),
            None if was_retired => rows.push((
                "memory.restore",
                serde_json::to_value(MemoryRestore { at: now }).unwrap_or_default(),
            )),
            _ => {}
        }
        if rows
            .iter()
            .any(|(kind, payload)| myriad_persona_log::payloads::check(kind, payload).is_err())
        {
            compared.skipped += 1;
            continue;
        }
        for (kind, payload) in &rows {
            store::write_row_on(
                txn,
                root,
                owner,
                &memory.audience,
                None,
                &object,
                kind,
                payload,
                her,
            )
            .await?;
            match *kind {
                "memory.write" => compared.written += 1,
                "memory.retire" => compared.retired += 1,
                _ => compared.restored += 1,
            }
        }
        state::keep(txn, owner.user, &object, &digest).await?;
    }
    // Gone from the site's table: retired as the owner's taking it back.
    for (object, digest) in known {
        if !object.starts_with("memory:") || here.contains(object) || digest.starts_with("retired:")
        {
            continue;
        }
        let audience = audience_in_log(txn, owner.user, object)
            .await?
            .unwrap_or_else(|| owner.personal());
        let payload = serde_json::to_value(MemoryRetire {
            at: now,
            why: "deleted".into(),
            by: None,
        })
        .unwrap_or_default();
        store::write_row_on(
            txn,
            root,
            owner,
            &audience,
            None,
            object,
            "memory.retire",
            &payload,
            her,
        )
        .await?;
        let words = digest.split_once(':').map_or("", |(_, words)| words);
        state::keep(txn, owner.user, object, &format!("retired:{words}")).await?;
        compared.retired += 1;
    }
    Ok(compared)
}

/// The audience her log holds `object`'s rows in.
async fn audience_in_log<C: ConnectionTrait>(
    db: &C,
    user: i32,
    object: &str,
) -> Result<Option<String>, Error> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT audience FROM agent_log_rows WHERE user_id = $1 AND object = $2 LIMIT 1",
            [user.into(), object.into()],
        ))
        .await?;
    Ok(row.map(|row| row.try_get("", "audience")).transpose()?)
}
