//! Her intents resting on what is forgotten go with it
//! ([`super::intent_grounds`]), by each path something is forgotten here,
//! on a database of their own (`MYRIAD_MEDIA_TEST_DATABASE_URL`, an isolated
//! schema each). Every word here is made up.

use std::collections::BTreeSet;

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use myriad_nodes::wire::{Batch, Carried, Withheld};
use myriad_persona_log::{
    Row,
    row::{Author as By, LOG_VERSION, Payload, Replicate},
};
use myriad_persona_sync::{Rev, canonical::canonical_bytes};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};
use serde_json::{Value, json};

use super::read_tests::owned;
use super::talk_tests::{OTHER, count, schema, talk};
use super::{Author, RowIn, audience, child, forget, logged, page_edit, partner, write};
use crate::services::agent::memory::{forgotten, manage};
use crate::services::agent::merope::nodes::Form;
use crate::services::her_log::{
    self, Owner, Settings, keys, pen, store, test_partner::PARTNER, test_settings,
};

const OWNER: i32 = 1;

fn row(node: &str, id: &str, kind: &str, body: Value) -> RowIn {
    RowIn {
        id: id.into(),
        node: node.into(),
        kind: kind.into(),
        body,
        author: Author::Her,
    }
}

fn created(audience: &str, key: &str, name: &str) -> RowIn {
    let node = myriad_nodes::node_id(audience, key).unwrap();
    row(
        &node,
        &format!("made-{key}").replace(':', "-"),
        "node.created",
        json!({"version": 1, "sync": "shared", "saidBy": ["her"], "key": key, "kind": "topic",
               "name": name}),
    )
}

fn span_body(audience: &str, text: &str, refs: &[&str]) -> Value {
    let said_by = if audience == audience::OWN {
        "her"
    } else {
        "partner"
    };
    json!({"version": 1, "sync": "shared", "saidBy": [said_by], "audience": audience,
           "text": text, "refs": refs})
}

fn span(audience: &str, node: &str, id: &str, refs: &[&str]) -> RowIn {
    row(
        node,
        id,
        "node.span",
        span_body(audience, "they watered the fern", refs),
    )
}

fn entry(audience: &str, node: &str, id: &str, refs: &[&str]) -> RowIn {
    row(
        node,
        id,
        "node.entry",
        span_body(audience, &format!("has a fern ({id})"), refs),
    )
}

fn intent_body(intent: &str, text: &str, because: &[&str], base: &[&str], status: &str) -> Value {
    json!({"version": 1, "sync": "shared", "saidBy": ["her"], "base": base, "intent": intent,
           "text": text, "because": because, "status": status, "origin": "her"})
}

/// A hand of hers.
fn hand(
    node: &str,
    id: &str,
    intent: &str,
    text: &str,
    because: &[&str],
    base: &[&str],
    status: &str,
) -> RowIn {
    row(
        node,
        id,
        "node.intent",
        intent_body(intent, text, because, base, status),
    )
}

/// An instruction of theirs.
fn told(node: &str, id: &str, because: &[&str]) -> RowIn {
    let mut body = intent_body(id, "water it on Friday", because, &[], "waiting");
    body["saidBy"] = json!(["partner"]);
    body["origin"] = json!("partner");
    row(node, id, "node.intent", body)
}

async fn put(db: &DatabaseConnection, user: i32, audience: &str, rows: &[RowIn]) {
    let thought = pen::think().expect("her mind is here in tests");
    // One at a time: their clocks go up in the order they are given.
    for row in rows {
        write::write(
            db,
            user,
            audience,
            std::slice::from_ref(row),
            Some(&thought),
        )
        .await
        .unwrap();
    }
}

/// The intent rows that still say anything.
async fn left(db: &DatabaseConnection) -> BTreeSet<String> {
    db.query_all_raw(Statement::from_string(
        DatabaseBackend::Postgres,
        "SELECT row_id FROM agent_node_rows WHERE kind = 'node.intent' AND body <> ''",
    ))
    .await
    .unwrap()
    .iter()
    .map(|row| row.try_get("", "row_id").unwrap())
    .collect()
}

/// What was forgotten, by object, in the order it was written.
async fn forgets(db: &DatabaseConnection) -> Vec<String> {
    let mut rows: Vec<Row> = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT envelope FROM agent_node_rows WHERE kind = 'forget'",
        ))
        .await
        .unwrap()
        .iter()
        .map(|row| serde_json::from_value(row.try_get("", "envelope").unwrap()).unwrap())
        .collect();
    rows.sort_by(|a, b| a.hlc.cmp(&b.hlc));
    rows.into_iter().map(|row| row.object).collect()
}

fn ids(ids: &[&str]) -> BTreeSet<String> {
    ids.iter().map(|id| id.to_string()).collect()
}

/// A line forgotten takes the span it was told in; what rests on the span
/// goes: a hand with a version that stands resting on it whole, the one
/// she let go of as well; only its older version when what stands rests
/// on something else; and the words carried on elsewhere on other grounds.
/// What they asked of her stays, whatever span it names. The span's own
/// forget is written before any of theirs.
#[tokio::test]
async fn a_line_forgotten_takes_what_rests_on_the_span_it_was_told_in() {
    let Some(schema) = schema("resting_on_line").await else {
        return;
    };
    let db = &schema.db;
    pen::reset_for_test();
    let talk = talk();
    let made = created(&talk, "minted:ferns", "ferns");
    let other = created(&talk, "minted:trips", "trips");
    let (node, away) = (made.node.clone(), other.node.clone());
    let ask = "ask how the fern is doing";
    put(
        db,
        OTHER,
        &talk,
        &[
            made,
            other,
            span(&talk, &node, "sp1", &["message:501"]),
            span(&talk, &node, "sp2", &["message:502"]),
            hand(&node, "let1", "let", ask, &["span:sp1"], &[], "waiting"),
            hand(
                &node,
                "let2",
                "let",
                ask,
                &["span:sp1"],
                &["let1"],
                "dropped",
            ),
            hand(
                &node,
                "old1",
                "old",
                "ask about the pot",
                &["span:sp1"],
                &[],
                "waiting",
            ),
            hand(
                &node,
                "old2",
                "old",
                "ask about Friday",
                &["span:sp2"],
                &["old1"],
                "waiting",
            ),
            hand(&away, "copy1", "let", ask, &["span:sp2"], &[], "waiting"),
            hand(
                &node,
                "stays",
                "stays",
                "ask about tea",
                &["span:sp2"],
                &[],
                "waiting",
            ),
            told(&node, "theirs", &["message:502", "span:sp1"]),
        ],
    )
    .await;
    assert_eq!(left(db).await.len(), 7);
    let txn = db.begin().await.unwrap();
    forget::forget(&txn, &[], &[501]).await.unwrap();
    txn.commit().await.unwrap();
    assert_eq!(left(db).await, ids(&["old2", "stays", "theirs"]));
    let forgets = forgets(db).await;
    assert_eq!(forgets[0], "row:sp1", "{forgets:?}");
    assert_eq!(
        forgets[1..].iter().cloned().collect::<BTreeSet<_>>(),
        ids(&["row:let1", "row:let2", "row:old1", "row:copy1"])
    );
    assert_eq!(forgets.len(), 5, "each once");
    // Nothing of it stands on her nodes either.
    let heads = "SELECT COUNT(*) AS n FROM agent_node_heads WHERE node::text LIKE";
    assert_eq!(count(db, &format!("{heads} '%{ask}%'")).await, 0);
    assert_eq!(
        count(db, &format!("{heads} '%ask about the pot%'")).await,
        0
    );
    assert_eq!(count(db, &format!("{heads} '%ask about Friday%'")).await, 1);
    schema.drop().await;
}

/// A memory or a line forgotten takes what names it: whole when what
/// stands does, in the same words on other grounds too; only the older
/// version when what stands says something else; theirs too.
#[tokio::test]
async fn a_memory_forgotten_takes_the_words_kept_on_other_grounds() {
    let Some(schema) = schema("resting_on_memory").await else {
        return;
    };
    let db = &schema.db;
    pen::reset_for_test();
    let talk = talk();
    let made = created(&talk, "minted:ferns", "ferns");
    let node = made.node.clone();
    db.execute_unprepared(
        "INSERT INTO agent_memories (id, user_id, kind, content, source, venue, audience, \
         valid_from, created_at, updated_at) VALUES \
         ('m1', 7, 'fact', 'has a fern', 'chat', 'private', '[7]'::jsonb, now(), now(), now())",
    )
    .await
    .unwrap();
    put(
        db,
        OTHER,
        &talk,
        &[
            made,
            hand(
                &node,
                "let1",
                "let",
                "ask about it",
                &["memory:m1"],
                &[],
                "waiting",
            ),
            hand(
                &node,
                "let2",
                "let",
                "ask about it",
                &["message:502"],
                &["let1"],
                "dropped",
            ),
            hand(
                &node,
                "old1",
                "old",
                "ask when",
                &["memory:m1"],
                &[],
                "waiting",
            ),
            hand(
                &node,
                "old2",
                "old",
                "ask where",
                &["message:502"],
                &["old1"],
                "waiting",
            ),
            told(&node, "theirs", &["memory:m1"]),
            told(&node, "said", &["message:502"]),
        ],
    )
    .await;
    let txn = db.begin().await.unwrap();
    forget::forget(&txn, &["m1".into()], &[]).await.unwrap();
    txn.commit().await.unwrap();
    assert_eq!(left(db).await, ids(&["old2", "said"]));
    schema.drop().await;
}

/// The owner deletes an entry or a node on the page: what rests on what
/// went, wherever it is, goes; an intent of the node itself goes with the
/// node, by no forget of its own.
#[tokio::test]
async fn what_the_owner_deletes_on_the_page_takes_what_rests_on_it() {
    let Some(schema) = owned("resting_on_page").await else {
        return;
    };
    let db = &schema.db;
    pen::reset_for_test();
    let talk = audience::owners_talk();
    let made = created(&talk, "minted:ferns", "ferns");
    let other = created(&talk, "minted:trips", "trips");
    let (node, away) = (made.node.clone(), other.node.clone());
    put(
        db,
        OWNER,
        &talk,
        &[
            made,
            other,
            entry(&talk, &node, "en1", &["record:r1"]),
            entry(&talk, &node, "en2", &["record:r2"]),
            span(&talk, &node, "sp1", &["record:r3"]),
            hand(
                &away,
                "on-en1",
                "a",
                "ask one",
                &["entry:en1"],
                &[],
                "waiting",
            ),
            hand(
                &away,
                "on-en2",
                "b",
                "ask two",
                &["entry:en2"],
                &[],
                "waiting",
            ),
            hand(
                &away,
                "on-sp1",
                "c",
                "ask three",
                &["span:sp1"],
                &[],
                "dropped",
            ),
            hand(&node, "own", "d", "ask four", &["span:sp1"], &[], "waiting"),
            hand(
                &away,
                "stays",
                "e",
                "ask five",
                &["message:502"],
                &[],
                "waiting",
            ),
        ],
    )
    .await;
    page_edit::forget_entry(db, OWNER, &talk, &node, "en1")
        .await
        .unwrap();
    assert_eq!(left(db).await, ids(&["on-en2", "on-sp1", "own", "stays"]));
    assert_eq!(forgets(db).await, ["row:en1", "row:on-en1"]);
    page_edit::forget_node(db, OWNER, &talk, &node)
        .await
        .unwrap();
    assert_eq!(left(db).await, ids(&["stays"]));
    let forgets = forgets(db).await;
    assert_eq!(forgets[2], format!("node:{node}"));
    assert_eq!(
        forgets[3..].iter().cloned().collect::<BTreeSet<_>>(),
        ids(&["row:on-en2", "row:on-sp1"])
    );
    schema.drop().await;
}

/// A child's node forgotten whole, and a removed device's nodes: what rests
/// on a span of theirs goes.
#[tokio::test]
async fn a_node_forgotten_whole_takes_what_rests_on_its_spans() {
    let Some(schema) = owned("resting_on_whole").await else {
        return;
    };
    let db = &schema.db;
    pen::reset_for_test();
    let talk = audience::owners_talk();
    let repo = "github.com/me/repo";
    let key = child::key_of(Form::Repo, repo).unwrap();
    let made = created(&talk, &key, "repo");
    let folder = created(&talk, "minted:folder", "folder");
    let other = created(&talk, "minted:trips", "trips");
    let (node, held, away) = (made.node.clone(), folder.node.clone(), other.node.clone());
    let mut made = made;
    made.id = "made-repo".into();
    put(
        db,
        OWNER,
        &talk,
        &[
            made,
            folder,
            other,
            span(&talk, &node, "sp1", &["record:r1"]),
            span(&talk, &held, "sp2", &["record:r2"]),
            hand(
                &away,
                "on-repo",
                "a",
                "ask one",
                &["span:sp1"],
                &[],
                "waiting",
            ),
            hand(
                &away,
                "on-folder",
                "b",
                "ask two",
                &["span:sp2"],
                &[],
                "waiting",
            ),
            hand(
                &away,
                "stays",
                "c",
                "ask three",
                &["message:502"],
                &[],
                "waiting",
            ),
        ],
    )
    .await;
    let site_owner = super::hanging::site_owner(db).await;
    assert!(
        child::forget_node(db, OWNER, site_owner, Form::Repo, repo)
            .await
            .unwrap()
    );
    assert_eq!(left(db).await, ids(&["on-folder", "stays"]));
    let txn = db.begin().await.unwrap();
    forget::forget_nodes(&txn, OWNER, &talk, &[held.clone()].into_iter().collect())
        .await
        .unwrap();
    txn.commit().await.unwrap();
    assert_eq!(left(db).await, ids(&["stays"]));
    assert_eq!(
        forgets(db).await,
        [
            format!("node:{node}"),
            "row:on-repo".into(),
            format!("node:{held}"),
            "row:on-folder".into(),
        ]
    );
    schema.drop().await;
}

/// A grant removed takes the rows naming it; what rests on a span among
/// them, or on one of a node that went whole, goes too.
#[tokio::test]
async fn a_grant_removed_takes_what_rests_on_what_it_told() {
    let Some(schema) = schema("resting_on_grant").await else {
        return;
    };
    let db = &schema.db;
    pen::reset_for_test();
    let talk = talk();
    let made = created(&talk, "minted:ferns", "ferns");
    let folder = created(&talk, "minted:folder", "folder");
    let (node, held) = (made.node.clone(), folder.node.clone());
    let grant = myriad_nodes::sessions::grant_ref("mac", "g_removed");
    put(
        db,
        OTHER,
        &talk,
        &[
            made,
            folder,
            span(&talk, &node, "sp1", &[&grant]),
            span(&talk, &held, "sp2", &[&grant]),
            span(&talk, &node, "sp3", &["message:502"]),
            hand(
                &node,
                "on-one",
                "a",
                "ask one",
                &["span:sp1"],
                &[],
                "waiting",
            ),
            hand(
                &node,
                "on-whole",
                "b",
                "ask two",
                &["span:sp2"],
                &[],
                "dropped",
            ),
            hand(
                &node,
                "stays",
                "c",
                "ask three",
                &["span:sp3"],
                &[],
                "waiting",
            ),
        ],
    )
    .await;
    let txn = db.begin().await.unwrap();
    forget::forget_named(&txn, OTHER, &talk, &grant, |key| key == "minted:folder")
        .await
        .unwrap();
    txn.commit().await.unwrap();
    assert_eq!(left(db).await, ids(&["stays"]));
    let forgets = forgets(db).await;
    assert_eq!(
        forgets[..2].iter().cloned().collect::<BTreeSet<_>>(),
        ids(&["row:sp1", &format!("node:{held}")])
    );
    assert_eq!(
        forgets[2..].iter().cloned().collect::<BTreeSet<_>>(),
        ids(&["row:on-one", "row:on-whole"])
    );
    schema.drop().await;
}

// What other ends write and forget.

/// The owner's computer, as a side of her log.
struct Mac {
    clock: Option<Rev>,
    count: u64,
}

struct Here {
    schema: crate::db::IsolatedSchema,
    owner: Owner,
    talk: String,
    mac: Mac,
}

const ROOT: keys::test_root::TestRoot = keys::test_root::TestRoot([9; 32]);

/// The site owner's talk, in her log.
async fn here(prefix: &str) -> Option<Here> {
    let schema = owned(prefix).await?;
    let db = &schema.db;
    test_settings::set(Settings {
        enabled: true,
        ..Settings::default()
    });
    keys::test_root::use_here(keys::test_root::TestRoot([9; 32]));
    her_log::test_partner::give(db, OWNER, PARTNER).await;
    partner::remember(Some(PARTNER));
    let talk = audience::owners_talk();
    store::start(db, OWNER).await.unwrap();
    let owner = her_log::owner(db, OWNER).await.unwrap();
    Some(Here {
        schema,
        owner,
        talk,
        mac: Mac {
            clock: None,
            count: 0,
        },
    })
}

impl Here {
    fn db(&self) -> &DatabaseConnection {
        &self.schema.db
    }

    /// A row the computer writes: its envelope and what its payload holds.
    fn by_mac(
        &mut self,
        id: &str,
        object: &str,
        kind: &str,
        body: Option<Value>,
    ) -> (Row, Option<Value>) {
        let now = crate::clock::now().timestamp_micros();
        let hlc =
            myriad_persona_log::clock::next(self.mac.clock.as_ref(), now, 0, "dev_mac").unwrap();
        self.mac.clock = Some(hlc.clone());
        self.mac.count += 1;
        let row = Row {
            version: LOG_VERSION,
            id: id.into(),
            origin: "dev_mac".into(),
            author: By::Owner,
            epoch: None,
            dseq: Some(self.mac.count),
            hlc,
            audience: self.talk.clone(),
            subject: None,
            object: object.into(),
            kind: kind.into(),
            payload: body.as_ref().map(|_| Payload {
                reference: format!("blob:{id}"),
                key: None,
            }),
            replicate: Replicate::Shared,
            forgotten: false,
            to: None,
            takes: Vec::new(),
        };
        row.check().unwrap();
        (row, body)
    }

    fn intent(&mut self, node: &str, id: &str, body: Value) -> (Row, Option<Value>) {
        self.by_mac(id, &format!("node:{node}"), "node.intent", Some(body))
    }

    fn forget(&mut self, object: &str) -> (Row, Option<Value>) {
        let id = format!("forget-{object}").replace(':', "-");
        self.by_mac(&id, object, "forget", None)
    }

    /// The computer sends `rows` (those in `bare` without their body), and
    /// the site takes them into her log and her nodes.
    async fn sent(&self, rows: Vec<(Row, Option<Value>)>, bare: &[&str]) {
        let carried = rows
            .iter()
            .filter(|(row, _)| !bare.contains(&row.id.as_str()))
            .filter_map(|(row, body)| {
                Some(Carried {
                    reference: row.payload.as_ref()?.reference.clone(),
                    data: BASE64.encode(canonical_bytes(body.as_ref()?)),
                })
            })
            .collect();
        let asked: BTreeSet<String> = rows.iter().map(|(row, _)| row.id.clone()).collect();
        let batch = Batch {
            audience: self.talk.clone(),
            withheld: bare
                .iter()
                .map(|id| Withheld {
                    id: id.to_string(),
                    why: myriad_nodes::wire::Why::NoBody,
                })
                .collect(),
            rows: rows.into_iter().map(|(row, _)| row).collect(),
            payloads: carried,
            more: None,
        };
        let now = crate::clock::now().timestamp_micros() + 1_000_000;
        let taken = store::take(self.db(), &ROOT, &self.owner, "dev_mac", batch, &asked, now)
            .await
            .unwrap();
        assert!(taken.not_taken.is_empty(), "{:?}", taken.not_taken);
        logged::take_in(self.db(), &ROOT, &self.owner)
            .await
            .unwrap();
    }

    /// Her log's forgets, by object, in the site's own order.
    async fn logged_forgets(&self) -> Vec<String> {
        let mut rows: Vec<Row> = store::rows(self.db(), OWNER, &self.talk)
            .await
            .unwrap()
            .into_iter()
            .filter(|row| row.kind == "forget" && row.origin == her_log::NODE)
            .collect();
        rows.sort_by_key(|row| row.dseq);
        rows.into_iter().map(|row| row.object).collect()
    }
}

/// A span another end forgot: what rests on it here goes, and what its
/// words were carried on as; and the site tells the others of those rows.
#[tokio::test]
async fn a_span_another_end_forgot_takes_what_rests_on_it_here() {
    let Some(mut here) = here("resting_on_take_in").await else {
        return;
    };
    let talk = here.talk.clone();
    let made = created(&talk, "minted:ferns", "ferns");
    let node = made.node.clone();
    put(
        here.db(),
        OWNER,
        &talk,
        &[
            made,
            span(&talk, &node, "sp1", &["record:r1"]),
            span(&talk, &node, "sp2", &["record:r2"]),
            hand(
                &node,
                "let1",
                "let",
                "ask about it",
                &["span:sp1"],
                &[],
                "waiting",
            ),
            hand(
                &node,
                "let2",
                "let",
                "ask about it",
                &["span:sp2"],
                &["let1"],
                "dropped",
            ),
            // Its own row is forgotten there with the span; the words stay
            // here on other grounds.
            hand(
                &node,
                "gone1",
                "gone",
                "ask again",
                &["span:sp1"],
                &[],
                "waiting",
            ),
            hand(
                &node,
                "kept2",
                "gone",
                "ask again",
                &["span:sp2"],
                &["gone1"],
                "waiting",
            ),
            hand(
                &node,
                "stays",
                "stays",
                "ask about tea",
                &["span:sp2"],
                &[],
                "waiting",
            ),
            told(&node, "theirs", &["span:sp1"]),
        ],
    )
    .await;
    let away = vec![here.forget("row:sp1"), here.forget("row:gone1")];
    here.sent(away, &[]).await;
    assert_eq!(left(here.db()).await, ids(&["stays", "theirs"]));
    assert_eq!(
        here.logged_forgets()
            .await
            .into_iter()
            .collect::<BTreeSet<_>>(),
        ids(&["row:let1", "row:let2", "row:kept2"])
    );
    // Once: nothing more the next time.
    logged::take_in(here.db(), &ROOT, &here.owner)
        .await
        .unwrap();
    assert_eq!(here.logged_forgets().await.len(), 3);
    here.schema.drop().await;
}

/// A whole node another end forgot takes what rests on its spans, on
/// another node.
#[tokio::test]
async fn a_node_another_end_forgot_takes_what_rests_on_its_spans() {
    let Some(mut here) = here("resting_on_take_in_node").await else {
        return;
    };
    let talk = here.talk.clone();
    let made = created(&talk, "minted:ferns", "ferns");
    let other = created(&talk, "minted:trips", "trips");
    let (node, away) = (made.node.clone(), other.node.clone());
    put(
        here.db(),
        OWNER,
        &talk,
        &[
            made,
            other,
            span(&talk, &node, "sp1", &["record:r1"]),
            hand(&away, "on", "a", "ask one", &["span:sp1"], &[], "waiting"),
            hand(
                &away,
                "stays",
                "b",
                "ask two",
                &["message:502"],
                &[],
                "waiting",
            ),
        ],
    )
    .await;
    let gone = vec![here.forget(&format!("node:{node}"))];
    here.sent(gone, &[]).await;
    assert_eq!(left(here.db()).await, ids(&["stays"]));
    assert_eq!(here.logged_forgets().await, ["row:on"]);
    here.schema.drop().await;
}

/// An intent that comes after what it rests on was forgotten here (a span, a
/// memory, a line), or whose body does, goes as it comes, with the versions
/// of it that are here already. One here from before that nothing came of
/// stays as it was.
#[tokio::test]
async fn what_comes_late_is_looked_at_again() {
    let Some(mut here) = here("resting_on_late").await else {
        return;
    };
    let talk = here.talk.clone();
    let made = created(&talk, "minted:ferns", "ferns");
    let node = made.node.clone();
    put(
        here.db(),
        OWNER,
        &talk,
        &[
            made,
            span(&talk, &node, "sp1", &["record:r1"]),
            span(&talk, &node, "sp2", &["record:r2"]),
        ],
    )
    .await;
    // One whose body is still to come as the span goes.
    let bodied = here.intent(
        &node,
        "late-body",
        intent_body("w", "ask zero", &["span:sp1"], &[], "waiting"),
    );
    here.sent(vec![bodied.clone()], &["late-body"]).await;
    assert!(left(here.db()).await.is_empty());
    // A line and a memory forgotten here.
    here.db()
        .execute_unprepared(
            "INSERT INTO agent_sessions (id, user_id, title, context, message_count, archived, \
             created_at, last_active_at) \
             VALUES ('s_one', 1, 't', '{\"mode\":\"chat\"}', 1, false, NOW(), NOW()); \
             INSERT INTO agent_messages (id, session_id, role, content, created_at) \
             VALUES (501, 's_one', 'user', 'the fern is dry', NOW()), \
                    (502, 's_one', 'user', 'tea is ready', NOW())",
        )
        .await
        .unwrap();
    manage::forget_line(here.db(), OWNER, "s_one", 501)
        .await
        .unwrap();
    forgotten::keep(here.db(), OWNER, forgotten::MEMORY, &["m1".into()])
        .await
        .unwrap();
    // The span, and an intent of hers on it that was here before this rule.
    let before = hand(
        &node,
        "before",
        "old",
        "ask before",
        &["span:sp1"],
        &[],
        "waiting",
    );
    put(here.db(), OWNER, &talk, &[before]).await;
    let (gone, _) = here.forget("row:sp1");
    let batch = Batch {
        audience: talk.clone(),
        rows: vec![gone],
        payloads: Vec::new(),
        withheld: Vec::new(),
        more: None,
    };
    store::take(
        here.db(),
        &ROOT,
        &here.owner,
        "dev_mac",
        batch,
        &BTreeSet::new(),
        crate::clock::now().timestamp_micros() + 1_000_000,
    )
    .await
    .unwrap();
    // As an older build of the site took it in: the span's stub, no more.
    here.db()
        .execute_unprepared(
            "UPDATE agent_node_rows n SET body = '', envelope = l.envelope FROM agent_log_rows l \
             WHERE l.user_id = n.user_id AND l.id = n.row_id AND n.row_id = 'sp1'",
        )
        .await
        .unwrap();

    here.sent(vec![bodied], &[]).await;
    assert_eq!(
        left(here.db()).await,
        ids(&["before"]),
        "a body that came; nothing came of the one before"
    );
    // What it says: one carried on in the same words, an older one it
    // is written over, one that stands on other words.
    let late = vec![
        here.intent(
            &node,
            "on-span",
            intent_body("a", "ask one", &["span:sp1"], &[], "waiting"),
        ),
        here.intent(
            &node,
            "on-span2",
            intent_body("a", "ask one", &["span:sp2"], &["on-span"], "dropped"),
        ),
        here.intent(
            &node,
            "on-memory",
            intent_body("b", "ask two", &["memory:m1"], &[], "waiting"),
        ),
        here.intent(
            &node,
            "on-line",
            intent_body("c", "ask three", &["message:501"], &[], "waiting"),
        ),
        here.intent(
            &node,
            "over1",
            intent_body("d", "ask four", &["span:sp1"], &[], "waiting"),
        ),
        here.intent(
            &node,
            "over2",
            intent_body("d", "ask five", &["message:502"], &["over1"], "waiting"),
        ),
        here.intent(
            &node,
            "fine",
            intent_body("e", "ask six", &["span:sp2"], &[], "waiting"),
        ),
    ];
    here.sent(late, &[]).await;
    assert_eq!(left(here.db()).await, ids(&["before", "over2", "fine"]));
    let forgets = here.logged_forgets().await;
    for row in [
        "late-body",
        "on-span",
        "on-span2",
        "on-memory",
        "on-line",
        "over1",
    ] {
        let object = format!("row:{row}");
        assert_eq!(
            forgets.iter().filter(|it| **it == object).count(),
            1,
            "{row}"
        );
    }
    // A later version of the one from before brings it up again: it goes whole.
    let more = here.intent(
        &node,
        "again",
        intent_body("old", "ask before", &["span:sp2"], &["before"], "dropped"),
    );
    here.sent(vec![more], &[]).await;
    assert_eq!(left(here.db()).await, ids(&["over2", "fine"]));
    here.schema.drop().await;
}

/// One of her rows whose body is still to come, named by a version that
/// goes: forgotten by name, so when its body comes it is a stub already.
#[tokio::test]
async fn one_whose_body_is_to_come_goes_with_the_version_written_on_it() {
    let Some(mut here) = here("resting_on_bare").await else {
        return;
    };
    let talk = here.talk.clone();
    let made = created(&talk, "minted:ferns", "ferns");
    let other = created(&talk, "minted:trips", "trips");
    let (node, away) = (made.node.clone(), other.node.clone());
    put(
        here.db(),
        OWNER,
        &talk,
        &[made, other, span(&talk, &node, "sp1", &["record:r1"])],
    )
    .await;
    let first = here.intent(
        &away,
        "first",
        intent_body("w", "ask about it", &["span:sp1"], &[], "waiting"),
    );
    let next = here.intent(
        &away,
        "next",
        intent_body("w", "ask about it", &["span:sp1"], &["first"], "waiting"),
    );
    here.sent(vec![first.clone(), next], &["first"]).await;
    assert_eq!(left(here.db()).await, ids(&["next"]));
    page_edit::forget_node(here.db(), OWNER, &talk, &node)
        .await
        .unwrap();
    assert!(left(here.db()).await.is_empty());
    assert_eq!(
        here.logged_forgets().await[1..],
        ["row:first".to_string(), "row:next".to_string()]
    );
    here.sent(vec![first], &[]).await;
    assert!(left(here.db()).await.is_empty(), "it does not come back");
    here.schema.drop().await;
}

/// A node another end takes back from sync is no forgetting: its spans are
/// stubs here, and stay whole there. What rests on them stays, what comes of
/// it later too.
#[tokio::test]
async fn a_node_taken_back_from_sync_takes_nothing_that_rests_on_it() {
    let Some(mut here) = here("resting_on_retract").await else {
        return;
    };
    let talk = here.talk.clone();
    let made = created(&talk, "minted:ferns", "ferns");
    let other = created(&talk, "minted:trips", "trips");
    let (node, away) = (made.node.clone(), other.node.clone());
    put(
        here.db(),
        OWNER,
        &talk,
        &[
            made,
            other,
            span(&talk, &node, "sp1", &["record:r1"]),
            hand(&away, "on", "a", "ask one", &["span:sp1"], &[], "waiting"),
        ],
    )
    .await;
    let back = here.by_mac("back", &format!("node:{node}"), "retract", None);
    here.sent(vec![back], &[]).await;
    assert_eq!(
        count(
            here.db(),
            "SELECT COUNT(*) AS n FROM agent_node_rows WHERE row_id = 'sp1' AND body = ''"
        )
        .await,
        1,
        "a stub here"
    );
    let late = here.intent(
        &away,
        "late",
        intent_body("a", "ask one more", &["span:sp1"], &["on"], "waiting"),
    );
    here.sent(vec![late], &[]).await;
    assert_eq!(left(here.db()).await, ids(&["on", "late"]));
    assert!(here.logged_forgets().await.is_empty());
    here.schema.drop().await;
}

/// An intent written from a line that was forgotten while it was being made
/// is left out, as one on a memory is.
#[tokio::test]
async fn one_made_from_a_line_forgotten_meanwhile_is_not_written() {
    let Some(schema) = owned("resting_on_in_flight").await else {
        return;
    };
    let db = &schema.db;
    pen::reset_for_test();
    let talk = audience::owners_talk();
    db.execute_unprepared(
        "INSERT INTO agent_sessions (id, user_id, title, context, message_count, archived, \
         created_at, last_active_at) \
         VALUES ('s_one', 1, 't', '{\"mode\":\"chat\"}', 2, false, NOW(), NOW()); \
         INSERT INTO agent_messages (id, session_id, role, content, created_at) \
         VALUES (501, 's_one', 'user', 'the fern is dry', NOW()), \
                (502, 's_one', 'user', 'tea is ready', NOW())",
    )
    .await
    .unwrap();
    manage::forget_line(db, OWNER, "s_one", 501).await.unwrap();
    let made = created(&talk, "minted:ferns", "ferns");
    let node = made.node.clone();
    put(
        db,
        OWNER,
        &talk,
        &[
            made,
            hand(
                &node,
                "gone",
                "a",
                "ask one",
                &["message:501"],
                &[],
                "waiting",
            ),
            hand(
                &node,
                "fine",
                "b",
                "ask two",
                &["message:502"],
                &[],
                "waiting",
            ),
        ],
    )
    .await;
    assert_eq!(left(db).await, ids(&["fine"]));
    schema.drop().await;
}

/// What is forgotten goes into her log before what rested on it: a line's
/// row and a memory before the intents, in the one transaction that forgets
/// them here.
#[tokio::test]
async fn her_log_has_what_is_forgotten_before_what_rested_on_it() {
    let Some(here) = here("resting_on_order").await else {
        return;
    };
    let talk = here.talk.clone();
    let db = here.db();
    db.execute_unprepared(
        "INSERT INTO agent_sessions (id, user_id, title, context, message_count, archived, \
         created_at, last_active_at) \
         VALUES ('s_one', 1, 't', '{\"mode\":\"chat\"}', 1, false, NOW(), NOW()); \
         INSERT INTO agent_messages (id, session_id, role, content, created_at) \
         VALUES (501, 's_one', 'user', 'the fern is dry', NOW()); \
         INSERT INTO agent_memories (id, user_id, kind, content, source, venue, audience, \
         valid_from, created_at, updated_at) VALUES \
         ('m1', 1, 'fact', 'keeps a kettle', 'chat', 'private', '[1]'::jsonb, \
          now(), now(), now())",
    )
    .await
    .unwrap();
    let line = her_log::talk::keep(
        db,
        &ROOT,
        OWNER,
        "s_one",
        501,
        "the fern is dry",
        crate::clock::now(),
        "web",
    )
    .await
    .unwrap()
    .expect("a row of the line");
    her_log::state::sync(db, &ROOT, OWNER).await.unwrap();
    let made = created(&talk, "minted:ferns", "ferns");
    let node = made.node.clone();
    put(
        db,
        OWNER,
        &talk,
        &[
            made,
            hand(
                &node,
                "a1",
                "a",
                "ask one",
                &["message:501"],
                &[],
                "waiting",
            ),
            hand(&node, "b1", "b", "ask two", &["memory:m1"], &[], "waiting"),
        ],
    )
    .await;
    manage::forget_line(db, OWNER, "s_one", 501).await.unwrap();
    assert!(manage::forget_any(db, "m1").await.unwrap());
    assert!(left(db).await.is_empty());
    assert_eq!(
        here.logged_forgets().await,
        [
            format!("row:{}", line.id),
            "row:a1".into(),
            "memory:m1".into(),
            "row:b1".into(),
        ]
    );
    here.schema.drop().await;
}
