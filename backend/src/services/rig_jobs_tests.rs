//! Jobs as rows against a database: claiming under the limit, the caller's
//! key, a lapsed lease, cancel, steps, and what a job's calls cost.

use sea_orm::{ConnectionTrait, DatabaseConnection};
use serde_json::json;

use super::*;
use crate::services::ai_cost_ledger::{AiCostLedgerEntry, record_ai_cost};

async fn schema(prefix: &str) -> Option<crate::db::IsolatedSchema> {
    let url = crate::db::test_url(&["MYRIAD_MEDIA_TEST_DATABASE_URL"])?;
    let schema = crate::db::IsolatedSchema::migrated(&url, prefix).await;
    schema
        .db
        .execute_unprepared("INSERT INTO users (id, username) VALUES (7, 'owner'), (8, 'other')")
        .await
        .unwrap();
    Some(schema)
}

fn job(id: &str, subject: &str, key: Option<&str>) -> NewJob {
    let input = json!({ "sourceMasterAssetId": subject });
    NewJob {
        id: id.to_owned(),
        kind: "bust".into(),
        caller: user_caller(7),
        owner_user_id: Some(7),
        idempotency_key: key.map(str::to_owned),
        request_digest: digest(&input),
        subject: Some(subject.to_owned()),
        input,
        progress: json!({ "stage": "generating" }),
        ledger_task_id: ledger_task_id(id),
        runner: "proc-a".into(),
    }
}

async fn lapse(db: &DatabaseConnection, id: &str) {
    db.execute_unprepared(&format!(
        "UPDATE rig_jobs SET heartbeat_at = NOW() - INTERVAL '{} seconds' WHERE id = '{id}'",
        LEASE.as_secs() + 1
    ))
    .await
    .unwrap();
}

#[tokio::test]
async fn jobs_are_claimed_up_to_the_limit_and_the_same_subject_is_joined() {
    let Some(schema) = schema("rig_jobs_claim").await else {
        return;
    };
    let db = &schema.db;
    assert_eq!(
        claim(db, job("a", "m1", None), 1).await.unwrap(),
        Claimed::New("a".into())
    );
    let row = get(db, "a").await.unwrap().expect("claimed");
    assert_eq!(row.state(), State::Running);
    assert_eq!(row.attempts, 1);
    assert_eq!(row.runner.as_deref(), Some("proc-a"));
    assert_eq!(row.input["sourceMasterAssetId"], json!("m1"));
    assert_eq!(row.ledger_task_id, "rig-job:a");
    // A second tab asking for the same master joins the running job.
    assert_eq!(
        claim(db, job("b", "m1", None), 1).await.unwrap(),
        Claimed::Existing("a".into())
    );
    // Another master waits for a place.
    assert!(matches!(
        claim(db, job("c", "m2", None), 1).await,
        Err(ClaimError::Busy)
    ));
    // With two places it runs beside the first.
    assert_eq!(
        claim(db, job("c", "m2", None), 2).await.unwrap(),
        Claimed::New("c".into())
    );
    // An ended job holds no place and is not joined.
    finish(
        db,
        "a",
        Ending::Succeeded { result_ref: None },
        &json!({ "stage": "done" }),
    )
    .await
    .unwrap();
    assert_eq!(
        claim(db, job("d", "m1", None), 2).await.unwrap(),
        Claimed::New("d".into())
    );
    schema.drop().await;
}

#[tokio::test]
async fn the_same_key_gives_the_same_job_and_refuses_another_request() {
    let Some(schema) = schema("rig_jobs_key").await else {
        return;
    };
    let db = &schema.db;
    assert_eq!(
        claim(db, job("a", "m1", Some("k1")), 1).await.unwrap(),
        Claimed::New("a".into())
    );
    finish(
        db,
        "a",
        Ending::Failed {
            code: Some("see_through_timeout".into()),
            message: Some("timed out".into()),
            detail: json!({ "error": "timed out", "code": "see_through_timeout" }),
        },
        &json!({}),
    )
    .await
    .unwrap();
    // Sent again with its key, however the job ended: the same job.
    assert_eq!(
        claim(db, job("b", "m1", Some("k1")), 1).await.unwrap(),
        Claimed::Existing("a".into())
    );
    assert_eq!(
        by_key(db, &user_caller(7), "k1")
            .await
            .unwrap()
            .map(|(id, _)| id),
        Some("a".into())
    );
    // The key with something else asked is refused.
    assert!(matches!(
        claim(db, job("c", "m2", Some("k1")), 1).await,
        Err(ClaimError::KeyReused)
    ));
    // Another caller's key is its own.
    let other = NewJob {
        caller: user_caller(8),
        owner_user_id: Some(8),
        ..job("d", "m1", Some("k1"))
    };
    assert_eq!(claim(db, other, 1).await.unwrap(), Claimed::New("d".into()));
    schema.drop().await;
}

#[tokio::test]
async fn a_running_job_whose_lease_lapsed_is_failed_as_interrupted() {
    let Some(schema) = schema("rig_jobs_lease").await else {
        return;
    };
    let db = &schema.db;
    claim(db, job("a", "m1", None), 1).await.unwrap();
    let began = crate::clock::now();
    let step = StepRow {
        seq: 0,
        name: "head-drawings".into(),
        state: State::Running,
        attempts: 1,
        backend: "image_provider".into(),
        backend_target: Some("openai/m".into()),
        started_at: began,
        ended_at: None,
        wall_seconds: None,
        gpu_seconds: None,
    };
    save_steps(db, "a", std::slice::from_ref(&step))
        .await
        .unwrap();
    // Renewed: still running.
    let beat = heartbeat(db, "a", "proc-a", Some(&json!({ "stage": "decomposing" })))
        .await
        .unwrap();
    assert!(beat.running && !beat.cancel_requested);
    assert_eq!(
        get(db, "a").await.unwrap().unwrap().progress["stage"],
        json!("decomposing")
    );
    // Another process does not renew it.
    assert!(!heartbeat(db, "a", "proc-b", None).await.unwrap().running);
    // Its process went away: the lease lapses, and the next read finds it interrupted.
    lapse(db, "a").await;
    let row = get(db, "a").await.unwrap().unwrap();
    assert_eq!(row.state(), State::Failed);
    assert!(row.interrupted());
    assert!(row.ended_at.is_some() && row.runner.is_none());
    assert_eq!(steps(db, "a").await.unwrap()[0].state, State::Failed);
    // It holds no place, and its process, were it back, is told it is not its own.
    assert_eq!(
        claim(db, job("b", "m1", None), 1).await.unwrap(),
        Claimed::New("b".into())
    );
    assert!(!heartbeat(db, "a", "proc-a", None).await.unwrap().running);
    assert!(
        !finish(db, "a", Ending::Succeeded { result_ref: None }, &json!({}))
            .await
            .unwrap(),
        "an interrupted job stays as it ended"
    );
    schema.drop().await;
}

#[tokio::test]
async fn cancel_is_seen_at_the_next_renewal_and_ends_the_job_cancelled() {
    let Some(schema) = schema("rig_jobs_cancel").await else {
        return;
    };
    let db = &schema.db;
    claim(db, job("a", "m1", None), 1).await.unwrap();
    // Another caller cannot cancel it.
    assert!(
        request_cancel(db, &user_caller(8), "a")
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        !heartbeat(db, "a", "proc-a", None)
            .await
            .unwrap()
            .cancel_requested
    );
    let asked = request_cancel(db, &user_caller(7), "a")
        .await
        .unwrap()
        .unwrap();
    assert!(asked.cancel_requested_at.is_some());
    assert_eq!(asked.state(), State::Running, "the runner ends it");
    assert!(
        heartbeat(db, "a", "proc-a", None)
            .await
            .unwrap()
            .cancel_requested
    );
    assert!(
        finish(db, "a", Ending::Cancelled, &json!({}))
            .await
            .unwrap()
    );
    assert_eq!(
        get(db, "a").await.unwrap().unwrap().state(),
        State::Cancelled
    );
    // A job that ended is not cancelled after.
    claim(db, job("b", "m2", None), 1).await.unwrap();
    finish(db, "b", Ending::Succeeded { result_ref: None }, &json!({}))
        .await
        .unwrap();
    let ended = request_cancel(db, &user_caller(7), "b")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ended.state(), State::Succeeded);
    assert!(ended.cancel_requested_at.is_none());
    schema.drop().await;
}

#[tokio::test]
async fn steps_are_written_as_they_change_and_end_with_the_job() {
    let Some(schema) = schema("rig_jobs_steps").await else {
        return;
    };
    let db = &schema.db;
    claim(db, job("a", "m1", None), 1).await.unwrap();
    let began = crate::clock::now();
    let mut fit = StepRow {
        seq: 0,
        name: "head-fit".into(),
        state: State::Running,
        attempts: 1,
        backend: "hf_space".into(),
        backend_target: Some("owner/space".into()),
        started_at: began,
        ended_at: None,
        wall_seconds: None,
        gpu_seconds: None,
    };
    let judging = StepRow {
        seq: 1,
        name: "head-judging".into(),
        ..fit.clone()
    };
    save_steps(db, "a", &[fit.clone(), judging]).await.unwrap();
    fit.state = State::Succeeded;
    fit.attempts = 3;
    fit.ended_at = Some(crate::clock::now());
    fit.wall_seconds = Some(12.5);
    save_steps(db, "a", std::slice::from_ref(&fit))
        .await
        .unwrap();
    finish(db, "a", Ending::Cancelled, &json!({}))
        .await
        .unwrap();
    let written = steps(db, "a").await.unwrap();
    assert_eq!(written.len(), 2);
    assert_eq!(written[0].state, State::Succeeded);
    assert_eq!(written[0].attempts, 3);
    assert_eq!(written[0].wall_seconds, Some(12.5));
    assert_eq!(written[0].backend_target.as_deref(), Some("owner/space"));
    assert_eq!(written[0].gpu_seconds, None);
    // The step still running when the job was cancelled ends cancelled with it.
    assert_eq!(written[1].state, State::Cancelled);
    assert!(written[1].ended_at.is_some());
    schema.drop().await;
}

#[tokio::test]
async fn a_jobs_spend_is_the_ledger_rows_written_under_its_ledger_id() {
    let Some(schema) = schema("rig_jobs_spend").await else {
        return;
    };
    let db = &schema.db;
    // Spent under the job's id before the job was claimed is not the job's.
    db.execute_unprepared(
        "INSERT INTO ai_cost_ledger (occurred_at, subject_id, owner_id, task_id, source, \
         operation, provider, model, input_tokens, output_tokens, cost_micro_usd, status) \
         VALUES (NOW() - INTERVAL '1 hour', 7, 7, 'rig-job:a', 'merope', 'turn-reference', \
         'openai', 'm', 100, 100, 999, 'success')",
    )
    .await
    .unwrap();
    claim(db, job("a", "m1", None), 1).await.unwrap();
    let row = |task_id: &'static str, cost: Option<i64>| AiCostLedgerEntry {
        subject_id: 7,
        owner_id: 7,
        tapp_id: None,
        task_id,
        source: "merope",
        operation: "turn-reference",
        provider: "openai",
        model: "m",
        input_tokens: 10,
        output_tokens: 1000,
        tokens_estimated: cost.is_none(),
        cost_micro_usd: cost,
        status: "success",
        error_code: None,
    };
    record_ai_cost(db, row("rig-job:a", Some(40_000))).await;
    record_ai_cost(db, row("rig-job:a", Some(2_500))).await;
    record_ai_cost(db, row("rig-job:a", None)).await;
    record_ai_cost(db, row("rig-job:b", Some(70_000))).await;
    record_ai_cost(db, row("turn-reference", Some(70_000))).await;
    let job = get(db, "a").await.unwrap().unwrap();
    assert_eq!(
        spend(db, &job).await.unwrap(),
        Spend {
            calls: 3,
            input_tokens: 30,
            output_tokens: 3000,
            priced_micro_usd: 42_500,
            unpriced_calls: 1,
        }
    );
    schema.drop().await;
}
