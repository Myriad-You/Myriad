//! 页面接上任务：做完没取走的结果等页面来取、只交一次；失败只报一次；状态带出任务当初的选择。

use std::sync::Arc;

use axum::{extract::Query, http::Uri};
use serde_json::{Value, json};

use super::super::{
    Phase, Stage, Submission, TurnKeyformsRequest, claim, end,
    live::{
        self,
        tests::{caller, schema},
    },
    together::JOB_TESTS,
    update,
};
use super::{Asked, Beside, Part, fetched, status_for_its_page, to_pick_up};
use crate::services::rig_jobs::Ending;

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

/// A job of the bust's master claimed as `start` claims one, asked as `asked`.
async fn claimed(db: &sea_orm::DatabaseConnection, outfit: Option<&str>, asked: Asked) -> String {
    let submission = Submission {
        user_id: 7,
        idempotency_key: None,
        outfit_id: outfit.map(str::to_owned),
        source_master_asset_id: MASTER.into(),
        source_generation_fingerprint: None,
        asked,
    };
    let phase = if outfit.is_some() {
        Phase::Figure
    } else {
        Phase::Bust
    };
    let (id, new) = claim(
        db,
        &submission,
        phase,
        Stage::Generating,
        4,
        Default::default(),
    )
    .await
    .unwrap();
    assert!(new);
    id
}

/// A job's end as `finish` leaves it: its files with the job, its record ended.
async fn done(db: &sea_orm::DatabaseConnection, id: &str, psd: &[u8], keys: &[u8]) {
    let (psd, keys) = (psd.to_vec(), keys.to_vec());
    update(id, |job| {
        job.stage = Stage::Done;
        job.psd = Some(Arc::new(psd));
        job.keyforms = Some(Arc::new(keys));
    });
    end(db, id, Ending::Succeeded { result_ref: None }).await;
}

#[tokio::test]
async fn a_status_carries_what_its_job_was_asked() {
    let _slot = JOB_TESTS.lock().await;
    let Some(schema) = schema("pickup_asked").await else {
        return;
    };
    let db = &schema.db;
    let payload: TurnKeyformsRequest = serde_json::from_value(
        json!({ "sourceMasterAssetId": MASTER, "head": false, "body": true }),
    )
    .unwrap();
    let asked = Asked::started(true, &payload, beside("?arms=true"));
    let id = claimed(db, Some("pickup-asked"), asked).await;
    let asked = json!({
        "head": false, "body": true, "plain": false, "fromArchive": null,
        "expressions": null, "arms": true
    });
    // The same to a page opened now and to the one polling the job by its id.
    let waiting = to_pick_up(db, &caller())
        .await
        .unwrap()
        .expect("a running job");
    assert_eq!(waiting["asked"], asked);
    assert_eq!(waiting["outfitId"], json!("pickup-asked"));
    assert_eq!(waiting["phase"], json!("figure"));
    assert_eq!(
        status_for_its_page(db, &caller(), &id).await.unwrap()["asked"],
        asked
    );
    schema.drop().await;
}

#[tokio::test]
async fn a_result_no_page_fetched_waits_for_one_and_is_handed_out_once() {
    let _slot = JOB_TESTS.lock().await;
    let Some(schema) = schema("pickup_result").await else {
        return;
    };
    let db = &schema.db;
    let id = claimed(db, None, Asked::default()).await;
    assert_eq!(
        to_pick_up(db, &caller())
            .await
            .unwrap()
            .expect("a running job")["stage"],
        json!("generating")
    );
    // Nothing to fetch yet, and nothing counted as fetched.
    assert!(fetched(db, &caller(), &id, Part::Psd).await.is_err());
    done(db, &id, b"psd", b"null").await;
    // Done with no page there: it waits, however often a page asks what there is. The
    // page takes it by the portrait and set it shows.
    for _ in 0..2 {
        let waiting = to_pick_up(db, &caller())
            .await
            .unwrap()
            .expect("a result no page fetched");
        assert_eq!(waiting["jobId"], json!(id));
        assert_eq!(waiting["stage"], json!("done"));
        assert_eq!(waiting["sourceMasterAssetId"], json!(MASTER));
        assert_eq!(waiting["outfitId"], Value::Null);
    }
    // Its status read by id is not its result fetched.
    assert_eq!(
        status_for_its_page(db, &caller(), &id).await.unwrap()["stage"],
        json!("done")
    );
    assert!(to_pick_up(db, &caller()).await.unwrap().is_some());
    // One of its two files is not the result either: a page that got only one comes back for it.
    let psd = fetched(db, &caller(), &id, Part::Psd).await.unwrap();
    assert_eq!(psd.as_slice(), b"psd");
    assert!(to_pick_up(db, &caller()).await.unwrap().is_some());
    let keys = fetched(db, &caller(), &id, Part::Keys).await.unwrap();
    assert_eq!(keys.as_slice(), b"null");
    assert!(
        to_pick_up(db, &caller()).await.unwrap().is_none(),
        "fetched: not handed to a reloaded page again"
    );
    // The page that has its id still reads it; another caller does not.
    assert!(fetched(db, &caller(), &id, Part::Psd).await.is_ok());
    assert!(fetched(db, "user:8", &id, Part::Psd).await.is_err());
    assert_eq!(
        status_for_its_page(db, &caller(), &id).await.unwrap()["stage"],
        json!("done")
    );
    schema.drop().await;
}

#[tokio::test]
async fn a_failure_no_page_saw_is_told_once() {
    let _slot = JOB_TESTS.lock().await;
    let Some(schema) = schema("pickup_failed").await else {
        return;
    };
    let db = &schema.db;
    let id = claimed(db, Some("pickup-failed"), Asked::default()).await;
    let error = json!({ "error": "timed out", "code": "see_through_timeout" });
    end(
        db,
        &id,
        Ending::Failed {
            code: Some("see_through_timeout".into()),
            message: Some("timed out".into()),
            detail: error,
        },
    )
    .await;
    // Pages of any portrait ask what there is to pick up: that tells none of them.
    for _ in 0..2 {
        let waiting = to_pick_up(db, &caller())
            .await
            .unwrap()
            .expect("a failure no page was told");
        assert_eq!(waiting["stage"], json!("failed"));
        assert_eq!(waiting["error"]["code"], json!("see_through_timeout"));
        assert_eq!(waiting["sourceMasterAssetId"], json!(MASTER));
        assert_eq!(waiting["outfitId"], json!("pickup-failed"));
    }
    // Another job's id reads nothing and tells nothing.
    assert!(
        status_for_its_page(db, &caller(), "another-job")
            .await
            .is_err()
    );
    assert!(to_pick_up(db, &caller()).await.unwrap().is_some());
    // The page whose job it is reads it by id, and is the one told.
    let told = status_for_its_page(db, &caller(), &id).await.unwrap();
    assert_eq!(told["error"]["code"], json!("see_through_timeout"));
    assert!(
        to_pick_up(db, &caller()).await.unwrap().is_none(),
        "told: not shown to a reloaded page again"
    );
    assert_eq!(
        status_for_its_page(db, &caller(), &id).await.unwrap()["stage"],
        json!("failed")
    );
    schema.drop().await;
}

#[tokio::test]
async fn a_result_waiting_for_a_page_does_not_hold_the_slot() {
    let _slot = JOB_TESTS.lock().await;
    let Some(schema) = schema("pickup_slot").await else {
        return;
    };
    let db = &schema.db;
    let id = claimed(db, None, Asked::default()).await;
    done(db, &id, b"", b"").await;
    assert!(to_pick_up(db, &caller()).await.unwrap().is_some());
    let next = claimed(db, Some("pickup-next"), Asked::default()).await;
    assert_ne!(next, id);
    // A page opened now picks up the job claimed last.
    assert_eq!(
        to_pick_up(db, &caller())
            .await
            .unwrap()
            .expect("the new job")["jobId"],
        json!(next)
    );
    assert!(
        fetched(db, &caller(), &id, Part::Psd).await.is_err(),
        "the process let the earlier result go"
    );
    assert!(live::with_job(&id, |_| ()).is_none());
    schema.drop().await;
}
