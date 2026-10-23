//! Her log on the server, against a real database
//! (`MYRIAD_MEDIA_TEST_DATABASE_URL`; skipped without one).
//!
//! Two host cores are simulated in memory with the same log rules the
//! native app links (`myriad_persona_log`): they write, swap vectors with
//! the server over protocol-2 frames (each one checked against the contract
//! on the way in and out), and must fold to the same her. The lease cases
//! of the shared conformance files run through the server's wiring.

use std::collections::BTreeMap;

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use myriad_persona_log::{
    AdmitInput, Entry, FoldInput, Grant, MergeInput, Row, Vector, admit, fold, merge,
    row::{Author, LOG_VERSION, Payload, Replicate},
    vector,
};
use myriad_persona_sync::{Rev, canonical::canonical_bytes};
use sea_orm::{ConnectionTrait, DatabaseConnection};
use serde_json::{Value, json};

use super::{
    NODE, Settings,
    keys::test_root::TestRoot,
    lease, relay,
    relay::{Outgoing, To},
    store,
};

const OWNER: i32 = 41;
const PERSONAL: &str = "personal:o41";
const OWN: &str = "own:";
const GROUP: &str = "group:telegram:-100";
/// A fixed time, microseconds.
const T0: i64 = 1_761_200_000_000_000;
const MINUTE: i64 = 60_000_000;

fn root() -> TestRoot {
    TestRoot([9; 32])
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
    Some(schema)
}

/// One host core in memory: a log per audience, its ledger, its payloads,
/// its clock and counts.
struct Side {
    id: String,
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
}

impl Side {
    fn new(id: &str) -> Self {
        Self {
            id: id.to_string(),
            logs: BTreeMap::new(),
            ledger: Vec::new(),
            payloads: BTreeMap::new(),
            clock: None,
            counts: BTreeMap::new(),
            written: 0,
            seq: 0,
            epoch: None,
            now: T0,
        }
    }

    fn log(&self, audience: &str) -> Vec<Row> {
        self.logs.get(audience).cloned().unwrap_or_default()
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
        self.now = self.now.max(now);
        let hlc = myriad_persona_log::clock::next(self.clock.as_ref(), now, 0, &self.id).unwrap();
        self.clock = Some(hlc.clone());
        self.written += 1;
        let count = self.counts.entry(audience.to_string()).or_default();
        *count += 1;
        let id = format!("{}_{}", self.id, self.written);
        let payload = payload.map(|text| {
            let reference = format!("blob:{id}");
            self.payloads
                .insert(reference.clone(), text.as_bytes().to_vec());
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
        };
        row.check().unwrap();
        // Its own rows go through the same merge as anyone's.
        self.merge(audience, vec![row.clone()]);
        row
    }

    fn merge(&mut self, audience: &str, incoming: Vec<Row>) {
        let admitted = admit(AdmitInput {
            rows: incoming,
            now: self.now,
            known: BTreeMap::new(),
            members: BTreeMap::new(),
            ledger: self.ledger.clone(),
        });
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
        });
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

    fn rows_frame(&mut self, audience: &str, rows: Vec<Row>) -> Value {
        let payloads: Vec<Value> = rows
            .iter()
            .filter(|row| !row.forgotten)
            .filter_map(|row| row.payload.as_ref())
            .filter_map(|payload| {
                self.payloads
                    .get(&payload.reference)
                    .map(|data| json!({ "ref": payload.reference, "data": BASE64.encode(data) }))
            })
            .collect();
        let mut body = json!({
            "audience": audience,
            "rows": rows.iter().map(Row::to_value).collect::<Vec<_>>(),
        });
        if !payloads.is_empty() {
            body["payloads"] = json!(payloads);
        }
        let id = format!("batch_{}", self.seq + 1);
        self.frame("log.rows", Some(&id), body)
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
                let lacking = vector::answer(&self.log(&audience), &theirs).unwrap();
                if lacking.is_empty() {
                    Vec::new()
                } else {
                    vec![self.rows_frame(&audience, lacking)]
                }
            }
            "lease.grant" => {
                let grant: Grant = serde_json::from_value(body["grant"].clone()).unwrap();
                if grant.holder == self.id {
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
            let outs = relay::take(db, &root, &on(), OWNER, &side.id, &frame, now)
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
        rows: store::rows(db, audience).await.unwrap(),
        leases: leases.to_vec(),
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
        "node.view_changed",
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
        "node.view_changed",
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
        assert!(
            side.payloads
                .values()
                .all(|data| !data.starts_with(b"too bright"))
        );
        assert!(
            side.payloads
                .values()
                .all(|data| !data.starts_with(b"dimmer"))
        );
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
    // What was not forgotten still reads back on the other side.
    assert_eq!(
        win.payloads
            .values()
            .filter(|data| data.as_slice() == b"likes cats")
            .count(),
        1
    );
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
    let outs = relay::take(db, &root(), &on(), OWNER, "dev_win", &frame, T0)
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
    let t0 = super::now().0;
    let mut mac = Side::new("dev_mac");
    mac.now = t0;
    mac.write(
        PERSONAL,
        "persona:likes",
        "persona.field",
        Author::Owner,
        Some("tea"),
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

    let forget = store::forget(db, &root(), PERSONAL, "memory:trip")
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
    assert_eq!(count(db, "SELECT COUNT(*) FROM agent_log_ledger").await, 1);

    let answered = store::answer(db, &root(), PERSONAL, &Vector::new())
        .await
        .unwrap();
    assert_eq!(answered[0].row.kind, "forget");
    let trip = answered
        .iter()
        .find(|sent| sent.row.object == "memory:trip" && sent.row.kind != "forget")
        .unwrap();
    assert!(trip.row.forgotten && trip.payload.is_none());
    let tea = answered
        .iter()
        .find(|sent| sent.row.object == "persona:likes")
        .unwrap();
    assert_eq!(tea.payload.as_deref(), Some(b"tea".as_slice()));

    // The Mac gets the forget and loses the trip too.
    sync(db, &mut mac, PERSONAL, t0 + 2 * MINUTE).await;
    assert!(
        mac.payloads
            .values()
            .all(|data| data.as_slice() != b"Kyoto")
    );
    let leases = lease::history(db).await.unwrap();
    assert_eq!(
        mac.fold(PERSONAL, &leases),
        server_fold(db, PERSONAL, &leases).await
    );
    schema.drop().await;
}

/// A group's log is the server's: only the core her mind is on gets its
/// rows or may add to it.
#[tokio::test]
async fn a_groups_rows_go_only_to_her_mind_and_come_back_only_from_it() {
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
    sync(db, &mut mac, GROUP, T0 + MINUTE).await;
    assert_eq!(store::rows(db, GROUP).await.unwrap().len(), 1);

    // Windows is not her mind: neither its vector nor its rows are taken.
    let vector = win.vector(GROUP);
    let outs = relay::take(db, &root(), &on(), OWNER, "dev_win", &vector, T0)
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
    let outs = relay::take(db, &root(), &on(), OWNER, "dev_win", &frame, T0)
        .await
        .unwrap();
    assert_eq!(delivered(&outs[0])["body"]["code"], "not_holder");
    assert_eq!(store::rows(db, GROUP).await.unwrap().len(), 1);
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
    let outs = relay::take(db, &root(), &on(), OWNER, "dev_mac", &frame, T0 + MINUTE)
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
        (standing.holder.as_str(), standing.how),
        (NODE, myriad_persona_log::How::Handover)
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
    let outs = relay::take(db, &root(), &on(), OWNER, "dev_mac", &old, T0 + 3 * MINUTE)
        .await
        .unwrap();
    assert_eq!(delivered(&outs[0])["body"]["code"], "bad_frame");
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
        relay::take(db, &root(), &off, OWNER, "dev_win", &vector, T0).await,
        Err(super::Error::Off)
    ));
    store::stop(db, OWNER).await.unwrap();
    assert!(matches!(
        relay::take(db, &root(), &on(), OWNER, "dev_win", &vector, T0).await,
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
        (grant.holder.as_str(), grant.how),
        (NODE, myriad_persona_log::How::Takeover)
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

/// The shared lease cases (`crates/conformance/v1/lease/move.json`) run
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
        .join("../crates/conformance/v1/lease/move.json");
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
                &standing.holder,
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

/// `log.rows` batches stay within the contract's limits, forgets first.
#[test]
fn batches_fit_the_frame_limits() {
    let mut side = Side::new("dev_mac");
    let mut sent = Vec::new();
    for n in 0..300 {
        let row = side.write(
            PERSONAL,
            &format!("memory:m{n}"),
            "memory.write",
            Author::Owner,
            Some("x"),
            T0 + n,
        );
        sent.push(store::Sent {
            payload: Some(vec![b'x'; 4096]),
            row,
        });
    }
    let bodies = relay::batches(PERSONAL, sent);
    assert!(bodies.len() >= 5, "{}", bodies.len());
    for body in &bodies {
        assert!(body["rows"].as_array().unwrap().len() <= relay::MAX_ROWS);
        assert!(body["payloads"].as_array().unwrap().len() <= relay::MAX_PAYLOADS);
        let out = Outgoing {
            to: To::Sender,
            t: "log.rows",
            id: Some("rows_1".into()),
            body: body.clone(),
        };
        relay::encode(&out, 1, 0, 1).unwrap();
    }
    let total: usize = bodies
        .iter()
        .map(|body| body["rows"].as_array().unwrap().len())
        .sum();
    assert_eq!(total, 300);
}
