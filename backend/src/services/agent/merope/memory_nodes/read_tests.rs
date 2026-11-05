//! What she is shown of her nodes in a private talk while she reads them,
//! on a database of its own, as `talk_tests`: only that talk's audience,
//! the one exception (what stings from a group) read as such, her own
//! audience never, what went never shown, and all of a kind listed by code.
//! The site owner is account 1; account 7 talks with her in private and in
//! the group `qq:5`; account 8 is someone else.

use std::collections::{BTreeSet, HashMap};

use myriad_nodes::{
    SaidBy,
    body::{Action, IntentStatus, Origin},
    entries::{Placement, Start},
};
use sea_orm::{ConnectionTrait, DatabaseConnection, EntityTrait};
use serde_json::{Value, json};

use super::context::{self, NodeTurn};
use super::facts::{self, Fact};
use super::intent_store::Held;
use super::recall::{self, Recollection};
use super::switch::{Mode, set_for_test};
use super::{audience, hanging};
use crate::services::agent::memory::unified::{self, Audience, MemoryKind, NewMemory, Speaker};
use crate::services::agent::merope::{making_sense, sore, store};

pub(crate) const OWNER: i32 = 1;
pub(crate) const SEVEN: i32 = 7;
pub(crate) const GROUP: &str = "qq:5";

/// A database of its own whose site owner is account 1, with account 8.
pub(crate) async fn owned(prefix: &str) -> Option<crate::db::IsolatedSchema> {
    let schema = super::talk_tests::schema(prefix).await?;
    schema
        .db
        .execute_unprepared(
            "UPDATE users SET is_owner = true WHERE id = 1; \
             INSERT INTO users (id, username) VALUES (8, 'eight')",
        )
        .await
        .unwrap();
    Some(schema)
}

/// A memory row of `user_id`'s, said where `audience` is, by `speaker`.
async fn memory(
    db: &DatabaseConnection,
    user_id: i32,
    speaker: Speaker,
    source: &'static str,
    audience: Audience,
    content: &str,
    evidence: Option<Value>,
) -> String {
    unified::remember(
        db,
        NewMemory {
            user_id,
            kind: MemoryKind::Fact,
            content: content.into(),
            evidence: evidence.map(|evidence| evidence.to_string()),
            speaker,
            source,
            audience,
            importance: 0.5,
            concepts: Vec::new(),
        },
    )
    .await
    .unwrap()
    .unwrap()
}

/// A row of one of her other sources, hung where it goes.
async fn kept(
    db: &DatabaseConnection,
    source: &'static str,
    content: &str,
    evidence: Option<Value>,
) -> String {
    let id = memory(
        db,
        SEVEN,
        Speaker::Agent,
        source,
        Audience::private(SEVEN),
        content,
        evidence,
    )
    .await;
    hanging::kept(db, &id).await;
    id
}

/// Facts of `them`'s kept for good, put where `placement` says in
/// `user_id`'s `audience`; their memory rows' ids.
#[allow(clippy::too_many_arguments)]
async fn put(
    db: &DatabaseConnection,
    user_id: i32,
    audience: &str,
    them: i32,
    said_in: Audience,
    said_by: SaidBy,
    placement: Placement,
    texts: &[&str],
) -> Vec<String> {
    let pen = crate::services::her_log::pen::think().expect("her mind is here in tests");
    let mut ids = Vec::new();
    let mut put = Vec::new();
    for text in texts {
        let id = memory(db, them, Speaker::User, "chat", said_in.clone(), text, None).await;
        put.push(Fact {
            memory: id.clone(),
            text: text.to_string(),
            said_by,
            placement: placement.clone(),
            standing: Default::default(),
        });
        ids.push(id);
    }
    facts::put_about(db, user_id, audience, them, &put, &pen)
        .await
        .unwrap();
    ids
}

fn start(name: &str) -> Placement {
    Placement::Start {
        name: name.into(),
        aliases: Vec::new(),
    }
}

/// What she keeps, in every audience: account 7's private talk (facts on
/// 「关于你」 and on a topic, one let go since, what they are to her, how
/// she comes across, a bit, what they told her of herself, a sore spot),
/// 7 in the group (a fact a member told of them, a sore spot born there),
/// account 8's private talk, and her own. Each text says where it is.
/// The memory ids of the plant facts, and of 7's facts on 「关于你」.
pub(crate) async fn seed(db: &DatabaseConnection) -> (Vec<String>, Vec<String>) {
    set_for_test(Some(Mode::Shadow));
    let talk = audience::private_talk(SEVEN, Some(OWNER));
    let group = audience::group(GROUP);
    let plants = put(
        db,
        SEVEN,
        &talk,
        SEVEN,
        Audience::private(SEVEN),
        SaidBy::Partner,
        start("植物"),
        &["阳台上养了一盆绿萝", "上个月买了一盆龟背竹"],
    )
    .await;
    let about = put(
        db,
        SEVEN,
        &talk,
        SEVEN,
        Audience::private(SEVEN),
        SaidBy::Partner,
        Placement::AboutThem,
        &["在杭州做设计", "不吃香菜", "OLD-JOB 以前在银行上班"],
    )
    .await;
    // Let go of since: replaced.
    db.execute_unprepared(&format!(
        "UPDATE agent_memories SET invalid_at = NOW(), superseded_by = '{}' WHERE id = '{}'",
        about[0], about[2]
    ))
    .await
    .unwrap();
    kept(db, "us", "总在半夜来吐槽工作的朋友", None).await;
    kept(db, "lands", "跟他说话可以随便一点", None).await;
    kept(db, "bit", "一提就笑", Some(json!({"handle": "小笼包"}))).await;
    making_sense::remember_told_by(db, SEVEN, "说她笑起来像猫").await;
    sore::keep(
        db,
        SEVEN,
        Audience::private(SEVEN),
        "PRIVATE-SORE 说我画得丑",
        sore::Weight::Hurt,
    )
    .await;
    // In the group: what a member said of them, and what stings from there.
    put(
        db,
        OWNER,
        &group,
        SEVEN,
        Audience::group(GROUP, SEVEN),
        SaidBy::OtherPerson,
        Placement::AboutThem,
        &["GROUP-FACT 群里有人说他要搬家"],
    )
    .await;
    sore::keep(
        db,
        SEVEN,
        Audience::group(GROUP, SEVEN),
        "GROUP-SORE 在群里笑我的头像",
        sore::Weight::Hurt,
    )
    .await;
    // Someone else's private talk, and her own.
    put(
        db,
        8,
        &audience::private_talk(8, Some(OWNER)),
        8,
        Audience::private(8),
        SaidBy::Partner,
        start("植物"),
        &["OTHER-ACCOUNT 也养绿萝"],
    )
    .await;
    put(
        db,
        OWNER,
        audience::OWN,
        OWNER,
        Audience::private(OWNER),
        SaidBy::Her,
        start("她的歌"),
        &["OWN-TOPIC 最近总听同一首歌"],
    )
    .await;
    set_for_test(Some(Mode::On));
    (plants, about)
}

/// What a ranking that found `named` (most relevant first) would hand over.
pub(crate) async fn ranked(db: &DatabaseConnection, named: &[&str]) -> store::Recalled {
    let mut recalled = store::Recalled {
        named: Vec::new(),
        brought_to_mind: Vec::new(),
        recent: Vec::new(),
        ids: Vec::new(),
        not_all_here: Vec::new(),
        said: HashMap::new(),
    };
    for id in named {
        let row = crate::models::entities::agent_memories::Entity::find_by_id(id.to_string())
            .one(db)
            .await
            .unwrap()
            .unwrap();
        let line = store::as_known(&unified::MemoryRecord::from(row));
        recalled.ids.push((line.clone(), id.to_string()));
        recalled.named.push(line);
    }
    recalled
}

fn everything(turn: &NodeTurn, recollection: &Recollection) -> String {
    let now = crate::clock::now();
    [
        turn.told_section(now),
        turn.her_head_section(),
        turn.bits_section(3),
        turn.lands_section(now),
        turn.us_section(now),
        turn.about_head_section(),
        sore::section(&turn.sores, now),
        turn.intents_section(),
        turn.ongoing_section(&recollection.picked),
        context::heard_section(&recollection.heard),
        recollection.listed.clone(),
    ]
    .into_iter()
    .flatten()
    .chain(recollection.named.iter().cloned())
    .collect::<Vec<_>>()
    .join("\n")
}

#[tokio::test]
async fn a_private_talk_reads_its_own_audience_and_what_stings_from_a_group() {
    let Some(schema) = owned("memory_nodes_read_private").await else {
        return;
    };
    let db = &schema.db;
    let (plants, about) = seed(db).await;
    let now = crate::clock::now();
    let turn = context::load(db, SEVEN, now).await.unwrap();
    assert!(
        turn.shown
            .nodes
            .iter()
            .all(|node| node.head["audience"] == "personal:7")
    );
    let recalled = ranked(db, &[&plants[1], &about[1]]).await;
    let recollection = recall::recollect(&turn.shown, &turn.own, &recalled, None, now);
    let shown = everything(&turn, &recollection);
    // What she keeps of them in their talk.
    for here in [
        "植物: ",
        "龟背竹",
        "绿萝",
        "在杭州做设计",
        "不吃香菜",
        "吐槽工作",
        "随便一点",
        "小笼包",
        "像猫",
        "PRIVATE-SORE",
    ] {
        assert!(shown.contains(here), "{here} is shown:\n{shown}");
    }
    // What stings from the group, felt in private as it always was.
    assert!(shown.contains("GROUP-SORE"), "{shown}");
    // Nothing else of the group, nothing of another account's talk or of
    // her own, and nothing let go of.
    for never in ["GROUP-FACT", "OTHER-ACCOUNT", "OWN-TOPIC", "OLD-JOB"] {
        assert!(!shown.contains(never), "{never} is not shown:\n{shown}");
    }
    // A bit is a bit, not a fact as well.
    assert!(
        !recollection
            .named
            .iter()
            .any(|line| line.contains("小笼包"))
    );
    // What their words found leads; the topic is one node, picked.
    assert!(recollection.named[0].starts_with("植物: "));
    assert_eq!(recollection.picked.len(), 1);
    assert!(recollection.heard.is_empty() && recollection.listed.is_none());
}

#[tokio::test]
async fn what_stings_from_a_group_is_theirs_alone_and_only_what_stings() {
    let Some(schema) = owned("memory_nodes_read_sores").await else {
        return;
    };
    let db = &schema.db;
    seed(db).await;
    // Someone else's sore spot in the same group.
    sore::keep(
        db,
        8,
        Audience::group(GROUP, 8),
        "EIGHT-SORE 抢了我的话",
        sore::Weight::Petty,
    )
    .await;
    let sores = context::sores_in_groups(db, SEVEN, OWNER).await.unwrap();
    let what: Vec<&str> = sores.iter().map(|sore| sore.what.as_str()).collect();
    assert_eq!(what, ["GROUP-SORE 在群里笑我的头像"]);
    assert!(sores[0].venue.starts_with("group:"));
    // Let go of, it is not felt any more.
    sore::let_go(db, &sore::open_in_group(db, GROUP, Some(SEVEN)).await, &[0]).await;
    assert!(
        context::sores_in_groups(db, SEVEN, OWNER)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn going_through_all_of_a_kind_lists_every_one_with_their_words() {
    let Some(schema) = owned("memory_nodes_read_listed").await else {
        return;
    };
    let db = &schema.db;
    let (plants, _) = seed(db).await;
    // Their words behind one: what the fact left out.
    db.execute_unprepared(&format!(
        "UPDATE agent_memories SET evidence = '我家阳台那盆绿萝叫阿绿' WHERE id = '{}'",
        plants[0]
    ))
    .await
    .unwrap();
    let now = crate::clock::now();
    let turn = context::load(db, SEVEN, now).await.unwrap();
    // Their words found one of the kind only.
    let recalled = ranked(db, &[&plants[1]]).await;
    let cues = myriad_merope::remembering::Cues {
        thorough: true,
        ..Default::default()
    };
    let recollection = recall::recollect(&turn.shown, &turn.own, &recalled, Some(&cues), now);
    let listed = recollection.listed.expect("a list");
    assert!(listed.starts_with("## All of it, listed"), "{listed}");
    assert!(
        listed.contains("龟背竹") && listed.contains("绿萝"),
        "{listed}"
    );
    assert!(listed.contains("阿绿"), "their words beside it: {listed}");
    assert!(listed.contains("all 2 facts"), "{listed}");
    for never in ["OTHER-ACCOUNT", "在杭州做设计", "OLD-JOB"] {
        assert!(!listed.contains(never), "{never}: {listed}");
    }
}

fn held(id: &str, origin: Origin, action: Action, trigger: Option<Value>, at: i64) -> Held {
    Held {
        node: "n".into(),
        id: id.into(),
        text: id.into(),
        because: Vec::new(),
        status: IntentStatus::Waiting,
        trigger,
        origin: Some(origin),
        action: Some(action),
        said_by: json!(["her"]),
        sync: json!("shared"),
        base: Vec::new(),
        at,
    }
}

#[test]
fn what_she_may_bring_up_now_is_due_or_hers_waiting_for_nothing() {
    let time = Some(json!({"at": 1, "zone": "+00:00"}));
    let waiting = vec![
        held("hers-undated", Origin::Her, Action::Say, None, 3),
        held("hers-later", Origin::Her, Action::Say, time.clone(), 1),
        held("theirs-due", Origin::Partner, Action::Say, time.clone(), 2),
        held("work-due", Origin::Partner, Action::Work, time.clone(), 0),
        held("theirs-undated", Origin::Partner, Action::Say, None, 0),
    ];
    let due: BTreeSet<(String, String)> = ["theirs-due", "work-due"]
        .iter()
        .map(|id| ("n".to_string(), id.to_string()))
        .collect();
    let now: Vec<String> = context::choose_now(waiting.clone(), &due)
        .into_iter()
        .map(|held| held.id)
        .collect();
    // Oldest first; never an instruction to do something; theirs only due.
    assert_eq!(now, ["theirs-due", "hers-undated"]);
    let mut many = waiting;
    for at in 10..14 {
        many.push(held(
            &format!("hers-{at}"),
            Origin::Her,
            Action::Say,
            None,
            at,
        ));
    }
    assert_eq!(context::choose_now(many, &due).len(), context::INTENTS);
}
