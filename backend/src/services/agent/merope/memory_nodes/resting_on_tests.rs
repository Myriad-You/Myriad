//! Her intents resting on what is forgotten go with it
//! ([`super::intent_grounds`]), by each path something is forgotten here,
//! on a database of their own (`MYRIAD_MEDIA_TEST_DATABASE_URL`, an isolated
//! schema each). Every word here is made up.

use std::collections::{BTreeMap, BTreeSet};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use myriad_nodes::wire::{Batch, Carried, Withheld};
use myriad_persona_log::{
    Row,
    row::{Author as By, LOG_VERSION, Payload, Replicate},
};
use myriad_persona_sync::{Rev, canonical::canonical_bytes};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};
use serde_json::{Value, json};

use super::intent_tests::told as told_at;
use super::read_tests::owned;
use super::talk_tests::{OTHER, count, schema, talk};
use super::{
    Author, RowIn, audience, child, forget, forget_locks, logged, page_edit, partner, pieces_due,
    write,
};
use crate::services::agent::memory::{forgotten, manage, origins::Gone};
use crate::services::agent::merope::nodes::Form;
use crate::services::channel_group::g0;
use crate::services::her_log::{
    self, Owner, Settings, keys, pen, rounds::Rounds, store, test_partner::PARTNER, test_settings,
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
    made.body["object"] = json!({"kind": "repo", "name": "repo"});
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

    /// `row`, the last of the computer's made, as another of theirs wrote it.
    fn by_another(&mut self, row: (Row, Option<Value>), dseq: u64) -> (Row, Option<Value>) {
        let (mut row, body) = row;
        self.mac.count -= 1;
        row.origin = "dev_other".into();
        row.hlc.node = "dev_other".into();
        row.dseq = Some(dseq);
        (row, body)
    }

    /// A node of hers here, as rows name it.
    async fn ferns(&self) -> String {
        let made = created(&self.talk, "minted:ferns", "ferns");
        let object = format!("node:{}", made.node);
        put(self.db(), OWNER, &self.talk, &[made]).await;
        object
    }

    /// A span of the computer's on `object`.
    fn said(&mut self, id: &str, object: &str, text: &str) -> (Row, Option<Value>) {
        self.said_of(id, object, text, "message:aa")
    }

    fn said_of(
        &mut self,
        id: &str,
        object: &str,
        text: &str,
        reference: &str,
    ) -> (Row, Option<Value>) {
        let body = span_body(&self.talk, text, &[reference]);
        self.by_mac(id, object, "node.span", Some(body))
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
        self.sent_in(rows, bare, &self.round(false, 0)).await;
    }

    /// A link of the computer's: its last batch ended a page another follows
    /// (`more`), and it said it holds its own rows up to `holds`.
    fn round(&self, more: bool, holds: u64) -> Rounds {
        let mut rounds = Rounds::default();
        rounds.vector(
            "dev_mac",
            &self.talk,
            &BTreeMap::from([("dev_mac".to_string(), holds)]),
        );
        rounds.batch("dev_mac", &self.talk, more);
        rounds
    }

    /// What waits for the rest of what its end wrote.
    async fn waiting(&self) -> Value {
        pieces_due::count(self.db()).await
    }

    /// [`Self::sent`], as far as `rounds` says.
    async fn sent_in(&self, rows: Vec<(Row, Option<Value>)>, bare: &[&str], rounds: &Rounds) {
        self.sent_by("dev_mac", rows, bare, rounds).await;
    }

    /// [`Self::sent_in`], by the device `device`.
    async fn sent_by(
        &self,
        device: &str,
        rows: Vec<(Row, Option<Value>)>,
        bare: &[&str],
        rounds: &Rounds,
    ) {
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
        // Asked for again: what is here whole.
        let here: BTreeSet<String> = store::rows(self.db(), OWNER, &self.talk)
            .await
            .unwrap()
            .into_iter()
            .filter(|row| !row.forgotten)
            .map(|row| row.id)
            .collect();
        let asked: BTreeSet<String> = rows
            .iter()
            .map(|(row, _)| row.id.clone())
            .filter(|id| here.contains(id))
            .collect();
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
        let taken = store::take(self.db(), &ROOT, &self.owner, device, batch, &asked, now)
            .await
            .unwrap();
        assert!(taken.not_taken.is_empty(), "{:?}", taken.not_taken);
        logged::take_in(self.db(), &ROOT, &self.owner, rounds)
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
    logged::take_in(here.db(), &ROOT, &here.owner, &here.round(false, 0))
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

/// An intent being written from a line as the line is forgotten, when no
/// row names the line yet: it waits for the forgetting, and is left out.
#[tokio::test]
async fn one_written_as_its_line_is_forgotten_waits_and_is_left_out() {
    let Some(schema) = owned("resting_on_at_once").await else {
        return;
    };
    let db = &schema.db;
    pen::reset_for_test();
    let talk = audience::owners_talk();
    db.execute_unprepared(
        "INSERT INTO agent_sessions (id, user_id, title, context, message_count, archived, \
         created_at, last_active_at) \
         VALUES ('s_one', 1, 't', '{\"mode\":\"chat\"}', 1, false, NOW(), NOW()); \
         INSERT INTO agent_messages (id, session_id, role, content, created_at) \
         VALUES (501, 's_one', 'user', 'the fern is dry', NOW())",
    )
    .await
    .unwrap();
    let made = created(&talk, "minted:ferns", "ferns");
    let node = made.node.clone();
    put(db, OWNER, &talk, &[made]).await;
    // The line going, as `manage::forget_line` has it go, not yet committed.
    let line = Gone {
        id: 501,
        session_id: "s_one".into(),
        role: "user".into(),
        log_row: None,
    };
    let txn = db.begin().await.unwrap();
    forget_locks::lock_for(&txn, Some(OWNER), &[], &[line])
        .await
        .unwrap();
    forget::forget(&txn, &[], &[501]).await.unwrap();
    forgotten::keep(&txn, OWNER, forgotten::MESSAGE, &["501".into()])
        .await
        .unwrap();
    let writing = tokio::spawn({
        let (db, talk) = (db.clone(), talk.clone());
        let row = hand(
            &node,
            "late",
            "a",
            "ask one",
            &["message:501"],
            &[],
            "waiting",
        );
        async move { put(&db, OWNER, &talk, &[row]).await }
    });
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    assert!(
        !writing.is_finished(),
        "it waits for what is being forgotten"
    );
    txn.commit().await.unwrap();
    writing.await.unwrap();
    assert!(left(db).await.is_empty());
    schema.drop().await;
}

/// A node locked for every audience a memory may be named in, its row here
/// or not.
#[tokio::test]
async fn every_audience_a_memory_may_be_named_in_is_locked() {
    let Some(schema) = owned("resting_on_locks").await else {
        return;
    };
    let db = &schema.db;
    db.execute_unprepared(
        "INSERT INTO agent_memories (id, user_id, kind, content, source, venue, audience, \
         valid_from, created_at, updated_at) VALUES \
         ('g1', 7, 'fact', 'keeps a kettle', 'chat', 'group:qq:5', '[7]'::jsonb, \
          now(), now(), now())",
    )
    .await
    .unwrap();
    let locked = |memory: &'static str| async move {
        let txn = db.begin().await.unwrap();
        forget_locks::lock_for(&txn, Some(OWNER), &[memory.into()], &[])
            .await
            .unwrap();
        // By what this connection holds: other tests hold locks of theirs.
        let here = &txn;
        let held = |user: i32, audience: String| async move {
            here.query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT EXISTS (SELECT 1 FROM pg_locks l, \
                   (SELECT hashtextextended('myriad-memory-nodes:' || $1 || ':' || $2, 0) AS key) k \
                 WHERE l.locktype = 'advisory' AND l.pid = pg_backend_pid() AND l.granted \
                   AND ((l.classid::bigint << 32) | l.objid::bigint) = k.key) AS held",
                [user.to_string().into(), audience.into()],
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get::<bool>("", "held")
            .unwrap()
        };
        let out = [
            held(OWNER, "group:qq:5".into()).await,
            held(7, "personal:7".into()).await,
            held(OWNER, audience::owners_talk()).await,
            held(OWNER, audience::OWN.into()).await,
        ];
        txn.rollback().await.unwrap();
        out
    };
    assert_eq!(locked("g1").await, [true, true, false, false]);
    assert_eq!(locked("from-elsewhere").await, [false, false, true, true]);
    schema.drop().await;
}

/// A Mac that marks what a span came from writes it again and forgets the
/// row: a version of it stands, in whatever words, and what rests on it stays,
/// what comes late too. Once none stands, it is gone, and they go.
#[tokio::test]
async fn a_span_written_again_is_not_forgotten() {
    let Some(mut here) = here("resting_on_marked").await else {
        return;
    };
    let object = here.ferns().await;
    let node = object.trim_start_matches("node:").to_string();
    let told = vec![
        here.said("p-1@mac", &object, "they watered it"),
        here.said("p-2@mac", &object, "they moved it, at noon"),
        here.intent(
            &node,
            "on-marked",
            intent_body("a", "ask one", &["span:p-1@mac"], &[], "waiting"),
        ),
        here.intent(
            &node,
            "on-words",
            intent_body("b", "ask two", &["span:p-2@mac"], &[], "waiting"),
        ),
    ];
    here.sent(told, &[]).await;
    let again = vec![
        here.said("p-1@mac~1", &object, "they watered it"),
        here.said("p-2@mac~1", &object, "they moved it"),
        here.forget("row:p-1@mac"),
        here.forget("row:p-2@mac"),
    ];
    here.sent(again, &["p-1@mac~1"]).await;
    assert_eq!(left(here.db()).await, ids(&["on-marked", "on-words"]));
    let late = vec![
        here.intent(
            &node,
            "late-marked",
            intent_body("c", "ask three", &["span:p-1@mac"], &[], "waiting"),
        ),
        here.intent(
            &node,
            "late-words",
            intent_body("d", "ask four", &["span:p-2@mac"], &[], "waiting"),
        ),
    ];
    here.sent(late, &[]).await;
    assert_eq!(
        left(here.db()).await,
        ids(&["late-marked", "late-words", "on-marked", "on-words"])
    );
    assert!(here.logged_forgets().await.is_empty());
    let gone = vec![here.forget("row:p-2@mac~1")];
    here.sent(gone, &[]).await;
    assert_eq!(left(here.db()).await, ids(&["late-marked", "on-marked"]));
    here.schema.drop().await;
}

/// A row of the site's that the Mac marked: it is written again in the Mac's
/// own slot for it, and what rests on it stays. Once that is forgotten, there
/// or here, it was forgotten.
#[tokio::test]
async fn a_row_of_the_sites_another_end_wrote_again_is_not_forgotten() {
    for (place, there) in [("n-", true), ("p-", false)] {
        let Some(mut here) = here("resting_on_marked_here").await else {
            return;
        };
        let talk = here.talk.clone();
        let object = here.ferns().await;
        let node = object.trim_start_matches("node:").to_string();
        put(
            here.db(),
            OWNER,
            &talk,
            &[
                span(&talk, &node, "u1", &["record:r1"]),
                span(&talk, &node, "u2", &["record:r2"]),
                hand(&node, "on", "a", "ask one", &["span:u1"], &[], "waiting"),
                hand(&node, "on2", "b", "ask two", &["span:u2"], &[], "waiting"),
            ],
        )
        .await;
        let version = format!("{place}u1@mac");
        let again = vec![
            here.said_of(&version, &object, "they watered it", "message:501"),
            // Not a version of it: a slot that only ends alike.
            here.said("x-u2@mac", &object, "they moved it"),
            here.forget("row:u1"),
            here.forget("row:u2"),
        ];
        here.sent(again, &[]).await;
        assert_eq!(left(here.db()).await, ids(&["on"]), "{place}");
        match there {
            true => {
                let gone = vec![here.forget(&format!("row:{version}"))];
                here.sent(gone, &[]).await;
            }
            false => {
                let txn = here.db().begin().await.unwrap();
                forget::forget(&txn, &[], &[501]).await.unwrap();
                txn.commit().await.unwrap();
            }
        }
        assert!(left(here.db()).await.is_empty(), "{place}");
        here.schema.drop().await;
    }
}

/// An intent names the row that stood as it was written. Marked since, by one
/// end after another, or never here at all, or with a later version of its
/// own standing: once what was written in its place is forgotten, it goes.
/// Not while the row itself stands.
#[tokio::test]
async fn an_intent_on_a_row_written_again_goes_by_the_name_it_had() {
    let Some(mut here) = here("resting_on_old_name").await else {
        return;
    };
    let talk = here.talk.clone();
    let object = here.ferns().await;
    let node = object.trim_start_matches("node:").to_string();
    let on = |id: &str, reference: &str| {
        hand(
            &node,
            id,
            id,
            &format!("ask {id}"),
            &[reference],
            &[],
            "waiting",
        )
    };
    put(
        here.db(),
        OWNER,
        &talk,
        &[
            entry(&talk, &node, "u1", &["record:r1"]),
            entry(&talk, &node, "u4", &["record:r4"]),
            on("on1", "entry:u1"),
            on("on2", "entry:u2"),
            on("on4", "entry:u4"),
        ],
    )
    .await;
    let mac = |here: &mut Here, id: &str, line: &str| {
        let body = entry(&talk, &node, id, &[line]).body;
        here.by_mac(id, &object, "node.entry", Some(body))
    };
    let again = vec![
        mac(&mut here, "n-u1@mac", "message:500"),
        mac(&mut here, "n-u2@mac", "message:501"),
        mac(&mut here, "n-u2@mac~1", "message:500"),
        mac(&mut here, "n-u4@mac", "message:501"),
        here.forget("row:u1"),
    ];
    here.sent(again, &[]).await;
    // Marked by another of theirs after.
    let other = mac(&mut here, "n-n-u1@mac@other", "message:501");
    let other = here.by_another(other, 1);
    let gone = here.forget("row:n-u1@mac");
    let gone = here.by_another(gone, 2);
    here.sent(vec![other, gone], &[]).await;
    assert_eq!(left(here.db()).await, ids(&["on1", "on2", "on4"]));
    let txn = here.db().begin().await.unwrap();
    forget::forget(&txn, &[], &[501]).await.unwrap();
    txn.commit().await.unwrap();
    assert_eq!(left(here.db()).await, ids(&["on4"]));
    let late = here.intent(
        &node,
        "late",
        intent_body("late", "ask late", &["entry:u1"], &[], "waiting"),
    );
    here.sent(vec![late], &[]).await;
    assert_eq!(left(here.db()).await, ids(&["on4"]));
    here.schema.drop().await;
}

/// A span moved to another node and marked there: the stub it left does not
/// say it is forgotten when the node it was on is. Once the node it stands on
/// is, it is.
#[tokio::test]
async fn the_stub_of_a_span_written_again_elsewhere_takes_nothing_with_its_node() {
    let Some(mut here) = here("resting_on_old_stub").await else {
        return;
    };
    let talk = here.talk.clone();
    let made = created(&talk, "minted:ferns", "ferns");
    let into = created(&talk, "minted:pots", "pots");
    let other = created(&talk, "minted:trips", "trips");
    let (node, into_node, away) = (made.node.clone(), into.node.clone(), other.node.clone());
    put(
        here.db(),
        OWNER,
        &talk,
        &[
            made,
            into,
            other,
            span(&talk, &node, "u1", &["record:r1"]),
            hand(&away, "on", "a", "ask one", &["span:u1"], &[], "waiting"),
        ],
    )
    .await;
    let again = vec![
        here.said("p-u1@mac", &format!("node:{into_node}"), "they watered it"),
        here.forget("row:u1"),
    ];
    here.sent(again, &[]).await;
    assert_eq!(left(here.db()).await, ids(&["on"]));
    page_edit::forget_node(here.db(), OWNER, &talk, &node)
        .await
        .unwrap();
    assert_eq!(left(here.db()).await, ids(&["on"]));
    page_edit::forget_node(here.db(), OWNER, &talk, &into_node)
        .await
        .unwrap();
    assert!(left(here.db()).await.is_empty());
    here.schema.drop().await;
}

/// A node taken back from sync is forgotten by a forget that names it, and a
/// span on it by one that names the span. One of the node it went into is
/// not: its home holds it still.
#[tokio::test]
async fn a_node_taken_back_is_forgotten_only_by_its_own_name() {
    let Some(mut here) = here("resting_on_retract_named").await else {
        return;
    };
    let talk = here.talk.clone();
    let away = created(&talk, "minted:trips", "trips");
    let whole = created(&talk, "minted:garden", "garden");
    let (hands, into) = (away.node.clone(), whole.node.clone());
    let mut rows = vec![away, whole];
    let mut nodes = Vec::new();
    for name in ["into", "named", "by-row"] {
        let key = format!("minted:{name}");
        let made = created(&talk, &key, name);
        let node = made.node.clone();
        rows.push(made);
        rows.push(span(&talk, &node, &format!("sp-{name}"), &["record:r1"]));
        let because = format!("span:sp-{name}");
        rows.push(hand(
            &hands,
            &format!("on-{name}"),
            name,
            &format!("ask {name}"),
            &[&because],
            &[],
            "waiting",
        ));
        nodes.push(node);
    }
    rows.push(row(
        &nodes[0],
        "went",
        "node.absorbed",
        json!({"version": 1, "sync": "shared", "saidBy": ["her"], "into": "minted:garden",
               "how": "merged", "reason": "one thing", "because": ["message:502"]}),
    ));
    put(here.db(), OWNER, &talk, &rows).await;
    let mut away = Vec::new();
    for (n, node) in nodes.iter().enumerate() {
        let id = format!("back-{n}");
        away.push(here.by_mac(&id, &format!("node:{node}"), "retract", None));
    }
    away.push(here.forget(&format!("node:{into}")));
    away.push(here.forget(&format!("node:{}", nodes[1])));
    away.push(here.forget("row:sp-by-row"));
    here.sent(away, &[]).await;
    assert_eq!(left(here.db()).await, ids(&["on-into"]));
    let late = here.intent(
        &hands,
        "late",
        intent_body("late", "ask late", &["span:sp-into"], &[], "waiting"),
    );
    here.sent(vec![late], &[]).await;
    assert_eq!(left(here.db()).await, ids(&["late", "on-into"]));
    // Named here, its spans stubs already.
    let txn = here.db().begin().await.unwrap();
    let named = nodes[..1].iter().cloned().collect();
    forget::forget_nodes(&txn, OWNER, &talk, &named)
        .await
        .unwrap();
    txn.commit().await.unwrap();
    assert!(left(here.db()).await.is_empty());
    here.schema.drop().await;
}

/// A span of the Mac's forgotten here while the Mac marked it: the forget
/// here is not the Mac's, so what rests on the span goes, what comes late too.
#[tokio::test]
async fn a_span_forgotten_here_as_its_end_marked_it_takes_what_rests_on_it() {
    let Some(mut here) = here("resting_on_both").await else {
        return;
    };
    let object = here.ferns().await;
    let node = object.trim_start_matches("node:").to_string();
    let told = vec![
        here.said_of("p-1@mac", &object, "they watered it", "message:501"),
        here.intent(
            &node,
            "on",
            intent_body("a", "ask one", &["span:p-1@mac"], &[], "waiting"),
        ),
    ];
    here.sent(told, &[]).await;
    let again = vec![
        here.said("p-1@mac~1", &object, "they watered it"),
        here.forget("row:p-1@mac"),
        here.intent(
            &node,
            "late",
            intent_body("b", "ask two", &["span:p-1@mac"], &[], "waiting"),
        ),
    ];
    let txn = here.db().begin().await.unwrap();
    forget::forget(&txn, &[], &[501]).await.unwrap();
    txn.commit().await.unwrap();
    assert!(left(here.db()).await.is_empty());
    here.sent(again, &[]).await;
    assert!(left(here.db()).await.is_empty());
    here.schema.drop().await;
}

/// It is whoever wrote the forget that has to be here whole, and its version
/// that counts: another end's does not.
#[tokio::test]
async fn a_forget_is_taken_for_what_the_end_that_wrote_it_kept() {
    let Some(mut here) = here("resting_on_whose").await else {
        return;
    };
    let object = here.ferns().await;
    let node = object.trim_start_matches("node:").to_string();
    let other = here.said("p-1@other", &object, "they watered it");
    let other = here.by_another(other, 1);
    let version = here.said("p-1@other~1", &object, "they watered it");
    let version = here.by_another(version, 2);
    let told = vec![
        other,
        version,
        here.intent(
            &node,
            "on",
            intent_body("a", "ask one", &["span:p-1@other"], &[], "waiting"),
        ),
    ];
    here.sent(told, &[]).await;
    let missing = here.said("p-2@mac", &object, "they moved it");
    let gone = here.forget("row:p-1@other");
    here.sent(vec![gone], &[]).await;
    assert_eq!(left(here.db()).await, ids(&["on"]));
    assert_eq!(here.waiting().await, 1);
    here.sent(vec![missing], &[]).await;
    assert!(left(here.db()).await.is_empty());
    here.schema.drop().await;
}

/// Forgets go ahead: the version written before one comes batches after it.
/// Until it has, what rests on the piece stays, and is not looked at.
#[tokio::test]
async fn a_forget_is_looked_at_once_what_was_written_before_it_came() {
    let Some(mut here) = here("resting_on_ahead").await else {
        return;
    };
    let object = here.ferns().await;
    let node = object.trim_start_matches("node:").to_string();
    let told = vec![
        here.said("p-1@mac", &object, "they watered it"),
        here.intent(
            &node,
            "on",
            intent_body("a", "ask one", &["span:p-1@mac"], &[], "waiting"),
        ),
    ];
    here.sent(told, &[]).await;
    let again = here.said("p-1@mac~1", &object, "they watered it");
    let gone = here.forget("row:p-1@mac");
    // The page's last batch names no page after it, either.
    here.sent_in(vec![gone], &[], &here.round(false, 0)).await;
    assert_eq!(
        count(
            here.db(),
            "SELECT COUNT(*) AS n FROM agent_node_rows WHERE row_id = 'p-1@mac' AND body = ''"
        )
        .await,
        1,
        "the words go at once"
    );
    assert_eq!(left(here.db()).await, ids(&["on"]));
    assert_eq!(here.waiting().await, 1);
    here.sent(vec![again], &[]).await;
    assert_eq!(left(here.db()).await, ids(&["on"]));
    assert_eq!(here.waiting().await, 0);
    assert!(here.logged_forgets().await.is_empty());
    here.schema.drop().await;
}

/// An older build of the Mac wrote the forget first, the next version right
/// after: everything up to the forget is here before the version, so the
/// round's end is waited for, by its page (`more`) and by what the Mac said
/// it holds.
#[tokio::test]
async fn a_forget_written_before_the_next_version_waits_for_the_rounds_end() {
    for by_page in [true, false] {
        let Some(mut here) = here("resting_on_old_order").await else {
            return;
        };
        let object = here.ferns().await;
        let node = object.trim_start_matches("node:").to_string();
        let told = vec![
            here.said("p-1@mac", &object, "they watered it"),
            here.said("p-2@mac", &object, "they moved it"),
            here.intent(
                &node,
                "on",
                intent_body("a", "ask one", &["span:p-1@mac"], &[], "waiting"),
            ),
            here.intent(
                &node,
                "on-gone",
                intent_body("b", "ask two", &["span:p-2@mac"], &[], "waiting"),
            ),
        ];
        here.sent(told, &[]).await;
        let gone = here.forget("row:p-1@mac");
        let truly = here.forget("row:p-2@mac");
        let again = here.said("p-1@mac~1", &object, "they watered it");
        let holds = again.0.dseq.unwrap();
        let open = match by_page {
            true => here.round(true, 0),
            false => here.round(false, holds),
        };
        here.sent_in(vec![gone, truly], &[], &open).await;
        assert_eq!(left(here.db()).await, ids(&["on", "on-gone"]), "{by_page}");
        assert_eq!(here.waiting().await, 2, "{by_page}");
        // Nothing more of it on a link that came to its end: looked at on
        // what is here.
        here.sent_in(vec![again], &[], &here.round(false, holds))
            .await;
        assert_eq!(left(here.db()).await, ids(&["on"]), "{by_page}");
        assert_eq!(here.waiting().await, 0, "{by_page}");
        here.schema.drop().await;
    }
}

/// A forget past a gap waits, however long and whatever else comes, and so
/// does what one forgotten meanwhile says of the words it was carried on in.
/// Another link later brings what was missing, and no version: it was
/// forgotten, and all of it goes.
#[tokio::test]
async fn a_forget_past_a_gap_waits_across_links() {
    let Some(mut here) = here("resting_on_gap").await else {
        return;
    };
    let object = here.ferns().await;
    let node = object.trim_start_matches("node:").to_string();
    let told = vec![
        here.said("p-1@mac", &object, "they watered it"),
        here.said("p-2@mac", &object, "they moved it"),
        here.intent(
            &node,
            "on",
            intent_body("a", "ask one", &["span:p-1@mac"], &[], "waiting"),
        ),
        here.intent(
            &node,
            "said",
            intent_body("b", "ask two", &["span:p-1@mac"], &[], "waiting"),
        ),
    ];
    here.sent(told, &[]).await;
    put(
        here.db(),
        OWNER,
        &here.talk.clone(),
        &[hand(
            &node,
            "carried",
            "b",
            "ask two",
            &["span:p-2@mac"],
            &["said"],
            "waiting",
        )],
    )
    .await;
    let missing = here.said("p-3@mac", &object, "they sold it");
    let gone = vec![here.forget("row:p-1@mac"), here.forget("row:said")];
    here.sent(gone, &[]).await;
    assert_eq!(left(here.db()).await, ids(&["carried", "on"]));
    let other = here.said("p-4@mac", &object, "they bought one");
    let late = here.intent(
        &node,
        "late",
        intent_body("c", "ask three", &["span:p-1@mac"], &[], "waiting"),
    );
    here.sent(vec![other, late], &[]).await;
    logged::take_in_due(here.db(), &ROOT, &here.owner, &Rounds::default())
        .await
        .unwrap();
    assert_eq!(left(here.db()).await, ids(&["carried", "late", "on"]));
    assert_eq!(here.waiting().await, 1);
    assert!(here.logged_forgets().await.is_empty());
    here.sent(vec![missing], &[]).await;
    assert!(left(here.db()).await.is_empty());
    assert_eq!(here.waiting().await, 0);
    assert_eq!(
        count(
            here.db(),
            "SELECT COUNT(*) AS n FROM agent_forgotten WHERE what = 'intent_due'"
        )
        .await,
        0
    );
    here.schema.drop().await;
}

/// What two ends of theirs forgot before the rest of it came: past a `gap`,
/// or before the round was over. `on` rests on what `dev_mac` forgot,
/// `carried` on words one forgotten with it said, and `other` on what
/// `dev_other` did. `again` rests on what `dev_mac` wrote again. The
/// computer's row that never came, when there is a gap.
async fn forgotten_early(here: &mut Here, object: &str, gap: bool) -> Option<(Row, Option<Value>)> {
    let node = object.trim_start_matches("node:").to_string();
    let told = vec![
        here.said("p-1@mac", object, "they watered it"),
        here.said("p-2@mac", object, "they moved it"),
        here.said("p-3@mac", object, "they cut it"),
        here.said("p-4@mac", object, "they turned it"),
        here.intent(
            &node,
            "on",
            intent_body("a", "ask one", &["span:p-1@mac"], &[], "waiting"),
        ),
        here.intent(
            &node,
            "said",
            intent_body("b", "ask two", &["span:p-1@mac"], &[], "waiting"),
        ),
        here.intent(
            &node,
            "other",
            intent_body("d", "ask four", &["span:p-3@mac"], &[], "waiting"),
        ),
        here.intent(
            &node,
            "again",
            intent_body("e", "ask five", &["span:p-4@mac"], &[], "waiting"),
        ),
    ];
    here.sent(told, &[]).await;
    put(
        here.db(),
        OWNER,
        &here.talk.clone(),
        &[hand(
            &node,
            "carried",
            "b",
            "ask two",
            &["span:p-2@mac"],
            &["said"],
            "waiting",
        )],
    )
    .await;
    let missing = gap.then(|| here.said("p-5@mac", object, "they sold it"));
    let mut gone = vec![
        here.said("p-4@mac~1", object, "they turned it"),
        here.forget("row:p-1@mac"),
        here.forget("row:p-4@mac"),
        here.forget("row:said"),
    ];
    let theirs = here.forget("row:p-3@mac");
    gone.push(here.by_another(theirs, if gap { 7 } else { 1 }));
    here.sent_in(gone, &[], &here.round(!gap, 0)).await;
    assert_eq!(
        left(here.db()).await,
        ids(&["again", "carried", "on", "other"])
    );
    assert_eq!(here.waiting().await, 3);
    missing
}

async fn marks(db: &DatabaseConnection, what: &str) -> i64 {
    count(
        db,
        &format!("SELECT COUNT(*) AS n FROM agent_forgotten WHERE what = '{what}'"),
    )
    .await
}

/// A device removed ends no round: what it forgot, all it wrote up to there
/// here, is looked at, as another end's waits on; that one's too as its
/// device is let go.
#[tokio::test]
async fn what_a_removed_device_forgot_is_looked_at_as_its_round_was_over() {
    use crate::services::persona_sync::devices;
    let Some(mut here) = here("resting_on_removed").await else {
        return;
    };
    linked(here.db(), &["dev_mac", "dev_other"]).await;
    let object = here.ferns().await;
    forgotten_early(&mut here, &object, false).await;
    // Hers, as folded (the computer's are the owner's word of hers: not).
    let before = folded(here.db()).await;
    assert!(before.contains("ask two"), "{before}");
    forgotten::keep(here.db(), OWNER, forgotten::MEMORY, &["m".to_string()])
        .await
        .unwrap();
    let why = devices::RevokeReason::ByOwner;
    devices::revoke(here.db(), OWNER, "dev_mac", why)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(left(here.db()).await, ids(&["again", "other"]));
    assert_eq!(here.waiting().await, 1);
    assert_eq!(marks(here.db(), "intent_due").await, 0);
    let after = folded(here.db()).await;
    assert!(after.contains("they turned it"), "{after}");
    assert!(!after.contains("ask two"), "{after}");
    // The other's, removed while that could not be looked at.
    here.db()
        .execute_unprepared(
            "UPDATE agent_sync_devices SET revoked_at = NOW(), revoked_reason = 'owner' \
             WHERE id = 'dev_other'",
        )
        .await
        .unwrap();
    assert!(
        devices::forget(here.db(), OWNER, "dev_other")
            .await
            .unwrap()
    );
    assert_eq!(left(here.db()).await, ids(&["again"]));
    assert_eq!(here.waiting().await, 0);
    assert_eq!(marks(here.db(), "memory").await, 1);
    here.schema.drop().await;
}

/// Her nodes as they are folded.
async fn folded(db: &DatabaseConnection) -> String {
    db.query_all_raw(Statement::from_string(
        DatabaseBackend::Postgres,
        "SELECT node::text AS node FROM agent_node_heads",
    ))
    .await
    .unwrap()
    .iter()
    .map(|row| row.try_get::<String>("", "node").unwrap())
    .collect()
}

async fn linked(db: &DatabaseConnection, devices: &[&str]) {
    for device in devices {
        db.execute_unprepared(&format!(
            "INSERT INTO agent_sync_devices (id, user_id, name, token_hash, token_hint, \
             created_at) VALUES ('{device}', {OWNER}, 'Mac', '{device}', 'x', NOW())"
        ))
        .await
        .unwrap();
    }
}

/// A computer cut off halfway through sending all of it, its forget here
/// before the version it wrote in its place; removed, and let go, as another
/// device is. What rests on it waits, and stays: the same computer, linked
/// anew, brings the version under the end it was.
#[tokio::test]
async fn what_waits_past_a_gap_waits_for_the_same_computer_linked_anew() {
    use crate::services::persona_sync::devices;
    let Some(mut here) = here("resting_on_anew").await else {
        return;
    };
    linked(here.db(), &["dev_mac", "dev_phone"]).await;
    let object = here.ferns().await;
    let node = object.trim_start_matches("node:").to_string();
    let told = vec![
        here.said("p-1@mac", &object, "they watered it"),
        here.intent(
            &node,
            "on",
            intent_body("a", "ask one", &["span:p-1@mac"], &[], "waiting"),
        ),
    ];
    here.sent(told, &[]).await;
    let version = here.said("p-1@mac~1", &object, "they watered it");
    let gone = here.forget("row:p-1@mac");
    let last = here.mac.count;
    here.sent(vec![gone], &[]).await;
    assert_eq!(here.waiting().await, 1);
    let why = devices::RevokeReason::ByOwner;
    for device in ["dev_mac", "dev_phone"] {
        devices::revoke(here.db(), OWNER, device, why)
            .await
            .unwrap()
            .unwrap();
        assert!(devices::forget(here.db(), OWNER, device).await.unwrap());
        assert_eq!(left(here.db()).await, ids(&["on"]), "{device}");
        assert_eq!(here.waiting().await, 1, "{device}");
    }
    linked(here.db(), &["dev_anew"]).await;
    let mut rounds = Rounds::default();
    rounds.vector(
        "dev_anew",
        &here.talk,
        &BTreeMap::from([("dev_mac".to_string(), last)]),
    );
    rounds.batch("dev_anew", &here.talk, false);
    here.sent_by("dev_anew", vec![version], &[], &rounds).await;
    assert_eq!(left(here.db()).await, ids(&["on"]));
    assert_eq!(here.waiting().await, 0);
    here.schema.drop().await;
}

/// Her log deleted, syncing off past the retention period: what waited only
/// for its round to be over is looked at first, what waited past a gap stays;
/// nothing waits after.
#[tokio::test]
async fn what_waits_is_looked_at_before_her_log_is_deleted() {
    for gap in [false, true] {
        deleted_past_retention(gap).await;
    }
}

async fn deleted_past_retention(gap: bool) {
    let Some(mut here) = here("resting_on_retention").await else {
        return;
    };
    let object = here.ferns().await;
    forgotten_early(&mut here, &object, gap).await;
    let db = here.db();
    forgotten::keep(db, OWNER, forgotten::MEMORY, &["m".to_string()])
        .await
        .unwrap();
    // Nothing here has them, so nothing looks at them.
    for what in [forgotten::PIECE_DUE, forgotten::INTENT_DUE] {
        forgotten::keep(db, OWNER, what, &["none".to_string()])
            .await
            .unwrap();
        forgotten::keep(db, OTHER, what, &["theirs".to_string()])
            .await
            .unwrap();
    }
    store::stop(db, OWNER).await.unwrap();
    let settings = her_log::current_settings();
    let at = crate::clock::now();
    her_log::retention::sweep(db, &settings, at).await.unwrap();
    assert_eq!(marks(db, "piece_due").await, 5, "not past it yet");
    let past = at + chrono::Duration::days(settings.retention_days + 1);
    let swept = her_log::retention::sweep(db, &settings, past)
        .await
        .unwrap();
    assert_eq!(swept.accounts, vec![OWNER]);
    let stays: &[&str] = match gap {
        true => &["again", "carried", "on", "other"],
        false => &["again"],
    };
    assert_eq!(left(db).await, ids(stays));
    for what in ["piece_due", "intent_due"] {
        assert_eq!(marks(db, what).await, 1, "another's: {what}");
    }
    assert_eq!(marks(db, "memory").await, 1);
    here.schema.drop().await;
}

/// Her persona cleared, or she moved off the site: her nodes and her log
/// stay, and what waits is looked at as the rest of it comes.
#[tokio::test]
async fn what_waits_is_looked_at_though_her_persona_was_cleared() {
    use crate::services::agent::merope::{clear_persona_on, retire_her_on};
    for cleared in [true, false] {
        let Some(mut here) = here("resting_on_cleared").await else {
            return;
        };
        let object = here.ferns().await;
        let missing = forgotten_early(&mut here, &object, true).await.unwrap();
        let txn = here.db().begin().await.unwrap();
        if cleared {
            clear_persona_on(&txn).await.unwrap();
        } else {
            retire_her_on(&txn, OWNER).await.unwrap();
        }
        txn.commit().await.unwrap();
        assert_eq!(here.waiting().await, 3);
        assert_eq!(
            left(here.db()).await,
            ids(&["again", "carried", "on", "other"])
        );
        here.sent(vec![missing], &[]).await;
        assert_eq!(left(here.db()).await, ids(&["again", "other"]));
        assert_eq!(here.waiting().await, 1);
        here.schema.drop().await;
    }
}

/// What they told her for later, judged as the line it was told in is
/// forgotten: no intent of it is written, and nothing of the instruction is
/// kept.
#[tokio::test]
async fn nothing_of_an_instruction_is_kept_when_its_intent_is_left_out() {
    let Some(schema) = schema("resting_on_instruction").await else {
        return;
    };
    let db = &schema.db;
    pen::reset_for_test();
    let any: Vec<String> = (1..100).map(|id| id.to_string()).collect();
    forgotten::keep(db, OTHER, forgotten::MESSAGE, &any)
        .await
        .unwrap();
    let (_, judged) = told_at(db, crate::services::agent::merope::clock::now()).await;
    g0::leave();
    assert!(judged.taken.is_empty(), "{:?}", judged.taken);
    assert!(left(db).await.is_empty());
    assert_eq!(
        count(db, "SELECT COUNT(*) AS n FROM agent_node_instructions").await,
        0
    );
    schema.drop().await;
}

/// A node forgotten whole takes what went into it: no word of that is kept
/// here either. Left here by a build before, what names a line forgotten now
/// loses its words.
#[tokio::test]
async fn a_node_forgotten_whole_keeps_no_word_of_what_went_into_it() {
    let Some(schema) = schema("resting_on_absorbed").await else {
        return;
    };
    let db = &schema.db;
    pen::reset_for_test();
    let talk = talk();
    let made = created(&talk, "minted:ferns", "ferns");
    let whole = created(&talk, "minted:trips", "trips");
    let (node, into) = (made.node.clone(), whole.node.clone());
    let went = row(
        &node,
        "went",
        "node.absorbed",
        json!({"version": 1, "sync": "shared", "saidBy": ["her"], "into": "minted:trips",
               "how": "merged", "reason": "one thing", "because": ["message:502"]}),
    );
    put(
        db,
        OTHER,
        &talk,
        &[
            made,
            whole,
            span(&talk, &node, "sp1", &["message:502"]),
            hand(
                &node,
                "inside",
                "a",
                "ask one",
                &["message:501"],
                &[],
                "waiting",
            ),
            went,
        ],
    )
    .await;
    // As a build before forgot it: the forget, and no more.
    let txn = db.begin().await.unwrap();
    forget::forget_one(&txn, OTHER, &talk, &into, forget::Forgets::Node)
        .await
        .unwrap();
    txn.commit().await.unwrap();
    assert_eq!(left(db).await, ids(&["inside"]));
    let txn = db.begin().await.unwrap();
    forget::forget(&txn, &[], &[501]).await.unwrap();
    txn.commit().await.unwrap();
    assert!(left(db).await.is_empty());
    // As it is forgotten now.
    let words = "SELECT COUNT(*) AS n FROM agent_node_rows WHERE row_id = 'sp1' AND body <> ''";
    assert_eq!(count(db, words).await, 1);
    let txn = db.begin().await.unwrap();
    forget::forget_nodes(&txn, OTHER, &talk, &[into].into_iter().collect())
        .await
        .unwrap();
    txn.commit().await.unwrap();
    assert_eq!(count(db, words).await, 0);
    schema.drop().await;
}

/// The words of an intent that comes late, carried on here on other grounds
/// under the same id on another node, go with it; one that rested on what
/// was forgotten before, in other words, stays as it was.
#[tokio::test]
async fn what_the_words_of_one_that_comes_late_were_carried_on_as_goes_too() {
    let Some(mut here) = here("resting_on_late_copy").await else {
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
            span(&talk, &node, "sp2", &["record:r2"]),
            hand(&away, "copy", "a", "ask one", &["span:sp2"], &[], "waiting"),
            hand(
                &away,
                "before",
                "old",
                "ask before",
                &["span:sp1"],
                &[],
                "waiting",
            ),
        ],
    )
    .await;
    // The span forgotten, as an older build of the site did it.
    let mut doomed = Vec::new();
    for kept in super::heads::rows_of(here.db(), OWNER, &talk)
        .await
        .unwrap()
    {
        if kept.row.id == "sp1" {
            doomed.push(kept.row);
        }
    }
    let txn = here.db().begin().await.unwrap();
    forget::forget_one(&txn, OWNER, &talk, &node, forget::Forgets::Row("sp1"))
        .await
        .unwrap();
    forget::stub(&txn, OWNER, &doomed).await.unwrap();
    txn.commit().await.unwrap();
    let late = here.intent(
        &node,
        "late",
        intent_body("a", "ask one", &["span:sp1"], &[], "waiting"),
    );
    here.sent(vec![late], &[]).await;
    assert_eq!(left(here.db()).await, ids(&["before"]));
    here.schema.drop().await;
}
