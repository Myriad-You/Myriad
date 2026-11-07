//! Her records as rows of her log (b1 of protocol 2): her persona, what she
//! wears, her mood's anchor and each change of it, her state toward the
//! owner, what she means to do, what she said first, her life (pace, what she is doing, her vitals,
//! her serials, the game of soup), her stickers and her diary, so another end
//! her mind moves to has her whole.
//!
//! The rows are written by comparing, not at each place a record changes
//! ([`sync`]): every record of these kinds as the site has it now
//! (`persona_sync::snapshot::of_kind`, the same entities a move carries) is
//! compared with what her log last has of it (`agent_her_state_rows`, one
//! digest an object), and one that differs is written again: a persona field
//! as `persona.field`, any other record as `state.write`. One the log has and
//! the site no longer does is written as gone: a record as `state.write`
//! deleted (live again under its id when it comes back), a persona field as
//! `persona.field` null. One the site keeps only one of (her pace, her
//! anchor…) never goes. Never a `forget` of the object: that would take every
//! row it ever has, a record of the same id coming back too. The first
//! comparison is the backfill. A row's clock is the comparison's, not the
//! change's: it says when the site wrote it to her log, not when the record
//! changed.
//!
//! Her rows only while the site holds the pen; otherwise only the owner
//! changes her here (her persona and her wardrobe, on the settings page), as
//! the owner's rows.
//!
//! The other way ([`take_in`]): the newest row of an object another end
//! wrote is written into the site's tables as a merge lands what it takes
//! (`persona_sync::merge::land_records`), and its digest kept, so the next
//! comparison does not write it back; one whose pictures or rig are not here
//! yet waits for them (`assets`). A record gone on another end (its
//! newest row deleted), or one the owner had her forget (every row of it
//! forgotten), goes from the site's tables.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use myriad_persona_log::{
    Author, Row,
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
pub const KINDS: [Kind; 17] = [
    Kind::Persona,
    Kind::Outfit,
    Kind::AffectAnchor,
    Kind::AffectEvent,
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
    /// Rows of her nodes put into the log that were not in it yet
    /// (`memory_nodes::logged`).
    pub nodes: usize,
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
    for (object, digest) in &known {
        // Gone here already as her log has it (`take_in`).
        if records.contains_key(object) || digest.is_empty() {
            continue;
        }
        let Some(kind) = kind_of(object) else {
            continue;
        };
        if !compared.contains(kind.as_str()) {
            continue;
        }
        if !gone(&txn, root, &owner, object, kind, now, author).await? {
            continue;
        }
        drop_known(&txn, site_owner, object).await?;
        synced.gone += 1;
    }
    txn.commit().await?;
    // Her nodes too: what was written while the log was off, or before.
    match crate::services::agent::merope::memory_nodes::logged::catch_up_all(db, root, &owner).await
    {
        Ok(put) => synced.nodes = put,
        Err(error) => tracing::warn!(%error, "[her-log] her nodes did not all reach her log"),
    }
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
    records_on(
        db,
        &assets::Pictures::new(db, owner.user),
        owner,
        kinds,
        now,
    )
    .await
}

/// [`records`] read on `db` (a transaction too).
async fn records_on<C: ConnectionTrait>(
    db: &C,
    pictures: &assets::Pictures,
    owner: &Owner,
    kinds: &[Kind],
    now: i64,
) -> Result<BTreeMap<String, Record>, Error> {
    let mut records = BTreeMap::new();
    for kind in kinds.iter().copied() {
        for entity in snapshot::of_kind(db, pictures, owner.user, kind, None, EVERY).await? {
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
                audience: Owner::OWN.to_string(),
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
        StateAudience::Own => Owner::OWN.to_string(),
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

/// A record gone from the site, written as gone: whether it was. A persona
/// field as null, any other record as deleted when its kind can go (the
/// core says: one the site keeps only one of never does).
async fn gone(
    txn: &DatabaseTransaction,
    root: &dyn keys::Root,
    owner: &Owner,
    object: &str,
    kind: Kind,
    now: i64,
    author: (Author, Option<u64>),
) -> Result<bool, Error> {
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
            Owner::OWN,
            None,
            object,
            "persona.field",
            &payload,
            author,
        )
        .await?;
        return Ok(true);
    }
    let Some(audience) = audience_of(owner, kind) else {
        return Ok(false);
    };
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
    if myriad_persona_log::payloads::check("state.write", &payload).is_err() {
        return Ok(false);
    }
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
    Ok(true)
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

/// The log has `object` at `digest` now, taken in from `row` another end wrote.
async fn taken_in<C: ConnectionTrait>(
    db: &C,
    user: i32,
    object: &str,
    digest: &str,
    row: &str,
) -> Result<(), Error> {
    db.execute_raw(Statement::from_sql_and_values(
        db.get_database_backend(),
        "INSERT INTO agent_her_state_rows (user_id, object, digest, took) VALUES ($1, $2, $3, $4) \
         ON CONFLICT (user_id, object) DO UPDATE SET digest = EXCLUDED.digest, took = EXCLUDED.took",
        vec![user.into(), object.into(), digest.into(), row.into()],
    ))
    .await?;
    Ok(())
}

/// The row from another end last taken in of each object.
async fn took<C: ConnectionTrait>(db: &C, user: i32) -> Result<BTreeMap<String, String>, Error> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            db.get_database_backend(),
            "SELECT object, took FROM agent_her_state_rows WHERE user_id = $1 AND took IS NOT NULL",
            vec![user.into()],
        ))
        .await?;
    let mut out = BTreeMap::new();
    for row in rows {
        out.insert(
            row.try_get::<String>("", "object")?,
            row.try_get::<String>("", "took")?,
        );
    }
    Ok(out)
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

/// Her records another end wrote, taken into the site's tables: for each
/// object of these kinds, its newest row when another end wrote it and the
/// site does not have it yet (one deleted goes from the site's tables), and
/// each record whose rows are all forgotten (the owner had her forget it).
/// How many records were taken in or went.
pub async fn take_in(
    db: &DatabaseConnection,
    root: &dyn keys::Root,
    site_owner: i32,
) -> Result<usize, Error> {
    let owner = super::owner(db, site_owner).await?;
    let known = known(db, site_owner).await?;
    let took = took(db, site_owner).await?;
    let mut came: Vec<(String, Row, Value)> = Vec::new();
    let mut forgotten: Vec<String> = Vec::new();
    for audience in [Owner::OWN.to_string(), owner.personal()] {
        let mut by_object: BTreeMap<String, Vec<Row>> = BTreeMap::new();
        for row in store::rows(db, site_owner, &audience).await? {
            if matches!(row.kind.as_str(), "state.write" | "persona.field")
                && kind_of(&row.object).is_some()
            {
                by_object.entry(row.object.clone()).or_default().push(row);
            }
        }
        let mut newest = Vec::new();
        for (object, rows) in by_object {
            let Some(last) = rows
                .into_iter()
                .filter(|row| !row.forgotten)
                .max_by(|a, b| a.hlc.cmp(&b.hlc).then_with(|| a.id.cmp(&b.id)))
            else {
                if known.contains_key(&object) {
                    forgotten.push(object);
                }
                continue;
            };
            // Taken in already (once a row), or the site's own is newest.
            if last.origin != NODE && took.get(&object) != Some(&last.id) {
                newest.push(last);
            }
        }
        let held: Vec<&Row> = newest.iter().collect();
        let mut bodies = store::bodies(db, root, site_owner, &audience, &held).await?;
        for row in newest {
            // A body still to come is taken in when it comes.
            if let Some(body) = bodies.remove(&row.id) {
                came.push((row.object.clone(), row, body));
            }
        }
    }

    // What came, as entities, past what the site has already.
    let mut fields: BTreeMap<String, myriad_persona_sync::persona::PersonaField> = BTreeMap::new();
    let mut entities: BTreeMap<(Kind, String), EntityVersion> = BTreeMap::new();
    let mut objects: BTreeMap<String, String> = BTreeMap::new();
    for (object, row, body) in came {
        if row.kind == "persona.field" {
            let Ok(field) = serde_json::from_value::<PersonaField>(body) else {
                continue;
            };
            if known.get(&object) == Some(&myriad_persona_sync::digest(&field.value)) {
                continue;
            }
            fields.insert(
                field.field,
                myriad_persona_sync::persona::PersonaField {
                    value: field.value,
                    rev: row.hlc.clone(),
                    origin: row.origin.clone(),
                },
            );
            objects.insert(object, row.id.clone());
            continue;
        }
        let Ok(write) = serde_json::from_value::<StateWrite>(body) else {
            continue;
        };
        let Some(kind) = Kind::parse(&write.kind) else {
            continue;
        };
        let Ok(entity) = EntityVersion::new(
            kind,
            write.id.clone(),
            row.hlc.clone(),
            row.origin.clone(),
            write.state,
            &write.body,
        ) else {
            tracing::warn!(%object, "[her-log] a record from another end does not read; left out");
            continue;
        };
        if known.get(&object) == Some(&format!("{}:{:?}", entity.digest, entity.state)) {
            continue;
        }
        entities.insert((kind, write.id), entity);
        objects.insert(object, row.id.clone());
    }
    // A record whose pictures or rig are not here yet waits for them, and is
    // taken in when the last comes (`assets::put`).
    let mut urls: BTreeMap<String, String> = BTreeMap::new();
    let mut rigs: BTreeSet<String> = BTreeSet::new();
    let mut waiting = Vec::new();
    for (key, entity) in &entities {
        let here = super::assets::here(db, site_owner, std::slice::from_ref(entity)).await?;
        if here.missing {
            waiting.push(key.clone());
            continue;
        }
        urls.extend(here.urls);
        rigs.extend(here.rigs);
    }
    for (kind, id) in waiting {
        entities.remove(&(kind, id.clone()));
        objects.remove(&state_object(kind.as_str(), &id));
    }
    if objects.is_empty() && forgotten.is_empty() {
        return Ok(0);
    }

    let pictures = assets::Pictures::new(db, site_owner);
    let mut touched: BTreeSet<Kind> = entities.keys().map(|(kind, _)| *kind).collect();
    if !fields.is_empty() {
        touched.insert(Kind::Persona);
    }
    // Her looks are written whole: her persona, her wardrobe and which set
    // each end wears go in together.
    let mut planned = touched.clone();
    if touched.contains(&Kind::Persona) || touched.contains(&Kind::Outfit) {
        planned.extend([Kind::Persona, Kind::Outfit, Kind::Wearing]);
    }
    let mut plan: Vec<EntityVersion> = Vec::new();
    let mut writes = Vec::new();
    let mut removes = Vec::new();
    // Records gone on another end, other than outfits (a merge settles where
    // those go): they go from the site's tables as a forgotten one does.
    let mut went: Vec<String> = Vec::new();
    for kind in planned.iter().copied() {
        let mut have: BTreeMap<String, EntityVersion> =
            snapshot::of_kind(db, &pictures, site_owner, kind, None, EVERY)
                .await?
                .into_iter()
                .map(|entity| (entity.id.clone(), entity))
                .collect();
        if kind == Kind::Persona && !fields.is_empty() {
            let Some(mut persona) = have.remove(myriad_persona_sync::entity::PERSONA_ID) else {
                tracing::warn!("[her-log] her persona came from another end and the site has none");
                continue;
            };
            let mut body: PersonaBody = persona
                .body_as()
                .map_err(|error| Error::Refused(error.to_string()))?;
            body.fields.extend(fields.clone());
            persona = EntityVersion::new(
                Kind::Persona,
                persona.id.clone(),
                persona.rev.clone(),
                persona.origin.clone(),
                State::Live,
                &body,
            )
            .map_err(|error| Error::Refused(error.to_string()))?;
            writes.push(write_of(Kind::Persona, &persona.id, true));
            plan.push(persona);
        }
        for ((of, id), entity) in &entities {
            if *of != kind {
                continue;
            }
            let replaces = have.remove(id).is_some();
            if entity.state == State::Deleted {
                if replaces && kind == Kind::Outfit {
                    removes.push(write_of(kind, id, true));
                } else if replaces {
                    went.push(state_object(kind.as_str(), id));
                }
                continue;
            }
            writes.push(write_of(kind, id, replaces));
            plan.push(entity.clone());
        }
        plan.extend(have.into_values());
    }

    let txn = db.begin().await?;
    let rig = crate::services::persona_sync::merge::land_records(
        &txn, site_owner, plan, writes, removes, &urls, &rigs,
    )
    .await
    .map_err(|error| Error::Refused(format!("{error:?}")))?;
    for object in &went {
        remove_here(&txn, site_owner, object).await?;
    }
    for object in &forgotten {
        remove_here(&txn, site_owner, object).await?;
        drop_known(&txn, site_owner, object).await?;
    }
    // What the site has of them now is what the log has: the next comparison
    // writes none of it back.
    let now = records_on(
        &txn,
        &pictures,
        &owner,
        &touched.into_iter().collect::<Vec<_>>(),
        0,
    )
    .await?;
    // One gone here as her log has it keeps no digest, so the next comparison
    // writes nothing of it.
    for (object, row) in &objects {
        let digest = now.get(object).map(|record| record.digest.as_str());
        taken_in(&txn, site_owner, object, digest.unwrap_or(""), row).await?;
    }
    txn.commit().await?;
    if let Some(Some(rig)) = rig {
        crate::services::merope_rig::mirror_active_asset(Some(rig)).await;
    }
    Ok(objects.len() + forgotten.len())
}

fn write_of(kind: Kind, id: &str, replaces: bool) -> myriad_persona_sync::merge::Write {
    myriad_persona_sync::merge::Write {
        kind,
        id: id.to_string(),
        replaces,
        from: Some(myriad_persona_sync::merge::Side::Other),
    }
}

/// A record gone or forgotten on another end goes from the site's tables.
/// Her persona, her looks and the records she has one of (her anchor, her
/// pace, what she is doing) never go that way, only written again.
async fn remove_here<C: ConnectionTrait>(db: &C, user: i32, object: &str) -> Result<(), Error> {
    use crate::services::agent::merope::api::sync as her;
    let Some((kind, id)) = object
        .strip_prefix("state:")
        .and_then(|rest| rest.split_once(':'))
    else {
        return Ok(());
    };
    let (sql, values): (&str, Vec<sea_orm::Value>) = match kind {
        "affect_event" => (
            "DELETE FROM agent_affect_events WHERE user_id = $1 AND id = $2",
            vec![user.into(), id.into()],
        ),
        // The books she follows are one list (`reading`; a single one,
        // `following`, before it).
        "serial_following" => {
            db.execute_raw(Statement::from_sql_and_values(
                db.get_database_backend(),
                "DELETE FROM runtime_registry WHERE namespace = $1 AND record_id = 'following' \
                 AND payload->>'id' = $2",
                vec![her::SERIAL_NAMESPACE.into(), id.into()],
            ))
            .await?;
            (
                "UPDATE runtime_registry SET payload = COALESCE((SELECT jsonb_agg(book) \
                 FROM jsonb_array_elements(payload) AS book WHERE book->>'id' IS DISTINCT FROM $2), \
                 '[]'::jsonb) WHERE namespace = $1 AND record_id = 'reading' \
                 AND jsonb_typeof(payload) = 'array'",
                vec![her::SERIAL_NAMESPACE.into(), id.into()],
            )
        }
        "diary" => (
            "DELETE FROM agent_diary WHERE user_id = $1 AND id = $2",
            vec![user.into(), id.into()],
        ),
        "intention" => (
            "DELETE FROM agent_intentions WHERE user_id = $1 AND id = $2",
            vec![user.into(), id.into()],
        ),
        "sticker" => (
            "DELETE FROM merope_stickers WHERE venue IS NULL AND id = $1",
            vec![id.into()],
        ),
        "proactive_line" => match id.strip_prefix("say_") {
            Some(say) => (
                "DELETE FROM agent_proactive_messages WHERE user_id = $1 AND say_id = $2",
                vec![user.into(), say.into()],
            ),
            None => match id.strip_prefix("pro_").and_then(|n| n.parse::<i64>().ok()) {
                Some(number) => (
                    "DELETE FROM agent_proactive_messages WHERE user_id = $1 AND id = $2",
                    vec![user.into(), number.into()],
                ),
                None => return Ok(()),
            },
        },
        "vitals_day" => (
            "DELETE FROM runtime_registry WHERE namespace = $1 AND record_id = $2",
            vec![
                her::VITALS_NAMESPACE.into(),
                format!("day:{}", id.split('@').next().unwrap_or_default()).into(),
            ],
        ),
        "serial_work" => (
            "DELETE FROM runtime_registry WHERE namespace = $1 AND record_id = $2",
            vec![her::SERIAL_NAMESPACE.into(), format!("work:{id}").into()],
        ),
        _ => return Ok(()),
    };
    db.execute_raw(Statement::from_sql_and_values(
        db.get_database_backend(),
        sql,
        values,
    ))
    .await?;
    Ok(())
}
