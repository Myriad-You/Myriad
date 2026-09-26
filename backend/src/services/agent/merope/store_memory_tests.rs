use super::*;
use crate::models::entities::agent_memories;
use sea_orm::{Database, Schema};
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
    }
    .insert(db)
    .await
    .unwrap()
}

async fn seed_fact(db: &DatabaseConnection, user_id: i32, content: &str) -> agent_memories::Model {
    seed_fact_at(db, user_id, content, Utc::now().fixed_offset()).await
}

async fn test_database() -> DatabaseConnection {
    let url = std::env::var("MEROPE_MEMORY_TEST_DATABASE_URL").expect("disposable DB URL");
    let db = Database::connect(url).await.unwrap();
    let name = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT current_database() AS name",
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "name")
        .unwrap();
    assert_eq!(
        name, "merope_memory_test",
        "refuse to write to any other database"
    );
    let schema = Schema::new(DatabaseBackend::Postgres);
    for mut table in [
        schema.create_table_from_entity(agent_diary::Entity),
        schema.create_table_from_entity(agent_memories::Entity),
    ] {
        table.if_not_exists();
        db.execute(&table).await.unwrap();
    }
    db
}

/// Exercises real pagination and transaction serialization, never the dev DB.
#[tokio::test]
#[ignore = "requires a disposable MEROPE_MEMORY_TEST_DATABASE_URL"]
async fn recall_and_memory_writes_cover_old_rows_duplicates_and_addressee_isolation() {
    let db = test_database().await;
    let user_id = (Uuid::new_v4().as_u128() % 1_000_000_000) as i32 + 1;
    let other_user = user_id + 1_000_000_000;

    // More than two pages, all at the same timestamp, also exercise id cursors.
    let old = seed_fact(&db, user_id, "  prefers   saffron tea  ").await;
    let now = Utc::now().fixed_offset();
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
        vec!["prefers saffron tea"]
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
    assert_eq!(recent[0], "likes jasmine tea");
    assert_eq!(
        recent
            .iter()
            .filter(|fact| fact.as_str() == "likes jasmine tea")
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
}

#[tokio::test]
#[ignore = "requires a disposable MEROPE_MEMORY_TEST_DATABASE_URL"]
async fn explicit_corrections_retire_only_scoped_facts_and_recheck_the_input_under_lock() {
    use super::super::chat_remember::ChatMemoryUpdate;
    let db = test_database().await;
    let schema = Schema::new(DatabaseBackend::Postgres);
    for mut table in [
        schema.create_table_from_entity(agent_persona::Entity),
        schema.create_table_from_entity(agent_addressee_state::Entity),
    ] {
        table.if_not_exists();
        db.execute(&table).await.unwrap();
    }
    let user_id = (Uuid::new_v4().as_u128() % 1_000_000_000) as i32 + 1;
    let other_user = user_id + 1_000_000_000;
    let (_, state) = update_affect(&db, user_id, true, |_| {}).await.unwrap();
    let input_at = state.last_user_message_at.unwrap();
    let old = seed_fact(&db, user_id, "喜欢咖啡").await;
    seed_fact(&db, other_user, "喜欢咖啡").await;
    insert_diary(&db, user_id, "喜欢咖啡", DIARY_SOURCE_EVENT)
        .await
        .unwrap();
    let correction = ChatMemoryUpdate {
        fact: Some("现在不喝咖啡".into()),
        supersedes: vec!["喜欢咖啡".into()],
        evidence: Some("我不喝咖啡了".into()),
        concepts: vec![],
    };
    assert!(
        apply_chat_memory_update(&db, user_id, input_at, &correction)
            .await
            .unwrap()
    );
    assert_eq!(
        recall_remembered(&db, user_id, Some("咖啡"), 8)
            .await
            .unwrap(),
        vec!["现在不喝咖啡"]
    );
    let retired = agent_memories::Entity::find_by_id(old.id)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(retired.invalid_reason.as_deref(), Some("superseded"));
    assert_eq!(retired.content, "喜欢咖啡");
    assert_eq!(retired.created_at, old.created_at);
    assert_eq!(
        recall_remembered(&db, other_user, None, 8).await.unwrap(),
        vec!["喜欢咖啡"]
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
        !apply_chat_memory_update(&db, user_id, input_at, &correction)
            .await
            .unwrap(),
        "replay cannot reapply retired targets"
    );
    assert!(
        !insert_remembered_if_new(&db, user_id, "喜欢咖啡", None)
            .await
            .unwrap(),
        "event cannot resurrect corrected fact"
    );

    let addition = ChatMemoryUpdate {
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
    let invalid = ChatMemoryUpdate {
        fact: Some("不应写入".into()),
        supersedes: vec!["喜欢茶".into(), "不存在的事实".into()],
        evidence: Some("更正".into()),
        concepts: vec![],
    };
    assert!(
        !apply_chat_memory_update(&db, user_id, input_at, &invalid)
            .await
            .unwrap()
    );
    assert_eq!(
        recall_remembered(&db, user_id, None, 8)
            .await
            .unwrap()
            .len(),
        2,
        "invalid edit is atomic"
    );

    let withdrawal = ChatMemoryUpdate {
        fact: None,
        supersedes: vec!["喜欢茶".into()],
        evidence: Some("茶的偏好记错了".into()),
        concepts: vec![],
    };
    assert!(
        apply_chat_memory_update(&db, user_id, input_at, &withdrawal)
            .await
            .unwrap()
    );
    assert_eq!(
        recall_remembered(&db, user_id, None, 8).await.unwrap(),
        vec!["现在不喝咖啡"]
    );
    // A fresh explicit user assertion may re-establish a withdrawn preference.
    let (_, second) = update_affect(&db, user_id, true, |_| {}).await.unwrap();
    let second_input = second.last_user_message_at.unwrap();
    assert!(
        apply_chat_memory_update(&db, user_id, second_input, &addition)
            .await
            .unwrap()
    );
    assert!(
        !apply_chat_memory_update(&db, user_id, input_at, &withdrawal)
            .await
            .unwrap(),
        "late old input cannot retract a newer assertion"
    );

    // The stale check must happen after acquiring the lock, not before waiting.
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
    let locked = get_or_create_state(&transaction, user_id).await.unwrap();
    save_affect_on(
        &transaction,
        locked,
        affect_from_state(&second),
        true,
        false,
    )
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    assert!(!late.await.unwrap());
    assert!(
        recall_remembered(&db, user_id, None, 8)
            .await
            .unwrap()
            .contains(&"喜欢茶".into())
    );
}
