//! Jobs as rows against a database: claiming under the limit (also from two
//! connections at once), the caller's key, a lapsed lease, a process
//! stopping, an end after interruption, an account deleted, cancel, steps,
//! retention, and what a job's calls cost.

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

async fn ended(db: &DatabaseConnection, id: &str, ending: Ending) -> Ended {
    finish(db, id, ending, &json!({})).await.unwrap()
}

fn failure() -> Ending {
    Ending::Failed {
        code: Some("see_through_timeout".into()),
        message: Some("timed out".into()),
        detail: json!({ "error": "timed out", "code": "see_through_timeout" }),
    }
}

fn succeeded() -> Ending {
    Ending::Succeeded {
        result_ref: Some("turn-archive:a1".into()),
    }
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
    ended(db, "a", Ending::Succeeded { result_ref: None }).await;
    assert_eq!(
        claim(db, job("d", "m1", None), 2).await.unwrap(),
        Claimed::New("d".into())
    );
    schema.drop().await;
}

#[tokio::test]
async fn two_claims_at_once_take_one_place_between_them() {
    let Some(schema) = schema("rig_jobs_race").await else {
        return;
    };
    let db = &schema.db;
    // Two connections of the pool, two masters, one place.
    for round in 0..5 {
        let (a, b) = tokio::join!(
            claim(db, job(&format!("a{round}"), "m1", None), 1),
            claim(db, job(&format!("b{round}"), "m2", None), 1),
        );
        let won = [&a, &b]
            .iter()
            .filter(|claimed| matches!(claimed, Ok(Claimed::New(_))))
            .count();
        let busy = [&a, &b]
            .iter()
            .filter(|claimed| matches!(claimed, Err(ClaimError::Busy)))
            .count();
        assert_eq!((won, busy), (1, 1), "round {round}: {a:?} {b:?}");
        db.execute_unprepared("UPDATE rig_jobs SET state = 'succeeded', ended_at = NOW()")
            .await
            .unwrap();
    }
    schema.drop().await;
}

#[tokio::test]
async fn the_same_key_gives_the_same_job_until_it_fails_or_expires() {
    let Some(schema) = schema("rig_jobs_key").await else {
        return;
    };
    let db = &schema.db;
    assert_eq!(
        claim(db, job("a", "m1", Some("k1")), 1).await.unwrap(),
        Claimed::New("a".into())
    );
    // Sent again while it runs, and after it succeeded: the same job.
    assert_eq!(
        claim(db, job("b", "m1", Some("k1")), 1).await.unwrap(),
        Claimed::Existing("a".into())
    );
    ended(db, "a", succeeded()).await;
    assert_eq!(
        claim(db, job("b", "m1", Some("k1")), 1).await.unwrap(),
        Claimed::Existing("a".into())
    );
    let keyed = by_key(db, &user_caller(7), "k1").await.unwrap().unwrap();
    assert_eq!((keyed.job_id.as_str(), keyed.holds()), ("a", true));
    // The key with something else asked is refused.
    assert!(matches!(
        claim(db, job("c", "m2", Some("k1")), 1).await,
        Err(ClaimError::KeyReused)
    ));
    // Another caller's key is its own.
    let other = NewJob {
        caller: user_caller(8),
        owner_user_id: Some(8),
        ..job("d", "m3", Some("k1"))
    };
    assert_eq!(claim(db, other, 2).await.unwrap(), Claimed::New("d".into()));
    ended(db, "d", Ending::Cancelled).await;

    // A key whose job failed or was cancelled is let go: the same request asks anew.
    claim(db, job("e", "m4", Some("k2")), 1).await.unwrap();
    ended(db, "e", failure()).await;
    assert!(
        !by_key(db, &user_caller(7), "k2")
            .await
            .unwrap()
            .unwrap()
            .holds()
    );
    assert_eq!(
        claim(db, job("f", "m4", Some("k2")), 1).await.unwrap(),
        Claimed::New("f".into())
    );
    assert_eq!(
        by_key(db, &user_caller(7), "k2")
            .await
            .unwrap()
            .unwrap()
            .job_id,
        "f"
    );
    // Still refused for another request while its job failed.
    ended(db, "f", Ending::Cancelled).await;
    assert!(matches!(
        claim(db, job("g", "m5", Some("k2")), 1).await,
        Err(ClaimError::KeyReused)
    ));

    // A key expires after a day: then it asks anew.
    db.execute_unprepared(&format!(
        "UPDATE rig_job_keys SET created_at = NOW() - INTERVAL '{} seconds' \
         WHERE idempotency_key = 'k1' AND caller = 'user:7'",
        KEY_TTL.as_secs() + 1
    ))
    .await
    .unwrap();
    assert_eq!(by_key(db, &user_caller(7), "k1").await.unwrap(), None);
    assert_eq!(
        claim(db, job("h", "m1", Some("k1")), 1).await.unwrap(),
        Claimed::New("h".into())
    );
    schema.drop().await;
}

#[tokio::test]
async fn a_keyed_request_that_joins_a_running_job_keeps_its_key_on_it() {
    let Some(schema) = schema("rig_jobs_join_key").await else {
        return;
    };
    let db = &schema.db;
    claim(db, job("a", "m1", None), 1).await.unwrap();
    // Asked for the same master with a key (and its own choices): joined.
    let keyed = NewJob {
        input: json!({ "sourceMasterAssetId": "m1", "plain": true }),
        request_digest: digest(&json!({ "sourceMasterAssetId": "m1", "plain": true })),
        ..job("b", "m1", Some("k1"))
    };
    assert_eq!(
        claim(db, keyed.clone(), 1).await.unwrap(),
        Claimed::Existing("a".into())
    );
    // Sent again with that key, it is the joined job.
    let again = NewJob {
        id: "c".into(),
        ..keyed
    };
    assert_eq!(
        claim(db, again, 1).await.unwrap(),
        Claimed::Existing("a".into())
    );
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
    assert_eq!(
        beat,
        Beat::Renewed {
            cancel_requested: false
        }
    );
    assert_eq!(
        get(db, "a").await.unwrap().unwrap().progress["stage"],
        json!("decomposing")
    );
    // Another process does not renew it.
    assert_eq!(
        heartbeat(db, "a", "proc-b", None).await.unwrap(),
        Beat::Lost
    );
    // Its process went away: the lease lapses, and the sweep finds it interrupted.
    lapse(db, "a").await;
    assert_eq!(fail_interrupted(db).await.unwrap(), 1);
    let row = get(db, "a").await.unwrap().unwrap();
    assert_eq!(row.state(), State::Failed);
    assert!(row.interrupted());
    assert!(row.ended_at.is_some() && row.runner.is_none());
    assert_eq!(steps(db, "a").await.unwrap()[0].state, State::Failed);
    // It holds no place, and its process, were it back, is told the lease is lost.
    assert_eq!(
        claim(db, job("b", "m2", None), 1).await.unwrap(),
        Claimed::New("b".into())
    );
    assert_eq!(
        heartbeat(db, "a", "proc-a", None).await.unwrap(),
        Beat::Lost
    );
    // Failing or cancelling it after does not change how it ended.
    assert_eq!(ended(db, "a", failure()).await, Ended::Already);
    assert_eq!(ended(db, "a", Ending::Cancelled).await, Ended::Already);
    assert!(get(db, "a").await.unwrap().unwrap().interrupted());
    schema.drop().await;
}

#[tokio::test]
async fn a_job_that_finishes_after_it_was_taken_for_interrupted_is_recorded_as_succeeded() {
    let Some(schema) = schema("rig_jobs_reconcile").await else {
        return;
    };
    let db = &schema.db;
    claim(db, job("a", "m1", None), 1).await.unwrap();
    lapse(db, "a").await;
    fail_interrupted(db).await.unwrap();
    // Its process went on and kept the result it paid for.
    assert_eq!(ended(db, "a", succeeded()).await, Ended::Reconciled);
    let row = get(db, "a").await.unwrap().unwrap();
    assert_eq!(row.state(), State::Succeeded);
    assert_eq!(row.result_ref.as_deref(), Some("turn-archive:a1"));
    assert_eq!(row.error_code, None);
    // A success is written once.
    assert_eq!(ended(db, "a", succeeded()).await, Ended::Already);
    schema.drop().await;
}

#[tokio::test]
async fn a_stopping_process_lets_its_jobs_go_at_once() {
    let Some(schema) = schema("rig_jobs_release").await else {
        return;
    };
    let db = &schema.db;
    let mine = NewJob {
        runner: this_process().to_owned(),
        ..job("a", "m1", None)
    };
    claim(db, mine, 2).await.unwrap();
    claim(db, job("b", "m2", None), 2).await.unwrap();
    assert_eq!(release_this_process(db).await.unwrap(), 1);
    assert!(get(db, "a").await.unwrap().unwrap().interrupted());
    assert_eq!(get(db, "b").await.unwrap().unwrap().state(), State::Running);
    // Its place is free at once, not after the lease.
    assert_eq!(
        claim(db, job("c", "m3", None), 2).await.unwrap(),
        Claimed::New("c".into())
    );
    schema.drop().await;
}

#[tokio::test]
async fn a_job_whose_account_is_deleted_is_gone_to_its_runner() {
    let Some(schema) = schema("rig_jobs_account").await else {
        return;
    };
    let db = &schema.db;
    claim(db, job("a", "m1", Some("k1")), 1).await.unwrap();
    db.execute_unprepared("DELETE FROM users WHERE id = 7")
        .await
        .unwrap();
    assert!(get(db, "a").await.unwrap().is_none());
    assert_eq!(by_key(db, &user_caller(7), "k1").await.unwrap(), None);
    assert_eq!(
        heartbeat(db, "a", "proc-a", None).await.unwrap(),
        Beat::Gone
    );
    assert_eq!(ended(db, "a", succeeded()).await, Ended::Already);
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
    assert_eq!(
        heartbeat(db, "a", "proc-a", None).await.unwrap(),
        Beat::Renewed {
            cancel_requested: false
        }
    );
    let asked = request_cancel(db, &user_caller(7), "a")
        .await
        .unwrap()
        .unwrap();
    assert!(asked.cancel_requested_at.is_some());
    assert_eq!(asked.state(), State::Running, "the runner ends it");
    assert_eq!(
        heartbeat(db, "a", "proc-a", None).await.unwrap(),
        Beat::Renewed {
            cancel_requested: true
        }
    );
    assert_eq!(ended(db, "a", Ending::Cancelled).await, Ended::Written);
    assert_eq!(
        get(db, "a").await.unwrap().unwrap().state(),
        State::Cancelled
    );
    // A job that ended is not cancelled after.
    claim(db, job("b", "m2", None), 1).await.unwrap();
    ended(db, "b", Ending::Succeeded { result_ref: None }).await;
    let after = request_cancel(db, &user_caller(7), "b")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after.state(), State::Succeeded);
    assert!(after.cancel_requested_at.is_none());
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
        // Longer than its column: kept cut, not refused.
        backend_target: Some(format!("owner/{}", "s".repeat(400))),
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
    ended(db, "a", Ending::Cancelled).await;
    let written = steps(db, "a").await.unwrap();
    assert_eq!(written.len(), 2);
    assert_eq!(written[0].state, State::Succeeded);
    assert_eq!(written[0].attempts, 3);
    assert_eq!(written[0].wall_seconds, Some(12.5));
    assert_eq!(
        written[0].backend_target.as_deref().map(str::len),
        Some(255)
    );
    assert_eq!(written[0].gpu_seconds, None);
    // The step still running when the job was cancelled ends cancelled with it.
    assert_eq!(written[1].state, State::Cancelled);
    assert!(written[1].ended_at.is_some());
    schema.drop().await;
}

#[tokio::test]
async fn ended_jobs_past_their_retention_are_removed_at_a_claim() {
    let Some(schema) = schema("rig_jobs_retention").await else {
        return;
    };
    let db = &schema.db;
    claim(db, job("old", "m1", None), 1).await.unwrap();
    ended(db, "old", Ending::Succeeded { result_ref: None }).await;
    claim(db, job("recent", "m2", None), 1).await.unwrap();
    ended(db, "recent", Ending::Succeeded { result_ref: None }).await;
    db.execute_unprepared(&format!(
        "UPDATE rig_jobs SET ended_at = NOW() - INTERVAL '{} seconds' WHERE id = 'old'",
        RETENTION.as_secs() + 1
    ))
    .await
    .unwrap();
    claim(db, job("next", "m3", None), 1).await.unwrap();
    assert!(get(db, "old").await.unwrap().is_none());
    assert!(get(db, "recent").await.unwrap().is_some());
    schema.drop().await;
}

#[tokio::test]
async fn a_jobs_spend_is_its_completed_calls_with_its_failed_ones_apart() {
    let Some(schema) = schema("rig_jobs_spend").await else {
        return;
    };
    let db = &schema.db;
    // Spent under the job's id before the job was claimed is not the job's.
    db.execute_unprepared(
        "INSERT INTO ai_cost_ledger (occurred_at, subject_id, owner_id, task_id, source, \
         operation, provider, model, input_tokens, output_tokens, cost_micro_usd, status) \
         VALUES (NOW() - INTERVAL '1 hour', 7, 7, 'rig-job:a', 'merope', 'turn-reference', \
         'openai', 'm', 100, 100, 999, 'completed')",
    )
    .await
    .unwrap();
    claim(db, job("a", "m1", None), 1).await.unwrap();
    let row = |task_id: &'static str, cost: Option<i64>, status: &'static str| AiCostLedgerEntry {
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
        status,
        error_code: None,
    };
    record_ai_cost(db, row("rig-job:a", Some(40_000), "completed")).await;
    record_ai_cost(db, row("rig-job:a", Some(2_500), "completed")).await;
    record_ai_cost(db, row("rig-job:a", None, "completed")).await;
    record_ai_cost(db, row("rig-job:a", None, "failed")).await;
    record_ai_cost(db, row("rig-job:b", Some(70_000), "completed")).await;
    record_ai_cost(db, row("turn-reference", Some(70_000), "completed")).await;
    let job = get(db, "a").await.unwrap().unwrap();
    assert_eq!(
        spend(db, &job).await.unwrap(),
        Spend {
            completed: Calls {
                calls: 3,
                input_tokens: 30,
                output_tokens: 3000,
                priced_micro_usd: 42_500,
                unpriced_calls: 1,
            },
            failed: Calls {
                calls: 1,
                input_tokens: 10,
                output_tokens: 1000,
                priced_micro_usd: 0,
                unpriced_calls: 1,
            },
        }
    );
    schema.drop().await;
}
