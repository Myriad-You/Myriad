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
            status: "success",
            error_code: None,
        },
    )
    .await;
    let record = served::record(&db, &caller(), &id).await.unwrap();
    assert_eq!(record["state"], json!("succeeded"));
    assert_eq!(record["kind"], json!("plain"));
    assert_eq!(record["steps"][0]["attempts"], json!(2));
    assert_eq!(record["steps"][1]["backend"], json!("image_provider"));
    assert_eq!(record["spend"]["calls"], json!(1));
    assert_eq!(record["spend"]["unpricedCalls"], json!(1));
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
    super::super::end(&db, &id, rig_jobs::Ending::Cancelled).await;
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
    // Its status still reads as it ended.
    assert_eq!(
        pickup::status_for_its_page(&db, &caller(), &id)
            .await
            .unwrap()["stage"],
        json!("done")
    );
    schema.drop().await;
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
