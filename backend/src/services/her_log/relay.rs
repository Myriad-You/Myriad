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
//!   the rows it lacks, every forget first, in `log.rows` batches that fit
//!   the frame limits (`myriad_nodes::wire::send` for the partner's
//!   computer: a body goes only when it may, the batch's `withheld` says
//!   why the others do not), then the rows it asks again for (`ask`), and
//!   with the server's own vector so it sends what the server lacks. A core
//!   that holds nothing of a large audience gets a single-use HTTPS link
//!   instead (`log.full`).
//! - `log.rows`: checked and merged by the log rules ([`store::take`], the
//!   wire's `take` as the server); a clock too far ahead refuses the batch
//!   whole (`error` `clock_ahead` with the server's time, as the HTTPS
//!   `clock_ahead` compares it). Bodies that did not come, or came spoilt,
//!   are asked for again in the `log.vector` that answers the batch, each
//!   at most once per link session ([`Session`]). Rows sealed to another PC
//!   are relayed unread.
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
//! What is not here yet: speaking protocol 2 on the WebSocket itself (the
//! site's link still speaks protocol 1, and these frames reach nothing
//! until it negotiates 2), delivering to other connected devices
//! ([`To::Device`], [`To::Hosts`], [`To::Everyone`] say where a frame goes;
//! the connection would send them), and asking a holder to give her up
//! when another core asks for her.

use std::collections::BTreeSet;

use myriad_nodes::wire::{ASK_ROWS, Batch};
use myriad_persona_log::{
    Vector,
    lease::{Refusal as MoveRefusal, Request, Through},
};
use sea_orm::DatabaseConnection;
use serde_json::{Value, json};

use super::{
    Error, NODE, Settings, full, keys,
    lease::{self, Core, Outcome},
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

/// A core that holds nothing of an audience with more rows than this gets
/// the HTTPS download instead of rows over the link.
pub const FULL_AFTER_ROWS: usize = 2 * myriad_nodes::wire::BATCH_ROWS;

/// What the server remembers of one link session with one device: the rows
/// it asked that device for again, so it asks for each at most once
/// (`log.vector` `ask`). The connection keeps one per link and drops it
/// when the link closes.
#[derive(Debug, Clone, Default)]
pub struct Session {
    asked: BTreeSet<String>,
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

/// Who holds her mind now: the server when no grant was ever issued.
async fn holder(db: &DatabaseConnection) -> Result<String, Error> {
    Ok(lease::standing(db)
        .await?
        .map_or_else(|| NODE.to_string(), |grant| grant.holder))
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
        pair[0].holder == device
            && pair[1].how == myriad_persona_log::How::Takeover
            && pair[1].holder == NODE
    });
    if standing.holder != device && taken_from_it {
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

/// Takes one frame from `device`, one of `owner`'s computers, at the
/// server's time `now` (microseconds), in that device's link `session`.
/// `Err(Error::Off)` when her log on the server is off or the owner has
/// not turned it on: the connection then treats the frame as one it does
/// not know.
#[allow(clippy::too_many_arguments)]
pub async fn take(
    db: &DatabaseConnection,
    root: &dyn keys::Root,
    settings: &Settings,
    owner: i32,
    device: &str,
    session: &mut Session,
    frame: &Value,
    now: i64,
) -> Result<Vec<Outgoing>, Error> {
    if !settings.enabled
        || !store::account(db, owner)
            .await?
            .is_some_and(|account| account.on())
    {
        return Err(Error::Off);
    }
    let seq = frame.get("seq").and_then(Value::as_u64);
    let holder = holder(db).await?;
    let mut as_roles = vec!["device", "host"];
    if holder == device {
        as_roles.push("mind");
    }
    let checked = match myriad_her_link::check(frame, &roles(&as_roles)) {
        Ok(checked) => checked,
        Err(refusal) => return Ok(vec![Outgoing::error(&refusal.code, seq)]),
    };
    let body = &frame["body"];
    let mine = holder == device;
    let pc = super::pc(owner);
    match checked.t.as_str() {
        "log.vector" => {
            let audience = audience_of(body)?;
            if !pc.holds(&audience) {
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
            lease::heard(db, device, Some(Core::Host), now).await?;
            vector(db, root, device, &pc, &audience, &theirs, ask, now).await
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
            // What a computer may not hold, only her mind writes, for here.
            if !pc.holds(&audience) && !mine {
                return Ok(vec![Outgoing::error("not_holder", seq)]);
            }
            let Ok(batch) = serde_json::from_value::<Batch>(body.clone()) else {
                return Ok(vec![Outgoing::error("bad_frame", seq)]);
            };
            lease::heard(db, device, Some(Core::Host), now).await?;
            rows(db, root, owner, device, session, batch, seq, now).await
        }
        "lease.ask" => {
            lease::heard(db, device, Some(Core::Host), now).await?;
            let request = match body["why"].as_str() {
                Some("force") => Request::Force {
                    to: device.to_string(),
                },
                Some("talk" | "place") if holder == NODE => {
                    let standing = lease::current(db).await?;
                    let through = lease::through_of(db, NODE).await?;
                    Request::Handover {
                        from: NODE.to_string(),
                        epoch: standing.epoch,
                        to: device.to_string(),
                        through: (!through.is_empty()).then_some(through),
                    }
                }
                // Asking another holder to give her up is not done yet: no
                // answer, as the contract allows.
                _ => return Ok(Vec::new()),
            };
            moved(db, settings, request, seq, now).await
        }
        "lease.yield" => {
            lease::heard(db, device, Some(Core::Host), now).await?;
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
            moved(db, settings, request, seq, now).await
        }
        "lease.stopped" => {
            lease::heard(db, device, Some(Core::Host), now).await?;
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
        _ => Err(Error::Refused("not a log or lease frame".into())),
    }
}

#[allow(clippy::too_many_arguments)]
async fn vector(
    db: &DatabaseConnection,
    root: &dyn keys::Root,
    device: &str,
    to: &myriad_persona_log::Holder,
    audience: &str,
    theirs: &Vector,
    ask: Vec<String>,
    now: i64,
) -> Result<Vec<Outgoing>, Error> {
    let ours = store::holdings(db, audience).await?;
    let mut out = Vec::new();
    let held: u64 = ours.vector.values().sum();
    if theirs.is_empty() && held as usize > FULL_AFTER_ROWS {
        let link = full::issue(db, device, audience, now).await?;
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
        let sent = store::send(db, root, audience, theirs, ask, to).await?;
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

#[allow(clippy::too_many_arguments)]
async fn rows(
    db: &DatabaseConnection,
    root: &dyn keys::Root,
    owner: i32,
    device: &str,
    session: &mut Session,
    batch: Batch,
    seq: Option<u64>,
    now: i64,
) -> Result<Vec<Outgoing>, Error> {
    let audience = batch.audience.clone();
    let here = super::server(owner);
    let taken = match store::take(db, root, device, &here, batch, &session.asked, now).await {
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
    let ours = store::holdings(db, &audience).await?;
    let mut out = vec![Outgoing::back(
        "log.vector",
        vector_body(&audience, &ours, &ask),
    )];
    // The other host cores hear there is more; what they may not hold
    // stays here.
    if !taken.added.is_empty() && super::pc(owner).holds(&audience) {
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
    request: Request,
    seq: Option<u64>,
    now: i64,
) -> Result<Vec<Outgoing>, Error> {
    match lease::request(db, settings, request, now).await? {
        Outcome::Granted(grant) => {
            let mut out = Vec::new();
            if grant.holder != NODE {
                out.push(Outgoing {
                    to: To::Device(grant.holder.clone()),
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
