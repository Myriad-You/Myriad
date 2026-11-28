//! What she is shown when she speaks comes from her nodes. On a database
//! of its own, with what she keeps in every audience
//! (`memory_nodes::read_tests`).

use serde_json::json;

use super::super::memory_nodes::group_read_tests::seed_group;
use super::super::memory_nodes::read_tests::{GROUP, OWNER, SEVEN, owned, put, seed};
use super::super::memory_nodes::{Author, RowIn, audience, read, write};
use super::*;
use crate::services::agent::memory::unified::Audience;

/// Her prompt for speaking up to account 7 in private, without the minute
/// it is (which moves while the test runs).
async fn prompt(db: &sea_orm::DatabaseConnection) -> Vec<String> {
    let present = Audience::private(SEVEN);
    let sections =
        speaking_prompt_from_db(db, SEVEN, Turn::Event("聊起阳台上的植物"), &present, None).await;
    assert!(put_together_in_part_order(&sections), "{sections:#?}");
    texts(sections)
        .into_iter()
        .filter(|section| !section.starts_with("## Now\n"))
        .collect()
}

#[tokio::test]
async fn a_private_talk_reads_her_nodes() {
    let Some(schema) = owned("memory_nodes_read_private").await else {
        return;
    };
    let db = &schema.db;
    // No model answers: nothing here may ask one.
    crate::services::channel_group::g0::enter(crate::services::channel_group::g0::World::new(0.5));
    seed(db).await;
    let on = prompt(db).await.join("\n\n");
    crate::services::channel_group::g0::leave();
    // What they are to her and the rest, as ever.
    for kept in [
        "吐槽工作",
        "随便一点",
        "小笼包",
        "像猫",
        "PRIVATE-SORE",
        "GROUP-SORE",
    ] {
        assert!(on.contains(kept), "{kept}:\n{on}");
    }
    // What she keeps of a topic comes under its name, and nothing of
    // another account's talk, a group's facts, her own audience, or what
    // was let go.
    assert!(on.contains("植物: "), "{on}");
    for never in ["GROUP-FACT", "OTHER-ACCOUNT", "OWN-TOPIC", "OLD-JOB"] {
        assert!(!on.contains(never), "{never}:\n{on}");
    }
    schema.drop().await;
}

/// What they are to her and how she comes across with them are rows her
/// nightly review keeps; an upkeep that put other words on the views they
/// hang as does not take them out of what she is shown, nor is it said for
/// them.
#[tokio::test]
async fn what_they_are_to_her_stays_whatever_upkeep_made_of_its_view() {
    let Some(schema) = owned("speaking_us_after_upkeep").await else {
        return;
    };
    let db = &schema.db;
    crate::services::channel_group::g0::enter(crate::services::channel_group::g0::World::new(0.5));
    seed(db).await;
    let titles = ["## What they are to you", "## How they take you"];
    let first = prompt(db).await;
    let before = titled(&first, &titles);
    assert_eq!(before.len(), 2, "{first:#?}");

    let talk = audience::private_talk(SEVEN, Some(OWNER));
    let node = myriad_nodes::node_id(&talk, &format!("person:{SEVEN}")).unwrap();
    let head = read::node(db, SEVEN, &talk, &node).await.unwrap().unwrap();
    let rested = head["entries"][0]["row"].as_str().unwrap().to_string();
    let rewritten: Vec<RowIn> = ["us", "lands"]
        .into_iter()
        .map(|view| RowIn {
            id: format!("u0upkeep{view}"),
            node: node.clone(),
            kind: "node.view_changed".into(),
            body: json!({
                "version": 1, "sync": "shared", "saidBy": ["her"],
                "base": head["views"][view]["base"], "view": view,
                "text": "UPKEEP-WORDS", "because": [format!("entry:{rested}")],
            }),
            author: Author::Her,
        })
        .collect();
    let pen = crate::services::her_log::pen::think().expect("her mind is here in tests");
    write::write(db, SEVEN, &talk, &rewritten, Some(&pen))
        .await
        .unwrap();
    drop(pen);
    let head = read::node(db, SEVEN, &talk, &node).await.unwrap().unwrap();
    for view in ["us", "lands"] {
        assert_eq!(head["views"][view]["value"]["text"], "UPKEEP-WORDS");
    }

    let after = prompt(db).await;
    crate::services::channel_group::g0::leave();
    assert_eq!(titled(&after, &titles), before);
    assert!(!after.join("\n").contains("UPKEEP-WORDS"), "{after:#?}");
    schema.drop().await;
}

/// How she was a moment ago with them already says how she feels toward
/// them, so her mood is not said again; her day is facts of her own and
/// stands beside it.
#[tokio::test]
async fn her_day_stands_beside_how_she_was_a_moment_ago() {
    let Some(schema) = owned("speaking_day_beside_inner").await else {
        return;
    };
    let db = &schema.db;
    crate::services::channel_group::g0::enter(crate::services::channel_group::g0::World::new(0.5));
    seed(db).await;
    let present = Audience::private(SEVEN);
    super::super::inner::tests::hold(SEVEN, &present, "刚才聊得挺开心");
    let parts =
        speaking_prompt_from_db(db, SEVEN, Turn::Chat("今天过得怎么样"), &present, None).await;
    crate::services::channel_group::g0::leave();
    super::super::inner::tests::let_go(SEVEN, &present);
    assert!(put_together_in_part_order(&parts), "{parts:#?}");
    let sections = texts(parts);
    let titled_as = |title: &str| sections.iter().filter(|s| s.starts_with(title)).count();
    assert_eq!(
        titled_as("## Inside you a moment ago\n"),
        1,
        "{sections:#?}"
    );
    assert_eq!(titled_as("## Your day\n"), 1, "{sections:#?}");
    assert_eq!(
        titled_as("## How you feel toward them\n"),
        0,
        "{sections:#?}"
    );
    schema.drop().await;
}

/// Her prompt for speaking up to account 7 in the group, as [`prompt`].
async fn prompt_in_group(db: &sea_orm::DatabaseConnection) -> Vec<String> {
    let present = Audience::group(GROUP, SEVEN);
    let sections = speaking_prompt_from_db(
        db,
        SEVEN,
        Turn::Event("阿绿又在玩游戏"),
        &present,
        Some("七"),
    )
    .await;
    assert!(put_together_in_part_order(&sections), "{sections:#?}");
    texts(sections)
        .into_iter()
        .filter(|section| !section.starts_with("## Now\n"))
        .collect()
}

fn titled<'a>(sections: &'a [String], titles: &[&str]) -> Vec<&'a String> {
    sections
        .iter()
        .filter(|section| titles.iter().any(|title| section.starts_with(title)))
        .collect()
}

#[tokio::test]
async fn a_group_reads_its_own_nodes() {
    let Some(schema) = owned("memory_nodes_read_group").await else {
        return;
    };
    let db = &schema.db;
    crate::services::channel_group::g0::enter(crate::services::channel_group::g0::World::new(0.5));
    seed_group(db).await;
    let on = prompt_in_group(db).await;
    crate::services::channel_group::g0::leave();
    // What crosses from her other groups, as ever.
    let elsewhere = ["## Them, in other groups", "## From your other groups"];
    assert!(!titled(&on, &elsewhere).is_empty(), "{on:#?}");
    // The group's own, read from its nodes.
    let own = [
        "## In this group",
        "## Days in this group",
        "## How this group takes you",
        "## What still stings",
    ];
    assert_eq!(titled(&on, &own).len(), 4, "{on:#?}");
    let on = on.join("\n\n");
    assert!(on.contains("OTHER-GROUP-BIT"), "{on}");
    assert!(on.contains("游戏: "), "{on}");
    for never in [
        "PRIVATE-SORE",
        "在杭州做设计",
        "龟背竹",
        "吐槽工作",
        "OTHER-ACCOUNT",
        "OWN-TOPIC",
    ] {
        assert!(!on.contains(never), "{never}:\n{on}");
    }
    schema.drop().await;
}

/// What they ask by another name a node goes by brings that node to mind
/// as she answers, though the ranking finds nothing of it in their words.
#[tokio::test]
async fn a_node_they_name_comes_to_mind_as_she_answers() {
    let Some(schema) = owned("speaking_node_by_name").await else {
        return;
    };
    let db = &schema.db;
    crate::services::channel_group::g0::enter(crate::services::channel_group::g0::World::new(0.5));
    seed(db).await;
    put(
        db,
        SEVEN,
        &audience::private_talk(SEVEN, Some(OWNER)),
        SEVEN,
        Audience::private(SEVEN),
        myriad_nodes::SaidBy::Partner,
        myriad_nodes::entries::Placement::Start {
            name: "乐器".into(),
            aliases: vec!["木吉他".into()],
        },
        &["NAMED-FACT 周末总练一小时指弹"],
    )
    .await;
    let present = Audience::private(SEVEN);
    let parts = speaking_prompt_from_db(
        db,
        SEVEN,
        Turn::Chat("我的木吉他是不是该换弦了"),
        &present,
        None,
    )
    .await;
    crate::services::channel_group::g0::leave();
    let on = texts(parts).join("\n");
    let named = |line: &str| line.contains("乐器: ") && line.contains("NAMED-FACT");
    assert!(on.lines().any(named), "{on}");
    schema.drop().await;
}

/// Her sections come out in the order of their parts (but how their words
/// just landed, which a private chat moves last): laid out by part, a
/// private chat's prompt is what putting them together in turn made it.
fn put_together_in_part_order(parts: &Parts) -> bool {
    parts
        .iter()
        .filter(|(part, _)| *part != Part::JustNow)
        .collect::<Vec<_>>()
        .windows(2)
        .all(|pair| pair[0].0 <= pair[1].0)
}
