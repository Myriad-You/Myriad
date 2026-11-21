//! The owner's sessions beside her nodes and in her inbox, on a database of their own
//! (`MYRIAD_MEDIA_TEST_DATABASE_URL`, an isolated schema each), with the
//! model's answers given by the test (`channel_group::g0`).

use std::time::Duration;

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::{Value, json};

use super::*;
use crate::services::agent::memory::unified::Audience;
use crate::services::agent::merope::memory_nodes::shown;
use crate::services::channel_group::g0;

const OWNER: i32 = 1;
const VISITOR: i32 = 7;
const DEVICE: &str = "dev_mac";
const GRANT: &str = "g_0123456789";
/// Another grant: what a drop leaves behind in this process is by grant.
const ASKING: &str = "g_1111111111";
/// The grant of the hollow-answer test alone: one question is looked at
/// once in this process (`look::LOOKED`).
const HOLLOW: &str = "g_4444444444";
/// The grant of the card test alone.
const CARDS: &str = "g_5555555555";
/// The grant of the test of an empty answer kept alone.
const EMPTY: &str = "g_4444444455";
/// The grant of the test of what comes together alone: it drops its grant
/// at the end, which every test in this process then sees dropped.
const TOGETHER: &str = "g_6666666666";
/// The grant of the progress test alone.
const PROGRESS: &str = "g_7777777777";
/// The grant of the status-card test alone.
const ENDED: &str = "g_8888888888";
/// What the owner said to it: theirs alone.
const OWNERS_WORDS: &str = "把存储拆开然后提交";
/// What another session sent it, with someone else's name in it.
const PEERS_WORDS: &str = "照小林说的改";

async fn schema() -> Option<crate::db::IsolatedSchema> {
    let url = crate::db::test_url(&["MYRIAD_MEDIA_TEST_DATABASE_URL"])?;
    let schema = crate::db::IsolatedSchema::migrated(&url, "sessions").await;
    schema
        .db
        .execute_unprepared(
            "INSERT INTO users (id, username) VALUES (1, 'one'), (7, 'seven'); \
             INSERT INTO agent_sync_devices (id, user_id, name, token_hash, token_hint, created_at) \
             VALUES ('dev_mac', 1, 'Mac', 'hash', 'hint', now())",
        )
        .await
        .unwrap();
    Some(schema)
}

async fn count(db: &DatabaseConnection, sql: &str) -> i64 {
    db.query_one_raw(Statement::from_string(DatabaseBackend::Postgres, sql))
        .await
        .unwrap()
        .unwrap()
        .try_get::<i64>("", "n")
        .unwrap()
}

fn session(change: &str, at: i64) -> Session {
    under(GRANT, change, at)
}

fn under(grant: &str, change: &str, at: i64) -> Session {
    myriad_her_link::now::read_session(&json!({
        "grant": grant,
        "session": "s-1",
        "change": change,
        "occurredAt": at,
        "tool": "coding-agent",
        "remote": "github.com/me/repo",
        "title": "Split the store",
        "askedBy": "session",
        "askedByTitle": "Plan the release",
        "fromOwner": OWNERS_WORDS,
        "fromPeer": PEERS_WORDS,
        "fromAgent": "Committed locally. Push now? (push / wait)",
        "touched": ["src/a.rs"],
        "checks": [{"kind": "test", "state": "passed", "at": at - 1}],
    }))
    .unwrap()
}

/// A change told as it happened, on a link open since before it.
async fn take(
    db: &DatabaseConnection,
    owner: i32,
    device: &str,
    session: Session,
) -> Result<(), Error> {
    let at = session.occurred_at;
    super::take(db, owner, device, session, Arrival { linked_at: 0, at }).await
}

/// A model that answers each look with `answers`, in order.
fn answering(answers: &[&str]) {
    let mut world = g0::World::new(0.5);
    for answer in answers {
        world
            .models
            .entry("session_look".to_string())
            .or_default()
            .push_back(Some(answer.to_string()));
    }
    g0::enter(world);
}

fn owners() -> String {
    audience::private_talk(OWNER, Some(OWNER))
}

/// What she made of `grant`'s session `s-1`, as kept for her next look.
async fn made_of(db: &DatabaseConnection, grant: &str, session: &str) -> Option<String> {
    let mut told = under(grant, "asked", 2);
    told.session = session.into();
    let picture = Picture {
        device: DEVICE.into(),
        session: told,
    };
    keep::before(db, OWNER, &picture).await.unwrap()
}

/// Waits for the look under way to keep what it made of it.
async fn looked(db: &DatabaseConnection) -> String {
    looked_under(db, ASKING).await
}

async fn looked_under(db: &DatabaseConnection, grant: &str) -> String {
    for _ in 0..100 {
        if let Some(made) = made_of(db, grant, "s-1").await {
            return made;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("no look was kept");
}

/// What waits in every inbox: (audience, id, item).
async fn inbox_of(db: &DatabaseConnection) -> Vec<(String, String, Value)> {
    db.query_all_raw(Statement::from_string(
        DatabaseBackend::Postgres,
        "SELECT audience, id, item FROM agent_node_inbox WHERE placed_at IS NULL ORDER BY id"
            .to_string(),
    ))
    .await
    .unwrap()
    .iter()
    .map(|row| {
        (
            row.try_get("", "audience").unwrap(),
            row.try_get("", "id").unwrap(),
            row.try_get("", "item").unwrap(),
        )
    })
    .collect()
}

/// Every row of her nodes the site keeps, wherever.
async fn node_rows(db: &DatabaseConnection) -> i64 {
    count(db, "SELECT count(*) AS n FROM agent_node_rows").await
}

/// A session that asks the owner puts nothing on her nodes: she looks at
/// it once (grounds that name what she was not shown are asked again), and
/// what she made of it waits in the owner's private talk's inbox alone, a
/// third party's, for her upkeep to judge. Nothing of it is anywhere a
/// group, a visitor or her own audience reads.
#[tokio::test]
async fn an_asking_session_is_kept_for_the_owner_alone_and_looked_at_once() {
    let Some(schema) = schema().await else {
        return;
    };
    let db = &schema.db;
    // Her first answer cites what is not there; the second holds, and says
    // nothing is needed of them, which an asking session never gets.
    answering(&[
        r#"{"changes":[{"change":1,"verdict":"nod","about":"push","suggestion":"push","because":[99],"missing":null,"ownerMustSay":false}],"say":null}"#,
        r#"{"changes":[{"change":1,"verdict":"no_need","about":"它问要不要推送","suggestion":null,"because":[1],"missing":null,"ownerMustSay":false}],"say":null}"#,
    ]);
    take(db, OWNER, DEVICE, under(ASKING, "asked", 1_760_000_000_000))
        .await
        .unwrap();
    let made = looked(db).await;
    assert_eq!(g0::leave().unwrap().models["session_look"].len(), 0);
    // Both answers are in the owner's ledger, word for word.
    let ledger = keep::ledger_of(db, OWNER).await;
    assert_eq!(ledger.len(), 1);
    assert_eq!(ledger[0].answers.len(), 2);
    assert!(ledger[0].answers[0].contains("[99]"));
    // How each came beside it: why it stopped and what it wrote, as said.
    assert_eq!(ledger[0].replies.len(), 2);
    // What she made of it: theirs to nod to at least.
    assert!(made.starts_with("你点头就行"), "{made}");
    // Nothing on her nodes: a session is not a thing of hers.
    assert_eq!(node_rows(db).await, 0);
    // What she made of it waits in the owner's private talk's inbox alone,
    // one item for the session, a third party's, in her words.
    let inbox = inbox_of(db).await;
    assert_eq!(inbox.len(), 1, "{inbox:?}");
    assert_eq!(inbox[0].0, owners());
    assert_eq!(inbox[0].1, sessions::session_ref(DEVICE, ASKING, "s-1"));
    assert_eq!(inbox[0].2["saidBy"], json!(["third_party"]));
    assert_eq!(inbox[0].2["text"], "它问要不要推送");
    assert!(
        inbox[0].2["refs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reference| reference == &json!(sessions::grant_ref(DEVICE, ASKING)))
    );
    let ours = shown::shown(db, OWNER, &owners(), None).await.unwrap();
    assert!(ours.nodes.is_empty());
    let elsewhere = [
        (OWNER, audience::group("telegram:-100123")),
        (OWNER, audience::OWN.to_string()),
        (VISITOR, audience::private_talk(VISITOR, Some(OWNER))),
    ];
    for (user, audience) in &elsewhere {
        let there = shown::shown(db, *user, audience, None).await.unwrap();
        assert!(there.nodes.is_empty(), "{user} {audience}");
    }
    // The same in a group's talk and a visitor's: where each is read from.
    let group = Audience::group("telegram:-100123", OWNER);
    let home = audience::of_talk(&group, OWNER, Some(OWNER)).unwrap();
    assert_ne!(home.audience, owners());
    let visitor = audience::of_talk(&Audience::private(VISITOR), VISITOR, Some(OWNER)).unwrap();
    assert_ne!((visitor.user_id, visitor.audience), (OWNER, owners()));

    // Told again (its taken was lost): nothing more, no second look.
    take(db, OWNER, DEVICE, under(ASKING, "asked", 1_760_000_000_000))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        made_of(db, ASKING, "s-1").await.as_deref(),
        Some(made.as_str())
    );
    assert_eq!(inbox_of(db).await.len(), 1);
    schema.drop().await;
}

/// Neither answer holds (the form with nothing in it, twice): a question
/// that waits on the owner's own answer still comes to them to decide,
/// never as a nod, with no word of hers, whoever gave the work (here
/// another session); both empty answers are in the ledger as such.
#[tokio::test]
async fn what_waits_on_the_owners_answer_is_theirs_to_decide_when_her_answers_are_hollow() {
    let Some(schema) = schema().await else {
        return;
    };
    let db = &schema.db;
    let hollow = r#"{"changes":[{"change":1,"verdict":"decide","about":"","suggestion":null,"because":[],"missing":null,"ownerMustSay":false}],"say":null}"#;
    answering(&[hollow, hollow]);
    let asked = myriad_her_link::now::read_session(&json!({
        "grant": HOLLOW,
        "session": "s-1",
        "change": "asked",
        "occurredAt": 1_760_000_000_000_i64,
        "tool": "coding-agent",
        "remote": "github.com/me/repo",
        "title": "Split the store",
        "askedBy": "session",
        "askedByTitle": "Plan the release",
        "waitingOn": "owner",
        "fromOwner": OWNERS_WORDS,
        "fromAgent": "Committed locally. Push now? (push / wait)",
    }))
    .unwrap();
    take(db, OWNER, DEVICE, asked).await.unwrap();
    let made = looked_under(db, HOLLOW).await;
    assert!(made.starts_with("要你定"), "{made}");
    let ledger = keep::ledger_of(db, OWNER).await;
    let reads: Vec<Option<&str>> = ledger[0]
        .replies
        .iter()
        .map(|reply| reply.read.as_deref())
        .collect();
    assert_eq!(reads, [Some("hollow"), Some("hollow")]);
    schema.drop().await;
}

/// The form with nothing in it where every change is one she leaves to
/// itself is her answer: asked once, not again. A question that waits on
/// the owner's own answer still comes to them to decide, never as a nod;
/// the ledger keeps the answer as the empty form she was taken at.
#[tokio::test]
async fn an_empty_answer_that_leaves_all_to_itself_is_not_asked_again() {
    let Some(schema) = schema().await else {
        return;
    };
    let db = &schema.db;
    let empty = r#"{"changes":[{"change":1,"verdict":"no_need","about":"","suggestion":null,"because":[],"missing":null,"ownerMustSay":false}],"say":null}"#;
    answering(&[empty]);
    let asked = myriad_her_link::now::read_session(&json!({
        "grant": EMPTY,
        "session": "s-1",
        "change": "asked",
        "occurredAt": 1_760_000_000_000_i64,
        "tool": "coding-agent",
        "remote": "github.com/me/repo",
        "title": "Split the store",
        "waitingOn": "owner",
        "fromOwner": OWNERS_WORDS,
        "fromAgent": "Committed locally. Push now? (push / wait)",
    }))
    .unwrap();
    take(db, OWNER, DEVICE, asked).await.unwrap();
    let made = looked_under(db, EMPTY).await;
    assert!(made.starts_with("要你定"), "{made}");
    let ledger = keep::ledger_of(db, OWNER).await;
    let reads: Vec<Option<&str>> = ledger[0]
        .replies
        .iter()
        .map(|reply| reply.read.as_deref())
        .collect();
    assert_eq!(reads, [Some("hollow_kept")]);
    schema.drop().await;
}

/// A grant the owner ends is forgotten for good: the pictures, what she
/// made of its sessions, beside her nodes and in her inbox. A removed
/// device's grants go too. Nothing of a session is ever on her nodes.
#[tokio::test]
async fn a_dropped_grant_is_forgotten_for_good() {
    let Some(schema) = schema().await else {
        return;
    };
    let db = &schema.db;
    // No model: nothing is looked at; only the picture is kept.
    take(db, OWNER, DEVICE, session("started", 1_760_000_000_000))
        .await
        .unwrap();
    take(db, OWNER, DEVICE, session("finished", 1_760_000_100_000))
        .await
        .unwrap();
    assert_eq!(node_rows(db).await, 0);
    let kept = pictures(db, OWNER).await;
    assert_eq!(kept.len(), 1);
    // What a look would have kept of it.
    let judgment: myriad_nodes::session_look::Judgment = serde_json::from_value(json!({
        "verdict": "no_need", "about": "把存储拆开了", "suggestion": "", "because": [],
        "missing": null, "ownerMustSay": false,
    }))
    .unwrap();
    keep::made(db, OWNER, &kept[0], &judgment).await.unwrap();
    assert!(made_of(db, GRANT, "s-1").await.is_some());
    assert_eq!(inbox_of(db).await.len(), 1);

    forget_grant(db, OWNER, DEVICE, GRANT).await.unwrap();
    assert!(pictures(db, OWNER).await.is_empty());
    assert_eq!(made_of(db, GRANT, "s-1").await, None);
    assert!(inbox_of(db).await.is_empty());

    // A later grant, a session with no remote too: still nothing on her
    // nodes.
    let later = "g_9876543210";
    take(
        db,
        OWNER,
        DEVICE,
        under(later, "started", 1_760_000_200_000),
    )
    .await
    .unwrap();
    let mut folder = under(later, "started", 1_760_000_200_000);
    folder.session = "s-2".into();
    folder.remote = None;
    folder.folder = Some("scratch".into());
    take(db, OWNER, DEVICE, folder).await.unwrap();
    assert_eq!(pictures(db, OWNER).await.len(), 2);
    assert_eq!(node_rows(db).await, 0);

    // The device removed: every grant it was given; what it tells after
    // that is not taken.
    crate::services::persona_sync::devices::revoke(
        db,
        OWNER,
        DEVICE,
        crate::services::persona_sync::devices::RevokeReason::ByOwner,
    )
    .await
    .unwrap()
    .unwrap();
    assert!(pictures(db, OWNER).await.is_empty());
    take(db, OWNER, DEVICE, under(later, "asked", 1_760_000_300_000))
        .await
        .unwrap();
    assert!(pictures(db, OWNER).await.is_empty());
    assert_eq!(node_rows(db).await, 0);
    schema.drop().await;
}

/// A grant dropped stays dropped across a restart: a change told late under
/// it (the device sent it before it heard of the drop) is never taken.
#[tokio::test]
async fn a_late_change_under_a_dropped_grant_is_refused_after_a_restart() {
    let Some(schema) = schema().await else {
        return;
    };
    let db = &schema.db;
    let grant = "g_2222222222";
    take(
        db,
        OWNER,
        DEVICE,
        under(grant, "started", 1_760_000_000_000),
    )
    .await
    .unwrap();
    assert_eq!(pictures(db, OWNER).await.len(), 1);
    forget_grant(db, OWNER, DEVICE, grant).await.unwrap();
    assert!(pictures(db, OWNER).await.is_empty());
    look::restarted();
    take(db, OWNER, DEVICE, under(grant, "asked", 1_760_000_100_000))
        .await
        .unwrap();
    assert!(pictures(db, OWNER).await.is_empty());
    assert_eq!(node_rows(db).await, 0);
    schema.drop().await;
}

/// A question told late is not looked at: what it asked may be answered
/// by now.
#[tokio::test]
async fn a_question_told_late_keeps_its_card_and_is_not_looked_at() {
    let Some(schema) = schema().await else {
        return;
    };
    let db = &schema.db;
    answering(&[
        r#"{"changes":[{"change":1,"verdict":"nod","about":"push","suggestion":"push","because":[1],"missing":null,"ownerMustSay":false}],"say":{"change":1,"text":"推吧","because":[1]}}"#,
    ]);
    let late = "g_3333333333";
    let at = 1_760_000_000_000;
    super::take(
        db,
        OWNER,
        DEVICE,
        under(late, "asked", at),
        Arrival {
            linked_at: at + 5 * 60 * 1000,
            at: at + 5 * 60 * 1000 + 1_000,
        },
    )
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        g0::leave().unwrap().models["session_look"].len(),
        1,
        "no look was asked"
    );
    // Not looked at: nothing she made of it, nothing on her nodes.
    assert_eq!(made_of(db, late, "s-1").await, None);
    assert!(inbox_of(db).await.is_empty());
    assert_eq!(node_rows(db).await, 0);
    schema.drop().await;
}

/// What comes together is looked at once: one answer for all of it, a card
/// for what is theirs, none for what is not (what she says of it is
/// `judge`'s to test: saying drains the speaking queue tests share); the look is kept word for word
/// in the owner's ledger until the grant is dropped.
#[tokio::test]
async fn what_comes_together_is_looked_at_once_and_kept_word_for_word() {
    let Some(schema) = schema().await else {
        return;
    };
    let db = &schema.db;
    answering(&[
        r#"{"changes":[{"change":1,"verdict":"decide","about":"它问要不要推送","suggestion":"推，测试都过了","because":[4],"missing":null,"ownerMustSay":true},{"change":2,"verdict":"no_need","about":"写完了","suggestion":null,"because":[],"missing":null,"ownerMustSay":false}],"say":null}"#,
    ]);
    let grant = TOGETHER;
    let at = 1_760_000_000_000;
    take(db, OWNER, DEVICE, under(grant, "asked", at))
        .await
        .unwrap();
    let mut done = under(grant, "finished", at + 1);
    done.session = "s-2".into();
    take(db, OWNER, DEVICE, done).await.unwrap();
    let made = looked_under(db, grant).await;
    assert_eq!(
        g0::leave().unwrap().models["session_look"].len(),
        0,
        "one look for both"
    );
    assert!(made.starts_with("要你定：它问要不要推送"), "{made}");
    // Nothing for them is no card, and still hers to keep in mind: both
    // wait in her inbox, one item a session.
    assert!(made_of(db, grant, "s-2").await.is_some());
    let inbox = inbox_of(db).await;
    let texts: Vec<&str> = inbox
        .iter()
        .map(|(_, _, item)| item["text"].as_str().unwrap())
        .collect();
    assert!(
        texts.contains(&"它问要不要推送") && texts.contains(&"写完了"),
        "{texts:?}"
    );
    let ledger = keep::ledger_of(db, OWNER).await;
    assert_eq!(ledger.len(), 1);
    assert_eq!(ledger[0].grants, vec![format!("{DEVICE}/{grant}")]);
    assert_eq!(ledger[0].answers.len(), 1);
    assert!(ledger[0].material.contains("Change 2 of 2"));
    forget_grant(db, OWNER, DEVICE, grant).await.unwrap();
    assert!(keep::ledger_of(db, OWNER).await.is_empty());
    schema.drop().await;
}

/// The cards of `owner`'s private talk, by role: (role, card id, whole).
async fn card_lines(db: &DatabaseConnection) -> Vec<(String, String, bool)> {
    db.query_all_raw(Statement::from_string(
        DatabaseBackend::Postgres,
        format!(
            "SELECT role, metadata FROM agent_proactive_messages \
             WHERE user_id = {OWNER} AND metadata->>'cardId' IS NOT NULL ORDER BY id"
        ),
    ))
    .await
    .unwrap()
    .iter()
    .map(|row| {
        let metadata: Value = row.try_get("", "metadata").unwrap();
        (
            row.try_get("", "role").unwrap(),
            metadata["cardId"].as_str().unwrap().to_string(),
            metadata.get("card").is_some(),
        )
    })
    .collect()
}

/// What she makes of a question that is the owner's to decide comes into
/// their private talk at once as a card, in their language, without
/// waiting for her line; her line about it, once said, carries it; their
/// answer is kept, told to her as a fact, and is in what she knows she told
/// them; the session going on settles it, and its end has its own card.
#[tokio::test]
async fn her_card_comes_at_once_moves_to_her_line_and_settles_as_the_session_goes_on() {
    use crate::services::agent::merope::cards;
    let Some(schema) = schema().await else {
        return;
    };
    let db = &schema.db;
    db.execute_unprepared(&format!(
        "UPDATE users SET locale = 'zh-CN' WHERE id = {OWNER}"
    ))
    .await
    .unwrap();
    answering(&[
        r#"{"changes":[{"change":1,"verdict":"decide","about":"它问现在推送还是先等","suggestion":"我倾向先等测试跑完再推","because":[1],"missing":null,"ownerMustSay":true,"leans":null,"answers":true}],"say":{"change":1,"text":"推送那件事要你定一下。","because":[1]}}"#,
        r#"{"changes":[{"change":1,"verdict":"decide","about":"它又问推到哪个分支","suggestion":"我倾向推到 preview","because":[1],"missing":null,"ownerMustSay":true,"leans":null,"answers":true}],"say":null}"#,
        r#"{"changes":[{"change":1,"verdict":"decide","about":"它问下一步做什么","suggestion":"","because":[1],"missing":"它没说有哪几条路","ownerMustSay":false,"leans":null,"answers":false}],"say":null}"#,
    ]);
    take(db, OWNER, DEVICE, under(CARDS, "asked", 1_760_000_000_000))
        .await
        .unwrap();
    looked_under(db, CARDS).await;
    // At once, on a line of its own: her line has not been said.
    let mut lines = Vec::new();
    for _ in 0..100 {
        lines = card_lines(db).await;
        if !lines.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!((lines[0].0.as_str(), lines[0].2), ("card", true));
    let id = lines[0].1.clone();
    let held = cards::now(db, OWNER, &id).await.unwrap().unwrap();
    assert_eq!(held.card.kind, "decide");
    assert!(held.waiting);
    // Of the picture it is about: an end that sees the session sends a
    // choice back only while the card is of its latest picture.
    assert_eq!(held.card.at, 1_760_000_000_000);
    assert!(
        held.card.title.contains("Split the store"),
        "{}",
        held.card.title
    );
    // The program's words on it are the owner's language.
    assert!(
        held.card.facts.iter().any(|fact| fact.text == "测试通过"),
        "{:?}",
        held.card.facts
    );
    assert!(
        held.card
            .view
            .as_ref()
            .is_some_and(|view| view.text.contains("它问现在推送还是先等"))
    );

    // Her line goes out and carries it.
    let to = crate::services::agent::speech::private(OWNER);
    use crate::services::agent::speech::ledger::{self, Line, Out, Transcript};
    ledger::write(
        db,
        Line {
            to: &to,
            source: myriad_merope::speech::Source::Event,
            called: false,
            thing: None,
            text: "推送那件事要你定一下。",
        },
        Out::full("web", "face").line(Some("spk_cards".into())),
        Some(Transcript {
            event_key: crate::services::agent::merope::gates::PEER_AGENT_EVENT,
            notified: false,
            say_id: None,
            card: Some(&id),
        }),
    )
    .await
    .unwrap();
    let lines = card_lines(db).await;
    assert_eq!(
        lines,
        [
            ("card".to_string(), id.clone(), false),
            ("assistant".to_string(), id.clone(), true)
        ]
    );

    // The owner answers on the site: kept, and told to her as a fact.
    let answer = held.card.actions[0].clone();
    let answered = cards::answer(db, OWNER, &id, &held.version, &answer.id)
        .await
        .unwrap();
    assert!(answered.kept && answered.code.is_none());
    let told = crate::services::agent::consciousness::drain_speak_intents(crate::clock::now());
    assert!(
        told.iter().any(|intent| intent.user_id == OWNER
            && intent.gist == format!("站长在卡片「{}」上选了：{}", held.card.title, answer.label)),
        "{told:?}"
    );
    // What she told them of their sessions holds the card and the answer.
    let said = super::said_lately(db, OWNER).await;
    assert!(
        said.iter().any(|line| line.id == format!("card:{id}")
            && line.text.contains(&format!("站长选了「{}」", answer.label))),
        "{said:?}"
    );

    // It asks them again: the card asks anew, on a line of its own.
    let mut again = under(CARDS, "asked", 1_760_000_100_000);
    again.from_agent = Some("Pushed to which branch? (preview / main)".into());
    take(db, OWNER, DEVICE, again).await.unwrap();
    let mut lines = Vec::new();
    for _ in 0..100 {
        lines = card_lines(db).await;
        if lines.len() == 3 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert_eq!(lines[2], ("card".to_string(), id.clone(), true));
    let asks = cards::now(db, OWNER, &id).await.unwrap().unwrap();
    assert!(asks.waiting && asks.answered.is_none());
    assert_eq!(asks.card.at, 1_760_000_100_000);

    // Answered in the session instead: it went on, and the card settles.
    take(
        db,
        OWNER,
        DEVICE,
        under(CARDS, "started", 1_760_000_150_000),
    )
    .await
    .unwrap();
    let settled = cards::now(db, OWNER, &id).await.unwrap().unwrap();
    assert!(!settled.waiting);
    assert_eq!(settled.card.at, 1_760_000_150_000);
    assert!(
        settled
            .card
            .actions
            .iter()
            .all(|action| action.id == "open")
    );
    // Its end: a card of where it came to, asking nothing.
    take(
        db,
        OWNER,
        DEVICE,
        under(CARDS, "finished", 1_760_000_200_000),
    )
    .await
    .unwrap();
    let status = cards::lately(db, OWNER, 10)
        .await
        .unwrap()
        .into_iter()
        .find(|card| card.kind == "status")
        .expect("a status card");
    assert!(!status.waiting);
    assert_eq!(status.at, 1_760_000_200_000);

    // An open question she has no answer to: only going to the session,
    // so it waits on nobody here; the session going on still settles it.
    let mut open_question = under(CARDS, "asked", 1_760_000_250_000);
    open_question.from_agent = Some("What next?".into());
    take(db, OWNER, DEVICE, open_question).await.unwrap();
    let mut open = None;
    for _ in 0..100 {
        open = cards::now(db, OWNER, &id).await.unwrap().filter(|held| {
            held.card
                .view
                .as_ref()
                .is_some_and(|view| view.text.contains("它问下一步做什么"))
        });
        if open.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let open = open.expect("her card on the open question");
    assert!(!open.waiting);
    assert_eq!(open.card.actions.len(), 1);
    assert_eq!(open.card.at, 1_760_000_250_000);
    take(
        db,
        OWNER,
        DEVICE,
        under(CARDS, "started", 1_760_000_300_000),
    )
    .await
    .unwrap();
    let went_on = cards::now(db, OWNER, &id).await.unwrap().unwrap();
    assert_ne!(went_on.version, open.version);
    assert_ne!(went_on.card.state, open.card.state);
    assert_eq!(went_on.card.at, 1_760_000_300_000);
    schema.drop().await;
}

/// Her line on the repository of what she looked at is its progress card,
/// before the owner at once: the day's counts its facts, her line its view,
/// asking nothing. Nothing of it goes on a node.
#[tokio::test]
async fn her_line_on_the_repository_is_a_progress_card_and_on_no_node() {
    use crate::services::agent::merope::cards;
    let Some(schema) = schema().await else {
        return;
    };
    let db = &schema.db;
    db.execute_unprepared(&format!(
        "UPDATE users SET locale = 'zh-CN' WHERE id = {OWNER}"
    ))
    .await
    .unwrap();
    answering(&[
        r#"{"changes":[{"change":1,"verdict":"decide","about":"它问现在推送还是先等","suggestion":"我倾向先等","because":[1],"missing":null,"ownerMustSay":true,"leans":null,"answers":true}],"say":null,"repos":[{"repo":1,"now":"拆仓库做到一半，在等你定推不推"}]}"#,
    ]);
    take(
        db,
        OWNER,
        DEVICE,
        under(PROGRESS, "asked", 1_760_000_000_000),
    )
    .await
    .unwrap();
    looked_under(db, PROGRESS).await;
    let mut progress = None;
    for _ in 0..100 {
        progress = cards::lately(db, OWNER, 10)
            .await
            .unwrap()
            .into_iter()
            .find(|card| card.kind == "progress");
        if progress.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let progress = progress.expect("a progress card");
    assert!(!progress.waiting);
    assert_eq!(node_rows(db).await, 0);

    // Her card that it needs them (`decide`) still waits: its task is one
    // the progress counts take as waiting on them, until they answer it.
    let pictures = pictures(db, OWNER).await;
    let picture = pictures
        .iter()
        .find(|picture| picture.session.grant == PROGRESS)
        .unwrap();
    let task = keep::task_id(OWNER, picture).unwrap();
    assert_eq!(
        keep::tasks_asking(db, OWNER, &pictures).await,
        [task.clone()]
    );
    let decide = keep::card_of(&task, "decide").unwrap();
    let held = cards::now(db, OWNER, &decide)
        .await
        .unwrap()
        .expect("her decide card");
    assert!(held.waiting);
    cards::answer(db, OWNER, &decide, &held.version, &held.card.actions[0].id)
        .await
        .unwrap();
    assert!(keep::tasks_asking(db, OWNER, &pictures).await.is_empty());

    // The grant dropped: her line and its card go with what they rested on.
    forget_grant(db, OWNER, DEVICE, PROGRESS).await.unwrap();
    let left = cards::lately(db, OWNER, 10).await.unwrap();
    assert!(left.is_empty(), "{left:?}");
    schema.drop().await;
}

/// Her line about a session that ended carries the card of where it came
/// to, when the owner has it; one still going has none to carry.
#[tokio::test]
async fn her_line_on_a_session_that_ended_carries_its_status_card() {
    use crate::services::agent::merope::cards;
    let Some(schema) = schema().await else {
        return;
    };
    let db = &schema.db;
    take(
        db,
        OWNER,
        DEVICE,
        under(ENDED, "started", 1_760_000_000_000),
    )
    .await
    .unwrap();
    let going = pictures(db, OWNER)
        .await
        .into_iter()
        .find(|picture| picture.session.grant == ENDED)
        .unwrap();
    assert_eq!(keep::status_card(db, OWNER, &going).await, None);
    take(
        db,
        OWNER,
        DEVICE,
        under(ENDED, "finished", 1_760_000_100_000),
    )
    .await
    .unwrap();
    let ended = pictures(db, OWNER)
        .await
        .into_iter()
        .find(|picture| picture.session.grant == ENDED)
        .unwrap();
    let card = keep::status_card(db, OWNER, &ended)
        .await
        .expect("the status card");
    let held = cards::now(db, OWNER, &card).await.unwrap().unwrap();
    assert_eq!(held.card.kind, "status");
    schema.drop().await;
}
