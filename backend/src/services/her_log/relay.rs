//! Her log and her lease over her link's protocol-2 frames: what the server
//! does with a `log.*` or `lease.*` frame from one of the owner's devices,
//! and the frames it sends back.
//!
//! Every frame in is checked against the contract by `myriad_her_link`
//! (fields, sizes, roles) before it is read; every frame out is checked by
//! the same rules when it is encoded ([`encode`]). Frames the server cannot
//! take are answered with `error` and a contract code, never a message.
//!
//! - `log.vector` (what a host core holds of one audience): answered with
//!   the rows it lacks, every forget and retract first, in `log.rows`
//!   batches that fit
//!   the frame limits (`myriad_nodes::wire::send` for the partner's
//!   computer: a body goes only when it may, the batch's `withheld` says
//!   why the others do not), then the rows it asks again for (`ask`), and
//!   with the server's own vector so it sends what the server lacks. A core
//!   that holds nothing of a large audience gets a single-use HTTPS link
//!   instead (`log.full`). A core older than ABI 2 (its `hello` did not list
//!   `log2`) gets no rows while one it would refuse (a merge's or a split's
//!   names, a retract) is to go (`tooOld`): only the server's vector, so it
//!   still sends what the server lacks.
//! - `log.rows`: checked and merged by the log rules ([`store::take`], the
//!   wire's `take` as the server); a clock too far ahead refuses the batch
//!   whole (`error` `clock_ahead` with the server's time, as the HTTPS
//!   `clock_ahead` compares it). Bodies that did not come, or came spoilt,
//!   are asked for again in the `log.vector` that answers the batch, each
//!   at most once per link session ([`Session`]). Rows sealed to another PC
//!   are relayed unread.
//! - Every side of an account goes by its partner id (`personal:<id>`),
//!   which the site mints ([`super::partner_id`]) and gives in `welcome`:
//!   the server's side is that id, and so is every device's as the server
//!   names it (`peer`). A device still on another id refuses to sync in its
//!   own core until it has moved.
//! - The partner's computers hold only the partner's private talk and hers
//!   (`myriad_persona_log::holder`): a group's log and anyone else's
//!   private talk are kept only here and never sent to one (`not_holder`).
//!   Only the core her mind is on may send rows of those audiences (what
//!   she said and noted, written back only here); what a group says reaches
//!   her mind as `group.lend`, not as rows.
//! - `lease.ask`, `lease.yield`, `lease.stopped`: moves by [`lease`]; a new
//!   grant goes to the new holder (`lease.grant`) and every device is told
//!   where she is now (`lease.epoch`), both with the server's stop time.
//!
//! - `group.say`, `chat.say`: her mind on a PC says something in a group
//!   or a private chat the site lent it ([`super::lend`]); sent there as
//!   her, answered with `group.said` or `chat.said`.
//!
//! The link hands these frames here once its welcome said sync on, and
//! sends what comes back where [`To`] says. What is not here yet: asking a
//! holder to give her up when another core asks for her.

use std::collections::{BTreeMap, BTreeSet};

use myriad_nodes::wire::{ASK_ROWS, Batch};
use myriad_persona_log::{
    Core, Holder, Vector,
    lease::{Refusal as MoveRefusal, Request, Through},
    row::Author,
};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::{Value, json};

use super::{
    Error, NODE, Owner, Settings, full, keys,
    lease::{self, Core as Seen, Outcome},
    store,
};

/// Where an outgoing frame goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum To {
    /// Back to the device the frame came from.
    Sender,
    /// One device.
    Device(String),
    /// Every other host core of the owner's (not the sender).
    Hosts,
    /// Every device of the owner's.
    Everyone,
}

/// A frame to send: the connection gives it its `seq`, `ack` and `at`.
#[derive(Debug, Clone, PartialEq)]
pub struct Outgoing {
    pub to: To,
    pub t: &'static str,
    /// For reliable frames.
    pub id: Option<String>,
    pub body: Value,
}

impl Outgoing {
    fn back(t: &'static str, body: Value) -> Self {
        Self {
            to: To::Sender,
            t,
            id: None,
            body,
        }
    }

    fn error(code: &str, seq: Option<u64>) -> Self {
        let mut body = json!({ "code": code });
        if let Some(seq) = seq {
            body["ref"] = json!(seq);
        }
        Self::back("error", body)
    }
}

/// A core that holds nothing of an audience with more rows than this, and
/// listed [`FULL`], gets the HTTPS download instead of rows over the link.
pub const FULL_AFTER_ROWS: usize = 2 * myriad_nodes::wire::BATCH_ROWS;

/// What the server remembers of one link session with one device: its
/// core's ABI as its `hello` said ([`peer_abi`]), whether it downloads a
/// first full sync by a link ([`FULL`]), and the rows it asked that device
/// for again, so it asks for each at most once (`log.vector` `ask`). The
/// connection keeps one per link and drops it when the link closes.
#[derive(Debug, Clone)]
pub struct Session {
    peer_abi: u32,
    full: bool,
    asked: BTreeSet<String>,
}

/// The capability a host core lists when it downloads a first full sync by
/// the link `log.full` gives: only such a link is given one; another gets
/// the rows over the link in `log.rows` batches, as any increment.
pub const FULL: &str = "log.full";

impl Session {
    pub fn new(peer_abi: u32) -> Self {
        Self {
            peer_abi,
            full: false,
            asked: BTreeSet::new(),
        }
    }

    /// The session of a link whose `hello` listed `capabilities`, as
    /// accepted.
    pub fn of(capabilities: &[String]) -> Self {
        Self {
            full: capabilities.iter().any(|capability| capability == FULL),
            ..Self::new(peer_abi(capabilities))
        }
    }
}

impl Default for Session {
    /// A device that said nothing of its core: ABI 1.
    fn default() -> Self {
        Self::new(1)
    }
}

/// The core ABI of the other side of a link (`nodes.wire_send` `peerAbi`):
/// the highest N for which its `hello` (or `welcome`) listed `logN`, 1 when
/// none. `capabilities` is the list as accepted, so it holds only the `logN`
/// the contract of the core this site is built with has, none above what
/// that core reads.
pub fn peer_abi(capabilities: &[String]) -> u32 {
    capabilities
        .iter()
        .filter_map(|capability| capability.strip_prefix("log")?.parse::<u32>().ok())
        .filter(|abi| *abi >= 2)
        .max()
        .unwrap_or(1)
}

/// The core ABI each of the owner's devices last synced her log with, by
/// device: a side of hers is a device that synced, until it is removed.
const SIDES: &str = "her_log_side_abi";
const SIDE_SECS: i64 = 100 * 365 * 24 * 60 * 60;

#[derive(serde::Serialize, serde::Deserialize)]
struct Side {
    abi: u32,
}

/// Keeps that `device` syncs her log with a core of `abi`.
pub async fn remember_side<C: sea_orm::ConnectionTrait>(
    db: &C,
    owner: i32,
    device: &str,
    abi: u32,
) -> Result<(), sea_orm::DbErr> {
    crate::services::runtime_registry::put(
        db,
        SIDES,
        &format!("{owner}/{device}"),
        crate::services::runtime_registry::RegistryIdentity {
            subject_id: Some(owner),
            owner_id: Some(owner),
            tapp_id: None,
            runtime_id: Some(device),
        },
        &Side { abi },
        crate::clock::now().timestamp() + SIDE_SECS,
    )
    .await
}

/// Whether every side of the owner's that syncs her log reads `abi`: there
/// is one, and each device of theirs that synced and is not removed last
/// synced with a core of `abi` or later. A row only such a core reads may
/// then be written to go to all of them (`myriad_nodes::wire`: a side
/// below it gets nothing while one is to go).
pub async fn every_side_reads<C: sea_orm::ConnectionTrait>(
    db: &C,
    owner: i32,
    abi: u32,
) -> Result<bool, sea_orm::DbErr> {
    let rows = crate::services::runtime_registry::list(db, SIDES, Some(owner), None).await?;
    let removed = db
        .query_all_raw(sea_orm::Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT id FROM agent_sync_devices WHERE user_id = $1 AND revoked_at IS NOT NULL",
            [owner.into()],
        ))
        .await?
        .into_iter()
        .filter_map(|row| row.try_get::<String>("", "id").ok())
        .collect::<BTreeSet<String>>();
    let sides: Vec<_> = rows
        .into_iter()
        .filter(|row| {
            !row.runtime_id
                .as_deref()
                .is_some_and(|device| removed.contains(device))
        })
        .collect();
    Ok(!sides.is_empty()
        && sides.into_iter().all(|row| {
            serde_json::from_value::<Side>(row.payload).is_ok_and(|side| side.abi >= abi)
        }))
}

/// The roles the server speaks in.
const SERVER_ROLES: [&str; 2] = ["server", "host"];

fn roles(of: &[&str]) -> Vec<String> {
    of.iter().map(|role| role.to_string()).collect()
}

/// `out` as the frame to send: the protocol-2 envelope, checked against the
/// contract, in its one byte form.
pub fn encode(out: &Outgoing, seq: u64, ack: u64, at_ms: i64) -> Result<String, Error> {
    let mut frame = json!({
        "v": 1,
        "t": out.t,
        "seq": seq,
        "ack": ack,
        "at": at_ms,
        "body": out.body,
    });
    if let Some(id) = &out.id {
        frame["id"] = json!(id);
    }
    myriad_her_link::check(&frame, &roles(&SERVER_ROLES))
        .map(|checked| checked.canonical)
        .map_err(|refusal| Error::Refused(format!("{}: {}", refusal.code, refusal.detail)))
}

fn batch_id() -> String {
    format!("rows_{}", &uuid::Uuid::new_v4().simple().to_string()[..20])
}

fn vector_body(audience: &str, holdings: &myriad_persona_log::Holdings, ask: &[String]) -> Value {
    let mut body = json!({ "audience": audience, "vector": holdings.vector });
    if !holdings.beyond.is_empty() {
        body["beyond"] = json!(holdings.beyond);
    }
    if !ask.is_empty() {
        body["ask"] = json!(ask);
    }
    body
}

/// Who holds her mind now: the server when no grant was ever issued (none
/// while she is frozen between ends, which the site keeps no grant of yet).
async fn holder(db: &DatabaseConnection) -> Result<String, Error> {
    Ok(lease::standing(db)
        .await?
        .map_or_else(|| Some(NODE.to_string()), |grant| grant.holder)
        .unwrap_or_default())
}

/// What the server sends a device that just connected: where her mind is,
/// and, to a holder coming back after the server took her over, the
/// takeover and the last of its rows the server had.
pub async fn welcome(
    db: &DatabaseConnection,
    settings: &Settings,
    device: &str,
) -> Result<Vec<Outgoing>, Error> {
    if !settings.enabled {
        return Err(Error::Off);
    }
    let history = lease::history(db).await?;
    let Some(standing) = history.last() else {
        return Ok(Vec::new());
    };
    let mut out = vec![Outgoing::back(
        "lease.epoch",
        lease::epoch_body(standing, settings),
    )];
    let taken_from_it = history.windows(2).any(|pair| {
        pair[0].held_by(device)
            && pair[1].how == myriad_persona_log::How::Takeover
            && pair[1].held_by(NODE)
    });
    if !standing.held_by(device) && taken_from_it {
        let takeover = history
            .iter()
            .rev()
            .find(|grant| grant.how == myriad_persona_log::How::Takeover)
            .unwrap_or(standing);
        out.push(Outgoing::back(
            "lease.takeover",
            json!({
                "grant": takeover,
                "through": lease::through_of(db, device).await?,
            }),
        ));
    }
    Ok(out)
}

fn field<'a>(body: &'a Value, name: &str) -> Option<&'a Value> {
    body.get(name)
}

fn audience_of(body: &Value) -> Result<String, Error> {
    field(body, "audience")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| Error::Refused("no audience".into()))
}

/// Takes one frame from `device`, one of the ends of `user`'s account, at
/// the server's time `now` (microseconds), in that device's link `session`. `core` is
/// what the device is as a side of her log, as its hello says: a computer
/// (`Pc`) is a host core and holds the partner's private talk and hers; a
/// phone or XR (`Child`) holds no log, so it is no host and gets none.
/// `Err(Error::Off)` when her log on the server is off or the owner has
/// not turned it on: the connection then treats the frame as one it does
/// not know.
#[allow(clippy::too_many_arguments)]
pub async fn take(
    db: &DatabaseConnection,
    root: &dyn keys::Root,
    settings: &Settings,
    user: i32,
    device: &str,
    core: Core,
    session: &mut Session,
    frame: &Value,
    now: i64,
) -> Result<Vec<Outgoing>, Error> {
    if !settings.enabled
        || !store::account(db, user)
            .await?
            .is_some_and(|account| account.on())
    {
        return Err(Error::Off);
    }
    let owner = super::owner(db, user).await?;
    let seq = frame.get("seq").and_then(Value::as_u64);
    let holder = holder(db).await?;
    let host = core == Core::Pc;
    let mut as_roles = vec!["device"];
    if host {
        as_roles.push("host");
    }
    if host && holder == device {
        as_roles.push("mind");
    }
    let checked = match myriad_her_link::check(frame, &roles(&as_roles)) {
        Ok(checked) => checked,
        Err(refusal) => return Ok(vec![Outgoing::error(&refusal.code, seq)]),
    };
    let body = &frame["body"];
    let mine = host && holder == device;
    // Which lease frame came from whom and when, with the epoch it names:
    // no reasons, places or anything else in the body.
    if checked.t.starts_with("lease.") {
        tracing::info!(
            device,
            t = %checked.t,
            epoch = ?body.get("epoch").and_then(serde_json::Value::as_u64),
            holder = %holder,
            "[her-log] a lease frame"
        );
    }
    let side = Holder {
        core,
        owner: owner.partner.clone(),
    };
    match checked.t.as_str() {
        "log.vector" => {
            let audience = audience_of(body)?;
            if !side.holds(&audience) {
                return Ok(vec![Outgoing::error("not_holder", seq)]);
            }
            let theirs: Vector = serde_json::from_value(body["vector"].clone())
                .map_err(|_| Error::Refused("vector".into()))?;
            let ask: Vec<String> = match body.get("ask") {
                Some(ask) => {
                    serde_json::from_value(ask.clone()).map_err(|_| Error::Refused("ask".into()))?
                }
                None => Vec::new(),
            };
            // The counts it holds past a gap, by origin: not in its vector,
            // so a side that took rows after a refused one shows them here.
            if let Some(beyond) = body.get("beyond") {
                tracing::debug!(
                    device,
                    audience,
                    beyond = %beyond,
                    "[her-log] a side's rows past a gap"
                );
            }
            lease::heard(db, device, Some(Seen::Host), now).await?;
            // How far a host core is: the holder on what it was handed, a
            // device marked behind until it catches up, anyone's for when
            // it is handed her later (`stuck`).
            if host {
                let standing = if mine || super::stuck::held_back(device) {
                    lease::standing(db).await?
                } else {
                    None
                };
                super::stuck::heard(device, standing.as_ref(), &audience, &theirs, now);
            }
            vector(
                db, root, &owner, device, &side, session, &audience, &theirs, ask, now,
            )
            .await
        }
        "log.rows" => {
            let audience = audience_of(body)?;
            if let Some(sealed) = body.get("sealed") {
                // Rows only PCs keep, sealed to another PC: relayed unread.
                let to = sealed["to"].as_str().unwrap_or_default().to_string();
                return Ok(vec![Outgoing {
                    to: To::Device(to),
                    t: "log.rows",
                    id: frame.get("id").and_then(Value::as_str).map(str::to_string),
                    body: body.clone(),
                }]);
            }
            let Ok(batch) = serde_json::from_value::<Batch>(body.clone()) else {
                return Ok(vec![Outgoing::error("bad_frame", seq)]);
            };
            // What a computer may not hold, only her mind writes, for here:
            // her mind there now, or there before a takeover sending what it
            // wrote while it held her (her own memory catching up).
            if !side.holds(&audience) && !mine && !(host && held_then(db, device, &batch).await?) {
                return Ok(vec![Outgoing::error("not_holder", seq)]);
            }
            lease::heard(db, device, Some(Seen::Host), now).await?;
            rows(db, root, settings, &owner, device, session, batch, seq, now).await
        }
        "lease.ask" => {
            lease::heard(db, device, Some(Seen::Host), now).await?;
            let request = match body["why"].as_str() {
                Some("force") => {
                    // Taken from here by force: her records as they are now
                    // go into her log first, while she still writes here.
                    if holder == NODE {
                        records_before_leaving(db, root, user).await;
                    }
                    Request::Force {
                        to: device.to_string(),
                    }
                }
                // Taken back from it for never catching up, and still
                // behind: it would hold her and stop again (`stuck`).
                Some("talk" | "place") if holder == NODE && super::stuck::held_back(device) => {
                    return Ok(Vec::new());
                }
                Some("talk" | "place") if holder == NODE => {
                    let standing = lease::current(db).await?;
                    // Nothing new starts here from now on; what is in hand
                    // ends before the grant goes out, so her rows here stop
                    // at `through`.
                    if !super::pen::quiet(device, standing.epoch + 1, super::pen::HANDOVER_WAIT)
                        .await
                    {
                        tracing::warn!(
                            device,
                            thinking = super::pen::thinking(),
                            "[her-log] handing her mind over with a thought still in hand"
                        );
                    }
                    // Her records as they are now go into her log before
                    // `through` is read: the device has her whole.
                    records_before_leaving(db, root, user).await;
                    let through = lease::through_of(db, NODE).await?;
                    let request = Request::Handover {
                        from: NODE.to_string(),
                        epoch: standing.epoch,
                        to: device.to_string(),
                        through: (!through.is_empty()).then_some(through),
                    };
                    let out = moved(db, settings, user, request, seq, now).await;
                    super::pen::handed(db, settings, user).await?;
                    return out;
                }
                // Asking another holder to give her up is not done yet: no
                // answer, as the contract allows.
                _ => return Ok(Vec::new()),
            };
            moved(db, settings, user, request, seq, now).await
        }
        "lease.yield" => {
            lease::heard(db, device, Some(Seen::Host), now).await?;
            let to = body["to"].as_str().unwrap_or(NODE).to_string();
            // `through` is {audience: last dseq}, one count per audience.
            let Ok(through) = serde_json::from_value::<Through>(body["through"].clone()) else {
                return Ok(vec![Outgoing::error("bad_frame", seq)]);
            };
            let request = Request::Handover {
                from: device.to_string(),
                epoch: body["epoch"].as_u64().unwrap_or(0),
                to,
                through: (!through.is_empty()).then_some(through),
            };
            moved(db, settings, user, request, seq, now).await
        }
        "lease.place" => {
            let place = body["place"].as_str().filter(|place| !place.is_empty());
            let owner_here = body["ownerHere"].as_bool().unwrap_or(false);
            lease::place(db, device, place, owner_here, now).await?;
            Ok(Vec::new())
        }
        "lease.stopped" => {
            lease::heard(db, device, Some(Seen::Host), now).await?;
            let stopped_ms = body["stoppedAt"].as_i64().unwrap_or(0);
            lease::stopped(
                db,
                device,
                body["epoch"].as_u64().unwrap_or(0),
                stopped_ms.saturating_mul(1000),
            )
            .await?;
            Ok(Vec::new())
        }
        // Her mind on a PC says something where the site lent her a chat.
        "group.say" | "chat.say" => {
            Ok(super::lend::say(db, &super::lend::Platforms, device, mine, frame).await)
        }
        _ => Err(Error::Refused("not a log or lease frame".into())),
    }
}

#[allow(clippy::too_many_arguments)]
async fn vector(
    db: &DatabaseConnection,
    root: &dyn keys::Root,
    owner: &Owner,
    device: &str,
    to: &myriad_persona_log::Holder,
    session: &Session,
    audience: &str,
    theirs: &Vector,
    ask: Vec<String>,
    now: i64,
) -> Result<Vec<Outgoing>, Error> {
    let ours = store::holdings(db, owner.user, audience).await?;
    // How far a side has her log, by origin: counts only, no rows. What a
    // check after a forget reads to see the side has the forget row.
    tracing::debug!(
        device,
        audience,
        theirs = ?theirs,
        ours = ?ours.vector,
        "[her-log] a side's vector"
    );
    let mut out = Vec::new();
    let held: u64 = ours.vector.values().sum();
    // A link is given only to one that said it downloads by it: one that
    // did not would drop it and never catch up (a Mac did, 2026-11-14).
    if theirs.is_empty() && session.full && held as usize > FULL_AFTER_ROWS {
        let link = full::issue(db, device, audience, session.peer_abi, now).await?;
        out.push(Outgoing::back(
            "log.full",
            json!({
                "audience": audience,
                "url": link.path,
                "expiresAt": link.expires_at_ms,
                "vector": ours.vector,
            }),
        ));
    } else {
        let sent =
            store::send(db, root, owner, audience, theirs, ask, to, session.peer_abi).await?;
        if sent.too_old {
            tracing::info!(
                device,
                "[her-log] a device's core is older than ABI 2: no rows go to it until it repins"
            );
        }
        if !sent.held_back.is_empty() {
            tracing::warn!(
                device,
                held_back = sent.held_back.len(),
                "[her-log] rows about the partner under another id went as stubs: a fault in the log"
            );
        }
        for batch in sent.batches {
            out.push(Outgoing {
                to: To::Sender,
                t: "log.rows",
                id: Some(batch_id()),
                body: serde_json::to_value(batch)
                    .map_err(|_| Error::Refused("a batch does not write".into()))?,
            });
        }
    }
    out.push(Outgoing::back(
        "log.vector",
        vector_body(audience, &ours, &[]),
    ));
    Ok(out)
}

/// Every row of `batch` is hers from `device`, at an epoch it held her: her
/// mind there before the server took her over, back with what it wrote
/// then.
async fn held_then(db: &DatabaseConnection, device: &str, batch: &Batch) -> Result<bool, Error> {
    if batch.rows.is_empty() {
        return Ok(false);
    }
    let history = lease::history(db).await?;
    Ok(batch.rows.iter().all(|row| {
        row.author == Author::Her
            && row.origin == device
            && row.epoch.is_some_and(|epoch| {
                history
                    .iter()
                    .any(|grant| grant.epoch == epoch && grant.held_by(device))
            })
    }))
}

/// The warn line [`unheld_as_stubs`] writes, the same each time so it can
/// be searched for; it carries counts, sides and kinds only.
pub const UNHELD: &str = "[her-log] rows of hers from a device without her mind were kept as stubs";

/// Her rows a device the site issued wrote without her mind are taken as
/// stubs: their place in that device's count is kept, so the vectors still
/// meet, but not what they say, since no mind of hers was there to say it.
/// Which rows those are is the log's own reading of the lease
/// ([`myriad_persona_log::lease::standing`]): one at an epoch whose grant
/// names another holder, or one written alone (epoch 0) while the last
/// grant before it stood elsewhere (a migration or a rewrite run on a linked
/// end before it learned another end holds her). An epoch the history does
/// not have (it is emptied when syncing stays off past the retention
/// period) passes, and so does a row alone with no grant before it, or with
/// its own side's grant last (a host she moved to that never heard its
/// grant), or just before its own side's grant ([`landing`]). Rows the
/// site already holds whole merge as they are; so do rows of a side id the
/// site never issued its owner (`loc_…`: written before that end was
/// linked, by the only mind of hers it had then), the site's own, and
/// forgets (never stubs).
async fn unheld_as_stubs(
    db: &DatabaseConnection,
    owner: &Owner,
    device: &str,
    batch: Batch,
) -> Result<Batch, Error> {
    let suspects: Vec<&myriad_persona_log::Row> =
        batch.rows.iter().filter(|row| suspect(row)).collect();
    if suspects.is_empty() {
        return Ok(batch);
    }
    let ids: Vec<String> = suspects.iter().map(|row| row.id.clone()).collect();
    let origins: Vec<String> = suspects
        .iter()
        .map(|row| row.origin.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let issued: BTreeSet<String> = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id FROM agent_sync_devices WHERE user_id = $1 AND id = ANY($2)",
            [owner.user.into(), origins.into()],
        ))
        .await?
        .into_iter()
        .map(|row| row.try_get::<String>("", "id"))
        .collect::<Result<_, _>>()?;
    if issued.is_empty() {
        return Ok(batch);
    }
    // Held whole: a stub here stays one when the row comes again whole.
    let here: BTreeSet<String> = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id FROM agent_log_rows WHERE user_id = $1 AND id = ANY($2) AND NOT forgotten",
            [owner.user.into(), ids.into()],
        ))
        .await?
        .into_iter()
        .map(|row| row.try_get::<String>("", "id"))
        .collect::<Result<_, _>>()?;
    let history = lease::history(db).await?;
    let (batch, unheld) = stub_unheld(batch, &issued, &here, &history);
    if unheld.stubbed > 0 {
        tracing::warn!(
            device,
            audience = %batch.audience,
            stubbed = unheld.stubbed,
            origins = ?unheld.origins,
            kinds = ?unheld.kinds,
            "{UNHELD}"
        );
    }
    Ok(batch)
}

/// What [`stub_unheld`] made stubs of, for its warn line: counts, the sides
/// they are of and their kinds, never what they said.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Unheld {
    pub stubbed: usize,
    pub origins: BTreeMap<String, usize>,
    pub kinds: BTreeMap<String, usize>,
}

/// How long before its own grant a row written alone may be: an end's clock
/// is set by the server's at each hello, short of half a round trip and the
/// drift since, and it may not have seen the grant's clock before it wrote
/// (a move lands without one).
const LANDING_MS: u64 = 2_000;

/// `row`, written alone, is just before a grant to its own side: her mind
/// had landed there, its clock a little behind the server's.
fn landing(row: &myriad_persona_log::Row, history: &[myriad_persona_log::Grant]) -> bool {
    history.iter().any(|grant| {
        grant.held_by(&row.origin)
            && grant.issued > row.hlc
            && grant.issued.wall_ms <= row.hlc.wall_ms + LANDING_MS
    })
}

/// A row [`unheld_as_stubs`] looks at: hers, whole, of another side.
fn suspect(row: &myriad_persona_log::Row) -> bool {
    row.author == Author::Her && !row.forgotten && row.origin != NODE
}

/// [`unheld_as_stubs`] on what the database told it: the side ids the site
/// issued its owner, the rows it holds whole, the grants.
pub(super) fn stub_unheld(
    mut batch: Batch,
    issued: &BTreeSet<String>,
    here: &BTreeSet<String>,
    history: &[myriad_persona_log::Grant],
) -> (Batch, Unheld) {
    use myriad_persona_log::lease::{ALONE, Stand, standing};
    let mut unheld = Unheld::default();
    let mut stubbed: BTreeSet<String> = BTreeSet::new();
    let mut bodies: BTreeSet<String> = BTreeSet::new();
    for row in &mut batch.rows {
        if !suspect(row) || !issued.contains(&row.origin) || here.contains(&row.id) {
            continue;
        }
        let without_her = match standing(row, history) {
            Stand::NotHolder => true,
            Stand::Concurrent => row.epoch == Some(ALONE) && !landing(row, history),
            Stand::Normal | Stand::AfterHandover => false,
        };
        if !without_her {
            continue;
        }
        // A forget is never a stub: it goes as it is, and the log's rules
        // take or refuse it.
        let stub = row.stub();
        if stub.check().is_err() {
            continue;
        }
        unheld.stubbed += 1;
        *unheld.origins.entry(row.origin.clone()).or_default() += 1;
        *unheld.kinds.entry(row.kind.clone()).or_default() += 1;
        stubbed.insert(row.id.clone());
        bodies.extend(
            row.payload
                .as_ref()
                .map(|payload| payload.reference.clone()),
        );
        *row = stub;
    }
    if stubbed.is_empty() {
        return (batch, unheld);
    }
    // Bodies only stubbed rows pointed at do not come in either, and what
    // the sender said of a stubbed row's body no longer names a body.
    for row in &batch.rows {
        if let Some(payload) = &row.payload {
            bodies.remove(&payload.reference);
        }
    }
    batch
        .payloads
        .retain(|carried| !bodies.contains(&carried.reference));
    batch
        .withheld
        .retain(|withheld| !stubbed.contains(&withheld.id));
    (batch, unheld)
}

#[allow(clippy::too_many_arguments)]
async fn rows(
    db: &DatabaseConnection,
    root: &dyn keys::Root,
    settings: &Settings,
    owner: &Owner,
    device: &str,
    session: &mut Session,
    batch: Batch,
    seq: Option<u64>,
    now: i64,
) -> Result<Vec<Outgoing>, Error> {
    let batch = unheld_as_stubs(db, owner, device, batch).await?;
    let audience = batch.audience.clone();
    let taken = match store::take(db, root, owner, device, batch, &session.asked, now).await {
        Ok(taken) => taken,
        Err(Error::ClockAhead { server_time, .. }) => {
            let mut error = Outgoing::error("clock_ahead", seq);
            error.body["serverTime"] = json!(server_time);
            return Ok(vec![error]);
        }
        // A batch the wire would not read (a payload twice, a withheld
        // body of no row, or carried too).
        Err(Error::Refused(_)) => return Ok(vec![Outgoing::error("bad_frame", seq)]),
        Err(error) => return Err(error),
    };
    if !taken.not_taken.is_empty() || !taken.withheld.is_empty() {
        tracing::info!(
            device,
            not_taken = taken.not_taken.len(),
            withheld = taken.withheld.len(),
            "[her-log] some rows of a batch were not taken, or came without their body"
        );
    }
    // Each body that may yet come is asked for once in this link session.
    let ask: Vec<String> = taken
        .ask
        .into_iter()
        .filter(|id| !session.asked.contains(id))
        .take(ASK_ROWS)
        .collect();
    session.asked.extend(ask.iter().cloned());
    // What another end heard and she said there in the owner's private
    // talk shows in the site's conversations too (D6-e).
    if audience == owner.personal()
        && let Err(error) = super::talk::fold(db, root, owner.user).await
    {
        tracing::warn!(%error, "[her-log] lines said on another end were not copied here");
    }
    // The owner's answers to her cards there are answers here too.
    if audience == owner.personal()
        && !taken.added.is_empty()
        && let Err(error) = super::cards::fold(db, root, owner.user).await
    {
        tracing::warn!(%error, "[her-log] answers to her cards on another end were not taken here");
    }
    // Her records another end wrote go into the site's tables (b1).
    if (audience == owner.personal() || audience == Owner::OWN)
        && !taken.added.is_empty()
        && let Err(error) = super::state::take_in(db, root, owner.user).await
    {
        tracing::warn!(%error, "[her-log] her records from another end were not taken in here");
    }
    // And her memories (b2).
    if (audience == owner.personal() || audience == Owner::OWN)
        && !taken.added.is_empty()
        && let Err(error) = super::memories::take_in(db, root, owner).await
    {
        tracing::warn!(%error, "[her-log] her memories from another end were not taken in here");
    }
    // And her nodes, after the memories their entries name.
    if (audience == owner.personal() || audience == Owner::OWN)
        && !taken.added.is_empty()
        && let Err(error) =
            crate::services::agent::merope::memory_nodes::logged::take_in(db, root, owner).await
    {
        tracing::warn!(%error, "[her-log] her nodes from another end were not taken in here");
    }
    // A catch-up waiting on these rows may be over: she thinks here again.
    if !taken.added.is_empty()
        && matches!(super::pen::current(), super::pen::Pen::CatchingUp { .. })
    {
        super::pen::settle_for(db, settings, Some(owner.user)).await?;
    }
    let ours = store::holdings(db, owner.user, &audience).await?;
    let mut out = vec![Outgoing::back(
        "log.vector",
        vector_body(&audience, &ours, &ask),
    )];
    // The other host cores hear there is more; what they may not hold
    // stays here.
    if !taken.added.is_empty() && owner.pc().holds(&audience) {
        out.push(Outgoing {
            to: To::Hosts,
            t: "log.vector",
            id: None,
            body: vector_body(&audience, &ours, &[]),
        });
    }
    Ok(out)
}

async fn moved(
    db: &DatabaseConnection,
    settings: &Settings,
    user: i32,
    request: Request,
    seq: Option<u64>,
    now: i64,
) -> Result<Vec<Outgoing>, Error> {
    let outcome = lease::request(db, settings, request, now).await?;
    // Where her mind is, read again here (the other processes hear the
    // grant from the database).
    super::pen::settle_for(db, settings, Some(user)).await?;
    match outcome {
        Outcome::Granted(grant) => {
            let mut out = Vec::new();
            if let Some(holder) = grant.holder.clone().filter(|holder| holder != NODE) {
                out.push(Outgoing {
                    to: To::Device(holder),
                    t: "lease.grant",
                    id: Some(grant.epoch.to_string()),
                    body: lease::grant_body(&grant, settings),
                });
            }
            out.push(Outgoing {
                to: To::Everyone,
                t: "lease.epoch",
                id: None,
                body: lease::epoch_body(&grant, settings),
            });
            Ok(out)
        }
        Outcome::Refused(MoveRefusal::NotHolder) => Ok(vec![Outgoing::error("not_holder", seq)]),
        Outcome::Refused(MoveRefusal::StaleEpoch) => Ok(vec![Outgoing::error("stale_epoch", seq)]),
        Outcome::Refused(_) | Outcome::NotAHost => Ok(Vec::new()),
    }
}

/// Her records compared with her log (`state::sync`) as she leaves the
/// server: a handover or a force. A comparison that fails does not stop her
/// leaving; what it missed is the gap a takeover has anyway.
async fn records_before_leaving(db: &DatabaseConnection, root: &dyn keys::Root, user: i32) {
    if let Err(error) = super::state::sync(db, root, user).await {
        tracing::warn!(%error, "[her-log] her records did not reach her log before she left");
    }
}
