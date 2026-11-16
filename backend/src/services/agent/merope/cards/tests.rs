//! Her cards in the owner's talk, on an isolated schema (skipped without a
//! test database).

use myriad_persona_log::payloads::{CardAction, CardFact, CardVersion};
use sea_orm::ConnectionTrait;
use serde_json::Value;

use super::*;

const OWNER: i32 = 61;

fn card(kind: &str, title: &str, actions: &[(&str, &str)]) -> CardVersion {
    let subject = "node:n_task_1".to_string();
    CardVersion {
        at: crate::clock::now().timestamp_millis(),
        id: myriad_nodes::cards::card_id("agents", kind, &subject).unwrap(),
        kind: kind.into(),
        scene: "agents".into(),
        subject: Some(subject),
        title: title.into(),
        state: None,
        facts: vec![CardFact {
            text: "Session: Split the store".into(),
            reference: None,
        }],
        view: None,
        actions: actions
            .iter()
            .map(|(id, label)| CardAction {
                id: (*id).into(),
                label: (*label).into(),
            })
            .collect(),
    }
}

fn asking(title: &str) -> CardVersion {
    card("decide", title, &[("agree", "照这样"), ("open", "打开")])
}

async fn lines(db: &impl ConnectionTrait) -> Vec<(i64, String, Option<Value>)> {
    db.query_all_raw(Statement::from_string(
        DatabaseBackend::Postgres,
        format!(
            "SELECT id, role, metadata FROM agent_proactive_messages WHERE user_id = {OWNER} ORDER BY id"
        ),
    ))
    .await
    .unwrap()
    .iter()
    .map(|row| {
        (
            row.try_get("", "id").unwrap(),
            row.try_get("", "role").unwrap(),
            row.try_get("", "metadata").unwrap(),
        )
    })
    .collect()
}

async fn fixture() -> Option<crate::federation::test_db::SchemaDb> {
    let fixture = crate::federation::test_db::SchemaDb::new_or_media().await?;
    fixture
        .db
        .execute_unprepared(&format!(
            "INSERT INTO users (id, username) VALUES ({OWNER}, 'owner{OWNER}')"
        ))
        .await
        .unwrap();
    Some(fixture)
}

/// A card comes on a line of its own; the same again changes nothing; a
/// newer one that still waits takes its place; answered, a version that
/// asks again comes anew, and the answer to the old one is stale.
#[tokio::test]
async fn a_card_comes_once_changes_in_place_and_asks_anew_on_a_new_line() {
    let Some(fixture) = fixture().await else {
        return;
    };
    let db = &fixture.db;
    let first = asking("发版说明写不写已知问题");
    let id = first.id.clone();
    assert_eq!(put(db, OWNER, first.clone()).await.unwrap(), Put::New);
    let mut again = first.clone();
    again.at += 5_000;
    assert_eq!(put(db, OWNER, again).await.unwrap(), Put::Same);
    let mut newer = first.clone();
    newer.title = "发版说明里写不写已知问题".into();
    assert_eq!(put(db, OWNER, newer).await.unwrap(), Put::Changed);
    let kept = lines(db).await;
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].1, CARD_ROLE);
    let held = now(db, OWNER, &id).await.unwrap().unwrap();
    assert!(held.waiting);
    assert_eq!(held.card.title, "发版说明里写不写已知问题");

    // Answered: no longer waiting, and the same answer again is one.
    let version = held.version.clone();
    let answered = answer(db, OWNER, &id, &version, "agree").await.unwrap();
    assert!(answered.kept && answered.code.is_none());
    let now_held = answered.now.unwrap();
    assert!(!now_held.waiting);
    assert_eq!(now_held.answered.as_ref().unwrap().action, "agree");
    assert!(
        answer(db, OWNER, &id, &version, "agree")
            .await
            .unwrap()
            .kept
    );
    let told = lately(db, OWNER, LATELY).await.unwrap();
    assert_eq!(told.len(), 1);
    assert_eq!(
        told[0].told(),
        "卡片「发版说明里写不写已知问题」（要你定）：站长选了「照这样」"
    );

    // It asks again: on a new line, which holds the card whole.
    let mut asks_again = first.clone();
    asks_again.title = "已知问题放在哪一节".into();
    assert_eq!(put(db, OWNER, asks_again).await.unwrap(), Put::New);
    let kept = lines(db).await;
    assert_eq!(kept.len(), 2);
    assert!(kept[0].2.as_ref().unwrap().get("card").is_none());
    assert_eq!(kept[0].2.as_ref().unwrap()["cardId"], id.as_str());
    assert!(
        kept[1].2.as_ref().unwrap()["card"]["waiting"]
            .as_bool()
            .unwrap()
    );

    // An answer to the version they saw before: kept as stale, and the
    // latest still waits.
    let stale = answer(db, OWNER, &id, &version, "agree").await.unwrap();
    assert_eq!(stale.code, Some("card_stale"));
    assert!(stale.now.unwrap().waiting);
    fixture.close().await;
}

/// What cannot be an answer is refused for its own reason.
#[tokio::test]
async fn an_answer_that_is_none_is_refused_by_its_code() {
    let Some(fixture) = fixture().await else {
        return;
    };
    let db = &fixture.db;
    let waiting = asking("要不要拆分仓库");
    let id = waiting.id.clone();
    put(db, OWNER, waiting).await.unwrap();
    let version = now(db, OWNER, &id).await.unwrap().unwrap().version;
    let code = |answered: Answered| (answered.kept, answered.code);
    assert_eq!(
        code(
            answer(
                db,
                OWNER,
                "c_00000000000000000000000000000000",
                &version,
                "agree"
            )
            .await
            .unwrap()
        ),
        (false, Some("card_unknown"))
    );
    assert_eq!(
        code(answer(db, OWNER, &id, "v_nope", "agree").await.unwrap()),
        (false, Some("card_unknown"))
    );
    assert_eq!(
        code(answer(db, OWNER, &id, &version, "open").await.unwrap()),
        (false, Some("card_action_unknown"))
    );
    assert_eq!(
        code(answer(db, OWNER, &id, &version, "nope").await.unwrap()),
        (false, Some("card_action_unknown"))
    );
    // Settled: that version asks nothing.
    let settled = card("decide", "要不要拆分仓库", &[("open", "打开")]);
    assert_eq!(put(db, OWNER, settled).await.unwrap(), Put::Changed);
    let closed = now(db, OWNER, &id).await.unwrap().unwrap();
    assert!(!closed.waiting);
    assert_eq!(
        code(
            answer(db, OWNER, &id, &closed.version, "agree")
                .await
                .unwrap()
        ),
        (false, Some("card_closed"))
    );
    fixture.close().await;
}

/// Her line about a card carries it: the card moves to her line, and the
/// line it was on keeps only its id. The site's chat shows both where the
/// chat stood.
#[tokio::test]
async fn her_line_carries_the_card_and_the_chat_shows_it_in_place() {
    let Some(fixture) = fixture().await else {
        return;
    };
    let db = &fixture.db;
    db.execute_unprepared(&format!(
        "INSERT INTO agent_sessions (id, user_id, title, context, message_count, archived, \
         created_at, last_active_at) VALUES ('s_c', {OWNER}, NULL, '{{\"mode\":\"chat\"}}', 1, \
         false, now() - interval '1 hour', now() - interval '1 hour')"
    ))
    .await
    .unwrap();
    db.execute_unprepared(
        "INSERT INTO agent_messages (session_id, role, content, created_at) VALUES \
         ('s_c', 'user', '在吗', now() - interval '1 hour')",
    )
    .await
    .unwrap();
    let waiting = asking("这次要不要回滚");
    let id = waiting.id.clone();
    put(db, OWNER, waiting).await.unwrap();
    let said = crate::services::agent::merope::store::insert_proactive(
        db,
        OWNER,
        "回滚那件事要你定一下。",
        Some("agent.merope.peer_agent"),
        false,
        Some("say_1"),
    )
    .await
    .unwrap();
    carry(db, OWNER, said.id, &id, "spk_1").await.unwrap();
    // Cut off while said aloud, and answered meanwhile: both stand.
    let version = now(db, OWNER, &id).await.unwrap().unwrap().version;
    assert!(
        crate::services::agent::merope::store::mark_line_cut_off(db, OWNER, "say_1", Some(4))
            .await
            .unwrap()
    );
    assert!(
        answer(db, OWNER, &id, &version, "agree")
            .await
            .unwrap()
            .kept
    );
    let marked = lines(db).await[1].2.clone().unwrap();
    assert_eq!(marked["heardChars"], 4);
    assert_eq!(marked["card"]["answered"]["action"], "agree");
    let kept = lines(db).await;
    assert_eq!(kept.len(), 2);
    assert!(kept[0].2.as_ref().unwrap().get("card").is_none());
    let carried = kept[1].2.as_ref().unwrap();
    assert_eq!(carried["line"], "spk_1");
    assert_eq!(carried["card"]["card"]["id"], id.as_str());
    assert_eq!(kept[1].1, "assistant");

    let shown = lines_in(db, OWNER, "s_c").await.unwrap();
    assert_eq!(shown.len(), 2);
    assert_eq!(shown[0]["role"], CARD_ROLE);
    assert!(shown[0].get("card").is_none());
    assert_eq!(shown[1]["content"], "回滚那件事要你定一下。");
    assert_eq!(shown[1]["card"]["waiting"], false);
    assert!(lines_in(db, OWNER, "s_other").await.unwrap().is_empty());
    // Not one of her lines: what counts what she said does not count it.
    let recent = crate::services::agent::merope::store::recent_proactive(db, OWNER, 10)
        .await
        .unwrap();
    assert_eq!(recent.len(), 1);
    fixture.close().await;
}
