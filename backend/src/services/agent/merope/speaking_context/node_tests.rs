//! What she is shown when she speaks comes from her nodes. On a database
//! of its own, with what she keeps in every audience
//! (`memory_nodes::read_tests`).

use super::super::memory_nodes::group_read_tests::seed_group;
use super::super::memory_nodes::read_tests::{GROUP, SEVEN, owned, seed};
use super::*;
use crate::services::agent::memory::unified::Audience;

/// Her prompt for speaking up to account 7 in private, without the minute
/// it is (which moves while the test runs).
async fn prompt(db: &sea_orm::DatabaseConnection) -> Vec<String> {
    let present = Audience::private(SEVEN);
    let sections =
        speaking_prompt_from_db(db, SEVEN, Turn::Event("聊起阳台上的植物"), &present, None).await;
    sections
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
    for kept in ["吐槽工作", "小笼包", "像猫", "PRIVATE-SORE", "GROUP-SORE"] {
        assert!(on.contains(kept), "{kept}:\n{on}");
    }
    // What she keeps of a topic comes under its name, and nothing of
    // another account's talk, a group's facts, her own audience, or what
    // was let go.
    assert!(on.contains("植物: "), "{on}");
    for never in ["GROUP-FACT", "OTHER-ACCOUNT", "OWN-TOPIC", "OLD-JOB"] {
        assert!(!on.contains(never), "{never}:\n{on}");
    }
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
    sections
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
}
