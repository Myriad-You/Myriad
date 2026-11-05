//! Her records as rows of her log (b1 of protocol 2): her persona, what she
//! wears, her mood's anchor, her state toward the owner, what she means to
//! do, what she said first, her life (pace, what she is doing, her vitals,
//! her serials, the game of soup), her stickers and her diary, so another end
//! her mind moves to has her whole.
//!
//! The rows are written by comparing, not at each place a record changes
//! ([`sync`]): every record of these kinds as the site has it now
//! (`persona_sync::snapshot::of_kind`, the same entities a move carries) is
//! compared with what her log last has of it (`agent_her_state_rows`, one
//! digest an object), and one that differs is written again: a persona field
//! as `persona.field`, any other record as `state.write`. One the log has and
//! the site no longer does is written as gone: an outfit as `state.write`
//! deleted, a persona field as `persona.field` null, the rest (the only state
//! they have is live) as a `forget` of the object. The first comparison is the
//! backfill. A row's clock is the comparison's, not the change's: it says when
//! the site wrote it to her log, not when the record changed.
//!
//! Her rows only while the site holds the pen; otherwise only the owner
//! changes her here (her persona and her wardrobe, on the settings page), as
//! the owner's rows.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use myriad_persona_log::{
    Author,
    payloads::{
        PersonaField, STATE_KINDS, StateAudience, StateWrite, state_audience, state_object,
    },
};
use myriad_persona_sync::{EntityVersion, FORMAT_VERSION, Kind, State, persona::PersonaBody};
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DatabaseTransaction, Statement, TransactionTrait,
};
use serde_json::Value;

use super::{Error, NODE, Owner, keys, store};
use crate::services::persona_sync::{assets, snapshot};

/// The kinds whose records are her log's rows here, in the order they are
/// compared. Her memories and diary of nodes come with b2; her talk is D6.
pub const KINDS: [Kind; 16] = [
    Kind::Persona,
    Kind::Outfit,
    Kind::AffectAnchor,
    Kind::OwnerState,
    Kind::Intention,
    Kind::ProactiveLine,
    Kind::Pace,
    Kind::Present,
    Kind::VitalsDay,
    Kind::SerialFollowing,
    Kind::SerialPast,
    Kind::SerialShelf,
    Kind::SerialWork,
    Kind::SoupGame,
    Kind::Sticker,
    Kind::Diary,
];

/// What the owner changes of her on the site's settings page: compared (as
/// the owner's rows) when her mind is elsewhere too.
const THE_OWNERS: [Kind; 2] = [Kind::Persona, Kind::Outfit];

/// No limit on how many records of a kind are compared (the most a query
/// takes).
const EVERY: usize = i64::MAX as usize;

/// A comparison above this is logged: it runs at the end of each turn.
const SLOW: Duration = Duration::from_millis(100);

/// What one comparison wrote.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Synced {
    /// Records written again (changed or new).
    pub written: usize,
    /// Records written as gone.
    pub gone: usize,
    pub took: Duration,
}

/// One record as a row: its object, audience, kind and payload, and the
/// digest that says whether the log has it already.
struct Record {
    object: String,
    audience: String,
    kind: &'static str,
    payload: Value,
    digest: String,
}

/// How long [`soon`] waits, so what a turn changes after her line (her mood,
/// her diary) is in the one comparison.
const SETTLE: Duration = Duration::from_secs(5);

/// One comparison waiting in [`soon`].
static WAITING: AtomicBool = AtomicBool::new(false);

/// [`sync_now`] a moment from now, once however many ask meanwhile: after
/// each line she says (a reply, or one she says first).
pub fn soon(db: &DatabaseConnection) {
    if !super::enabled() || WAITING.swap(true, Ordering::SeqCst) {
        return;
    }
    let db = db.clone();
    tokio::spawn(async move {
        tokio::time::sleep(SETTLE).await;
        WAITING.store(false, Ordering::SeqCst);
        sync_now(&db).await;
    });
}

/// Compares her records here with her log and writes what differs, when
/// the log is on and the owner syncs it. Never fails what called it: a
/// comparison that does not run now runs the next time.
pub async fn sync_now(db: &DatabaseConnection) {
    if !super::enabled() {
        return;
    }
    let Ok(site_owner) = crate::services::site_owner::site_owner_user_id(db).await else {
        return;
    };
    match sync(db, keys::root(), site_owner).await {
        Ok(synced) if synced.took > SLOW => tracing::info!(
            took_ms = synced.took.as_millis() as u64,
            written = synced.written,
            gone = synced.gone,
            "[her-log] comparing her records with her log took long"
        ),
        Ok(_) => {}
        Err(error) => {
            tracing::warn!(%error, "[her-log] her records were not compared with her log")
        }
    }
}

/// [`sync_now`] for the site owner `site_owner`.
pub async fn sync(
    db: &DatabaseConnection,
    root: &dyn keys::Root,
    site_owner: i32,
) -> Result<Synced, Error> {
    let started = Instant::now();
    if !store::account(db, site_owner)
        .await?
        .is_some_and(|account| account.on())
    {
        return Ok(Synced::default());
    }
    let grant = super::lease::current(db).await?;
    let author = if grant.held_by(NODE) {
        (Author::Her, Some(grant.epoch))
    } else {
        (Author::Owner, None)
    };
    let kinds: Vec<Kind> = KINDS
        .into_iter()
        .filter(|kind| author.0 == Author::Her || THE_OWNERS.contains(kind))
        .collect();
    let owner = super::owner(db, site_owner).await?;
    let now = crate::clock::now().timestamp_millis();
    let records = records(db, &owner, &kinds, now).await?;

    let txn = db.begin().await?;
    let known = known(&txn, site_owner).await?;
    let mut synced = Synced::default();
    for record in records.values() {
        if known.get(&record.object) == Some(&record.digest) {
            continue;
        }
        store::write_row_on(
            &txn,
            root,
            &owner,
            &record.audience,
            None,
            &record.object,
            record.kind,
            &record.payload,
            author,
        )
        .await?;
        keep(&txn, site_owner, &record.object, &record.digest).await?;
        synced.written += 1;
    }
    let compared: BTreeSet<&str> = kinds.iter().map(|kind| kind.as_str()).collect();
    for object in known.keys() {
        if records.contains_key(object) {
            continue;
        }
        let Some(kind) = kind_of(object) else {
            continue;
        };
        if !compared.contains(kind.as_str()) {
            continue;
        }
        gone(&txn, root, &owner, object, kind, now, author).await?;
        drop_known(&txn, site_owner, object).await?;
        synced.gone += 1;
    }
    txn.commit().await?;
    synced.took = started.elapsed();
    Ok(synced)
}

/// Every record of `kinds` as the site has it now, as rows, by object.
async fn records(
    db: &DatabaseConnection,
    owner: &Owner,
    kinds: &[Kind],
    now: i64,
) -> Result<BTreeMap<String, Record>, Error> {
    let pictures = assets::Pictures::new(db, owner.user);
    let mut records = BTreeMap::new();
    for kind in kinds.iter().copied() {
        for entity in snapshot::of_kind(db, &pictures, owner.user, kind, None, EVERY).await? {
            for record in records_of(owner, &entity, now) {
                records.insert(record.object.clone(), record);
            }
        }
    }
    Ok(records)
}

/// The rows a record is: one a persona field, one any other record.
fn records_of(owner: &Owner, entity: &EntityVersion, now: i64) -> Vec<Record> {
    if entity.kind == Kind::Persona {
        let Ok(persona) = entity.body_as::<PersonaBody>() else {
            return Vec::new();
        };
        return persona
            .fields
            .iter()
            .map(|(field, value)| Record {
                object: format!("persona:{field}"),
                audience: "own".to_string(),
                kind: "persona.field",
                payload: serde_json::to_value(PersonaField {
                    at: now,
                    field: field.clone(),
                    value: value.value.clone(),
                })
                .unwrap_or_default(),
                digest: myriad_persona_sync::digest(&value.value),
            })
            .collect();
    }
    let kind = entity.kind.as_str();
    let Some(audience) = audience_of(owner, entity.kind) else {
        return Vec::new();
    };
    let write = StateWrite {
        at: now,
        format: FORMAT_VERSION,
        kind: kind.to_string(),
        id: entity.id.clone(),
        state: entity.state,
        body: entity.body.clone(),
    };
    vec![Record {
        object: state_object(kind, &entity.id),
        audience,
        kind: "state.write",
        payload: serde_json::to_value(write).unwrap_or_default(),
        digest: format!("{}:{:?}", entity.digest, entity.state),
    }]
}

/// Whom a record's rows are for. The site's diary is of its talk with the
/// owner only (every entry is about them), so it is theirs.
fn audience_of(owner: &Owner, kind: Kind) -> Option<String> {
    Some(match state_audience(kind.as_str())? {
        StateAudience::Own => "own".to_string(),
        StateAudience::Partner | StateAudience::Either => owner.personal(),
    })
}

/// The kind an object written here is of.
fn kind_of(object: &str) -> Option<Kind> {
    if object.starts_with("persona:") {
        return Some(Kind::Persona);
    }
    let rest = object.strip_prefix("state:")?;
    let (kind, _) = rest.split_once(':')?;
    STATE_KINDS
        .contains(&kind)
        .then(|| Kind::parse(kind))
        .flatten()
}

/// A record gone from the site, written as gone.
async fn gone(
    txn: &DatabaseTransaction,
    root: &dyn keys::Root,
    owner: &Owner,
    object: &str,
    kind: Kind,
    now: i64,
    author: (Author, Option<u64>),
) -> Result<(), Error> {
    if let Some(field) = object.strip_prefix("persona:") {
        let payload = serde_json::to_value(PersonaField {
            at: now,
            field: field.to_string(),
            value: Value::Null,
        })
        .unwrap_or_default();
        store::write_row_on(
            txn,
            root,
            owner,
            "own",
            None,
            object,
            "persona.field",
            &payload,
            author,
        )
        .await?;
        return Ok(());
    }
    let Some(audience) = audience_of(owner, kind) else {
        return Ok(());
    };
    if kind == Kind::Outfit {
        let id = object
            .rsplit_once(':')
            .map(|(_, id)| id)
            .unwrap_or_default();
        let payload = serde_json::to_value(StateWrite {
            at: now,
            format: FORMAT_VERSION,
            kind: kind.as_str().to_string(),
            id: id.to_string(),
            state: State::Deleted,
            body: Value::Null,
        })
        .unwrap_or_default();
        store::write_row_on(
            txn,
            root,
            owner,
            &audience,
            None,
            object,
            "state.write",
            &payload,
            author,
        )
        .await?;
        return Ok(());
    }
    store::forget_on(txn, owner, &audience, object).await?;
    Ok(())
}

/// What her log last has of each object written here, by its digest.
async fn known<C: ConnectionTrait>(db: &C, user: i32) -> Result<BTreeMap<String, String>, Error> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            db.get_database_backend(),
            "SELECT object, digest FROM agent_her_state_rows WHERE user_id = $1",
            vec![user.into()],
        ))
        .await?;
    let mut out = BTreeMap::new();
    for row in rows {
        out.insert(
            row.try_get::<String>("", "object")?,
            row.try_get::<String>("", "digest")?,
        );
    }
    Ok(out)
}

/// The log has `object` at `digest` now (written here, or taken in).
pub(super) async fn keep<C: ConnectionTrait>(
    db: &C,
    user: i32,
    object: &str,
    digest: &str,
) -> Result<(), Error> {
    db.execute_raw(Statement::from_sql_and_values(
        db.get_database_backend(),
        "INSERT INTO agent_her_state_rows (user_id, object, digest) VALUES ($1, $2, $3) \
         ON CONFLICT (user_id, object) DO UPDATE SET digest = EXCLUDED.digest",
        vec![user.into(), object.into(), digest.into()],
    ))
    .await?;
    Ok(())
}

async fn drop_known<C: ConnectionTrait>(db: &C, user: i32, object: &str) -> Result<(), Error> {
    db.execute_raw(Statement::from_sql_and_values(
        db.get_database_backend(),
        "DELETE FROM agent_her_state_rows WHERE user_id = $1 AND object = $2",
        vec![user.into(), object.into()],
    ))
    .await?;
    Ok(())
}
