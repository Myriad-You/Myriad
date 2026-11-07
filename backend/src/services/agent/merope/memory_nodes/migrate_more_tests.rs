//! Moving her memory ([`super::migrate`]): a dry run, one run at a time, the cap on asking the model,
//! and an old topic her nodes already keep, as `migrate_tests`.

use std::collections::BTreeSet;

use serde_json::{Value, json};

use super::migrate::{self, Options, Status};
use super::migrate_tests::{
    OPERATION, OWNER, PLANTS, SEVEN, memories_on, memory, moved, old_tables, old_topic, owned,
    real, seed, sql,
};
use super::placements::{self, How};
use super::talk_tests::{answering, asked, count};
use super::{audience, facts, read};
use crate::services::her_log::pen;

/// Her answer putting the first `n` facts on 「关于你」, `padding` spaces
/// long besides.
fn about_them(n: usize, padding: usize) -> String {
    let facts: Vec<Value> = (1..=n)
        .map(|at| json!({"fact": at, "topic": {"on": "", "name": "", "aliases": []}}))
        .collect();
    let body = json!({ "facts": facts }).to_string();
    format!("{}{}}}", &body[..body.len() - 1], " ".repeat(padding))
}

#[tokio::test]
async fn asking_the_model_stops_past_twice_its_estimate_and_the_next_run_goes_on() {
    let Some(schema) = owned("memory_nodes_migrate_cap").await else {
        return;
    };
    let db = &schema.db;
    let batch = myriad_merope::topics::SORT_BATCH;
    for at in 0..=batch {
        let id = format!("f{at:02}");
        memory(
            db,
            &id,
            Some(SEVEN),
            ("chat", "private", "user"),
            &format!("第{at}件事"),
            None,
            100 - at as i64,
        )
        .await;
    }
    // The first answer is far longer than any answer is counted for.
    answering(&[(OPERATION, &about_them(batch, 400_000))]);
    let stopped = moved(db, real(true)).await;
    assert_eq!(asked(OPERATION), 1, "no ask after the cap");
    assert_eq!(stopped.status, Status::Stopped);
    assert_eq!(stopped.estimate.asks, 2);
    assert!(stopped.spent.tokens > 2 * stopped.estimate.tokens());
    let note = stopped.note.clone().unwrap();
    assert!(
        note.contains("more than 2×") && note.contains("next run"),
        "{note}"
    );
    assert_eq!((stopped.counts.asks, stopped.counts.sorted), (1, batch));
    let last = format!("f{batch:02}");
    assert!(placements::placement(db, &last).await.unwrap().is_none());
    assert_eq!(migrate::runs(db, 1).await.unwrap()[0]["status"], "stopped");

    // Taken up again: only what is still on no node.
    answering(&[(OPERATION, &about_them(1, 0))]);
    let resumed = moved(db, real(true)).await;
    assert_eq!(asked(OPERATION), 1);
    assert_eq!((resumed.status, resumed.estimate.asks), (Status::Done, 1));
    assert_eq!(
        placements::placement(db, &last).await.unwrap().unwrap().how,
        How::About
    );
    schema.drop().await;
}

#[tokio::test]
async fn an_old_topic_her_nodes_already_keep_by_name_is_that_node() {
    let Some(schema) = owned("memory_nodes_migrate_join").await else {
        return;
    };
    let db = &schema.db;
    let mine = audience::owners_talk();
    let private = ("chat", "private", "user");
    memory(db, "m1", Some(OWNER), private, "养了一盆绿萝", None, 2).await;
    memory(db, "m9", Some(OWNER), private, "想买一盆龟背竹", None, 1).await;
    old_topic(db, OWNER, Some((PLANTS, "plants")), &["m1", "m9"]).await;
    // Going over a stretch while the switch wrote started it by its name.
    let fact = facts::Fact {
        memory: "m9".into(),
        text: "想买一盆龟背竹".into(),
        said_by: myriad_nodes::SaidBy::Partner,
        placement: myriad_nodes::entries::Placement::Start {
            name: "Plants".into(),
            aliases: Vec::new(),
        },
        standing: Default::default(),
    };
    let thought = pen::think().unwrap();
    assert_eq!(
        facts::put(db, OWNER, &mine, &[fact], &thought)
            .await
            .unwrap(),
        1
    );
    drop(thought);
    let named = myriad_nodes::entries::topic_key(&mine, "plants").unwrap();

    let report = moved(db, real(false)).await;
    assert_eq!(
        (report.counts.topics_joined, report.counts.topics_started),
        (1, 0)
    );
    assert_eq!((report.counts.placed, report.counts.placed_before), (1, 1));
    assert_eq!(
        memories_on(db, OWNER, &mine, &named).await,
        BTreeSet::from(["m1".to_string(), "m9".to_string()])
    );
    assert!(
        read::node(
            db,
            OWNER,
            &mine,
            &myriad_nodes::node_id(&mine, PLANTS).unwrap()
        )
        .await
        .unwrap()
        .is_none(),
        "not a second one"
    );
    let check = report.counts.check.unwrap();
    assert!(check.whole(), "{check:?}");
    assert_eq!((check.expected, check.matched, check.extra), (2, 2, 0));
    schema.drop().await;
}

#[tokio::test]
async fn a_dry_run_counts_and_estimates_and_writes_nothing() {
    let Some(schema) = owned("memory_nodes_migrate_dry").await else {
        return;
    };
    let db = &schema.db;
    seed(db).await;
    let before = old_tables(db).await;
    answering(&[]);
    let dry = moved(
        db,
        Options {
            dry_run: true,
            replay: true,
        },
    )
    .await;
    assert_eq!(asked(OPERATION), 0);
    assert_eq!((dry.status, dry.dry_run), (Status::Done, true));
    assert_eq!(
        (dry.counts.topics, dry.counts.placements, dry.counts.sources),
        (2, 3, 4)
    );
    assert_eq!((dry.counts.child_notes, dry.counts.threads), (1, 2));
    assert_eq!(
        (
            dry.estimate.facts,
            dry.estimate.audiences,
            dry.estimate.asks
        ),
        (3, 3, 3)
    );
    assert!(dry.estimate.input_tokens > 0 && dry.estimate.usd > 0.0);
    for table in [
        "agent_node_rows",
        "agent_node_heads",
        "agent_node_placements",
    ] {
        assert_eq!(
            count(db, &format!("SELECT COUNT(*) AS n FROM {table}")).await,
            0,
            "{table}"
        );
    }
    assert_eq!(old_tables(db).await, before);
    assert_eq!(migrate::runs(db, 10).await.unwrap()[0]["dryRun"], true);

    // Without the part that asks the model: nothing asked, nothing sorted.
    answering(&[]);
    let moved_only = moved(db, real(false)).await;
    assert_eq!(asked(OPERATION), 0);
    assert_eq!((moved_only.counts.placed, moved_only.counts.asks), (3, 0));
    assert!(placements::placement(db, "m5").await.unwrap().is_none());
    schema.drop().await;
}

#[tokio::test]
async fn one_run_at_a_time() {
    let Some(schema) = owned("memory_nodes_migrate_one").await else {
        return;
    };
    let db = &schema.db;
    sql(db, "INSERT INTO agent_node_migrations (dry_run, status, started_at) VALUES (false, 'running', now())").await;
    assert!(migrate::run(db, real(false)).await.unwrap().is_none());
    // One left running by a process long gone is not waited for.
    sql(
        db,
        "UPDATE agent_node_migrations SET started_at = now() - interval '1 day'",
    )
    .await;
    let report = moved(db, real(false)).await;
    assert_eq!(report.status, Status::Done);
    schema.drop().await;
}

/// After a run that asks the model, each audience is counted: what belongs
/// on a node is on one or put nowhere (a fact the answer left out), what
/// does not is counted by source and kind; what is still undecided stops
/// the run, saying where.
#[tokio::test]
async fn each_audience_is_counted_and_what_is_undecided_stops_the_run() {
    let Some(schema) = owned("memory_nodes_migrate_census").await else {
        return;
    };
    let db = &schema.db;
    memory(
        db,
        "c1",
        Some(SEVEN),
        ("chat", "private", "user"),
        "养了一只猫",
        None,
        3,
    )
    .await;
    memory(
        db,
        "c2",
        Some(SEVEN),
        ("chat", "private", "user"),
        "今天下雨",
        None,
        2,
    )
    .await;
    memory(
        db,
        "d1",
        Some(SEVEN),
        ("day_with", "private", "user"),
        "一起看了电影",
        None,
        1,
    )
    .await;
    answering(&[(OPERATION, &about_them(1, 0))]);
    let done = moved(db, real(true)).await;
    assert_eq!(done.status, Status::Done, "{:?}", done.note);
    let census = done.counts.census.clone().unwrap();
    assert_eq!(census.undecided, 0);
    let private = &census.audiences[0];
    assert_eq!(private.audience, format!("private:{SEVEN}"));
    assert_eq!(
        (
            private.held,
            private.belong,
            private.on_node,
            private.nowhere
        ),
        (3, 2, 1, 1),
        "the left-out fact is put nowhere"
    );
    assert_eq!(private.not_sorted.get("day_with/fact"), Some(&1));

    // An answer that cannot be read leaves what it was asked undecided.
    memory(
        db,
        "c3",
        Some(SEVEN),
        ("chat", "private", "user"),
        "换了工作",
        None,
        0,
    )
    .await;
    answering(&[(OPERATION, "not an answer")]);
    let stopped = moved(db, real(true)).await;
    assert_eq!(stopped.status, Status::Stopped);
    let note = stopped.note.clone().unwrap();
    assert!(note.contains(&format!("private:{SEVEN}: 1")), "{note}");
    assert_eq!(stopped.counts.census.unwrap().undecided, 1);
    schema.drop().await;
}
