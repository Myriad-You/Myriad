//! 页面接上任务：做完没取走的结果等页面来取、只交一次；失败只报一次；状态带出任务当初的选择。

use std::sync::Arc;

use axum::{extract::Query, http::Uri};
use serde_json::{Value, json};

use super::super::{JOB, Stage, TurnKeyformsRequest, claim_job, together::JOB_TESTS, update};
use super::{Asked, Beside, Part, fetched, status_for_its_page, to_pick_up};

const MASTER: &str = "/media/assets/m/portrait.png";

/// What a request says its page puts on the result, as the handlers read its query.
fn beside(query: &str) -> Beside {
    let uri: Uri = format!("/see-through/turn-keyforms{query}")
        .parse()
        .expect("a request's path");
    Query::<Beside>::try_from_uri(&uri)
        .expect("a readable query")
        .0
}

/// What a job started with `request` (its body, its query) says it was asked for.
fn started(standing: bool, request: Value, query: &str) -> Value {
    let payload: TurnKeyformsRequest = serde_json::from_value(request).expect("a start request");
    serde_json::to_value(Asked::started(standing, &payload, beside(query))).expect("asked as JSON")
}

#[test]
fn a_started_job_says_what_it_was_asked_for() {
    // A bust keyed, its page putting expressions on it and no gesture arms.
    assert_eq!(
        started(
            false,
            json!({ "sourceMasterAssetId": MASTER }),
            "?expressions=true&arms=false"
        ),
        json!({
            "head": true, "body": false, "plain": false, "fromArchive": null,
            "expressions": true, "arms": false
        })
    );
    // A bust split once: no keys, though the request's `head` is left at its default.
    assert_eq!(
        started(
            false,
            json!({ "sourceMasterAssetId": MASTER, "plain": true }),
            "?expressions=true"
        ),
        json!({
            "head": false, "body": false, "plain": true, "fromArchive": null,
            "expressions": true, "arms": null
        })
    );
    // A figure keyed by its head alone, by its body alone, and by both (the request's default).
    assert_eq!(
        started(
            true,
            json!({ "sourceMasterAssetId": MASTER, "head": true, "body": false }),
            "?arms=true"
        ),
        json!({
            "head": true, "body": false, "plain": false, "fromArchive": null,
            "expressions": null, "arms": true
        })
    );
    assert_eq!(
        started(
            true,
            json!({ "sourceMasterAssetId": MASTER, "head": false, "body": true }),
            ""
        ),
        json!({
            "head": false, "body": true, "plain": false, "fromArchive": null,
            "expressions": null, "arms": null
        })
    );
    assert_eq!(
        started(true, json!({ "sourceMasterAssetId": MASTER }), ""),
        json!({
            "head": true, "body": true, "plain": false, "fromArchive": null,
            "expressions": null, "arms": null
        })
    );
}

#[test]
fn a_kept_job_used_again_says_which_and_how() {
    let refit = Asked::kept("kept-1", true, beside("?expressions=false&arms=true"));
    assert_eq!(
        serde_json::to_value(refit).unwrap(),
        json!({
            "head": false, "body": false, "plain": false,
            "fromArchive": { "id": "kept-1", "refit": true },
            "expressions": false, "arms": true
        })
    );
    // Loaded as it was, by a page that says nothing of what it puts on it.
    assert_eq!(
        serde_json::to_value(Asked::kept("kept-1", false, beside(""))).unwrap(),
        json!({
            "head": false, "body": false, "plain": false,
            "fromArchive": { "id": "kept-1", "refit": false },
            "expressions": null, "arms": null
        })
    );
}

#[tokio::test]
async fn a_status_carries_what_its_job_was_asked() {
    let _slot = JOB_TESTS.lock().await;
    let outfit = Some("pickup-asked".to_string());
    let (id, _) = claim_job(&outfit, MASTER, Stage::Upscaling, 1).unwrap();
    let payload: TurnKeyformsRequest = serde_json::from_value(
        json!({ "sourceMasterAssetId": MASTER, "head": false, "body": true }),
    )
    .unwrap();
    update(&id, |job| {
        job.asked = Asked::started(true, &payload, beside("?arms=true"));
    });
    let asked = json!({
        "head": false, "body": true, "plain": false, "fromArchive": null,
        "expressions": null, "arms": true
    });
    // The same to a page opened now and to the one polling the job by its id.
    assert_eq!(to_pick_up().expect("a running job")["asked"], asked);
    assert_eq!(status_for_its_page(&id).unwrap()["asked"], asked);
    update(&id, |job| job.stage = Stage::Done);
}

#[tokio::test]
async fn a_result_no_page_fetched_waits_for_one_and_is_handed_out_once() {
    let _slot = JOB_TESTS.lock().await;
    let (id, _) = claim_job(&None, MASTER, Stage::Generating, 4).unwrap();
    assert_eq!(
        to_pick_up().expect("a running job")["stage"],
        json!("generating")
    );
    // Nothing to fetch yet, and nothing counted as fetched.
    assert!(fetched(&id, Part::Psd).is_err());
    update(&id, |job| {
        job.stage = Stage::Done;
        job.psd = Some(Arc::new(b"psd".to_vec()));
        job.keyforms = Some(Arc::new(b"null".to_vec()));
    });
    // Done with no page there: it waits, however often a page asks what there is. The
    // page takes it by the portrait and set it shows.
    for _ in 0..2 {
        let waiting = to_pick_up().expect("a result no page fetched");
        assert_eq!(waiting["jobId"], json!(id));
        assert_eq!(waiting["stage"], json!("done"));
        assert_eq!(waiting["sourceMasterAssetId"], json!(MASTER));
        assert_eq!(waiting["outfitId"], Value::Null);
    }
    // Its status read by id is not its result fetched.
    assert_eq!(status_for_its_page(&id).unwrap()["stage"], json!("done"));
    assert!(to_pick_up().is_some());
    // One of its two files is not the result either: a page that got only one comes back for it.
    assert_eq!(fetched(&id, Part::Psd).unwrap().as_slice(), b"psd");
    assert!(to_pick_up().is_some());
    assert_eq!(fetched(&id, Part::Keys).unwrap().as_slice(), b"null");
    assert!(
        to_pick_up().is_none(),
        "fetched: not handed to a reloaded page again"
    );
    // The page that has its id still reads it.
    assert!(fetched(&id, Part::Psd).is_ok());
    assert_eq!(status_for_its_page(&id).unwrap()["stage"], json!("done"));
}

#[tokio::test]
async fn a_failure_no_page_saw_is_told_once() {
    let _slot = JOB_TESTS.lock().await;
    let outfit = Some("pickup-failed".to_string());
    let (id, _) = claim_job(&outfit, MASTER, Stage::Decomposing, 3).unwrap();
    update(&id, |job| {
        job.stage = Stage::Failed;
        job.error = Some(json!({ "error": "timed out", "code": "see_through_timeout" }));
    });
    // Pages of any portrait ask what there is to pick up: that tells none of them.
    for _ in 0..2 {
        let waiting = to_pick_up().expect("a failure no page was told");
        assert_eq!(waiting["stage"], json!("failed"));
        assert_eq!(waiting["error"]["code"], json!("see_through_timeout"));
        assert_eq!(waiting["sourceMasterAssetId"], json!(MASTER));
        assert_eq!(waiting["outfitId"], json!("pickup-failed"));
    }
    // Another job's id reads nothing and tells nothing.
    assert!(status_for_its_page("another-job").is_err());
    assert!(to_pick_up().is_some());
    // The page whose job it is reads it by id, and is the one told.
    let told = status_for_its_page(&id).unwrap();
    assert_eq!(told["error"]["code"], json!("see_through_timeout"));
    assert!(
        to_pick_up().is_none(),
        "told: not shown to a reloaded page again"
    );
    assert_eq!(status_for_its_page(&id).unwrap()["stage"], json!("failed"));
}

#[tokio::test]
async fn a_result_waiting_for_a_page_does_not_hold_the_slot() {
    let _slot = JOB_TESTS.lock().await;
    let (id, _) = claim_job(&None, MASTER, Stage::Generating, 4).unwrap();
    update(&id, |job| {
        job.stage = Stage::Done;
        job.psd = Some(Arc::new(Vec::new()));
        job.keyforms = Some(Arc::new(Vec::new()));
    });
    assert!(to_pick_up().is_some());
    let outfit = Some("pickup-next".to_string());
    let (next, new) = claim_job(&outfit, MASTER, Stage::Upscaling, 1).unwrap();
    assert!(new && next != id);
    assert!(
        fetched(&id, Part::Psd).is_err(),
        "the slot is the new job's"
    );
    assert_eq!(
        JOB.lock().unwrap().as_ref().map(|job| job.id.clone()),
        Some(next.clone())
    );
    update(&next, |job| job.stage = Stage::Done);
}
