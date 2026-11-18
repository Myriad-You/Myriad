//! What still stings, closed on her node, let go of; and what could not be
//! put on her nodes, put there at her upkeep (`hanging`). On a database of
//! their own, as `talk_tests`: the site owner is account 1, account 7
//! talks with her in private.

use sea_orm::{ConnectionTrait, DatabaseConnection};
use serde_json::{Value, json};

use super::{Author, RowIn, audience, hanging, read, write};
use crate::services::agent::memory::unified::{self, Audience, MemoryKind, NewMemory, Speaker};
use crate::services::agent::merope::sore;
use crate::services::her_log::pen;
use crate::services::runtime_registry as registry;

const OWNER: i32 = 1;
const SEVEN: i32 = 7;

async fn owned(prefix: &str) -> Option<crate::db::IsolatedSchema> {
    let schema = super::talk_tests::schema(prefix).await?;
    schema
        .db
        .execute_unprepared("UPDATE users SET is_owner = true WHERE id = 1")
        .await
        .unwrap();
    Some(schema)
}

/// Their node in their talk with her, folded.
async fn theirs(db: &DatabaseConnection, talk: &str) -> (String, Value) {
    let node = myriad_nodes::node_id(talk, "person:7").unwrap();
    let head = read::node(db, SEVEN, talk, &node).await.unwrap().unwrap();
    (node, head)
}

/// Her upkeep's version of the open item `item` on `node`, closed when
/// `closed`, for a reason that is not the sore spot itself.
fn upkeep_closes(node: &str, head: &Value, item: &str, closed: bool) -> RowIn {
    let mut body = json!({
        "version": myriad_nodes::BODY_VERSION, "sync": "shared", "saidBy": ["her"],
        "base": head["open"][item]["base"], "item": item,
        "text": "那件事说开了", "because": ["span:talked"],
    });
    if closed {
        body["closed"] = json!(true);
    }
    RowIn {
        id: myriad_nodes::entries::row_id_from(
            "node.open_item",
            node,
            &format!("upkeep-{closed}"),
            "",
        ),
        node: node.to_string(),
        kind: "node.open_item".into(),
        body,
        author: Author::Her,
    }
}

/// An open item her upkeep closed that once named a sore spot: she let it
/// go, as at night, so it weighs on her mood toward them no more, on that
/// very item. One her upkeep only rewrote is left as it is, and so is a
/// closed item that only names it.
#[tokio::test]
async fn a_sore_spot_closed_on_her_node_is_let_go_of() {
    let Some(schema) = owned("memory_nodes_sore_closed").await else {
        return;
    };
    let db = &schema.db;
    let talk = audience::private_talk(SEVEN, Some(OWNER));
    sore::keep(
        db,
        SEVEN,
        Audience::private(SEVEN),
        "说我画得丑",
        sore::Weight::Hurt,
    )
    .await;
    assert!(sore::weighs_on(db, SEVEN).await.unwrap() > 0.0);
    let thought = pen::think().unwrap();
    // Rewritten, still open: nothing let go of.
    let (node, head) = theirs(db, &talk).await;
    let item = head["open"]
        .as_object()
        .unwrap()
        .keys()
        .next()
        .unwrap()
        .clone();
    let rewritten = upkeep_closes(&node, &head, &item, false);
    write::write(db, SEVEN, &talk, &[rewritten.clone()], Some(&thought))
        .await
        .unwrap();
    hanging::closed(db, SEVEN, &talk, &[rewritten]).await;
    assert_eq!(sore::open_all(db, SEVEN).await.len(), 1);
    // Another item that only names it (named as her upkeep names its own,
    // before the sore spot's in order), closed: not the sore spot itself.
    let sore_id = sore::open_all(db, SEVEN).await[0].id.clone();
    for closed in [false, true] {
        let (_, head) = theirs(db, &talk).await;
        let mut body = json!({
            "version": myriad_nodes::BODY_VERSION, "sync": "shared", "saidBy": ["her"],
            "base": head["open"]["o-talk-it-out"]["base"].as_array().cloned().unwrap_or_default(),
            "item": "o-talk-it-out", "text": "想找机会说清楚",
            "because": [format!("memory:{sore_id}")],
        });
        if closed {
            body["closed"] = json!(true);
        }
        let other = RowIn {
            id: myriad_nodes::entries::row_id_from(
                "node.open_item",
                &node,
                &format!("talk-{closed}"),
                "",
            ),
            node: node.clone(),
            kind: "node.open_item".into(),
            body,
            author: Author::Her,
        };
        write::write(db, SEVEN, &talk, &[other.clone()], Some(&thought))
            .await
            .unwrap();
        hanging::closed(db, SEVEN, &talk, &[other]).await;
    }
    assert_eq!(sore::open_all(db, SEVEN).await.len(), 1, "it still stings");
    // Closed, for what was said since: the sore spot named by an older
    // version is let go of.
    let (_, head) = theirs(db, &talk).await;
    let closing = upkeep_closes(&node, &head, &item, true);
    write::write(db, SEVEN, &talk, &[closing.clone()], Some(&thought))
        .await
        .unwrap();
    hanging::closed(db, SEVEN, &talk, &[closing]).await;
    assert!(sore::open_all(db, SEVEN).await.is_empty());
    assert_eq!(sore::weighs_on(db, SEVEN).await.unwrap(), 0.0);
    let let_go = db
        .query_one_raw(sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT count(*) AS n FROM agent_memories WHERE user_id = 7 \
             AND content LIKE '%后来放下了%' AND invalid_at IS NULL",
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<i64>("", "n")
        .unwrap();
    assert_eq!(let_go, 1);
    // Let go of on the very item, not on one of its own.
    let (_, head) = theirs(db, &talk).await;
    let open = head["open"].as_object().unwrap();
    assert_eq!(open.len(), 2, "the sore spot's item and the other");
    assert!(
        open[&item]["value"]["text"]
            .as_str()
            .unwrap()
            .contains("后来放下了")
    );
    schema.drop().await;
}

/// A memory that could not be put on her nodes when it was kept waits by
/// its id, and is put there at her upkeep, once; one replaced since is
/// not, and leaves the list too.
#[tokio::test]
async fn what_could_not_be_put_on_her_nodes_is_put_there_at_upkeep() {
    let Some(schema) = owned("memory_nodes_hang_later").await else {
        return;
    };
    let db = &schema.db;
    let talk = audience::private_talk(SEVEN, Some(OWNER));
    // Kept as a sore spot is, and not put on her nodes then.
    let id = unified::remember(
        db,
        NewMemory {
            user_id: SEVEN,
            kind: MemoryKind::Fact,
            content: "说我画得丑".into(),
            evidence: Some(json!({"weight": "hurt", "since": "2026-11-01T00:00:00Z"}).to_string()),
            speaker: Speaker::Agent,
            source: sore::SOURCE,
            audience: Audience::private(SEVEN),
            importance: 0.7,
            concepts: Vec::new(),
        },
    )
    .await
    .unwrap()
    .unwrap();
    // And one replaced since: nothing of it is put there.
    let replaced = unified::remember(
        db,
        NewMemory {
            user_id: SEVEN,
            kind: MemoryKind::Fact,
            content: "说我唱歌跑调".into(),
            evidence: Some(json!({"weight": "petty", "since": "2026-11-01T00:00:00Z"}).to_string()),
            speaker: Speaker::Agent,
            source: sore::SOURCE,
            audience: Audience::private(SEVEN),
            importance: 0.4,
            concepts: Vec::new(),
        },
    )
    .await
    .unwrap()
    .unwrap();
    db.execute_unprepared(&format!(
        "UPDATE agent_memories SET invalid_at = now(), invalid_reason = 'superseded' \
         WHERE id = '{replaced}'"
    ))
    .await
    .unwrap();
    for memory in [&id, &replaced] {
        registry::put(
            db,
            hanging::LATER_NAMESPACE,
            memory,
            registry::RegistryIdentity {
                subject_id: None,
                owner_id: None,
                tapp_id: None,
                runtime_id: None,
            },
            &(),
            crate::clock::now().timestamp() + 86_400,
        )
        .await
        .unwrap();
    }
    let node = myriad_nodes::node_id(&talk, "person:7").unwrap();
    assert!(read::node(db, SEVEN, &talk, &node).await.unwrap().is_none());
    hanging::hang_later(db).await;
    let (_, head) = theirs(db, &talk).await;
    assert_eq!(head["open"].as_object().map(|open| open.len()), Some(1));
    let waiting = registry::list(db, hanging::LATER_NAMESPACE, None, None)
        .await
        .unwrap();
    assert!(waiting.is_empty(), "put there once: it leaves the list");
    schema.drop().await;
}
