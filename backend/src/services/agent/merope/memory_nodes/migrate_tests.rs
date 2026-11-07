//! Moving what she kept before her nodes onto them ([`super::migrate`]),
//! on a database of their own, as `talk_tests`, with the model's answers
//! given by the test. Everything is kept the old way first, with the
//! switch off; then moved. The site owner is account 1; account 7 talks
//! with her in private and in the group `qq:5`. Every word is made up.

use std::collections::BTreeSet;

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::{Value, json};

use super::migrate::{self, Options, Report, Status};
use super::placements::{self, How};
use super::talk_tests::{answering, asked, count};
use super::{audience, facts, home_intents, intent_store, read};
use crate::services::agent::memory::unified::Audience;
use crate::services::agent::merope::{nodes, store, threads};
use crate::services::her_log::pen;

pub(super) const OWNER: i32 = 1;
pub(super) const SEVEN: i32 = 7;
pub(super) const OPERATION: &str = "memory_node_migrate_sort";
/// The old topics' keys, as the old tables mint them.
pub(super) const PLANTS: &str = "minted:0123456789abcdef0123456789abcdef";
pub(super) const DOCTORS: &str = "minted:fedcba9876543210fedcba9876543210";

/// A database of its own whose site owner is account 1.
pub(super) async fn owned(prefix: &str) -> Option<crate::db::IsolatedSchema> {
    let schema = super::talk_tests::schema(prefix).await?;
    schema
        .db
        .execute_unprepared("UPDATE users SET is_owner = true WHERE id = 1")
        .await
        .unwrap();
    pen::reset_for_test();
    Some(schema)
}

pub(super) async fn sql(db: &DatabaseConnection, sql: &str) {
    db.execute_unprepared(sql).await.unwrap();
}

/// A memory row kept the old way: `hours` ago, held unless `retired`.
pub(super) async fn memory(
    db: &DatabaseConnection,
    id: &str,
    user_id: Option<i32>,
    (source, venue, speaker): (&str, &str, &str),
    content: &str,
    evidence: Option<&str>,
    hours: i64,
) {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_memories (id, user_id, kind, content, evidence, speaker, source, venue, \
         valid_from, created_at, updated_at) VALUES ($1, $2, 'fact', $3, $4, $5, $6, $7, \
         now(), now() - make_interval(hours => $8::int), now())",
        [
            id.into(),
            user_id.into(),
            content.into(),
            evidence.map(str::to_string).into(),
            speaker.into(),
            source.into(),
            venue.into(),
            (hours as i32).into(),
        ],
    ))
    .await
    .unwrap();
}

/// An old topic of `user_id` with these memories under it (`None` for a
/// memory looked at and put under none).
pub(super) async fn old_topic(
    db: &DatabaseConnection,
    user_id: i32,
    key: Option<(&str, &str)>,
    under: &[&str],
) {
    let id = key.map(|(key, name)| {
        let id = myriad_nodes::node_id("personal:local-owner", key).unwrap();
        (id, key, name)
    });
    if let Some((id, key, name)) = &id {
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO agent_topic_nodes (id, user_id, audience, node_key, name, aliases, created_at) \
             VALUES ($1, $2, 'personal:local-owner', $3, $4, '[\"绿植\"]'::jsonb, now())",
            [id.clone().into(), user_id.into(), (*key).into(), (*name).into()],
        ))
        .await
        .unwrap();
    }
    for memory in under {
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO agent_memory_topics (memory_id, user_id, topic_id) VALUES ($1, $2, $3)",
            [
                (*memory).into(),
                user_id.into(),
                id.as_ref().map(|(id, _, _)| id.clone()).into(),
            ],
        ))
        .await
        .unwrap();
    }
}

/// A line of `user_id`'s in their private talk, `hours` ago; its id.
async fn line(db: &DatabaseConnection, user_id: i32, text: &str, hours: i64) -> i32 {
    let session = format!("s_{user_id}");
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_sessions (id, user_id, title, context, message_count, archived, \
         created_at, last_active_at) VALUES ($1, $2, 't', '{\"mode\":\"chat\"}', 0, false, NOW(), NOW()) \
         ON CONFLICT (id) DO NOTHING",
        [session.clone().into(), user_id.into()],
    ))
    .await
    .unwrap();
    db.query_one_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_messages (session_id, role, content, created_at) \
         VALUES ($1, 'user', $2, now() - make_interval(hours => $3::int)) RETURNING id",
        [session.into(), text.into(), (hours as i32).into()],
    ))
    .await
    .unwrap()
    .unwrap()
    .try_get("", "id")
    .unwrap()
}

/// Every row of the old tables, as text: what moving must leave as it was.
pub(super) async fn old_tables(db: &DatabaseConnection) -> Vec<String> {
    let mut out = Vec::new();
    for table in [
        "agent_memories",
        "agent_memory_topics",
        "agent_topic_nodes",
        "agent_nodes",
        "agent_node_events",
        "agent_messages",
        "agent_sessions",
    ] {
        let row = db
            .query_one_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                format!(
                    "SELECT COALESCE(string_agg(to_jsonb(t)::text, '\n' ORDER BY to_jsonb(t)::text), '') \
                     AS rows FROM {table} t"
                ),
            ))
            .await
            .unwrap()
            .unwrap();
        out.push(format!(
            "{table}\n{}",
            row.try_get::<String>("", "rows").unwrap()
        ));
    }
    out
}

pub(super) async fn moved(db: &DatabaseConnection, options: Options) -> Report {
    migrate::run(db, options)
        .await
        .unwrap()
        .expect("no other run")
}

pub(super) fn real(replay: bool) -> Options {
    Options {
        dry_run: false,
        replay,
    }
}

pub(super) async fn memories_on(
    db: &DatabaseConnection,
    user_id: i32,
    audience: &str,
    key: &str,
) -> BTreeSet<String> {
    let node = myriad_nodes::node_id(audience, key).unwrap();
    read::entries(db, user_id, audience, &node)
        .await
        .unwrap()
        .into_iter()
        .flat_map(|entry| entry.memories)
        .collect()
}

/// Everything the old way: two accounts' topics, what is under them and
/// what is not, a retired memory, her other sources, a child's note,
/// threads, and a group's fact. The line the owner's first fact quotes.
pub(super) async fn seed(db: &DatabaseConnection) -> i32 {
    let private = |speaker| ("chat", "private", speaker);
    let quoted = line(db, OWNER, "我养了一盆绿萝，放在窗台上", 3).await;
    memory(
        db,
        "m1",
        Some(OWNER),
        private("user"),
        "养了一盆绿萝",
        Some("我养了一盆绿萝"),
        2,
    )
    .await;
    memory(
        db,
        "m2",
        Some(OWNER),
        private("user"),
        "每周日浇水",
        None,
        2,
    )
    .await;
    memory(db, "m3", Some(OWNER), private("user"), "不吃香菜", None, 2).await;
    memory(
        db,
        "m_old",
        Some(OWNER),
        private("user"),
        "以前养过仙人掌",
        None,
        5,
    )
    .await;
    sql(
        db,
        "UPDATE agent_memories SET invalid_at = now(), invalid_reason = 'faded' WHERE id = 'm_old'",
    )
    .await;
    old_topic(db, OWNER, Some((PLANTS, "plants")), &["m1", "m2", "m_old"]).await;
    old_topic(db, OWNER, None, &["m3"]).await;
    memory(
        db,
        "m4",
        Some(SEVEN),
        private("user"),
        "在看李医生",
        None,
        2,
    )
    .await;
    memory(db, "m5", Some(SEVEN), private("user"), "在学吉他", None, 2).await;
    old_topic(db, SEVEN, Some((DOCTORS, "doctors")), &["m4"]).await;
    // Her other sources.
    memory(
        db,
        "us7",
        Some(SEVEN),
        ("us", "private", "agent"),
        "认识很久的朋友",
        None,
        2,
    )
    .await;
    memory(
        db,
        "me7",
        Some(SEVEN),
        ("about_me", "private", "user"),
        "说她讲话很慢",
        None,
        2,
    )
    .await;
    memory(
        db,
        "bit5",
        Some(OWNER),
        ("bit", "group:qq:5", "agent"),
        "叫他小笼包",
        Some("{\"handle\":\"包子\"}"),
        2,
    )
    .await;
    memory(
        db,
        "day_2026-11-04",
        None,
        ("narrative", "own", "agent"),
        "下午读了一本书",
        None,
        2,
    )
    .await;
    // A group's fact, kept going over a stretch there.
    memory(
        db,
        "g1",
        Some(SEVEN),
        ("chat", "group:qq:5", "user"),
        "在上海上班",
        None,
        2,
    )
    .await;
    // A child's note.
    let facts = std::collections::BTreeMap::from([(
        "repo".to_string(),
        "git@github.com:Me/Repo.git".to_string(),
    )]);
    let place = nodes::place(myriad_merope::notify::PEER_AGENT_EVENT, &facts).unwrap();
    let note = nodes::Note {
        event: "evt_a".into(),
        happened: "waiting".into(),
        action: "speak".into(),
        line: "它在等你回答。".into(),
        at: crate::clock::now(),
        said: false,
    };
    let decided = nodes::Decided {
        event: "evt_a",
        happened: "waiting",
        action: "speak",
        reason: "test",
        origin: "dev_mac",
        note: Some(note),
    };
    nodes::decided(db, OWNER, &place, decided).await.unwrap();
    // Threads: one grown from their line, one from none.
    let now = crate::services::agent::merope::clock::now();
    line(db, SEVEN, "明天考试，有点慌", 0).await;
    let from = home_intents::origin_of(db, SEVEN, "明天考试，有点慌", now).await;
    assert!(from.is_some());
    let kept = |about: &str, then: &str| threads::Kept {
        about: about.into(),
        then: then.into(),
        due_in_hours: None,
    };
    threads::keep_from(
        db,
        SEVEN,
        &kept("考试", "问问考得怎么样"),
        now,
        from.as_deref(),
    )
    .await;
    threads::keep_from(db, SEVEN, &kept("天气", "提醒带伞"), now, None).await;
    // As it was before her nodes: what writing it put on them is not there.
    sql(
        db,
        "DELETE FROM agent_node_instructions; DELETE FROM agent_node_inbox; \
         DELETE FROM agent_node_placements; DELETE FROM agent_node_heads; \
         DELETE FROM agent_node_rows",
    )
    .await;
    quoted
}

/// Where the model is asked, in order: the group (kept with the site
/// owner), the owner's talk, account 7's.
const ANSWERS: [(&str, &str); 3] = [
    (
        OPERATION,
        r#"{"facts":[{"fact":1,"topic":{"on":"","name":"","aliases":[]}}]}"#,
    ),
    (
        OPERATION,
        r#"{"facts":[{"fact":1,"topic":{"on":"","name":"","aliases":[]}}]}"#,
    ),
    (
        OPERATION,
        r#"{"facts":[{"fact":1,"topic":{"on":"","name":"music","aliases":[]}}]}"#,
    ),
];

#[tokio::test]
async fn moving_once_puts_each_thing_where_her_nodes_keep_it_and_again_writes_nothing() {
    let Some(schema) = owned("memory_nodes_migrate_once").await else {
        return;
    };
    let db = &schema.db;
    let quoted = seed(db).await;
    let before = old_tables(db).await;
    assert_eq!(
        count(db, "SELECT COUNT(*) AS n FROM agent_node_rows").await,
        0
    );

    answering(&ANSWERS);
    let first = moved(db, real(true)).await;
    assert_eq!(asked(OPERATION), 3);
    assert_eq!(first.status, Status::Done, "{:?}", first.note);
    let counts = &first.counts;
    assert_eq!((counts.topics, counts.topics_started), (2, 2));
    assert_eq!(
        (counts.placements, counts.placed, counts.with_message),
        (3, 3, 1)
    );
    assert_eq!((counts.sources, counts.hung), (4, 4));
    assert_eq!((counts.child_notes, counts.child_put), (1, 1));
    assert_eq!((counts.threads, counts.intended, counts.refused), (2, 1, 1));
    assert_eq!((counts.to_sort, counts.asks, counts.sorted), (3, 3, 3));
    assert_eq!(first.estimate.asks, 3);
    assert!(first.spent.tokens > 0 && first.spent.tokens <= 2 * first.estimate.tokens());
    let check = counts.check.as_ref().unwrap();
    assert!(check.whole(), "{check:?}");
    assert_eq!((check.expected, check.matched, check.extra), (3, 3, 0));

    // The site owner's topic keeps its id; account 7's is in its own talk.
    let mine = audience::owners_talk();
    let plants = myriad_nodes::node_id(&mine, PLANTS).unwrap();
    assert_eq!(
        plants,
        myriad_nodes::node_id("personal:local-owner", PLANTS).unwrap()
    );
    let head = read::node(db, OWNER, &mine, &plants)
        .await
        .unwrap()
        .unwrap();
    assert!(head["entries"].is_array());
    let kept = read::heads(db, OWNER, &mine, None).await.unwrap();
    let kept = kept.iter().find(|kept| kept.node_id == plants).unwrap();
    assert_eq!(kept.name.as_deref(), Some("plants"));
    assert_eq!(kept.aliases, ["绿植"]);
    assert_eq!(kept.node_key.as_deref(), Some(PLANTS));
    assert_eq!(
        memories_on(db, OWNER, &mine, PLANTS).await,
        BTreeSet::from(["m1".to_string(), "m2".to_string()]),
        "what was under it, and not what faded"
    );
    let entries = read::entries(db, OWNER, &mine, &plants).await.unwrap();
    let m1 = entries
        .iter()
        .find(|entry| entry.memories == ["m1"])
        .unwrap();
    assert_eq!(
        m1.refs,
        vec!["memory:m1".to_string(), format!("message:{quoted}")]
    );
    assert!(!m1.quote_only);
    assert!(
        entries
            .iter()
            .any(|entry| entry.memories == ["m2"] && entry.quote_only)
    );
    let theirs = audience::private_talk(SEVEN, Some(OWNER));
    assert_eq!(
        memories_on(db, SEVEN, &theirs, DOCTORS).await,
        BTreeSet::from(["m4".to_string()])
    );
    assert!(
        read::node(
            db,
            SEVEN,
            &mine,
            &myriad_nodes::node_id(&mine, DOCTORS).unwrap()
        )
        .await
        .unwrap()
        .is_none()
    );

    // Sorted again: looked at and put under none, kept before topics, a
    // group's.
    let placed =
        |id: &'static str| async move { placements::placement(db, id).await.unwrap().unwrap() };
    assert_eq!(
        (placed("m3").await.how, placed("m3").await.audience),
        (How::About, mine.clone())
    );
    assert_eq!(
        (placed("m5").await.how, placed("m5").await.audience),
        (How::Node, theirs.clone())
    );
    let g1 = placed("g1").await;
    assert_eq!(
        (g1.how, g1.user_id, g1.audience.as_str()),
        (How::About, OWNER, "group:qq:5")
    );
    assert!(
        memories_on(db, SEVEN, &theirs, "person:7")
            .await
            .contains("g1"),
        "their own words, in private too"
    );
    assert!(placements::placement(db, "m_old").await.unwrap().is_none());

    // Her other sources, the child's note, her intent.
    assert!(memories_on(db, SEVEN, &theirs, "her").await.contains("me7"));
    assert!(
        memories_on(db, OWNER, "group:qq:5", "group:qq:5")
            .await
            .contains("bit5")
    );
    let person = read::node(
        db,
        SEVEN,
        &theirs,
        &myriad_nodes::node_id(&theirs, "person:7").unwrap(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        person["views"]["us"].is_object(),
        "her view of what they are to her"
    );
    let repo = read::node(
        db,
        OWNER,
        &mine,
        &myriad_nodes::node_id(&mine, "repo:github.com/me/repo").unwrap(),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(repo["spans"][0]["refs"], json!(["event:evt_a"]));
    let her = read::node(
        db,
        OWNER,
        "own:",
        &myriad_nodes::node_id("own:", "her").unwrap(),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(her["spans"].as_array().map(Vec::len), Some(1));
    let intents = intent_store::held(db, SEVEN, &theirs).await.unwrap();
    assert_eq!(intents.len(), 1, "the thread grown from their line");
    assert!(intents[0].id.starts_with(home_intents::THREAD) && intents[0].waiting());

    // The old tables are as they were.
    assert_eq!(old_tables(db).await, before);

    // Again: nothing new, nothing asked.
    let rows = count(db, "SELECT COUNT(*) AS n FROM agent_node_rows").await;
    let before = old_tables(db).await;
    answering(&[]);
    let again = moved(db, real(true)).await;
    assert_eq!(asked(OPERATION), 0);
    assert_eq!(again.status, Status::Done, "{:?}", again.note);
    assert_eq!(
        count(db, "SELECT COUNT(*) AS n FROM agent_node_rows").await,
        rows
    );
    assert_eq!(again.counts.rows_after, again.counts.rows_before);
    let counts = &again.counts;
    assert_eq!(
        (
            counts.placed,
            counts.hung,
            counts.child_put,
            counts.intended,
            counts.asks
        ),
        (0, 0, 0, 0, 0)
    );
    assert_eq!((counts.topics_kept, counts.placed_before), (2, 3));
    assert!(counts.check.as_ref().unwrap().whole());
    assert_eq!(old_tables(db).await, before);

    // Both runs are on the record.
    let runs = migrate::runs(db, 10).await.unwrap();
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0]["status"], "done");
    assert_eq!(runs[1]["counts"]["placed"], 3);
    assert_eq!(runs[1]["estimate"]["asks"], 3);
    schema.drop().await;
}
