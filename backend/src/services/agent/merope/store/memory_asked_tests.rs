//! What they ask her not to keep from now on is kept itself, under its own
//! source: it holds wherever they say more, is never crowded out or faded,
//! and is taken back wherever they take it back.

use super::*;
use crate::models::entities::agent_memories;
use crate::services::agent::memory::unified::{self, Audience};
use crate::services::agent::merope::chat_remember::settle_raw::{Spoken, what_she_knows};
use crate::services::agent::merope::chat_remember::{ChatMemoryUpdate, ChatMemoryUpdates};

const ASK: &str = "以后不要记和房东吵架的事";
const LATER: &str = "以后不要记和房东有关的事";

fn asks(fact: &str, replaces: &[&str]) -> ChatMemoryUpdates {
    let mut updates = letting_go("");
    updates.updates[0] = ChatMemoryUpdate {
        fact: Some(fact.into()),
        supersedes: replaces.iter().map(|old| old.to_string()).collect(),
        evidence: Some("以后别记我和房东吵架的事".into()),
        ..updates.updates[0].clone()
    };
    updates.asked.insert(0);
    updates
}

fn letting_go(entry: &str) -> ChatMemoryUpdates {
    ChatMemoryUpdates {
        updates: vec![ChatMemoryUpdate {
            standing: Default::default(),
            fact: None,
            supersedes: vec![entry.into()],
            evidence: None,
            concepts: Vec::new(),
            topic: None,
            cues: Vec::new(),
        }],
        ..Default::default()
    }
}

/// As the second look lets go of it.
async fn let_go(db: &DatabaseConnection, content: &str) {
    db.execute_unprepared(&format!(
        "UPDATE agent_memories SET invalid_at = NOW(), invalid_reason = 'superseded', \
           superseded_by = NULL \
         WHERE content = '{content}' AND source = 'asked'"
    ))
    .await
    .unwrap();
}

async fn shown(db: &DatabaseConnection, user_id: i32) -> Vec<String> {
    unified::asked_not_to_keep(db, user_id)
        .await
        .unwrap()
        .into_iter()
        .map(|row| row.content)
        .collect()
}

#[tokio::test]
async fn what_they_ask_not_to_keep_holds_everywhere_and_is_taken_back_anywhere() {
    let Some(url) = crate::db::test_url(&["MYRIAD_MEROPE_TEST_DATABASE_URL"]) else {
        return;
    };
    let schema = crate::db::IsolatedSchema::migrated(&url, "store_asked").await;
    let db = schema.db.clone();
    for user in ["(31, 'asked_a')", "(32, 'asked_b')"] {
        db.execute_unprepared(&format!("INSERT INTO users (id, username) VALUES {user}"))
            .await
            .unwrap();
    }
    let (_, state) = update_affect(&db, 31, true, AffectChange::utterance())
        .await
        .unwrap();
    let at = state.last_user_message_at.unwrap();
    let alone = Audience::private(31);
    let group = Audience::group("qq:42", 31);
    let settle = |updates: ChatMemoryUpdates, present: Audience| {
        let db = db.clone();
        async move {
            let tx = db.begin().await.unwrap();
            let kept = settle_chat_memory_on(&tx, 31, at, &updates, &[], &present)
                .await
                .unwrap();
            tx.commit().await.unwrap();
            kept
        }
    };
    let noted = |updates: ChatMemoryUpdates, present: Audience| {
        let db = db.clone();
        async move {
            let tx = db.begin().await.unwrap();
            apply_chat_memory_updates_on(&tx, 31, at, &updates, &present, Keep::Note)
                .await
                .unwrap();
            tx.commit().await.unwrap();
        }
    };

    assert!(settle(asks(ASK, &[]), alone.clone()).await);
    let row = agent_memories::Entity::find()
        .filter(agent_memories::Column::Source.eq(unified::ASKED_SOURCE))
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!((row.venue.as_str(), row.importance), ("private", 1.0));
    assert_eq!(row.user_id, Some(31));
    // Heard by no one: a build that does not know its source says nothing
    // from it, to them or in a group.
    assert_eq!(row.audience, serde_json::json!([0]));
    for present in [alone.clone(), group.clone()] {
        assert!(!unified::audience_admits(&[0], &present));
    }
    // It is not among what she knows of them (it is shown beside it), and
    // is another person's of no one.
    assert_eq!(shown(&db, 31).await, [ASK]);
    assert!(shown(&db, 32).await.is_empty());
    // Asked again, it is one thing; what may have let her keep it, not looked
    // at yet, is over.
    let another = unified::remember(
        &db,
        unified::NewMemory {
            user_id: 32,
            kind: unified::MemoryKind::Fact,
            content: "在学面包".into(),
            evidence: None,
            speaker: unified::Speaker::User,
            source: "chat",
            audience: Audience::private(32),
            importance: 0.5,
            concepts: Vec::new(),
        },
    )
    .await
    .unwrap()
    .unwrap();
    db.execute_unprepared(&format!(
        "INSERT INTO agent_takeback_check (user_id, memory_id, state, at) \
         VALUES (31, '{}', 'pending', now()), (32, '{another}', 'pending', now())",
        row.id
    ))
    .await
    .unwrap();
    assert!(!settle(asks(ASK, &[]), alone.clone()).await);
    assert_eq!(shown(&db, 31).await.len(), 1);
    let states: Vec<String> = db
        .query_all_raw(sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT state FROM agent_takeback_check ORDER BY user_id",
        ))
        .await
        .unwrap()
        .iter()
        .map(|row| row.try_get("", "state").unwrap())
        .collect();
    assert_eq!(states, ["unsure", "pending"]);
    // Asked again while that second look was being asked: its answer lets
    // nothing go.
    use crate::services::agent::merope::memory_nodes::take_back_check;
    db.execute_unprepared("UPDATE agent_takeback_check SET state = 'pending' WHERE user_id = 31")
        .await
        .unwrap();
    take_back_check::confirmed_after(&db, 31, async {
        assert!(!settle(asks(ASK, &[]), alone.clone()).await);
    })
    .await;
    assert_eq!(shown(&db, 31).await, [ASK]);
    db.execute_unprepared("DELETE FROM agent_takeback_check")
        .await
        .unwrap();

    // Nothing crowds it out of what a pass is shown, in private or in a
    // group; what is said where it was asked stays there.
    for index in 0..45 {
        let fact = ChatMemoryUpdate {
            fact: Some(format!("第{index}件事：在学做面包")),
            supersedes: Vec::new(),
            evidence: Some("在学做面包".into()),
            ..letting_go("").updates.remove(0)
        };
        settle(
            ChatMemoryUpdates {
                updates: vec![fact],
                ..Default::default()
            },
            alone.clone(),
        )
        .await;
    }
    for present in [&alone, &group] {
        let line = Spoken {
            id: 1,
            theirs: true,
            text: "今天又去学做面包了".into(),
            at,
            gone: false,
            in_game: false,
        };
        let known = what_she_knows(&db, 31, present, &[line]).await.unwrap();
        assert_eq!(known.asked, [ASK]);
        assert!(!known.known.iter().any(|fact| fact == ASK));
        assert!(known.known.len() <= 60);
    }

    // Read nowhere but to hold back what is kept: not among what she knows of
    // them, in private or in a group, nor brought to mind, nor on a node.
    let alike = "以后别记和房东吵架的事";
    for present in [&alone, &group] {
        assert!(
            unified::active_in(&db, 31, present, &unified::MemoryKind::ABOUT_PERSON)
                .await
                .unwrap()
                .iter()
                .all(|row| row.content != ASK)
        );
    }
    let (recalled, _) = unified::quietly(recall_remembered_primed(
        &db,
        31,
        &group,
        Some(alike),
        8,
        &unified::Priming::default(),
        1.0,
    ))
    .await
    .unwrap();
    assert!(recalled.iter().all(|fact| fact != ASK), "{recalled:?}");
    assert!(
        !crate::services::agent::merope::memory_nodes::sorting::SORTED
            .contains(&unified::ASKED_SOURCE)
    );

    // In a group, taking back what was asked in private lets it go; a
    // request in another place takes its place by what it says.
    settle(asks(LATER, &[ASK]), group.clone()).await;
    assert_eq!(shown(&db, 31).await, [LATER]);
    // Letting her keep it again waits for a second look, with no line to
    // look at for good.
    noted(letting_go(LATER), alone.clone()).await;
    assert_eq!(shown(&db, 31).await, [LATER]);
    let_go(&db, LATER).await;
    settle(asks(ASK, &[]), group.clone()).await;
    assert_eq!(shown(&db, 31).await, [ASK]);
    // Asked in a group, it stays between the two of them too.
    let asked_there = agent_memories::Entity::find()
        .filter(agent_memories::Column::Venue.eq("group:qq:42"))
        .all(&db)
        .await
        .unwrap();
    assert!(asked_there.iter().all(|row| row.content != ASK));
    assert!(!settle(letting_go(ASK), alone.clone()).await);
    assert_eq!(shown(&db, 31).await, [ASK]);
    let_go(&db, ASK).await;

    // In a group, a line that asks lets go at once of what she knew for good
    // of that, wherever it is kept; with something else told beside, what it
    // lets go of may be what that corrects, and waits to be gone over.
    let about = |fact: &str| ChatMemoryUpdates {
        updates: vec![ChatMemoryUpdate {
            fact: Some(fact.into()),
            supersedes: Vec::new(),
            evidence: Some(fact.into()),
            ..letting_go("").updates.remove(0)
        }],
        ..Default::default()
    };
    settle(about("和房东为空调吵过"), group.clone()).await;
    settle(about("和房东为漏水吵过"), group.clone()).await;
    let mut line = letting_go("和房东为空调吵过");
    line.updates.push(asks(ASK, &[]).updates.remove(0));
    line.asked.insert(1);
    noted(line, group.clone()).await;
    let mut line = letting_go("和房东为漏水吵过");
    line.updates.push(about("养了只龟").updates.remove(0));
    line.updates.push(asks(ASK, &[]).updates.remove(0));
    line.asked.insert(2);
    line.said_new = crate::services::agent::merope::chat_remember::SaidNew::Something;
    noted(line, group.clone()).await;
    assert_eq!(shown(&db, 31).await, [ASK]);
    let now: Vec<String> = unified::active_in(&db, 31, &group, &unified::MemoryKind::ABOUT_PERSON)
        .await
        .unwrap()
        .into_iter()
        .map(|row| row.content)
        .collect();
    for kept in ["养了只龟", "和房东为漏水吵过"] {
        assert!(now.contains(&kept.to_string()), "{now:?}");
    }
    assert!(!now.contains(&"和房东为空调吵过".to_string()));
}

/// It is recalled by no association: she is never asked what it is about.
#[tokio::test]
async fn what_they_ask_is_given_no_concepts() {
    let Some(url) = crate::db::test_url(&["MYRIAD_MEROPE_TEST_DATABASE_URL"]) else {
        return;
    };
    let schema = crate::db::IsolatedSchema::migrated(&url, "store_asked_concepts").await;
    let db = schema.db.clone();
    db.execute_unprepared("INSERT INTO users (id, username) VALUES (31, 'asked_a')")
        .await
        .unwrap();
    crate::services::agent::merope::asked::tests::asks(&db, 31, ASK).await;
    assert!(
        unified::without_concepts(&db, 31, 10)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        unified::people_without_concepts(&db, 10)
            .await
            .unwrap()
            .is_empty()
    );
    schema.drop().await;
}
