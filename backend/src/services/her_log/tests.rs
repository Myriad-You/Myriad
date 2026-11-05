//! Her log on the server, against a real database
//! (`MYRIAD_MEDIA_TEST_DATABASE_URL`; skipped without one).
//!
//! Two host cores are simulated in memory with the same log rules the
//! native app links (`myriad_persona_log`, and `myriad_nodes::wire` for
//! what they send): they write, swap vectors with the server over
//! protocol-2 frames (each one checked against the contract on the way in
//! and out), and must fold to the same her. The lease cases of the shared
//! conformance files run through the server's wiring.

use std::collections::BTreeMap;

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use myriad_nodes::wire::{self, SendInput};
use myriad_persona_log::{
    AdmitInput, Core, Entry, FoldInput, Grant, Holder, MergeInput, Row, Vector, admit, fold, merge,
    row::{Author, LOG_VERSION, Payload, Replicate},
    vector,
};
use myriad_persona_sync::{Rev, canonical::canonical_bytes};
use sea_orm::{ConnectionTrait, DatabaseConnection};
use serde_json::{Value, json};

use super::{
    NODE, Owner, Settings,
    keys::test_root::TestRoot,
    lease, lend, relay,
    relay::{Outgoing, To},
    store,
    test_partner::PARTNER,
};

const OWNER: i32 = 41;
/// The partner's private talk: `personal:<partner id>` ([`PARTNER`]).
const PERSONAL: &str = "personal:partner_019b76daa80000112233445566778899";
/// Someone else's private talk with her: kept on the server only.
const OTHER: &str = "personal:99";
const OWN: &str = "own:";
const GROUP: &str = "group:telegram:-100";
/// A fixed time, microseconds.
const T0: i64 = 1_761_200_000_000_000;
const MINUTE: i64 = 60_000_000;

fn root() -> TestRoot {
    TestRoot([9; 32])
}

/// What a side writes as a row's body: the shape its kind has in the core
/// (a node's created row and span, a memory, a mood), or, for a kind with
/// none, the text as it is.
fn body_of(kind: &str, audience: &str, object: &str, text: &str, at: i64) -> Value {
    let at = at / 1000;
    match kind {
        "node.created" => json!({
            "key": format!("minted:{}", object.trim_start_matches("node:")),
            "kind": "task",
            "name": text,
            "saidBy": ["her"],
            "sync": "shared",
            "version": 1,
        }),
        "node.span" => json!({
            "audience": audience,
            "saidBy": ["her"],
            "sync": "shared",
            "text": text,
            "version": 1,
        }),
        "memory.write" => json!({ "at": at, "kind": "fact", "text": text }),
        "affect.change" => json!({
            "at": at, "valence": 60, "arousal": 40, "band": "calm", "cause": text,
        }),
        _ => json!(text),
    }
}

/// Whether `data`, a body's bytes, says `text`.
fn says(data: &[u8], text: &str) -> bool {
    String::from_utf8_lossy(data).contains(text)
}

fn on() -> Settings {
    Settings {
        enabled: true,
        ..Settings::default()
    }
}

async fn schema(prefix: &str) -> Option<crate::db::IsolatedSchema> {
    let url = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL").ok()?;
    let schema = crate::db::IsolatedSchema::migrated(&url, prefix).await;
    schema
        .db
        .execute_unprepared(&format!(
            "INSERT INTO users (id, username) VALUES ({OWNER}, 'owner')"
        ))
        .await
        .unwrap();
    store::start(&schema.db, OWNER).await.unwrap();
    super::test_partner::give(&schema.db, OWNER, PARTNER).await;
    assert_eq!(owner(&schema.db).await.personal(), PERSONAL);
    Some(schema)
}

async fn owner(db: &DatabaseConnection) -> Owner {
    super::owner(db, OWNER).await.unwrap()
}

/// The partner's computers, as the server sees one.
fn pc() -> Holder {
    Holder {
        core: Core::Pc,
        owner: PARTNER.to_string(),
    }
}

/// One host core in memory: a log per audience, its ledger, its payloads,
/// its clock and counts.
struct Side {
    id: String,
    /// The account it is of, and the partner id it goes by.
    user: i32,
    partner: String,
    logs: BTreeMap<String, Vec<Row>>,
    ledger: Vec<Entry>,
    payloads: BTreeMap<String, Vec<u8>>,
    clock: Option<Rev>,
    counts: BTreeMap<String, u64>,
    written: u64,
    seq: u64,
    epoch: Option<u64>,
    /// This side's clock, microseconds.
    now: i64,
    /// Its link session with the server, as the server keeps it.
    link: relay::Session,
}

impl Side {
    fn new(id: &str) -> Self {
        Self::of(id, OWNER, PARTNER)
    }

    /// A computer of another account.
    fn of(id: &str, user: i32, partner: &str) -> Self {
        Self {
            id: id.to_string(),
            user,
            partner: partner.to_string(),
            logs: BTreeMap::new(),
            ledger: Vec::new(),
            payloads: BTreeMap::new(),
            clock: None,
            counts: BTreeMap::new(),
            written: 0,
            seq: 0,
            epoch: None,
            now: T0,
            // A computer whose core reads every row (its hello lists log2).
            link: relay::Session::new(relay::peer_abi(&["log2".to_string()])),
        }
    }

    fn log(&self, audience: &str) -> Vec<Row> {
        self.logs.get(audience).cloned().unwrap_or_default()
    }

    /// This computer as a side of her log.
    fn pc(&self) -> Holder {
        Holder {
            core: Core::Pc,
            owner: self.partner.clone(),
        }
    }

    /// Writes one shared row: hers (with the epoch it holds) or the owner's.
    fn write(
        &mut self,
        audience: &str,
        object: &str,
        kind: &str,
        author: Author,
        payload: Option<&str>,
        now: i64,
    ) -> Row {
        let body = payload.map(|text| body_of(kind, audience, object, text, now));
        self.write_body(audience, object, kind, author, body, now)
    }

    /// [`Side::write`] with the body as given.
    fn write_body(
        &mut self,
        audience: &str,
        object: &str,
        kind: &str,
        author: Author,
        body: Option<Value>,
        now: i64,
    ) -> Row {
        self.now = self.now.max(now);
        let hlc = myriad_persona_log::clock::next(self.clock.as_ref(), now, 0, &self.id).unwrap();
        self.clock = Some(hlc.clone());
        self.written += 1;
        let count = self.counts.entry(audience.to_string()).or_default();
        *count += 1;
        let id = format!("{}_{}", self.id, self.written);
        let payload = body.map(|body| {
            let reference = format!("blob:{id}");
            self.payloads
                .insert(reference.clone(), canonical_bytes(&body));
            Payload {
                reference,
                key: None,
            }
        });
        let row = Row {
            version: LOG_VERSION,
            id,
            origin: self.id.clone(),
            author,
            epoch: (author == Author::Her).then(|| self.epoch.expect("holds the lease")),
            dseq: Some(*count),
            hlc,
            audience: audience.to_string(),
            subject: None,
            object: object.to_string(),
            kind: kind.to_string(),
            payload,
            replicate: Replicate::Shared,
            forgotten: false,
            to: None,
            takes: Vec::new(),
        };
        row.check().unwrap();
        // Its own rows go through the same merge as anyone's.
        self.merge(audience, vec![row.clone()]);
        row
    }

    fn merge(&mut self, audience: &str, incoming: Vec<Row>) {
        // Her mind writes rows of audiences no computer keeps (a group's,
        // someone else's talk) only to send them back to the server; this
        // stand-in keeps them in memory to send, as the server would.
        let holder = if self.pc().holds(audience) {
            self.pc()
        } else {
            Holder {
                core: Core::Server,
                owner: self.partner.clone(),
            }
        };
        let admitted = admit(AdmitInput {
            rows: incoming,
            now: self.now,
            known: BTreeMap::new(),
            members: BTreeMap::new(),
            ledger: Some(self.ledger.clone()),
            epochs: BTreeMap::new(),
            holder: holder.clone(),
            peer: self.partner.clone(),
        })
        .unwrap();
        assert!(admitted.clock_ahead.is_none());
        assert!(admitted.refused.is_empty(), "{:?}", admitted.refused);
        self.ledger = admitted.ledger;
        for row in &admitted.accepted {
            self.clock = Some(
                myriad_persona_log::clock::observe(
                    self.clock.as_ref(),
                    &row.hlc,
                    self.now,
                    0,
                    &self.id,
                )
                .unwrap(),
            );
        }
        let merged = merge(MergeInput {
            log: self.log(audience),
            incoming: admitted.accepted,
            ledger: self.ledger.clone(),
            holder: Some(holder),
        })
        .unwrap();
        assert!(merged.conflicts.is_empty(), "{:?}", merged.conflicts);
        for row in merged.log.iter().filter(|row| row.forgotten) {
            if let Some(payload) = &row.payload {
                self.payloads.remove(&payload.reference);
            }
        }
        for id in &merged.stubbed {
            self.payloads.remove(&format!("blob:{id}"));
        }
        self.logs.insert(audience.to_string(), merged.log);
    }

    fn frame(&mut self, t: &str, id: Option<&str>, body: Value) -> Value {
        self.seq += 1;
        let mut frame = json!({ "v": 1, "t": t, "seq": self.seq, "ack": 0, "at": 1, "body": body });
        if let Some(id) = id {
            frame["id"] = json!(id);
        }
        frame
    }

    fn vector(&mut self, audience: &str) -> Value {
        let holdings = vector::holdings(&self.log(audience)).unwrap();
        self.frame(
            "log.vector",
            None,
            json!({ "audience": audience, "vector": holdings.vector }),
        )
    }

    /// This side's bodies of `rows`, by row id, as read.
    fn bodies(&self, rows: &[Row]) -> BTreeMap<String, Value> {
        rows.iter()
            .filter_map(|row| {
                let data = self.payloads.get(&row.payload.as_ref()?.reference)?;
                Some((row.id.clone(), serde_json::from_slice(data).ok()?))
            })
            .collect()
    }

    /// `log.rows` frames for the server, as a PC sends them
    /// (`nodes.wire_send` with the server as the other side): what of
    /// `audience` it lacks past `theirs`, then the rows it asks again for.
    fn send(&mut self, audience: &str, theirs: &Vector, ask: Vec<String>) -> Vec<Value> {
        let rows = self.log(audience);
        let sent = wire::send(SendInput {
            audience: audience.to_string(),
            bodies: self.bodies(&rows),
            rows,
            theirs: theirs.clone(),
            ask,
            holder: Holder {
                core: Core::Server,
                owner: self.partner.clone(),
            },
            here: self.partner.clone(),
            peer_abi: 2,
            ledger: self.ledger.clone(),
            elsewhere: Vec::new(),
        })
        .unwrap();
        sent.batches
            .into_iter()
            .map(|batch| {
                let id = format!("batch_{}", self.seq + 1);
                self.frame("log.rows", Some(&id), serde_json::to_value(batch).unwrap())
            })
            .collect()
    }

    /// `rows`, and no others, in one `log.rows` frame.
    fn rows_frame(&mut self, audience: &str, rows: Vec<Row>) -> Value {
        let mut batch = wire::Batch {
            audience: audience.to_string(),
            rows: Vec::new(),
            payloads: Vec::new(),
            withheld: Vec::new(),
        };
        for row in rows {
            let carried = row
                .payload
                .as_ref()
                .filter(|_| !row.forgotten)
                .and_then(|payload| {
                    self.payloads
                        .get(&payload.reference)
                        .map(|data| wire::Carried {
                            reference: payload.reference.clone(),
                            data: BASE64.encode(data),
                        })
                });
            batch.payloads.extend(carried);
            batch.rows.push(row);
        }
        let id = format!("batch_{}", self.seq + 1);
        self.frame("log.rows", Some(&id), serde_json::to_value(batch).unwrap())
    }

    /// Takes one frame from the server; the frames it sends back.
    fn take(&mut self, frame: &Value) -> Vec<Value> {
        let body = &frame["body"];
        match frame["t"].as_str().unwrap() {
            "log.rows" => {
                let audience = body["audience"].as_str().unwrap().to_string();
                let rows: Vec<Row> = serde_json::from_value(body["rows"].clone()).unwrap();
                for payload in body["payloads"].as_array().into_iter().flatten() {
                    self.payloads.insert(
                        payload["ref"].as_str().unwrap().to_string(),
                        BASE64.decode(payload["data"].as_str().unwrap()).unwrap(),
                    );
                }
                self.merge(&audience, rows);
                Vec::new()
            }
            "log.vector" => {
                let audience = body["audience"].as_str().unwrap().to_string();
                let theirs: Vector = serde_json::from_value(body["vector"].clone()).unwrap();
                let ask: Vec<String> =
                    serde_json::from_value(body.get("ask").cloned().unwrap_or(json!([]))).unwrap();
                self.send(&audience, &theirs, ask)
            }
            "lease.grant" => {
                let grant: Grant = serde_json::from_value(body["grant"].clone()).unwrap();
                if grant.held_by(&self.id) {
                    self.epoch = Some(grant.epoch);
                }
                Vec::new()
            }
            "lease.epoch" => {
                if body["holder"] != json!(self.id) {
                    self.epoch = None;
                }
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    fn fold(&self, audience: &str, leases: &[Grant]) -> Vec<u8> {
        let folded = fold(FoldInput {
            rows: self.log(audience),
            leases: leases.to_vec(),
            ledger: self.ledger.clone(),
        })
        .unwrap();
        canonical_bytes(&serde_json::to_value(folded).unwrap())
    }
}

/// Everything the server sends: checked against the contract, read back.
fn delivered(out: &Outgoing) -> Value {
    let text = relay::encode(out, 1, 0, T0 / 1000).unwrap();
    serde_json::from_str(&text).unwrap()
}

/// Sends `frames` from `side` until neither has more to say. Frames to
/// another device are not delivered here; their count is returned.
async fn exchange(
    db: &DatabaseConnection,
    side: &mut Side,
    mut inbox: Vec<Value>,
    now: i64,
) -> Vec<Outgoing> {
    let root = root();
    let mut elsewhere = Vec::new();
    for _ in 0..16 {
        if inbox.is_empty() {
            return elsewhere;
        }
        let mut next = Vec::new();
        for frame in inbox.drain(..) {
            let device = side.id.clone();
            let outs = relay::take(
                db,
                &root,
                &on(),
                side.user,
                &device,
                Core::Pc,
                &mut side.link,
                &frame,
                now,
            )
            .await
            .unwrap();
            for out in outs {
                let for_it = match &out.to {
                    To::Sender | To::Everyone => true,
                    To::Device(id) => *id == side.id,
                    To::Hosts => false,
                };
                if for_it {
                    next.extend(side.take(&delivered(&out)));
                } else {
                    elsewhere.push(out);
                }
            }
        }
        inbox = next;
    }
    panic!("the exchange did not settle");
}

async fn sync(db: &DatabaseConnection, side: &mut Side, audience: &str, now: i64) {
    side.now = side.now.max(now);
    let vector = side.vector(audience);
    exchange(db, side, vec![vector], now).await;
}

async fn server_fold(db: &DatabaseConnection, audience: &str, leases: &[Grant]) -> Vec<u8> {
    let folded = fold(FoldInput {
        rows: store::rows(db, OWNER, audience).await.unwrap(),
        leases: leases.to_vec(),
        ledger: store::ledger(db, OWNER, audience).await.unwrap(),
    })
    .unwrap();
    canonical_bytes(&serde_json::to_value(folded).unwrap())
}

async fn count(db: &DatabaseConnection, sql: &str) -> i64 {
    db.query_one_raw(sea_orm::Statement::from_string(
        sea_orm::DatabaseBackend::Postgres,
        sql.to_string(),
    ))
    .await
    .unwrap()
    .unwrap()
    .try_get_by_index::<i64>(0)
    .unwrap()
}

/// Two host cores, one of them her mind, push and pull through the server,
/// one forgets while the other writes offline; afterwards both and the
/// server fold every audience to the same bytes, and what was forgotten is
/// gone everywhere, its key on the server too.
#[tokio::test]
async fn two_devices_fold_the_same_after_pushing_and_pulling_through_the_server() {
    let Some(schema) = schema("her_log_two").await else {
        return;
    };
    let db = &schema.db;
    let mut mac = Side::new("dev_mac");
    let mut win = Side::new("dev_win");

    // The Mac asks for her mind (the owner talks to it there): the server
    // holds her and hands her over.
    let ask = mac.frame("lease.ask", None, json!({ "why": "talk" }));
    exchange(db, &mut mac, vec![ask], T0).await;
    assert_eq!(
        mac.epoch,
        Some(2),
        "first grant was the server's, then the Mac's"
    );

    mac.write(
        PERSONAL,
        "node:lamp",
        "node.created",
        Author::Her,
        Some("a lamp"),
        T0,
    );
    mac.write(
        PERSONAL,
        "node:lamp",
        "node.span",
        Author::Her,
        Some("too bright"),
        T0 + 1,
    );
    mac.write(
        PERSONAL,
        "memory:cats",
        "memory.write",
        Author::Her,
        Some("likes cats"),
        T0 + 2,
    );
    mac.write(
        OWN,
        "affect:day",
        "affect.change",
        Author::Her,
        Some("calm"),
        T0 + 3,
    );
    win.write(
        PERSONAL,
        "persona:name",
        "persona.field",
        Author::Owner,
        Some("Merope"),
        T0 + 4,
    );

    for audience in [PERSONAL, OWN] {
        sync(db, &mut mac, audience, T0 + MINUTE).await;
        sync(db, &mut win, audience, T0 + MINUTE).await;
        sync(db, &mut mac, audience, T0 + MINUTE).await;
    }
    assert_eq!(mac.log(PERSONAL).len(), 4);
    assert_eq!(win.log(PERSONAL).len(), 4);

    // The owner forgets the lamp on Windows while the Mac, offline, changes
    // its view of it once more.
    let forget = win.write(
        PERSONAL,
        "node:lamp",
        "forget",
        Author::Owner,
        None,
        T0 + 2 * MINUTE,
    );
    mac.write(
        PERSONAL,
        "node:lamp",
        "node.span",
        Author::Her,
        Some("dimmer now"),
        T0 + 2 * MINUTE,
    );
    // Windows first: the server learns the forget before the Mac's row.
    sync(db, &mut win, PERSONAL, T0 + 3 * MINUTE).await;
    sync(db, &mut mac, PERSONAL, T0 + 3 * MINUTE).await;
    sync(db, &mut win, PERSONAL, T0 + 3 * MINUTE).await;

    let leases = lease::history(db).await.unwrap();
    for audience in [PERSONAL, OWN] {
        let on_server = server_fold(db, audience, &leases).await;
        assert_eq!(
            mac.fold(audience, &leases),
            on_server,
            "{audience}: the Mac"
        );
        assert_eq!(
            win.fold(audience, &leases),
            on_server,
            "{audience}: Windows"
        );
    }
    let folded: Value = serde_json::from_slice(&server_fold(db, PERSONAL, &leases).await).unwrap();
    let lamp = folded["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|object| object["object"] == "node:lamp")
        .unwrap();
    assert_eq!(
        lamp,
        &json!({ "audience": PERSONAL, "object": "node:lamp", "forgotten": true })
    );
    // Every lamp row a stub on every side, its payloads gone everywhere.
    for side in [&mac, &win] {
        assert!(
            side.log(PERSONAL)
                .iter()
                .filter(|row| row.object == "node:lamp" && row.id != forget.id)
                .all(|row| row.forgotten && row.payload.is_none())
        );
        assert!(side.payloads.values().all(|data| !says(data, "too bright")));
        assert!(side.payloads.values().all(|data| !says(data, "dimmer")));
    }
    assert_eq!(
        count(db, "SELECT COUNT(*) FROM agent_log_payloads p JOIN agent_log_rows r ON r.id = p.row_id WHERE r.object = 'node:lamp'").await,
        0
    );
    assert_eq!(
        count(
            db,
            "SELECT COUNT(*) FROM agent_log_object_keys WHERE object = 'node:lamp'"
        )
        .await,
        0,
        "the lamp's key is gone"
    );
    // What was not forgotten still reads back on the other side, byte for
    // byte as the Mac wrote it.
    let cats = |side: &Side| {
        side.payloads
            .values()
            .filter(|data| says(data, "likes cats"))
            .cloned()
            .collect::<Vec<_>>()
    };
    assert_eq!(cats(&win).len(), 1);
    assert_eq!(cats(&win), cats(&mac));
    // A kind the core has no shape for travels as a row only: its body
    // stays with the side that wrote it.
    assert_eq!(
        count(db, "SELECT COUNT(*) FROM agent_log_payloads p JOIN agent_log_rows r ON r.id = p.row_id WHERE r.object = 'persona:name'").await,
        0
    );
    assert!(mac.payloads.values().all(|data| !says(data, "Merope")));
    // Nothing is in the clear at rest.
    assert_eq!(
        count(
            db,
            "SELECT COUNT(*) FROM agent_log_payloads WHERE position('cats'::bytea in sealed) > 0"
        )
        .await,
        0
    );
    schema.drop().await;
}

/// A batch with a row more than five minutes ahead is refused whole, with
/// the server's time in the error, and nothing of it is kept.
#[tokio::test]
async fn a_clock_too_far_ahead_is_refused_whole_with_the_servers_time() {
    let Some(schema) = schema("her_log_ahead").await else {
        return;
    };
    let db = &schema.db;
    let mut win = Side::new("dev_win");
    let fine = win.write(
        PERSONAL,
        "persona:name",
        "persona.field",
        Author::Owner,
        Some("a"),
        T0,
    );
    let ahead = win.write(
        PERSONAL,
        "persona:mood",
        "persona.field",
        Author::Owner,
        Some("b"),
        T0 + 10 * MINUTE,
    );
    let frame = win.rows_frame(PERSONAL, vec![fine, ahead]);
    let outs = relay::take(
        db,
        &root(),
        &on(),
        OWNER,
        "dev_win",
        Core::Pc,
        &mut relay::Session::default(),
        &frame,
        T0,
    )
    .await
    .unwrap();
    assert_eq!(outs.len(), 1);
    let error = delivered(&outs[0]);
    assert_eq!(error["t"], "error");
    assert_eq!(error["body"]["code"], "clock_ahead");
    assert_eq!(error["body"]["serverTime"], T0);
    assert_eq!(error["body"]["ref"], frame["seq"]);
    assert_eq!(count(db, "SELECT COUNT(*) FROM agent_log_rows").await, 0);
    schema.drop().await;
}

/// The owner forgets on the server: the payload and the object's key go at
/// once, and a side that holds nothing gets the forget before anything.
#[tokio::test]
async fn a_forget_on_the_server_deletes_the_key_and_is_answered_first() {
    let Some(schema) = schema("her_log_forget").await else {
        return;
    };
    let db = &schema.db;
    // The server stamps its forget with its own time: the sides live now.
    let t0 = super::now();
    let mut mac = Side::new("dev_mac");
    mac.now = t0;
    mac.write(
        PERSONAL,
        "memory:tea",
        "memory.write",
        Author::Owner,
        Some("likes tea"),
        t0,
    );
    mac.write(
        PERSONAL,
        "memory:trip",
        "memory.write",
        Author::Owner,
        Some("Kyoto"),
        t0 + 1,
    );
    sync(db, &mut mac, PERSONAL, t0 + MINUTE).await;
    assert_eq!(
        count(db, "SELECT COUNT(*) FROM agent_log_payloads").await,
        2
    );

    let forget = store::forget(db, &owner(db).await, PERSONAL, "memory:trip")
        .await
        .unwrap();
    assert_eq!(forget.origin, NODE);
    assert_eq!(
        count(db, "SELECT COUNT(*) FROM agent_log_payloads").await,
        1
    );
    assert_eq!(
        count(
            db,
            "SELECT COUNT(*) FROM agent_log_object_keys WHERE object = 'memory:trip'"
        )
        .await,
        0
    );
    assert_eq!(store::ledger(db, OWNER, PERSONAL).await.unwrap().len(), 1);

    let answered = store::send(
        db,
        &root(),
        &owner(db).await,
        PERSONAL,
        &Vector::new(),
        Vec::new(),
        &pc(),
        2,
    )
    .await
    .unwrap();
    assert_eq!(answered.batches.len(), 1);
    let batch = &answered.batches[0];
    assert_eq!(batch.rows[0].kind, "forget");
    let trip = batch
        .rows
        .iter()
        .find(|row| row.object == "memory:trip" && row.kind != "forget")
        .unwrap();
    assert!(trip.forgotten && trip.payload.is_none());
    let tea = batch
        .rows
        .iter()
        .find(|row| row.object == "memory:tea")
        .unwrap();
    let carried = batch
        .payloads
        .iter()
        .find(|one| one.reference == tea.payload.as_ref().unwrap().reference)
        .unwrap();
    assert!(says(&BASE64.decode(&carried.data).unwrap(), "likes tea"));
    assert!(batch.withheld.is_empty(), "{:?}", batch.withheld);

    // The Mac gets the forget and loses the trip too.
    sync(db, &mut mac, PERSONAL, t0 + 2 * MINUTE).await;
    assert!(mac.payloads.values().all(|data| !says(data, "Kyoto")));
    let leases = lease::history(db).await.unwrap();
    assert_eq!(
        mac.fold(PERSONAL, &leases),
        server_fold(db, PERSONAL, &leases).await
    );
    schema.drop().await;
}

/// A group's log is the server's: no computer gets its rows, not even the
/// one her mind is on (what a group says reaches her mind as `group.lend`),
/// and only her mind may add to it.
#[tokio::test]
async fn a_groups_rows_go_to_no_computer_and_come_back_only_from_her_mind() {
    let Some(schema) = schema("her_log_group").await else {
        return;
    };
    let db = &schema.db;
    let mut mac = Side::new("dev_mac");
    let mut win = Side::new("dev_win");
    let ask = mac.frame("lease.ask", None, json!({ "why": "talk" }));
    exchange(db, &mut mac, vec![ask], T0).await;
    assert!(mac.epoch.is_some());

    // Her mind on the Mac writes in the group: kept on the server.
    mac.write(
        GROUP,
        "case:loud",
        "case.verdict",
        Author::Her,
        Some("too loud"),
        T0,
    );
    let vector = mac.vector(GROUP);
    let outs = relay::take(
        db,
        &root(),
        &on(),
        OWNER,
        "dev_mac",
        Core::Pc,
        &mut relay::Session::default(),
        &vector,
        T0,
    )
    .await
    .unwrap();
    assert_eq!(delivered(&outs[0])["body"]["code"], "not_holder");
    let frame = mac.rows_frame(GROUP, mac.log(GROUP));
    exchange(db, &mut mac, vec![frame], T0 + MINUTE).await;
    assert_eq!(store::rows(db, OWNER, GROUP).await.unwrap().len(), 1);

    // Windows is not her mind: neither its vector nor its rows are taken.
    let vector = win.vector(GROUP);
    let outs = relay::take(
        db,
        &root(),
        &on(),
        OWNER,
        "dev_win",
        Core::Pc,
        &mut relay::Session::default(),
        &vector,
        T0,
    )
    .await
    .unwrap();
    assert_eq!(delivered(&outs[0])["body"]["code"], "not_holder");
    let row = win.write(
        GROUP,
        "case:quiet",
        "case.verdict",
        Author::Owner,
        Some("x"),
        T0,
    );
    let frame = win.rows_frame(GROUP, vec![row]);
    let outs = relay::take(
        db,
        &root(),
        &on(),
        OWNER,
        "dev_win",
        Core::Pc,
        &mut relay::Session::default(),
        &frame,
        T0,
    )
    .await
    .unwrap();
    assert_eq!(delivered(&outs[0])["body"]["code"], "not_holder");
    assert_eq!(store::rows(db, OWNER, GROUP).await.unwrap().len(), 1);
    // A change in a group is not announced to the other host cores.
    mac.write(
        GROUP,
        "case:loud",
        "case.outcome",
        Author::Her,
        None,
        T0 + 2,
    );
    let frame = mac.rows_frame(GROUP, mac.log(GROUP));
    let outs = relay::take(
        db,
        &root(),
        &on(),
        OWNER,
        "dev_mac",
        Core::Pc,
        &mut relay::Session::default(),
        &frame,
        T0 + MINUTE,
    )
    .await
    .unwrap();
    assert!(outs.iter().all(|out| out.to == To::Sender));

    // Counts are per audience: the Mac's group rows take no place in its
    // private count, so Windows, which never gets the group, holds all of
    // the Mac's private rows without a gap.
    mac.write(
        PERSONAL,
        "node:lamp",
        "node.created",
        Author::Her,
        Some("a lamp"),
        T0 + 3,
    );
    sync(db, &mut mac, PERSONAL, T0 + MINUTE).await;
    sync(db, &mut win, PERSONAL, T0 + MINUTE).await;
    let held = vector::holdings(&win.log(PERSONAL)).unwrap();
    assert_eq!(held.vector.get("dev_mac"), Some(&1));
    assert!(held.beyond.is_empty(), "{:?}", held.beyond);
    // Giving her up, the Mac says its last count in each audience it wrote
    // in; the grant keeps them as they were.
    let through = json!({ PERSONAL: 1, GROUP: 2 });
    let yielded = mac.frame(
        "lease.yield",
        Some("yield_1"),
        json!({ "epoch": mac.epoch, "through": through }),
    );
    exchange(db, &mut mac, vec![yielded], T0 + 2 * MINUTE).await;
    let standing = lease::standing(db).await.unwrap().unwrap();
    assert_eq!(
        (standing.holder.as_deref(), standing.how),
        (Some(NODE), myriad_persona_log::How::Handover)
    );
    assert_eq!(json!(standing.through), through);
    // A count for the whole side, not per audience, is not a yield.
    let ask = mac.frame("lease.ask", None, json!({ "why": "talk" }));
    exchange(db, &mut mac, vec![ask], T0 + 3 * MINUTE).await;
    let old = mac.frame(
        "lease.yield",
        Some("yield_2"),
        json!({ "epoch": mac.epoch, "through": 3 }),
    );
    let outs = relay::take(
        db,
        &root(),
        &on(),
        OWNER,
        "dev_mac",
        Core::Pc,
        &mut relay::Session::default(),
        &old,
        T0 + 3 * MINUTE,
    )
    .await
    .unwrap();
    assert_eq!(delivered(&outs[0])["body"]["code"], "bad_frame");
    schema.drop().await;
}

/// What a group or a private chat says is lent only to the PC her mind is
/// on, and only once its link speaks protocol 2 (none does yet): until then
/// the site holds it as before. A say for a place never lent is answered
/// with its code and nothing is sent; one from a device her mind is not on
/// is refused by the contract.
#[tokio::test]
async fn chats_are_lent_only_to_her_mind_on_a_pc_whose_link_speaks_protocol_two() {
    let Some(schema) = schema("her_log_lend").await else {
        return;
    };
    let db = &schema.db;
    // Her mind is on the server: nothing to lend to.
    assert_eq!(lend::mind_on_a_pc_if(db, true).await, None);
    let mut mac = Side::new("dev_mac");
    let ask = mac.frame("lease.ask", None, json!({ "why": "talk" }));
    exchange(db, &mut mac, vec![ask], T0).await;
    assert!(mac.epoch.is_some());
    // The Mac holds her, but its link does not speak protocol 2.
    assert_eq!(lend::mind_on_a_pc_if(db, true).await, None);
    let (send, _lent) = tokio::sync::mpsc::unbounded_channel();
    lend::open("dev_mac", send);
    assert_eq!(
        lend::mind_on_a_pc_if(db, true).await.as_deref(),
        Some("dev_mac")
    );
    // Her log off on the server: closed.
    assert_eq!(lend::mind_on_a_pc_if(db, false).await, None);

    let say = mac.frame(
        "group.say",
        Some("gsay_1"),
        json!({ "group": GROUP, "text": "嗯" }),
    );
    let outs = relay::take(
        db,
        &root(),
        &on(),
        OWNER,
        "dev_mac",
        Core::Pc,
        &mut relay::Session::default(),
        &say,
        T0,
    )
    .await
    .unwrap();
    let said = delivered(&outs[0]);
    assert_eq!(said["t"], "group.said");
    assert_eq!(
        said["body"],
        json!({ "sayId": "gsay_1", "sent": false, "code": "group_unknown" })
    );
    let mut win = Side::new("dev_win");
    let say = win.frame(
        "group.say",
        Some("gsay_2"),
        json!({ "group": GROUP, "text": "嗯" }),
    );
    let outs = relay::take(
        db,
        &root(),
        &on(),
        OWNER,
        "dev_win",
        Core::Pc,
        &mut relay::Session::default(),
        &say,
        T0,
    )
    .await
    .unwrap();
    assert_eq!(delivered(&outs[0])["body"]["code"], "bad_frame");
    lend::close("dev_mac");
    assert_eq!(lend::mind_on_a_pc_if(db, true).await, None);
    schema.drop().await;
}

/// Off, nothing answers; the owner not having turned it on is the same.
#[tokio::test]
async fn while_off_no_frame_is_taken() {
    let Some(schema) = schema("her_log_off").await else {
        return;
    };
    let db = &schema.db;
    let mut win = Side::new("dev_win");
    let vector = win.vector(PERSONAL);
    let off = Settings::default();
    assert!(matches!(
        relay::take(
            db,
            &root(),
            &off,
            OWNER,
            "dev_win",
            Core::Pc,
            &mut relay::Session::default(),
            &vector,
            T0
        )
        .await,
        Err(super::Error::Off)
    ));
    store::stop(db, OWNER).await.unwrap();
    assert!(matches!(
        relay::take(
            db,
            &root(),
            &on(),
            OWNER,
            "dev_win",
            Core::Pc,
            &mut relay::Session::default(),
            &vector,
            T0
        )
        .await,
        Err(super::Error::Off)
    ));
    assert_eq!(
        count(db, "SELECT COUNT(*) FROM agent_lease_grants").await,
        0
    );
    schema.drop().await;
}

/// The holder goes quiet, the owner shows up somewhere else: the server
/// takes her over only once the stop time has passed for both, tells the
/// devices with the stop time, and tells the old holder on its return.
#[tokio::test]
async fn the_server_takes_over_only_when_the_holder_has_stopped() {
    let Some(schema) = schema("her_log_takeover").await else {
        return;
    };
    let db = &schema.db;
    let settings = on();
    let mut mac = Side::new("dev_mac");
    let ask = mac.frame("lease.ask", None, json!({ "why": "talk" }));
    exchange(db, &mut mac, vec![ask], T0).await;
    lease::place(db, "dev_mac", Some("home"), true, T0)
        .await
        .unwrap();
    mac.write(
        PERSONAL,
        "node:walk",
        "node.created",
        Author::Her,
        Some("a walk"),
        T0,
    );
    sync(db, &mut mac, PERSONAL, T0).await;

    // The owner's phone, a child, sees them out from a minute later.
    lease::place(db, "dev_phone", Some("office"), true, T0 + MINUTE)
        .await
        .unwrap();
    for minutes in [1, 3] {
        lease::place(db, "dev_phone", Some("office"), true, T0 + minutes * MINUTE)
            .await
            .unwrap();
        assert_eq!(
            lease::watch(db, &settings, T0 + minutes * MINUTE)
                .await
                .unwrap(),
            None,
            "{minutes} minutes"
        );
    }
    lease::place(db, "dev_phone", Some("office"), true, T0 + 5 * MINUTE)
        .await
        .unwrap();
    let grant = lease::watch(db, &settings, T0 + 5 * MINUTE)
        .await
        .unwrap()
        .expect("taken over");
    assert_eq!(
        (grant.holder.as_deref(), grant.how),
        (Some(NODE), myriad_persona_log::How::Takeover)
    );

    let welcome = relay::welcome(db, &settings, "dev_mac").await.unwrap();
    let frames: Vec<Value> = welcome.iter().map(delivered).collect();
    assert_eq!(frames[0]["t"], "lease.epoch");
    assert_eq!(frames[0]["body"]["stopAfter"], settings.stop_after_micros);
    assert_eq!(frames[1]["t"], "lease.takeover");
    assert_eq!(frames[1]["body"]["through"], json!({ PERSONAL: 1 }));
    // The phone, which never held her, is only told where she is.
    assert_eq!(
        relay::welcome(db, &settings, "dev_phone")
            .await
            .unwrap()
            .len(),
        1
    );
    schema.drop().await;
}

/// Her mind on the Mac is taken over while it is out of reach; what it
/// wrote before it stopped and had not sent comes up once it is back, in
/// the partner's private talk and in a group alike: taken (not refused as
/// from a computer that is not her mind), and folded on the server as hers,
/// one her catching up, not two. Until then the server's her did not know
/// it.
#[tokio::test]
async fn what_her_mind_wrote_before_a_takeover_is_taken_when_it_comes_back() {
    let Some(schema) = schema("her_log_takeover_late").await else {
        return;
    };
    let db = &schema.db;
    let settings = on();
    let mut mac = Side::new("dev_mac");
    let ask = mac.frame("lease.ask", None, json!({ "why": "talk" }));
    exchange(db, &mut mac, vec![ask], T0).await;
    lease::place(db, "dev_mac", Some("home"), true, T0)
        .await
        .unwrap();
    mac.write(
        PERSONAL,
        "node:walk",
        "node.created",
        Author::Her,
        Some("a walk"),
        T0,
    );
    sync(db, &mut mac, PERSONAL, T0).await;
    // Written before it went out of reach, never sent.
    let late = mac.write(
        PERSONAL,
        "node:rain",
        "node.created",
        Author::Her,
        Some("rain"),
        T0 + 1,
    );
    let late_group = mac.write(
        GROUP,
        "case:loud",
        "case.verdict",
        Author::Her,
        Some("too loud"),
        T0 + 2,
    );

    for minutes in [1, 3, 5] {
        lease::place(db, "dev_phone", Some("office"), true, T0 + minutes * MINUTE)
            .await
            .unwrap();
    }
    lease::watch(db, &settings, T0 + 5 * MINUTE)
        .await
        .unwrap()
        .expect("taken over");
    assert!(
        !store::rows(db, OWNER, PERSONAL)
            .await
            .unwrap()
            .iter()
            .any(|row| row.id == late.id)
    );

    // Back: it sends what it wrote.
    sync(db, &mut mac, PERSONAL, T0 + 6 * MINUTE).await;
    let frame = mac.rows_frame(GROUP, mac.log(GROUP));
    exchange(db, &mut mac, vec![frame], T0 + 6 * MINUTE).await;
    let history = lease::history(db).await.unwrap();
    for (audience, id, object) in [
        (PERSONAL, &late.id, "node:rain"),
        (GROUP, &late_group.id, "case:loud"),
    ] {
        let rows = store::rows(db, OWNER, audience).await.unwrap();
        assert!(rows.iter().any(|row| &row.id == id), "{audience}");
        assert!(
            myriad_persona_log::lease::standings(&rows, &history).is_empty(),
            "{audience}: hers, standing normal"
        );
        let folded = fold(FoldInput {
            rows,
            leases: history.clone(),
            ledger: store::ledger(db, OWNER, audience).await.unwrap(),
        })
        .unwrap();
        let folded = serde_json::to_value(folded).unwrap();
        let object = folded["objects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|folded| folded["object"] == object)
            .unwrap_or_else(|| panic!("{audience}: {object} folded"));
        assert!(object["rows"].as_array().unwrap().contains(&json!(id)));
        assert_ne!(object["concurrent"], true, "{audience}");
    }

    // A row of hers at an epoch it never held is still not taken there.
    let mut win = Side::new("dev_win");
    win.epoch = history
        .iter()
        .find(|grant| grant.held_by("dev_mac"))
        .map(|grant| grant.epoch);
    let stray = win.write(
        GROUP,
        "case:quiet",
        "case.verdict",
        Author::Her,
        Some("x"),
        T0 + 6 * MINUTE,
    );
    let frame = win.rows_frame(GROUP, vec![stray]);
    let outs = relay::take(
        db,
        &root(),
        &settings,
        OWNER,
        "dev_win",
        Core::Pc,
        &mut relay::Session::default(),
        &frame,
        T0 + 6 * MINUTE,
    )
    .await
    .unwrap();
    assert_eq!(delivered(&outs[0])["body"]["code"], "not_holder");
    schema.drop().await;
}

/// What the partner says to her in private goes into her log as their own
/// lines (D6), with her mind wherever it is: one segment a conversation,
/// opened before its first line, each line with its message's id and the
/// site's pictures counted out of it. A group's talk, Work and an empty line
/// are not written. The Mac her mind is on takes them as the owner's.
#[tokio::test]
async fn the_partners_private_lines_are_theirs_in_her_log_wherever_her_mind_is() {
    let Some(schema) = schema("her_log_partner_lines").await else {
        return;
    };
    let db = &schema.db;
    db.execute_unprepared(&format!(
        "INSERT INTO agent_sessions (id, user_id, title, context, message_count, archived, \
         created_at, last_active_at) VALUES \
         ('ses_a', {OWNER}, 'a', '{{\"mode\":\"chat\"}}', 0, false, NOW(), NOW()), \
         ('ses_g', {OWNER}, 'g', '{{\"mode\":\"chat\",\"venue\":\"telegram:-100\"}}', 0, false, NOW(), NOW()), \
         ('ses_w', {OWNER}, 'w', '{{\"mode\":\"work\"}}', 0, false, NOW(), NOW())"
    ))
    .await
    .unwrap();
    let mut mac = Side::new("dev_mac");
    let ask = mac.frame("lease.ask", None, json!({ "why": "talk" }));
    exchange(db, &mut mac, vec![ask], T0).await;
    assert!(mac.epoch.is_some(), "her mind is on the Mac");

    let at = chrono::Utc::now();
    let root = root();
    let keep = |session: &'static str, message: i32, text: &'static str| {
        super::talk::keep(db, &root, OWNER, session, message, text, at, "web")
    };
    assert!(
        keep("ses_a", 1, "早 ![猫](/api/media/1)")
            .await
            .unwrap()
            .is_some()
    );
    assert!(keep("ses_a", 2, "在吗").await.unwrap().is_some());
    for (session, message, text) in [
        ("ses_a", 3, "   "),
        ("ses_g", 4, "群里的话"),
        ("ses_w", 5, "办事"),
        ("ses_none", 6, "没有这场"),
    ] {
        assert!(
            keep(session, message, text).await.unwrap().is_none(),
            "{session}"
        );
    }
    let rows = store::rows(db, OWNER, PERSONAL).await.unwrap();
    assert_eq!(
        rows.iter()
            .map(|row| (
                row.kind.as_str(),
                row.object.as_str(),
                row.author,
                row.epoch
            ))
            .collect::<Vec<_>>(),
        vec![
            ("segment.open", "segment:site_ses_a", Author::Owner, None),
            ("segment.line", "segment:site_ses_a", Author::Owner, None),
            ("segment.line", "segment:site_ses_a", Author::Owner, None),
        ]
    );
    // The Mac her mind is on takes them as the owner's (the site writes
    // them at its own time).
    sync(db, &mut mac, PERSONAL, super::now()).await;
    let lines: Vec<&Row> = mac
        .log(PERSONAL)
        .iter()
        .filter(|row| row.kind == "segment.line" && row.origin == NODE)
        .map(|row| rows.iter().find(|kept| kept.id == row.id).unwrap())
        .collect();
    assert_eq!(lines.len(), 2);
    assert!(
        mac.payloads
            .values()
            .any(|data| says(data, "\"pictures\":1")
                && says(data, "\"text\":\"早\"")
                && says(data, "\"line\":\"1\""))
    );
    assert!(
        mac.payloads
            .values()
            .all(|data| !says(data, "/api/media/1"))
    );
    schema.drop().await;
}

/// What the partner and she said in private on the Mac shows in the site's
/// conversations as copies (D6-e): a segment goes on with the conversation
/// last talked in within six hours before it, or opens a new one, and her
/// line goes where the talk was; each copy is at its row's time, says where
/// it was said and how many pictures came, and is copied once. Copies are
/// never written to the log again, and go when their row is forgotten.
#[tokio::test]
async fn lines_said_on_the_mac_show_in_the_sites_conversations() {
    let Some(schema) = schema("her_log_partner_copies").await else {
        return;
    };
    let db = &schema.db;
    let at = |micros: i64| format!("to_timestamp({}::double precision / 1000000)", micros);
    db.execute_unprepared(&format!(
        "INSERT INTO agent_sync_devices (id, user_id, name, token_hash, token_hint, created_at) \
         VALUES ('dev_mac', {OWNER}, 'Mac', 'h', 'x', NOW()); \
         INSERT INTO agent_sessions (id, user_id, title, context, message_count, archived, \
         created_at, last_active_at) VALUES \
         ('ses_a', {OWNER}, 'a', '{{\"mode\":\"chat\"}}', 1, false, {t}, {t}); \
         INSERT INTO agent_messages (session_id, role, content, created_at) \
         VALUES ('ses_a', 'user', '早', {t})",
        t = at(T0 - 60 * MINUTE)
    ))
    .await
    .unwrap();
    let mut mac = Side::new("dev_mac");
    let ask = mac.frame("lease.ask", None, json!({ "why": "talk" }));
    exchange(db, &mut mac, vec![ask], T0).await;
    assert!(mac.epoch.is_some(), "her mind is on the Mac");

    let later = T0 + 7 * 60 * MINUTE;
    for (object, start, line) in [
        ("segment:dev_mac_s1", T0 + MINUTE, "在 Mac 上说的"),
        ("segment:dev_mac_s2", later, "隔了很久"),
    ] {
        let open = json!({ "at": start / 1000, "place": "app" });
        mac.write_body(
            PERSONAL,
            object,
            "segment.open",
            Author::Owner,
            Some(open),
            start,
        );
        let body =
            json!({ "at": (start + MINUTE) / 1000, "text": line, "line": "m1", "pictures": 2 });
        mac.write_body(
            PERSONAL,
            object,
            "segment.line",
            Author::Owner,
            Some(body),
            start + MINUTE,
        );
        let reply = said(&format!("回：{line}"), (start + 2 * MINUTE) / 1000);
        mac.write_body(
            PERSONAL,
            &format!("speech:{object}"),
            "speech.said",
            Author::Her,
            Some(reply),
            start + 2 * MINUTE,
        );
    }
    sync(db, &mut mac, PERSONAL, later + 3 * MINUTE).await;

    let messages = |session: &'static str| async move {
        db.query_all_raw(sea_orm::Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT role, content, origin, metadata, created_at FROM agent_messages \
             WHERE session_id = $1 ORDER BY created_at, id",
            [session.into()],
        ))
        .await
        .unwrap()
        .into_iter()
        .map(|row| {
            let metadata: Option<Value> = row.try_get("", "metadata").unwrap();
            (
                row.try_get::<String>("", "role").unwrap(),
                row.try_get::<String>("", "content").unwrap(),
                row.try_get::<Option<String>>("", "origin").unwrap(),
                metadata.unwrap_or_default(),
            )
        })
        .collect::<Vec<_>>()
    };
    let first = messages("ses_a").await;
    assert_eq!(
        first
            .iter()
            .map(|(role, content, origin, _)| (role.as_str(), content.as_str(), origin.as_deref()))
            .collect::<Vec<_>>(),
        vec![
            ("user", "早", None),
            ("user", "在 Mac 上说的", Some("dev_mac")),
            ("assistant", "回：在 Mac 上说的", Some("dev_mac")),
        ]
    );
    assert_eq!(
        first[1].3,
        json!({ "said": { "on": "dev_mac", "name": "Mac", "place": "app" }, "pictures": 2 })
    );
    assert_eq!(first[2].3["said"]["place"], json!("web"));
    assert!(first[2].3.get("unprompted").is_none());
    // Seven hours on, the talk is a new conversation, her reply with it.
    let sessions = db
        .query_all_raw(sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT object, session_id FROM agent_her_segments ORDER BY object",
        ))
        .await
        .unwrap()
        .into_iter()
        .map(|row| row.try_get::<String>("", "session_id").unwrap())
        .collect::<Vec<_>>();
    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[0], "ses_a");
    let second: &'static str = Box::leak(sessions[1].clone().into_boxed_str());
    assert_ne!(second, "ses_a");
    assert_eq!(
        messages(second)
            .await
            .iter()
            .map(|(role, content, _, _)| (role.as_str(), content.as_str()))
            .collect::<Vec<_>>(),
        vec![("user", "隔了很久"), ("assistant", "回：隔了很久")]
    );
    // Copied once; nothing written to the log here.
    assert_eq!(super::talk::fold(db, &root(), OWNER).await.unwrap(), 0);
    sync(db, &mut mac, PERSONAL, later + 4 * MINUTE).await;
    assert_eq!(messages("ses_a").await.len(), 3);
    assert!(
        store::rows(db, OWNER, PERSONAL)
            .await
            .unwrap()
            .iter()
            .all(|row| row.origin != NODE)
    );
    // Forgotten, its copy goes.
    let line = store::rows(db, OWNER, PERSONAL)
        .await
        .unwrap()
        .into_iter()
        .find(|row| row.kind == "segment.line" && row.object == "segment:dev_mac_s1")
        .unwrap();
    store::forget(db, &owner(db).await, PERSONAL, &format!("row:{}", line.id))
        .await
        .unwrap();
    super::talk::fold(db, &root(), OWNER).await.unwrap();
    assert_eq!(
        messages("ses_a")
            .await
            .iter()
            .map(|(_, content, _, _)| content.as_str())
            .collect::<Vec<_>>(),
        vec!["早", "回：在 Mac 上说的"]
    );
    schema.drop().await;
}

/// Forgetting a line of the owner's private talk is forgetting it on every
/// end (D6-g): a line forgotten here is a forget of its row in her log (the
/// partner's by the row it was written as, hers by what she said), and it
/// goes with the memories that quote it; a row forgotten anywhere takes its
/// line here with what it taught her.
#[tokio::test]
async fn forgetting_a_line_forgets_it_on_every_end_with_what_it_taught_her() {
    let Some(schema) = schema("her_log_forget_line").await else {
        return;
    };
    let db = &schema.db;
    let memory = |id: &str, evidence: &str| {
        format!(
            "INSERT INTO agent_memories (id, user_id, kind, content, evidence, speaker, source, \
             venue, audience, concepts, valid_from, created_at, updated_at) VALUES \
             ('{id}', {OWNER}, 'fact', '{evidence}', '{evidence}', 'user', 'chat', 'private', \
              '[{OWNER}]', '[]', NOW(), NOW() + interval '1 minute', NOW())"
        )
    };
    db.execute_unprepared(&format!(
        "INSERT INTO agent_sessions (id, user_id, title, context, message_count, archived, \
         created_at, last_active_at) VALUES \
         ('ses_a', {OWNER}, 'a', '{{\"mode\":\"chat\"}}', 3, false, NOW(), NOW()); \
         INSERT INTO agent_messages (id, session_id, role, content, created_at) VALUES \
         (901, 'ses_a', 'user', '我养了一只猫叫团子', NOW()), \
         (902, 'ses_a', 'assistant', '团子好可爱', NOW() + interval '1 second'), \
         (903, 'ses_a', 'user', '下周去看海', NOW() + interval '2 seconds'); \
         {}; {}; {}",
        memory("m_cat", "养了一只猫叫团子"),
        memory("m_sea", "下周去看海"),
        memory("m_rain", "喜欢雨天"),
    ))
    .await
    .unwrap();
    let at = chrono::Utc::now();
    let root = root();
    let mut rows = Vec::new();
    for (message, text) in [(901, "我养了一只猫叫团子"), (903, "下周去看海")] {
        let row = super::talk::keep(db, &root, OWNER, "ses_a", message, text, at, "web")
            .await
            .unwrap()
            .unwrap();
        rows.push(row);
    }
    // Her line, as the site's ledger of what she said keeps it.
    let owner = owner(db).await;
    let hers = store::write_owners(
        db,
        &root,
        &owner,
        PERSONAL,
        None,
        "speech:sd1",
        "speech.said",
        &said("团子好可爱", at.timestamp_millis()),
    )
    .await
    .unwrap();
    db.execute_unprepared(&format!(
        "INSERT INTO agent_speech_said (id, audience, user_id, called, at, payload) VALUES \
         ('sd1', 'personal:{OWNER}', {OWNER}, true, NOW(), '{{\"line\":\"902\"}}')"
    ))
    .await
    .unwrap();
    use crate::models::entities::agent_messages;
    use sea_orm::EntityTrait;
    let line = |id: i32| async move {
        agent_messages::Entity::find_by_id(id)
            .one(db)
            .await
            .unwrap()
    };
    assert_eq!(
        line(901).await.unwrap().log_row.as_deref(),
        Some(rows[0].id.as_str())
    );

    // Forgotten here: the line, the memory that quotes it and what that came
    // from (her answer to it), then a forget of each one's row in her log
    // (`forget_lines`, here without the switch it waits on).
    let gone = crate::services::agent::memory::manage::forget_line(db, OWNER, "ses_a", 901)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        gone.iter().map(|line| line.id).collect::<Vec<_>>(),
        vec![901, 902]
    );
    for (line, row) in gone.iter().zip([&rows[0].id, &hers.id]) {
        let forget = super::talk::forget_row_of(db, OWNER, line)
            .await
            .unwrap()
            .expect("a forget of its row");
        assert_eq!(forget.object, format!("row:{row}"));
        // Once is enough.
        assert!(
            super::talk::forget_row_of(db, OWNER, line)
                .await
                .unwrap()
                .is_none()
        );
    }
    let held = store::rows(db, OWNER, PERSONAL).await.unwrap();
    for row in [&rows[0].id, &hers.id] {
        assert!(
            held.iter().any(|kept| kept.id == *row && kept.forgotten),
            "{row}"
        );
    }
    // Forgotten on another end: the next fold takes the line and its memory.
    store::forget(db, &owner, PERSONAL, &format!("row:{}", rows[1].id))
        .await
        .unwrap();
    super::talk::fold(db, &root, OWNER).await.unwrap();
    for gone in [901, 902, 903] {
        assert!(line(gone).await.is_none(), "{gone}");
    }
    let memories = db
        .query_all_raw(sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT id FROM agent_memories ORDER BY id",
        ))
        .await
        .unwrap()
        .into_iter()
        .map(|row| row.try_get::<String>("", "id").unwrap())
        .collect::<Vec<_>>();
    assert_eq!(memories, vec!["m_rain"]);
    // A line the log never had is forgotten here only.
    db.execute_unprepared(
        "INSERT INTO agent_messages (id, session_id, role, content, created_at) \
         VALUES (904, 'ses_a', 'user', '没进日志', NOW())",
    )
    .await
    .unwrap();
    let unlogged = crate::services::agent::memory::origins::Gone::of(&line(904).await.unwrap());
    assert!(
        super::talk::forget_row_of(db, OWNER, &unlogged)
            .await
            .unwrap()
            .is_none()
    );
    schema.drop().await;
}

/// The owner turns syncing her log off while her mind is on the Mac: the
/// server forces it back here, the site's copy being live. Off again with
/// her mind here, or while she lives on a device the site let its copy go
/// to, nothing moves.
#[tokio::test]
async fn syncing_off_brings_her_mind_back_from_a_device() {
    let Some(schema) = schema("her_log_sync_off").await else {
        return;
    };
    let db = &schema.db;
    let settings = on();
    let mut mac = Side::new("dev_mac");
    let ask = mac.frame("lease.ask", None, json!({ "why": "talk" }));
    exchange(db, &mut mac, vec![ask], T0).await;
    let held = mac.epoch.expect("her mind is on the Mac");
    let grant = lease::sync_off(db, &settings, OWNER, T0 + MINUTE)
        .await
        .unwrap()
        .expect("back here");
    assert_eq!(
        (grant.holder.as_deref(), grant.how, grant.epoch),
        (Some(NODE), myriad_persona_log::How::Force, held + 1)
    );
    assert!(!store::account(db, OWNER).await.unwrap().unwrap().on());
    assert_eq!(
        lease::sync_off(db, &settings, OWNER, T0 + 2 * MINUTE)
            .await
            .unwrap(),
        None
    );
    use crate::services::persona_sync::lease as host;
    host::seed(db, "dev_home", "l-1", held as i64 + 2, host::State::Away)
        .await
        .unwrap();
    assert_eq!(
        lease::sync_off(db, &settings, OWNER, T0 + 3 * MINUTE)
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        lease::standing(db)
            .await
            .unwrap()
            .unwrap()
            .holder
            .as_deref(),
        Some("dev_home")
    );
    schema.drop().await;
}

/// The shared lease cases (`core/crates/conformance/v1/lease/move.json`) run
/// through the server's wiring: the lease, the holder and the owner's
/// sighting put where the server keeps them, the move asked as the server
/// asks it. Every case answers as recorded.
#[tokio::test]
async fn the_lease_conformance_cases_pass_through_the_servers_wiring() {
    let Some(schema) = schema("her_log_lease_cases").await else {
        return;
    };
    let db = &schema.db;
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../core/crates/conformance/v1/lease/move.json");
    let file: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(file["call"], "lease.move");
    let silence = 30 * 1_000_000;
    let mut ran = 0;
    for case in file["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let input = &case["input"];
        db.execute_unprepared("DELETE FROM agent_lease_ends; TRUNCATE agent_lease_grants;")
            .await
            .unwrap();
        let standing: Grant = serde_json::from_value(input["lease"].clone()).unwrap();
        db.execute_raw(sea_orm::Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            "INSERT INTO agent_lease_grants (epoch, holder, issued, how, through, created_at) \
             VALUES ($1, $2, $3, $4, $5, NOW())",
            [
                (standing.epoch as i64).into(),
                standing.holder.clone().into(),
                standing.issued.to_string().into(),
                serde_json::to_value(standing.how)
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .into(),
                standing.through.as_ref().map(|t| json!(t)).into(),
            ],
        ))
        .await
        .unwrap();
        if let Some(last_seen) = input["holderLastSeen"].as_i64() {
            lease::heard(
                db,
                standing.holder.as_deref().unwrap(),
                Some(lease::Core::Host),
                last_seen - silence,
            )
            .await
            .unwrap();
        }
        if let Some(place) = input["holderPlace"].as_str() {
            db.execute_raw(sea_orm::Statement::from_sql_and_values(
                sea_orm::DatabaseBackend::Postgres,
                "INSERT INTO agent_lease_ends (device_id, core, place) VALUES ($1, 'host', $2) \
                 ON CONFLICT (device_id) DO UPDATE SET place = EXCLUDED.place",
                [standing.holder.clone().into(), place.into()],
            ))
            .await
            .unwrap();
        }
        if let Some(owner) = input.get("ownerPlace") {
            db.execute_raw(sea_orm::Statement::from_sql_and_values(
                sea_orm::DatabaseBackend::Postgres,
                "INSERT INTO agent_lease_ends (device_id, core, place, owner_since, owner_seen_at) \
                 VALUES ('dev_phone', 'child', $1, $2, $3)",
                [
                    owner["place"].as_str().unwrap().into(),
                    owner["since"].as_i64().unwrap().into(),
                    owner["seenAt"].as_i64().unwrap().into(),
                ],
            ))
            .await
            .unwrap();
        }
        // The cores the cases move her to are host cores.
        db.execute_unprepared(
            "INSERT INTO agent_lease_ends (device_id, core) VALUES ('dev_mac', 'host'), ('dev_win', 'host') \
             ON CONFLICT (device_id) DO UPDATE SET core = 'host'",
        )
        .await
        .unwrap();
        let settings = Settings {
            stop_after_micros: input["stopAfter"].as_i64().unwrap_or(180_000_000),
            ..on()
        };
        let request: myriad_persona_log::Request =
            serde_json::from_value(input["request"].clone()).unwrap();
        let issued: Rev = input["issued"].as_str().unwrap().parse().unwrap();
        let bad_stop = myriad_persona_log::lease::stop_after(input["stopAfter"].as_i64()).is_err();
        let got = if bad_stop {
            // The server never runs with such a setting (`Settings` refuses it).
            json!({ "error": "bad_input" })
        } else {
            match lease::request_issued(
                db,
                &settings,
                request,
                input["now"].as_i64().unwrap(),
                Some(issued),
            )
            .await
            {
                Ok(lease::Outcome::Granted(grant)) => json!({ "granted": grant }),
                Ok(lease::Outcome::Refused(refusal)) => json!({ "refused": refusal }),
                Ok(lease::Outcome::NotAHost) => panic!("{name}: not a host"),
                Err(super::Error::Refused(_)) => json!({ "error": "bad_input" }),
                Err(error) => panic!("{name}: {error}"),
            }
        };
        assert_eq!(
            canonical_bytes(&got),
            canonical_bytes(&case["output"]),
            "{name}\n     got {got}"
        );
        ran += 1;
    }
    assert!(ran >= 15);
    schema.drop().await;
}

/// The settings read from the environment: off unless switched on; a stop
/// time outside a minute to a day falls back to three minutes.
#[test]
fn settings_are_off_unless_switched_on() {
    let none = Settings::from_lookup(|_| None);
    assert_eq!(none, Settings::default());
    assert!(!none.enabled);
    let set = |pairs: &'static [(&'static str, &'static str)]| {
        Settings::from_lookup(move |key| {
            pairs
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| value.to_string())
        })
    };
    let on = set(&[
        ("HER_LOG_ENABLED", "on"),
        ("HER_LEASE_STOP_AFTER_SECS", "300"),
        ("HER_LOG_RETENTION_DAYS", "7"),
    ]);
    assert!(on.enabled);
    assert_eq!(on.stop_after_micros, 300_000_000);
    assert_eq!(on.retention_days, 7);
    let wrong = set(&[
        ("HER_LOG_ENABLED", "maybe"),
        ("HER_LEASE_STOP_AFTER_SECS", "10"),
    ]);
    assert!(!wrong.enabled);
    assert_eq!(wrong.stop_after_micros, 180_000_000);
}

/// What a computer sends stays within the contract's limits.
#[test]
fn batches_fit_the_frame_limits() {
    let mut side = Side::new("dev_mac");
    let text = "她记得的一件事。".repeat(500);
    for n in 0..300 {
        side.write(
            PERSONAL,
            &format!("memory:m{n}"),
            "memory.write",
            Author::Owner,
            Some(&text),
            T0 + n,
        );
    }
    let frames = side.send(PERSONAL, &Vector::new(), Vec::new());
    assert!(frames.len() >= 5, "{}", frames.len());
    for frame in &frames {
        let body = &frame["body"];
        assert!(body["rows"].as_array().unwrap().len() <= wire::BATCH_ROWS);
        assert!(body["payloads"].as_array().unwrap().len() <= wire::BATCH_PAYLOADS);
        let out = Outgoing {
            to: To::Sender,
            t: "log.rows",
            id: Some("rows_1".into()),
            body: body.clone(),
        };
        relay::encode(&out, 1, 0, 1).unwrap();
    }
    let total: usize = frames
        .iter()
        .map(|frame| frame["body"]["rows"].as_array().unwrap().len())
        .sum();
    assert_eq!(total, 300);
}

/// A body that did not come with its row is asked for again in the
/// `log.vector` that answers the batch, once in a link session, and taken
/// when it comes: both sides then hold the same bytes.
#[tokio::test]
async fn a_body_that_did_not_come_is_asked_for_once_and_taken_when_it_comes() {
    let Some(schema) = schema("her_log_ask").await else {
        return;
    };
    let db = &schema.db;
    let mut mac = Side::new("dev_mac");
    let row = mac.write(
        PERSONAL,
        "memory:tea",
        "memory.write",
        Author::Owner,
        Some("likes tea"),
        T0,
    );
    let reference = row.payload.clone().unwrap().reference;
    let data = mac.payloads.remove(&reference).unwrap();
    let frames = mac.send(PERSONAL, &Vector::new(), Vec::new());
    assert_eq!(
        frames[0]["body"]["withheld"],
        json!([{ "id": row.id, "why": "no_body" }])
    );
    let mut link = relay::Session::default();
    let outs = relay::take(
        db,
        &root(),
        &on(),
        OWNER,
        "dev_mac",
        Core::Pc,
        &mut link,
        &frames[0],
        T0,
    )
    .await
    .unwrap();
    let answer = delivered(&outs[0]);
    assert_eq!(answer["t"], "log.vector");
    assert_eq!(answer["body"]["ask"], json!([row.id]));
    assert_eq!(
        count(db, "SELECT COUNT(*) FROM agent_log_payloads").await,
        0
    );

    // Still without it: sent again as a row alone, and not asked for twice.
    let again = mac.take(&answer);
    assert_eq!(again.len(), 1);
    let outs = relay::take(
        db,
        &root(),
        &on(),
        OWNER,
        "dev_mac",
        Core::Pc,
        &mut link,
        &again[0],
        T0,
    )
    .await
    .unwrap();
    assert!(delivered(&outs[0])["body"].get("ask").is_none());

    // It comes back: the server takes the body of the row it holds.
    mac.payloads.insert(reference, data.clone());
    let again = mac.take(&answer);
    let outs = relay::take(
        db,
        &root(),
        &on(),
        OWNER,
        "dev_mac",
        Core::Pc,
        &mut link,
        &again[0],
        T0,
    )
    .await
    .unwrap();
    assert!(delivered(&outs[0])["body"].get("ask").is_none());
    assert_eq!(
        count(db, "SELECT COUNT(*) FROM agent_log_payloads").await,
        1
    );
    let mut win = Side::new("dev_win");
    sync(db, &mut win, PERSONAL, T0 + MINUTE).await;
    assert_eq!(win.payloads.values().collect::<Vec<_>>(), vec![&data]);
    schema.drop().await;
}

/// The partner's computers hold only the partner's private talk and hers:
/// anyone else's private talk with her, kept here, is never sent to one,
/// over the link or in the first full download, and only her mind adds to
/// it.
#[tokio::test]
async fn a_partners_computer_never_gets_anyone_elses_talk() {
    let Some(schema) = schema("her_log_holder").await else {
        return;
    };
    let db = &schema.db;
    let mut mac = Side::new("dev_mac");
    let mut win = Side::new("dev_win");
    let ask = mac.frame("lease.ask", None, json!({ "why": "talk" }));
    exchange(db, &mut mac, vec![ask], T0).await;
    mac.write(
        OTHER,
        "memory:theirs",
        "memory.write",
        Author::Her,
        Some("their cat is called Mochi"),
        T0,
    );
    let frame = mac.rows_frame(OTHER, mac.log(OTHER));
    exchange(db, &mut mac, vec![frame], T0 + MINUTE).await;
    assert_eq!(store::rows(db, OWNER, OTHER).await.unwrap().len(), 1);

    let vector = win.vector(OTHER);
    let outs = relay::take(
        db,
        &root(),
        &on(),
        OWNER,
        "dev_win",
        Core::Pc,
        &mut win.link,
        &vector,
        T0,
    )
    .await
    .unwrap();
    assert_eq!(outs.len(), 1);
    assert_eq!(delivered(&outs[0])["body"]["code"], "not_holder");
    let row = win.write(
        OTHER,
        "memory:x",
        "memory.write",
        Author::Owner,
        Some("x"),
        T0,
    );
    let frame = win.rows_frame(OTHER, vec![row]);
    let outs = relay::take(
        db,
        &root(),
        &on(),
        OWNER,
        "dev_win",
        Core::Pc,
        &mut win.link,
        &frame,
        T0,
    )
    .await
    .unwrap();
    assert_eq!(delivered(&outs[0])["body"]["code"], "not_holder");
    assert_eq!(store::rows(db, OWNER, OTHER).await.unwrap().len(), 1);

    let sent = store::send(
        db,
        &root(),
        &owner(db).await,
        OTHER,
        &Vector::new(),
        Vec::new(),
        &pc(),
        2,
    )
    .await
    .unwrap();
    assert!(sent.not_held && sent.batches.is_empty());
    let link = super::full::issue(db, "dev_win", OTHER, 2, T0)
        .await
        .unwrap();
    let secret = link.path.trim_start_matches(super::full::PATH);
    let download = super::full::redeem(db, &root(), OWNER, "dev_win", secret, T0)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(download["rows"], json!([]));
    assert_eq!(download["payloads"], json!([]));
    schema.drop().await;
}

/// A body with a key in it is not kept, even when the sender carried it:
/// the row is, and the body stays where it was written.
#[tokio::test]
async fn a_body_holding_a_key_is_not_taken() {
    let Some(schema) = schema("her_log_secret").await else {
        return;
    };
    let db = &schema.db;
    let mut mac = Side::new("dev_mac");
    let row = mac.write(
        PERSONAL,
        "memory:deploy",
        "memory.write",
        Author::Owner,
        Some("伙伴的 token: example-only-not-real"),
        T0,
    );
    // Carried anyway, past the sender's own check.
    let frame = mac.rows_frame(PERSONAL, vec![row.clone()]);
    exchange(db, &mut mac, vec![frame], T0 + MINUTE).await;
    assert_eq!(store::rows(db, OWNER, PERSONAL).await.unwrap().len(), 1);
    assert_eq!(
        count(db, "SELECT COUNT(*) FROM agent_log_payloads").await,
        0
    );
    // The sender's own check withholds it for good: nothing to ask for.
    let frames = mac.send(PERSONAL, &Vector::new(), Vec::new());
    assert_eq!(
        frames[0]["body"]["withheld"],
        json!([{ "id": row.id, "why": "secret" }])
    );
    schema.drop().await;
}

/// A phone or XR holds no log: its link is no host's, so it gets no rows,
/// sends none, and is never her mind.
#[tokio::test]
async fn a_phone_gets_no_log() {
    let Some(schema) = schema("her_log_child").await else {
        return;
    };
    let db = &schema.db;
    let mut mac = Side::new("dev_mac");
    mac.write(
        PERSONAL,
        "memory:tea",
        "memory.write",
        Author::Owner,
        Some("likes tea"),
        T0,
    );
    sync(db, &mut mac, PERSONAL, T0 + MINUTE).await;
    assert_eq!(store::rows(db, OWNER, PERSONAL).await.unwrap().len(), 1);

    let mut phone = Side::new("dev_phone");
    let vector = phone.vector(PERSONAL);
    let outs = relay::take(
        db,
        &root(),
        &on(),
        OWNER,
        "dev_phone",
        Core::Child,
        &mut phone.link,
        &vector,
        T0 + MINUTE,
    )
    .await
    .unwrap();
    assert_eq!(outs.len(), 1);
    assert_eq!(delivered(&outs[0])["t"], "error");
    let row = phone.write(
        PERSONAL,
        "memory:x",
        "memory.write",
        Author::Owner,
        Some("x"),
        T0,
    );
    let frame = phone.rows_frame(PERSONAL, vec![row]);
    let outs = relay::take(
        db,
        &root(),
        &on(),
        OWNER,
        "dev_phone",
        Core::Child,
        &mut phone.link,
        &frame,
        T0 + MINUTE,
    )
    .await
    .unwrap();
    assert_eq!(delivered(&outs[0])["t"], "error");
    assert_eq!(store::rows(db, OWNER, PERSONAL).await.unwrap().len(), 1);
    schema.drop().await;
}

/// Two accounts keep two logs: the same row and node ids under each are
/// each account's own, a computer of one never gets the other's rows or
/// bodies, and its partner's private talk is not the other's to ask for.
#[tokio::test]
async fn two_accounts_keep_two_logs() {
    use super::test_partner::OTHER_PARTNER;
    let Some(schema) = schema("her_log_accounts").await else {
        return;
    };
    let db = &schema.db;
    let other = OWNER + 1;
    db.execute_unprepared(&format!(
        "INSERT INTO users (id, username) VALUES ({other}, 'other')"
    ))
    .await
    .unwrap();
    store::start(db, other).await.unwrap();
    super::test_partner::give(db, other, OTHER_PARTNER).await;
    let theirs = format!("personal:{OTHER_PARTNER}");

    // The same device id, so the same row ids, in both accounts.
    let mut mine = Side::new("dev_mac");
    let mut yours = Side::of("dev_mac", other, OTHER_PARTNER);
    mine.write(
        OWN,
        "memory:tea",
        "memory.write",
        Author::Owner,
        Some("likes tea"),
        T0,
    );
    yours.write(
        OWN,
        "memory:tea",
        "memory.write",
        Author::Owner,
        Some("likes coffee"),
        T0,
    );
    yours.write(
        &theirs,
        "memory:walk",
        "memory.write",
        Author::Owner,
        Some("walks at night"),
        T0,
    );
    sync(db, &mut mine, OWN, T0 + MINUTE).await;
    sync(db, &mut yours, OWN, T0 + MINUTE).await;
    sync(db, &mut yours, &theirs, T0 + MINUTE).await;
    assert_eq!(store::rows(db, OWNER, OWN).await.unwrap().len(), 1);
    assert_eq!(store::rows(db, other, OWN).await.unwrap().len(), 1);
    assert!(store::rows(db, OWNER, &theirs).await.unwrap().is_empty());

    let mut win = Side::new("dev_win");
    sync(db, &mut win, OWN, T0 + 2 * MINUTE).await;
    assert!(win.payloads.values().any(|data| says(data, "likes tea")));
    assert!(win.payloads.values().all(|data| !says(data, "coffee")));
    let mut their_win = Side::of("dev_win", other, OTHER_PARTNER);
    sync(db, &mut their_win, OWN, T0 + 2 * MINUTE).await;
    sync(db, &mut their_win, &theirs, T0 + 2 * MINUTE).await;
    assert!(their_win.payloads.values().any(|data| says(data, "coffee")));
    assert!(
        their_win
            .payloads
            .values()
            .any(|data| says(data, "at night"))
    );
    assert!(
        their_win
            .payloads
            .values()
            .all(|data| !says(data, "likes tea"))
    );

    // The other partner's private talk is not this account's computer's.
    let vector = win.vector(&theirs);
    let outs = relay::take(
        db,
        &root(),
        &on(),
        OWNER,
        "dev_win",
        Core::Pc,
        &mut win.link,
        &vector,
        T0 + 3 * MINUTE,
    )
    .await
    .unwrap();
    assert_eq!(delivered(&outs[0])["body"]["code"], "not_holder");
    schema.drop().await;
}

/// A node taken back from sync on a computer is a forget on the server
/// (it is never a home): its bodies go there and its key with them. A
/// computer whose core reads retracts gets it before anything else; one
/// older than ABI 2 gets no rows at all while it is to go (`tooOld`),
/// only the server's vector, and so does its first full download.
#[tokio::test]
async fn a_retract_is_a_forget_here_and_no_row_goes_to_an_older_core() {
    let Some(schema) = schema("her_log_retract").await else {
        return;
    };
    let db = &schema.db;
    let mut mac = Side::new("dev_mac");
    mac.write(
        PERSONAL,
        "node:lamp",
        "node.created",
        Author::Owner,
        Some("the lamp"),
        T0,
    );
    mac.write(
        PERSONAL,
        "memory:tea",
        "memory.write",
        Author::Owner,
        Some("likes tea"),
        T0,
    );
    sync(db, &mut mac, PERSONAL, T0 + MINUTE).await;
    assert_eq!(
        count(db, "SELECT COUNT(*) FROM agent_log_payloads").await,
        2
    );
    let retract = mac.write(
        PERSONAL,
        "node:lamp",
        "retract",
        Author::Owner,
        None,
        T0 + MINUTE,
    );
    sync(db, &mut mac, PERSONAL, T0 + 2 * MINUTE).await;
    let ledger = store::ledger(db, OWNER, PERSONAL).await.unwrap();
    assert!(
        ledger
            .iter()
            .any(|entry| entry.is_retract() && entry.target == "node:lamp")
    );
    assert_eq!(
        count(db, "SELECT COUNT(*) FROM agent_log_payloads").await,
        1,
        "the lamp's body is gone, the tea's stays"
    );
    assert_eq!(
        count(
            db,
            "SELECT COUNT(*) FROM agent_log_object_keys WHERE object = 'node:lamp'"
        )
        .await,
        0
    );

    // A computer whose core reads retracts gets it first, and the lamp
    // only as a stub.
    let mut win = Side::new("dev_win");
    sync(db, &mut win, PERSONAL, T0 + 3 * MINUTE).await;
    let log = win.log(PERSONAL);
    assert!(log.iter().any(|row| row.id == retract.id));
    assert!(
        log.iter()
            .filter(|row| row.object == "node:lamp" && row.kind != "retract")
            .all(|row| row.forgotten)
    );
    assert!(win.payloads.values().any(|data| says(data, "likes tea")));
    assert!(win.payloads.values().all(|data| !says(data, "the lamp")));

    // One older than ABI 2: nothing, though it lacks the tea too.
    let mut old = Side::new("dev_old");
    old.link = relay::Session::new(relay::peer_abi(&[]));
    let vector = old.vector(PERSONAL);
    let outs = relay::take(
        db,
        &root(),
        &on(),
        OWNER,
        "dev_old",
        Core::Pc,
        &mut old.link,
        &vector,
        T0 + 3 * MINUTE,
    )
    .await
    .unwrap();
    let ts: Vec<&str> = outs.iter().map(|out| out.t).collect();
    assert_eq!(ts, ["log.vector"]);
    let sent = store::send(
        db,
        &root(),
        &owner(db).await,
        PERSONAL,
        &Vector::new(),
        Vec::new(),
        &pc(),
        1,
    )
    .await
    .unwrap();
    assert!(sent.too_old && sent.batches.is_empty());
    let link = super::full::issue(db, "dev_old", PERSONAL, 1, T0)
        .await
        .unwrap();
    let secret = link.path.trim_start_matches(super::full::PATH);
    let download = super::full::redeem(db, &root(), OWNER, "dev_old", secret, T0)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(download["tooOld"], true);
    assert_eq!(download["rows"], json!([]));
    schema.drop().await;
}

/// A node someone else's in her own audience, learnt from its created row
/// in one batch, keeps every later body of it off the server, in whatever
/// batch it comes: the server keeps the node and gives it to the wire.
#[tokio::test]
async fn a_node_of_someone_else_in_hers_stays_bodiless_across_batches() {
    let Some(schema) = schema("her_log_elsewhere").await else {
        return;
    };
    let db = &schema.db;
    let mut mac = Side::new("dev_mac");
    // A computer that does not go by the core's ids writes a person's node
    // into her own audience, and sends its bodies as they are.
    let created = mac.write_body(
        OWN,
        "node:alice",
        "node.created",
        Author::Owner,
        Some(json!({
            "key": "person:alice",
            "kind": "person",
            "name": "Alice",
            "saidBy": ["her"],
            "sync": "shared",
            "version": 1,
        })),
        T0,
    );
    let frame = mac.rows_frame(OWN, vec![created]);
    exchange(db, &mut mac, vec![frame], T0).await;
    assert_eq!(
        store::elsewhere(db, OWNER, OWN).await.unwrap(),
        ["node:alice"]
    );
    let span = mac.write(
        OWN,
        "node:alice",
        "node.span",
        Author::Owner,
        Some("Alice said she moved"),
        T0 + MINUTE,
    );
    let frame = mac.rows_frame(OWN, vec![span.clone()]);
    exchange(db, &mut mac, vec![frame], T0 + MINUTE).await;
    assert_eq!(store::rows(db, OWNER, OWN).await.unwrap().len(), 2);
    assert_eq!(
        count(db, "SELECT COUNT(*) FROM agent_log_payloads").await,
        0
    );
    let sent = store::send(
        db,
        &root(),
        &owner(db).await,
        OWN,
        &Vector::new(),
        Vec::new(),
        &pc(),
        2,
    )
    .await
    .unwrap();
    assert!(sent.batches.iter().all(|batch| batch.payloads.is_empty()));
    schema.drop().await;
}

/// A stub kept for its place of her mood about the partner under
/// `local-owner` (a host's fault: held back whole) is filled when the same
/// row comes moved to the partner's id: the server then holds it whole, its
/// body too, and looks it up by the id it names now. When the moved row
/// comes as a stub too, the held stub takes its envelope (`relabelled`).
#[tokio::test]
async fn a_stub_held_back_under_the_old_id_is_filled_or_relabelled_by_the_row_moved() {
    use myriad_persona_log::partner::{LEGACY, Move};
    let Some(schema) = schema("her_log_filled").await else {
        return;
    };
    let db = &schema.db;
    let mut mac = Side::new("dev_mac");
    let row = Row {
        version: LOG_VERSION,
        id: "dev_mac_walk".to_string(),
        origin: "dev_mac".to_string(),
        author: Author::Her,
        epoch: Some(0),
        dseq: Some(1),
        hlc: myriad_persona_log::clock::next(None, T0, 0, "dev_mac").unwrap(),
        audience: OWN.to_string(),
        subject: None,
        object: format!("affect:{LEGACY}"),
        kind: "affect.change".to_string(),
        payload: Some(Payload {
            reference: "blob:dev_mac_walk".to_string(),
            key: None,
        }),
        replicate: Replicate::Shared,
        forgotten: false,
        to: None,
        takes: Vec::new(),
    };
    let stub = wire::Batch {
        audience: OWN.to_string(),
        rows: vec![row.stub()],
        payloads: Vec::new(),
        withheld: Vec::new(),
    };
    let said = store::take(
        db,
        &root(),
        &owner(db).await,
        "dev_mac",
        stub,
        &Default::default(),
        T0,
    )
    .await
    .unwrap();
    assert_eq!(
        count(
            db,
            "SELECT COUNT(*) FROM agent_log_rows WHERE object = 'affect:local-owner' AND forgotten"
        )
        .await,
        1,
        "{said:?}"
    );

    let moved = myriad_persona_log::partner::row(
        &row,
        &Move {
            from: LEGACY.to_string(),
            to: PARTNER.to_string(),
        },
    );
    assert_eq!(moved.object, format!("affect:{PARTNER}"));
    mac.payloads.insert(
        "blob:dev_mac_walk".to_string(),
        canonical_bytes(&body_of(
            "affect.change",
            OWN,
            &moved.object,
            "a quiet evening",
            T0,
        )),
    );
    let frame = mac.rows_frame(OWN, vec![moved.clone()]);
    exchange(db, &mut mac, vec![frame], T0 + MINUTE).await;
    assert_eq!(store::rows(db, OWNER, OWN).await.unwrap(), [moved]);
    assert_eq!(
        count(
            db,
            &format!(
                "SELECT COUNT(*) FROM agent_log_rows \
                 WHERE object = 'affect:{PARTNER}' AND NOT forgotten"
            )
        )
        .await,
        1
    );
    assert_eq!(
        count(
            db,
            "SELECT COUNT(*) FROM agent_log_rows WHERE object = 'affect:local-owner'"
        )
        .await,
        0
    );
    assert_eq!(
        count(db, "SELECT COUNT(*) FROM agent_log_payloads").await,
        1
    );

    // The same again, the moved row coming as a stub too (forgotten where it
    // was moved): the held stub takes its envelope and stays a stub.
    let earlier = Row {
        id: "dev_mac_mood".to_string(),
        dseq: Some(2),
        hlc: myriad_persona_log::clock::next(Some(&row.hlc), T0, 0, "dev_mac").unwrap(),
        payload: Some(Payload {
            reference: "blob:dev_mac_mood".to_string(),
            key: None,
        }),
        ..row.clone()
    };
    let as_held = wire::Batch {
        audience: OWN.to_string(),
        rows: vec![earlier.stub()],
        payloads: Vec::new(),
        withheld: Vec::new(),
    };
    store::take(
        db,
        &root(),
        &owner(db).await,
        "dev_mac",
        as_held,
        &Default::default(),
        T0 + MINUTE,
    )
    .await
    .unwrap();
    let moved_stub = myriad_persona_log::partner::row(
        &earlier,
        &Move {
            from: LEGACY.to_string(),
            to: PARTNER.to_string(),
        },
    )
    .stub();
    let taken = store::take(
        db,
        &root(),
        &owner(db).await,
        "dev_mac",
        wire::Batch {
            audience: OWN.to_string(),
            rows: vec![moved_stub.clone()],
            payloads: Vec::new(),
            withheld: Vec::new(),
        },
        &Default::default(),
        T0 + MINUTE,
    )
    .await
    .unwrap();
    assert_eq!(taken.relabelled, [earlier.id.clone()], "{taken:?}");
    let held = store::rows(db, OWNER, OWN).await.unwrap();
    assert!(held.contains(&moved_stub));
    assert_eq!(
        count(
            db,
            &format!(
                "SELECT COUNT(*) FROM agent_log_rows \
                 WHERE id = 'dev_mac_mood' AND object = 'affect:{PARTNER}' AND forgotten"
            )
        )
        .await,
        1
    );
    assert_eq!(
        count(db, "SELECT COUNT(*) FROM agent_log_payloads").await,
        1
    );

    // The site never writes a forget about the partner under an id it does
    // not go by: it would forget under it later, once moved.
    let refused = store::forget(db, &owner(db).await, OWN, &format!("affect:{LEGACY}")).await;
    assert!(
        matches!(refused, Err(super::Error::Refused(_))),
        "{refused:?}"
    );
    assert_eq!(
        count(
            db,
            "SELECT COUNT(*) FROM agent_log_rows WHERE kind = 'forget'"
        )
        .await,
        0
    );

    // Whole this time, but the ledger here takes it away: the site forgot
    // her mood towards the partner. The held stub takes the moved envelope
    // and stays a stub: relabelled, not filled, and no payload is kept.
    let rain = Row {
        id: "dev_mac_rain".to_string(),
        dseq: Some(3),
        hlc: myriad_persona_log::clock::next(Some(&earlier.hlc), T0, 0, "dev_mac").unwrap(),
        payload: Some(Payload {
            reference: "blob:dev_mac_rain".to_string(),
            key: None,
        }),
        ..row.clone()
    };
    store::take(
        db,
        &root(),
        &owner(db).await,
        "dev_mac",
        wire::Batch {
            audience: OWN.to_string(),
            rows: vec![rain.stub()],
            payloads: Vec::new(),
            withheld: Vec::new(),
        },
        &Default::default(),
        T0 + 2 * MINUTE,
    )
    .await
    .unwrap();
    store::forget(db, &owner(db).await, OWN, &format!("affect:{PARTNER}"))
        .await
        .unwrap();
    let moved_rain = myriad_persona_log::partner::row(
        &rain,
        &Move {
            from: LEGACY.to_string(),
            to: PARTNER.to_string(),
        },
    );
    mac.payloads.insert(
        "blob:dev_mac_rain".to_string(),
        canonical_bytes(&body_of(
            "affect.change",
            OWN,
            &moved_rain.object,
            "rain on the window",
            T0,
        )),
    );
    let frame = mac.rows_frame(OWN, vec![moved_rain]);
    let taken = store::take(
        db,
        &root(),
        &owner(db).await,
        "dev_mac",
        serde_json::from_value(frame["body"].clone()).unwrap(),
        &Default::default(),
        T0 + 2 * MINUTE,
    )
    .await
    .unwrap();
    assert_eq!(taken.relabelled, [rain.id.clone()], "{taken:?}");
    assert!(taken.filled.is_empty(), "{taken:?}");
    assert_eq!(
        count(
            db,
            &format!(
                "SELECT COUNT(*) FROM agent_log_rows \
                 WHERE id = 'dev_mac_rain' AND object = 'affect:{PARTNER}' AND forgotten"
            )
        )
        .await,
        1
    );
    // The forget took the walk's payload too; none is left.
    assert_eq!(
        count(db, "SELECT COUNT(*) FROM agent_log_payloads").await,
        0
    );

    // On the server a move never changes a subject (`own:` has none, and no
    // side holds `personal:local-owner`), so restate's subject column is
    // checked on its own: a group's stub about one member, restated about
    // another.
    let about = Row {
        id: "dev_mac_group".to_string(),
        audience: GROUP.to_string(),
        subject: Some("tg_1".to_string()),
        object: "memory:dev_mac_group".to_string(),
        kind: "memory.write".to_string(),
        dseq: Some(1),
        payload: Some(Payload {
            reference: "blob:dev_mac_group".to_string(),
            key: None,
        }),
        ..row.clone()
    }
    .stub();
    store::take(
        db,
        &root(),
        &owner(db).await,
        "dev_mac",
        wire::Batch {
            audience: GROUP.to_string(),
            rows: vec![about.clone()],
            payloads: Vec::new(),
            withheld: Vec::new(),
        },
        &Default::default(),
        T0 + 2 * MINUTE,
    )
    .await
    .unwrap();
    store::restate(
        db,
        OWNER,
        &Row {
            subject: Some("tg_2".to_string()),
            ..about
        },
    )
    .await
    .unwrap();
    assert_eq!(
        count(
            db,
            "SELECT COUNT(*) FROM agent_log_rows WHERE id = 'dev_mac_group' AND subject = 'tg_2'"
        )
        .await,
        1
    );
    schema.drop().await;
}

/// A log of the shape from before it was one per account that holds rows is
/// kept as it is by the startup heal, and her log stays off while it is
/// there, switched on or not; emptied, the heal makes it again and her log
/// may run.
#[tokio::test]
async fn her_log_stays_off_while_a_log_of_the_old_shape_holds_rows() {
    let Some(schema) = schema("her_log_old_shape").await else {
        return;
    };
    let db = &schema.db;
    let heal = include_str!("../../../migrations/persona_log.sql");
    // As the schema check leaves one: keyed by row id, `user_id` added and
    // nullable, the unique place without it.
    db.execute_unprepared(
        "DROP TABLE agent_log_payloads, agent_log_rows CASCADE;
         CREATE TABLE agent_log_rows (
             id VARCHAR(512) PRIMARY KEY, audience VARCHAR(512) NOT NULL,
             origin VARCHAR(64) NOT NULL, dseq BIGINT NOT NULL, hlc VARCHAR(96) NOT NULL,
             object VARCHAR(512) NOT NULL, subject VARCHAR(512), kind VARCHAR(64) NOT NULL,
             forgotten BOOLEAN NOT NULL DEFAULT false, envelope JSONB NOT NULL,
             received_from VARCHAR(64) NOT NULL, received_at TIMESTAMPTZ NOT NULL,
             user_id INTEGER);
         CREATE UNIQUE INDEX idx_agent_log_rows_place ON agent_log_rows (audience, origin, dseq);
         CREATE INDEX idx_agent_log_rows_object ON agent_log_rows (audience, object);
         CREATE INDEX idx_agent_log_rows_subject ON agent_log_rows (audience, subject);
         CREATE TABLE agent_log_payloads (
             row_id VARCHAR(512) PRIMARY KEY REFERENCES agent_log_rows(id) ON DELETE CASCADE,
             sealed BYTEA NOT NULL, created_at TIMESTAMPTZ NOT NULL, user_id INTEGER);
         CREATE TABLE agent_log_ledger (target VARCHAR(512) PRIMARY KEY);
         INSERT INTO agent_log_rows (id, audience, origin, dseq, hlc, object, kind, envelope,
                                     received_from, received_at)
             VALUES ('dev_old_1', 'own:', 'dev_old', 1, 'h', 'affect:x', 'affect.change', '{}',
                     'dev_old', now());
         INSERT INTO agent_log_ledger VALUES ('memory:m1');",
    )
    .await
    .unwrap();
    db.execute_unprepared(heal).await.unwrap();
    let left = super::old_shape(db).await.unwrap();
    assert_eq!(
        left.as_deref(),
        Some("agent_log_* keyed by row id, agent_log_ledger")
    );
    assert_eq!(count(db, "SELECT COUNT(*) FROM agent_log_rows").await, 1);
    assert!(!super::admit(db, &on()).await);
    assert!(!super::admit(db, &Settings::default()).await);

    db.execute_unprepared("DELETE FROM agent_log_rows; DELETE FROM agent_log_ledger")
        .await
        .unwrap();
    db.execute_unprepared(heal).await.unwrap();
    assert_eq!(super::old_shape(db).await.unwrap(), None);
    assert!(super::admit(db, &on()).await);
    assert!(!super::admit(db, &Settings::default()).await);
    schema.drop().await;
}

// ------------------------------------------------- her mind on another core

/// Where her mind is, read as every process reads it.
async fn settled(db: &DatabaseConnection) -> super::pen::Pen {
    super::pen::settle_for(db, &on(), Some(OWNER))
        .await
        .unwrap()
}

/// A line she said, as the ledger writes it.
fn said(text: &str, at: i64) -> Value {
    json!({
        "at": at,
        "text": text,
        "place": "web",
        "source": "reply",
        "called": true,
        "exit": "web",
        "extent": "full",
    })
}

/// The Mac asks for her mind and gets it: from then on nothing on the
/// server thinks as her. Not a thought, not a turn, not a driver tick, not
/// a row of hers; a run is refused as her being on another device.
#[tokio::test]
async fn while_her_mind_is_on_the_mac_the_server_does_not_think() {
    let Some(schema) = schema("her_log_pen_away").await else {
        return;
    };
    let db = &schema.db;
    super::pen::reset_for_test();
    assert_eq!(settled(db).await, super::pen::Pen::Here);
    assert!(super::pen::think().is_some());

    let mut mac = Side::new("dev_mac");
    let ask = mac.frame("lease.ask", None, json!({ "why": "place" }));
    exchange(db, &mut mac, vec![ask], T0).await;
    let epoch = mac.epoch.expect("the Mac holds her");
    assert_eq!(
        super::pen::current(),
        super::pen::Pen::Away {
            holder: "dev_mac".into(),
            epoch
        }
    );
    assert!(super::pen::think().is_none());
    use crate::services::persona_sync::lease as host;
    assert_eq!(host::place(), host::Where::MindOn("dev_mac".into()));
    assert!(host::paused("a driver of hers in a test"));
    let refused = host::refuse_while_away().unwrap_err();
    assert_eq!(refused.code(), Some("mind_elsewhere"));
    assert!(host::refuse_work_while_away().is_err());
    // Every agent turn passes the same gate first.
    let turn = crate::services::agent::AgentTurnBudget::run(
        db,
        OWNER,
        "agent.process",
        "t".into(),
        Box::pin(async { Ok::<_, String>(()) }),
    )
    .await;
    assert_eq!(turn, Err("her mind is on another device now".into()));
    // No row of hers is written here.
    let owner = owner(db).await;
    let hers = store::write_hers(
        db,
        &root(),
        &owner,
        PERSONAL,
        None,
        "speech:s1",
        "speech.said",
        &said("在吗", 1),
    )
    .await;
    assert!(hers.is_err());
    super::pen::reset_for_test();
    schema.drop().await;
}

/// While she lives on a device the site let its copy go to (H3e: her lease
/// went there with the move), no host core takes her mind by asking, not
/// even "come here": only a move moves her. The site, not being her then,
/// keeps its pen here (its Work runs as the plain assistant).
#[tokio::test]
async fn while_she_lives_on_a_device_only_a_move_moves_her() {
    let Some(schema) = schema("her_log_pen_copy_away").await else {
        return;
    };
    let db = &schema.db;
    super::pen::reset_for_test();
    use crate::services::persona_sync::lease as host;
    host::seed(db, "dev_home", "l-1", 2, host::State::Away)
        .await
        .unwrap();
    let mut mac = Side::new("dev_mac");
    let ask = mac.frame("lease.ask", None, json!({ "why": "force" }));
    exchange(db, &mut mac, vec![ask], T0).await;
    assert_eq!(mac.epoch, None, "no grant for the Mac");
    let standing = lease::standing(db).await.unwrap().unwrap();
    assert_eq!(
        (standing.holder.as_deref(), standing.epoch),
        (Some("dev_home"), 2)
    );
    assert_eq!(settled(db).await, super::pen::Pen::Here);
    super::pen::reset_for_test();
    schema.drop().await;
}

/// Asking for her mind while the server is in the middle of a thought:
/// nothing new starts, and the grant goes out only once that thought ends.
#[tokio::test]
async fn asking_for_her_waits_for_the_servers_thought_in_hand() {
    let Some(schema) = schema("her_log_pen_quiet").await else {
        return;
    };
    let db = &schema.db;
    super::pen::reset_for_test();
    settled(db).await;
    let thought = super::pen::think().expect("she thinks here");
    let mut mac = Side::new("dev_mac");
    let ask = mac.frame("lease.ask", None, json!({ "why": "talk" }));
    let root = root();
    let settings = on();
    {
        let asking = relay::take(
            db,
            &root,
            &settings,
            OWNER,
            "dev_mac",
            Core::Pc,
            &mut mac.link,
            &ask,
            T0,
        );
        tokio::pin!(asking);
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(100), asking.as_mut())
                .await
                .is_err(),
            "the grant waits"
        );
        assert!(super::pen::think().is_none(), "nothing new starts");
        assert_eq!(
            count(
                db,
                "SELECT COUNT(*) FROM agent_lease_grants WHERE holder = 'dev_mac'"
            )
            .await,
            0
        );
        drop(thought);
        let outs = asking.await.unwrap();
        assert!(outs.iter().any(|out| out.t == "lease.grant"));
        assert_eq!(super::pen::thinking(), 0);
    }
    super::pen::reset_for_test();
    schema.drop().await;
}

/// The Mac gives her back with rows the server does not have yet: she
/// thinks on the server only once those arrived, and what she says here
/// then is a row of hers at the server's epoch, which the Mac reads.
#[tokio::test]
async fn handed_back_she_catches_up_before_she_thinks() {
    let Some(schema) = schema("her_log_pen_back").await else {
        return;
    };
    let db = &schema.db;
    super::pen::reset_for_test();
    settled(db).await;
    let mut mac = Side::new("dev_mac");
    let ask = mac.frame("lease.ask", None, json!({ "why": "talk" }));
    exchange(db, &mut mac, vec![ask], T0).await;
    mac.write_body(
        PERSONAL,
        "speech:m1",
        "speech.said",
        Author::Her,
        Some(said("我在 Mac 这边。", 1)),
        T0,
    );
    sync(db, &mut mac, PERSONAL, T0).await;
    // A line the Mac wrote just before it locked, not synced yet.
    let late = mac.write_body(
        PERSONAL,
        "speech:m2",
        "speech.said",
        Author::Her,
        Some(said("先睡了", 2)),
        T0 + 1,
    );
    let yielded = mac.frame(
        "lease.yield",
        Some("yield_back"),
        json!({ "epoch": mac.epoch, "through": { PERSONAL: 2 } }),
    );
    exchange(db, &mut mac, vec![yielded], T0 + MINUTE).await;
    assert!(matches!(
        super::pen::current(),
        super::pen::Pen::CatchingUp { ref behind, .. } if behind.get(PERSONAL) == Some(&2)
    ));
    assert!(super::pen::think().is_none(), "not before she caught up");
    use crate::services::persona_sync::lease as host;
    assert_eq!(host::place(), host::Where::MindOn(host::SITE.into()));

    let rows = mac.rows_frame(PERSONAL, vec![late]);
    exchange(db, &mut mac, vec![rows], T0 + MINUTE).await;
    assert_eq!(super::pen::current(), super::pen::Pen::Here);
    assert!(super::pen::think().is_some());

    let owner = owner(db).await;
    let row = store::write_hers(
        db,
        &root(),
        &owner,
        PERSONAL,
        None,
        "speech:web1",
        "speech.said",
        &said("我在网页这边。", 3),
    )
    .await
    .unwrap();
    let standing = lease::standing(db).await.unwrap().unwrap();
    assert_eq!(standing.from.as_deref(), Some("dev_mac"));
    assert_eq!(
        (row.origin.as_str(), row.epoch),
        (NODE, Some(standing.epoch))
    );
    sync(db, &mut mac, PERSONAL, super::now()).await;
    assert!(
        mac.log(PERSONAL)
            .iter()
            .any(|held| held.id == row.id && held.kind == "speech.said")
    );
    super::pen::reset_for_test();
    schema.drop().await;
}

/// The Mac holds her and drops off the network; the owner's phone says, by
/// `lease.place`, they are somewhere else. Within the stop time nothing
/// moves; past it the server takes her and thinks again.
#[tokio::test]
async fn the_server_takes_her_back_from_a_mac_cut_off_with_the_owner_elsewhere() {
    let Some(schema) = schema("her_log_pen_takeover").await else {
        return;
    };
    let db = &schema.db;
    let settings = on();
    super::pen::reset_for_test();
    settled(db).await;
    let mut mac = Side::new("dev_mac");
    let ask = mac.frame("lease.ask", None, json!({ "why": "place" }));
    let here = mac.frame(
        "lease.place",
        None,
        json!({ "place": "pl_home", "ownerHere": true }),
    );
    exchange(db, &mut mac, vec![ask, here], T0).await;
    assert!(super::pen::think().is_none());

    let mut phone = Side::new("dev_phone");
    let out = phone.frame(
        "lease.place",
        None,
        json!({ "place": "pl_out", "ownerHere": true }),
    );
    let root = root();
    for minutes in [1, 3] {
        let at = T0 + minutes * MINUTE;
        relay::take(
            db,
            &root,
            &settings,
            OWNER,
            "dev_phone",
            Core::Child,
            &mut phone.link,
            &out,
            at,
        )
        .await
        .unwrap();
        assert_eq!(
            lease::watch(db, &settings, at).await.unwrap(),
            None,
            "{minutes} min"
        );
        settled(db).await;
        assert!(super::pen::think().is_none(), "{minutes} min");
    }
    let at = T0 + 5 * MINUTE;
    relay::take(
        db,
        &root,
        &settings,
        OWNER,
        "dev_phone",
        Core::Child,
        &mut phone.link,
        &out,
        at,
    )
    .await
    .unwrap();
    let grant = lease::watch(db, &settings, at)
        .await
        .unwrap()
        .expect("taken over");
    assert_eq!(grant.how, myriad_persona_log::How::Takeover);
    assert_eq!(settled(db).await, super::pen::Pen::Here);
    assert!(super::pen::think().is_some());
    super::pen::reset_for_test();
    schema.drop().await;
}
