//! What she says about herself is hers: kept once, naming no one, and
//! brought back by talk that touches it. Said in a group, whoever she is
//! talking to; said in private, only in that private chat.

use super::*;
use crate::models::entities::agent_memories;
use crate::services::agent::memory::unified::{self, Audience, Concept};
use crate::services::agent::merope::chat_remember::{ChatMemoryUpdates, Said};

#[tokio::test]
async fn what_she_says_of_herself_is_kept_where_she_said_it() {
    let Some(url) = crate::db::test_url(&["MYRIAD_MEROPE_TEST_DATABASE_URL"]) else {
        return;
    };
    let schema = crate::db::IsolatedSchema::migrated(&url, "store_own_said").await;
    let db = schema.db.clone();
    let user_id = (Uuid::new_v4().as_u128() % 1_000_000_000) as i32 + 1;
    db.execute_unprepared(&format!(
        "INSERT INTO users (id, username) VALUES ({user_id}, 'own_said_{user_id}')"
    ))
    .await
    .unwrap();
    let (_, state) = update_affect(&db, user_id, true, AffectChange::utterance())
        .await
        .unwrap();
    let input_at = state.last_user_message_at.unwrap();
    let updates = ChatMemoryUpdates {
        said: vec![
            Said {
                said: "我在追《北窗》，看到第五集".into(),
                evidence: "刚看到第五集".into(),
                hers: true,
                concepts: vec![Concept {
                    name: "北窗".into(),
                    aliases: vec!["North Window".into()],
                }],
            },
            Said {
                said: "我推荐了「夜航」这首歌".into(),
                evidence: "试试「夜航」".into(),
                hers: false,
                concepts: Vec::new(),
            },
        ],
        ..Default::default()
    };
    let apply = |updates: ChatMemoryUpdates, present: Audience| {
        let db = db.clone();
        async move {
            let transaction = db.begin().await.unwrap();
            let kept = apply_chat_memory_updates_on(
                &transaction,
                user_id,
                input_at,
                &updates,
                &present,
                Keep::Note,
            )
            .await
            .unwrap();
            transaction.commit().await.unwrap();
            kept
        }
    };
    let here = Audience::private(user_id);
    assert!(apply(updates.clone(), here.clone()).await);
    let hers = agent_memories::Entity::find()
        .filter(agent_memories::Column::Source.eq(myriad_merope::own_said::SOURCE))
        .all(&db)
        .await
        .unwrap();
    assert_eq!(hers.len(), 1, "only what is about herself is hers");
    assert_eq!(hers[0].content, "我在追《北窗》，看到第五集");
    // Said in private, it stays in that private chat.
    assert_eq!(hers[0].user_id, Some(user_id));
    assert_eq!(hers[0].venue, "private");
    // What she told them stays where she said it.
    let told = agent_memories::Entity::find()
        .filter(agent_memories::Column::Source.eq(SAID_SOURCE))
        .all(&db)
        .await
        .unwrap();
    assert_eq!(told.len(), 1);
    assert_eq!(told[0].user_id, Some(user_id));
    // Said again, kept once.
    let again = ChatMemoryUpdates {
        said: vec![updates.said[0].clone()],
        ..Default::default()
    };
    assert!(!apply(again, here.clone()).await);
    // Talk that touches it brings it back there, and only there.
    let recalled = said_of_herself(&db, &here, "你看过《北窗》吗", 3).await;
    assert_eq!(recalled.len(), 1);
    assert_eq!(recalled[0].said, "我在追《北窗》，看到第五集");
    assert!(myriad_merope::own_said::section(&recalled).is_some());
    assert!(
        said_of_herself(&db, &here, "今天吃什么", 3)
            .await
            .is_empty()
    );
    let someone_else = Audience::private(user_id + 1);
    let a_group = Audience::group("telegram:-100123", user_id);
    for elsewhere in [&someone_else, &a_group] {
        assert!(
            said_of_herself(&db, elsewhere, "你看过《北窗》吗", 3)
                .await
                .is_empty(),
            "{elsewhere:?}"
        );
    }
    // Said in a group, it is hers anywhere, with no one on it.
    let in_group = ChatMemoryUpdates {
        said: vec![Said {
            said: "我在读《白塔》，读到一半".into(),
            evidence: "读到一半".into(),
            hers: true,
            concepts: Vec::new(),
        }],
        ..Default::default()
    };
    assert!(apply(in_group, a_group.clone()).await);
    let own = unified::own_rows(&db, myriad_merope::own_said::SOURCE, 10)
        .await
        .unwrap();
    assert_eq!(own.len(), 1);
    assert_eq!(own[0].user_id, None);
    assert_eq!(own[0].venue, unified::OWN_VENUE);
    for anywhere in [&here, &someone_else, &a_group] {
        let recalled = said_of_herself(&db, anywhere, "《白塔》你读完没", 3).await;
        assert_eq!(recalled.len(), 1, "{anywhere:?}");
        assert_eq!(recalled[0].said, "我在读《白塔》，读到一半");
    }
}
