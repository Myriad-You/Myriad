//! Her lease on the server: which host core her one mind is on. The server
//! issues it, one grant at a time, for every audience at once; the epoch
//! only goes up (a trigger refuses anything else). How it may move is
//! `myriad_persona_log::lease::decide`, and only that:
//!
//! 1. the holder hands it over (`lease.yield`), to a host core or here;
//! 2. the server takes it over when the holder has been out of reach for
//!    the stop time and the owner's own position proves the owner is in
//!    another place ([`watch`]);
//! 3. the owner brings her somewhere ("come here", `lease.ask` with
//!    `force`).
//!
//! What is stored here is only what the rule needs: the grants, and per
//! end ([`End`]) whether it is a host core, when it was last heard, the
//! place it says it is in (an opaque token of the device's, never a name)
//! and when the owner was last seen there. The stop time is the server's
//! setting ([`super::Settings::stop_after_micros`]), given to the holders
//! with every grant and epoch so they stop by the same value.
//!
//! It is also the one record of which end holds her, log or no log:
//! `persona_sync::lease` reads where she lives from the grant that stands,
//! and her moves house sign theirs here ([`for_move`]): a freeze while she
//! moves (no end holds her), then a thaw or the commit. The rest moves it
//! only while her log on the server is on, and never while the site's copy
//! of her is not the live one ([`request`]).

use myriad_persona_log::lease::{
    self as rules, Grant, How, MoveInput, Moved, OwnerPlace, Refusal, Request, Through,
};
use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, QueryResult, Statement, TransactionTrait,
};
use serde_json::{Value, json};

use super::{Error, NODE, Settings, store};

fn how_name(how: How) -> &'static str {
    match how {
        How::First => "first",
        How::Handover => "handover",
        How::Takeover => "takeover",
        How::Force => "force",
        How::Freeze => "freeze",
        How::Thaw => "thaw",
    }
}

fn grant_of(row: QueryResult) -> Result<Grant, Error> {
    let issued: String = row.try_get("", "issued")?;
    let how: String = row.try_get("", "how")?;
    let how = match how.as_str() {
        "first" => How::First,
        "handover" => How::Handover,
        "takeover" => How::Takeover,
        "force" => How::Force,
        "freeze" => How::Freeze,
        "thaw" => How::Thaw,
        _ => return Err(Error::Refused("a kept grant has no known way".into())),
    };
    Ok(Grant {
        epoch: row.try_get::<i64>("", "epoch")? as u64,
        holder: row.try_get("", "holder")?,
        issued: issued
            .parse()
            .map_err(|_| Error::Refused("a kept grant has no clock".into()))?,
        how,
        from: row.try_get("", "from_holder")?,
        moving: row.try_get("", "move_id")?,
        through: row
            .try_get::<Option<Value>>("", "through")?
            .map(serde_json::from_value)
            .transpose()
            .map_err(|_| Error::Refused("a kept grant's through is not per audience".into()))?,
    })
}

/// Every grant issued, oldest first: what `log.fold` reads her rows
/// against.
pub async fn history<C: ConnectionTrait>(db: &C) -> Result<Vec<Grant>, Error> {
    db.query_all_raw(Statement::from_string(
        DatabaseBackend::Postgres,
        "SELECT epoch, holder, issued, how, through, from_holder, move_id FROM agent_lease_grants \
         ORDER BY epoch",
    ))
    .await?
    .into_iter()
    .map(grant_of)
    .collect()
}

/// The grant that stands, if any was ever issued.
pub async fn standing<C: ConnectionTrait>(db: &C) -> Result<Option<Grant>, Error> {
    db.query_one_raw(Statement::from_string(
        DatabaseBackend::Postgres,
        "SELECT epoch, holder, issued, how, through, from_holder, move_id FROM agent_lease_grants \
         ORDER BY epoch DESC LIMIT 1",
    ))
    .await?
    .map(grant_of)
    .transpose()
}

async fn insert<C: ConnectionTrait>(db: &C, grant: &Grant) -> Result<(), Error> {
    grant.check()?;
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_lease_grants \
         (epoch, holder, issued, how, through, created_at, from_holder, move_id) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        [
            (grant.epoch as i64).into(),
            grant.holder.clone().into(),
            grant.issued.to_string().into(),
            how_name(grant.how).into(),
            grant.through.as_ref().map(|through| json!(through)).into(),
            crate::clock::now().into(),
            grant.from.clone().into(),
            grant.moving.clone().into(),
        ],
    ))
    .await?;
    Ok(())
}

/// The lease moves one at a time.
pub(crate) async fn lock<C: ConnectionTrait>(db: &C) -> Result<(), Error> {
    db.execute_unprepared("SELECT pg_advisory_xact_lock(hashtextextended('myriad-her-lease', 0))")
        .await?;
    Ok(())
}

/// The grant that stands, or the first one: her mind starts here, on the
/// server, when the server has never issued one.
async fn standing_or_first<C: ConnectionTrait>(db: &C) -> Result<Grant, Error> {
    if let Some(grant) = standing(db).await? {
        return Ok(grant);
    }
    let grant = Grant {
        epoch: 1,
        holder: Some(NODE.to_string()),
        issued: store::next_rev(db).await?,
        how: How::First,
        from: None,
        through: None,
        moving: None,
    };
    insert(db, &grant).await?;
    Ok(grant)
}

/// The grant that stands, issuing the first one (to the server) when none
/// was ever issued.
pub async fn current(db: &DatabaseConnection) -> Result<Grant, Error> {
    let txn = db.begin().await?;
    lock(&txn).await?;
    let grant = standing_or_first(&txn).await?;
    txn.commit().await?;
    Ok(grant)
}

/// A site that never issued a grant takes in a her from `holder`, who has
/// held her up to `epoch`: the first grant names it, at that epoch, so the
/// epoch she is at never goes down. The grant that stands when one was
/// issued already. In the caller's transaction.
pub(crate) async fn begin_with<C: ConnectionTrait>(
    db: &C,
    holder: &str,
    epoch: u64,
) -> Result<Grant, Error> {
    lock(db).await?;
    if let Some(grant) = standing(db).await? {
        return Ok(grant);
    }
    let grant = Grant {
        epoch: epoch.max(1),
        holder: Some(holder.to_string()),
        issued: store::next_rev(db).await?,
        how: How::First,
        from: None,
        through: None,
        moving: None,
    };
    insert(db, &grant).await?;
    Ok(grant)
}

/// The lease moves for her moving house (`persona_sync::lease`), in the
/// transaction that moves her: `Freeze` when a move stops her, `Thaw` when
/// it is called off, `Commit` when she lands on its target, `Force` when a
/// new birth or a landing brings her to the site. By the rule, like any
/// other move; a refusal is an error, since the move's own step decided.
/// `at_least` keeps the epoch from going below one an end already heard
/// (a file's, a device's).
pub(crate) async fn for_move<C: ConnectionTrait>(
    db: &C,
    request: Request,
    at_least: Option<u64>,
) -> Result<Grant, Error> {
    lock(db).await?;
    let lease = standing_or_first(db).await?;
    let moved = rules::decide(MoveInput {
        lease,
        request: request.clone(),
        now: super::now(),
        issued: store::next_rev(db).await?,
        holder_last_seen: None,
        holder_place: None,
        owner_place: None,
        stop_after: None,
    })?;
    let mut grant = match moved {
        Moved::Granted(grant) => grant,
        Moved::Refused(refusal) => {
            return Err(Error::Refused(format!("{request:?}: {refusal:?}")));
        }
    };
    grant.epoch = grant.epoch.max(at_least.unwrap_or(0));
    insert(db, &grant).await?;
    tracing::info!(
        epoch = grant.epoch,
        holder = ?grant.holder,
        how = how_name(grant.how),
        "[her-log] her lease moved with her move"
    );
    Ok(grant)
}

/// Whether the site's copy of her is the live one, or there is none (no
/// her on the site at all). While she lives on a device the site let its
/// copy go to, or the site lost or let her go, only a move moves her.
async fn copy_live<C: ConnectionTrait>(db: &C) -> Result<bool, Error> {
    use crate::services::persona_sync::lease::{State, copy_state};
    Ok(copy_state(db)
        .await?
        .is_none_or(|state| state == State::Live))
}

/// Whether an end is a host core (keeps her log, can hold her mind) or a
/// child core (a phone, XR).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Core {
    Host,
    Child,
}

/// What the server knows of one end.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct End {
    pub device_id: String,
    pub host: bool,
    /// Server microseconds.
    pub heard_at: Option<i64>,
    pub place: Option<String>,
    pub owner_since: Option<i64>,
    pub owner_seen_at: Option<i64>,
}

fn end_of(row: QueryResult) -> Result<End, Error> {
    Ok(End {
        device_id: row.try_get("", "device_id")?,
        host: row.try_get::<String>("", "core")? == "host",
        heard_at: row.try_get("", "heard_at")?,
        place: row.try_get("", "place")?,
        owner_since: row.try_get("", "owner_since")?,
        owner_seen_at: row.try_get("", "owner_seen_at")?,
    })
}

async fn ends<C: ConnectionTrait>(db: &C) -> Result<Vec<End>, Error> {
    db.query_all_raw(Statement::from_string(
        DatabaseBackend::Postgres,
        "SELECT device_id, core, heard_at, place, owner_since, owner_seen_at \
         FROM agent_lease_ends ORDER BY device_id",
    ))
    .await?
    .into_iter()
    .map(end_of)
    .collect()
}

/// The server heard from `device` at `at`. `core` says what it is, when
/// this frame shows it (a frame only a host core sends makes it one; an
/// end is never made a child again by a later frame).
pub async fn heard<C: ConnectionTrait>(
    db: &C,
    device: &str,
    core: Option<Core>,
    at: i64,
) -> Result<(), Error> {
    let core = match core {
        Some(Core::Host) => "host",
        _ => "child",
    };
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_lease_ends (device_id, core, heard_at) VALUES ($1, $2, $3) \
         ON CONFLICT (device_id) DO UPDATE SET heard_at = GREATEST( \
             COALESCE(agent_lease_ends.heard_at, EXCLUDED.heard_at), EXCLUDED.heard_at), \
           core = CASE WHEN EXCLUDED.core = 'host' THEN 'host' ELSE agent_lease_ends.core END",
        [device.into(), core.into(), at.into()],
    ))
    .await?;
    Ok(())
}

/// Where `device` says it is (its own opaque token for the place; none when
/// it does not know), and whether the owner is in front of it now. The
/// owner's stay there counts from the first time they are seen there after
/// being elsewhere.
pub async fn place<C: ConnectionTrait>(
    db: &C,
    device: &str,
    place: Option<&str>,
    owner_here: bool,
    at: i64,
) -> Result<(), Error> {
    heard(db, device, Some(Core::Child), at).await?;
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE agent_lease_ends SET \
           owner_since = CASE \
             WHEN NOT $3 THEN owner_since \
             WHEN place IS DISTINCT FROM $2 OR owner_seen_at IS NULL THEN $4 \
             ELSE owner_since END, \
           owner_seen_at = CASE WHEN $3 THEN $4 ELSE owner_seen_at END, \
           place = $2 \
         WHERE device_id = $1",
        [
            device.into(),
            place.map(str::to_string).into(),
            owner_here.into(),
            at.into(),
        ],
    ))
    .await?;
    Ok(())
}

/// The holder stopped by itself at `at` while it held `epoch`: where the
/// gap in her memory on the server begins.
pub async fn stopped<C: ConnectionTrait>(
    db: &C,
    device: &str,
    epoch: u64,
    at: i64,
) -> Result<(), Error> {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE agent_lease_ends SET stopped_epoch = $2, stopped_at = $3 WHERE device_id = $1",
        [device.into(), (epoch as i64).into(), at.into()],
    ))
    .await?;
    Ok(())
}

/// How long the link may be quiet before the server counts it gone: the
/// holder believes itself in reach until then.
fn link_silence_micros() -> i64 {
    let silence = crate::services::agent::link::Timing::from_contract().silence();
    i64::try_from(silence.as_micros()).unwrap_or(i64::MAX)
}

/// What a move did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Granted(Grant),
    Refused(Refusal),
    /// A handover or force to an end that is not a host core: only those
    /// keep her log and can think. Also any of them while the site's copy
    /// of her is not the live one: then only a move moves her.
    NotAHost,
}

/// Asks to move the lease as `request` says, at the server's time `now`.
pub async fn request(
    db: &DatabaseConnection,
    settings: &Settings,
    request: Request,
    now: i64,
) -> Result<Outcome, Error> {
    request_issued(db, settings, request, now, None).await
}

/// [`request`], with the new grant's revision given (the conformance cases
/// give theirs) instead of the server's next.
pub(super) async fn request_issued(
    db: &DatabaseConnection,
    settings: &Settings,
    request: Request,
    now: i64,
    issued: Option<myriad_persona_sync::Rev>,
) -> Result<Outcome, Error> {
    let txn = db.begin().await?;
    lock(&txn).await?;
    let lease = standing_or_first(&txn).await?;
    let ends = ends(&txn).await?;
    let is_host = |id: &str| id == NODE || ends.iter().any(|end| end.device_id == id && end.host);
    match &request {
        Request::Handover { to, .. } | Request::Force { to } if !is_host(to) => {
            txn.commit().await?;
            return Ok(Outcome::NotAHost);
        }
        // She lives on a device the site let its copy go to, or is not
        // here: no host core may take her mind but by a move.
        Request::Handover { .. } | Request::Force { .. } | Request::Takeover { .. }
            if !copy_live(&txn).await? =>
        {
            txn.commit().await?;
            return Ok(Outcome::NotAHost);
        }
        _ => {}
    }
    let holder = ends.iter().find(|end| lease.held_by(&end.device_id));
    // The owner's freshest sighting by any other end.
    let owner_place = ends
        .iter()
        .filter(|end| !lease.held_by(&end.device_id))
        .filter_map(|end| {
            Some(OwnerPlace {
                place: end.place.clone()?,
                since: end.owner_since?,
                seen_at: end.owner_seen_at?,
            })
        })
        .max_by_key(|place| place.seen_at);
    let moved = rules::decide(MoveInput {
        lease: lease.clone(),
        request,
        now,
        issued: match issued {
            Some(issued) => issued,
            None => store::next_rev(&txn).await?,
        },
        holder_last_seen: holder
            .and_then(|end| end.heard_at)
            .map(|heard| heard.saturating_add(link_silence_micros())),
        holder_place: holder.and_then(|end| end.place.clone()),
        owner_place,
        stop_after: Some(settings.stop_after_micros),
    })?;
    let outcome = match moved {
        Moved::Granted(grant) => {
            insert(&txn, &grant).await?;
            Outcome::Granted(grant)
        }
        Moved::Refused(refusal) => Outcome::Refused(refusal),
    };
    txn.commit().await?;
    Ok(outcome)
}

/// Takes her mind over when the rule allows it: the holder out of reach
/// for the stop time and the owner proven elsewhere. Run every so often
/// while her log on the server is on. The grant when it moved.
pub async fn watch(
    db: &DatabaseConnection,
    settings: &Settings,
    now: i64,
) -> Result<Option<Grant>, Error> {
    match standing(db).await? {
        Some(grant) if !grant.held_by(NODE) => {}
        _ => return Ok(None),
    }
    match request(
        db,
        settings,
        Request::Takeover {
            server: NODE.to_string(),
        },
        now,
    )
    .await
    {
        Ok(Outcome::Granted(grant)) => Ok(Some(grant)),
        Ok(_) => Ok(None),
        // No sighting of the holder at all: nothing to decide on.
        Err(Error::Refused(_)) => Ok(None),
        Err(error) => Err(error),
    }
}

/// The owner turned syncing her log off: her mind moves between host cores
/// only while her log syncs (D2-d), so when it is on a device and the site's
/// copy of her is live, the server takes it back here by a force. The device
/// stops on hearing it, or once it is back (`lease.here`, `lease.epoch`).
/// The grant when it moved.
pub async fn sync_off(
    db: &DatabaseConnection,
    settings: &Settings,
    owner: i32,
    now: i64,
) -> Result<Option<Grant>, Error> {
    store::stop(db, owner).await?;
    match standing(db).await? {
        Some(grant) if grant.holder.is_some() && !grant.held_by(NODE) => {}
        _ => return Ok(None),
    }
    match request(
        db,
        settings,
        Request::Force {
            to: NODE.to_string(),
        },
        now,
    )
    .await?
    {
        Outcome::Granted(grant) => Ok(Some(grant)),
        _ => Ok(None),
    }
}

/// `lease.epoch`: where her mind is, for every device.
pub fn epoch_body(grant: &Grant, settings: &Settings) -> Value {
    json!({
        "epoch": grant.epoch,
        "holder": grant.holder,
        "how": how_name(grant.how),
        "stopAfter": settings.stop_after_micros,
    })
}

/// `lease.grant`: to the new holder.
pub fn grant_body(grant: &Grant, settings: &Settings) -> Value {
    json!({
        "grant": grant,
        "stopAfter": settings.stop_after_micros,
    })
}

/// The last shared count of `device`'s the server holds in each audience:
/// where its rows past a takeover start, or what the server's own handover
/// says it wrote. Counts are per audience, so each has its own.
pub async fn through_of<C: ConnectionTrait>(db: &C, device: &str) -> Result<Through, Error> {
    db.query_all_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT audience, MAX(dseq) AS through FROM agent_log_rows \
         WHERE origin = $1 GROUP BY audience",
        [device.into()],
    ))
    .await?
    .into_iter()
    .map(|row| {
        let audience: String = row.try_get("", "audience")?;
        let through: i64 = row.try_get("", "through")?;
        Ok((audience, through.max(0) as u64))
    })
    .filter(|kept| !matches!(kept, Ok((_, 0))))
    .collect()
}
