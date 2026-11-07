//! Her other memory sources, a group's going-over and a child end's
//! moments written onto her nodes where they are written today, and
//! forgotten with their rows, on a database of their own, as `talk_tests`.
//! The site owner is account 1; account 7 talks with her in private and in
//! the group `qq:5`.

use sea_orm::{ConnectionTrait, DatabaseConnection, TransactionTrait};
use serde_json::{Value, json};

use super::facts::GoesUnder;
use super::talk_tests::{count, update};
use super::{audience, facts, hanging, read};
use crate::services::agent::memory::{
    manage,
    unified::{self, Audience, MemoryKind, NewMemory, Speaker},
};
use crate::services::agent::merope::chat_remember::ChatMemoryUpdates;
use crate::services::agent::merope::store::{self, Keep};
use crate::services::agent::merope::{jargon, making_sense, nodes, sore};

const OWNER: i32 = 1;
const SEVEN: i32 = 7;
const GROUP: &str = "qq:5";

/// A database of its own whose site owner is account 1.
async fn owned(prefix: &str) -> Option<crate::db::IsolatedSchema> {
    let schema = super::talk_tests::schema(prefix).await?;
    schema
        .db
        .execute_unprepared("UPDATE users SET is_owner = true WHERE id = 1")
        .await
        .unwrap();
    Some(schema)
}

fn node(audience: &str, key: &str) -> String {
    myriad_nodes::node_id(audience, key).unwrap()
}

async fn head(db: &DatabaseConnection, user_id: i32, audience: &str, key: &str) -> Value {
    read::node(db, user_id, audience, &node(audience, key))
        .await
        .unwrap()
        .unwrap_or(Value::Null)
}

/// A row of `source` kept as its writer keeps it, then hung on her nodes.
async fn kept(
    db: &DatabaseConnection,
    user_id: i32,
    source: &'static str,
    audience: Audience,
    content: &str,
    evidence: Option<Value>,
) -> String {
    let id = unified::remember(
        db,
        NewMemory {
            user_id,
            kind: MemoryKind::Fact,
            content: content.into(),
            evidence: evidence.map(|evidence| evidence.to_string()),
            speaker: Speaker::Agent,
            source,
            audience,
            importance: 0.5,
            concepts: Vec::new(),
        },
    )
    .await
    .unwrap()
    .unwrap();
    hanging::kept(db, &id).await;
    id
}

/// The one open item on a head: its id and how it stands.
fn the_open_item(head: &Value) -> (String, Value) {
    let open = head["open"].as_object().expect("an open item");
    assert_eq!(open.len(), 1);
    let (id, item) = open.iter().next().unwrap();
    (id.clone(), item.clone())
}

#[tokio::test]
async fn what_she_keeps_of_someone_in_private_hangs_on_their_node() {
    let Some(schema) = owned("memory_nodes_sources_private").await else {
        return;
    };
    let db = &schema.db;
    let talk = audience::private_talk(SEVEN, Some(OWNER));
    assert_eq!(talk, "personal:7");

    // A sore spot: not wrapped up, then mended (the same item), then let go.
    sore::keep(
        db,
        SEVEN,
        Audience::private(SEVEN),
        "说我画得丑",
        sore::Weight::Hurt,
    )
    .await;
    let (item, open) = the_open_item(&head(db, SEVEN, &talk, "person:7").await);
    assert_eq!(open["value"]["text"], "说我画得丑");
    assert_eq!(open["saidBy"], json!(["her"]));
    assert!(open["value"]["closed"] != json!(true));
    sore::mend(db, &sore::open_all(db, SEVEN).await, &[0]).await;
    let (again, mended) = the_open_item(&head(db, SEVEN, &talk, "person:7").await);
    assert_eq!(again, item, "a mended sore is the same thing");
    assert_eq!(mended["history"].as_array().map(Vec::len), Some(1));
    sore::let_go(db, &sore::open_all(db, SEVEN).await, &[0]).await;
    let (again, let_go) = the_open_item(&head(db, SEVEN, &talk, "person:7").await);
    assert_eq!(again, item);
    assert_eq!(let_go["value"]["closed"], json!(true));
    assert!(
        let_go["value"]["text"]
            .as_str()
            .unwrap()
            .contains("后来放下了")
    );

    // What they told her about herself: an entry on her, said by them.
    making_sense::remember_told_by(db, SEVEN, "说她笑起来像猫").await;
    let told = read::entries(db, SEVEN, &talk, &node(&talk, "her"))
        .await
        .unwrap();
    assert_eq!(told.len(), 1);
    assert_eq!(
        (told[0].text.as_str(), told[0].said_by.clone()),
        ("说她笑起来像猫", vec!["partner".to_string()])
    );

    // What they are to her: her view, which counts, on 「关于你」; once.
    let us = kept(
        db,
        SEVEN,
        "us",
        Audience::private(SEVEN),
        "总在半夜来吐槽工作的朋友",
        None,
    )
    .await;
    let about = head(db, SEVEN, &talk, "person:7").await;
    assert_eq!(
        about["views"]["us"]["value"]["text"],
        "总在半夜来吐槽工作的朋友"
    );
    assert_eq!(
        about["views"]["us"]["value"]["because"],
        json!([format!("memory:{us}")])
    );
    let rows = "SELECT COUNT(*) AS n FROM agent_node_rows";
    let before = count(db, rows).await;
    assert!(!hanging::hang(db, Some(OWNER), &us).await.unwrap());
    assert_eq!(count(db, rows).await, before, "hung already: nothing again");
    let placed = super::placements::placement(db, &us)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(placed.how, super::placements::How::About);
    kept(
        db,
        SEVEN,
        "lands",
        Audience::private(SEVEN),
        "跟他说话很放松",
        None,
    )
    .await;
    let about = head(db, SEVEN, &talk, "person:7").await;
    assert_eq!(about["views"]["lands"]["value"]["text"], "跟他说话很放松");

    // A bit: an entry with its handle, between the two of them.
    kept(
        db,
        SEVEN,
        "bit",
        Audience::private(SEVEN),
        "被叫作包子就会生气",
        Some(json!({"handle": "小笼包"})),
    )
    .await;
    let bits = read::entries(db, SEVEN, &talk, &node(&talk, "person:7"))
        .await
        .unwrap();
    assert_eq!(bits.len(), 1);
    assert_eq!(bits[0].text, "小笼包：被叫作包子就会生气");
    assert_eq!(
        bits[0].said_by,
        vec!["her".to_string(), "partner".to_string()]
    );

    // Nothing of a private talk is anywhere but in it.
    let elsewhere = "SELECT COUNT(*) AS n FROM agent_node_rows WHERE audience <> 'personal:7'";
    assert_eq!(count(db, elsewhere).await, 0);
    schema.drop().await;
}

#[tokio::test]
async fn what_she_keeps_of_a_group_stays_in_the_group() {
    let Some(schema) = owned("memory_nodes_sources_group").await else {
        return;
    };
    let db = &schema.db;
    let group = audience::group(GROUP);
    let in_group = || Audience::group(GROUP, OWNER);

    // How she comes across there rests on what people there said: never
    // a view, an entry by them on the group's node.
    kept(db, OWNER, "lands", in_group(), "在这里像个爱接梗的", None).await;
    let the_group = head(db, OWNER, &group, &format!("group:{GROUP}")).await;
    assert!(the_group.get("views").is_none());
    let entries = read::entries(db, OWNER, &group, &node(&group, &format!("group:{GROUP}")))
        .await
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].said_by, vec!["other_person".to_string()]);

    // A day there: a span of that day.
    kept(
        db,
        OWNER,
        "group_day",
        in_group(),
        "大家在聊考试",
        Some(json!({"day": "2026-11-05"})),
    )
    .await;
    let the_group = head(db, OWNER, &group, &format!("group:{GROUP}")).await;
    let span = &the_group["spans"][0];
    assert_eq!(span["text"], "大家在聊考试");
    assert_eq!(span["saidBy"], json!(["her", "other_person"]));
    let (from, to) = (span["from"].as_i64().unwrap(), span["to"].as_i64().unwrap());
    assert!(
        (82_800_000_000..=90_000_000_000).contains(&(to - from)),
        "one day"
    );

    // A word of theirs, as someone there told it.
    jargon::told(
        db,
        GROUP,
        &myriad_merope::group_reading::Explained {
            term: "摸鱼王".into(),
            meaning: "总在上班时间聊天的人".into(),
            who: "A".into(),
            quote: "他就是摸鱼王".into(),
        },
    )
    .await;
    let entries = read::entries(db, OWNER, &group, &node(&group, &format!("group:{GROUP}")))
        .await
        .unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[1].said_by, vec!["other_person".to_string()]);

    // What they said of her there: on her of the group, by them.
    let id = unified::remember_in_venue(db, &group, "说她很会接梗", "{}", "about_me")
        .await
        .unwrap()
        .unwrap();
    hanging::kept(db, &id).await;
    let told = read::entries(db, OWNER, &group, &node(&group, "her"))
        .await
        .unwrap();
    assert_eq!(told[0].said_by, vec!["other_person".to_string()]);

    // A sore spot from someone there: on their node in the group.
    sore::keep(
        db,
        SEVEN,
        Audience::group(GROUP, SEVEN),
        "当众拿她开玩笑",
        sore::Weight::Petty,
    )
    .await;
    let (_, open) = the_open_item(&head(db, OWNER, &group, "person:7").await);
    assert_eq!(open["value"]["text"], "当众拿她开玩笑");

    // Nothing of the group reached anyone's private talk.
    let private = "SELECT COUNT(*) AS n FROM agent_node_rows WHERE audience LIKE 'personal:%'";
    assert_eq!(count(db, private).await, 0);

    // Her own day: a span on her, in her own audience.
    let day = chrono::NaiveDate::from_ymd_opt(2026, 11, 4).unwrap();
    assert!(
        unified::write_own_day(db, day, "今天写完了一首歌。")
            .await
            .unwrap()
    );
    assert!(
        hanging::hang(db, Some(OWNER), "day_2026-11-04")
            .await
            .unwrap()
    );
    let hers = head(db, OWNER, audience::OWN, "her").await;
    assert_eq!(hers["spans"][0]["text"], "今天写完了一首歌。");
    // No group or her own without a site owner.
    assert!(!hanging::hang(db, None, "day_2026-11-04").await.unwrap());
    schema.drop().await;
}

#[tokio::test]
async fn forgetting_a_row_takes_its_view_its_sore_and_its_entries() {
    let Some(schema) = owned("memory_nodes_sources_forget").await else {
        return;
    };
    let db = &schema.db;
    let talk = audience::private_talk(SEVEN, Some(OWNER));
    let us = kept(
        db,
        SEVEN,
        "us",
        Audience::private(SEVEN),
        "总在半夜来吐槽工作的朋友",
        None,
    )
    .await;
    sore::keep(
        db,
        SEVEN,
        Audience::private(SEVEN),
        "说我画得丑",
        sore::Weight::Hurt,
    )
    .await;
    sore::mend(db, &sore::open_all(db, SEVEN).await, &[0]).await;
    let bit = kept(
        db,
        SEVEN,
        "bit",
        Audience::private(SEVEN),
        "被叫作包子就会生气",
        Some(json!({"handle": "小笼包"})),
    )
    .await;
    let before = head(db, SEVEN, &talk, "person:7").await.to_string();
    assert!(before.contains("总在半夜") && before.contains("画得丑") && before.contains("包子"));

    assert!(manage::forget_any(db, &us).await.unwrap());
    let mended = sore::open_all(db, SEVEN).await;
    assert!(
        manage::forget_any(db, &mended[0].id).await.unwrap(),
        "with what it took the place of"
    );
    assert!(manage::forget_any(db, &bit).await.unwrap());
    let after = head(db, SEVEN, &talk, "person:7").await.to_string();
    assert!(!after.contains("总在半夜"), "the view is gone");
    assert!(
        !after.contains("画得丑"),
        "every version of the sore is gone"
    );
    assert!(!after.contains("包子"), "the entry is gone");
    let words = "SELECT COUNT(*) AS n FROM agent_node_rows WHERE body LIKE '%画得丑%' OR body LIKE '%半夜%'";
    assert_eq!(count(db, words).await, 0, "no word of them is kept");
    schema.drop().await;
}

#[tokio::test]
async fn what_someone_said_of_themselves_in_a_group_is_theirs_in_private_too() {
    let Some(schema) = owned("memory_nodes_sources_settle").await else {
        return;
    };
    let db = &schema.db;
    let present = Audience::group(GROUP, SEVEN);
    let updates = vec![
        update("works nights", &[], Some(GoesUnder::AboutThem)),
        update(
            "the group plays chess on Fridays",
            &[],
            Some(GoesUnder::New {
                name: "chess".into(),
                aliases: Vec::new(),
            }),
        ),
    ];
    let transaction = db.begin().await.unwrap();
    for update in &updates {
        store::apply_chat_memory_update_on(
            &transaction,
            SEVEN,
            crate::clock::now().fixed_offset(),
            update,
            &present,
            Keep::Lasting,
        )
        .await
        .unwrap();
    }
    transaction.commit().await.unwrap();
    let updates = ChatMemoryUpdates {
        updates,
        ..ChatMemoryUpdates::default()
    };
    facts::settled(db, SEVEN, Some(OWNER), &present, &updates).await;

    // The group's: on their node there, and a topic there, by someone there.
    let group = audience::group(GROUP);
    let theirs = read::entries(db, OWNER, &group, &node(&group, "person:7"))
        .await
        .unwrap();
    assert_eq!(theirs.len(), 1);
    assert_eq!(
        (theirs[0].text.as_str(), theirs[0].said_by.clone()),
        ("works nights", vec!["other_person".to_string()])
    );
    let topics = read::heads(db, OWNER, &group, None).await.unwrap();
    assert!(
        topics
            .iter()
            .any(|head| head.name.as_deref() == Some("chess"))
    );

    // Theirs in private: only what they said of themselves, kept as said
    // in the group, by them.
    let talk = audience::private_talk(SEVEN, Some(OWNER));
    let mine = read::entries(db, SEVEN, &talk, &node(&talk, "person:7"))
        .await
        .unwrap();
    assert_eq!(mine.len(), 1);
    assert_eq!(mine[0].text, "works nights");
    assert_eq!(
        (mine[0].audience.as_str(), mine[0].said_by.clone()),
        ("group:qq:5", vec!["partner".to_string()])
    );
    let private = "SELECT COUNT(*) AS n FROM agent_node_rows WHERE audience = 'personal:7' AND body LIKE '%chess%'";
    assert_eq!(count(db, private).await, 0);

    // Forgotten: from both.
    let id = super::talk_tests::memory_id(db, "works nights").await;
    assert!(manage::forget_any(db, &id).await.unwrap());
    assert!(
        read::entries(db, OWNER, &group, &node(&group, "person:7"))
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        read::entries(db, SEVEN, &talk, &node(&talk, "person:7"))
            .await
            .unwrap()
            .is_empty()
    );
    schema.drop().await;
}

#[tokio::test]
async fn a_child_ends_moments_hang_on_the_site_owners_repository() {
    let Some(schema) = owned("memory_nodes_sources_child").await else {
        return;
    };
    let db = &schema.db;
    let facts = std::collections::BTreeMap::from([(
        "repo".to_string(),
        "git@github.com:Me/Repo.git".to_string(),
    )]);
    let place = nodes::place(myriad_merope::notify::PEER_AGENT_EVENT, &facts).unwrap();
    let decided = |event: &'static str| nodes::Decided {
        event,
        happened: "waiting",
        action: "speak",
        reason: "test",
        origin: "dev_mac",
        note: Some(nodes::Note {
            event: event.into(),
            happened: "waiting".into(),
            action: "speak".into(),
            line: "它在等你回答。".into(),
            at: crate::clock::now(),
            said: false,
        }),
    };
    nodes::decided(db, OWNER, &place, decided("evt_a"))
        .await
        .unwrap();
    nodes::decided(db, OWNER, &place, decided("evt_a"))
        .await
        .unwrap();
    let talk = audience::owners_talk();
    let repo = head(db, OWNER, &talk, "repo:github.com/me/repo").await;
    assert_eq!(repo["object"]["value"]["kind"], "repo");
    assert_eq!(
        repo["spans"].as_array().map(Vec::len),
        Some(1),
        "each moment once"
    );
    assert_eq!(repo["spans"][0]["refs"], json!(["event:evt_a"]));

    // Forgotten on the child: forgotten here too.
    let id = nodes::node_id(OWNER, &place.key);
    assert!(nodes::forget(db, OWNER, &id).await.unwrap());
    let gone = head(db, OWNER, &talk, "repo:github.com/me/repo").await;
    assert!(gone.is_null() || gone["forgotten"] == json!(true));
    let words = "SELECT COUNT(*) AS n FROM agent_node_rows WHERE body LIKE '%等你回答%'";
    assert_eq!(count(db, words).await, 0);
    schema.drop().await;
}
