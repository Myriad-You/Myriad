//! Her nodes in her log ([`super::logged`]).

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::{Value, json};

use super::forget::{self, Forgets};
use super::{Author, RowIn, audience, logged, partner, write};
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
