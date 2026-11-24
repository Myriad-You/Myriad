//! What the server holds of an audience is counted by the database, from its
//! rows' places (`store::holdings`): the same the core counts from the rows
//! themselves (`vector::holdings`).

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
