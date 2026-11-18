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

async fn memory(db: &sea_orm::DatabaseConnection, id: &str, set: &str) {
    db.execute_unprepared(&format!(
        "UPDATE agent_memories SET {set} WHERE id = '{id}'"
    ))
    .await
    .unwrap();
}

/// A version is written with the one before it in view, so its words may
/// carry what the earlier one rested on though it names only its own
/// memory: taken back, the earlier one's memory takes the item with it,
/// standing or ended, and so does one replaced by a memory taken back.
/// The words an ended item stood in are those its ending was written
/// over; that version forgotten, the line says only that it ended.
#[tokio::test]
async fn an_earlier_versions_memory_taken_back_takes_the_item() {
    let Some(schema) = schema("memory_nodes_versions").await else {
        return;
    };
    let db = &schema.db;
    let first = stands(
        db,
        r#"{"whose":"them","set":[],"end":[],"add":[{"topic":"跑步","text":"每周三晚上夜跑"}]}"#,
        "每周三晚上夜跑",
    )
    .await;
    stands(
        db,
        r#"{"whose":"them","set":[{"item":1,"text":"每周五晚上夜跑"}],"end":[],"add":[]}"#,
        "夜跑改到周五了",
    )
    .await;
    let now = |resident: Vec<String>| saying(&resident, "每周五晚上夜跑").len();
    assert_eq!(now(recollect(db, &[]).await.resident), 1);
    // The first version's memory let go: the second, written over it, is
    // not said either. Let go on another end, as of long ago: not swept
    // here (`forget::before_reading`), only not said, so it can come back.
    let let_go = "invalid_reason = 'superseded', invalid_at = now(), superseded_by = NULL, \
                  updated_at = '2000-01-01'";
    let held = "invalid_reason = NULL, invalid_at = NULL, superseded_by = NULL";
    memory(db, &first, let_go).await;
    assert_eq!(now(recollect(db, &[]).await.resident), 0);
    memory(db, &first, held).await;
    assert_eq!(now(recollect(db, &[]).await.resident), 1);

    // Ended: the line names the version the ending was written over.
    let ending = stands(
        db,
        r#"{"whose":"them","set":[],"end":[1],"add":[]}"#,
        "膝盖伤了，不夜跑了",
    )
    .await;
    let ended = |named: Vec<String>| {
        saying(&named, "no longer")
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>()
    };
    let lines = ended(recollect(db, &[&ending]).await.named);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0].starts_with("跑步: no longer: 每周五晚上夜跑, since "),
        "{lines:?}"
    );
    // The first version's memory replaced by another (one no version of
    // the item names) that was then taken back: taken back with it, and the
    // whole line goes.
    db.execute_unprepared(&format!(
        "INSERT INTO agent_memories SELECT * FROM jsonb_populate_record(NULL::agent_memories, \
           (SELECT to_jsonb(m) || '{{\"id\": \"mem_replacer\", \"content\": \"其实是周四\"}}'::jsonb \
            FROM agent_memories m WHERE m.id = '{first}'))"
    ))
    .await
    .unwrap();
    memory(
        db,
        &first,
        "invalid_reason = 'superseded', invalid_at = now(), superseded_by = 'mem_replacer'",
    )
    .await;
    assert_eq!(ended(recollect(db, &[&ending]).await.named).len(), 1);
    memory(
        db,
        "mem_replacer",
        "invalid_reason = 'deleted', invalid_at = now(), updated_at = '2000-01-01'",
    )
    .await;
    assert!(ended(recollect(db, &[&ending]).await.named).is_empty());
    memory(db, &first, held).await;
    assert_eq!(ended(recollect(db, &[&ending]).await.named).len(), 1);

    // The version it stood in forgotten (a stub, no body): the line says
    // only that it is no longer as it stood.
    let stubbed = db
        .execute_unprepared(
            "UPDATE agent_node_rows SET body = '' WHERE kind = 'node.standing' \
             AND body <> '' AND body::jsonb->>'text' = '每周五晚上夜跑'",
        )
        .await
        .unwrap();
    assert_eq!(stubbed.rows_affected(), 1);
    let lines = ended(recollect(db, &[&ending]).await.named);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0].starts_with("跑步: no longer as it stood here, since "),
        "{lines:?}"
    );
    schema.drop().await;
}

/// In a group the same rules hold. Nothing writes how a life stands into a
/// group's audience today (`facts::settled` does so in a private talk
/// only), so what stands there is put in by hand, as another end might
/// send it: it is read there as in private, with since when; it is not
/// read in their private talk; and it goes with the memory it rests on.
#[tokio::test]
async fn in_a_group_what_stands_is_read_and_unsaid_the_same() {
    use super::group_read_tests::seed_group;
    use super::read_tests::{GROUP, SEVEN, owned};
    let Some(schema) = owned("memory_nodes_since_group").await else {
        return;
    };
    let db = &schema.db;
    let (theirs, _) = seed_group(db).await;
    let because = format!(r#"["memory:{theirs}"]"#);
    let changed = db
        .execute_unprepared(&format!(
            "UPDATE agent_node_heads SET node = node || jsonb_build_object('standing', \
               jsonb_build_object( \
                 'them-aaaaaaaaaa', jsonb_build_object('at', 1794000000000000, 'author', 'her', \
                   'value', jsonb_build_object('text', 'GROUP-NOW 每周六开黑', \
                     'because', '{because}'::jsonb)), \
                 'them-bbbbbbbbbb', jsonb_build_object('at', 1794000000000001, 'author', 'her', \
                   'value', jsonb_build_object('text', 'GROUP-END 公会解散了', 'ended', true, \
                     'because', '{because}'::jsonb)))) \
             WHERE audience = 'group:qq:5' AND node::text LIKE '%{theirs}%'"
        ))
        .await
        .unwrap();
    assert_eq!(changed.rows_affected(), 1);
    let in_group = || async {
        let turn = super::group_context::load(db, SEVEN, GROUP)
            .await
            .unwrap()
            .expect("a site owner keeps it");
        let recalled = ranked(db, &[&theirs]).await;
        let here = turn.group.as_ref().map(|group| &group.here);
        let now = crate::clock::now();
        recall::recollect_here(&turn.shown, &turn.own, here, &recalled, None, now)
    };
    let found = in_group().await;
    let now = saying(&found.named, "GROUP-NOW");
    assert_eq!(now.len(), 1, "{:?}", found.named);
    assert!(now[0].contains("now, since "), "{now:?}");
    let ended = saying(&found.named, "GROUP-END");
    assert_eq!(ended.len(), 1, "{:?}", found.named);
    assert!(
        ended[0].contains("no longer as it stood here, since "),
        "{ended:?}"
    );
    // A group has no list of how their life stands.
    assert!(found.resident.is_empty(), "{:?}", found.resident);
    // Not in their private talk: another audience.
    let private = recollect_as(db, SEVEN, &[]).await;
    let all = [private.named, private.resident].concat();
    assert!(saying(&all, "GROUP-").is_empty(), "{all:?}");
    // The memory it rests on let go: neither is said in the group.
    memory(
        db,
        &theirs,
        "invalid_reason = 'superseded', invalid_at = now(), superseded_by = NULL",
    )
    .await;
    let found = in_group().await;
    for gone in ["GROUP-NOW", "GROUP-END"] {
        assert!(saying(&found.named, gone).is_empty(), "{:?}", found.named);
    }
    // What the group knows of them otherwise is still read.
    assert!(
        !saying(&found.named, "GROUP-FACT").is_empty(),
        "{:?}",
        found.named
    );
    schema.drop().await;
}

async fn recollect_as(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    found: &[&str],
) -> recall::Recollection {
    let now = crate::clock::now();
    let turn = context::load(db, user_id, now).await.unwrap();
    let recalled = ranked(db, found).await;
    recall::recollect(&turn.shown, &turn.own, &recalled, None, now)
}

/// A fact put to what stands and leaving it standing (the core's
/// `stillSo`, noted in `agent_node_kept`) says until when it was seen so:
/// the line carries since when and that day, not the day told besides.
/// That fact taken back, the line is as it was before it.
#[tokio::test]
async fn what_was_seen_to_still_hold_says_until_when() {
    let Some(schema) = schema("memory_nodes_still_so").await else {
        return;
    };
    let db = &schema.db;
    let said = stands(
        db,
        r#"{"whose":"them","set":[],"end":[],"add":[{"topic":"跑步","text":"每周三晚上夜跑"}]}"#,
        "每周三晚上夜跑",
    )
    .await;
    let seen = stands(
        db,
        r#"{"whose":"them","set":[],"end":[],"add":[]}"#,
        "跑步时听播客",
    )
    .await;
    let ago = |days: i64| crate::clock::now() - chrono::Duration::days(days);
    let (told, again) = (ago(21), ago(7));
    for (id, at) in [(&said, told), (&seen, again)] {
        memory(db, id, &format!("created_at = '{}'", at.to_rfc3339())).await;
    }
    // As the line writes a day: the year only when it is not this one.
    let day = |at: chrono::DateTime<chrono::Utc>| {
        use chrono::Datelike;
        let (date, year) = (
            clock::date_of(&at),
            clock::date_of(&crate::clock::now()).year(),
        );
        let form = if date.year() == year {
            "%m-%d"
        } else {
            "%Y-%m-%d"
        };
        date.format(form).to_string()
    };
    let line = |resident: Vec<String>| {
        let lines = saying(&resident, "每周三晚上夜跑");
        assert_eq!(lines.len(), 1, "{resident:?}");
        lines[0].to_string()
    };
    let start = |at| format!("跑步, since {}", day(at));
    let resident = line(recollect(db, &[]).await.resident);
    assert!(
        resident.starts_with(&start(told))
            && resident.contains(&format!(", still so on {}：", day(again)))
            && !resident.contains("told"),
        "{resident}"
    );

    // What showed it taken back (let go on another end, not swept here):
    // as it was, since the day told.
    let let_go = "invalid_reason = 'superseded', invalid_at = now(), superseded_by = NULL, \
                  updated_at = '2000-01-01'";
    memory(db, &seen, let_go).await;
    let resident = line(recollect(db, &[]).await.resident);
    assert!(
        resident.starts_with(&start(told)) && !resident.contains("still so"),
        "{resident}"
    );

    // Ended, it holds no longer: a note of it left behind (one written as
    // it ended, before the ending was in) says nothing on its line.
    let held = "invalid_reason = NULL, invalid_at = NULL, superseded_by = NULL";
    memory(db, &seen, held).await;
    let left = db
        .query_all_raw(sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT audience, node_id, item FROM agent_node_kept",
        ))
        .await
        .unwrap();
    assert_eq!(left.len(), 1);
    let ending = stands(
        db,
        r#"{"whose":"them","set":[],"end":[1],"add":[]}"#,
        "膝盖伤了，不夜跑了",
    )
    .await;
    // Told of its ending before the note's memory was learned: were the
    // note read, its day would be the later.
    memory(
        db,
        &ending,
        &format!("created_at = '{}'", ago(14).to_rfc3339()),
    )
    .await;
    let column = |name: &str| left[0].try_get::<String>("", name).unwrap();
    db.execute_unprepared(&format!(
        "INSERT INTO agent_node_kept (user_id, audience, node_id, item, by_memory, at) \
         VALUES ({OTHER}, '{}', '{}', '{}', '{seen}', now())",
        column("audience"),
        column("node_id"),
        column("item"),
    ))
    .await
    .unwrap();
    let found = recollect(db, &[&ending]).await;
    let ended = saying(&found.named, "no longer");
    assert_eq!(ended.len(), 1, "{:?}", found.named);
    assert!(!ended[0].contains("still so"), "{ended:?}");
    schema.drop().await;
}
