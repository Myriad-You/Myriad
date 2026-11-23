use super::*;
use crate::models::entities::agent_memories;
use serde_json::json;

async fn seed_fact_at(
    db: &DatabaseConnection,
    user_id: i32,
    content: &str,
    at: chrono::DateTime<chrono::FixedOffset>,
) -> agent_memories::Model {
    agent_memories::ActiveModel {
        id: Set(format!("mem_{}", Uuid::new_v4().simple())),
        user_id: Set(Some(user_id)),
        kind: Set("fact".into()),
        content: Set(content.into()),
        evidence: Set(None),
        speaker: Set("user".into()),
        source: Set("chat".into()),
        venue: Set("private".into()),
        audience: Set(json!([user_id])),
        concepts: Set(json!([])),
        importance: Set(0.5),
        access_count: Set(0),
        last_accessed_at: Set(None),
        valid_from: Set(at),
        invalid_at: Set(None),
        invalid_reason: Set(None),
        created_at: Set(at),
        updated_at: Set(at),
        superseded_by: Set(None),
        origin: Set(crate::services::agent::memory::unified::ORIGIN_SITE.into()),
    }
    .insert(db)
    .await
    .unwrap()
}

async fn seed_fact(db: &DatabaseConnection, user_id: i32, content: &str) -> agent_memories::Model {
    seed_fact_at(db, user_id, content, crate::clock::now().fixed_offset()).await
}

/// A migrated schema of its own on the test database, for `users`: never
/// the dev database, never its default schema.
async fn test_database(users: &[i32]) -> Option<crate::db::IsolatedSchema> {
    let url = crate::db::test_url(&["MYRIAD_MEROPE_TEST_DATABASE_URL"])?;
    let schema = crate::db::IsolatedSchema::migrated(&url, "store_memory").await;
    for user in users {
        schema
            .db
            .execute_unprepared(&format!(
                "INSERT INTO users (id, username) VALUES ({user}, 'memory_{user}')"
            ))
            .await
            .unwrap();
    }
    Some(schema)
}

/// Exercises real pagination and transaction serialization, never the dev DB.
#[tokio::test]
async fn recall_and_memory_writes_cover_old_rows_duplicates_and_addressee_isolation() {
    let user_id = (Uuid::new_v4().as_u128() % 1_000_000_000) as i32 + 1;
    let other_user = user_id + 1_000_000_000;
    let Some(schema) = test_database(&[user_id, other_user]).await else {
        return;
    };
    let db = schema.db.clone();

    // More than two pages, all at the same timestamp, also exercise id cursors.
    let old = seed_fact(&db, user_id, "  prefers   saffron tea  ").await;
    let now = crate::clock::now().fixed_offset();
    for index in 0..260 {
        seed_fact_at(&db, user_id, &format!("unrelated recent fact {index}"), now).await;
    }
    seed_fact(&db, other_user, "saffron tea with honey").await;
    insert_diary(&db, user_id, "saffron tea tool failed", DIARY_SOURCE_EVENT)
        .await
        .unwrap();
    insert_diary(
        &db,
        user_id,
        "saffron tea assistant guess",
        DIARY_SOURCE_CHAT,
    )
    .await
    .unwrap();

    assert_eq!(
        recall_remembered(&db, user_id, Some("saffron tea"), 8)
            .await
            .unwrap(),
        vec![dated("prefers saffron tea")]
    );
    assert!(
        !insert_remembered_if_new(&db, user_id, "prefers saffron tea", None)
            .await
            .unwrap()
    );
    assert_eq!(
        agent_memories::Entity::find_by_id(old.id)
            .one(&db)
            .await
            .unwrap()
            .unwrap()
            .content,
        "  prefers   saffron tea  ",
        "dedup must not rewrite historical rows"
    );

    // Concurrent Chat/event writers must serialize their read-check-insert.
    let (first, second) = tokio::join!(
        insert_remembered_if_new(&db, user_id, "likes jasmine tea", None),
        insert_remembered_if_new(&db, user_id, "  likes   jasmine tea ", None),
    );
    assert_ne!(
        first.unwrap(),
        second.unwrap(),
        "exactly one writer inserts"
    );
    assert!(
        insert_remembered_if_new(&db, other_user, "likes jasmine tea", None)
            .await
            .unwrap()
    );
    assert!(
        !insert_remembered_if_new(&db, -1, "guest memory", None)
            .await
            .unwrap()
    );
    assert!(
        !insert_remembered_if_new(&db, 0, "system memory", None)
            .await
            .unwrap()
    );
    assert!(
        !insert_remembered_if_new(&db, user_id, "{\"secret\":true}", None)
            .await
            .unwrap()
    );

    // No-query recall also fills its budget through legacy blank/duplicate rows.
    for _ in 0..10 {
        seed_fact(&db, user_id, "likes jasmine tea").await;
        seed_fact(&db, user_id, " ").await;
    }
    let recent = recall_remembered(&db, user_id, None, 8).await.unwrap();
    assert_eq!(recent.len(), 8);
    assert_eq!(recent[0], dated("likes jasmine tea"));
    assert_eq!(
        recent
            .iter()
            .filter(|fact| **fact == dated("likes jasmine tea"))
            .count(),
        1
    );
    assert_eq!(
        recall_remembered(&db, user_id, Some("  "), 8)
            .await
            .unwrap(),
        recent
    );
    assert!(
        recall_remembered(&db, user_id, Some("tea"), 0)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        recall_remembered(&db, -1, Some("tea"), 8)
            .await
            .unwrap()
            .is_empty()
    );
    schema.drop().await;
}

#[tokio::test]
async fn explicit_corrections_retire_only_scoped_facts_and_recheck_the_input_under_lock() {
    use super::super::chat_remember::ChatMemoryUpdate;
    let user_id = (Uuid::new_v4().as_u128() % 1_000_000_000) as i32 + 1;
    let other_user = user_id + 1_000_000_000;
    let Some(schema) = test_database(&[user_id, other_user]).await else {
        return;
    };
    let db = schema.db.clone();
    let (_, state) = update_affect(&db, user_id, true, AffectChange::utterance())
        .await
        .unwrap();
    let input_at = state.last_user_message_at.unwrap();
    let old = seed_fact(&db, user_id, "喜欢咖啡").await;
    seed_fact(&db, other_user, "喜欢咖啡").await;
    insert_diary(&db, user_id, "喜欢咖啡", DIARY_SOURCE_EVENT)
        .await
        .unwrap();
    let correction = ChatMemoryUpdate {
        standing: Default::default(),
        topic: None,
        cues: Vec::new(),
        fact: Some("现在不喝咖啡".into()),
        supersedes: vec!["喜欢咖啡".into()],
        evidence: Some("我不喝咖啡了".into()),
        concepts: vec![],
    };
    // Said in passing, it is a short note: what she knows for good is not
    // changed by a line, only once the stretch is gone over.
    assert!(
        apply_chat_memory_update(&db, user_id, input_at, &correction)
            .await
            .unwrap()
    );
    let mut both = recall_remembered(&db, user_id, Some("咖啡"), 8)
        .await
        .unwrap();
    both.sort();
    assert_eq!(both, [dated("喜欢咖啡"), dated("现在不喝咖啡")]);
    let note = agent_memories::Entity::find()
        .filter(agent_memories::Column::UserId.eq(user_id))
        .filter(agent_memories::Column::Content.eq("现在不喝咖啡"))
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        note.source,
        crate::services::agent::memory::unified::SHORT_NOTE_SOURCE
    );
    let still = agent_memories::Entity::find_by_id(old.id.clone())
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        still.invalid_at, None,
        "a line does not change a lasting fact"
    );

    // Gone over, the stretch keeps it for good in place of the old fact.
    let settle = |update: &ChatMemoryUpdate, notes: Vec<String>| {
        let db = db.clone();
        let updates = super::super::chat_remember::ChatMemoryUpdates {
            updates: vec![update.clone()],
            ..Default::default()
        };
        async move {
            let transaction = db.begin().await.unwrap();
            let kept = settle_chat_memory_on(
                &transaction,
                user_id,
                input_at,
                &updates,
                &[notes],
                &crate::services::agent::memory::unified::Audience::private(user_id),
            )
            .await
            .unwrap();
            transaction.commit().await.unwrap();
            kept
        }
    };
    assert!(settle(&correction, vec![note.id.clone()]).await);
    assert_eq!(
        recall_remembered(&db, user_id, Some("咖啡"), 8)
            .await
            .unwrap(),
        vec![dated("现在不喝咖啡")]
    );
    let retired = agent_memories::Entity::find_by_id(old.id)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(retired.invalid_reason.as_deref(), Some("superseded"));
    assert_eq!(retired.content, "喜欢咖啡");
    // The corrected row names what holds the fact now, and so does the note.
    let corrected = agent_memories::Entity::find()
        .filter(agent_memories::Column::UserId.eq(user_id))
        .filter(agent_memories::Column::Content.eq("现在不喝咖啡"))
        .filter(agent_memories::Column::Source.eq("chat"))
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        retired.superseded_by.as_deref(),
        Some(corrected.id.as_str())
    );
    let settled = agent_memories::Entity::find_by_id(note.id)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(settled.invalid_reason.as_deref(), Some("settled"));
    assert_eq!(
        settled.superseded_by.as_deref(),
        Some(corrected.id.as_str())
    );
    assert_eq!(retired.created_at, old.created_at);
    assert_eq!(
        recall_remembered(&db, other_user, None, 8).await.unwrap(),
        vec![dated("喜欢咖啡")]
    );
    assert_eq!(
        latest_diary(&db, user_id, DIARY_SOURCE_EVENT)
            .await
            .unwrap()
            .unwrap()
            .content,
        "喜欢咖啡"
    );
    assert!(
        !settle(&correction, Vec::new()).await,
        "replay cannot reapply retired targets"
    );
    assert!(
        !insert_remembered_if_new(&db, user_id, "喜欢咖啡", None)
            .await
            .unwrap(),
        "event cannot resurrect corrected fact"
    );

    let addition = ChatMemoryUpdate {
        standing: Default::default(),
        topic: None,
        cues: Vec::new(),
        fact: Some("喜欢茶".into()),
        supersedes: vec![],
        evidence: Some("我也喜欢茶".into()),
        concepts: vec![],
    };
    assert!(
        apply_chat_memory_update(&db, user_id, input_at, &addition)
            .await
            .unwrap()
    );
    assert_eq!(
        recall_remembered(&db, user_id, None, 8)
            .await
            .unwrap()
            .len(),
        2,
        "addition keeps unrelated facts"
    );
    let tea = agent_memories::Entity::find()
        .filter(agent_memories::Column::UserId.eq(user_id))
        .filter(agent_memories::Column::Content.eq("喜欢茶"))
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert!(settle(&addition, vec![tea.id]).await);
    // What it names that is not there is left out of it; it is kept.
    let of_none = ChatMemoryUpdate {
        standing: Default::default(),
        topic: None,
        cues: Vec::new(),
        fact: Some("也喜欢果汁".into()),
        supersedes: vec!["不存在的事实".into()],
        evidence: Some("更正".into()),
        concepts: vec![],
    };
    assert!(settle(&of_none, Vec::new()).await);
    let there = recall_remembered(&db, user_id, None, 8).await.unwrap();
    assert_eq!(there.len(), 3, "nothing else is replaced");
    // Of what it names, what is there is replaced and the rest left out.
    let of_some = ChatMemoryUpdate {
        fact: Some("只喜欢果汁".into()),
        supersedes: vec!["也喜欢果汁".into(), "不存在的事实".into()],
        ..of_none
    };
    assert!(settle(&of_some, Vec::new()).await);
    let there = recall_remembered(&db, user_id, None, 8).await.unwrap();
    assert_eq!(there.len(), 3, "{there:?}");
    assert!(there.contains(&dated("只喜欢果汁")) && !there.contains(&dated("也喜欢果汁")));
    let juice = async |content: &str| {
        agent_memories::Entity::find()
            .filter(agent_memories::Column::UserId.eq(user_id))
            .filter(agent_memories::Column::Content.eq(content))
            .one(&db)
            .await
            .unwrap()
            .unwrap()
    };
    let (was, only) = (juice("也喜欢果汁").await, juice("只喜欢果汁").await);
    assert_eq!(was.invalid_reason.as_deref(), Some("superseded"));
    assert_eq!(was.superseded_by.as_deref(), Some(only.id.as_str()));
    assert!(only.invalid_at.is_none());
    db.execute_unprepared("DELETE FROM agent_memories WHERE content LIKE '%喜欢果汁'")
        .await
        .unwrap();

    // A line that only takes something back lets it go wherever it is
    // kept, as a line is gone over.
    let line = |at, update: &ChatMemoryUpdate| {
        let db = db.clone();
        let updates = super::super::chat_remember::ChatMemoryUpdates {
            updates: vec![update.clone()],
            ..Default::default()
        };
        async move {
            let transaction = db.begin().await.unwrap();
            let kept = apply_chat_memory_updates_on(
                &transaction,
                user_id,
                at,
                &updates,
                &crate::services::agent::memory::unified::Audience::private(user_id),
                Keep::Note,
            )
            .await
            .unwrap();
            transaction.commit().await.unwrap();
            kept
        }
    };
    let withdrawal = ChatMemoryUpdate {
        standing: Default::default(),
        topic: None,
        cues: Vec::new(),
        fact: None,
        supersedes: vec!["喜欢茶".into()],
        evidence: Some("茶的偏好记错了".into()),
        concepts: vec![],
    };
    assert!(line(input_at, &withdrawal).await);
    assert_eq!(
        recall_remembered(&db, user_id, None, 8).await.unwrap(),
        vec![dated("现在不喝咖啡")]
    );
    // A fresh explicit user assertion may re-establish a withdrawn preference.
    let (_, second) = update_affect(&db, user_id, true, AffectChange::utterance())
        .await
        .unwrap();
    let second_input = second.last_user_message_at.unwrap();
    assert!(
        apply_chat_memory_update(&db, user_id, second_input, &addition)
            .await
            .unwrap()
    );
    assert!(
        !line(input_at, &withdrawal).await,
        "late old input cannot retract a newer assertion"
    );

    // The check must happen after acquiring the lock, not before waiting: a
    // newer input gone over while this one waited wins.
    let transaction = db.begin().await.unwrap();
    lock_addressee(&transaction, user_id).await.unwrap();
    let mut late = Box::pin(apply_chat_memory_update(
        &db,
        user_id,
        second_input,
        &withdrawal,
    ));
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(30), &mut late)
            .await
            .is_err()
    );
    let newer = ChatMemoryUpdate {
        standing: Default::default(),
        topic: None,
        cues: Vec::new(),
        fact: Some("喜欢抹茶".into()),
        supersedes: vec![],
        evidence: Some("我喜欢抹茶".into()),
        concepts: vec![],
    };
    assert!(
        apply_chat_memory_update_on(
            &transaction,
            user_id,
            second_input + chrono::Duration::seconds(1),
            &newer,
            &crate::services::agent::memory::unified::Audience::private(user_id),
            Keep::Note,
        )
        .await
        .unwrap()
    );
    transaction.commit().await.unwrap();
    assert!(!late.await.unwrap());
    assert!(
        recall_remembered(&db, user_id, None, 8)
            .await
            .unwrap()
            .contains(&dated("喜欢茶"))
    );
    schema.drop().await;
}

/// What they told her today, as she holds it: placed in time.
fn dated(fact: &str) -> String {
    format!(
        "[{}] {fact}",
        super::super::clock::local_now().format("%m-%d")
    )
}
