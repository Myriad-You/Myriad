//! An end that was offline while the site forgot comes back without
//! bringing any of it back to life (P11), with this file's sides (`Side`,
//! the protocol-2 frames, the server's wiring).
//!
//! The owner's Mac holds everything of a line of their private talk before
//! it goes offline: the line in her log, the memory it taught her, her node
//! on it, and a line of its own copied into the site's conversation. While
//! it is away the owner forgets both lines on the site. Back, it first
//! sends every row it still holds whole, with its words, as an end that
//! lost track of what it sent would, and an edit of the memory it wrote
//! offline; then syncs as ever. Nothing comes back on the site (not a line,
//! a memory, a node's words, nor a payload that opens to any of it), and
//! the Mac is left holding stubs only, folding to the server's bytes.

use super::*;
use crate::services::agent::memory::manage;
use crate::services::agent::merope::memory_nodes::{
    self, Author as NodeAuthor, RowIn, partner, privacy_forget_tests::holding,
    privacy_forget_tests::log_says,
};

/// In every word that goes.
const MARK: &str = "QZXOFFLINE";
const SECRET: &str = "抑郁症";

fn node_row(node: &str, id: &str, kind: &str, body: Value) -> RowIn {
    RowIn {
        id: id.into(),
        node: node.into(),
        kind: kind.into(),
        body,
        author: NodeAuthor::Her,
    }
}

/// The Mac's rows about `object`, and whether each is a stub.
fn held(side: &Side, object: &str) -> Vec<(String, bool)> {
    side.log(PERSONAL)
        .into_iter()
        .filter(|row| row.object == object && row.kind != "forget")
        .map(|row| (row.id.clone(), row.forgotten && row.payload.is_none()))
        .collect()
}

#[tokio::test]
async fn an_end_offline_while_the_site_forgot_brings_nothing_back() {
    let Some(schema) = schema("p11_offline_back").await else {
        return;
    };
    let db = &schema.db;
    let root = root();
    // The site owner's, with her log on and its talk under the partner id.
    db.execute_unprepared(&format!(
        "UPDATE users SET is_owner = true WHERE id = {OWNER}; \
         INSERT INTO agent_sync_devices (id, user_id, name, token_hash, token_hint, created_at) \
         VALUES ('dev_mac', {OWNER}, 'Mac', 'h', 'x', NOW())"
    ))
    .await
    .unwrap();
    super::super::test_settings::set(on());
    super::super::keys::test_root::use_here(super::root());
    partner::remember(Some(PARTNER));
    assert_eq!(memory_nodes::audience::owners_talk(), PERSONAL);
    let t0 = crate::clock::now_micros();

    // On the site: their line, kept in her log; the memory it taught her,
    // compared into it; her topic on it, written into it.
    let said = format!("{MARK}-LINE 我得了{SECRET}，一直在吃药");
    db.execute_unprepared(&format!(
        "INSERT INTO agent_sessions (id, user_id, title, context, message_count, archived, \
         created_at, last_active_at) \
         VALUES ('ses_p11', {OWNER}, 'a', '{{\"mode\":\"chat\"}}', 1, false, NOW(), NOW())"
    ))
    .await
    .unwrap();
    db.execute_raw(sea_orm::Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Postgres,
        "INSERT INTO agent_messages (id, session_id, role, content, created_at) \
         VALUES (9101, 'ses_p11', 'user', $1, NOW() - INTERVAL '10 minutes')",
        [said.clone().into()],
    ))
    .await
    .unwrap();
    let at = crate::clock::now() - chrono::Duration::minutes(10);
    let line_row = super::super::talk::keep(db, &root, OWNER, "ses_p11", 9101, &said, at, "web")
        .await
        .unwrap()
        .expect("their line goes into her log");
    db.execute_raw(sea_orm::Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Postgres,
        "INSERT INTO agent_memories (id, user_id, kind, content, evidence, speaker, source, \
         venue, audience, concepts, valid_from, created_at, updated_at) VALUES \
         ('m_p11', $1, 'fact', $2, $3, 'user', 'chat', 'private', jsonb_build_array($1), '[]', \
          NOW(), NOW(), NOW())",
        [
            OWNER.into(),
            format!("{MARK}-MEMORY 得了{SECRET}在吃药").into(),
            said.clone().into(),
        ],
    ))
    .await
    .unwrap();
    let node = myriad_nodes::node_id(PERSONAL, "minted:p11-meds").unwrap();
    let thought = super::super::pen::think().expect("her mind is here in tests");
    for row in [
        node_row(
            &node,
            "p11-made",
            "node.created",
            json!({"version": 1, "sync": "shared", "saidBy": ["her"], "key": "minted:p11-meds",
                   "kind": "topic", "name": format!("{MARK}-NODE {SECRET}")}),
        ),
        node_row(
            &node,
            "p11-entry",
            "node.entry",
            json!({"version": 1, "sync": "shared", "saidBy": ["partner"], "audience": PERSONAL,
                   "text": format!("{MARK}-ENTRY 在吃{SECRET}的药"), "refs": ["memory:m_p11"]}),
        ),
    ] {
        memory_nodes::write(db, OWNER, PERSONAL, &[row], Some(&thought))
            .await
            .unwrap();
    }
    drop(thought);
    super::super::state::sync(db, &root, OWNER).await.unwrap();

    // The Mac: a line said there, copied into the site's conversation, and
    // everything of the site's taken whole.
    let mut mac = Side::new("dev_mac");
    mac.now = t0;
    let open = json!({ "at": t0 / 1000, "place": "app" });
    mac.write_body(
        PERSONAL,
        "segment:dev_mac_p11",
        "segment.open",
        Author::Owner,
        Some(open),
        t0,
    );
    let on_mac = format!("{MARK}-MAC {SECRET}又犯了");
    let mac_line = mac.write_body(
        PERSONAL,
        "segment:dev_mac_p11",
        "segment.line",
        Author::Owner,
        Some(json!({ "at": t0 / 1000, "text": on_mac, "line": "m1" })),
        t0 + 1,
    );
    sync(db, &mut mac, PERSONAL, t0 + 2).await;
    super::super::talk::fold(db, &root, OWNER).await.unwrap();
    let copy = db
        .query_one_raw(sea_orm::Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT session_id, id FROM agent_messages WHERE log_row = $1",
            [mac_line.id.clone().into()],
        ))
        .await
        .unwrap()
        .expect("the Mac's line is in the site's conversation");
    let copy: (String, i32) = (
        copy.try_get("", "session_id").unwrap(),
        copy.try_get("", "id").unwrap(),
    );
    let says = |side: &Side, text: &str| side.payloads.values().any(|data| super::says(data, text));
    for word in ["-LINE", "-MEMORY", "-NODE", "-ENTRY", "-MAC"] {
        assert!(
            says(&mac, &format!("{MARK}{word}")),
            "the Mac holds {word} before it goes"
        );
    }
    let stale: Vec<Row> = mac
        .log(PERSONAL)
        .into_iter()
        .filter(|row| row.payload.is_some())
        .collect();
    assert!(stale.len() >= 6, "{}", stale.len());

    // Offline, the Mac hears nothing while the owner forgets both lines on
    // the site, and edits the memory there.
    let gone = manage::forget_line(db, OWNER, "ses_p11", 9101)
        .await
        .unwrap()
        .expect("the line");
    assert_eq!(gone[0].id, 9101);
    manage::forget_line(db, OWNER, &copy.0, copy.1)
        .await
        .unwrap()
        .expect("the copy");
    let before = holding(db, MARK).await;
    assert!(
        before.is_empty(),
        "forgotten on the site before the Mac is back: {before:?}"
    );
    mac.write_body(
        PERSONAL,
        "memory:m_p11",
        "memory.write",
        Author::Owner,
        Some(json!({ "at": (t0 + MINUTE) / 1000, "kind": "fact",
                     "text": format!("{MARK}-EDIT {SECRET}好些了") })),
        t0 + MINUTE,
    );

    // Back: everything it holds whole, sent again with its words, then the
    // offline edit and the rest as any sync.
    let resent = mac.rows_frame(PERSONAL, stale);
    exchange(db, &mut mac, vec![resent], t0 + 2 * MINUTE).await;
    sync(db, &mut mac, PERSONAL, t0 + 2 * MINUTE).await;
    sync(db, &mut mac, PERSONAL, t0 + 3 * MINUTE).await;
    super::super::talk::fold(db, &root, OWNER).await.unwrap();
    super::super::state::sync(db, &root, OWNER).await.unwrap();

    // Nothing of it on the site: no column of any table, no payload of her
    // log that opens to it, no line, memory or node of it.
    for needle in [MARK, SECRET] {
        let left = holding(db, needle).await;
        assert!(left.is_empty(), "{needle} came back: {left:#?}");
        let opened = log_says(db, &root, OWNER, needle).await;
        assert!(opened.is_empty(), "{needle} opens again: {opened:?}");
    }
    assert_eq!(
        count(db, "SELECT COUNT(*) FROM agent_memories WHERE id = 'm_p11'").await,
        0
    );
    assert_eq!(
        count(
            db,
            "SELECT COUNT(*) FROM agent_messages WHERE id = 9101 OR log_row IS NOT NULL"
        )
        .await,
        0
    );
    assert_eq!(
        count(
            db,
            "SELECT COUNT(*) FROM agent_node_rows WHERE body <> '' AND row_id LIKE 'p11-%'"
        )
        .await,
        0
    );
    // The Mac holds stubs of all of it, and none of its words.
    let node_object = format!("node:{node}");
    for object in [
        line_row.object.as_str(),
        "segment:dev_mac_p11",
        "memory:m_p11",
        node_object.as_str(),
    ] {
        let rows = held(&mac, object);
        let whole: Vec<&(String, bool)> = rows
            .iter()
            .filter(|(id, stub)| {
                !stub
                    && (object.starts_with("memory:")
                        || object.starts_with("node:")
                        || *id == line_row.id
                        || *id == mac_line.id)
            })
            .collect();
        assert!(whole.is_empty(), "{object}: still whole on the Mac {whole:?}");
    }
    for word in [MARK, SECRET] {
        assert!(!says(&mac, word), "the Mac still holds {word}");
    }
    let leases = lease::history(db).await.unwrap();
    assert_eq!(
        mac.fold(PERSONAL, &leases),
        server_fold(db, PERSONAL, &leases).await,
        "the Mac folds to the server's bytes"
    );
    schema.drop().await;
}
