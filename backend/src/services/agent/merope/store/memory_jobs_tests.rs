use super::super::super::{TurnContext, call, chat_remember::ChatMemoryUpdate, memory_jobs};
use super::*;
use crate::services::agent::memory::unified::Audience;
use sea_orm::{ActiveModelTrait, ActiveValue::Set, IntoActiveModel};

async fn fixture() -> crate::db::IsolatedSchema {
    let url =
        std::env::var("MYRIAD_MEROPE_TEST_DATABASE_URL").expect("explicit disposable database");
    let schema = crate::db::IsolatedSchema::migrated(&url, "memory_jobs").await;
    schema
        .db
        .execute_unprepared("INSERT INTO users (id, username) VALUES (7, 'memory_test')")
        .await
        .unwrap();
    schema
}
async fn chat(db: &DatabaseConnection, id: &str, audience: Audience) {
    let mut state = super::super::get_or_create_state(db, 7)
        .await
        .unwrap()
        .into_active_model();
    let input = chrono::Utc::now().fixed_offset();
    state.last_user_message_at = Set(Some(input));
    let input = state
        .update(db)
        .await
        .unwrap()
        .last_user_message_at
        .unwrap();
    enqueue(
        db,
        id,
        7,
        Payload::Chat {
            user_text: "我养了一只猫叫年糕".into(),
            reply: "年糕真可爱".into(),
            input_at: input,
            present: audience,
            turn: TurnContext::default(),
        },
        None,
    )
    .await
    .unwrap();
}
fn fact() -> Effect {
    Effect::Chat(
        crate::services::agent::merope::chat_remember::ChatMemoryUpdates {
            updates: vec![ChatMemoryUpdate {
                fact: Some("养了一只猫叫年糕".into()),
                supersedes: vec![],
                evidence: Some("我养了一只猫叫年糕".into()),
                concepts: vec![],
            }],
            said: vec![],
        },
    )
}
async fn loaded(db: &DatabaseConnection, id: &str) -> Job {
    registry::get(db, NAMESPACE, id).await.unwrap().unwrap()
}
async fn due(db: &DatabaseConnection, id: &str) {
    db.execute_raw(Statement::from_sql_and_values(DatabaseBackend::Postgres,
        "UPDATE runtime_registry SET payload = jsonb_set(jsonb_set(payload, '{ready}', '0'), '{lease}', '0') WHERE namespace=$1 AND record_id=$2",
        [NAMESPACE.into(), id.into()])).await.unwrap();
}
async fn facts(db: &DatabaseConnection) -> i64 {
    db.query_one_raw(Statement::from_string(
        DatabaseBackend::Postgres,
        "SELECT COUNT(*) AS n FROM agent_memories WHERE invalid_at IS NULL".to_string(),
    ))
    .await
    .unwrap()
    .unwrap()
    .try_get("", "n")
    .unwrap()
}

#[tokio::test]
#[ignore = "requires MYRIAD_MEROPE_TEST_DATABASE_URL; isolated migrated schema"]
async fn durable_memory_claim_fencing_and_atomic_acknowledgement() {
    let schema = fixture().await;
    let db = &schema.db;
    chat(db, "one", Audience::group("telegram:1", 7)).await;
    let claims = futures::future::join_all((0..8).map(|_| claim(db))).await;
    let mut claims: Vec<_> = claims.into_iter().filter_map(Result::unwrap).collect();
    assert_eq!(claims.len(), 1);
    let old = claims.pop().unwrap();
    assert!(claim(db).await.unwrap().is_none());
    due(db, "one").await;
    let recovered = claim(db).await.unwrap().unwrap();
    assert_ne!(old.job.token, recovered.job.token);
    finish(db, &old, Ok(fact())).await.unwrap();
    assert_eq!(facts(db).await, 0, "expired worker cannot write");
    // Force acknowledgement failure AFTER the memory INSERT. Both must roll back.
    db.execute_unprepared("CREATE FUNCTION reject_memory_ack() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.namespace = 'merope_memory_jobs' AND NEW.payload->'data' = 'null'::jsonb THEN RAISE EXCEPTION 'test ack failure'; END IF; RETURN NEW; END $$").await.unwrap();
    db.execute_unprepared("CREATE TRIGGER reject_ack BEFORE UPDATE ON runtime_registry FOR EACH ROW EXECUTE FUNCTION reject_memory_ack()").await.unwrap();
    assert!(finish(db, &recovered, Ok(fact())).await.is_err());
    assert_eq!(facts(db).await, 0);
    assert!(loaded(db, "one").await.data.is_some());
    db.execute_unprepared("DROP TRIGGER reject_ack ON runtime_registry")
        .await
        .unwrap();
    finish(db, &recovered, Ok(fact())).await.unwrap();
    finish(db, &recovered, Ok(fact())).await.unwrap();
    assert_eq!(facts(db).await, 1);
    let row = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT venue, audience FROM agent_memories WHERE invalid_at IS NULL".to_string(),
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        row.try_get::<String>("", "venue").unwrap(),
        "group:telegram:1"
    );
    assert_eq!(
        row.try_get::<serde_json::Value>("", "audience").unwrap(),
        serde_json::json!([7])
    );
    assert!(
        loaded(db, "one").await.data.is_none(),
        "raw text removed after acknowledgement"
    );
    schema.drop().await;
}

#[tokio::test]
#[ignore = "requires MYRIAD_MEROPE_TEST_DATABASE_URL; isolated migrated schema"]
async fn durable_memory_retries_are_finite_and_revalidate_input_persona_and_session() {
    let schema = fixture().await;
    let db = &schema.db;
    chat(db, "retry", Audience::private(7)).await;
    for attempt in 1..=3 {
        let claimed = claim(db).await.unwrap().unwrap();
        assert_eq!(claimed.job.attempts, attempt);
        finish(db, &claimed, Err(Failure::Model(call::Failure::Timeout)))
            .await
            .unwrap();
        assert!(claim(db).await.unwrap().is_none(), "backoff or terminal");
        if attempt < 3 {
            due(db, "retry").await;
        }
    }
    assert!(loaded(db, "retry").await.data.is_none());
    for (id, failure) in [
        ("denied", call::Failure::Rejected),
        ("quota", call::Failure::Quota),
        ("invalid", call::Failure::InvalidOutput),
        ("limited", call::Failure::RateLimited),
    ] {
        chat(db, id, Audience::private(7)).await;
        let claimed = claim(db).await.unwrap().unwrap();
        finish(db, &claimed, Err(Failure::Model(failure)))
            .await
            .unwrap();
        assert!(loaded(db, id).await.data.is_none());
    }
    chat(db, "old_input", Audience::private(7)).await;
    let old = claim(db).await.unwrap().unwrap();
    chat(db, "new_input", Audience::private(7)).await;
    finish(db, &old, Ok(fact())).await.unwrap();
    assert_eq!(facts(db).await, 0);
    let fresh = claim(db).await.unwrap().unwrap();
    db.execute_unprepared("UPDATE users SET token_version=token_version+1 WHERE id=7")
        .await
        .unwrap();
    finish(db, &fresh, Ok(fact())).await.unwrap();
    assert_eq!(facts(db).await, 0);
    chat(db, "old_persona", Audience::private(7)).await;
    let old = claim(db).await.unwrap().unwrap();
    let tx = db.begin().await.unwrap();
    super::super::upsert_persona_on(
        &tx,
        "New".into(),
        "New personality".into(),
        super::super::PortraitUpdate::Keep,
        Default::default(),
        7,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    finish(db, &old, Ok(fact())).await.unwrap();
    assert_eq!(facts(db).await, 0);
    chat(db, "cleared", Audience::private(7)).await;
    let old = claim(db).await.unwrap().unwrap();
    let tx = db.begin().await.unwrap();
    super::super::clear_persona_on(&tx).await.unwrap();
    tx.commit().await.unwrap();
    finish(db, &old, Ok(fact())).await.unwrap();
    assert!(
        registry::get::<Job>(db, NAMESPACE, "cleared")
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(facts(db).await, 0);
    schema.drop().await;
}

async fn stranger(db: &DatabaseConnection, event: usize) {
    enqueue(
        db,
        "stranger",
        7,
        Payload::Stranger {
            venue: "telegram:2".into(),
            stranger: myriad_merope::strangers::Stranger {
                who: "telegram:3".into(),
                name: "访客".into(),
            },
            exchanges: vec![myriad_merope::strangers::Exchange {
                they: format!("{event} 我养猫"),
                you: "嗯".into(),
            }],
            count: 0,
        },
        Some(&event.to_string()),
    )
    .await
    .unwrap();
}
#[tokio::test]
#[ignore = "requires MYRIAD_MEROPE_TEST_DATABASE_URL; isolated migrated schema"]
async fn durable_memory_stranger_batches_deduplicate_and_keep_arrivals_during_a_lease() {
    let schema = fixture().await;
    let db = &schema.db;
    stranger(db, 0).await;
    assert!(claim(db).await.unwrap().is_none(), "wait for quiet period");
    for event in 1..8 {
        stranger(db, event).await;
    }
    for _ in 0..5 {
        stranger(db, 0).await;
    }
    let first = claim(db).await.unwrap().unwrap();
    let Some(Payload::Stranger {
        exchanges, count, ..
    }) = &first.job.data
    else {
        panic!()
    };
    assert_eq!(exchanges.len(), 8);
    assert_eq!(*count, 8);
    stranger(db, 8).await;
    finish(
        db,
        &first,
        Ok(Effect::Stranger {
            previous: None,
            note: Some("喜欢猫".into()),
        }),
    )
    .await
    .unwrap();
    assert_eq!(facts(db).await, 1);
    let Some(Payload::Stranger {
        exchanges, count, ..
    }) = loaded(db, "stranger").await.data
    else {
        panic!()
    };
    assert_eq!(exchanges.len(), 1);
    assert_eq!(count, 9);
    assert!(
        claim(db).await.unwrap().is_none(),
        "new partial batch waits for quiet"
    );
    // An acknowledged event cannot re-enter the next batch or inflate count.
    stranger(db, 0).await;
    due(db, "stranger").await;
    let second = claim(db).await.unwrap().unwrap();
    let Some(Payload::Stranger {
        exchanges, count, ..
    }) = &second.job.data
    else {
        panic!()
    };
    assert_eq!(exchanges.len(), 1);
    assert_eq!(*count, 9);
    finish(
        db,
        &first,
        Ok(Effect::Stranger {
            previous: None,
            note: Some("旧工作线程".into()),
        }),
    )
    .await
    .unwrap();
    finish(db, &second, Ok(Effect::NoChange)).await.unwrap();
    assert!(loaded(db, "stranger").await.data.is_none());
    // Last-attempt crash only retires the claimed prefix, never new arrivals.
    stranger(db, 9).await;
    due(db, "stranger").await;
    for _ in 0..3 {
        claim(db).await.unwrap().unwrap();
        due(db, "stranger").await;
    }
    stranger(db, 10).await;
    due(db, "stranger").await;
    assert!(claim(db).await.unwrap().is_none());
    let Some(Payload::Stranger { exchanges, .. }) = loaded(db, "stranger").await.data else {
        panic!()
    };
    assert_eq!(exchanges.len(), 1);
    assert!(exchanges[0].they.starts_with("10 "));
    due(db, "stranger").await;
    assert!(claim(db).await.unwrap().is_some());
    schema.drop().await;
}

#[test]
fn durable_memory_keys_do_not_alias_fields_or_audiences() {
    assert_ne!(
        memory_jobs::key(&["ab", "c"]),
        memory_jobs::key(&["a", "bc"])
    );
    assert_ne!(
        memory_jobs::key(&["chat", "1", "private"]),
        memory_jobs::key(&["chat", "1", "group:1"])
    );
}

#[tokio::test]
#[ignore = "requires MYRIAD_MEROPE_TEST_DATABASE_URL; isolated migrated schema"]
async fn durable_memory_resumes_in_a_fresh_process() {
    let schema = fixture().await;
    let db = &schema.db;
    chat(db, "restart", Audience::private(7)).await;
    let abandoned = claim(db).await.unwrap().unwrap();
    due(db, "restart").await;
    let schema_name: String = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT current_schema() AS name".to_string(),
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "name")
        .unwrap();
    let result = tokio::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "services::agent::merope::store::memory_jobs::tests::memory_process_fixture",
            "--ignored",
            "--nocapture",
        ])
        .env("MYRIAD_MEMORY_PROCESS_SCHEMA", schema_name)
        .output()
        .await
        .unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(loaded(db, "restart").await.data.is_none());
    finish(db, &abandoned, Ok(fact())).await.unwrap();
    assert_eq!(facts(db).await, 0);
    schema.drop().await;
}
#[tokio::test]
#[ignore = "subprocess fixture; parent supplies isolated schema"]
async fn memory_process_fixture() {
    let url = std::env::var("MYRIAD_MEROPE_TEST_DATABASE_URL").unwrap();
    let schema = std::env::var("MYRIAD_MEMORY_PROCESS_SCHEMA").unwrap();
    assert!(
        schema.starts_with("memory_jobs_")
            && schema
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
    );
    let mut options = sea_orm::ConnectOptions::new(url);
    options
        .set_schema_search_path(&schema)
        .max_connections(2)
        .sqlx_logging(false);
    let db = sea_orm::Database::connect(options).await.unwrap();
    let recovered = claim(&db).await.unwrap().unwrap();
    assert_eq!(recovered.id, "restart");
    assert_eq!(recovered.job.attempts, 2);
    finish(&db, &recovered, Ok(Effect::NoChange)).await.unwrap();
    db.close().await.unwrap();
}
