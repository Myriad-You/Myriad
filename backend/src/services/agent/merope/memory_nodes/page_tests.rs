//! The site owner's page of her nodes, on a database of its own, as
//! `read_tests` keeps it (account 7's private talk, account 8's, the group
//! `qq:5`, her own), with the site owner's own private talk beside: only
//! theirs, hers and her groups are on it; what they delete goes truly; a
//! name they set stands against her upkeep; an intent they cancel is
//! dropped and recorded so. Every word here is made up.

use myriad_nodes::{
    SaidBy,
    entries::Placement,
    upkeep::{self, Head, UpkeepInput},
};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::json;

use super::group_read_tests::seed_group;
use super::page_edit::{self, Answer, Refusal};
use super::read_tests::{OWNER, SEVEN, owned, put, start};
use super::switch::{Mode, set_for_test};
use super::talk_tests::count;
use super::{Author, RowIn, audience, heads, intent_store, page, read, write};
use crate::services::agent::memory::unified::Audience;

const COFFEE: [&str; 2] = ["OWNER-TOPIC 每天早上手冲一杯", "OWNER-TOPIC 豆子只买浅烘"];

/// What `seed_group` keeps, and the site owner's own talk: a topic of
/// coffee and a fact on 「关于你」. The topic's node id and its memory ids.
async fn seeded(db: &DatabaseConnection) -> (String, Vec<String>) {
    seed_group(db).await;
    set_for_test(Some(Mode::Shadow));
    let talk = audience::owners_talk();
    let ids = put(
        db,
        OWNER,
        &talk,
        OWNER,
        Audience::private(OWNER),
        SaidBy::Partner,
        start("咖啡"),
        &COFFEE,
    )
    .await;
    put(
        db,
        OWNER,
        &talk,
        OWNER,
        Audience::private(OWNER),
        SaidBy::Partner,
        Placement::AboutThem,
        &["OWNER-ABOUT 住在新加坡"],
    )
    .await;
    let node = page::audiences(db, OWNER).await.unwrap()[0]
        .nodes
        .iter()
        .find(|node| node.name.as_deref() == Some("咖啡"))
        .unwrap()
        .id
        .clone();
    (node, ids)
}

/// Every node of the page, read whole, as one text.
async fn everything(db: &DatabaseConnection) -> String {
    let mut all = String::new();
    for line in page::audiences(db, OWNER).await.unwrap() {
        for node in &line.nodes {
            let shown = page::node(db, OWNER, &line.audience, &node.id)
                .await
                .unwrap()
                .unwrap();
            all.push_str(&serde_json::to_string(&shown).unwrap());
        }
    }
    all
}

#[tokio::test]
async fn the_page_is_their_own_talk_her_own_and_her_groups() {
    let Some(schema) = owned("memory_nodes_page").await else {
        return;
    };
    let db = &schema.db;
    seeded(db).await;
    let lines = page::audiences(db, OWNER).await.unwrap();
    let audiences: Vec<&str> = lines.iter().map(|line| line.audience.as_str()).collect();
    assert_eq!(audiences[0], audience::owners_talk());
    assert_eq!(audiences[1], audience::OWN);
    assert!(audiences[2..].iter().all(|it| it.starts_with("group:")));
    assert!(audiences.contains(&"group:qq:5"));
    // 「关于你」 first, as their own node; the group by its id.
    assert!(lines[0].nodes[0].own && lines[0].nodes[0].kind.as_deref() == Some("person"));
    assert_eq!(lines[0].nodes[0].entries, 1);
    let group = lines.iter().find(|line| line.place == "group").unwrap();
    assert_eq!(group.group.as_deref(), Some("qq:5"));

    let all = everything(db).await;
    for theirs in ["OWNER-TOPIC", "OWNER-ABOUT", "OWN-TOPIC", "GROUP-TOPIC"] {
        assert!(all.contains(theirs), "{theirs}");
    }
    // Another account's private talk: never on the page, not even by name.
    for elsewhere in [
        "OTHER-ACCOUNT",
        "阳台上养了一盆绿萝",
        "在杭州做设计",
        "personal:7",
        "personal:8",
    ] {
        assert!(!all.contains(elsewhere), "{elsewhere}");
    }
    let sevens = audience::private_talk(SEVEN, Some(OWNER));
    let their_node = read::heads(db, SEVEN, &sevens, None).await.unwrap()[0]
        .node_id
        .clone();
    assert!(!page::on_the_page(&sevens) && !page::on_the_page("personal:8"));
    assert_eq!(
        page::node(db, OWNER, &sevens, &their_node).await.unwrap(),
        None
    );
    assert!(matches!(
        page_edit::forget_node(db, OWNER, &sevens, &their_node).await,
        Err(Refusal::NotFound)
    ));
    assert!(page::on_the_page("group:qq:5") && !page::on_the_page("group:"));
    set_for_test(None);
    schema.drop().await;
}

#[tokio::test]
async fn a_name_they_set_is_theirs_and_upkeep_holds_against_it() {
    let Some(schema) = owned("memory_nodes_page_name").await else {
        return;
    };
    let db = &schema.db;
    let (node, _) = seeded(db).await;
    let talk = audience::owners_talk();
    for bad in ["", "   ", &"长".repeat(81)] {
        assert!(matches!(
            page_edit::rename(db, OWNER, &talk, &node, bad).await,
            Err(Refusal::No("bad_name"))
        ));
    }
    page_edit::rename(db, OWNER, &talk, &node, " 手冲咖啡 ")
        .await
        .unwrap();
    // The same name again writes nothing.
    page_edit::rename(db, OWNER, &talk, &node, "手冲咖啡")
        .await
        .unwrap();
    let shown = page::node(db, OWNER, &talk, &node).await.unwrap().unwrap();
    assert_eq!(shown.name.as_deref(), Some("手冲咖啡"));
    assert!(shown.named_by_owner);
    let renames = "SELECT COUNT(*) AS n FROM agent_node_rows \
                   WHERE kind = 'node.revised' AND author = 'owner'";
    assert_eq!(count(db, renames).await, 1);
    // What her upkeep is shown of it: the name is the partner's, only a hand.
    let kept = heads::rows_of(db, OWNER, &talk).await.unwrap();
    let prepared = upkeep::prepare(UpkeepInput {
        bodies: heads::bodies(&kept).unwrap(),
        rows: kept.into_iter().map(|kept| kept.row).collect(),
        ledger: Vec::new(),
        audience: talk.clone(),
        node: Some(node.clone()),
        candidates: Vec::new(),
        inbox: Vec::new(),
    })
    .unwrap();
    assert!(prepared.node.unwrap().partner_set.contains(&Head::Name));
    // A group's node and her own are named so too.
    let group = page::audiences(db, OWNER).await.unwrap();
    let group = group
        .iter()
        .find(|line| line.audience == "group:qq:5")
        .unwrap();
    let topic = group
        .nodes
        .iter()
        .find(|node| node.kind.as_deref() == Some("topic"))
        .unwrap();
    page_edit::rename(db, OWNER, "group:qq:5", &topic.id, "群里的游戏")
        .await
        .unwrap();
    // Off: nothing is written.
    set_for_test(Some(Mode::Off));
    assert!(matches!(
        page_edit::rename(db, OWNER, &talk, &node, "咖啡豆").await,
        Err(Refusal::No("nodes_off"))
    ));
    set_for_test(None);
    schema.drop().await;
}

#[tokio::test]
async fn what_they_delete_goes_truly() {
    let Some(schema) = owned("memory_nodes_page_forget").await else {
        return;
    };
    let db = &schema.db;
    let (node, ids) = seeded(db).await;
    let talk = audience::owners_talk();
    let memories = |id: &str| format!("SELECT COUNT(*) AS n FROM agent_memories WHERE id = '{id}'");

    // One entry: its memory row goes, and the entry with it.
    let shown = page::node(db, OWNER, &talk, &node).await.unwrap().unwrap();
    let first = shown
        .entries
        .iter()
        .find(|entry| entry.text == COFFEE[0])
        .unwrap();
    page_edit::forget_entry(db, OWNER, &talk, &node, &first.row)
        .await
        .unwrap();
    assert_eq!(count(db, &memories(&ids[0])).await, 0);
    let shown = page::node(db, OWNER, &talk, &node).await.unwrap().unwrap();
    assert_eq!(
        shown
            .entries
            .iter()
            .map(|entry| entry.text.as_str())
            .collect::<Vec<_>>(),
        vec![COFFEE[1]]
    );
    assert!(matches!(
        page_edit::forget_entry(db, OWNER, &talk, &node, &first.row).await,
        Err(Refusal::NotFound)
    ));

    // 「关于你」 is not deleted whole.
    let about = &page::audiences(db, OWNER).await.unwrap()[0].nodes[0];
    assert!(about.own);
    assert!(matches!(
        page_edit::forget_node(db, OWNER, &talk, &about.id).await,
        Err(Refusal::No("person_node"))
    ));

    // The node: every memory on it, then the node; its rows stubs.
    page_edit::forget_node(db, OWNER, &talk, &node)
        .await
        .unwrap();
    assert_eq!(count(db, &memories(&ids[1])).await, 0);
    let listed = page::audiences(db, OWNER).await.unwrap();
    assert!(listed[0].nodes.iter().all(|line| line.id != node));
    // What is left of it is the mark that it was.
    let gone = page::node(db, OWNER, &talk, &node).await.unwrap();
    assert!(gone.is_none_or(|gone| gone.gone && gone.name.is_none() && gone.entries.is_empty()));
    let words = format!(
        "SELECT COUNT(*) AS n FROM agent_node_rows WHERE node_id = '{node}' \
         AND kind <> 'forget' AND body <> ''"
    );
    assert_eq!(count(db, &words).await, 0);
    assert!(!everything(db).await.contains("OWNER-TOPIC"));
    // Deleting is the person's act: it runs whatever the switch says.
    set_for_test(Some(Mode::Off));
    let about = page::node(db, OWNER, &talk, &about.id)
        .await
        .unwrap()
        .unwrap();
    page_edit::forget_entry(db, OWNER, &talk, &about.id, &about.entries[0].row)
        .await
        .unwrap();
    assert!(!everything(db).await.contains("OWNER-ABOUT"));
    set_for_test(None);
    schema.drop().await;
}

/// The owner's instruction, kept as S2b keeps one: their message, her
/// intent on 「关于你」 resting on it, and what the site keeps beside. The
/// message id and the intent id.
async fn instructed(db: &DatabaseConnection, about: &str) -> (i32, String) {
    db.execute_unprepared(
        "INSERT INTO agent_sessions (id, user_id, title, context, message_count, archived, \
         created_at, last_active_at) VALUES ('s_owner', 1, 't', '{\"mode\":\"chat\"}', 0, false, NOW(), NOW())",
    )
    .await
    .unwrap();
    let line: i32 = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "INSERT INTO agent_messages (session_id, role, content, created_at) \
             VALUES ('s_owner', 'user', 'OWNER-ASK 明天九点提醒我开会', NOW()) RETURNING id",
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "id")
        .unwrap();
    let talk = audience::owners_talk();
    let intent = "i-meeting".to_string();
    let at = (crate::clock::now() + chrono::Duration::days(1)).timestamp_micros();
    let row = RowIn {
        id: "u-intent-meeting".into(),
        node: about.to_string(),
        kind: "node.intent".into(),
        body: json!({
            "version": 1, "sync": "shared", "saidBy": ["partner"], "base": [],
            "intent": intent, "text": "提醒开会", "because": [format!("message:{line}")],
            "status": "waiting", "trigger": {"at": at, "zone": "Asia/Singapore"},
            "origin": "partner", "action": "say",
        }),
        author: Author::Her,
    };
    let pen = crate::services::her_log::pen::think().unwrap();
    write(db, OWNER, &talk, &[row], Some(&pen)).await.unwrap();
    intent_store::keep(
        db,
        &intent_store::Instruction {
            intent_id: intent.clone(),
            user_id: OWNER,
            audience: talk,
            node_id: about.to_string(),
            message_ref: format!("message:{line}"),
            echo: "好，明天九点叫你".into(),
            given: intent_store::standing_now(db, OWNER).await,
        },
    )
    .await
    .unwrap();
    (line, intent)
}

#[tokio::test]
async fn an_intent_they_cancel_is_dropped_and_recorded_so() {
    let Some(schema) = owned("memory_nodes_page_intent").await else {
        return;
    };
    let db = &schema.db;
    seeded(db).await;
    let talk = audience::owners_talk();
    let about = page::audiences(db, OWNER).await.unwrap()[0].nodes[0]
        .id
        .clone();
    let (line, intent) = instructed(db, &about).await;
    let shown = page::node(db, OWNER, &talk, &about).await.unwrap().unwrap();
    assert_eq!(shown.intents.len(), 1);
    let it = &shown.intents[0];
    assert_eq!(
        (it.status.as_str(), it.origin.as_deref()),
        ("waiting", Some("partner"))
    );
    assert!(it.when.as_ref().unwrap().at.is_some());
    let instruction = it.instruction.as_ref().unwrap();
    assert_eq!(instruction.echo, "好，明天九点叫你");
    assert_eq!(
        instruction.message.as_deref(),
        Some("OWNER-ASK 明天九点提醒我开会")
    );
    assert_eq!(
        page::audiences(db, OWNER).await.unwrap()[0].nodes[0].waiting,
        1
    );

    // Their message deleted elsewhere: the page says so, the echo stays.
    db.execute_unprepared(&format!("DELETE FROM agent_messages WHERE id = {line}"))
        .await
        .unwrap();
    let shown = page::node(db, OWNER, &talk, &about).await.unwrap().unwrap();
    assert_eq!(shown.intents[0].instruction.as_ref().unwrap().message, None);

    page_edit::cancel_intent(db, OWNER, &talk, &about, &intent)
        .await
        .unwrap();
    let shown = page::node(db, OWNER, &talk, &about).await.unwrap().unwrap();
    assert_eq!(shown.intents[0].status, "dropped");
    assert_eq!(
        shown.intents[0]
            .instruction
            .as_ref()
            .unwrap()
            .way
            .as_deref(),
        Some("cancelled")
    );
    // Not waiting any more: nothing to cancel.
    assert!(matches!(
        page_edit::cancel_intent(db, OWNER, &talk, &about, &intent).await,
        Err(Refusal::NotFound)
    ));
    // Nothing is heard on the site yet: no answer has anything to answer.
    assert!(shown.heard.is_empty());
    let row = shown.entries[0].row.clone();
    assert!(matches!(
        page_edit::answer_heard(db, OWNER, &talk, &about, &row, Answer::Denied).await,
        Err(Refusal::NotFound)
    ));
    set_for_test(None);
    schema.drop().await;
}
