//! The spans of a line that went with a memory they took back: hidden by
//! the line, with what was written from them, never forgotten, and nothing
//! else that line said (`hidden`), on a database of its own.

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::json;

use super::talk_tests::{OTHER, answering, count, schema, talk, topic};
use super::{Author, RowIn, forget, hidden, inbox, intent_store, placing, read, take_back_check};
use crate::services::agent::memory::unified::{self, Audience, MemoryKind, NewMemory, Speaker};
use crate::services::agent::merope::chat_lines::tests::{line, session};
use crate::services::her_log::pen;

const TAKEN: &str = "面试没过";
const KEPT: &str = "猫快没粮了";

/// A fact of `user`'s kept from `quote`.
async fn kept(db: &DatabaseConnection, fact: &str, quote: &str, audience: Audience) -> String {
    unified::remember(
        db,
        NewMemory {
            user_id: OTHER,
            kind: MemoryKind::Fact,
            content: fact.into(),
            evidence: Some(quote.into()),
            speaker: Speaker::User,
            source: "chat",
            audience,
            importance: 0.5,
            concepts: Vec::new(),
        },
    )
    .await
    .unwrap()
    .unwrap()
}

/// What became of a row: `(rows standing, rows forgotten)` of `kind`.
async fn rows(db: &DatabaseConnection, kind: &str) -> (i64, i64) {
    let of = async |body: &str| {
        count(
            db,
            &format!(
                "SELECT COUNT(*) AS n FROM agent_node_rows WHERE kind = '{kind}' AND body {body} ''"
            ),
        )
        .await
    };
    (of("<>").await, of("=").await)
}

struct Talk {
    node: String,
    taken: String,
}

/// A line that said two things: one they will take back, and one kept (a
/// fact and something she is to do). Her node has a span of it and one of
/// another line, a summary written from the first, an item in her inbox
/// resting on each, the fact of the other thing and the intent.
async fn said_two_things(db: &DatabaseConnection) -> Talk {
    pen::reset_for_test();
    session(db, "mine", OTHER, Some(r#"{"mode":"chat"}"#), false).await;
    let two = line(
        db,
        "mine",
        "user",
        "今天面试没过，对了猫快没粮了，九点提醒我买",
        30,
        None,
    )
    .await;
    let hers = line(db, "mine", "assistant", "抱抱，九点叫你", 29, None).await;
    let other = line(db, "mine", "user", "周末想去爬香山看红叶", 20, None).await;
    let alone = Audience::private(OTHER);
    let taken = kept(db, TAKEN, "今天面试没过", alone.clone()).await;
    let cat = kept(db, KEPT, "猫快没粮了", alone).await;
    let node = topic(db, "近况", &[(cat.as_str(), KEPT)]).await;
    let now = crate::clock::now().fixed_offset();
    let spoken = |id, theirs, text: &str, minutes| placing::Line {
        id,
        theirs,
        text: text.into(),
        at: now - chrono::Duration::minutes(minutes),
    };
    answering(&[(
        "memory_node_place",
        r#"{"spans":[{"start":1,"node":"n1","gist":"面试没过"},{"start":3,"node":"n1","gist":"周末想爬香山"}]}"#,
    )]);
    placing::place(
        db,
        OTHER,
        None,
        &[
            spoken(two, true, "今天面试没过，对了猫快没粮了，九点提醒我买", 30),
            spoken(hers, false, "抱抱，九点叫你", 29),
            spoken(other, true, "周末想去爬香山看红叶", 20),
        ],
    )
    .await;
    assert_eq!(rows(db, "node.span").await, (2, 0));
    let span: String = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT row_id FROM agent_node_rows WHERE kind = 'node.span' AND body LIKE $1",
            [format!("%\"message:{two}\"%").into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "row_id")
        .unwrap();
    let written = [
        RowIn {
            id: myriad_nodes::entries::row_id_from("node.revised", &node, "summary", "1"),
            node: node.clone(),
            kind: "node.revised".into(),
            body: json!({
                "version": myriad_nodes::BODY_VERSION,
                "sync": {"local": crate::services::her_log::NODE},
                "saidBy": ["her"], "base": [], "field": "summary",
                "value": [{"text": "最近面试没过。", "because": [format!("span:{span}")]}],
            }),
            author: Author::Her,
        },
        RowIn {
            id: "u-intent-cat".into(),
            node: node.clone(),
            kind: "node.intent".into(),
            body: json!({
                "version": 1, "sync": "shared", "saidBy": ["partner"], "base": [],
                "intent": "i-cat", "text": "提醒买猫粮",
                "because": [format!("message:{two}")], "status": "waiting",
                "trigger": {
                    "at": (crate::clock::now() + chrono::Duration::days(1)).timestamp_micros(),
                    "zone": "Asia/Singapore",
                },
                "origin": "partner", "action": "say",
            }),
            author: Author::Her,
        },
    ];
    let thought = pen::think().unwrap();
    super::write::write(db, OTHER, &talk(), &written, Some(&thought))
        .await
        .unwrap();
    for (id, message) in [("waits-two", two), ("waits-other", other)] {
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO agent_node_inbox (user_id, audience, day, id, item, created_at) \
             VALUES ($1, $2, CURRENT_DATE, $3, jsonb_build_object('saidBy', \
               '[\"partner\"]'::jsonb, 'text', $3::text, 'refs', jsonb_build_array($4::text)), now())",
            [
                OTHER.into(),
                talk().into(),
                id.into(),
                format!("message:{message}").into(),
            ],
        ))
        .await
        .unwrap();
    }
    Talk { node, taken }
}

async fn let_go(db: &DatabaseConnection, memory: &str) {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE agent_memories SET invalid_at = now(), invalid_reason = 'superseded', \
         superseded_by = NULL, updated_at = now() WHERE id = $1",
        [memory.into()],
    ))
    .await
    .unwrap();
}

/// What is left to read once it went: the other line's, the other thing's
/// fact and the intent.
async fn only_it_is_gone(db: &DatabaseConnection, node: &str) {
    let read = read::node(db, OTHER, &talk(), node)
        .await
        .unwrap()
        .unwrap()
        .to_string();
    assert!(!read.contains("面试"), "{read}");
    assert!(read.contains("香山") && read.contains(KEPT), "{read}");
    let waiting = hidden::inbox_waiting(db, OTHER, &talk(), 10).await.unwrap();
    let ids: Vec<&str> = waiting.iter().map(|item| item.id.as_str()).collect();
    assert_eq!(ids, ["waits-other"]);
    let intents = intent_store::held(db, OTHER, &talk()).await.unwrap();
    assert_eq!(intents.len(), 1, "what she is to do stands");
    assert!(intents[0].waiting());
    assert_eq!(rows(db, "node.intent").await, (1, 0));
    assert_eq!(
        count(
            db,
            &format!(
                "SELECT COUNT(*) AS n FROM agent_memories WHERE content = '{KEPT}' \
                 AND invalid_at IS NULL"
            ),
        )
        .await,
        1,
        "the other thing that line said is known"
    );
}

/// Hidden as it is let go, and only hidden once the second look agrees.
#[tokio::test]
async fn a_line_taken_back_takes_its_spans_and_nothing_else_it_said() {
    let Some(schema) = schema("memory_nodes_lines_taken_back").await else {
        return;
    };
    let db = &schema.db;
    let said = said_two_things(db).await;
    let_go(db, &said.taken).await;
    take_back_check::hold(db, OTHER, &[said.taken.clone()], Some(1))
        .await
        .unwrap();
    only_it_is_gone(db, &said.node).await;
    assert_eq!(rows(db, "node.span").await, (2, 0), "only hidden");
    assert_eq!(rows(db, "node.revised").await.1, 0);
    assert_eq!(
        inbox::waiting(db, OTHER, &talk(), 10).await.unwrap().len(),
        2
    );
    // Confirmed: as hidden, and nothing made of the line forgotten.
    take_back_check::confirmed_now(db, OTHER).await;
    only_it_is_gone(db, &said.node).await;
    assert_eq!(rows(db, "node.span").await, (2, 0));
    assert_eq!(rows(db, "node.revised").await.1, 0);
    assert_eq!(
        inbox::waiting(db, OTHER, &talk(), 10).await.unwrap().len(),
        2
    );
    schema.drop().await;
}

/// What forgetting there left (or was taken back before it reached lines):
/// the sweep knows it by the line, and shows it again if it comes back
/// while only hidden.
#[tokio::test]
async fn the_sweep_knows_what_went_by_the_line() {
    let Some(schema) = schema("memory_nodes_lines_swept").await else {
        return;
    };
    let db = &schema.db;
    let said = said_two_things(db).await;
    let_go(db, &said.taken).await;
    forget::taken_back_here(db, OTHER, &talk()).await.unwrap();
    only_it_is_gone(db, &said.node).await;
    assert_eq!(rows(db, "node.span").await, (2, 0), "only hidden");
    db.execute_unprepared(&format!(
        "UPDATE agent_memories SET invalid_at = NULL, invalid_reason = NULL WHERE id = '{}'",
        said.taken
    ))
    .await
    .unwrap();
    forget::taken_back_here(db, OTHER, &talk()).await.unwrap();
    let read = read::node(db, OTHER, &talk(), &said.node)
        .await
        .unwrap()
        .unwrap()
        .to_string();
    assert!(read.contains("面试"), "{read}");
    let_go(db, &said.taken).await;
    super::talk_tests::confirmed(db).await;
    only_it_is_gone(db, &said.node).await;
    assert_eq!(rows(db, "node.span").await, (2, 0), "never forgotten");
    schema.drop().await;
}

/// A line of a group's talk is never taken by what they took back there.
#[tokio::test]
async fn a_groups_line_is_not_taken_by_it() {
    let Some(schema) = schema("memory_nodes_lines_group").await else {
        return;
    };
    let db = &schema.db;
    session(
        db,
        "there",
        OTHER,
        Some(r#"{"mode":"chat","venue":"qq:42"}"#),
        false,
    )
    .await;
    line(db, "there", "user", "今天面试没过", 30, None).await;
    let taken = kept(db, TAKEN, "今天面试没过", Audience::group("qq:42", OTHER)).await;
    let mine = kept(db, "面试没过的事", "今天面试没过", Audience::private(OTHER)).await;
    for memory in [&taken, &mine] {
        let_go(db, memory).await;
    }
    let lines = crate::services::agent::merope::chat_lines::taken_back_with(db, OTHER, |_| true)
        .await
        .unwrap();
    assert!(lines.is_empty(), "{lines:?}");
    schema.drop().await;
}

/// Taken back in the stretch it was said in, kept as no memory: what was
/// placed of the line is hidden at once, nothing is forgotten, and nothing
/// else it said goes.
#[tokio::test]
async fn a_line_let_go_in_its_stretch_takes_its_spans() {
    let Some(schema) = schema("memory_nodes_lines_let_go").await else {
        return;
    };
    let db = &schema.db;
    let said = said_two_things(db).await;
    let two: i32 = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT id FROM agent_messages WHERE content LIKE '今天面试没过%'",
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "id")
        .unwrap();
    db.execute_unprepared(&format!(
        "UPDATE agent_messages SET metadata = '{{\"notKept\":true,\"takenBack\":true}}' \
         WHERE id = {two}"
    ))
    .await
    .unwrap();
    forget::hide_lines_let_go(db, OTHER, &[two]).await;
    only_it_is_gone(db, &said.node).await;
    // Nothing is forgotten by the sweep either.
    forget::taken_back_here(db, OTHER, &talk()).await.unwrap();
    only_it_is_gone(db, &said.node).await;
    assert_eq!(rows(db, "node.span").await, (2, 0), "only hidden");
    assert_eq!(rows(db, "node.revised").await.1, 0);
    schema.drop().await;
}

/// A node kept of spans alone, as the talks of them with another AI and a
/// repository are, with something she is to do `because` of a line (0:
/// nothing): `lines` are the lines its spans are of.
async fn of_spans(db: &DatabaseConnection, key: &str, lines: &[i32], because: i32) -> String {
    let node = myriad_nodes::node_id(&talk(), key).unwrap();
    let at = crate::clock::now().timestamp_micros();
    let row = |id: String, kind: &str, body: serde_json::Value| RowIn {
        id,
        node: node.clone(),
        kind: kind.into(),
        body,
        author: Author::Her,
    };
    let mut created = json!({
        "version": myriad_nodes::BODY_VERSION, "sync": "shared", "saidBy": ["her"],
        "kind": "topic", "key": key, "name": "一起聊的",
    });
    if key.starts_with("repo:") {
        created["object"] = json!({ "kind": "repo", "name": "一起聊的" });
    }
    let mut rows = vec![row(
        myriad_nodes::entries::row_id_from("node.created", &node, key, ""),
        "node.created",
        created,
    )];
    for line in lines {
        let cite = format!("message:{line}");
        rows.push(row(
            myriad_nodes::entries::row_id_from("node.span", &node, &cite, ""),
            "node.span",
            json!({
                "version": myriad_nodes::BODY_VERSION, "sync": "shared", "saidBy": ["her"],
                "audience": talk(), "text": format!("第{line}句"), "refs": [cite],
                "from": at, "to": at,
            }),
        ));
    }
    if because == 0 {
        let thought = pen::think().unwrap();
        super::write::write(db, OTHER, &talk(), &rows, Some(&thought))
            .await
            .unwrap();
        return node;
    }
    rows.push(row(
        format!("u-intent-{key}"),
        "node.intent",
        json!({
            "version": 1, "sync": "shared", "saidBy": ["partner"], "base": [],
            "intent": format!("i-{}", key.replace(|ch: char| !ch.is_ascii_alphanumeric(), "-")), "text": "提醒买猫粮",
            "because": [format!("message:{because}")], "status": "waiting",
            "trigger": {
                "at": (crate::clock::now() - chrono::Duration::minutes(1)).timestamp_micros(),
                "zone": "Asia/Singapore",
            },
            "origin": "partner", "action": "say",
        }),
    ));
    let thought = pen::think().unwrap();
    super::write::write(db, OTHER, &talk(), &rows, Some(&thought))
        .await
        .unwrap();
    node
}

async fn waiting_on(db: &DatabaseConnection, node: &str) -> usize {
    intent_store::held(db, OTHER, &talk())
        .await
        .unwrap()
        .iter()
        .filter(|held| held.node == node && held.waiting())
        .count()
}

/// A node of spans alone is not emptied by one of them going: hidden or
/// forgotten, what else it holds stands, and what she is to do from it is
/// still hers to do.
#[tokio::test]
async fn a_node_of_spans_alone_keeps_the_rest_and_what_she_is_to_do() {
    let Some(schema) = schema("memory_nodes_spans_alone").await else {
        return;
    };
    let db = &schema.db;
    pen::reset_for_test();
    session(db, "mine", OTHER, Some(r#"{"mode":"chat"}"#), false).await;
    let taken = line(db, "mine", "user", "今天面试没过真难受", 30, None).await;
    let kept = line(db, "mine", "user", "九点提醒我买猫粮", 20, None).await;
    let one = of_spans(db, "minted:together-chats", &[taken], kept).await;
    let two = of_spans(db, "minted:repository-talks", &[taken, kept], kept).await;
    // Taken back in its stretch: hidden.
    db.execute_unprepared(&format!(
        "UPDATE agent_messages SET metadata = '{{\"notKept\":true,\"takenBack\":true}}' \
         WHERE id = {taken}"
    ))
    .await
    .unwrap();
    forget::hide_lines_let_go(db, OTHER, &[taken]).await;
    for node in [&one, &two] {
        let read = read::node(db, OTHER, &talk(), node).await.unwrap().unwrap();
        assert!(
            !read.to_string().contains(&format!("第{taken}句")),
            "{read}"
        );
        assert_eq!(waiting_on(db, node).await, 1, "{read}");
    }
    assert_eq!(rows(db, "node.intent").await.1, 0, "none of it is gone");
    // Forgotten, with the line itself.
    forget::forget(db, &[], &[taken]).await.unwrap();
    let read = read::node(db, OTHER, &talk(), &two).await.unwrap().unwrap();
    assert!(read.to_string().contains(&format!("第{kept}句")), "{read}");
    assert_eq!(
        rows(db, "node.created").await,
        (2, 0),
        "neither node is gone"
    );
    assert_eq!(rows(db, "node.span").await, (1, 2));
    schema.drop().await;
}

/// A topic of hers emptied is gone with what it held; not while a span is
/// left on it, nor a rule's own, whose key would never come back.
#[tokio::test]
async fn an_emptied_topic_goes_but_never_a_rules_own() {
    let Some(schema) = schema("memory_nodes_spans_emptied").await else {
        return;
    };
    let db = &schema.db;
    pen::reset_for_test();
    session(db, "mine", OTHER, Some(r#"{"mode":"chat"}"#), false).await;
    let first = line(db, "mine", "user", "今天面试没过真难受", 30, None).await;
    let last = line(db, "mine", "user", "周末想去爬香山看红叶", 20, None).await;
    let talks = of_spans(db, "minted:together-chats", &[first], 0).await;
    let part = of_spans(db, "minted:half-left", &[first, last], 0).await;
    let whole = of_spans(db, "minted:nothing-left", &[first], 0).await;
    let repo = of_spans(db, "repo:someone/something", &[first], 0).await;
    let mut beside = Vec::new();
    for (item, kind) in [
        ("view", "node.view_changed"),
        ("item", "node.open_item"),
        ("item", "node.standing"),
    ] {
        let node = of_spans(db, &format!("minted:{}", &kind[5..]), &[first], 0).await;
        let row = RowIn {
            id: myriad_nodes::entries::row_id_from(kind, &node, "beside", "her"),
            node: node.clone(),
            kind: kind.into(),
            body: json!({
                "version": myriad_nodes::BODY_VERSION, "sync": "shared", "saidBy": ["her"],
                "base": [], item: "them-0a1b2c3d4e", "text": "周末去爬香山",
                "because": [format!("message:{last}")],
            }),
            author: Author::Her,
        };
        let thought = pen::think().unwrap();
        super::write::write(db, OTHER, &talk(), &[row], Some(&thought))
            .await
            .unwrap();
        beside.push(node);
    }
    forget::forget(db, &[], &[first]).await.unwrap();
    let mut gone = Vec::new();
    for node in [&talks, &part, &repo, &whole].into_iter().chain(&beside) {
        let forgotten = count(
            db,
            &format!(
                "SELECT COUNT(*) AS n FROM agent_node_heads \
                 WHERE node_id = '{node}' AND status = 'forgotten'"
            ),
        )
        .await;
        gone.push(forgotten == 1);
    }
    assert_eq!(gone, [false, false, false, true, false, false, false]);
    // What comes to it next is there.
    of_spans(db, "minted:together-chats", &[last], 0).await;
    let read = read::node(db, OTHER, &talk(), &talks)
        .await
        .unwrap()
        .unwrap();
    assert!(read.to_string().contains(&format!("第{last}句")), "{read}");
    schema.drop().await;
}

/// Marked as what they asked her not to keep, after it was placed: hidden on
/// her nodes as a line taken back is, whoever sweeps after.
#[tokio::test]
async fn a_line_marked_as_not_to_keep_takes_its_spans() {
    use crate::services::agent::merope::asked;
    let Some(schema) = schema("memory_nodes_lines_marked").await else {
        return;
    };
    let db = &schema.db;
    let said = said_two_things(db).await;
    asked::tests::asks(db, OTHER, "不要记面试的事").await;
    // Of the three lines, the first.
    answering(&[("not_to_keep", r#"{"drop":[0]}"#)]);
    let now = crate::clock::now().fixed_offset();
    asked::lines::mark(
        db,
        OTHER,
        &Audience::private(OTHER),
        now - chrono::Duration::minutes(31),
        now - chrono::Duration::minutes(19),
        std::time::Instant::now() + std::time::Duration::from_secs(60),
        false,
    )
    .await
    .unwrap();
    crate::services::channel_group::g0::leave();
    let read = read::node(db, OTHER, &talk(), &said.node)
        .await
        .unwrap()
        .unwrap()
        .to_string();
    assert!(!read.contains("面试"), "{read}");
    assert!(read.contains("香山"), "{read}");
    assert_eq!(rows(db, "node.span").await, (2, 0), "only hidden");
    schema.drop().await;
}

/// Lines of a stretch not judged yet by what they asked her not to keep:
/// what was placed of them, and what waits in her inbox, is not read till
/// they are; nothing is forgotten, and what she is to do stands.
#[tokio::test]
async fn a_line_not_judged_yet_is_not_read_on_her_nodes() {
    use crate::services::agent::merope::{memory_jobs::Payload, store::memory_jobs::enqueue};
    let Some(schema) = schema("memory_nodes_lines_waiting").await else {
        return;
    };
    let db = &schema.db;
    said_two_things(db).await;
    let spans = async || {
        hidden::rows_of(db, OTHER, &talk())
            .await
            .unwrap()
            .into_iter()
            .filter(|kept| kept.row.kind == "node.span" && !kept.row.forgotten)
            .count()
    };
    let inbox = async || {
        hidden::inbox_waiting(db, OTHER, &talk(), 10)
            .await
            .unwrap()
            .into_iter()
            .map(|item| item.id)
            .collect::<Vec<_>>()
    };
    assert_eq!((spans().await, inbox().await.len()), (2, 2));
    let now = crate::clock::now().fixed_offset();
    for (id, present) in [
        ("elsewhere", Audience::group("qq:42", OTHER)),
        ("here", Audience::private(OTHER)),
    ] {
        let data = Payload::NotKept {
            present,
            from: now - chrono::Duration::minutes(31),
            last: now - chrono::Duration::minutes(28),
        };
        enqueue(db, id, OTHER, data, None).await.unwrap();
        if id == "elsewhere" {
            assert_eq!((spans().await, inbox().await.len()), (2, 2));
        }
    }
    assert_eq!(spans().await, 1);
    assert_eq!(inbox().await, ["waits-other"]);
    assert_eq!(rows(db, "node.span").await, (2, 0), "only not read");
    assert_eq!(
        intent_store::held(db, OTHER, &talk()).await.unwrap().len(),
        1
    );
    db.execute_unprepared("DELETE FROM runtime_registry WHERE record_id = 'here'")
        .await
        .unwrap();
    assert_eq!((spans().await, inbox().await.len()), (2, 2));
    schema.drop().await;
}

/// What rests on lines let go that could not be hidden then is not read: her
/// nodes are swept before she next reads them.
#[tokio::test]
async fn lines_let_go_that_could_not_be_hidden_are_swept_before_she_reads() {
    let Some(schema) = schema("memory_nodes_lines_unhidden").await else {
        return;
    };
    let db = &schema.db;
    let said = said_two_things(db).await;
    let two = count(
        db,
        "SELECT id::bigint AS n FROM agent_messages WHERE content LIKE '今天面试没过%'",
    )
    .await as i32;
    forget::before_reading(db, OTHER, &talk()).await.unwrap();
    db.execute_unprepared(&format!(
        "UPDATE agent_messages SET metadata = '{{\"notKept\":true,\"takenBack\":true}}' \
         WHERE id = {two}"
    ))
    .await
    .unwrap();
    let shown = async || {
        read::node(db, OTHER, &talk(), &said.node)
            .await
            .unwrap()
            .unwrap()
            .to_string()
    };
    assert!(shown().await.contains("面试"));
    // When she was last swept is told by whose, whichever test's: once more
    // when another's sweep came between.
    for _ in 0..5 {
        db.execute_unprepared("ALTER TABLE agent_node_rows RENAME TO away")
            .await
            .unwrap();
        forget::hide_lines_let_go(db, OTHER, &[two]).await;
        db.execute_unprepared("ALTER TABLE away RENAME TO agent_node_rows")
            .await
            .unwrap();
        forget::before_reading(db, OTHER, &talk()).await.unwrap();
        if !shown().await.contains("面试") {
            break;
        }
    }
    let shown = shown().await;
    assert!(!shown.contains("面试") && shown.contains("香山"), "{shown}");
    schema.drop().await;
}
