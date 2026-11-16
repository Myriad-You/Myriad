//! Her nodes in her log ([`super::logged`]).

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::{Value, json};

use super::forget::{self, Forgets};
use super::{Author, RowIn, audience, logged, partner, read, write};
use crate::services::her_log::{
    self, Settings, keys, pen, store, test_partner::PARTNER, test_settings,
};

const OWNER: i32 = 1;

async fn schema(prefix: &str) -> Option<crate::db::IsolatedSchema> {
    let url = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL").ok()?;
    let schema = crate::db::IsolatedSchema::migrated(&url, prefix).await;
    schema
        .db
        .execute_unprepared("INSERT INTO users (id, username) VALUES (1, 'one'), (7, 'seven')")
        .await
        .unwrap();
    Some(schema)
}

/// A topic she starts in `audience` with one fact on it, as her rows.
fn topic(audience: &str, memory: &str, text: &str) -> (String, Vec<RowIn>) {
    // Her own audience has no one else in it.
    let said_by = if audience == audience::OWN {
        "her"
    } else {
        "partner"
    };
    let made = myriad_nodes::entries::fact_rows(
        serde_json::from_value(json!({
            "audience": audience,
            "start": {"name": "houseplants"},
            "facts": [{"memory": memory, "text": text, "saidBy": [said_by]}],
        }))
        .unwrap(),
    )
    .unwrap();
    let rows = made
        .rows
        .into_iter()
        .map(|row| RowIn {
            id: row.id,
            node: made.node.clone(),
            kind: row.kind,
            body: row.body,
            author: Author::Her,
        })
        .collect();
    (made.node, rows)
}

/// `sql`'s rows as (id, dseq, envelope), by dseq.
async fn placed(db: &DatabaseConnection, sql: &str, audience: &str) -> Vec<(String, i64, Value)> {
    db.query_all_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        sql,
        [OWNER.into(), audience.into()],
    ))
    .await
    .unwrap()
    .into_iter()
    .map(|row| {
        (
            row.try_get("", "id").unwrap(),
            row.try_get("", "dseq").unwrap(),
            row.try_get("", "envelope").unwrap(),
        )
    })
    .collect()
}

const NODE_ROWS: &str = "SELECT row_id AS id, dseq, envelope FROM agent_node_rows \
                         WHERE user_id = $1 AND audience = $2 ORDER BY dseq";
const LOG_ROWS: &str = "SELECT id, dseq, envelope FROM agent_log_rows \
                        WHERE user_id = $1 AND audience = $2 ORDER BY dseq";

async fn count(db: &DatabaseConnection, sql: &str) -> i64 {
    db.query_one_raw(Statement::from_string(DatabaseBackend::Postgres, sql))
        .await
        .unwrap()
        .unwrap()
        .try_get::<i64>("", "n")
        .unwrap()
}

/// Rows written while her log is off keep the site's own counts. Once the
/// owner syncs it, the next batch puts them in first, under the log's next
/// counts after what the log already holds there, and then itself: the
/// same envelopes on both sides, each body sealed beside its row. A forget
/// goes in once and on the ledger; another account's talk never goes in.
#[tokio::test]
async fn her_nodes_go_into_her_log_under_its_counts() {
    let Some(schema) = schema("memory_nodes_logged").await else {
        return;
    };
    let db = &schema.db;
    test_settings::set(Settings {
        enabled: true,
        ..Settings::default()
    });
    keys::test_root::use_here(keys::test_root::TestRoot([9; 32]));
    her_log::test_partner::give(db, OWNER, PARTNER).await;
    partner::remember(Some(PARTNER));
    let talk = audience::owners_talk();
    assert_eq!(talk, format!("personal:{PARTNER}"));
    let thought = pen::think().expect("her mind is here in tests");

    // The log is not on for the account yet: the site's own counts.
    let (node, first) = topic(&talk, "m1", "has a snake plant");
    write::write(db, OWNER, &talk, &first, Some(&thought))
        .await
        .unwrap();
    assert_eq!(placed(db, NODE_ROWS, &talk).await.len(), 2);
    assert!(placed(db, LOG_ROWS, &talk).await.is_empty());

    // On: the log holds a row of its own there already.
    store::start(db, OWNER).await.unwrap();
    let owner = her_log::owner(db, OWNER).await.unwrap();
    store::forget(db, &owner, &talk, "node:elsewhere")
        .await
        .unwrap();
    let (_, second) = topic(&talk, "m2", "repots in spring");
    let written = write::write(db, OWNER, &talk, &second, Some(&thought))
        .await
        .unwrap();
    assert_eq!(written.rows.len(), 1);
    let nodes = placed(db, NODE_ROWS, &talk).await;
    let logged_rows = placed(db, LOG_ROWS, &talk).await;
    assert_eq!(
        nodes.iter().map(|(_, dseq, _)| *dseq).collect::<Vec<_>>(),
        vec![2, 3, 4],
        "the old rows first, in order, then the new one"
    );
    assert_eq!(logged_rows.len(), 4);
    assert_eq!(
        logged_rows[1..],
        nodes[..],
        "the same envelopes on both sides"
    );
    assert_eq!(
        count(db, "SELECT COUNT(*) AS n FROM agent_log_payloads").await,
        3,
        "each body sealed beside its row"
    );

    // A forget goes in once, on the ledger; the same again takes no count.
    for _ in 0..2 {
        forget::forget_one(db, OWNER, &talk, &node, Forgets::Node)
            .await
            .unwrap();
    }
    let logged_rows = placed(db, LOG_ROWS, &talk).await;
    assert_eq!(logged_rows.len(), 5);
    assert_eq!(logged_rows[4].2["kind"], "forget");
    assert_eq!(logged_rows[4].1, 5);
    let ledger = store::ledger(db, OWNER, &talk).await.unwrap();
    assert!(
        ledger
            .iter()
            .any(|entry| entry.target == format!("node:{node}")),
        "{ledger:?}"
    );

    // Another account's talk stays on the site.
    let theirs = audience::private_talk(7, Some(OWNER));
    let (_, others) = topic(&theirs, "m3", "keeps bees");
    write::write(db, 7, &theirs, &others, Some(&thought))
        .await
        .unwrap();
    assert_eq!(
        count(
            db,
            "SELECT COUNT(*) AS n FROM agent_log_rows WHERE audience NOT LIKE 'personal:partner_%'"
        )
        .await,
        0
    );
    // Nothing left to put in.
    assert_eq!(
        logged::catch_up_all(db, keys::root(), &owner)
            .await
            .unwrap(),
        0
    );
    schema.drop().await;
}

/// A kept row whose envelope is not whole stays out of her log, under the
/// site's own count and taking none of the log's; the rows after it go in.
/// Mended, it goes in on the next catch-up.
#[tokio::test]
async fn a_row_not_whole_stays_out_and_the_rest_go_in() {
    let Some(schema) = schema("memory_nodes_not_whole").await else {
        return;
    };
    let db = &schema.db;
    test_settings::set(Settings {
        enabled: true,
        ..Settings::default()
    });
    keys::test_root::use_here(keys::test_root::TestRoot([9; 32]));
    her_log::test_partner::give(db, OWNER, PARTNER).await;
    partner::remember(Some(PARTNER));
    let talk = audience::owners_talk();
    let thought = pen::think().expect("her mind is here in tests");
    // Written while the log is off: the site's own counts 1 and 2.
    let (_, rows) = topic(&talk, "m1", "has a snake plant");
    write::write(db, OWNER, &talk, &rows, Some(&thought))
        .await
        .unwrap();
    let kept = placed(db, NODE_ROWS, &talk).await;
    assert_eq!(kept.len(), 2);
    let (broken, count_before, _) = kept[0].clone();
    let set_epoch = |epoch: &str| {
        format!(
            "UPDATE agent_node_rows SET envelope = jsonb_set(envelope, '{{epoch}}', '{epoch}') \
             WHERE row_id = '{broken}'"
        )
    };
    let epoch = kept[0].2["epoch"].to_string();
    db.execute_unprepared(&set_epoch("null")).await.unwrap();

    store::start(db, OWNER).await.unwrap();
    let owner = her_log::owner(db, OWNER).await.unwrap();
    assert_eq!(
        logged::catch_up_all(db, keys::root(), &owner)
            .await
            .unwrap(),
        1
    );
    let logged_rows = placed(db, LOG_ROWS, &talk).await;
    assert_eq!(logged_rows.len(), 1);
    assert_eq!(logged_rows[0].0, kept[1].0);
    assert_eq!(logged_rows[0].1, 1, "no count taken by the one left out");
    let after = placed(db, NODE_ROWS, &talk).await;
    let left = after.iter().find(|(id, _, _)| *id == broken).unwrap();
    assert_eq!(left.1, count_before, "under the site's own count");
    // Again: still out, nothing else to put in.
    assert_eq!(
        logged::catch_up_all(db, keys::root(), &owner)
            .await
            .unwrap(),
        0
    );

    db.execute_unprepared(&set_epoch(&epoch)).await.unwrap();
    assert_eq!(
        logged::catch_up_all(db, keys::root(), &owner)
            .await
            .unwrap(),
        1
    );
    let logged_rows = placed(db, LOG_ROWS, &talk).await;
    assert_eq!(
        logged_rows
            .iter()
            .map(|(id, dseq, _)| (id.clone(), *dseq))
            .collect::<Vec<_>>(),
        vec![(kept[1].0.clone(), 1), (broken, 2)]
    );
    schema.drop().await;
}

/// Her own rows wait for the partner id: until the owner's talk is under
/// it a move may still rewrite them, and a row in her log never changes.
#[tokio::test]
async fn her_own_rows_wait_for_the_partner_id() {
    let Some(schema) = schema("memory_nodes_logged_own").await else {
        return;
    };
    let db = &schema.db;
    test_settings::set(Settings {
        enabled: true,
        ..Settings::default()
    });
    keys::test_root::use_here(keys::test_root::TestRoot([9; 32]));
    partner::remember(None);
    store::start(db, OWNER).await.unwrap();
    let thought = pen::think().expect("her mind is here in tests");
    let own = audience::OWN;
    let (_, first) = topic(own, "m1", "likes rain");
    write::write(db, OWNER, own, &first, Some(&thought))
        .await
        .unwrap();
    assert!(placed(db, LOG_ROWS, own).await.is_empty());

    her_log::test_partner::give(db, OWNER, PARTNER).await;
    let (_, second) = topic(own, "m2", "likes snow");
    write::write(db, OWNER, own, &second, Some(&thought))
        .await
        .unwrap();
    assert_eq!(
        placed(db, LOG_ROWS, own).await,
        placed(db, NODE_ROWS, own).await
    );
    assert_eq!(placed(db, LOG_ROWS, own).await.len(), 3);
    schema.drop().await;
}

/// A memory forgotten on another end comes as its rows in her log all
/// forgotten; taken in here, it goes from the site's table with what rests
/// on it on her nodes, as when it is forgotten here: how their life stands
/// resting on it becomes a stub, so it no longer stands and no longer goes
/// into her private talk's standing list (`recall`, read off the node's
/// head), while what else the topic holds stays.
#[tokio::test]
async fn a_memory_forgotten_on_another_end_takes_what_stands_on_it() {
    let Some(schema) = schema("memory_nodes_logged_forgotten").await else {
        return;
    };
    let db = &schema.db;
    test_settings::set(Settings {
        enabled: true,
        ..Settings::default()
    });
    keys::test_root::use_here(keys::test_root::TestRoot([9; 32]));
    let root = keys::test_root::TestRoot([9; 32]);
    her_log::test_partner::give(db, OWNER, PARTNER).await;
    partner::remember(Some(PARTNER));
    let talk = audience::owners_talk();
    let thought = pen::think().expect("her mind is here in tests");
    store::start(db, OWNER).await.unwrap();
    let owner = her_log::owner(db, OWNER).await.unwrap();
    db.execute_unprepared(
        "INSERT INTO agent_memories (id, user_id, kind, content, source, venue, audience, \
         valid_from, created_at, updated_at) VALUES \
         ('m1', 1, 'fact', 'trying a blue flask this week', 'chat', 'private', '[1]'::jsonb, \
          now(), now(), now()), \
         ('m2', 1, 'fact', 'has a snake plant', 'chat', 'private', '[1]'::jsonb, \
          now(), now(), now())",
    )
    .await
    .unwrap();
    let (node, rows) = topic(&talk, "m2", "has a snake plant");
    write::write(db, OWNER, &talk, &rows, Some(&thought))
        .await
        .unwrap();
    let standing = RowIn {
        id: myriad_nodes::entries::row_id_from("node.standing", &node, "them-0a1b2c3d4e", "her"),
        node: node.clone(),
        kind: "node.standing".into(),
        body: json!({
            "version": myriad_nodes::BODY_VERSION,
            "sync": "shared",
            "saidBy": ["her"],
            "base": [],
            "item": "them-0a1b2c3d4e",
            "text": "trying a blue flask this week",
            "because": ["memory:m1"],
        }),
        author: Author::Her,
    };
    write::write(db, OWNER, &talk, &[standing], Some(&thought))
        .await
        .unwrap();
    let stands = || async {
        let head = read::node(db, OWNER, &talk, &node).await.unwrap().unwrap();
        myriad_nodes::stands::shown_on(&node, &head)
            .into_iter()
            .map(|item| item.text)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        stands().await,
        vec!["trying a blue flask this week".to_string()]
    );
    // Both memories in her log, as the site compares them.
    her_log::state::sync(db, &root, OWNER).await.unwrap();

    // Forgotten on another end: what reaches the site is the forget row.
    store::forget(db, &owner, &talk, "memory:m1").await.unwrap();
    assert!(her_log::memories::take_in(db, &root, &owner).await.unwrap() >= 1);
    assert_eq!(
        count(
            db,
            "SELECT COUNT(*) AS n FROM agent_memories WHERE id = 'm1'"
        )
        .await,
        0
    );
    assert!(stands().await.is_empty(), "it no longer stands");
    assert_eq!(
        count(
            db,
            "SELECT COUNT(*) AS n FROM agent_node_rows \
             WHERE kind = 'node.standing' AND body <> ''"
        )
        .await,
        0,
        "no word of it is kept"
    );
    // The topic and its other fact stay.
    assert_eq!(
        count(
            db,
            "SELECT COUNT(*) AS n FROM agent_node_rows WHERE kind = 'node.entry' AND body <> ''"
        )
        .await,
        1
    );
    assert_eq!(
        count(
            db,
            "SELECT COUNT(*) AS n FROM agent_memories WHERE id = 'm2'"
        )
        .await,
        1
    );
    schema.drop().await;
}

/// Her log first turned on over her own nodes written while it was off:
/// her memories go in first under the epoch that stands, then her older
/// nodes under none held (0). So her own audience holds rows whose count
/// comes after rows of a higher epoch while their time comes before. That
/// is the log as it is meant to be, not a fault to mend here: a count is a
/// row's place for good, and a vector counts places without a gap, so the
/// site sends by count. The core checks a row's epoch against the rows
/// before it in time (`admit` sorts by log order), so a side that takes
/// the whole lot accepts every row. A side that takes it page by page must
/// take its floor from the rows it holds from before the page's earliest
/// row in time, not from every row it holds: with the highest it holds, as
/// below, the older nodes are refused as `epoch_back` and it never catches
/// up (what a Mac did on 2026-11-14).
#[tokio::test]
async fn her_old_nodes_come_after_her_memories_with_an_older_epoch() {
    use myriad_persona_log::admit::{AdmitInput, Reason, admit};

    let Some(schema) = schema("memory_nodes_logged_order").await else {
        return;
    };
    let db = &schema.db;
    test_settings::set(Settings {
        enabled: true,
        ..Settings::default()
    });
    keys::test_root::use_here(keys::test_root::TestRoot([9; 32]));
    her_log::test_partner::give(db, OWNER, PARTNER).await;
    partner::remember(Some(PARTNER));
    let thought = pen::think().expect("her mind is here in tests");
    let own = audience::OWN;

    // While the log is off and no grant was ever issued.
    let (_, before) = topic(own, "m1", "likes rain");
    write::write(db, OWNER, own, &before, Some(&thought))
        .await
        .unwrap();
    assert!(placed(db, LOG_ROWS, own).await.is_empty());

    // On: her memories go in under the first grant, then her nodes.
    store::start(db, OWNER).await.unwrap();
    let epoch = her_log::lease::current(db).await.unwrap().epoch;
    assert_eq!(epoch, 1);
    let owner = her_log::owner(db, OWNER).await.unwrap();
    let root = keys::root();
    for n in 0..2 {
        store::write_hers(
            db,
            root,
            &owner,
            own,
            None,
            &format!("memory:m{n}x"),
            "memory.write",
            &json!({"at": 1, "text": "she likes walks", "kind": "fact"}),
        )
        .await
        .unwrap();
    }
    assert_eq!(logged::catch_up_all(db, root, &owner).await.unwrap(), 2);

    let mut rows = store::rows(db, OWNER, own).await.unwrap();
    rows.sort_by_key(|row| row.dseq);
    let epochs: Vec<Option<u64>> = rows.iter().map(|row| row.epoch).collect();
    assert_eq!(epochs, vec![Some(1), Some(1), Some(0), Some(0)]);
    assert!(rows[2].hlc < rows[0].hlc && rows[3].hlc < rows[0].hlc);

    let take = |rows: Vec<myriad_persona_log::Row>, epochs| {
        admit(AdmitInput {
            rows,
            now: her_log::now(),
            known: Default::default(),
            members: Default::default(),
            ledger: Some(Vec::new()),
            epochs,
            holder: owner.pc(),
            peer: PARTNER.to_string(),
        })
        .unwrap()
    };
    // The whole lot: every row taken.
    let whole = take(rows.clone(), Default::default());
    assert!(whole.refused.is_empty(), "{:?}", whole.refused);
    assert_eq!(whole.accepted.len(), 4);
    // Page by page by count, the floor the highest epoch held: refused.
    let first = take(rows[..2].to_vec(), Default::default());
    let second = take(rows[2..].to_vec(), first.epochs);
    assert_eq!(
        second
            .refused
            .iter()
            .map(|refusal| refusal.reason)
            .collect::<Vec<_>>(),
        vec![Reason::EpochBack; 2]
    );
    schema.drop().await;
}
