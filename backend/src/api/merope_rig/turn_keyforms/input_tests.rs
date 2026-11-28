//! A caller's own picture checked as an untrusted upload and kept by its
//! content; a job of it run with no persona at all, from what its record
//! keeps, into its owner's storage and nowhere else; and the site's persona
//! flow recording its jobs as it did, so that what an older process claimed
//! is still read, joined and asked for again by its key.

use std::{io::Cursor, time::Duration};

use axum::body::Bytes;
use sea_orm::{ConnectionTrait, DatabaseConnection};
use serde_json::json;

use super::super::{
    Asked, Beside, Claim, Holds, Made, Phase, Stage, Submission, TurnCache, TurnKeyformsRequest,
    archive, claim, claim_for, live, pickup, start_input, together::JOB_TESTS,
};
use super::*;
use crate::{
    db::IsolatedSchema,
    services::{
        agent::merope,
        rig_jobs::{self, Owner},
    },
};

fn png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::DynamicImage::ImageLuma8(image::GrayImage::new(width, height))
        .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
        .unwrap();
    bytes
}

/// A picture with something drawn on it, so that its content is its own.
fn portrait() -> Vec<u8> {
    let mut picture = image::RgbaImage::new(48, 64);
    for (x, y, pixel) in picture.enumerate_pixels_mut() {
        *pixel = image::Rgba([(x * 5) as u8, (y * 3) as u8, 90, 255]);
    }
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgba8(picture)
        .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
        .unwrap();
    bytes
}

/// An owner of a test's own, with a root nobody else writes to.
fn isolated(test: &str) -> Owner {
    Owner::Isolated(format!("{test}-{}", uuid::Uuid::new_v4().simple()))
}

fn generation() -> image_generation::ImageGenerationConfig {
    image_generation::ImageGenerationConfig {
        provider: "openai".into(),
        model: "m".into(),
        api_key: String::new(),
        base_url: String::new(),
    }
}

/// The site's providers as a test has them: nothing asked of the Space
/// unless what it would make is not cached.
fn providers() -> Providers {
    Providers {
        generation: generation(),
        client: see_through::SeeThroughClient::offline(),
        space_name: "owner/space".into(),
    }
}

/// A migrated schema with site accounts and no persona row at all.
async fn without_persona(prefix: &str) -> Option<IsolatedSchema> {
    let url = crate::db::test_url(&["MYRIAD_MEDIA_TEST_DATABASE_URL"])?;
    let schema = IsolatedSchema::migrated(&url, prefix).await;
    schema
        .db
        .execute_unprepared(
            "DELETE FROM agent_persona; \
             INSERT INTO users (id, username, is_admin, is_owner) VALUES (7, 'owner', true, true)",
        )
        .await
        .unwrap();
    assert!(merope::get_persona(&schema.db).await.unwrap().is_none());
    Some(schema)
}

async fn ended(db: &DatabaseConnection, id: &str) -> rig_jobs::JobRow {
    for _ in 0..200 {
        let row = rig_jobs::get(db, id).await.unwrap().expect("the job");
        if row.state().ended() {
            return row;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the job did not end");
}

#[tokio::test]
async fn a_supplied_picture_is_taken_only_as_an_upload_may_be() {
    let owner = isolated("supplied");
    let storage = owner.storage();
    let refused = |bytes: Vec<u8>| {
        let storage = storage.clone();
        async move {
            supply_picture(&storage, Bytes::from(bytes))
                .await
                .expect_err("refused")
                .0
        }
    };
    // Too large, not a picture, a type the pipeline does not take.
    assert_eq!(
        refused(vec![0; MAX_PORTRAIT_BYTES + 1]).await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        refused(b"not a picture".to_vec()).await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        refused(b"GIF89a\x01\x00\x01\x00".to_vec()).await,
        StatusCode::BAD_REQUEST
    );
    // Past the media library's edge, or its pixels within the edge.
    assert_eq!(refused(png(8193, 1)).await, StatusCode::BAD_REQUEST);
    assert_eq!(refused(png(4100, 4100)).await, StatusCode::BAD_REQUEST);
    // Its header reads, its body is cut short.
    let whole = portrait();
    assert_eq!(
        refused(whole[..whole.len() - 40].to_vec()).await,
        StatusCode::BAD_REQUEST
    );
    assert!(!supplied_root(&storage).exists(), "nothing refused is kept");

    let picture = supply_picture(&storage, Bytes::from(whole.clone()))
        .await
        .unwrap();
    let PictureRef::Supplied { sha256, media_type } = &picture else {
        panic!("a supplied picture");
    };
    assert_eq!(
        (sha256.as_str(), media_type.as_str()),
        (sha256_hex(&whole).as_str(), "image/png")
    );
    let loaded = load_picture(&storage, &picture).await.unwrap();
    assert_eq!(loaded.bytes.as_ref(), whole.as_slice());
    // Another owner does not have it, and a name that walks paths names nothing.
    let other = isolated("supplied-other").storage();
    assert!(load_picture(&other, &picture).await.is_err());
    let walking = PictureRef::Supplied {
        sha256: "../../etc/passwd".into(),
        media_type: "image/png".into(),
    };
    assert!(load_picture(&storage, &walking).await.is_err());
    // A kept file that is no longer what its name says is not used.
    std::fs::write(
        supplied_path(&storage, sha256, media_type).unwrap(),
        png(4, 4),
    )
    .unwrap();
    assert!(load_picture(&storage, &picture).await.is_err());
    let _ = std::fs::remove_dir_all(storage.root());
}

#[tokio::test]
async fn a_job_with_no_persona_runs_from_its_own_input_into_its_owners_storage() {
    let _slot = JOB_TESTS.lock().await;
    let Some(schema) = without_persona("rig_input_plain").await else {
        return;
    };
    let db = schema.db.clone();
    let owner = isolated("plain");
    let storage = owner.storage();
    let caller = owner.scope();
    let whole = portrait();
    let picture = supply_picture(&storage, Bytes::from(whole.clone()))
        .await
        .unwrap();
    // What the Space would make of the picture is cached in the owner's
    // storage, as a job of it before would have left it: nothing is asked of
    // the Space.
    let cache = TurnCache::for_master(
        &storage,
        &whole,
        &generation(),
        "owner/space",
        see_through::DecomposeOptions::default(),
    );
    cache.write("decomposition-0.psd", b"8BPS front").await;
    cache.write("decomposition-0.checked", b"checked").await;

    let (id, new) = start_input(
        db.clone(),
        owner.clone(),
        caller.clone(),
        Some("plain-1".into()),
        picture.clone(),
        Make::Plain,
        providers(),
    )
    .await
    .unwrap();
    assert!(new);
    let row = ended(&db, &id).await;
    assert_eq!(
        row.state(),
        rig_jobs::State::Succeeded,
        "{:?}",
        row.error_detail
    );
    // Its record says whose it is and what its steps ran from.
    assert_eq!(row.owner(), Some(owner.clone()));
    assert_eq!(
        (row.caller.as_str(), row.owner_user_id),
        (caller.as_str(), None)
    );
    let input = JobInput::Picture {
        picture: picture.clone(),
        make: Make::Plain,
    };
    assert_eq!(
        row.pipeline_input,
        Some(json!({
            "from": "picture",
            "picture": {
                "kind": "supplied",
                "sha256": sha256_hex(&whole),
                "mediaType": "image/png"
            },
            "make": { "shape": "plain" }
        }))
    );
    let stored: JobInput = serde_json::from_value(row.pipeline_input.clone().unwrap()).unwrap();
    assert_eq!(stored, input);
    // Its owner fetches what it made; the same key gives the same job.
    let psd = pickup::fetched(&db, &caller, &id, pickup::Part::Psd)
        .await
        .unwrap();
    assert_eq!(psd.as_slice(), b"8BPS front");
    let (again, new) = start_input(
        db.clone(),
        owner.clone(),
        caller.clone(),
        Some("plain-1".into()),
        picture.clone(),
        Make::Plain,
        providers(),
    )
    .await
    .unwrap();
    assert_eq!((again.as_str(), new), (id.as_str(), false));
    // Nothing of it is in the site's storage, and the site's owner has no such job.
    let site_cache = TurnCache::for_master(
        &rig_jobs::Storage::site(),
        &whole,
        &generation(),
        "owner/space",
        see_through::DecomposeOptions::default(),
    );
    assert!(site_cache.read("decomposition-0.psd").await.is_none());
    let site_caller = rig_jobs::user_caller(7);
    assert!(
        pickup::status_for_its_page(&db, &site_caller, &id)
            .await
            .is_err()
    );
    assert!(merope::get_persona(&db).await.unwrap().is_none());
    let _ = std::fs::remove_dir_all(storage.root());
    schema.drop().await;
}

/// A finished bust's files, as a keyed job hands them to its end.
fn kept_bust() -> Made {
    Made {
        files: ArchiveFiles {
            master: portrait(),
            turned: Default::default(),
            decompositions: Default::default(),
            keyforms: br#"{"keys":1}"#.to_vec(),
            baked: b"8BPS baked".to_vec(),
            figure: None,
        },
        master_media_type: "image/png".into(),
        redrawn: Vec::new(),
        figure: None,
        keep: true,
    }
}

fn meta_of(picture: &PictureRef) -> ArchiveMeta {
    ArchiveMeta {
        id: archive::new_id(),
        created_at: crate::clock::now(),
        outfit_id: None,
        source_master_asset_id: picture.name(),
        source_generation_fingerprint: None,
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

/// A bust of `picture` claimed for `owner`, its end held to `holds`.
async fn claimed_bust(
    db: &DatabaseConnection,
    owner: &Owner,
    picture: &PictureRef,
    holds: Holds,
) -> String {
    let input = JobInput::Picture {
        picture: picture.clone(),
        make: Make::Bust,
    };
    let claim = Claim {
        owner: owner.clone(),
        caller: owner.scope(),
        idempotency_key: None,
        request: input.to_value(),
        subject: picture.name(),
        input,
        holds,
    };
    let (id, new) = claim_for(
        db,
        claim,
        Phase::Bust,
        Stage::Generating,
        4,
        Default::default(),
    )
    .await
    .unwrap();
    assert!(new);
    id
}

#[tokio::test]
async fn a_finished_job_is_kept_in_its_owners_archive_and_fetched_from_it_after_a_restart() {
    let _slot = JOB_TESTS.lock().await;
    let Some(schema) = without_persona("rig_input_kept").await else {
        return;
    };
    let db = schema.db.clone();
    let owner = isolated("kept");
    let storage = owner.storage();
    let picture = supply_picture(&storage, Bytes::from(portrait()))
        .await
        .unwrap();
    let id = claimed_bust(&db, &owner, &picture, Holds::AsMade).await;
    let work = async { ApiResult::Ok(kept_bust()) };
    live::run_job(db.clone(), id.clone(), work, meta_of(&picture)).await;
    let row = rig_jobs::get(&db, &id).await.unwrap().unwrap();
    assert_eq!(
        row.state(),
        rig_jobs::State::Succeeded,
        "{:?}",
        row.error_detail
    );
    let archive_id = row
        .result_ref
        .as_deref()
        .and_then(archive::of_result_ref)
        .expect("kept")
        .to_owned();
    assert!(archive::archive_root(&storage).join(&archive_id).is_dir());
    assert!(!archive::site_root().join(&archive_id).exists());
    // The process restarts: its result comes from the owner's archive.
    live::JOBS.lock().unwrap().clear();
    let keys = pickup::fetched(&db, &owner.scope(), &id, pickup::Part::Keys)
        .await
        .unwrap();
    assert_eq!(keys.as_slice(), br#"{"keys":1}"#);

    // The same job held to the site persona's master, with no persona: failed, nothing kept.
    let held = claimed_bust(&db, &owner, &picture, Holds::PersonaMaster).await;
    let work = async { ApiResult::Ok(kept_bust()) };
    live::run_job(db.clone(), held.clone(), work, meta_of(&picture)).await;
    let row = rig_jobs::get(&db, &held).await.unwrap().unwrap();
    assert_eq!(row.state(), rig_jobs::State::Failed);
    assert_eq!(row.result_ref, None);
    let _ = std::fs::remove_dir_all(storage.root());
    schema.drop().await;
}

fn site_submission(key: Option<&str>) -> Submission {
    let payload: TurnKeyformsRequest =
        serde_json::from_value(json!({ "sourceMasterAssetId": MASTER, "plain": true })).unwrap();
    Submission {
        user_id: 7,
        idempotency_key: key.map(str::to_owned),
        outfit_id: None,
        source_master_asset_id: MASTER.into(),
        source_generation_fingerprint: None,
        asked: Asked::started(false, &payload, Beside::default()),
    }
}

const MASTER: &str = "/media/assets/m/portrait.png";

/// What the site's persona page asked for a plain bust, as its record has
/// always kept it.
fn site_request() -> Value {
    json!({
        "kind": "plain",
        "outfitId": null,
        "sourceMasterAssetId": MASTER,
        "sourceGenerationFingerprint": null,
        "asked": {
            "head": false, "body": false, "plain": true, "fromArchive": null,
            "expressions": null, "arms": null
        }
    })
}

#[tokio::test]
async fn the_site_records_its_jobs_as_it_did_with_its_owner_and_input_beside() {
    let _slot = JOB_TESTS.lock().await;
    let Some(schema) = super::super::live::tests::schema("rig_input_site").await else {
        return;
    };
    let db = schema.db.clone();
    let (id, new) = claim(
        &db,
        &site_submission(Some("site-1")),
        Phase::Plain,
        Stage::Decomposing,
        1,
        Default::default(),
    )
    .await
    .unwrap();
    assert!(new);
    let row = rig_jobs::get(&db, &id).await.unwrap().unwrap();
    assert_eq!(
        (
            row.caller.as_str(),
            row.owner_user_id,
            row.owner_scope.as_str()
        ),
        ("user:7", Some(7), "site")
    );
    assert_eq!(row.owner(), Some(Owner::Site { user_id: 7 }));
    assert_eq!(row.owner().unwrap().storage(), rig_jobs::Storage::site());
    // What was asked is kept as it always was, and its key's digest is of it.
    assert_eq!(row.input, site_request());
    let keyed = rig_jobs::by_key(&db, "user:7", "site-1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(keyed.request_digest, rig_jobs::digest(&site_request()));
    // Beside it, what its steps run from: the master in the site's media.
    assert_eq!(
        row.pipeline_input,
        Some(json!({
            "from": "picture",
            "picture": { "kind": "siteMedia", "url": MASTER },
            "make": { "shape": "plain" }
        }))
    );
    super::super::update(&id, |job| job.stage = Stage::Done);
    super::super::end(&db, &id, rig_jobs::Ending::Cancelled).await;
    schema.drop().await;
}

#[tokio::test]
async fn a_job_an_older_process_claimed_is_read_joined_and_given_by_its_key() {
    let _slot = JOB_TESTS.lock().await;
    let Some(schema) = super::super::live::tests::schema("rig_input_older").await else {
        return;
    };
    let db = schema.db.clone();
    // Claimed and running on a process that knew neither the owner nor the
    // input: its row and key as that process wrote them.
    db.execute_unprepared(&format!(
        "INSERT INTO rig_jobs (id, kind, caller, owner_user_id, subject, input, state, progress, \
         attempts, ledger_task_id, runner, heartbeat_at, started_at) \
         VALUES ('older', 'plain', 'user:7', 7, '|{MASTER}', '{request}', 'running', \
         '{{\"stage\": \"decomposing\", \"phase\": \"plain\", \"done\": 0, \"total\": 1}}', \
         1, 'rig-job:older', 'proc-older', NOW(), NOW()); \
         INSERT INTO rig_job_keys (caller, idempotency_key, job_id, request_digest) \
         VALUES ('user:7', 'older-1', 'older', '{digest}')",
        request = site_request(),
        digest = rig_jobs::digest(&site_request()),
    ))
    .await
    .unwrap();
    let row = rig_jobs::get(&db, "older").await.unwrap().unwrap();
    assert_eq!(row.owner(), Some(Owner::Site { user_id: 7 }));
    assert_eq!(row.pipeline_input, None);
    // Its page reads it as it did; the same submission under its key is
    // given it, and without a key joins it.
    let caller = rig_jobs::user_caller(7);
    let status = pickup::status_for_its_page(&db, &caller, "older")
        .await
        .unwrap();
    assert_eq!(status["stage"], json!("decomposing"));
    assert_eq!(status["sourceMasterAssetId"], json!(MASTER));
    assert_eq!(status["asked"], site_request()["asked"]);
    assert_eq!(
        site_submission(Some("older-1"))
            .given_before(&db, Phase::Plain)
            .await
            .unwrap()
            .as_deref(),
        Some("older")
    );
    let (joined, new) = claim(
        &db,
        &site_submission(None),
        Phase::Plain,
        Stage::Decomposing,
        1,
        Default::default(),
    )
    .await
    .unwrap();
    assert_eq!((joined.as_str(), new), ("older", false));
    schema.drop().await;
}
