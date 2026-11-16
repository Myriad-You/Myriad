//! Her nodes kept on the site, on a database of their own
//! (`MYRIAD_MEDIA_TEST_DATABASE_URL`, an isolated schema each).

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::{Value, json};

use super::audience::{self, OWN};
use super::heads::{KeptRow, fold_rows};
use super::placements::{self, How, Placement};
use super::read;
use super::{Author, Error, RowIn, Written, write};
use crate::services::her_log::pen;

const OWNER: i32 = 1;
const OTHER: i32 = 7;

async fn schema(prefix: &str) -> Option<crate::db::IsolatedSchema> {
    let url = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL").ok()?;
    let schema = crate::db::IsolatedSchema::migrated(&url, prefix).await;
    schema
        .db
        .execute_unprepared("INSERT INTO users (id, username) VALUES (1, 'one'), (7, 'seven')")
        .await
        .unwrap();
    Some(schema)
}

/// The rows the core makes for a topic she starts under `name` in
/// `audience`, with facts on it (`entries::fact_rows`), as her rows.
fn topic_rows(audience: &str, name: &str, facts: &[(&str, &str, &str)]) -> (String, Vec<RowIn>) {
    let facts: Vec<Value> = facts
        .iter()
        .map(|(memory, text, said_by)| json!({"memory": memory, "text": text, "saidBy": [said_by]}))
        .collect();
    let made = myriad_nodes::entries::fact_rows(
        serde_json::from_value(json!({
            "audience": audience,
            "start": {"name": name, "aliases": ["the green ones"]},
            "facts": facts,
        }))
        .unwrap(),
    )
    .unwrap();
    let rows = made
        .rows
        .into_iter()
        .map(|row| RowIn {
            id: row.id,
            node: made.node.clone(),
            kind: row.kind,
            body: row.body,
            author: Author::Her,
        })
        .collect();
    (made.node, rows)
}

async fn count(db: &DatabaseConnection, sql: &str) -> i64 {
    db.query_one_raw(Statement::from_string(DatabaseBackend::Postgres, sql))
        .await
        .unwrap()
        .unwrap()
        .try_get::<i64>("", "n")
        .unwrap()
}

/// Every row of `user_id`'s `audience`, as kept.
async fn kept_rows(db: &DatabaseConnection, user_id: i32, audience: &str) -> Vec<KeptRow> {
    db.query_all_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT body, envelope FROM agent_node_rows WHERE user_id = $1 AND audience = $2",
        [user_id.into(), audience.into()],
    ))
    .await
    .unwrap()
    .into_iter()
    .map(|row| KeptRow {
        row: serde_json::from_value(row.try_get::<Value>("", "envelope").unwrap()).unwrap(),
        body: Some(row.try_get("", "body").unwrap()),
    })
    .collect()
}

async fn her_write(
    db: &DatabaseConnection,
    user_id: i32,
    audience: &str,
    rows: &[RowIn],
) -> Result<Written, Error> {
    let thought = pen::think().expect("her mind is here in tests");
    write(db, user_id, audience, rows, Some(&thought)).await
}

#[tokio::test]
async fn writing_the_same_rows_twice_writes_nothing() {
    let Some(schema) = schema("memory_nodes_twice").await else {
        return;
    };
    let db = &schema.db;
    let talk = audience::owners_talk();
    let (node, rows) = topic_rows(
        &talk,
        "houseplants",
        &[("m1", "has a snake plant", "partner")],
    );
    let first = her_write(db, OWNER, &talk, &rows).await.unwrap();
    assert_eq!(first.rows.len(), 3, "created, aliases, one entry");
    assert_eq!(first.heads, vec![node.clone()]);
    let updated = "SELECT COUNT(*) AS n FROM agent_node_heads WHERE updated_at = \
                   (SELECT MIN(updated_at) FROM agent_node_heads)";
    assert_eq!(count(db, updated).await, 1);

    // The same again, and the same in one batch twice: nothing new.
    let mut twice = rows.clone();
    twice.extend(rows.iter().cloned());
    assert_eq!(
        her_write(db, OWNER, &talk, &twice).await.unwrap(),
        Written::default()
    );
    assert_eq!(
        count(db, "SELECT COUNT(*) AS n FROM agent_node_rows").await,
        3
    );
    // Its count in the audience has no holes.
    assert_eq!(
        count(db, "SELECT MAX(dseq) AS n FROM agent_node_rows").await,
        3
    );
    // Another row under a kept id is refused.
    let mut other = rows[2].clone();
    other.body["text"] = json!("has two snake plants");
    assert!(matches!(
        her_write(db, OWNER, &talk, &[other]).await,
        Err(Error::Refused(_))
    ));
    schema.drop().await;
}

#[tokio::test]
async fn heads_are_the_fold_of_the_rows() {
    let Some(schema) = schema("memory_nodes_fold").await else {
        return;
    };
    let db = &schema.db;
    let talk = audience::owners_talk();
    let (plants, rows) = topic_rows(
        &talk,
        "houseplants",
        &[("m1", "has a snake plant", "partner")],
    );
    her_write(db, OWNER, &talk, &rows).await.unwrap();
    let (doctors, rows) = topic_rows(
        &talk,
        "seeing doctors",
        &[
            ("m2", "sees Dr. Lee about a rash", "partner"),
            ("m3", "had a check-up with Dr. Patel", "partner"),
        ],
    );
    her_write(db, OWNER, &talk, &rows).await.unwrap();
    // A fact put on the first topic later.
    let more = myriad_nodes::entries::fact_rows(
        serde_json::from_value(json!({
            "audience": talk,
            "key": read::node(db, OWNER, &talk, &plants).await.unwrap().unwrap()["key"],
            "facts": [{"memory": "m4", "text": "waters on Sundays", "saidBy": ["partner"], "refs": ["message:9"]}],
        }))
        .unwrap(),
    )
    .unwrap();
    let more: Vec<RowIn> = more
        .rows
        .into_iter()
        .map(|row| RowIn {
            id: row.id,
            node: plants.clone(),
            kind: row.kind,
            body: row.body,
            author: Author::Her,
        })
        .collect();
    assert_eq!(
        her_write(db, OWNER, &talk, &more).await.unwrap().heads,
        vec![plants.clone()],
        "only the node the batch touches"
    );

    let folded = fold_rows(&kept_rows(db, OWNER, &talk).await).unwrap();
    assert!(folded.skipped.is_empty());
    assert_eq!(folded.nodes.len(), 2);
    for node in &folded.nodes {
        let kept = read::node(db, OWNER, &talk, &node.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(kept, serde_json::to_value(node).unwrap());
    }
    let heads = read::heads(db, OWNER, &talk, Some("active")).await.unwrap();
    let names: Vec<_> = heads
        .iter()
        .map(|head| head.name.clone().unwrap())
        .collect();
    assert_eq!(names, ["houseplants", "seeing doctors"]);
    assert_eq!(heads[0].kind.as_deref(), Some("topic"));
    assert_eq!(heads[0].aliases, ["the green ones"]);
    assert!(
        heads[0]
            .node_key
            .as_deref()
            .unwrap()
            .starts_with("minted:t")
    );
    assert!(
        read::heads(db, OWNER, &talk, Some("done"))
            .await
            .unwrap()
            .is_empty()
    );

    let entries = read::entries(db, OWNER, &talk, &plants).await.unwrap();
    let memories: Vec<_> = entries.iter().map(|entry| entry.memories.clone()).collect();
    assert_eq!(memories, [vec!["m1".to_string()], vec!["m4".to_string()]]);
    assert_eq!(entries[1].text, "waters on Sundays");
    assert_eq!(entries[1].refs, ["memory:m4", "message:9"]);
    assert!(entries[0].quote_only && !entries[1].quote_only);
    assert_eq!(
        read::entries(db, OWNER, &talk, &doctors)
            .await
            .unwrap()
            .len(),
        2
    );
    schema.drop().await;
}

#[tokio::test]
async fn her_rows_need_her_pen_and_the_persons_do_not() {
    let Some(schema) = schema("memory_nodes_pen").await else {
        return;
    };
    let db = &schema.db;
    let talk = audience::private_talk(OTHER, Some(OWNER));
    let (node, rows) = topic_rows(&talk, "houseplants", &[]);
    assert!(matches!(
        write(db, OTHER, &talk, &rows, None).await,
        Err(Error::NoPen)
    ));
    assert_eq!(
        count(db, "SELECT COUNT(*) AS n FROM agent_node_rows").await,
        0
    );
    her_write(db, OTHER, &talk, &rows).await.unwrap();

    // The person renames it: their row, said by them, written against the
    // name as it stands.
    let kept = read::node(db, OTHER, &talk, &node).await.unwrap().unwrap();
    let rename = |said_by: &str, id: &str| RowIn {
        id: id.into(),
        node: node.clone(),
        kind: "node.revised".into(),
        body: json!({
            "version": 1, "sync": "shared", "saidBy": [said_by],
            "base": kept["name"]["base"], "field": "name", "value": "plants at home",
        }),
        author: Author::Owner,
    };
    // Not said by them: the fold passes it by, so it is refused.
    assert!(matches!(
        write(db, OTHER, &talk, &[rename("her", "o1")], None).await,
        Err(Error::Refused(_))
    ));
    let written = write(db, OTHER, &talk, &[rename("partner", "o2")], None)
        .await
        .unwrap();
    assert_eq!(written.heads, vec![node.clone()]);
    let heads = read::heads(db, OTHER, &talk, None).await.unwrap();
    assert_eq!(heads[0].name.as_deref(), Some("plants at home"));
    let rows = "SELECT COUNT(*) AS n FROM agent_node_rows \
                WHERE author = 'owner' AND epoch IS NULL AND row_id = 'o2'";
    assert_eq!(count(db, rows).await, 1);
    let hers = "SELECT COUNT(*) AS n FROM agent_node_rows WHERE author = 'her' AND epoch = 0";
    assert_eq!(count(db, hers).await, 2);
    schema.drop().await;
}

#[tokio::test]
async fn what_the_core_refuses_is_not_written() {
    let Some(schema) = schema("memory_nodes_refused").await else {
        return;
    };
    let db = &schema.db;
    let talk = audience::owners_talk();
    let (node, rows) = topic_rows(
        &talk,
        "houseplants",
        &[("m1", "has a snake plant", "partner")],
    );
    // An entry on a node no created row makes.
    assert!(matches!(
        her_write(db, OWNER, &talk, &rows[2..]).await,
        Err(Error::Refused(_))
    ));
    // A kind the core does not read.
    let mut odd = rows[0].clone();
    odd.kind = "node.noted".into();
    assert!(her_write(db, OWNER, &talk, &[odd]).await.is_err());
    // A person in her own audience.
    let person = RowIn {
        id: "p1".into(),
        node: myriad_nodes::node_id(&talk, "person:7").unwrap(),
        kind: "node.created".into(),
        body: json!({"version": 1, "sync": "shared", "saidBy": ["her"], "kind": "person", "key": "person:7", "name": "seven"}),
        author: Author::Her,
    };
    assert!(her_write(db, OWNER, OWN, &[person]).await.is_err());
    // Not an audience the core reads.
    assert!(her_write(db, OWNER, "everyone", &rows).await.is_err());
    // A batch with one bad row writes none of it.
    let mut bad = rows.clone();
    bad[2].body["audience"] = json!("personal:7");
    assert!(her_write(db, OWNER, &talk, &bad).await.is_err());
    assert_eq!(
        count(db, "SELECT COUNT(*) AS n FROM agent_node_rows").await,
        0
    );
    assert_eq!(
        count(db, "SELECT COUNT(*) AS n FROM agent_node_heads").await,
        0
    );
    her_write(db, OWNER, &talk, &rows).await.unwrap();
    assert_eq!(
        read::heads(db, OWNER, &talk, None).await.unwrap()[0].node_id,
        node
    );
    schema.drop().await;
}

#[tokio::test]
async fn audiences_never_mix() {
    let Some(schema) = schema("memory_nodes_audiences").await else {
        return;
    };
    let db = &schema.db;
    let owners = audience::private_talk(OWNER, Some(OWNER));
    let others = audience::private_talk(OTHER, Some(OWNER));
    let group = audience::group("qq:42");
    let (owner_node, rows) = topic_rows(&owners, "houseplants", &[("m1", "has a fern", "partner")]);
    her_write(db, OWNER, &owners, &rows).await.unwrap();
    let (other_node, rows) =
        topic_rows(&others, "houseplants", &[("m2", "has a cactus", "partner")]);
    her_write(db, OTHER, &others, &rows).await.unwrap();
    let (group_node, group_rows) = topic_rows(
        &group,
        "houseplants",
        &[("m3", "someone grows basil", "other_person")],
    );
    her_write(db, OWNER, &group, &group_rows).await.unwrap();
    // One name, three audiences: three nodes.
    assert_ne!(owner_node, other_node);
    assert_ne!(owner_node, group_node);
    for (user_id, audience, node, memory) in [
        (OWNER, &owners, &owner_node, "m1"),
        (OTHER, &others, &other_node, "m2"),
        (OWNER, &group, &group_node, "m3"),
    ] {
        let heads = read::heads(db, user_id, audience, None).await.unwrap();
        assert_eq!(heads.len(), 1, "{audience}");
        assert_eq!(&heads[0].node_id, node);
        let entries = read::entries(db, user_id, audience, node).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].memories, [memory]);
    }
    // Another account never reads them, nor one audience another's.
    assert!(
        read::heads(db, OTHER, &owners, None)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        read::heads(db, OWNER, &others, None)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        read::node(db, OWNER, &owners, &group_node)
            .await
            .unwrap()
            .is_none()
    );
    // A row of one audience is not written into another under its id.
    let mut moved = group_rows[0].clone();
    moved.node = myriad_nodes::node_id(&owners, moved.body["key"].as_str().unwrap()).unwrap();
    assert!(matches!(
        her_write(db, OWNER, &owners, &[moved]).await,
        Err(Error::Refused(_))
    ));
    // Someone in a group is not the partner there.
    let mut claimed = group_rows[2].clone();
    claimed.id = "u-claimed".into();
    claimed.body["saidBy"] = json!(["partner"]);
    claimed.body["audience"] = json!(others);
    assert!(her_write(db, OWNER, &group, &[claimed]).await.is_err());
    schema.drop().await;
}

#[tokio::test]
async fn a_placement_goes_with_its_memory() {
    let Some(schema) = schema("memory_nodes_placements").await else {
        return;
    };
    let db = &schema.db;
    db.execute_unprepared(
        "INSERT INTO agent_memories (id, user_id, kind, content, source, venue, valid_from, \
         created_at, updated_at) VALUES \
         ('m1', 7, 'fact', 'has a fern', 'chat', 'private', now(), now(), now()), \
         ('m2', 7, 'fact', 'lives in Austin', 'chat', 'private', now(), now() + interval '1 second', now()), \
         ('m3', 1, 'fact', 'has a cat', 'chat', 'private', now(), now(), now()), \
         ('g1', NULL, 'fact', 'someone grows basil', 'group', 'group:qq:42', now(), now(), now())",
    )
    .await
    .unwrap();
    let talk = audience::private_talk(OTHER, Some(OWNER));
    let placed = Placement {
        memory_id: "m1".into(),
        user_id: OTHER,
        audience: talk.clone(),
        how: How::Node,
        node_id: Some("a".repeat(32)),
    };
    placements::place(db, &placed).await.unwrap();
    assert_eq!(placements::placement(db, "m1").await.unwrap(), Some(placed));
    // Nowhere names no node; a node is named.
    let nowhere = Placement {
        memory_id: "m2".into(),
        user_id: OTHER,
        audience: talk.clone(),
        how: How::Nowhere,
        node_id: Some("a".repeat(32)),
    };
    assert!(placements::place(db, &nowhere).await.is_err());
    assert_eq!(
        placements::unplaced(db, OTHER, &talk, &[], 10)
            .await
            .unwrap(),
        ["m2"]
    );
    assert_eq!(
        placements::unplaced(db, OWNER, &audience::owners_talk(), &["chat"], 10)
            .await
            .unwrap(),
        ["m3"]
    );
    assert!(
        placements::unplaced(db, OWNER, &audience::owners_talk(), &["import"], 10)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        placements::unplaced(db, OWNER, &audience::group("qq:42"), &[], 10)
            .await
            .unwrap(),
        ["g1"]
    );
    // Another end put it on a node: an entry names it, with no placement here.
    db.execute_unprepared(&format!(
        "INSERT INTO agent_node_rows (user_id, audience, node_id, row_id, kind, body, author, \
         at, hlc, origin, envelope) VALUES (7, '{talk}', '{node}', 'r1', 'node.entry', \
         '{{\"refs\":[\"memory:m2\"],\"text\":\"lives in Austin\"}}', 'her', now(), '0', \
         'mac', '{{}}')",
        node = "a".repeat(32)
    ))
    .await
    .unwrap();
    assert!(
        placements::unplaced(db, OTHER, &talk, &[], 10)
            .await
            .unwrap()
            .is_empty()
    );
    db.execute_unprepared("DELETE FROM agent_memories WHERE id = 'm1'")
        .await
        .unwrap();
    assert_eq!(placements::placement(db, "m1").await.unwrap(), None);
    assert_eq!(
        count(db, "SELECT COUNT(*) AS n FROM agent_node_placements").await,
        0
    );
    schema.drop().await;
}
