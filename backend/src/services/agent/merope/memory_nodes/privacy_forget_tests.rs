//! Forgetting leaves no byte of what went (P11), on a database of its own
//! (`MYRIAD_MEDIA_TEST_DATABASE_URL`, an isolated schema each): one line of
//! a private talk, what it taught her, what rests on that on her nodes and
//! in her log are written by the paths that write them, then forgotten by
//! the person's own two ways (a line, a memory). Afterwards every column of
//! every table is read for the words, for the digests her vectors are kept
//! under, and her log's payloads are opened: nothing says any of it. What
//! was said next to it stays. Every word here is made up.

use std::collections::{BTreeMap, BTreeSet};

use myriad_persona_log::Row;
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::{Value, json};

use super::read_tests::owned;
use super::{Author, RowIn, audience, inbox, partner, write};
use crate::services::agent::memory::{
    forgotten, manage, meaning,
    unified::{self, Audience, Concept, MemoryKind, NewMemory, Speaker},
};
use crate::services::agent::merope::store::{DIARY_SOURCE_CHAT, insert_diary};
use crate::services::her_log::{
    self, Settings, keys, pen, store, test_partner::PARTNER, test_settings,
};

const OWNER: i32 = 1;
const SEVEN: i32 = 7;
const ROOT: keys::test_root::TestRoot = keys::test_root::TestRoot([9; 32]);

/// In every word that goes, and in no other.
const SECRET: &str = "左肾移植";

/// Every column of every table of this schema that holds `needle`, as
/// `table.column`: any type as Postgres writes it out as text (JSON,
/// arrays, vectors), case aside, and bytes as they are.
pub(crate) async fn holding(db: &DatabaseConnection, needle: &str) -> BTreeSet<String> {
    let columns = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT c.table_name::text AS t, c.column_name::text AS c, c.data_type::text AS d \
             FROM information_schema.columns c \
             JOIN information_schema.tables t \
               ON t.table_schema = c.table_schema AND t.table_name = c.table_name \
             WHERE c.table_schema = current_schema() AND t.table_type = 'BASE TABLE' \
             ORDER BY c.table_name, c.ordinal_position",
        ))
        .await
        .unwrap();
    let mut tables: BTreeMap<String, Vec<(String, bool)>> = BTreeMap::new();
    for column in &columns {
        let table: String = column.try_get("", "t").unwrap();
        let name: String = column.try_get("", "c").unwrap();
        let kind: String = column.try_get("", "d").unwrap();
        tables.entry(table).or_default().push((name, kind == "bytea"));
    }
    assert!(tables.len() > 20, "the schema is migrated: {}", tables.len());
    let quoted = |name: &str| format!("\"{}\"", name.replace('"', "\"\""));
    let pattern = format!(
        "%{}%",
        needle
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    );
    let mut found = BTreeSet::new();
    for (table, columns) in &tables {
        let counts: Vec<String> = columns
            .iter()
            .enumerate()
            .map(|(at, (name, bytes))| {
                let column = quoted(name);
                if *bytes {
                    format!(
                        "COUNT(*) FILTER (WHERE position(convert_to($1::text, 'UTF8') IN {column}) \
                         > 0) AS c{at}"
                    )
                } else {
                    format!("COUNT(*) FILTER (WHERE {column}::text ILIKE $2::text) AS c{at}")
                }
            })
            .collect();
        let sql = format!(
            "SELECT {} FROM {} WHERE $1::text <> '' AND $2::text <> ''",
            counts.join(", "),
            quoted(table)
        );
        let row = db
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                sql,
                [needle.into(), pattern.clone().into()],
            ))
            .await
            .unwrap()
            .unwrap();
        for (at, (name, _)) in columns.iter().enumerate() {
            let held: i64 = row.try_get("", &format!("c{at}")).unwrap();
            if held > 0 {
                found.insert(format!("{table}.{name}"));
            }
        }
    }
    found
}

/// The rows of `user`'s log, in every audience, whose payload opens to
/// something saying `needle`.
pub(crate) async fn log_says(
    db: &DatabaseConnection,
    root: &dyn keys::Root,
    user: i32,
    needle: &str,
) -> Vec<String> {
    let mut saying = Vec::new();
    for audience in store::audiences(db, user).await.unwrap() {
        let rows: Vec<Row> = store::rows(db, user, &audience).await.unwrap();
        let held: Vec<&Row> = rows.iter().collect();
        for (id, body) in store::bodies(db, root, user, &audience, &held)
            .await
            .unwrap()
        {
            if body.to_string().contains(needle) {
                saying.push(id);
            }
        }
    }
    saying
}

async fn count(db: &DatabaseConnection, sql: &str, values: Vec<sea_orm::Value>) -> i64 {
    db.query_one_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        sql,
        values,
    ))
    .await
    .unwrap()
    .unwrap()
    .try_get_by_index::<i64>(0)
    .unwrap()
}

fn row(node: &str, id: &str, kind: &str, body: Value) -> RowIn {
    RowIn {
        id: id.into(),
        node: node.into(),
        kind: kind.into(),
        body,
        author: Author::Her,
    }
}

/// A line of `session`, `minutes_ago`; its id.
async fn line(
    db: &DatabaseConnection,
    session: &str,
    role: &str,
    content: &str,
    minutes_ago: i32,
) -> i32 {
    db.query_one_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_messages (session_id, role, content, created_at) \
         VALUES ($1, $2, $3, NOW() - $4 * INTERVAL '1 minute') RETURNING id",
        [
            session.into(),
            role.into(),
            content.into(),
            minutes_ago.into(),
        ],
    ))
    .await
    .unwrap()
    .unwrap()
    .try_get("", "id")
    .unwrap()
}

/// A memory of `user`'s they told her in private, quoting `evidence`.
async fn memory(
    db: &DatabaseConnection,
    user: i32,
    source: &'static str,
    content: &str,
    evidence: &str,
    concepts: Vec<Concept>,
) -> String {
    unified::remember(
        db,
        NewMemory {
            user_id: user,
            kind: MemoryKind::Fact,
            content: content.into(),
            evidence: Some(evidence.into()),
            speaker: Speaker::User,
            source,
            audience: Audience::private(user),
            importance: 0.5,
            concepts,
        },
    )
    .await
    .unwrap()
    .unwrap()
}

/// What was seeded for one talk: what goes, and what stays beside it.
struct Seeded {
    session: String,
    line: i32,
    answer: i32,
    memory: String,
    earlier: String,
    /// Every word that goes, each kept as said somewhere.
    words: Vec<String>,
    /// Her rows on the node that goes.
    rows: Vec<String>,
    /// What stays: a line, a memory, a row of another node.
    kept_line: i32,
    kept_memory: String,
    kept_row: String,
}

/// Seeds `user`'s private talk `talk`, everything that goes marked with
/// `mark` and [`SECRET`], by the paths that write each: the line and her
/// answer, the chat diary's note of it, the short note and the lasting
/// memory it taught her (with cues, concepts and a vector of each), a
/// vector of the line, a topic on her nodes with an entry of the memory, a
/// span of the line, what rests on the entry (a standing, a view, an open
/// item, an intent of hers), an alias, and the line waiting in her inbox.
/// Beside it a line, a memory and a topic that stay.
async fn seed(db: &DatabaseConnection, user: i32, talk: &str, mark: &str) -> Seeded {
    let session = format!("s-p11-{user}");
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_sessions (id, user_id, title, context, message_count, archived, \
         created_at, last_active_at) \
         VALUES ($1, $2, 'talk', '{\"mode\":\"chat\"}', 4, false, NOW() - INTERVAL '1 hour', NOW())",
        [session.clone().into(), user.into()],
    ))
    .await
    .unwrap();
    let said = format!("{mark}-LINE 我上个月做了{SECRET}手术，别跟任何人说");
    let answered = format!("{mark}-ANSWER {SECRET}的事我记下了，不会说出去");
    let kept_said = "今天把阳台的番茄都摘了";
    let line_id = line(db, &session, "user", &said, 30).await;
    let answer = line(db, &session, "assistant", &answered, 29).await;
    let kept_line = line(db, &session, "user", kept_said, 20).await;
    line(db, &session, "assistant", "番茄熟得正好", 19).await;
    // The chat diary noted it before the memory was kept.
    insert_diary(db, user, &format!("他说：{said}"), DIARY_SOURCE_CHAT)
        .await
        .unwrap();
    let earlier_text = format!("{mark}-NOTE 上个月做了{SECRET}手术");
    let earlier = memory(
        db,
        user,
        unified::SHORT_NOTE_SOURCE,
        &earlier_text,
        &said,
        Vec::new(),
    )
    .await;
    let memory_text = format!("{mark}-MEMORY 做过{SECRET}手术，不想让人知道");
    let concept = Concept {
        name: format!("{mark}-CONCEPT"),
        aliases: vec![SECRET.to_string()],
    };
    let memory_id = memory(db, user, "chat", &memory_text, &said, vec![concept]).await;
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE agent_memories SET invalid_at = NOW(), invalid_reason = 'superseded', \
         superseded_by = $1 WHERE id = $2",
        [memory_id.clone().into(), earlier.clone().into()],
    ))
    .await
    .unwrap();
    let kept_memory = memory(db, user, "chat", "喜欢自己种番茄", kept_said, Vec::new()).await;
    let cue = format!("{mark}-CUE 有人问起{SECRET}时");
    unified::keep_cues(db, &memory_id, &[cue.clone()])
        .await
        .unwrap();
    // Her vectors are kept under a digest of the words: a vector of each,
    // as an embedding model would leave it (no model is asked here).
    let vector: Vec<u8> = [0.25f32, -0.5, 1.0]
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    for (id, text) in [(&memory_id, &memory_text), (&earlier, &earlier_text)] {
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO agent_memory_embeddings (memory_id, model, digest, vector, created_at) \
             VALUES ($1, 'test-embed', $2, $3, NOW())",
            [
                id.clone().into(),
                meaning::digest(text).into(),
                vector.clone().into(),
            ],
        ))
        .await
        .unwrap();
    }
    for (id, text) in [(line_id, &said), (answer, &answered)] {
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO agent_message_embeddings (message_id, model, digest, vector, created_at) \
             VALUES ($1, 'test-embed', $2, $3, NOW())",
            [id.into(), meaning::digest(text).into(), vector.clone().into()],
        ))
        .await
        .unwrap();
    }

    // Her nodes: a topic she started for it, and one that stays.
    let node = myriad_nodes::node_id(talk, "minted:p11-doomed").unwrap();
    let other = myriad_nodes::node_id(talk, "minted:p11-kept").unwrap();
    let rest = |id: &str| format!("entry:{id}");
    let words = [
        ("p11-made", "node.created", "NAME"),
        ("p11-alias", "node.revised", "ALIAS"),
        ("p11-entry", "node.entry", "ENTRY"),
        ("p11-span", "node.span", "SPAN"),
        ("p11-summary", "node.revised", "SUMMARY"),
        ("p11-standing", "node.standing", "STANDING"),
        ("p11-view", "node.view_changed", "VIEW"),
        ("p11-open", "node.open_item", "OPEN"),
        ("p11-hand", "node.intent", "HAND"),
    ];
    let mut rows = Vec::new();
    let mut texts = vec![said.clone(), answered.clone(), earlier_text, memory_text, cue];
    for (id, kind, what) in words {
        let text = format!("{mark}-{what} {SECRET}");
        let body = match (kind, what) {
            ("node.revised", "SUMMARY") => json!({"version": 1, "sync": "shared",
                "saidBy": ["her"], "base": [], "field": "summary",
                "value": [{"text": text, "because": [rest("p11-entry")]}]}),
            ("node.created", _) => json!({"version": 1, "sync": "shared", "saidBy": ["her"],
                "key": "minted:p11-doomed", "kind": "topic", "name": text}),
            ("node.revised", _) => json!({"version": 1, "sync": "shared", "saidBy": ["her"],
                "base": [], "field": "aliases", "value": [text]}),
            ("node.entry", _) => json!({"version": 1, "sync": "shared", "saidBy": ["partner"],
                "audience": talk, "text": text, "refs": [format!("memory:{memory_id}")]}),
            ("node.span", _) => json!({"version": 1, "sync": "shared", "saidBy": ["partner"],
                "audience": talk, "text": text, "refs": [format!("message:{line_id}")]}),
            ("node.standing", _) => json!({"version": 1, "sync": "shared", "saidBy": ["her"],
                "base": [], "item": "p11-health", "text": text, "because": [rest("p11-entry")]}),
            ("node.view_changed", _) => json!({"version": 1, "sync": "shared", "saidBy": ["her"],
                "base": [], "view": "p11-view", "text": text, "because": [rest("p11-entry")]}),
            ("node.open_item", _) => json!({"version": 1, "sync": "shared", "saidBy": ["her"],
                "base": [], "item": "p11-check", "text": text, "because": [rest("p11-entry")]}),
            _ => json!({"version": 1, "sync": "shared", "saidBy": ["her"], "base": [],
                "intent": "p11-ask", "text": text, "because": [rest("p11-entry")],
                "status": "waiting", "origin": "her"}),
        };
        rows.push(row(&node, id, kind, body));
        texts.push(text);
    }
    rows.push(row(
        &other,
        "p11-kept-made",
        "node.created",
        json!({"version": 1, "sync": "shared", "saidBy": ["her"], "key": "minted:p11-kept",
               "kind": "topic", "name": "番茄"}),
    ));
    rows.push(row(
        &other,
        "p11-kept-entry",
        "node.entry",
        json!({"version": 1, "sync": "shared", "saidBy": ["partner"], "audience": talk,
               "text": "自己种番茄", "refs": [format!("memory:{kept_memory}")]}),
    ));
    let thought = pen::think().expect("her mind is here in tests");
    // One at a time: their clocks go up in the order they are given.
    for row in &rows {
        write::write(db, user, talk, std::slice::from_ref(row), Some(&thought))
            .await
            .unwrap();
    }
    drop(thought);
    let inboxed = format!("{mark}-INBOX {SECRET}");
    inbox::add(
        db,
        user,
        talk,
        &[inbox::Item {
            day: Some(crate::clock::now().date_naive()),
            id: "p11-inbox".into(),
            said_by: vec![myriad_nodes::SaidBy::Partner],
            text: inboxed.clone(),
            refs: vec![format!("message:{line_id}")],
            from: None,
            to: None,
        }],
    )
    .await
    .unwrap();
    texts.push(inboxed);
    Seeded {
        session,
        line: line_id,
        answer,
        memory: memory_id,
        earlier,
        words: texts,
        rows: words.iter().map(|(id, ..)| id.to_string()).collect(),
        kept_line,
        kept_memory,
        kept_row: "p11-kept-entry".into(),
    }
}

/// What holds `mark`, [`SECRET`] or the digest of any of `seeded`'s words:
/// by needle, the columns.
async fn residue(
    db: &DatabaseConnection,
    mark: &str,
    seeded: &Seeded,
) -> BTreeMap<String, BTreeSet<String>> {
    let mut needles = vec![mark.to_string(), SECRET.to_string()];
    needles.extend(seeded.words.iter().map(|text| meaning::digest(text)));
    let mut out = BTreeMap::new();
    for needle in needles {
        let held = holding(db, &needle).await;
        if !held.is_empty() {
            out.insert(needle, held);
        }
    }
    out
}

/// Before anything goes: the words are where each path keeps them, so a
/// sweep that finds nothing afterwards found it gone.
async fn there_before(db: &DatabaseConnection, mark: &str, seeded: &Seeded) {
    let held = holding(db, mark).await;
    for column in [
        "agent_messages.content",
        "agent_diary.content",
        "agent_memories.content",
        "agent_memories.evidence",
        "agent_memories.concepts",
        "agent_memory_cues.cues",
        "agent_node_rows.body",
        "agent_node_heads.name",
        "agent_node_heads.aliases",
        "agent_node_heads.node",
        "agent_node_inbox.item",
    ] {
        assert!(held.contains(column), "{column} holds it first: {held:#?}");
    }
    let memory_digest = holding(db, &meaning::digest(&seeded.words[3])).await;
    assert!(
        memory_digest.contains("agent_memory_embeddings.digest"),
        "{memory_digest:?}"
    );
    let line_digest = holding(db, &meaning::digest(&seeded.words[0])).await;
    assert!(
        line_digest.contains("agent_message_embeddings.digest"),
        "{line_digest:?}"
    );
}

/// After: nothing anywhere, every row on the node that went a stub (no
/// body, marked forgotten, no payload), kept as forgotten by mark only, and
/// what was beside it as it was.
async fn gone_after(db: &DatabaseConnection, user: i32, mark: &str, seeded: &Seeded) {
    let left = residue(db, mark, seeded).await;
    assert!(left.is_empty(), "forgetting left words behind: {left:#?}");
    let not_stubs = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT row_id FROM agent_node_rows WHERE user_id = $1 AND row_id = ANY($2) \
               AND (body <> '' OR NOT COALESCE((envelope->>'forgotten')::boolean, false) \
                    OR envelope ? 'payload')",
            [user.into(), seeded.rows.clone().into()],
        ))
        .await
        .unwrap();
    let not_stubs: Vec<String> = not_stubs
        .iter()
        .map(|row| row.try_get("", "row_id").unwrap())
        .collect();
    assert!(not_stubs.is_empty(), "not stubs: {not_stubs:?}");
    assert_eq!(
        count(
            db,
            "SELECT COUNT(*) FROM agent_node_rows WHERE user_id = $1 AND row_id = ANY($2)",
            vec![user.into(), seeded.rows.clone().into()],
        )
        .await,
        seeded.rows.len() as i64,
        "each keeps its place in the log as a stub"
    );
    // Kept as forgotten so no copy brings it back, by its id only.
    let marks = forgotten::among(
        db,
        user,
        forgotten::MEMORY,
        &[seeded.memory.clone(), seeded.earlier.clone()],
    )
    .await
    .unwrap();
    assert_eq!(marks.len(), 2, "{marks:?}");
    // Their words are gone with what they taught her; the rest stays.
    for id in [seeded.line, seeded.answer] {
        assert_eq!(
            count(
                db,
                "SELECT COUNT(*) FROM agent_messages WHERE id = $1",
                vec![id.into()]
            )
            .await,
            0,
            "line {id}"
        );
    }
    assert_eq!(
        count(
            db,
            "SELECT COUNT(*) FROM agent_messages WHERE id = $1",
            vec![seeded.kept_line.into()]
        )
        .await,
        1,
        "the line beside it stays"
    );
    assert_eq!(
        count(
            db,
            "SELECT COUNT(*) FROM agent_memories WHERE id = $1",
            vec![seeded.kept_memory.clone().into()]
        )
        .await,
        1,
        "the memory beside it stays"
    );
    assert_eq!(
        count(
            db,
            "SELECT COUNT(*) FROM agent_node_rows WHERE user_id = $1 AND row_id = $2 \
             AND body LIKE '%番茄%'",
            vec![user.into(), seeded.kept_row.clone().into()]
        )
        .await,
        1,
        "the topic beside it stays"
    );
    assert_eq!(
        count(
            db,
            "SELECT COUNT(*) FROM agent_messages WHERE session_id = $1",
            vec![seeded.session.clone().into()]
        )
        .await,
        2
    );
}

/// The site owner's talk while she syncs her log: their line forgotten
/// (`manage::forget_line`) takes it, her answer, the short note and memory
/// it taught her and all that rests on them, here and in her log, where
/// what is left opens to nothing of it and the ledger holds each forget.
#[tokio::test]
async fn a_forgotten_line_leaves_no_byte_of_itself_anywhere_nor_in_her_log() {
    let Some(schema) = owned("p11_residue_line").await else {
        return;
    };
    let db = &schema.db;
    test_settings::set(Settings {
        enabled: true,
        ..Settings::default()
    });
    keys::test_root::use_here(ROOT);
    her_log::test_partner::give(db, OWNER, PARTNER).await;
    partner::remember(Some(PARTNER));
    store::start(db, OWNER).await.unwrap();
    let talk = audience::owners_talk();
    assert_eq!(talk, format!("personal:{PARTNER}"));
    let mark = "QZXRESIDUELINE";
    let seeded = seed(db, OWNER, &talk, mark).await;
    // Their line in her log, as the site keeps one; her records compared
    // into it (the memory, its note).
    let at = crate::clock::now() - chrono::Duration::minutes(30);
    let logged = her_log::talk::keep(
        db,
        &ROOT,
        OWNER,
        &seeded.session,
        seeded.line,
        &seeded.words[0],
        at,
        "web",
    )
    .await
    .unwrap()
    .expect("the owner's private line goes into her log");
    her_log::state::sync(db, &ROOT, OWNER).await.unwrap();
    there_before(db, mark, &seeded).await;
    let opened = log_says(db, &ROOT, OWNER, mark).await;
    assert!(
        opened.contains(&logged.id) && opened.len() >= 4,
        "her log holds the line, the memory and her rows: {opened:?}"
    );

    let gone = manage::forget_line(db, OWNER, &seeded.session, seeded.line)
        .await
        .unwrap()
        .expect("the line was there");
    let gone: BTreeSet<i32> = gone.iter().map(|line| line.id).collect();
    assert!(
        gone.contains(&seeded.line) && gone.contains(&seeded.answer),
        "{gone:?}"
    );

    gone_after(db, OWNER, mark, &seeded).await;
    // Her log: nothing that is left opens to any of it, no payload is kept
    // for a row forgotten, and every forget is on the ledger for her other
    // ends to take.
    assert_eq!(log_says(db, &ROOT, OWNER, mark).await, Vec::<String>::new());
    assert_eq!(
        log_says(db, &ROOT, OWNER, SECRET).await,
        Vec::<String>::new()
    );
    assert_eq!(
        count(
            db,
            "SELECT COUNT(*) FROM agent_log_payloads p JOIN agent_log_rows r \
             ON r.user_id = p.user_id AND r.id = p.row_id WHERE r.forgotten",
            Vec::new()
        )
        .await,
        0
    );
    let ledger: BTreeSet<String> = store::ledger(db, OWNER, &talk)
        .await
        .unwrap()
        .into_iter()
        .map(|entry| entry.target)
        .collect();
    for target in [
        format!("row:{}", logged.id),
        format!("memory:{}", seeded.memory),
    ] {
        assert!(ledger.contains(&target), "{target} on {ledger:?}");
    }
    assert_eq!(
        count(
            db,
            "SELECT COUNT(*) FROM agent_her_state_rows WHERE object = ANY($1)",
            vec![
                vec![
                    format!("memory:{}", seeded.memory),
                    format!("memory:{}", seeded.earlier)
                ]
                .into()
            ]
        )
        .await,
        0
    );
    // The line kept as forgotten, by a digest that is not of its words.
    assert!(
        !forgotten::among(
            db,
            OWNER,
            forgotten::MESSAGE,
            &[seeded.line.to_string()]
        )
        .await
        .unwrap()
        .is_empty()
    );
    schema.drop().await;
}

/// Someone else's talk, her log not reaching it: the memory forgotten
/// (`manage::forget_any`) takes its earlier version, the line it came from
/// with her answer and the diary's note of it, its vectors and cues, and
/// all that rests on it on her nodes; nothing is left anywhere.
#[tokio::test]
async fn a_forgotten_memory_leaves_no_byte_of_itself_or_its_line_anywhere() {
    let Some(schema) = owned("p11_residue_memory").await else {
        return;
    };
    let db = &schema.db;
    let talk = audience::private_talk(SEVEN, Some(OWNER));
    let mark = "QZXRESIDUEMEMORY";
    let seeded = seed(db, SEVEN, &talk, mark).await;
    there_before(db, mark, &seeded).await;

    assert!(manage::forget_any(db, &seeded.memory).await.unwrap());

    gone_after(db, SEVEN, mark, &seeded).await;
    assert_eq!(
        count(db, "SELECT COUNT(*) FROM agent_log_rows", Vec::new()).await,
        0,
        "her log was never on"
    );
    schema.drop().await;
}
