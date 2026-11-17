//! How their life stands as she reads it, on a database of its own: what
//! stands and what ended come with the day they were told, and what rests
//! only on what they took back is not said.

use sea_orm::ConnectionTrait;

use super::facts::GoesUnder;
use super::read_tests::ranked;
use super::talk_tests::{OTHER, answering, asked, memory_id, schema, settle, update};
use super::{context, recall};
use crate::services::agent::merope::clock;

fn running() -> Option<GoesUnder> {
    Some(GoesUnder::New {
        name: "跑步".into(),
        aliases: Vec::new(),
    })
}

/// What a kept fact starts on a topic, as the model would answer.
async fn stands(db: &sea_orm::DatabaseConnection, step: &str, fact: &str) -> String {
    answering(&[("memory_standing_step", step)]);
    settle(db, vec![update(fact, &[], running())]).await;
    assert_eq!(asked("memory_standing_step"), 1);
    memory_id(db, fact).await
}

async fn recollect(db: &sea_orm::DatabaseConnection, found: &[&str]) -> recall::Recollection {
    let now = crate::clock::now();
    let turn = context::load(db, OTHER, now).await.unwrap();
    let recalled = ranked(db, found).await;
    recall::recollect(&turn.shown, &turn.own, &recalled, None, now)
}

fn saying<'a>(lines: &'a [String], words: &str) -> Vec<&'a str> {
    lines
        .iter()
        .map(String::as_str)
        .filter(|line| line.contains(words))
        .collect()
}

#[tokio::test]
async fn what_stands_and_what_ended_are_read_with_the_day_they_were_told() {
    let Some(schema) = schema("memory_nodes_since").await else {
        return;
    };
    let db = &schema.db;
    let said = stands(
        db,
        r#"{"whose":"them","set":[],"end":[],"add":[{"topic":"跑步","text":"每周三晚上夜跑"}]}"#,
        "每周三晚上夜跑",
    )
    .await;
    // Told three weeks ago; its row was written today.
    let told = crate::clock::now() - chrono::Duration::days(21);
    db.execute_unprepared(&format!(
        "UPDATE agent_memories SET created_at = '{}' WHERE id = '{said}'",
        told.to_rfc3339()
    ))
    .await
    .unwrap();
    let day = |at: chrono::DateTime<chrono::Utc>| clock::date_of(&at).format("%m-%d").to_string();
    let (then, today) = (day(told), day(crate::clock::now()));

    // Every time, with nothing found: under its topic, since the day told.
    let resident = recollect(db, &[]).await.resident;
    assert_eq!(resident.len(), 1, "{resident:?}");
    assert!(
        resident[0].starts_with("跑步, since ") && resident[0].ends_with("：每周三晚上夜跑"),
        "{resident:?}"
    );
    assert!(
        resident[0].contains(&then) && !resident[0].contains(&today),
        "{resident:?}"
    );
    // Their words find the topic: said there once, as what stands, not
    // again as the entry it came from; and not in the list besides.
    let found = recollect(db, &[&said]).await;
    let lines = saying(&found.named, "每周三晚上夜跑");
    assert_eq!(lines.len(), 1, "{:?}", found.named);
    assert!(lines[0].starts_with("跑步: now, since "), "{lines:?}");
    assert!(
        lines[0].contains(&then) && !lines[0].contains('('),
        "{lines:?}"
    );
    assert!(found.resident.is_empty(), "{:?}", found.resident);

    // It ended: said before the entry from when it stood, since the day
    // that was told, and what ended it is not said again as an entry.
    let ending = stands(
        db,
        r#"{"whose":"them","set":[],"end":[1],"add":[]}"#,
        "膝盖伤了，不夜跑了",
    )
    .await;
    let found = recollect(db, &[&said]).await;
    let ended = saying(&found.named, "膝盖伤了");
    assert_eq!(ended.len(), 1, "{:?}", found.named);
    // It says which thing no longer stands, in the words it stood in.
    assert!(
        ended[0].starts_with("跑步: no longer: 每周三晚上夜跑, since ")
            && ended[0].ends_with(": 膝盖伤了，不夜跑了")
            && ended[0].contains(&today),
        "{ended:?}"
    );
    let at = |words: &str| found.named.iter().position(|line| line.contains(words));
    assert!(at("膝盖伤了") < at("] 每周三晚上夜跑"), "{:?}", found.named);
    assert!(at("] 每周三晚上夜跑").is_some(), "{:?}", found.named);
    assert!(
        saying(&found.named, "now, since").is_empty(),
        "{:?}",
        found.named
    );
    assert!(found.resident.is_empty(), "{:?}", found.resident);

    // What ended it deleted: theirs to have unsaid, so it is not said.
    db.execute_unprepared(&format!(
        "UPDATE agent_memories SET invalid_reason = 'deleted', invalid_at = now() WHERE id = '{ending}'"
    ))
    .await
    .unwrap();
    let found = recollect(db, &[&said]).await;
    assert!(
        saying(&found.named, "膝盖伤了").is_empty(),
        "{:?}",
        found.named
    );

    // Something new stands. The fact it rests on replaced by a later one:
    // it still stands. Let go with nothing in its place (「当作没说过」):
    // it is not said again, in the list or on its topic.
    let half = stands(
        db,
        r#"{"whose":"them","set":[],"end":[],"add":[{"topic":"跑步","text":"在准备半马"}]}"#,
        "在准备半马",
    )
    .await;
    // Told ten days ago.
    let ten = crate::clock::now() - chrono::Duration::days(10);
    db.execute_unprepared(&format!(
        "UPDATE agent_memories SET created_at = '{}' WHERE id = '{half}'",
        ten.to_rfc3339()
    ))
    .await
    .unwrap();
    let told = |resident: Vec<String>| {
        let lines = saying(&resident, "在准备半马");
        assert_eq!(lines.len(), 1, "{resident:?}");
        assert!(
            lines[0].contains(&day(ten)) && !lines[0].contains(&today),
            "the day it was told, held or replaced: {lines:?}"
        );
    };
    told(recollect(db, &[]).await.resident);
    db.execute_unprepared(&format!(
        "UPDATE agent_memories SET invalid_reason = 'superseded', invalid_at = now(), \
         superseded_by = '{said}' WHERE id = '{half}'"
    ))
    .await
    .unwrap();
    told(recollect(db, &[]).await.resident);
    // It rests on a second memory too, one she still holds: with the first
    // let go it is not said all the same, for its words may carry that.
    db.execute_unprepared(&format!(
        "UPDATE agent_node_heads h SET node = jsonb_set(h.node, \
           ARRAY['standing', item.key, 'value', 'because'], \
           '[\"memory:{half}\", \"memory:{said}\"]'::jsonb) \
         FROM agent_node_heads g, jsonb_each(g.node->'standing') AS item \
         WHERE g.user_id = h.user_id AND g.audience = h.audience AND g.node_id = h.node_id \
           AND item.value->'value'->>'text' = '在准备半马'"
    ))
    .await
    .unwrap();
    told(recollect(db, &[]).await.resident);
    db.execute_unprepared(&format!(
        "UPDATE agent_memories SET superseded_by = NULL WHERE id = '{half}'"
    ))
    .await
    .unwrap();
    let found = recollect(db, &[&said]).await;
    assert!(saying(&found.resident, "在准备半马").is_empty());
    assert!(
        saying(&found.named, "在准备半马").is_empty(),
        "{:?}",
        found.named
    );
    assert!(saying(&recollect(db, &[]).await.resident, "在准备半马").is_empty());
    schema.drop().await;
}

/// What she makes of them (a view, something not wrapped up) is not said
/// once any memory it rests on was taken back, though another it names is
/// still held: her words there were written from both.
#[tokio::test]
async fn what_she_makes_of_them_is_unsaid_with_what_they_took_back() {
    use super::read_tests::{OWNER, SEVEN, owned, seed};
    let Some(schema) = owned("memory_nodes_head_unsaid").await else {
        return;
    };
    let db = &schema.db;
    let (_, about) = seed(db).await;
    let talk = super::audience::private_talk(SEVEN, Some(OWNER));
    let node = myriad_nodes::node_id(&talk, &super::audience::person_key(&talk, SEVEN)).unwrap();
    let because = format!(r#"["memory:{}", "memory:{}"]"#, about[0], about[1]);
    let changed = db
        .execute_unprepared(&format!(
            "UPDATE agent_node_heads SET node = node || jsonb_build_object( \
               'views', jsonb_build_object('tired', jsonb_build_object('value', \
                 jsonb_build_object('text', 'VIEW 他最近很累', 'because', '{because}'::jsonb))), \
               'open', jsonb_build_object('reply', jsonb_build_object('value', \
                 jsonb_build_object('text', 'OPEN 还没回他那件事', 'because', '{because}'::jsonb)))) \
             WHERE node_id = '{node}'"
        ))
        .await
        .unwrap();
    assert_eq!(changed.rows_affected(), 1);
    let head = || async {
        let turn = context::load(db, SEVEN, crate::clock::now()).await.unwrap();
        turn.about_head_section().unwrap_or_default()
    };
    let said = head().await;
    assert!(
        said.contains("VIEW 他最近很累") && said.contains("OPEN 还没回他那件事"),
        "{said}"
    );
    // One of the two let go, with nothing in its place.
    db.execute_unprepared(&format!(
        "UPDATE agent_memories SET invalid_reason = 'superseded', invalid_at = now(), \
         superseded_by = NULL WHERE id = '{}'",
        about[1]
    ))
    .await
    .unwrap();
    let said = head().await;
    assert!(!said.contains("VIEW") && !said.contains("OPEN"), "{said}");
    schema.drop().await;
}

/// What ended is said with the words it stood in; and those words rested
/// on a memory too: taken back, the whole line goes, what stood and what
/// ended it, though the memory of what ended it is still held.
#[tokio::test]
async fn what_ended_is_unsaid_with_the_words_it_stood_in() {
    let Some(schema) = schema("memory_nodes_ended_was").await else {
        return;
    };
    let db = &schema.db;
    let stood = stands(
        db,
        r#"{"whose":"them","set":[],"end":[],"add":[{"topic":"跑步","text":"每周三晚上夜跑"}]}"#,
        "每周三晚上夜跑",
    )
    .await;
    let ending = stands(
        db,
        r#"{"whose":"them","set":[],"end":[1],"add":[]}"#,
        "膝盖伤了，不夜跑了",
    )
    .await;
    let found = recollect(db, &[&ending]).await;
    let ended = saying(&found.named, "no longer");
    assert_eq!(ended.len(), 1, "{:?}", found.named);
    assert!(
        ended[0].starts_with("跑步: no longer: 每周三晚上夜跑, since "),
        "{ended:?}"
    );
    // What it stood on let go, with nothing in its place.
    db.execute_unprepared(&format!(
        "UPDATE agent_memories SET invalid_reason = 'superseded', invalid_at = now(), \
         superseded_by = NULL WHERE id = '{stood}'"
    ))
    .await
    .unwrap();
    let found = recollect(db, &[&ending]).await;
    assert!(
        saying(&found.named, "no longer").is_empty(),
        "{:?}",
        found.named
    );
    assert!(
        saying(&found.named, "每周三晚上夜跑").is_empty(),
        "{:?}",
        found.named
    );
    schema.drop().await;
}
