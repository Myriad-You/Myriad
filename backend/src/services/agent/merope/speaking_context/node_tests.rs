//! The one key back: while her nodes are off or only written (`shadow`),
//! what she is shown when she speaks is what it always was, to the byte,
//! and reads nothing of them; on, it comes from them. On a database of its
//! own, with what she keeps in every audience (`memory_nodes::read_tests`).

use sea_orm::ConnectionTrait;

use super::super::memory_nodes::group_read_tests::seed_group;
use super::super::memory_nodes::read_tests::{GROUP, SEVEN, owned, seed};
use super::super::memory_nodes::switch::{Mode, set_for_test};
use super::*;
use crate::services::agent::memory::unified::Audience;

/// Her prompt for speaking up to account 7 in private under `mode`,
/// without the minute it is (which moves while the test runs).
async fn prompt(db: &sea_orm::DatabaseConnection, mode: Mode) -> Vec<String> {
    set_for_test(Some(mode));
    let present = Audience::private(SEVEN);
    let sections =
        speaking_prompt_from_db(db, SEVEN, Turn::Event("聊起阳台上的植物"), &present, None).await;
    set_for_test(None);
    sections
        .into_iter()
        .filter(|section| !section.starts_with("## Now\n"))
        .collect()
}

#[tokio::test]
async fn off_and_shadow_read_as_before_and_on_reads_her_nodes() {
    let Some(schema) = owned("memory_nodes_read_rollback").await else {
        return;
    };
    let db = &schema.db;
    // No model answers: nothing here may ask one.
    crate::services::channel_group::g0::enter(crate::services::channel_group::g0::World::new(0.5));
    seed(db).await;
    let off = prompt(db, Mode::Off).await;
    let shadow = prompt(db, Mode::Shadow).await;
    let on = prompt(db, Mode::On).await;
    // Without any of her nodes, off and shadow are the same again.
    db.execute_unprepared("DELETE FROM agent_node_heads; DELETE FROM agent_node_rows")
        .await
        .unwrap();
    let shadow_without = prompt(db, Mode::Shadow).await;
    let off_without = prompt(db, Mode::Off).await;
    crate::services::channel_group::g0::leave();
    assert_eq!(off, shadow);
    assert_eq!(off, shadow_without);
    assert_eq!(off, off_without);
    let off = off.join("\n\n");
    let on = on.join("\n\n");
    // The old path: what they are to her and the rest, from the rows.
    for both in ["吐槽工作", "小笼包", "像猫", "PRIVATE-SORE", "GROUP-SORE"] {
        assert!(off.contains(both), "{both} off:\n{off}");
        assert!(on.contains(both), "{both} on:\n{on}");
    }
    // On, what she keeps of a topic comes under its name, and nothing of
    // another account's talk, a group's facts, her own audience, or what
    // was let go.
    assert!(on.contains("植物: "), "{on}");
    assert!(!off.contains("植物: "), "{off}");
    for never in ["GROUP-FACT", "OTHER-ACCOUNT", "OWN-TOPIC", "OLD-JOB"] {
        assert!(!on.contains(never), "{never}:\n{on}");
    }
}

/// Her prompt for speaking up to account 7 in the group under `mode`, as
/// [`prompt`].
async fn prompt_in_group(db: &sea_orm::DatabaseConnection, mode: Mode) -> Vec<String> {
    set_for_test(Some(mode));
    let present = Audience::group(GROUP, SEVEN);
    let sections = speaking_prompt_from_db(
        db,
        SEVEN,
        Turn::Event("阿绿又在玩游戏"),
        &present,
        Some("七"),
    )
    .await;
    set_for_test(None);
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
async fn a_group_reads_as_before_off_and_its_own_nodes_on() {
    let Some(schema) = owned("memory_nodes_read_group_rollback").await else {
        return;
    };
    let db = &schema.db;
    crate::services::channel_group::g0::enter(crate::services::channel_group::g0::World::new(0.5));
    seed_group(db).await;
    let off = prompt_in_group(db, Mode::Off).await;
    let shadow = prompt_in_group(db, Mode::Shadow).await;
    let on = prompt_in_group(db, Mode::On).await;
    db.execute_unprepared("DELETE FROM agent_node_heads; DELETE FROM agent_node_rows")
        .await
        .unwrap();
    let shadow_without = prompt_in_group(db, Mode::Shadow).await;
    let off_without = prompt_in_group(db, Mode::Off).await;
    crate::services::channel_group::g0::leave();
    assert_eq!(off, shadow);
    assert_eq!(off, shadow_without);
    assert_eq!(off, off_without);
    // What crosses from her other groups, and what she heard, as ever.
    let elsewhere = [
        "## Them, in other groups",
        "## From your other groups",
        "## Things you heard",
    ];
    assert_eq!(titled(&off, &elsewhere), titled(&on, &elsewhere));
    let off_text = off.join("\n\n");
    assert!(off_text.contains("OTHER-GROUP-BIT"), "{off_text}");
    // The group's own, read from its nodes as it was from its rows.
    let own = [
        "## In this group",
        "## Days in this group",
        "## How this group takes you",
        "## What still stings",
    ];
    assert_eq!(titled(&off, &own), titled(&on, &own));
    assert_eq!(titled(&on, &own).len(), 4, "{on:#?}");
    let on = on.join("\n\n");
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
