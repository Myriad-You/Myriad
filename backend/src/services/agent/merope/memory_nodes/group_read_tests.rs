//! What she is shown of her nodes in a group while she reads them, on a
//! database of its own, as `read_tests` (whose private talk, other account
//! and own audience are here too): that group's audience alone, what is
//! about another member said as theirs, and the group's own sections from
//! its node. Account 7 talks with her in `qq:5` and has been in `qq:6`;
//! account 8 is in `qq:5`.

use myriad_nodes::{SaidBy, entries::Placement};
use sea_orm::{ConnectionTrait, DatabaseConnection};
use serde_json::json;

use super::group_context;
use super::read_tests::{GROUP, OWNER, SEVEN, owned, put, ranked, seed, start};
use super::recall;
use super::{audience, hanging};
use crate::services::agent::memory::unified::{self, Audience};

/// The other group account 7 is in.
pub(crate) const ELSEWHERE: &str = "qq:6";

/// A row of a group's own, kept in `venue` as its writer keeps it, then
/// hung on her nodes.
async fn in_venue(
    db: &DatabaseConnection,
    venue: &str,
    source: &'static str,
    content: &str,
    evidence: serde_json::Value,
) -> String {
    let stored = Audience::group(venue, 0).venue();
    let id = unified::remember_in_venue(db, &stored, content, &evidence.to_string(), source)
        .await
        .unwrap()
        .unwrap();
    hanging::kept(db, &id).await;
    id
}

/// What `seed` keeps, and in the group: its own bit, a day, how it takes
/// her, a topic with what 7 and 8 told of it; in the other group, a bit,
/// and what 7 said of themselves there. The memory ids of 7's topic fact
/// and 8's.
pub(crate) async fn seed_group(db: &DatabaseConnection) -> (String, String) {
    seed(db).await;
    let group = audience::group(GROUP);
    in_venue(
        db,
        GROUP,
        "bit",
        "一提阿绿就笑场",
        json!({"handle": "GROUP-BIT 阿绿"}),
    )
    .await;
    in_venue(
        db,
        GROUP,
        "group_day",
        "GROUP-DAY 大家聊了一晚上游戏",
        json!({"day": "2026-11-05"}),
    )
    .await;
    in_venue(db, GROUP, "lands", "GROUP-LANDS 大家爱看她吐槽", json!({})).await;
    let theirs = put(
        db,
        OWNER,
        &group,
        SEVEN,
        Audience::group(GROUP, SEVEN),
        SaidBy::OtherPerson,
        start("游戏"),
        &["GROUP-TOPIC 在玩原神"],
    )
    .await;
    let eights = put(
        db,
        OWNER,
        &group,
        8,
        Audience::group(GROUP, 8),
        SaidBy::OtherPerson,
        start("游戏"),
        &["EIGHT-FACT 在玩星铁"],
    )
    .await;
    in_venue(
        db,
        ELSEWHERE,
        "bit",
        "OTHER-GROUP-BIT 一提阿绿就笑场",
        json!({"handle": "阿绿"}),
    )
    .await;
    put(
        db,
        OWNER,
        &audience::group(ELSEWHERE),
        SEVEN,
        Audience::group(ELSEWHERE, SEVEN),
        SaidBy::OtherPerson,
        Placement::AboutThem,
        &["OTHER-GROUP-FACT 阿绿是他的猫"],
    )
    .await;
    // 7 has talked with her there.
    db.execute_unprepared(&format!(
        "INSERT INTO agent_sessions (id, user_id, title, context, message_count, archived, \
         created_at, last_active_at) VALUES ('s-elsewhere', {SEVEN}, 't', \
         '{{\"mode\": \"chat\", \"venue\": \"{ELSEWHERE}\"}}'::jsonb, 1, false, NOW(), NOW())"
    ))
    .await
    .unwrap();
    (theirs[0].clone(), eights[0].clone())
}

#[tokio::test]
async fn a_group_reads_only_its_own_audience() {
    let Some(schema) = owned("memory_nodes_read_group").await else {
        return;
    };
    let db = &schema.db;
    let (theirs, _) = seed_group(db).await;
    let turn = group_context::load(db, SEVEN, GROUP)
        .await
        .unwrap()
        .expect("a site owner keeps it");
    assert!(
        turn.shown
            .nodes
            .iter()
            .all(|node| node.head["audience"] == "group:qq:5")
    );
    let now = crate::clock::now();
    let recalled = ranked(db, &[&theirs]).await;
    let here = turn.group.as_ref().map(|group| &group.here);
    let recollection = recall::recollect_here(&turn.shown, &turn.own, here, &recalled, None, now);
    let shown = [
        turn.bits_shown(db, 3, Some("阿绿")).await,
        myriad_merope::speaking::format_group_days_section(
            &turn.group_days(db, 3, Some("游戏")).await,
        ),
        turn.lands_section(now),
        turn.about_head_section(),
        turn.sores_here(now),
        turn.told_section(now),
        turn.us_section(now),
        turn.intents_section(),
    ]
    .into_iter()
    .flatten()
    .chain(recollection.named.iter().cloned())
    .collect::<Vec<_>>()
    .join("\n");
    for here in [
        "游戏: ",
        "GROUP-TOPIC",
        "GROUP-FACT",
        "GROUP-BIT",
        "GROUP-DAY",
        "GROUP-LANDS",
        "GROUP-SORE",
        "How this group takes you",
        "In this group",
    ] {
        assert!(shown.contains(here), "{here} is shown:\n{shown}");
    }
    // Another member's, said as theirs.
    let eight = recollection
        .named
        .iter()
        .find(|line| line.contains("EIGHT-FACT"))
        .expect("on the same topic");
    assert!(eight.contains("not the one talking to you"), "{eight}");
    // Nothing of a private talk, another group or her own.
    for never in [
        "PRIVATE-SORE",
        "在杭州做设计",
        "龟背竹",
        "吐槽工作",
        "像猫",
        "小笼包",
        "OTHER-ACCOUNT",
        "OTHER-GROUP",
        "OWN-TOPIC",
    ] {
        assert!(!shown.contains(never), "{never} is not shown:\n{shown}");
    }
}

#[tokio::test]
async fn going_through_all_of_a_kind_in_a_group_says_whose_each_is() {
    let Some(schema) = owned("memory_nodes_read_group_listed").await else {
        return;
    };
    let db = &schema.db;
    let (theirs, _) = seed_group(db).await;
    let turn = group_context::load(db, SEVEN, GROUP)
        .await
        .unwrap()
        .unwrap();
    let recalled = ranked(db, &[&theirs]).await;
    let cues = myriad_merope::remembering::Cues {
        thorough: true,
        ..Default::default()
    };
    let here = turn.group.as_ref().map(|group| &group.here);
    let recollection = recall::recollect_here(
        &turn.shown,
        &turn.own,
        here,
        &recalled,
        Some(&cues),
        crate::clock::now(),
    );
    let listed = recollection.listed.expect("a list");
    assert!(listed.contains("all 2 facts"), "{listed}");
    let eight = listed
        .lines()
        .find(|line| line.contains("EIGHT-FACT"))
        .expect("listed");
    assert!(eight.contains("not the one talking to you"), "{eight}");
    let seven = listed
        .lines()
        .find(|line| line.contains("GROUP-TOPIC"))
        .unwrap();
    assert!(!seven.contains("not the one talking to you"), "{seven}");
}
