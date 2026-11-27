//! A job run as the handlers run one, against a database: claimed, its
//! progress and steps written while it runs, ended in its record; cancelled
//! where it stands; asked for again by its key; gone with its process; its
//! drawings written to the ledger under its id.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use axum::http::{HeaderMap, HeaderValue, StatusCode};
use sea_orm::ConnectionTrait;
use serde_json::json;

use super::super::{
    Asked, Beside, Made, Phase, Stage, Submission, TurnKeyformsRequest, archive::ArchiveMeta,
    claim, figure::plain, pickup, served, timed, together::JOB_TESTS, update,
};
use super::*;
use crate::{db::IsolatedSchema, services::rig_jobs};

const MASTER: &str = "/media/assets/m/portrait.png";

pub(in crate::api::merope_rig::turn_keyforms) async fn schema(
    prefix: &str,
) -> Option<IsolatedSchema> {
    let url = crate::db::test_url(&["MYRIAD_MEDIA_TEST_DATABASE_URL"])?;
    let schema = IsolatedSchema::migrated(&url, prefix).await;
    schema
        .db
        .execute_unprepared(&format!(
            "INSERT INTO users (id, username, is_admin, is_owner) VALUES (7, 'owner', true, true); \
             INSERT INTO agent_persona (id, name, updated_at, portrait_asset_id) \
             VALUES ('site', 'Merope', NOW(), '{MASTER}')"
        ))
        .await
        .unwrap();
    Some(schema)
}

fn submission(key: Option<&str>, plain: bool) -> Submission {
    let payload: TurnKeyformsRequest =
        serde_json::from_value(json!({ "sourceMasterAssetId": MASTER, "plain": plain })).unwrap();
    Submission {
        user_id: 7,
        idempotency_key: key.map(str::to_owned),
        outfit_id: None,
        source_master_asset_id: MASTER.into(),
        source_generation_fingerprint: None,
        asked: Asked::started(false, &payload, Beside::default()),
    }
}

pub(in crate::api::merope_rig::turn_keyforms) fn caller() -> String {
    rig_jobs::user_caller(7)
}

/// The archive meta `start` makes for the bust's master as it stands.
async fn meta(db: &DatabaseConnection) -> ArchiveMeta {
    let master = super::super::super::master::current_master(db, super::super::slot_of(None))
        .await
        .unwrap()
        .expect("the bust's master");
    ArchiveMeta {
        id: super::super::archive::new_id(),
        created_at: crate::clock::now(),
        outfit_id: None,
        source_master_asset_id: master.asset_id,
        source_generation_fingerprint: master.generation_fingerprint,
        parent: None,
        master: String::new(),
        master_media_type: String::new(),
        generation_provider: "openai".into(),
        generation_model: "m".into(),
        prompts: Vec::new(),
        space: "owner/space".into(),
        redrawn: Vec::new(),
        bytes: 0,
        figure: None,
        body_motions: None,
    }
}

fn targets() -> Targets {
    Targets {
        space: Some("owner/space".into()),
        image: Some("openai/m".into()),
    }
}

/// Dropped with the job's work: whether the work was let go.
struct Dropped(Arc<AtomicBool>);

impl Drop for Dropped {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

#[tokio::test]
async fn a_jobs_progress_steps_and_end_are_written_to_its_record() {
    let _slot = JOB_TESTS.lock().await;
    let Some(schema) = schema("rig_live_lifecycle").await else {
        return;
    };
    let db = schema.db.clone();
    let (id, new) = claim(
        &db,
        &submission(None, true),
        Phase::Plain,
        Stage::Decomposing,
        1,
        targets(),
    )
    .await
    .unwrap();
    assert!(new);
    let work = {
        let id = id.clone();
        async move {
            // A step whose call is tried once more, then the drawings, counted as they come.
            timed(&id, "whole", async {
                note_retry();
                tokio::time::sleep(Duration::from_millis(1200)).await;
                ApiResult::Ok(())
            })
            .await?;
            update(&id, |job| job.done = 1);
            tokio::time::sleep(Duration::from_millis(2000)).await;
            super::super::took(&id, "head-drawings", Instant::now());
            ApiResult::<Made>::Ok(plain(b"psd".to_vec(), "image/png".into()))
        }
    };
    let running = tokio::spawn(run_job(db.clone(), id.clone(), work, meta(&db).await));
    // While it runs, its record has its progress as last written (once a second).
    tokio::time::sleep(Duration::from_millis(2400)).await;
    let row = rig_jobs::get(&db, &id).await.unwrap().unwrap();
    assert_eq!(row.state(), rig_jobs::State::Running);
    assert_eq!(row.progress["stage"], json!("decomposing"));
    assert_eq!(row.progress["done"], json!(1));
    assert_eq!(row.kind, "plain");
    assert_eq!(row.input["asked"]["plain"], json!(true));
    let status = pickup::status_for_its_page(&db, &caller(), &id)
        .await
        .unwrap();
    assert_eq!(status["stage"], json!("decomposing"));
    assert_eq!(status["phase"], json!("plain"));
    running.await.unwrap();
    let row = rig_jobs::get(&db, &id).await.unwrap().unwrap();
    assert_eq!(row.state(), rig_jobs::State::Succeeded);
    assert!(row.ended_at.is_some() && row.runner.is_none());
    assert_eq!(row.result_ref, None, "a plain split is not kept");
    let steps = rig_jobs::steps(&db, &id).await.unwrap();
    let names: Vec<_> = steps.iter().map(|step| step.name.as_str()).collect();
    assert_eq!(names, ["whole", "head-drawings"]);
    assert_eq!(steps[0].state, rig_jobs::State::Succeeded);
    assert_eq!(steps[0].attempts, 2);
    assert_eq!(steps[0].backend, "hf_space");
    assert_eq!(steps[0].backend_target.as_deref(), Some("owner/space"));
    assert!(steps[0].wall_seconds.unwrap() >= 1.2);
    assert_eq!(steps[1].backend, "image_provider");
    assert_eq!(steps[1].backend_target.as_deref(), Some("openai/m"));
    // The page fetches the result through the same record.
    assert_eq!(
        pickup::status_for_its_page(&db, &caller(), &id)
            .await
            .unwrap()["stage"],
        json!("done")
    );
    let psd = pickup::fetched(&db, &caller(), &id, pickup::Part::Psd)
        .await
        .unwrap();
    assert_eq!(psd.as_slice(), b"psd");
    // Its record, with its steps and what was spent under its ledger id.
    crate::services::ai_cost_ledger::record_ai_cost(
        &db,
        crate::services::ai_cost_ledger::AiCostLedgerEntry {
            subject_id: 7,
            owner_id: 7,
            tapp_id: None,
            task_id: &rig_jobs::ledger_task_id(&id),
            source: "merope",
            operation: "turn-reference",
            provider: "openai",
            model: "m",
            input_tokens: 10,
            output_tokens: 20,
            tokens_estimated: true,
            cost_micro_usd: None,
            status: "completed",
            error_code: None,
        },
    )
    .await;
    let record = served::record(&db, &caller(), &id).await.unwrap();
    assert_eq!(record["state"], json!("succeeded"));
    assert_eq!(record["kind"], json!("plain"));
    assert_eq!(record["steps"][0]["attempts"], json!(2));
    assert_eq!(record["steps"][1]["backend"], json!("image_provider"));
    assert_eq!(record["spend"]["completed"]["calls"], json!(1));
    assert_eq!(record["spend"]["completed"]["unpricedCalls"], json!(1));
    assert_eq!(record["spend"]["failed"]["calls"], json!(0));
    assert!(served::record(&db, "user:8", &id).await.is_err());
    schema.drop().await;
}

#[tokio::test]
async fn a_job_that_fails_keeps_its_error_as_the_page_reads_it() {
    let _slot = JOB_TESTS.lock().await;
    let Some(schema) = schema("rig_live_failed").await else {
        return;
    };
    let db = schema.db.clone();
    let (id, _) = claim(
        &db,
        &submission(None, false),
        Phase::Bust,
        Stage::Generating,
        4,
        targets(),
    )
    .await
    .unwrap();
    let work = async {
        ApiResult::<Made>::Err(super::super::see_through_error(
            crate::services::see_through::SeeThroughError::Timeout,
        ))
    };
    run_job(db.clone(), id.clone(), work, meta(&db).await).await;
    let row = rig_jobs::get(&db, &id).await.unwrap().unwrap();
    assert_eq!(row.state(), rig_jobs::State::Failed);
    let detail = row.error_detail.clone().unwrap();
    assert_eq!(row.error_code.as_deref(), detail["code"].as_str());
    assert_eq!(row.error_message.as_deref(), detail["error"].as_str());
    let status = pickup::status_for_its_page(&db, &caller(), &id)
        .await
        .unwrap();
    assert_eq!(status["stage"], json!("failed"));
    assert_eq!(status["error"], detail);
    schema.drop().await;
}

#[tokio::test]
async fn a_cancelled_job_stops_where_it_stands_and_ends_cancelled() {
    let _slot = JOB_TESTS.lock().await;
    let Some(schema) = schema("rig_live_cancel").await else {
        return;
    };
    let db = schema.db.clone();
    let (id, _) = claim(
        &db,
        &submission(None, false),
        Phase::Bust,
        Stage::Generating,
        4,
        targets(),
    )
    .await
    .unwrap();
    let dropped = Arc::new(AtomicBool::new(false));
    let work = {
        let id = id.clone();
        let guard = Dropped(dropped.clone());
        async move {
            let _guard = guard;
            // Waiting on a call that never answers.
            timed(
                &id,
                "head-drawings",
                std::future::pending::<ApiResult<()>>(),
            )
            .await?;
            ApiResult::<Made>::Ok(plain(Vec::new(), "image/png".into()))
        }
    };
    let running = tokio::spawn(run_job(db.clone(), id.clone(), work, meta(&db).await));
    tokio::time::sleep(Duration::from_millis(300)).await;
    let status = served::cancel(&db, &caller(), &id).await.unwrap();
    assert_eq!(status["jobId"], json!(id));
    tokio::time::timeout(Duration::from_secs(5), running)
        .await
        .expect("the job stops when cancelled")
        .unwrap();
    assert!(dropped.load(Ordering::Relaxed), "its work was let go");
    let row = rig_jobs::get(&db, &id).await.unwrap().unwrap();
    assert_eq!(row.state(), rig_jobs::State::Cancelled);
    assert_eq!(
        rig_jobs::steps(&db, &id).await.unwrap()[0].state,
        rig_jobs::State::Cancelled
    );
    // A page reads it as failed, with the reason; cancelling it again changes nothing.
    let status = pickup::status_for_its_page(&db, &caller(), &id)
        .await
        .unwrap();
    assert_eq!(status["stage"], json!("failed"));
    assert_eq!(status["error"]["code"], json!("turn_keyforms_cancelled"));
    let again = served::cancel(&db, &caller(), &id).await.unwrap();
    assert_eq!(again["error"]["code"], json!("turn_keyforms_cancelled"));
    // Another caller's job, or none, is not there to cancel.
    assert!(served::cancel(&db, "user:8", &id).await.is_err());
    assert!(served::cancel(&db, &caller(), "no-such-job").await.is_err());
    schema.drop().await;
}

#[tokio::test]
async fn a_cancel_asked_of_another_process_is_seen_at_its_next_write() {
    let _slot = JOB_TESTS.lock().await;
    let Some(schema) = schema("rig_live_cancel_elsewhere").await else {
        return;
    };
    let db = schema.db.clone();
    let (id, _) = claim(
        &db,
        &submission(None, false),
        Phase::Bust,
        Stage::Generating,
        4,
        targets(),
    )
    .await
    .unwrap();
    let work = {
        let id = id.clone();
        async move {
            loop {
                update(&id, |job| job.done = (job.done + 1) % 4);
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    };
    let running = tokio::spawn(run_job(db.clone(), id.clone(), work, meta(&db).await));
    tokio::time::sleep(Duration::from_millis(300)).await;
    // Asked in the record only, as a process that does not run the job asks.
    rig_jobs::request_cancel(&db, &caller(), &id).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), running)
        .await
        .expect("seen at the next write")
        .unwrap();
    assert_eq!(
        rig_jobs::get(&db, &id).await.unwrap().unwrap().state(),
        rig_jobs::State::Cancelled
    );
    schema.drop().await;
}

#[tokio::test]
async fn the_same_key_gives_the_same_job_and_another_request_under_it_is_refused() {
    let _slot = JOB_TESTS.lock().await;
    let Some(schema) = schema("rig_live_key").await else {
        return;
    };
    let db = schema.db.clone();
    let asked = submission(Some("k1"), false);
    assert_eq!(asked.given_before(&db, Phase::Bust).await.unwrap(), None);
    let (id, new) = claim(&db, &asked, Phase::Bust, Stage::Generating, 4, targets())
        .await
        .unwrap();
    assert!(new);
    super::super::end(&db, &id, rig_jobs::Ending::Succeeded { result_ref: None }).await;
    // Sent again after it ended: the same job, not a new one.
    assert_eq!(
        asked.given_before(&db, Phase::Bust).await.unwrap(),
        Some(id.clone())
    );
    let (again, new) = claim(&db, &asked, Phase::Bust, Stage::Generating, 4, targets())
        .await
        .unwrap();
    assert_eq!((again.as_str(), new), (id.as_str(), false));
    // The key with another request is refused, before and at the claim.
    let other = submission(Some("k1"), true);
    let refused = other.given_before(&db, Phase::Plain).await.unwrap_err();
    assert_eq!(refused.0, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(refused.1["code"], json!("idempotency_key_reused"));
    let refused = claim(&db, &other, Phase::Plain, Stage::Decomposing, 1, targets())
        .await
        .unwrap_err();
    assert_eq!(refused.1["code"], json!("idempotency_key_reused"));
    // A key whose job was cancelled asks anew: the same request is a new job.
    let cancelled = submission(Some("k2"), false);
    let (first, _) = claim(
        &db,
        &cancelled,
        Phase::Bust,
        Stage::Generating,
        4,
        targets(),
    )
    .await
    .unwrap();
    super::super::end(&db, &first, rig_jobs::Ending::Cancelled).await;
    assert_eq!(
        cancelled.given_before(&db, Phase::Bust).await.unwrap(),
        None
    );
    let (second, new) = claim(
        &db,
        &cancelled,
        Phase::Bust,
        Stage::Generating,
        4,
        targets(),
    )
    .await
    .unwrap();
    assert!(new && second != first);
    super::super::end(&db, &second, rig_jobs::Ending::Cancelled).await;
    // Without a key, a new submission is a new job.
    let (fresh, new) = claim(
        &db,
        &submission(None, false),
        Phase::Bust,
        Stage::Generating,
        4,
        targets(),
    )
    .await
    .unwrap();
    assert!(new && fresh != id);
    schema.drop().await;
}

#[test]
fn the_key_is_read_from_its_header() {
    let mut headers = HeaderMap::new();
    assert_eq!(super::super::idempotency_key(&headers).unwrap(), None);
    headers.insert("idempotency-key", HeaderValue::from_static(" k-1 "));
    assert_eq!(
        super::super::idempotency_key(&headers).unwrap().as_deref(),
        Some("k-1")
    );
    headers.insert("idempotency-key", HeaderValue::from_static(""));
    assert!(super::super::idempotency_key(&headers).is_err());
    let long = HeaderValue::from_str(&"k".repeat(256)).unwrap();
    headers.insert("idempotency-key", long);
    assert!(super::super::idempotency_key(&headers).is_err());
}

#[tokio::test]
async fn a_job_whose_process_went_away_reads_as_gone_and_holds_no_place() {
    let _slot = JOB_TESTS.lock().await;
    let Some(schema) = schema("rig_live_restart").await else {
        return;
    };
    let db = schema.db.clone();
    let (id, _) = claim(
        &db,
        &submission(None, false),
        Phase::Bust,
        Stage::Generating,
        4,
        targets(),
    )
    .await
    .unwrap();
    // The process restarts: what it had of the job is gone, and its lease lapses.
    JOBS.lock().unwrap().clear();
    db.execute_unprepared(&format!(
        "UPDATE rig_jobs SET heartbeat_at = NOW() - INTERVAL '1 hour' WHERE id = '{id}'"
    ))
    .await
    .unwrap();
    rig_jobs::fail_interrupted(&db).await.unwrap();
    // The page polling it reads it as no such job, as it did before jobs were
    // kept, and starts its own again; a page opened now has nothing to pick up.
    let polled = pickup::status_for_its_page(&db, &caller(), &id)
        .await
        .unwrap_err();
    assert_eq!(polled.0, StatusCode::NOT_FOUND);
    assert_eq!(polled.1["code"], json!("turn_keyforms_missing"));
    assert_eq!(pickup::to_pick_up(&db, &caller()).await.unwrap(), None);
    assert!(served::cancel(&db, &caller(), &id).await.is_err());
    // Its record says why it ended.
    let row = rig_jobs::get(&db, &id).await.unwrap().unwrap();
    assert_eq!(row.state(), rig_jobs::State::Failed);
    assert_eq!(row.error_code.as_deref(), Some(rig_jobs::INTERRUPTED));
    // Started again, it is a new job.
    let (again, new) = claim(
        &db,
        &submission(None, false),
        Phase::Bust,
        Stage::Generating,
        4,
        targets(),
    )
    .await
    .unwrap();
    assert!(new && again != id);
    schema.drop().await;
}

#[tokio::test]
async fn a_finished_job_survives_its_process_only_with_its_result_kept() {
    let _slot = JOB_TESTS.lock().await;
    let Some(schema) = schema("rig_live_restart_done").await else {
        return;
    };
    let db = schema.db.clone();
    let (id, _) = claim(
        &db,
        &submission(None, true),
        Phase::Plain,
        Stage::Decomposing,
        1,
        targets(),
    )
    .await
    .unwrap();
    update(&id, |job| {
        job.stage = Stage::Done;
        job.psd = Some(Arc::new(b"psd".to_vec()));
        job.keyforms = Some(Arc::new(b"null".to_vec()));
    });
    super::super::end(&db, &id, rig_jobs::Ending::Succeeded { result_ref: None }).await;
    assert!(pickup::to_pick_up(&db, &caller()).await.unwrap().is_some());
    // The process restarts: a result kept nowhere else is gone with it, and
    // is not handed to a page that cannot fetch it.
    JOBS.lock().unwrap().clear();
    assert_eq!(pickup::to_pick_up(&db, &caller()).await.unwrap(), None);
    let fetched = pickup::fetched(&db, &caller(), &id, pickup::Part::Psd)
        .await
        .unwrap_err();
    assert_eq!(fetched.0, StatusCode::NOT_FOUND);
    // The page asking by its id reads it as no such job, and starts its own again.
    let polled = pickup::status_for_its_page(&db, &caller(), &id)
        .await
        .unwrap_err();
    assert_eq!(polled.0, StatusCode::NOT_FOUND);
    assert_eq!(polled.1["code"], json!("turn_keyforms_missing"));
    // Its record still says how it ended.
    assert_eq!(
        rig_jobs::get(&db, &id).await.unwrap().unwrap().state(),
        rig_jobs::State::Succeeded
    );
    schema.drop().await;
}

/// The job's work for the tests of its keeping: counts on until `until`.
fn counting(id: &str, until: Duration) -> impl Future<Output = ApiResult<Made>> + use<> {
    let id = id.to_owned();
    async move {
        let began = Instant::now();
        while began.elapsed() < until {
            update(&id, |job| job.done = (job.done + 1) % 4);
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        ApiResult::<Made>::Ok(plain(b"psd".to_vec(), "image/png".into()))
    }
}

/// The job taken for interrupted by another process's sweep, as after its
/// database was out of reach for longer than its lease.
async fn interrupted_elsewhere(db: &DatabaseConnection, id: &str) {
    db.execute_unprepared(&format!(
        "UPDATE rig_jobs SET state = 'failed', error_code = '{}', runner = NULL, \
         ended_at = NOW() WHERE id = '{id}'",
        rig_jobs::INTERRUPTED
    ))
    .await
    .unwrap();
}

#[tokio::test]
async fn a_job_whose_lease_was_lost_takes_it_back_when_nothing_took_its_place() {
    let _slot = JOB_TESTS.lock().await;
    let Some(schema) = schema("rig_live_lost").await else {
        return;
    };
    let db = schema.db.clone();
    let (id, _) = claim(
        &db,
        &submission(None, true),
        Phase::Plain,
        Stage::Decomposing,
        1,
        targets(),
    )
    .await
    .unwrap();
    let running = tokio::spawn(run_job(
        db.clone(),
        id.clone(),
        counting(&id, Duration::from_millis(3000)),
        meta(&db).await,
    ));
    tokio::time::sleep(Duration::from_millis(1200)).await;
    // This process's own sweep leaves it be; another's took it for interrupted.
    rig_jobs::fail_interrupted(&db).await.unwrap();
    assert_eq!(
        rig_jobs::get(&db, &id).await.unwrap().unwrap().state(),
        rig_jobs::State::Running
    );
    interrupted_elsewhere(&db, &id).await;
    // Its keeper takes the lease back at its next write.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let row = rig_jobs::get(&db, &id).await.unwrap().unwrap();
    assert_eq!(row.state(), rig_jobs::State::Running);
    assert_eq!(row.runner.as_deref(), Some(rig_jobs::this_process()));
    tokio::time::timeout(Duration::from_secs(10), running)
        .await
        .expect("it ends")
        .unwrap();
    let row = rig_jobs::get(&db, &id).await.unwrap().unwrap();
    assert_eq!(row.state(), rig_jobs::State::Succeeded);
    assert_eq!(row.error_code, None);
    schema.drop().await;
}

#[tokio::test]
async fn a_job_whose_lease_was_lost_and_place_taken_stops() {
    let _slot = JOB_TESTS.lock().await;
    let Some(schema) = schema("rig_live_lost_taken").await else {
        return;
    };
    let db = schema.db.clone();
    let (id, _) = claim(
        &db,
        &submission(None, false),
        Phase::Bust,
        Stage::Generating,
        4,
        targets(),
    )
    .await
    .unwrap();
    let dropped = Arc::new(AtomicBool::new(false));
    let work = {
        let guard = Dropped(dropped.clone());
        let counting = counting(&id, Duration::from_secs(600));
        async move {
            let _guard = guard;
            counting.await
        }
    };
    let running = tokio::spawn(run_job(db.clone(), id.clone(), work, meta(&db).await));
    tokio::time::sleep(Duration::from_millis(300)).await;
    interrupted_elsewhere(&db, &id).await;
    // The page read it gone and started it again: that one holds the place.
    let again = rig_jobs::NewJob {
        id: "again".into(),
        kind: "bust".into(),
        caller: caller(),
        owner_user_id: Some(7),
        idempotency_key: None,
        request_digest: "d".into(),
        subject: Some(format!("|{MASTER}")),
        input: json!({}),
        progress: json!({}),
        ledger_task_id: rig_jobs::ledger_task_id("again"),
        runner: "proc-next".into(),
    };
    assert_eq!(
        rig_jobs::claim(&db, again, 1).await.unwrap(),
        rig_jobs::Claimed::New("again".into())
    );
    // The first stops rather than run beside it; the place stays the new one's.
    tokio::time::timeout(Duration::from_secs(5), running)
        .await
        .expect("it stops")
        .unwrap();
    assert!(dropped.load(Ordering::Relaxed), "its work was let go");
    assert!(
        rig_jobs::get(&db, &id)
            .await
            .unwrap()
            .unwrap()
            .interrupted()
    );
    assert_eq!(
        rig_jobs::get(&db, "again").await.unwrap().unwrap().state(),
        rig_jobs::State::Running
    );
    schema.drop().await;
}

#[tokio::test]
async fn a_job_whose_end_is_not_yet_written_is_not_done_to_a_page() {
    let _slot = JOB_TESTS.lock().await;
    let Some(schema) = schema("rig_live_end_pending").await else {
        return;
    };
    let db = schema.db.clone();
    // The first time the job's end is written, the database refuses it.
    db.execute_unprepared(
        "CREATE SEQUENCE end_tries; \
         CREATE FUNCTION refuse_first_end() RETURNS trigger AS $$ BEGIN \
           IF NEW.state <> 'running' AND nextval('end_tries') = 1 THEN \
             RAISE EXCEPTION 'end refused once'; \
           END IF; RETURN NEW; END $$ LANGUAGE plpgsql; \
         CREATE TRIGGER refuse_first_end BEFORE UPDATE ON rig_jobs \
           FOR EACH ROW EXECUTE FUNCTION refuse_first_end();",
    )
    .await
    .unwrap();
    let (id, _) = claim(
        &db,
        &submission(None, true),
        Phase::Plain,
        Stage::Decomposing,
        1,
        targets(),
    )
    .await
    .unwrap();
    let running = tokio::spawn(run_job(
        db.clone(),
        id.clone(),
        counting(&id, Duration::from_millis(1100)),
        meta(&db).await,
    ));
    // Its work is done and its end waits to be tried again: the page reads it
    // at its last stage, not done, and has nothing to fetch yet.
    tokio::time::sleep(Duration::from_millis(1800)).await;
    assert!(with_job(&id, |job| job.stage == Stage::Done).unwrap());
    let status = pickup::status_for_its_page(&db, &caller(), &id)
        .await
        .unwrap();
    assert_eq!(status["stage"], json!("decomposing"));
    let early = pickup::fetched(&db, &caller(), &id, pickup::Part::Psd)
        .await
        .unwrap_err();
    assert_eq!(early.1["code"], json!("turn_keyforms_not_ready"));
    running.await.unwrap();
    assert_eq!(
        pickup::status_for_its_page(&db, &caller(), &id)
            .await
            .unwrap()["stage"],
        json!("done")
    );
    schema.drop().await;
}

#[tokio::test]
async fn a_job_past_its_deadline_fails_as_timed_out() {
    let _slot = JOB_TESTS.lock().await;
    let Some(schema) = schema("rig_live_deadline").await else {
        return;
    };
    let db = schema.db.clone();
    let (id, _) = claim(
        &db,
        &submission(None, false),
        Phase::Bust,
        Stage::Generating,
        4,
        targets(),
    )
    .await
    .unwrap();
    run_job_within(
        db.clone(),
        id.clone(),
        std::future::pending::<ApiResult<Made>>(),
        meta(&db).await,
        Duration::from_millis(500),
    )
    .await;
    let row = rig_jobs::get(&db, &id).await.unwrap().unwrap();
    assert_eq!(row.state(), rig_jobs::State::Failed);
    assert_eq!(row.error_code.as_deref(), Some("turn_keyforms_timed_out"));
    // Its place is free.
    let (next, new) = claim(
        &db,
        &submission(None, true),
        Phase::Plain,
        Stage::Decomposing,
        1,
        targets(),
    )
    .await
    .unwrap();
    assert!(new && next != id);
    schema.drop().await;
}

#[tokio::test]
async fn a_page_reads_a_stopping_processs_job_running_until_the_process_lets_it_go() {
    let _slot = JOB_TESTS.lock().await;
    let Some(schema) = schema("rig_live_stopping").await else {
        return;
    };
    let db = schema.db.clone();
    let (id, _) = claim(
        &db,
        &submission(None, false),
        Phase::Bust,
        Stage::Generating,
        4,
        targets(),
    )
    .await
    .unwrap();
    // Told to stop, the process still answers: its job reads running, so a
    // page polling it does not start it again here.
    let stopping = std::sync::atomic::AtomicBool::new(true);
    let refused = rig_jobs::claim_unless(
        &db,
        rig_jobs::NewJob {
            id: "late".into(),
            kind: "plain".into(),
            caller: caller(),
            owner_user_id: Some(7),
            idempotency_key: None,
            request_digest: "d".into(),
            subject: None,
            input: json!({}),
            progress: json!({}),
            ledger_task_id: rig_jobs::ledger_task_id("late"),
            runner: rig_jobs::this_process().into(),
        },
        2,
        &stopping,
    )
    .await;
    assert!(matches!(refused, Err(rig_jobs::ClaimError::Stopping)));
    let status = pickup::status_for_its_page(&db, &caller(), &id)
        .await
        .unwrap();
    assert_eq!(status["stage"], json!("generating"));
    // Once it no longer answers, it lets its job go: the next process reads it
    // gone, and the page starts it again there, once.
    let released = std::sync::atomic::AtomicBool::new(false);
    rig_jobs::release_once(&db, &released).await;
    let polled = pickup::status_for_its_page(&db, &caller(), &id)
        .await
        .unwrap_err();
    assert_eq!(polled.0, StatusCode::NOT_FOUND);
    schema.drop().await;
}

#[tokio::test]
async fn a_step_that_cannot_be_written_does_not_hold_back_the_lease() {
    let _slot = JOB_TESTS.lock().await;
    let Some(schema) = schema("rig_live_step_fails").await else {
        return;
    };
    let db = schema.db.clone();
    let (id, _) = claim(
        &db,
        &submission(None, true),
        Phase::Plain,
        Stage::Decomposing,
        1,
        targets(),
    )
    .await
    .unwrap();
    // A step the database refuses (its name is longer than the column).
    update(&id, |job| {
        let seq = job.step_began(&"x".repeat(80), Instant::now());
        job.step_ended(seq, true);
    });
    let before = rig_jobs::get(&db, &id).await.unwrap().unwrap();
    let running = tokio::spawn(run_job(
        db.clone(),
        id.clone(),
        counting(&id, Duration::from_millis(2500)),
        meta(&db).await,
    ));
    tokio::time::sleep(Duration::from_millis(1800)).await;
    // Meanwhile its progress and lease were written all the same.
    let during = rig_jobs::get(&db, &id).await.unwrap().unwrap();
    assert_eq!(during.state(), rig_jobs::State::Running);
    assert_ne!(during.progress, before.progress);
    running.await.unwrap();
    let row = rig_jobs::get(&db, &id).await.unwrap().unwrap();
    assert_eq!(row.state(), rig_jobs::State::Succeeded);
    assert!(rig_jobs::steps(&db, &id).await.unwrap().is_empty());
    schema.drop().await;
}

#[tokio::test]
async fn a_jobs_end_refused_by_the_database_is_written_again() {
    let _slot = JOB_TESTS.lock().await;
    let Some(schema) = schema("rig_live_end_fails").await else {
        return;
    };
    let db = schema.db.clone();
    // The first time the job's end is written, the database refuses it.
    db.execute_unprepared(
        "CREATE SEQUENCE end_tries;          CREATE FUNCTION refuse_first_end() RETURNS trigger AS $$ BEGIN            IF NEW.state <> 'running' AND nextval('end_tries') = 1 THEN              RAISE EXCEPTION 'end refused once';            END IF; RETURN NEW; END $$ LANGUAGE plpgsql;          CREATE TRIGGER refuse_first_end BEFORE UPDATE ON rig_jobs            FOR EACH ROW EXECUTE FUNCTION refuse_first_end();",
    )
    .await
    .unwrap();
    let (id, _) = claim(
        &db,
        &submission(None, true),
        Phase::Plain,
        Stage::Decomposing,
        1,
        targets(),
    )
    .await
    .unwrap();
    super::super::end(&db, &id, rig_jobs::Ending::Succeeded { result_ref: None }).await;
    assert_eq!(
        rig_jobs::get(&db, &id).await.unwrap().unwrap().state(),
        rig_jobs::State::Succeeded
    );
    let tries = db
        .query_one_raw(sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT last_value FROM end_tries",
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<i64>("", "last_value")
        .unwrap();
    assert_eq!(tries, 2);
    schema.drop().await;
}

#[tokio::test]
async fn a_job_whose_account_is_deleted_while_it_runs_stops() {
    let _slot = JOB_TESTS.lock().await;
    let Some(schema) = schema("rig_live_account").await else {
        return;
    };
    let db = schema.db.clone();
    let (id, _) = claim(
        &db,
        &submission(None, false),
        Phase::Bust,
        Stage::Generating,
        4,
        targets(),
    )
    .await
    .unwrap();
    let dropped = Arc::new(AtomicBool::new(false));
    let work = {
        let guard = Dropped(dropped.clone());
        let counting = counting(&id, Duration::from_secs(600));
        async move {
            let _guard = guard;
            counting.await
        }
    };
    let running = tokio::spawn(run_job(db.clone(), id.clone(), work, meta(&db).await));
    tokio::time::sleep(Duration::from_millis(300)).await;
    db.execute_unprepared("DELETE FROM users WHERE id = 7")
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), running)
        .await
        .expect("it stops")
        .unwrap();
    assert!(dropped.load(Ordering::Relaxed), "its work was let go");
    assert!(rig_jobs::get(&db, &id).await.unwrap().is_none());
    schema.drop().await;
}

#[tokio::test]
async fn a_paid_call_once_sent_is_seen_to_its_end_when_the_job_stops_waiting() {
    let (sent, answered) = tokio::sync::oneshot::channel();
    let call = async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        // Written to the ledger under the job, as the drawing's hook does.
        let task = crate::services::ai_cost_ledger::current_ai_attribution().map(|a| a.task_id);
        let _ = sent.send(task);
    };
    let waiting = for_ledger("job-1", async {
        detached(drawing_attribution(7), call).await
    });
    // The job is cancelled while it waits: it stops waiting.
    assert!(
        tokio::time::timeout(Duration::from_millis(50), waiting)
            .await
            .is_err()
    );
    let task = tokio::time::timeout(Duration::from_secs(2), answered)
        .await
        .expect("the call went on")
        .unwrap();
    assert_eq!(task.as_deref(), Some("rig-job:job-1"));
}

#[tokio::test]
async fn a_jobs_drawings_are_written_to_the_ledger_under_its_id() {
    // Outside a job, a turned drawing is the site's, as before.
    assert_eq!(drawing_attribution(7).task_id, "turn-reference");
    let inside = for_ledger("job-1", async { drawing_attribution(7) }).await;
    assert_eq!(inside.task_id, rig_jobs::ledger_task_id("job-1"));
    assert_eq!(
        (
            inside.subject_id,
            inside.source.as_str(),
            inside.operation.as_str()
        ),
        (7, "merope", "turn-reference")
    );
    // Steps run side by side within the job see the same id.
    let (a, b) = for_ledger("job-1", async {
        tokio::join!(async { drawing_attribution(7).task_id }, async {
            tokio::task::yield_now().await;
            drawing_attribution(7).task_id
        })
    })
    .await;
    assert_eq!((a.as_str(), b.as_str()), ("rig-job:job-1", "rig-job:job-1"));
}

#[test]
fn one_job_runs_at_a_time_unless_configured() {
    assert_eq!(at_once_from(None), 1);
    assert_eq!(at_once_from(Some(" 3 ")), 3);
    assert_eq!(at_once_from(Some("0")), 1);
    assert_eq!(at_once_from(Some("many")), 1);
}

#[tokio::test]
async fn a_job_whose_work_panics_ends_failed_and_frees_its_place() {
    let _slot = JOB_TESTS.lock().await;
    let Some(schema) = schema("rig_live_panic").await else {
        return;
    };
    let db = schema.db.clone();
    let (id, _) = claim(
        &db,
        &submission(None, false),
        Phase::Bust,
        Stage::Generating,
        4,
        targets(),
    )
    .await
    .unwrap();
    fn a_bug() -> ApiResult<Made> {
        panic!("a bug in the pipeline")
    }
    let work = async {
        tokio::time::sleep(Duration::from_millis(100)).await;
        a_bug()
    };
    run_job(db.clone(), id.clone(), work, meta(&db).await).await;
    let row = rig_jobs::get(&db, &id).await.unwrap().unwrap();
    assert_eq!(row.state(), rig_jobs::State::Failed);
    assert_eq!(row.error_code.as_deref(), Some("turn_keyforms_crashed"));
    let (next, new) = claim(
        &db,
        &submission(None, true),
        Phase::Plain,
        Stage::Decomposing,
        1,
        targets(),
    )
    .await
    .unwrap();
    assert!(new && next != id);
    schema.drop().await;
}

#[tokio::test]
async fn a_job_let_go_before_its_end_was_written_is_written_failed() {
    let _slot = JOB_TESTS.lock().await;
    let Some(schema) = schema("rig_live_unwound").await else {
        return;
    };
    let db = schema.db.clone();
    let (id, _) = claim(
        &db,
        &submission(None, false),
        Phase::Bust,
        Stage::Generating,
        4,
        targets(),
    )
    .await
    .unwrap();
    // As when its run unwinds while it ends: the hold goes, the record ends failed.
    drop(Holding::new(db.clone(), id.clone()));
    tokio::time::sleep(Duration::from_millis(500)).await;
    let row = rig_jobs::get(&db, &id).await.unwrap().unwrap();
    assert_eq!(row.state(), rig_jobs::State::Failed);
    assert_eq!(row.error_code.as_deref(), Some("turn_keyforms_crashed"));
    // A hold let go when its job ended writes nothing more.
    let (other, _) = claim(
        &db,
        &submission(None, true),
        Phase::Plain,
        Stage::Decomposing,
        1,
        targets(),
    )
    .await
    .unwrap();
    Holding::new(db.clone(), other.clone()).end_reached();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        rig_jobs::get(&db, &other).await.unwrap().unwrap().state(),
        rig_jobs::State::Running
    );
    schema.drop().await;
}

#[tokio::test]
async fn a_job_whose_work_is_over_is_not_taken_back_by_a_stale_tick() {
    let _slot = JOB_TESTS.lock().await;
    let Some(schema) = schema("rig_live_stale_tick").await else {
        return;
    };
    let db = schema.db.clone();
    let (id, _) = claim(
        &db,
        &submission(None, false),
        Phase::Bust,
        Stage::Generating,
        4,
        targets(),
    )
    .await
    .unwrap();
    interrupted_elsewhere(&db, &id).await;
    // The tick read the job still working; its work ended before the lease was found lost.
    super::super::update_quietly(&id, |job| job.ending = true);
    let mut written = Written::default();
    let keep = renew(&db, &id, &mut written, 1, None, false).await;
    assert_eq!(keep, Keep::Go);
    assert!(
        rig_jobs::get(&db, &id)
            .await
            .unwrap()
            .unwrap()
            .interrupted()
    );
    schema.drop().await;
}

#[tokio::test]
async fn an_end_the_database_refuses_for_good_is_written_as_a_bare_failure() {
    let _slot = JOB_TESTS.lock().await;
    let Some(schema) = schema("rig_live_end_refused").await else {
        return;
    };
    let db = schema.db.clone();
    // Every end but a bare failure is refused, for what it says.
    db.execute_unprepared(
        "CREATE FUNCTION refuse_ends() RETURNS trigger AS $$ BEGIN \
           IF NEW.state <> 'running' \
             AND NEW.error_code IS DISTINCT FROM 'turn_keyforms_end_unwritable' THEN \
             RAISE EXCEPTION 'refused' USING ERRCODE = '22023'; \
           END IF; RETURN NEW; END $$ LANGUAGE plpgsql; \
         CREATE TRIGGER refuse_ends BEFORE UPDATE ON rig_jobs \
           FOR EACH ROW EXECUTE FUNCTION refuse_ends();",
    )
    .await
    .unwrap();
    let (id, _) = claim(
        &db,
        &submission(None, true),
        Phase::Plain,
        Stage::Decomposing,
        1,
        targets(),
    )
    .await
    .unwrap();
    tokio::time::timeout(
        Duration::from_secs(20),
        super::super::end(&db, &id, rig_jobs::Ending::Succeeded { result_ref: None }),
    )
    .await
    .expect("it gives up on the refused end");
    let row = rig_jobs::get(&db, &id).await.unwrap().unwrap();
    assert_eq!(row.state(), rig_jobs::State::Failed);
    assert_eq!(
        row.error_code.as_deref(),
        Some("turn_keyforms_end_unwritable")
    );
    schema.drop().await;
}

#[tokio::test]
async fn a_start_under_a_key_let_go_is_not_refused_for_another_request() {
    let _slot = JOB_TESTS.lock().await;
    let Some(schema) = schema("rig_live_start_key").await else {
        return;
    };
    let db = schema.db.clone();
    let start = |key: &'static str, plain: bool| {
        let db = db.clone();
        async move {
            let mut headers = HeaderMap::new();
            headers.insert("idempotency-key", HeaderValue::from_static(key));
            let payload: TurnKeyformsRequest =
                serde_json::from_value(json!({ "sourceMasterAssetId": MASTER, "plain": plain }))
                    .unwrap();
            super::super::start_as(db, 7, None, payload, Beside::default(), &headers).await
        }
    };
    // A key whose job runs gives it to the same request and refuses another.
    let (running, _) = claim(
        &db,
        &submission(Some("held"), false),
        Phase::Bust,
        Stage::Generating,
        4,
        targets(),
    )
    .await
    .unwrap();
    let again = start("held", false).await.unwrap();
    assert_eq!(again.0["jobId"], json!(running));
    let refused = start("held", true).await.unwrap_err();
    assert_eq!(refused.0, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(refused.1["code"], json!("idempotency_key_reused"));
    // Its job cancelled, the key is let go: another request under it goes on
    // past the key (here as far as the image provider, which a test leaves unset).
    super::super::end(&db, &running, rig_jobs::Ending::Cancelled).await;
    let went_on = start("held", true).await.unwrap_err();
    assert_ne!(went_on.1["code"], json!("idempotency_key_reused"));
    schema.drop().await;
}
