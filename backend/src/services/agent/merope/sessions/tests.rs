//! The owner's sessions on her nodes, on a database of their own
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
/// What the owner said to it: theirs alone.
const OWNERS_WORDS: &str = "把存储拆开然后提交";
/// What another session sent it, with someone else's name in it.
const PEERS_WORDS: &str = "照小林说的改";

async fn schema() -> Option<crate::db::IsolatedSchema> {
    let url = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL").ok()?;
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

fn task() -> String {
    task_of(GRANT)
}

fn task_of(grant: &str) -> String {
    myriad_nodes::node_id(&owners(), &sessions::task_key(DEVICE, grant, "s-1")).unwrap()
}

/// Waits for the look under way to keep what it made of it.
async fn looked(db: &DatabaseConnection) -> Value {
    for _ in 0..100 {
        if let Some(node) = read::node(db, OWNER, &owners(), &task_of(ASKING))
            .await
            .unwrap()
            && node["views"]
                .as_object()
                .is_some_and(|views| !views.is_empty())
        {
            return node;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("no look was kept");
}

/// Every word of every row the site keeps of her nodes, wherever.
async fn rows_say(db: &DatabaseConnection, words: &str) -> i64 {
    count(
        db,
        &format!("SELECT count(*) AS n FROM agent_node_rows WHERE body LIKE '%{words}%'"),
    )
    .await
}

/// A session that asks the owner lands on her nodes by rules, in the
/// owner's private talk only, waiting on them; she looks at it once, and
/// grounds that name what she was not shown are asked again. Nothing of it
/// is anywhere a group, a visitor or her own audience reads.
#[tokio::test]
async fn an_asking_session_is_kept_for_the_owner_alone_and_looked_at_once() {
    let Some(schema) = schema().await else {
        return;
    };
    let db = &schema.db;
    // Her first answer cites what is not there; the second holds, and says
    // nothing is needed of them, which an asking session never gets.
    answering(&[
        r#"{"verdict":"nod","about":"push","suggestion":"push","because":[99],"missing":null,"say":null,"ownerMustSay":false}"#,
        r#"{"verdict":"no_need","about":"它问要不要推送","suggestion":null,"because":[1],"missing":null,"say":null,"ownerMustSay":false}"#,
    ]);
    take(db, OWNER, DEVICE, under(ASKING, "asked", 1_760_000_000_000))
        .await
        .unwrap();
    let node = looked(db).await;
    assert_eq!(g0::leave().unwrap().models["session_look"].len(), 0);
    // Waiting on them: an open item, and how it stands now, by rule.
    let open = node["open"].as_object().unwrap();
    assert_eq!(open.len(), 1);
    assert!(open.values().all(|item| item["author"] == "rule"));
    assert_eq!(node["standing"]["now"]["author"], "rule");
    // Her view: theirs to nod to at least, on grounds she was shown.
    let view = node["views"].as_object().unwrap().values().next().unwrap();
    assert_eq!(view["author"], "her");
    assert!(
        view["value"]["text"]
            .as_str()
            .unwrap()
            .starts_with("你点头就行")
    );
    assert!(
        view["value"]["because"]
            .as_array()
            .unwrap()
            .iter()
            .all(|reference| {
                let reference = reference.as_str().unwrap();
                reference.starts_with("session:") || reference.starts_with("grant:")
            })
    );
    // Kept on the site's side until both ends read rows a rule wrote.
    assert_eq!(
        count(
            db,
            "SELECT count(*) AS n FROM agent_node_rows WHERE dseq IS NOT NULL"
        )
        .await,
        0
    );
    // Only the owner's private talk holds it.
    assert_eq!(
        count(
            db,
            &format!(
                "SELECT count(*) AS n FROM agent_node_rows WHERE user_id <> {OWNER} OR audience <> '{}'",
                owners()
            )
        )
        .await,
        0
    );
    let ours = shown::shown(db, OWNER, &owners(), None).await.unwrap();
    assert!(ours.node(&task_of(ASKING)).is_some());
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
    let again = read::node(db, OWNER, &owners(), &task_of(ASKING))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(again["views"], node["views"]);
}

/// A grant the owner ends is forgotten for good: the task, every row that
/// names it, the words, the pictures; the repository stays a bare name the
/// sessions of a later grant come back to. A removed device's grants go
/// too, and its folders with no remote.
#[tokio::test]
async fn a_dropped_grant_is_forgotten_for_good() {
    let Some(schema) = schema().await else {
        return;
    };
    let db = &schema.db;
    // No model: nothing is looked at, the rules still keep it.
    take(db, OWNER, DEVICE, session("started", 1_760_000_000_000))
        .await
        .unwrap();
    take(db, OWNER, DEVICE, session("finished", 1_760_000_100_000))
        .await
        .unwrap();
    assert!(rows_say(db, OWNERS_WORDS).await > 0);
    assert_eq!(pictures(db, OWNER).await.len(), 1);
    let repo = myriad_nodes::node_id(&owners(), "repo:github.com/me/repo").unwrap();
    let made = read::node(db, OWNER, &owners(), &repo)
        .await
        .unwrap()
        .unwrap();
    assert!(!made["entries"].as_array().unwrap().is_empty());

    forget_grant(db, OWNER, DEVICE, GRANT).await.unwrap();
    for words in [OWNERS_WORDS, PEERS_WORDS, "Split the store", "Push now"] {
        assert_eq!(rows_say(db, words).await, 0, "{words}");
    }
    assert!(pictures(db, OWNER).await.is_empty());
    let left = shown::shown(db, OWNER, &owners(), None).await.unwrap();
    assert!(left.node(&task()).is_none());
    let bare = read::node(db, OWNER, &owners(), &repo)
        .await
        .unwrap()
        .unwrap();
    assert!(bare["entries"].as_array().is_none_or(Vec::is_empty));

    // A later grant: the same repository, a new task under it.
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
    let back = shown::shown(db, OWNER, &owners(), None).await.unwrap();
    let task = myriad_nodes::node_id(&owners(), &sessions::task_key(DEVICE, later, "s-1")).unwrap();
    assert!(back.node(&task).is_some());
    assert!(back.node(&repo).is_some());
    let scratch =
        myriad_nodes::node_id(&owners(), &sessions::folder_key(DEVICE, "scratch")).unwrap();
    assert!(back.node(&scratch).is_some());

    // The device removed: every grant it was given, and its folder; what it
    // tells after that is not taken.
    crate::services::persona_sync::devices::revoke(
        db,
        OWNER,
        DEVICE,
        crate::services::persona_sync::devices::RevokeReason::ByOwner,
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(rows_say(db, OWNERS_WORDS).await, 0);
    take(db, OWNER, DEVICE, under(later, "asked", 1_760_000_300_000))
        .await
        .unwrap();
    assert_eq!(rows_say(db, OWNERS_WORDS).await, 0);
    assert!(pictures(db, OWNER).await.is_empty());
    let gone = shown::shown(db, OWNER, &owners(), None).await.unwrap();
    assert!(gone.node(&task).is_none());
    assert!(gone.node(&scratch).is_none());
    assert!(gone.node(&repo).is_some());
}
