use myriad_merope::speech::{Admission, Extent, Refusal, Source};
use sea_orm::ConnectionTrait;

use super::ledger::{self, Line, Out, Transcript};

#[test]
fn a_site_event_is_said_for_the_reason_the_core_names() {
    assert_eq!(
        super::source_of_event("agent.merope.greeting"),
        Source::Greeting
    );
    assert_eq!(super::source_of_event("agent.merope.touch"), Source::Touch);
    assert_eq!(
        super::source_of_event("agent.merope.thought"),
        Source::Wander
    );
    assert_eq!(
        super::source_of_event("agent.merope.reach_out"),
        Source::Reach
    );
    assert_eq!(
        super::source_of_event("agent.task_completed"),
        Source::WorkDone
    );
    assert_eq!(super::source_of_event("phantasi.digest"), Source::Event);
    assert_eq!(super::user_of(&super::private(7)), Some(7));
    assert_eq!(super::user_of(&super::group("telegram:-1")), None);
}

/// A line she said goes into the ledger (and, unasked in private, into her
/// transcript there); the ledger reads it back for the core, with what she
/// said before the ledger began, and the core refuses the same line again
/// (an isolated schema; skipped without a test database).
#[tokio::test]
async fn what_she_said_is_read_back_and_not_said_again() {
    let Some(fixture) = crate::federation::test_db::SchemaDb::new_or_media().await else {
        return;
    };
    let db = &fixture.db;
    db.execute_unprepared(
        "INSERT INTO users (id, username) VALUES (61, 'owner61'), (62, 'guest62')",
    )
    .await
    .unwrap();
    // Before the ledger: a line kept only in her transcript, an hour ago.
    db.execute_unprepared(
        "INSERT INTO agent_proactive_messages (user_id, role, content, event_key, notified, created_at) \
         VALUES (61, 'assistant', '早上那首歌还在转。', 'agent.merope.thought', false, now() - interval '1 hour')",
    )
    .await
    .unwrap();
    let to = super::private(61);
    let id = ledger::write(
        db,
        Line {
            to: &to,
            source: Source::Event,
            called: false,
            thing: Some("evt_61_phantasi.digest_1"),
            text: "你订的那个源更新了。",
        },
        Out::full("web", "notice"),
        Some(Transcript {
            event_key: "phantasi.digest",
            notified: true,
            say_id: None,
        }),
    )
    .await;
    assert!(id.is_some());
    // A reply is called on, and someone else's talk is not theirs.
    ledger::write(
        db,
        Line {
            to: &to,
            source: Source::Reply,
            called: true,
            thing: None,
            text: "嗯，我在。",
        },
        Out::full("web", "chat"),
        None,
    )
    .await
    .unwrap();
    ledger::write(
        db,
        Line {
            to: &super::private(62),
            source: Source::Event,
            called: false,
            thing: None,
            text: "给别人的",
        },
        Out::full("web", "face"),
        None,
    )
    .await
    .unwrap();

    let said = ledger::bearing(db, &to, None).await;
    let texts: Vec<&str> = said.iter().map(|said| said.text.as_str()).collect();
    assert_eq!(
        texts,
        ["嗯，我在。", "你订的那个源更新了。", "早上那首歌还在转。"]
    );
    assert!(said[0].called && !said[1].called);
    assert_eq!(said[2].source, Some(Source::Wander));
    let transcript: i64 = db
        .query_one_raw(sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT count(*) AS n FROM agent_proactive_messages WHERE user_id = 61",
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "n")
        .unwrap();
    assert_eq!(transcript, 2, "the unasked line, not the reply");

    let last = ledger::last_unasked_at(db, &to).await.unwrap();
    assert!(crate::clock::now() - last < chrono::Duration::minutes(1));
    assert!(
        ledger::last_about(db, 61, "evt_61_phantasi.digest_", "phantasi.digest")
            .await
            .is_some()
    );
    assert!(
        ledger::last_about(
            db,
            61,
            "evt_61_agent.merope.thought_",
            "agent.merope.thought"
        )
        .await
        .is_some(),
        "from before the ledger"
    );

    let mut wish = super::wish(to.clone(), Source::Event, "源更新了", super::private(61));
    wish.thing = Some("evt_61_phantasi.digest_1".into());
    assert!(matches!(
        super::admit::admit(db, &wish).await,
        Admission::Refused(Refusal::SameThing { .. })
    ));
    wish.thing = None;
    wish.text = Some("嗯，我在。".into());
    assert!(matches!(
        super::admit::admit(db, &wish).await,
        Admission::Refused(Refusal::Verbatim)
    ));
    wish.text = Some("另一句".into());
    let Admission::Ask { facts } = super::admit::admit(db, &wish).await else {
        panic!("hers to judge");
    };
    assert!(
        facts
            .iter()
            .any(|fact| fact.contains("你订的那个源更新了。")),
        "{facts:?}"
    );

    // A line that never went out is kept, but not as said.
    ledger::write(
        db,
        Line {
            to: &super::group("telegram:-5"),
            source: Source::GroupInterject,
            called: false,
            thing: None,
            text: "没发出去",
        },
        Out::full("group", "group").extent(Extent::None),
        None,
    )
    .await
    .unwrap();
    assert!(
        ledger::last_unasked_at(db, &super::group("telegram:-5"))
            .await
            .is_none()
    );
    fixture.close().await;
}
