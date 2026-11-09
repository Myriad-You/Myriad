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
//!
//! The other way ([`take_in`]): a memory whose newest row another end
//! wrote is folded from its rows (the last write, then whether a retire or
//! a restore came after) into the site's table under its own id, from that
//! end (`origin`), its cues as its concepts. Its digest is kept, so the
//! next comparison does not write it back. One whose rows are all forgotten
//! goes. Its vector is made here as any memory's is, when recall first
//! comes to it.

use std::collections::BTreeMap;

use myriad_persona_log::{
    Row,
    payloads::{MemoryRestore, MemoryRetire, MemoryWrite},
};
use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, DatabaseTransaction, Statement,
    TransactionTrait,
};
use serde_json::{Value, json};

use super::{Error, NODE, Owner, keys, state, store};
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

    /// Its words, as a digest.
    fn words(&self) -> String {
        myriad_persona_sync::digest(&serde_json::json!({
            "kind": self.kind,
            "text": self.text,
            "cues": self.cues,
        }))
    }

    /// What the log should have of it: its words, and whether it is in use.
    fn digest(&self) -> String {
        let words = self.words();
        match self.retired {
            None => format!("memory:{words}"),
            Some(_) => format!("retired:{words}"),
        }
    }
}

/// The words a memory is found by: each of its concepts' names, the
/// canonical one first, once each.
fn cues_of(concepts: &Value) -> Vec<String> {
    let mut cues: Vec<String> = Vec::new();
    for concept in concepts.as_array().into_iter().flatten() {
        let names = std::iter::once(&concept["name"])
            .chain(concept["aliases"].as_array().into_iter().flatten());
        for name in names.filter_map(Value::as_str).map(str::trim) {
            if !name.is_empty() && !cues.iter().any(|cue| cue == name) {
                cues.push(name.to_string());
            }
        }
    }
    cues
}

fn object(id: &str) -> String {
    format!("memory:{id}")
}

/// The site owner's memories her log keeps.
async fn memories<C: ConnectionTrait>(db: &C, owner: &Owner) -> Result<Vec<Memory>, Error> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id, kind, content, venue, audience, concepts, invalid_at, invalid_reason, \
             superseded_by, created_at FROM agent_memories \
             WHERE (user_id = $1 AND venue NOT LIKE 'group:%' AND venue <> $2) OR venue = $2 \
             ORDER BY created_at, id",
            [owner.user.into(), OWN_VENUE.into()],
        ))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let venue: String = row.try_get("", "venue")?;
        // Her own are kept with no account (`unified::own`); a private one
        // only the owner was in: another account's talk never goes in.
        let members: Value = row.try_get("", "audience")?;
        if venue != OWN_VENUE
            && members
                .as_array()
                .into_iter()
                .flatten()
                .any(|member| member.as_i64() != Some(i64::from(owner.user)))
        {
            continue;
        }
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
            cues: cues_of(&concepts),
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
        let skipped = format!("skipped:{}", memory.words());
        if had == Some(&digest) || had == Some(&skipped) {
            continue;
        }
        // Words the log was never given (they did not check) are not known.
        let words_known = had.is_some_and(|had| {
            !had.starts_with("skipped:")
                && had.split_once(':').map(|(_, words)| words)
                    == digest.split_once(':').map(|(_, w)| w)
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
            // Kept as looked at: counted once, until its words change.
            state::keep(txn, owner.user, &object, &skipped).await?;
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
        if !object.starts_with("memory:")
            || here.contains(object)
            || digest.starts_with("retired:")
            || digest.starts_with("skipped:")
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

/// A memory as its rows from another end leave it.
struct Folded {
    write: MemoryWrite,
    retired: Option<MemoryRetire>,
}

/// `rows` of one memory, in log order, with their `bodies`: none while the
/// body of its last write has not come.
fn fold(rows: &[&Row], bodies: &BTreeMap<String, Value>) -> Option<Folded> {
    let mut folded: Option<Folded> = None;
    for row in rows {
        let body = bodies.get(&row.id);
        match (row.kind.as_str(), body) {
            ("memory.write", Some(body)) => {
                folded = Some(Folded {
                    write: serde_json::from_value(body.clone()).ok()?,
                    retired: None,
                });
            }
            ("memory.write", None) => return None,
            ("memory.retire", Some(body)) => {
                if let Some(folded) = &mut folded {
                    folded.retired = serde_json::from_value(body.clone()).ok();
                }
            }
            ("memory.restore", _) => {
                if let Some(folded) = &mut folded {
                    folded.retired = None;
                }
            }
            _ => {}
        }
    }
    folded
}

fn at(ms: i64) -> chrono::DateTime<chrono::FixedOffset> {
    chrono::DateTime::from_timestamp_millis(ms)
        .unwrap_or_else(crate::clock::now)
        .fixed_offset()
}

/// Her memories another end wrote, into the site's table ([module docs](self)).
/// How many were taken in or went.
pub async fn take_in(
    db: &DatabaseConnection,
    root: &dyn keys::Root,
    owner: &Owner,
) -> Result<usize, Error> {
    let took = state::took(db, owner.user).await?;
    let mut taken = 0;
    for audience in [owner.personal(), Owner::OWN.to_string()] {
        let mut objects: BTreeMap<String, Vec<Row>> = BTreeMap::new();
        for row in store::rows(db, owner.user, &audience).await? {
            if row.object.starts_with("memory:") && row.kind.starts_with("memory.") {
                objects.entry(row.object.clone()).or_default().push(row);
            }
        }
        for (object, mut rows) in objects {
            myriad_persona_log::row::sort(&mut rows);
            let Some(newest) = rows.last() else {
                continue;
            };
            if newest.origin == NODE || took.get(&object) == Some(&newest.id) {
                continue;
            }
            let id = object.trim_start_matches("memory:");
            let txn = db.begin().await?;
            if rows.iter().all(|row| row.forgotten) {
                txn.execute_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "DELETE FROM agent_memories WHERE id = $2 \
                     AND (user_id = $1 OR (user_id IS NULL AND venue = $3))",
                    [owner.user.into(), id.into(), OWN_VENUE.into()],
                ))
                .await?;
                state::drop_known(&txn, owner.user, &object).await?;
                txn.commit().await?;
                taken += 1;
                continue;
            }
            let whole: Vec<&Row> = rows.iter().filter(|row| !row.forgotten).collect();
            let bodies = store::bodies(&txn, root, owner.user, &audience, &whole).await?;
            let Some(folded) = fold(&whole, &bodies) else {
                // Its words have not come yet: taken when they do.
                continue;
            };
            let memory = Memory {
                id: id.to_string(),
                audience: audience.clone(),
                kind: folded.write.kind.chars().take(16).collect(),
                text: folded.write.text.trim().to_string(),
                cues: folded.write.cues.clone(),
                retired: folded.retired.as_ref().map(|retired| {
                    (
                        retired.at,
                        retired.why.chars().take(16).collect(),
                        retired
                            .by
                            .as_deref()
                            .map(|by| by.trim_start_matches("memory:").to_string()),
                    )
                }),
                written_at: folded.write.at,
            };
            let own = audience == Owner::OWN;
            let concepts: Vec<Value> = memory
                .cues
                .iter()
                .map(|cue| json!({ "name": cue, "aliases": [] }))
                .collect();
            let (invalid_at, why, by) = match &memory.retired {
                Some((ms, why, by)) => (Some(at(*ms)), Some(why.clone()), by.clone()),
                None => (None, None, None),
            };
            txn.execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "INSERT INTO agent_memories (id, user_id, kind, content, source, venue, audience, \
                 concepts, valid_from, invalid_at, invalid_reason, superseded_by, created_at, \
                 updated_at, origin) \
                 VALUES ($1, $2, $3, $4, 'sync', $5, $6, $7, $8, $9, $10, $11, $8, $12, $13) \
                 ON CONFLICT (id) DO UPDATE SET kind = EXCLUDED.kind, content = EXCLUDED.content, \
                 concepts = EXCLUDED.concepts, invalid_at = EXCLUDED.invalid_at, \
                 invalid_reason = EXCLUDED.invalid_reason, superseded_by = EXCLUDED.superseded_by, \
                 updated_at = EXCLUDED.updated_at \
                 WHERE agent_memories.user_id IS NOT DISTINCT FROM EXCLUDED.user_id \
                   AND agent_memories.venue = EXCLUDED.venue",
                [
                    id.into(),
                    // Her own are kept with no account, as `unified::own` writes them.
                    (!own).then_some(owner.user).into(),
                    memory.kind.clone().into(),
                    memory.text.clone().into(),
                    if own { OWN_VENUE } else { "private" }.into(),
                    if own { json!([]) } else { json!([owner.user]) }.into(),
                    Value::from(concepts).into(),
                    at(memory.written_at).into(),
                    invalid_at.into(),
                    why.into(),
                    by.into(),
                    crate::clock::now().fixed_offset().into(),
                    newest.origin.chars().take(64).collect::<String>().into(),
                ],
            ))
            .await?;
            state::taken_in(&txn, owner.user, &object, &memory.digest(), &newest.id).await?;
            txn.commit().await?;
            taken += 1;
        }
    }
    Ok(taken)
}
