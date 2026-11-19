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
    row::{Audience, Author},
};
use sea_orm::DatabaseConnection;
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
/// first full sync by a link ([`FULL`]), the rows it asked that device
/// for again, so it asks for each at most once (`log.vector` `ask`), and
/// whether it takes what it lacks a page at a time ([`PAGES`]), with the
/// page of each audience on its way and the rows of each it asked again for
/// that wait for a page with room. The connection keeps one per link and
/// drops it when the link closes.
#[derive(Debug, Clone)]
pub struct Session {
    peer_abi: u32,
    full: bool,
    asked: BTreeSet<String>,
    pages: bool,
    open: BTreeMap<String, Page>,
    /// By audience, oldest first: a side asks for each row at most once on
    /// a link, so what it asked is kept until a page carries it, whatever
    /// becomes of the log.vector that asked.
    asks: BTreeMap<String, Vec<String>>,
    /// A batch that ended a page of the side's could not be settled (the
    /// database failed on it): the link is to close, so paging starts over.
    unsettled: bool,
    /// The link's number (`link::open`): what the status page keeps of a
    /// page on its way is this link's, and only this link lets it go.
    link: u64,
    /// Whether its end keeps up with her log and what it was given on this
    /// link, when its hello listed `deputy` (her temporary host).
    deputy: Option<super::deputy::Watch>,
}

/// The most rows asked again for an audience keeps waiting on a link: as
/// many as a page of asks each could carry. More are dropped and counted.
const ASKS_KEPT: usize = ASK_ROWS * myriad_nodes::wire::PAGE_BATCHES;

/// A page sent and not finished yet: its `more`, what the side held when it
/// asked for it, and whether it carried rows the side lacked (not only rows
/// asked again for).
#[derive(Debug, Clone)]
struct Page {
    id: String,
    vector: Vector,
    beyond: BTreeMap<String, Vec<u64>>,
    lacked: bool,
}

/// The capability a host core lists when it downloads a first full sync by
/// the link `log.full` gives: only such a link is given one; another gets
/// the rows over the link in `log.rows` batches, as any increment.
pub const FULL: &str = "log.full";

/// The capability an end lists when it takes what it lacks a page at a time
/// (`log.rows` `more`, `log.vector` `after`).
pub const PAGES: &str = "log.pages";

impl Session {
    pub fn new(peer_abi: u32) -> Self {
        Self {
            peer_abi,
            full: false,
            asked: BTreeSet::new(),
            pages: false,
            open: BTreeMap::new(),
            asks: BTreeMap::new(),
            unsettled: false,
            link: 0,
            deputy: None,
        }
    }

    /// This session, of the link numbered `link`.
    pub fn on_link(self, link: u64) -> Self {
        Self { link, ..self }
    }

    /// Whether the link is to close: a page the side sent could not be
    /// settled, and it names a page once (`log.pages`). On the next link
    /// paging starts over.
    pub fn unsettled(&self) -> bool {
        self.unsettled
    }

    /// The session of a link whose `hello` listed `capabilities`, as
    /// accepted.
    pub fn of(capabilities: &[String]) -> Self {
        let listed = |name: &str| capabilities.iter().any(|capability| capability == name);
        Self {
            full: listed(FULL),
            pages: listed(PAGES),
            deputy: listed(super::deputy::CAPABILITY).then(super::deputy::Watch::default),
            ..Self::new(peer_abi(capabilities))
        }
    }
}

/// Batches in a page: the contract's `limits.logPageBatches`; fewer in a
/// test that drains pages without writing thousands of rows.
fn page_batches() -> usize {
    #[cfg(test)]
    if let Some(batches) = PAGE_IN_TEST.with(std::cell::Cell::get) {
        return batches;
    }
    myriad_nodes::wire::PAGE_BATCHES
}

#[cfg(test)]
thread_local! {
    static PAGE_IN_TEST: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

/// Pages of `batches` on this thread, for a test, until what this gives is
/// dropped (a test may run on the thread of the one before it).
#[cfg(test)]
#[must_use]
pub(crate) fn pages_in_test(batches: usize) -> impl Drop {
    struct Pages;
    impl Drop for Pages {
        fn drop(&mut self) {
            PAGE_IN_TEST.with(|cell| cell.set(None));
        }
    }
    PAGE_IN_TEST.with(|cell| cell.set(Some(batches)));
    Pages
}

/// A page's id: never one of another link's.
fn page_id() -> String {
    format!("page_{}", &uuid::Uuid::new_v4().simple().to_string()[..20])
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

/// A deputy for `device` when one is due on its link (`session`), as the
/// frame that takes it there; none when none is, or when giving one failed
/// (said here: it is tried again on its next log.vector or heartbeat).
async fn deputize(
    db: &DatabaseConnection,
    owner: &Owner,
    device: &str,
    session: &mut Session,
    now: i64,
) -> Option<Outgoing> {
    let watch = session.deputy.as_mut()?;
    match super::deputy::give(db, owner, device, watch, now).await {
        Ok(Some(deputy)) => {
            tracing::info!(
                device,
                under = deputy.under,
                order = deputy.order,
                "[her-log] a deputy was given"
            );
            Some(Outgoing {
                to: To::Sender,
                t: super::deputy::FRAME,
                id: Some(super::deputy::frame_id(&deputy)),
                body: json!({ "deputy": deputy }),
            })
        }
        Ok(None) => None,
        Err(error) => {
            tracing::warn!(%error, device, "[her-log] a deputy was not given");
            None
        }
    }
}

/// Every heartbeat of a link that syncs: a deputy for its end when one is
/// due (the hourly renewal), while her log on the server is on for `user`.
pub async fn renew(
    db: &DatabaseConnection,
    settings: &Settings,
    user: i32,
    device: &str,
    session: &mut Session,
    now: i64,
) -> Vec<Outgoing> {
    if session.deputy.is_none() || !settings.enabled {
        return Vec::new();
    }
    // Nothing more is read while it does not keep up; one is due again an
    // hour on, or as soon as another epoch stands (a new grant).
    let Some(watch) = session.deputy.as_ref().filter(|watch| watch.keeps_up()) else {
        return Vec::new();
    };
    match lease::standing(db).await {
        Ok(Some(standing)) if standing.held_by(NODE) && watch.due(standing.epoch, now) => {}
        Ok(_) => return Vec::new(),
        Err(error) => {
            tracing::warn!(%error, device, "[her-log] whether a deputy is due could not be read");
            return Vec::new();
        }
    }
    let owner = match store::account(db, user).await {
        Ok(Some(account)) if account.on() => super::owner(db, user).await,
        Ok(_) => return Vec::new(),
        Err(error) => Err(error),
    };
    match owner {
        Ok(owner) => deputize(db, &owner, device, session, now)
            .await
            .into_iter()
            .collect(),
        Err(error) => {
            tracing::warn!(%error, device, "[her-log] whether a deputy is due could not be read");
            Vec::new()
        }
    }
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

/// An audience's kind (`personal`, `own`, `group`), what a log line says
/// of it: never whose it is.
fn kind_of(audience: &str) -> &str {
    audience.split(':').next().unwrap_or_default()
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
    let out = answer(db, root, settings, user, device, core, session, frame, now).await;
    let body = &frame["body"];
    // A log.vector that named its audience's unfinished page and left it
    // unfinished (not answered: her log off, the database away, a field
    // that does not read; or refused) lets it go: the side names a page
    // once, so the page would wait for a name that does not come, and every
    // log.vector after it would get nothing. Its next log.vector without
    // after gets a page again. One that did not name it leaves it.
    if frame["t"] == "log.vector"
        && let Some(audience) = body["audience"].as_str()
        && let Some(after) = body["after"].as_str()
        && session
            .open
            .get(audience)
            .is_some_and(|open| open.id == after)
    {
        session.open.remove(audience);
        crate::services::agent::link::counts::page_closed(device, session.link, audience);
        tracing::info!(
            device,
            audience = kind_of(audience),
            "[her-log] a log.vector naming a page was not answered: the page was let go"
        );
    }
    // A batch that ends a page of the side's and could not be settled
    // (the database failed while taking it, or after) is named by no
    // log.vector: the link closes and paging starts over on the next.
    if out.is_err()
        && session.pages
        && frame["t"] == "log.rows"
        && body["more"].is_string()
        && body.get("sealed").is_none()
    {
        session.unsettled = true;
        crate::services::agent::link::counts::page_unsettled(device);
        tracing::info!(
            device,
            audience = body["audience"].as_str().map_or("?", kind_of),
            "[her-log] a batch ending a page could not be settled: the link closes"
        );
    }
    let out = out?;
    // A frame answered with an error says so here too, once per frame:
    // the frame's type (a sync frame's name, else `?`) and the code, never
    // what it carried. Else a side sending rows the server will not take
    // looks the same as one sending none.
    for refused in out.iter().filter(|out| out.t == "error") {
        tracing::info!(
            device,
            t = frame["t"]
                .as_str()
                .filter(|t| myriad_her_link::needs_sync(t))
                .unwrap_or("?"),
            code = refused.body["code"].as_str().unwrap_or("?"),
            "[her-log] a frame was answered with an error"
        );
    }
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
async fn answer(
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
    let mine = host && holder == device;
    let side = Holder {
        core,
        owner: owner.partner.clone(),
    };
    let checked = match myriad_her_link::check(frame, &roles(&as_roles)) {
        Ok(checked) => checked,
        Err(refusal) => {
            let mut out = vec![Outgoing::error(&refusal.code, seq)];
            // A batch ending a page, refused by the contract (too large,
            // too many), is settled all the same: named back.
            let body = &frame["body"];
            if session.pages
                && frame["t"] == "log.rows"
                && body.get("sealed").is_none()
                && let (Some(audience), Some(more)) =
                    (body["audience"].as_str(), body["more"].as_str())
            {
                let readable = side.holds(audience) || mine;
                settled(db, owner.user, device, audience, more, readable, &mut out).await?;
            }
            return Ok(out);
        }
    };
    let body = &frame["body"];
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
            let beyond: BTreeMap<String, Vec<u64>> = match body.get("beyond") {
                Some(beyond) => {
                    tracing::debug!(
                        device,
                        audience,
                        beyond = %beyond,
                        "[her-log] a side's rows past a gap"
                    );
                    serde_json::from_value(beyond.clone())
                        .map_err(|_| Error::Refused("beyond".into()))?
                }
                None => BTreeMap::new(),
            };
            let after = body.get("after").and_then(Value::as_str);
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
            let asked = Asked {
                theirs: &theirs,
                beyond,
                ask,
                after,
            };
            let mut out = vector(
                db, root, &owner, device, &side, session, &audience, asked, now,
            )
            .await?;
            // Once it keeps up, a deputy (her temporary host).
            if host {
                out.extend(deputize(db, &owner, device, session, now).await);
            }
            Ok(out)
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
            // The page this batch ends, on a link that pages: named back once
            // the batch is settled, taken or refused.
            let more = body
                .get("more")
                .and_then(Value::as_str)
                .filter(|_| session.pages)
                .map(str::to_string);
            // Whether the side may read what the server holds of the
            // audience: it holds it, or her mind is there.
            let readable = side.holds(&audience) || mine;
            let mut out = match serde_json::from_value::<Batch>(body.clone()) {
                Err(_) => vec![Outgoing::error("bad_frame", seq)],
                Ok(batch) => {
                    // What a computer may not hold, only her mind writes,
                    // for here: her mind there now, or there before a
                    // takeover sending what it wrote while it held her (her
                    // own memory catching up).
                    if !readable && !(host && held_then(db, device, &batch).await?) {
                        vec![Outgoing::error("not_holder", seq)]
                    } else {
                        lease::heard(db, device, Some(Seen::Host), now).await?;
                        rows(db, root, settings, &owner, device, session, batch, seq, now).await?
                    }
                }
            };
            if let Some(more) = more {
                settled(db, owner.user, device, &audience, &more, readable, &mut out).await?;
            }
            Ok(out)
        }
        "lease.ask" => {
            lease::heard(db, device, Some(Seen::Host), now).await?;
            // Her mind stays on the site (`lease::minds_go_to_devices`): no
            // answer, as to one held back below, and nothing here stops
            // thinking for it. One holding her from before is brought back
            // (`lease::watch`), not handed its grant again.
            if !lease::minds_go_to_devices() {
                return Ok(Vec::new());
            }
            // Its holder asking: the grant it was sent did not reach it (the
            // link it went out on closed as it went, the device opening
            // another), though `lease.epoch` did. It gets the grant again,
            // as it stands; nothing moves.
            if holder == device
                && let Some(standing) = lease::standing(db)
                    .await?
                    .filter(|grant| grant.held_by(device))
            {
                tracing::info!(
                    device,
                    epoch = standing.epoch,
                    "[her-log] her holder asked again: its grant was sent again"
                );
                return Ok(vec![Outgoing {
                    to: To::Sender,
                    t: "lease.grant",
                    id: Some(standing.epoch.to_string()),
                    body: lease::grant_body(&standing, settings),
                }]);
            }
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
        // It has read a lease.grant or a lease.deputy through and keeps it:
        // only this, never an ack, says so (`deputy`).
        "lease.taken" => {
            let id = body["id"].as_str().unwrap_or_default();
            match body["of"].as_str() {
                Some(super::deputy::FRAME) => {
                    let taken = match super::deputy::issued_of(id) {
                        Some(issued) => super::deputy::received(db, device, &issued).await?,
                        None => false,
                    };
                    if !taken {
                        tracing::info!(device, "[her-log] a deputy it says it took is none the server gave it");
                    }
                }
                _ => tracing::info!(device, of = ?body["of"].as_str(), "[her-log] a lease frame was taken"),
            }
            Ok(Vec::new())
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

/// What a side's `log.vector` says it holds and wants of one audience.
struct Asked<'a> {
    theirs: &'a Vector,
    beyond: BTreeMap<String, Vec<u64>>,
    ask: Vec<String>,
    /// The page it holds now (`log.pages`), when it sent this for the next.
    after: Option<&'a str>,
}

#[allow(clippy::too_many_arguments)]
async fn vector(
    db: &DatabaseConnection,
    root: &dyn keys::Root,
    owner: &Owner,
    device: &str,
    to: &myriad_persona_log::Holder,
    session: &mut Session,
    audience: &str,
    asked: Asked<'_>,
    now: i64,
) -> Result<Vec<Outgoing>, Error> {
    let Asked {
        theirs,
        beyond,
        mut ask,
        after,
    } = asked;
    if session.pages {
        // What it asks again for waits behind what it asked before, and is
        // kept whether or not this log.vector gets a page.
        if !ask.is_empty() {
            let waited = session.asks.entry(audience.to_string()).or_default();
            let mut known: BTreeSet<String> = waited.iter().cloned().collect();
            let mut dropped = false;
            for id in ask.drain(..) {
                if known.contains(&id) {
                    continue;
                }
                if waited.len() >= ASKS_KEPT {
                    dropped = true;
                    break;
                }
                known.insert(id.clone());
                waited.push(id);
            }
            if dropped {
                tracing::info!(
                    device,
                    audience = kind_of(audience),
                    "[her-log] rows asked again for past what a link keeps were dropped"
                );
                crate::services::agent::link::counts::page_passed(device, "ask_dropped");
            }
        }
        // A page a time: while one is on its way, only the log.vector
        // naming it gets the next; a duplicate or a stale one gets nothing
        // again. One naming a page that is not on its way (the last page
        // twice, or a page of another link) gets nothing either: a page
        // goes only to a log.vector without after, or to the one naming it.
        let passed = match (session.open.get(audience), after) {
            (Some(open), Some(after)) if open.id == after => None,
            (Some(_), _) => Some("page_unfinished"),
            (None, Some(_)) => Some("no_such_page"),
            (None, None) => None,
        };
        // It holds nothing more than when it asked for the page, which
        // carried rows it lacked: they were refused. That page is done; the
        // next round starts with its next log.vector without after.
        let passed = passed.or_else(|| {
            let open = session.open.remove(audience)?;
            crate::services::agent::link::counts::page_closed(device, session.link, audience);
            (open.lacked && &open.vector == theirs && open.beyond == beyond).then_some("no_headway")
        });
        // While a page is on its way the side answers the server's
        // log.vector with its own: that it gets nothing is the rule, not
        // news. How long a page has been on its way is on the status page.
        if passed == Some("page_unfinished") {
            tracing::debug!(
                device,
                audience = kind_of(audience),
                after = after.is_some(),
                "[her-log] a log.vector got no page: one is on its way"
            );
            return Ok(Vec::new());
        }
        if let Some(why) = passed {
            tracing::info!(
                device,
                audience = kind_of(audience),
                why,
                after = after.is_some(),
                "[her-log] a log.vector got no page"
            );
            crate::services::agent::link::counts::page_passed(device, why);
            return Ok(Vec::new());
        }
        // Taken off only once a page is sent.
        ask = session.asks.get(audience).cloned().unwrap_or_default();
    }
    let waiting = if session.pages && ask.len() > ASK_ROWS {
        ask.split_off(ASK_ROWS)
    } else {
        Vec::new()
    };
    let ours = store::holdings(db, owner.user, audience).await?;
    if let Some(watch) = session.deputy.as_mut() {
        watch.heard(owner, audience, theirs, &ours.vector);
    }
    // How far a side has her log, by origin: counts only, no rows. What a
    // check after a forget reads to see the side has the forget row.
    tracing::debug!(
        device,
        audience,
        theirs = ?theirs,
        ours = ?ours.vector,
        "[her-log] a side's vector"
    );
    crate::services::agent::alive::log_heard(device, audience, theirs, &ours.vector);
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
        let paging = session.pages.then(|| store::Paging {
            batches: page_batches(),
            beyond: &beyond,
        });
        let asked: BTreeSet<String> = ask.iter().cloned().collect();
        let mut sent = store::send_page(
            db,
            root,
            owner,
            audience,
            theirs,
            ask,
            to,
            session.peer_abi,
            paging,
        )
        .await?;
        // More to go: the last batch names the page, which the side names
        // in its next log.vector once it has settled that batch. The rows
        // asked again for that did not fit wait in the session for the
        // next page, or, when this one carried nothing, for its next
        // log.vector.
        let mut ask_left = std::mem::take(&mut sent.ask_left);
        ask_left.extend(waiting);
        let more = sent.more || !ask_left.is_empty();
        if session.pages {
            if ask_left.is_empty() {
                session.asks.remove(audience);
            } else {
                session.asks.insert(audience.to_string(), ask_left);
            }
        }
        if session.pages
            && more
            && let Some(last) = sent.batches.last_mut()
        {
            let id = page_id();
            last.more = Some(id.clone());
            let lacked = sent
                .batches
                .iter()
                .flat_map(|batch| &batch.rows)
                .any(|row| !asked.contains(&row.id));
            session.open.insert(
                audience.to_string(),
                Page {
                    id,
                    vector: theirs.clone(),
                    beyond: beyond.clone(),
                    lacked,
                },
            );
            crate::services::agent::link::counts::page_opened(device, session.link, audience);
        }
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

/// The batch that ends a page of the side's (`more`, on a link that pages)
/// is settled, taken or refused with an error: the log.vector that answers
/// it names that page (`after`), so the side sends the next one. That is
/// the server's vector of the audience: when the batch was taken, the one
/// with the rows it asks again for; when it was refused, one of its own,
/// read only when the side may read it (`readable`: it holds the audience,
/// or her mind is there), else an empty one, which tells it nothing. A
/// page id or audience that is not one (`limits.idPattern`, an audience of
/// her log) is not named back: the contract's check reads neither shape,
/// so whatever the side wrote would come back to it as it was.
#[allow(clippy::too_many_arguments)]
async fn settled(
    db: &DatabaseConnection,
    user: i32,
    device: &str,
    audience: &str,
    more: &str,
    readable: bool,
    out: &mut Vec<Outgoing>,
) -> Result<(), Error> {
    if !myriad_her_link::valid_id(more) || Audience::parse(audience).is_err() {
        tracing::info!(
            device,
            "[her-log] a batch named a page or an audience that is not one: not named back"
        );
        return Ok(());
    }
    // A page of the side's named back: the link is settling again.
    crate::services::agent::link::counts::page_settled(device);
    if let Some(back) = out
        .iter_mut()
        .find(|out| out.to == To::Sender && out.t == "log.vector")
    {
        back.body["after"] = json!(more);
        return Ok(());
    }
    let mut body = if readable {
        vector_body(audience, &store::holdings(db, user, audience).await?, &[])
    } else {
        json!({ "audience": audience, "vector": {} })
    };
    body["after"] = json!(more);
    out.push(Outgoing::back("log.vector", body));
    Ok(())
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
    let audience = batch.audience.clone();
    let sent = batch.rows.len();
    // Where her rows of it stand against the lease as it arrives (the
    // log's rules, as its fold reads them): a side writing after it gave
    // her back is taken all the same, and only counted here. A row a
    // temporary host wrote is placed against the deputies kept here alone.
    let standings = if batch.rows.iter().any(|row| row.author == Author::Her) {
        let origins: BTreeSet<&str> = batch
            .rows
            .iter()
            .filter(|row| row.author == Author::Her)
            .map(|row| row.origin.as_str())
            .collect();
        let origins: Vec<&str> = origins.into_iter().collect();
        myriad_persona_log::lease::standings(
            &batch.rows,
            &lease::history(db).await?,
            &super::deputy::for_holders(db, &origins).await?,
        )
    } else {
        Vec::new()
    };
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
    // One line a batch, counts only: how many came, how many were new
    // here, and of the new ones how many of hers were written after her
    // epoch was handed on or by a side that did not hold it.
    {
        use myriad_persona_log::lease::Stand;
        let added: BTreeSet<&str> = taken.added.iter().map(String::as_str).collect();
        let placed = |stand: Stand| {
            standings
                .iter()
                .filter(|standing| {
                    standing.standing == stand && added.contains(standing.id.as_str())
                })
                .count()
        };
        tracing::info!(
            device,
            audience = audience.split(':').next().unwrap_or_default(),
            rows = sent,
            added = added.len(),
            after_handover = placed(Stand::AfterHandover),
            not_holder = placed(Stand::NotHolder),
            concurrent = placed(Stand::Concurrent),
            "[her-log] a batch of her log was taken"
        );
    }
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
