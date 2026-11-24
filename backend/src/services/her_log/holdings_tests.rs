//! What the server holds of an audience is counted by the database, from its
//! rows' places (`store::holdings`): the same the core counts from the rows
//! themselves (`vector::holdings`). And when a side lacks none of it
//! (`store::lacks_none`): the same times the core has no row for it
//! (`vector::answer`).

use super::*;

/// A small seeded generator: the cases are the same every run.
struct Seeded(u64);

impl Seeded {
    fn below(&mut self, most: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % most
    }
}

/// Origins that differ in letter case only, and in what sorts where.
const ORIGINS: [&str; 7] = [
    "site",
    "Site",
    "dev_mac",
    "dev_Mac",
    "DEV_MAC",
    "loc_a",
    "dev_mac_2",
];

/// One shared row, of `origin` at `dseq`, as a server keeps it.
fn row_at(whole: &Row, audience: &str, origin: &str, dseq: u64) -> Row {
    Row {
        id: format!("{origin}_{dseq}"),
        origin: origin.to_string(),
        dseq: Some(dseq),
        hlc: Rev {
            node: origin.to_string(),
            ..whole.hlc.clone()
        },
        audience: audience.to_string(),
        ..whole.clone()
    }
}

/// A row to make others from.
fn whole() -> Row {
    Side::new("dev_mac").write(
        PERSONAL,
        "node:a",
        "node.span",
        Author::Owner,
        Some("话"),
        T0,
    )
}

/// The places (origin, dseq) of one case: gaps, one row alone, one far off,
/// none.
fn places(seeded: &mut Seeded) -> BTreeSet<(&'static str, u64)> {
    let mut places = BTreeSet::new();
    for origin in ORIGINS {
        match seeded.below(6) {
            0 | 1 => {}
            2 => {
                places.insert((origin, 1 + seeded.below(3)));
            }
            3 => {
                places.extend((1..=1 + seeded.below(30)).map(|dseq| (origin, dseq)));
            }
            _ => {
                for dseq in 1..=1 + seeded.below(30) {
                    if seeded.below(4) > 0 {
                        places.insert((origin, dseq));
                    }
                }
                if seeded.below(3) == 0 {
                    places.insert((origin, 1_000_000 + seeded.below(5)));
                }
            }
        }
    }
    places
}

/// What the server keeps of `user`'s `audience`, at once.
async fn keep(db: &DatabaseConnection, user: i32, rows: &[Row]) {
    for rows in rows.chunks(200) {
        let values: Vec<String> = rows
            .iter()
            .map(|row| {
                format!(
                    "({user}, '{}', '{}', '{}', {}, '{}', '{}', NULL, '{}', {}, '{}', 'site', now())",
                    row.id,
                    row.audience,
                    row.origin,
                    row.dseq.unwrap(),
                    row.hlc,
                    row.object,
                    row.kind,
                    row.forgotten,
                    row.to_value(),
                )
            })
            .collect();
        db.execute_unprepared(&format!(
            "INSERT INTO agent_log_rows (user_id, id, audience, origin, dseq, hlc, object, \
             subject, kind, forgotten, envelope, received_from, received_at) VALUES {}",
            values.join(", ")
        ))
        .await
        .unwrap();
    }
}

/// The database counts what the core counts from the rows, whatever places
/// are held: gaps, one alone, none, far-off ones, origins that differ in
/// case, stubs, and another account's rows beside them.
#[tokio::test]
async fn what_is_held_is_counted_as_the_core_counts_it() {
    let Some(schema) = schema("her_log_holdings").await else {
        return;
    };
    let db = &schema.db;
    db.execute_unprepared("INSERT INTO users (id, username) VALUES (42, 'other')")
        .await
        .unwrap();
    let whole = whole();
    let mut seeded = Seeded(0x5eed);
    let named: Vec<BTreeSet<(&str, u64)>> = vec![
        [].into(),
        [("site", 1)].into(),
        [("site", 2)].into(),
        [("site", 1), ("site", 2), ("Site", 1)].into(),
        [("site", 1), ("site", 3), ("dev_mac", 2), ("dev_Mac", 1)].into(),
    ];
    let random = (0..200).map(|_| places(&mut seeded)).collect::<Vec<_>>();
    for (n, places) in named.into_iter().chain(random).enumerate() {
        db.execute_unprepared("DELETE FROM agent_log_rows")
            .await
            .unwrap();
        let mut rows: Vec<Row> = places
            .iter()
            .map(|(origin, dseq)| row_at(&whole, PERSONAL, origin, *dseq))
            .collect();
        for row in rows.iter_mut().skip(n % 3).step_by(5) {
            *row = row.stub();
        }
        keep(db, OWNER, &rows).await;
        // Another account's, in the same audience, and this one's in another:
        // neither is counted with it.
        let others = [1u64, 2, 3, 900].map(|dseq| row_at(&whole, PERSONAL, "site", dseq));
        keep(db, 42, &others).await;
        let elsewhere = [1u64, 4].map(|dseq| Row {
            id: format!("elsewhere_{dseq}"),
            ..row_at(&whole, OTHER, "site", dseq)
        });
        keep(db, OWNER, &elsewhere).await;
        let held = store::holdings(db, OWNER, PERSONAL).await.unwrap();
        assert_eq!(
            held,
            vector::holdings(&rows).unwrap(),
            "case {n}: {places:?}"
        );
        let kept = store::rows(db, OWNER, PERSONAL).await.unwrap();
        assert_eq!(kept.len(), rows.len());
        assert_eq!(held, vector::holdings(&kept).unwrap(), "case {n}, as kept");
        assert_eq!(
            store::holdings(db, 42, PERSONAL).await.unwrap(),
            vector::holdings(&others).unwrap()
        );
        assert_eq!(
            store::holdings(db, OWNER, OTHER).await.unwrap(),
            vector::holdings(&elsewhere).unwrap()
        );
    }
    // An audience that holds nothing.
    assert_eq!(
        store::holdings(db, OWNER, "group:none").await.unwrap(),
        vector::holdings(&[]).unwrap()
    );
    schema.drop().await;
}

/// A count the place's column does not hold is refused, not kept as another:
/// the core and the database would count it apart.
#[tokio::test]
async fn a_count_past_what_is_kept_is_refused() {
    let Some(schema) = schema("her_log_past_count").await else {
        return;
    };
    let db = &schema.db;
    let owner = owner(db).await;
    let row = row_at(&whole(), PERSONAL, "dev_mac", 1 << 63).stub();
    let batch = wire::Batch {
        audience: PERSONAL.to_string(),
        rows: vec![row],
        payloads: Vec::new(),
        withheld: Vec::new(),
        more: None,
    };
    let took = store::take(
        db,
        &root(),
        &owner,
        "dev_mac",
        batch,
        &BTreeSet::new(),
        T0 + MINUTE,
    )
    .await;
    assert!(
        matches!(&took, Err(super::super::Error::Refused(why)) if why.contains("count")),
        "{took:?}"
    );
    assert_eq!(
        count(
            db,
            "SELECT COUNT(*) FROM agent_log_rows WHERE id = 'dev_mac_9223372036854775808'"
        )
        .await,
        0
    );
    schema.drop().await;
}

/// A side lacks none of it exactly when the core has no row for it: none
/// too few, so no row it lacks stays behind, and none too many, so what is
/// read for nothing stays little.
#[test]
fn a_side_lacks_none_exactly_when_the_core_gives_it_no_row() {
    let whole = whole();
    let mut seeded = Seeded(0xfeed);
    let (mut none, mut some) = (0, 0);
    for n in 0..4000 {
        let places = places(&mut seeded);
        let rows: Vec<Row> = places
            .iter()
            .map(|(origin, dseq)| row_at(&whole, PERSONAL, origin, *dseq))
            .collect();
        let held = vector::holdings(&rows).unwrap();
        let mut theirs = Vector::new();
        for origin in ORIGINS {
            let top = (places.iter())
                .filter(|(of, _)| *of == origin)
                .map(|(_, dseq)| *dseq)
                .max()
                .unwrap_or_default();
            // Nothing said of it, less, the last of it, one short of it, more.
            match seeded.below(7) {
                0 => {}
                1 => {
                    theirs.insert(origin.into(), seeded.below(5));
                }
                2 | 3 => {
                    theirs.insert(origin.into(), top);
                }
                4 => {
                    theirs.insert(origin.into(), top.saturating_sub(1));
                }
                5 => {
                    theirs.insert(origin.into(), top + 1);
                }
                _ => {
                    let through = held.vector.get(origin).copied().unwrap_or_default();
                    theirs.insert(origin.into(), through);
                }
            }
        }
        let gives = !vector::answer(&rows, &theirs).unwrap().is_empty();
        assert_eq!(
            store::lacks_none(&held, &theirs),
            !gives,
            "case {n}: {places:?} {theirs:?}"
        );
        if gives { some += 1 } else { none += 1 }
    }
    // Both happen, or the case proves nothing.
    assert!(none > 100 && some > 100, "{none} {some}");
    // Nothing held is lacked by none.
    assert!(store::lacks_none(
        &vector::holdings(&[]).unwrap(),
        &Vector::new()
    ));
}

/// How often the rows were read whole for `frame` of `side`'s.
async fn read_for(db: &DatabaseConnection, side: &mut Side, frame: Value) -> usize {
    store::READS.with(|reads| reads.set(0));
    let device = side.id.clone();
    relay::take(
        db,
        &root(),
        &on(),
        OWNER,
        &device,
        Core::Pc,
        &mut side.link,
        &frame,
        T0,
    )
    .await
    .unwrap();
    store::READS.with(|reads| reads.take())
}

/// A log.vector of a side that lacks nothing reads none of the rows; one of
/// a side that lacks one, or asks again for one, reads them once.
#[tokio::test]
async fn the_rows_are_read_once_and_only_when_some_may_go() {
    let Some(schema) = schema("her_log_reads").await else {
        return;
    };
    let db = &schema.db;
    db.execute_unprepared("DELETE FROM agent_log_rows")
        .await
        .unwrap();
    let whole = whole();
    let rows = [("site", 1), ("site", 2), ("site", 5), ("dev_air", 1)]
        .map(|(origin, dseq)| row_at(&whole, PERSONAL, origin, dseq));
    keep(db, OWNER, &rows).await;
    let held = store::holdings(db, OWNER, PERSONAL).await.unwrap();
    let mut mac = Side::new("dev_mac");
    for (says, reads) in [
        // Its count covers the last of them, the gap too.
        (json!({ "vector": { "site": 5, "dev_air": 1 } }), 0),
        (
            json!({ "vector": { "site": 9, "dev_air": 1, "dev_mac": 3 } }),
            0,
        ),
        (
            json!({ "vector": { "site": 5, "dev_air": 1 }, "ask": ["site_1"] }),
            1,
        ),
        // A gap of its own, though it lacks none of them.
        (
            json!({ "vector": { "site": 5, "dev_air": 1 }, "beyond": { "site": [8] } }),
            1,
        ),
        (
            json!({ "vector": { "site": 5, "dev_air": 1, "dev_mac": 3 }, "beyond": { "dev_mac": [5] } }),
            1,
        ),
        (json!({ "vector": held.vector }), 1),
        (json!({ "vector": held.vector, "beyond": held.beyond }), 1),
        (json!({ "vector": { "site": 5 } }), 1),
        (json!({ "vector": {} }), 1),
    ] {
        let mut body = says.clone();
        body["audience"] = json!(PERSONAL);
        let frame = mac.frame("log.vector", None, body);
        assert_eq!(read_for(db, &mut mac, frame).await, reads, "{says}");
    }
    schema.drop().await;
}

/// The answers to `frames`, each a log.vector, on one link: the same whether
/// or not the rows are read. `after` in a body is answered with the page the
/// last answer named. The ids that are made anew each time are left out.
async fn said_to(
    db: &DatabaseConnection,
    mut link: relay::Session,
    frames: &[Value],
    read_always: bool,
) -> (Vec<String>, usize) {
    store::READ_ALWAYS.with(|always| always.set(read_always));
    store::READS.with(|reads| reads.set(0));
    let mut said = Vec::new();
    let mut page: Option<String> = None;
    for (n, body) in frames.iter().enumerate() {
        let mut body = body.clone();
        if body.get("after").is_some() {
            match &page {
                Some(page) => body["after"] = json!(page),
                None => body["after"] = json!("page_of_none"),
            }
        }
        let frame = json!({
            "v": 1, "t": "log.vector", "seq": n + 1, "ack": 0, "at": 1, "body": body,
        });
        let outs = relay::take(
            db,
            &root(),
            &on(),
            OWNER,
            "dev_air",
            Core::Pc,
            &mut link,
            &frame,
            T0 + MINUTE,
        )
        .await;
        said.push(format!("frame {n}"));
        for mut out in outs.unwrap() {
            if let Some(more) = out.body.get("more").and_then(Value::as_str) {
                page = Some(more.to_string());
                out.body["more"] = json!("page");
            }
            let id = out.id.is_some();
            said.push(format!("{:?} {} {id} {}", out.to, out.t, out.body));
        }
    }
    store::READ_ALWAYS.with(|always| always.set(false));
    (said, store::READS.with(|reads| reads.take()))
}

/// Everything the database keeps of her log: what answering may not change.
async fn kept(db: &DatabaseConnection) -> Vec<String> {
    let mut kept = Vec::new();
    for table in [
        "agent_log_rows",
        "agent_log_payloads",
        "agent_log_audience_keys",
        "agent_log_object_keys",
        "agent_log_ledgers",
        "agent_log_elsewhere",
        "agent_log_counters",
        "agent_log_clock",
        "agent_log_full_links",
    ] {
        let row = db
            .query_one_raw(sea_orm::Statement::from_string(
                sea_orm::DatabaseBackend::Postgres,
                format!(
                    "SELECT COUNT(*) || ' ' || COALESCE(md5(string_agg(t::text, '|' ORDER BY \
                     t::text)), '') AS all_of_it FROM {table} AS t"
                ),
            ))
            .await
            .unwrap()
            .unwrap();
        kept.push(format!(
            "{table} {}",
            row.try_get::<String>("", "all_of_it").unwrap()
        ));
    }
    kept
}

/// What a side hears is the same, byte for byte, as when every log.vector
/// had the rows read for it, frame after frame of a link that pages, one
/// that does not and one of an old core: with forgets and a retract and
/// what they took away in the log, bodies asked for again, gaps on either side, origins the
/// site has none of and counts past the site's. And answering keeps nothing.
#[tokio::test]
async fn a_side_is_answered_as_if_the_rows_were_read_every_time() {
    let Some(schema) = schema("her_log_same_answer").await else {
        return;
    };
    let db = &schema.db;
    let _pages = relay::pages_in_test(1);
    // Nodes with bodies, one of them forgotten by the owner on the Mac, one
    // on the site and one taken back.
    let mut mac = Side::new("dev_mac");
    for n in 0..5 {
        for kind in ["node.created", "node.span", "node.span"] {
            mac.write(
                PERSONAL,
                &format!("node:n{n}"),
                kind,
                Author::Owner,
                Some("话"),
                T0 + n,
            );
        }
    }
    sync(db, &mut mac, PERSONAL, T0 + MINUTE).await;
    mac.write(PERSONAL, "node:n1", "forget", Author::Owner, None, T0 + 9);
    mac.write(PERSONAL, "node:n3", "retract", Author::Owner, None, T0 + 9);
    sync(db, &mut mac, PERSONAL, T0 + MINUTE).await;
    let owner = owner(db).await;
    store::forget(db, &owner, PERSONAL, "node:n2")
        .await
        .unwrap();
    let with_bodies: Vec<String> = (store::rows(db, OWNER, PERSONAL).await.unwrap())
        .iter()
        .filter(|row| row.payload.is_some() && !row.forgotten)
        .map(|row| row.id.clone())
        .collect();
    assert!(with_bodies.len() > 3, "{with_bodies:?}");
    let stubs = count(
        db,
        "SELECT COUNT(*) FROM agent_log_rows WHERE forgotten AND kind <> 'forget'",
    )
    .await;
    assert!(stubs > 0, "what the forgets took away");
    // Two more origins: one with a gap, one of more than a page.
    let whole = whole();
    let mut rows: Vec<Row> = [1u64, 2, 5, 6]
        .map(|dseq| row_at(&whole, PERSONAL, "dev_gap", dseq))
        .to_vec();
    rows.extend((1..=300).map(|dseq| row_at(&whole, PERSONAL, "dev_many", dseq)));
    keep(db, OWNER, &rows).await;

    let held = store::holdings(db, OWNER, PERSONAL).await.unwrap();
    assert!(held.vector.contains_key("site"), "{:?}", held.vector);
    assert_eq!(held.beyond["dev_gap"], [5, 6]);
    let all: Vector = (held.vector.iter())
        .map(|(origin, through)| (origin.clone(), *through))
        .chain([("dev_gap".to_string(), 6)])
        .collect();
    let before = kept(db).await;
    let mut seeded = Seeded(0xbeef);
    let (mut skipped, mut read) = (0, 0);
    for case in 0..150 {
        let mut frames: Vec<Value> = (0..1 + seeded.below(4))
            .map(|_| {
                let mut theirs = all.clone();
                // Half of them lack nothing.
                if seeded.below(2) == 0 {
                    for through in theirs.values_mut() {
                        *through -= seeded.below(3).min(*through);
                    }
                }
                if seeded.below(4) == 0 {
                    theirs.insert("dev_gap".into(), held.vector["dev_gap"]);
                }
                if seeded.below(4) == 0 {
                    theirs.insert("dev_many".into(), seeded.below(60));
                }
                // More than the site's, and an origin it has none of.
                if seeded.below(3) == 0 {
                    *theirs.get_mut("site").unwrap() += 1 + seeded.below(9);
                }
                if seeded.below(3) == 0 {
                    theirs.insert("dev_new".into(), 1 + seeded.below(9));
                }
                if seeded.below(8) == 0 {
                    theirs.remove("dev_mac");
                }
                let mut body = json!({ "audience": PERSONAL, "vector": theirs });
                match seeded.below(6) {
                    0 => body["beyond"] = json!({ "dev_gap": [6] }),
                    1 => body["beyond"] = json!({ "dev_new": [40, 41] }),
                    _ => {}
                }
                match seeded.below(6) {
                    0 => {
                        let at = seeded.below(with_bodies.len() as u64) as usize;
                        body["ask"] = json!([with_bodies[at], "no_such_row"]);
                    }
                    1 => body["ask"] = json!(["no_such_row"]),
                    _ => {}
                }
                if seeded.below(4) == 0 {
                    body["after"] = json!("");
                }
                body
            })
            .collect();
        // Behind, then not, then behind as before: it gets the rows again.
        if case % 5 == 0 {
            let of = |vector: &Vector| json!({ "audience": PERSONAL, "vector": vector });
            frames = vec![of(&held.vector), of(&all), of(&held.vector)];
        }
        let link = || match case % 3 {
            0 => relay::Session::of(&["log2".to_string(), relay::PAGES.to_string()]),
            1 => relay::Session::of(&["log2".to_string()]),
            _ => relay::Session::new(1),
        };
        let (always, reads_always) = said_to(db, link(), &frames, true).await;
        let (now, reads) = said_to(db, link(), &frames, false).await;
        assert_eq!(now, always, "case {case}: {frames:#?}");
        assert!(reads <= reads_always);
        skipped += reads_always - reads;
        read += reads;
    }
    // Both happen, or the case proves nothing.
    assert!(skipped > 30 && read > 30, "{skipped} {read}");
    assert_eq!(kept(db).await, before);
    schema.drop().await;
}
