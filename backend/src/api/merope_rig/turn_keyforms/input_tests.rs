//! Jobs run from their records. The site's persona flow from its handler
//! through the run from the record to the end that checks the persona's
//! master; a caller's own picture checked as an untrusted upload and kept by
//! its content; an owner other than the site, with no persona at all, kept
//! to its own pictures, storage and keys; records that cannot be run
//! refused; a job run from its record cancelled; and what an older process
//! wrote read.

use std::{io::Cursor, path::PathBuf, time::Duration};

use axum::{
    body::Bytes,
    extract::{Extension, Query, State},
};
use sea_orm::{ConnectionTrait, DatabaseConnection};
use serde_json::json;

use super::super::{
    Asked, Beside, Claim, Holds, Made, Phase, Stage, Submission, TurnCache, TurnKeyformsRequest,
    archive, claim, claim_for, live, pickup, recorded, run_recorded, start_claimed, start_supplied,
    start_turn_keyforms, together::JOB_TESTS, update,
};
use super::*;
use crate::{
    db::IsolatedSchema,
    middleware::auth::Claims,
    services::{
        agent::merope,
        image_cache::ImageCacheService,
        rig_jobs::{self, Owner},
    },
};

/// A picture with something drawn on it, its content its own.
fn portrait() -> Vec<u8> {
    let seed = uuid::Uuid::new_v4().as_bytes()[0];
    let mut picture = image::RgbaImage::new(512, 640);
    for (x, y, pixel) in picture.enumerate_pixels_mut() {
        *pixel = image::Rgba([(x * 5) as u8, (y * 3) as u8, seed, 255]);
    }
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgba8(picture)
        .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
        .unwrap();
    bytes
}

/// A picture in the site's media: a file of the site's image cache, by the
/// URL it is served at. The file, to remove when the test is done.
fn in_site_media(bytes: &[u8]) -> (PictureRef, PathBuf) {
    use sha2::{Digest, Sha256};
    let stem = hex::encode(Sha256::digest(bytes));
    let url = format!("/api/phantasi/image-cache/{}/{stem}.png", &stem[..2]);
    let path = ImageCacheService::new()
        .local_path_for_public_url(&url)
        .expect("an image cache URL");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, bytes).unwrap();
    (PictureRef::SiteMedia { url }, path)
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

/// Providers as a test has them: nothing is asked of the Space while what it
/// would make is cached.
fn providers() -> Providers {
    Providers {
        generation: generation(),
        client: see_through::SeeThroughClient::offline(),
        space_name: "owner/space".into(),
    }
}

/// What the Space would make of `picture` as a plain split, cached in
/// `storage` as a job of it before would have left it.
async fn plain_cached(
    storage: &rig_jobs::Storage,
    picture: &[u8],
    generation: &image_generation::ImageGenerationConfig,
    space: &str,
    psd: &[u8],
) -> TurnCache {
    let cache = TurnCache::for_master(
        storage,
        picture,
        generation,
        space,
        see_through::DecomposeOptions::default(),
    );
    cache.write("decomposition-0.psd", psd).await;
    cache.write("decomposition-0.checked", b"checked").await;
    cache
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

/// A claim of `make` of `picture` for `owner`, its end held to `holds`.
fn claim_of(
    owner: &Owner,
    key: Option<&str>,
    picture: &PictureRef,
    make: Make,
    holds: Holds,
) -> Claim {
    let input = JobInput::Picture {
        picture: picture.clone(),
        make,
    };
    Claim {
        owner: owner.clone(),
        idempotency_key: key.map(str::to_owned),
        request: input.to_value(),
        subject: picture.name(),
        input,
        holds,
    }
}

#[tokio::test]
async fn a_job_for_an_owner_with_no_persona_runs_from_its_record_into_its_own_storage() {
    let _slot = JOB_TESTS.lock().await;
    let Some(schema) = without_persona("rig_input_plain").await else {
        return;
    };
    let db = schema.db.clone();
    let owner = isolated("plain");
    let storage = owner.storage();
    let whole = portrait();
    let supplied = supply_picture(&storage, Bytes::from(whole.clone()))
        .await
        .unwrap();
    let picture = PictureRef::from(supplied.clone());
    plain_cached(
        &storage,
        &whole,
        &generation(),
        "owner/space",
        b"8BPS front",
    )
    .await;

    let start = || {
        start_supplied(
            db.clone(),
            owner.clone(),
            Some("plain-1".into()),
            supplied.clone(),
            Make::Plain,
            providers(),
        )
    };
    let (id, new) = start().await.unwrap();
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
        (owner.caller().as_str(), None)
    );
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
    assert_eq!(
        serde_json::from_value::<JobInput>(row.pipeline_input.clone().unwrap()).unwrap(),
        JobInput::Picture {
            picture,
            make: Make::Plain
        }
    );
    // Its owner fetches what it made; the same key gives the same job.
    let psd = pickup::fetched(&db, &owner.caller(), &id, pickup::Part::Psd)
        .await
        .unwrap();
    assert_eq!(psd.as_slice(), b"8BPS front");
    let (again, new) = start().await.unwrap();
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
    assert!(
        pickup::status_for_its_page(&db, &rig_jobs::user_caller(7), &id)
            .await
            .is_err()
    );
    assert!(merope::get_persona(&db).await.unwrap().is_none());
    let _ = std::fs::remove_dir_all(storage.root());
    schema.drop().await;
}

#[tokio::test]
async fn two_owners_under_the_same_key_get_their_own_jobs_and_fetch_only_their_own() {
    let _slot = JOB_TESTS.lock().await;
    let Some(schema) = without_persona("rig_input_owners").await else {
        return;
    };
    let db = schema.db.clone();
    let picture = PictureRef::SiteMedia {
        url: "/media/assets/m/portrait.png".into(),
    };
    let mut jobs = Vec::new();
    for owner in [isolated("one"), isolated("two")] {
        let claim = claim_of(
            &owner,
            Some("same-key"),
            &picture,
            Make::Bust,
            Holds::AsMade,
        );
        let (id, new) = claim_for(
            &db,
            claim,
            Phase::Bust,
            Stage::Generating,
            4,
            Default::default(),
        )
        .await
        .unwrap();
        assert!(new, "a key of one owner's gives no job of another's");
        let mut made = kept_bust();
        made.files.baked = format!("8BPS {}", owner.scope()).into_bytes();
        let work = async { ApiResult::Ok(made) };
        live::run_job(db.clone(), id.clone(), work, meta_of(&picture)).await;
        jobs.push((owner, id));
    }
    // Each kept in its owner's archive, fetched from there.
    live::JOBS.lock().unwrap().clear();
    let [(one, first), (two, second)] = <[_; 2]>::try_from(jobs).unwrap();
    assert_ne!(first, second);
    for (owner, id) in [(&one, &first), (&two, &second)] {
        let mine = pickup::fetched(&db, &owner.caller(), id, pickup::Part::Psd)
            .await
            .unwrap();
        assert_eq!(
            mine.as_slice(),
            format!("8BPS {}", owner.scope()).as_bytes()
        );
    }
    for (caller, id) in [(one.caller(), &second), (two.caller(), &first)] {
        let refused = pickup::fetched(&db, &caller, id, pickup::Part::Psd)
            .await
            .unwrap_err();
        assert_eq!(refused.0, StatusCode::NOT_FOUND);
    }
    let _ = std::fs::remove_dir_all(one.storage().root());
    let _ = std::fs::remove_dir_all(two.storage().root());
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

async fn claimed_bust(
    db: &DatabaseConnection,
    owner: &Owner,
    picture: &PictureRef,
    holds: Holds,
) -> String {
    let claim = claim_of(owner, None, picture, Make::Bust, holds);
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
    let picture = PictureRef::SiteMedia {
        url: "/media/assets/m/portrait.png".into(),
    };
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
    let keys = pickup::fetched(&db, &owner.caller(), &id, pickup::Part::Keys)
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

const OWNER: i32 = 7;

fn claims() -> Claims {
    Claims {
        sub: OWNER.to_string(),
        username: "owner".into(),
        is_admin: true,
        is_owner: true,
        exp: i64::MAX,
        iat: 0,
        tv: 0,
        subject: crate::middleware::auth::AuthSubject::from_test_sub(&OWNER.to_string()),
    }
}

/// The site's settings as a test has them: her switch on, an image source
/// that answers nowhere, and a Space token of the right shape. What they
/// were, to put back.
async fn site_configured() -> crate::config::DynamicConfig {
    let mut config = crate::GLOBAL_DYNAMIC_CONFIG.write().await;
    let saved = config.clone();
    config.merope_enabled = true;
    config.pro_enabled = true;
    config.ai_image_source = "rig-input-test-images".into();
    config.ai_image_model = "test-image-model".into();
    config.ai_vendor_sources = vec![crate::config::AiVendorSource {
        slug: "rig-input-test-images".into(),
        kind: "openai".into(),
        display_name: "Test images".into(),
        enabled: true,
        credential_mode: "own".into(),
        api_key: Some("provider-test-key".into()),
        base_url: "http://127.0.0.1:9".into(),
        ..crate::config::AiVendorSource::default()
    }];
    config.see_through_hf_token = Some("hf_testtokentesttokentesttoken".into());
    saved
}

#[tokio::test]
async fn the_sites_start_runs_from_its_record_and_ends_on_the_personas_master() {
    let _turn = super::super::super::DYNAMIC_CONFIG_TURN.lock().await;
    let _slot = JOB_TESTS.lock().await;
    let Some(schema) = without_persona("rig_input_site_start").await else {
        return;
    };
    let db = schema.db.clone();
    let whole = portrait();
    let (picture, file) = in_site_media(&whole);
    let PictureRef::SiteMedia { url } = &picture else {
        unreachable!("a picture in the site's media")
    };
    db.execute_unprepared(&format!(
        "INSERT INTO agent_persona (id, name, updated_at, portrait_asset_id) \
         VALUES ('site', 'Merope', NOW(), '{url}')"
    ))
    .await
    .unwrap();
    let saved = site_configured().await;
    let (generation, space) = {
        let config = crate::GLOBAL_DYNAMIC_CONFIG.read().await;
        (
            image_generation::config_from_dynamic(&config).unwrap(),
            see_through::configured_space(&config).name().to_owned(),
        )
    };
    let site = rig_jobs::Storage::site();
    let cache = plain_cached(&site, &whole, &generation, &space, b"8BPS site").await;

    let payload: TurnKeyformsRequest =
        serde_json::from_value(json!({ "sourceMasterAssetId": url, "plain": true })).unwrap();
    let started = start_turn_keyforms(
        State(db.clone()),
        Extension(claims()),
        Query(Beside::default()),
        HeaderMap::new(),
        Json(payload),
    )
    .await;
    *crate::GLOBAL_DYNAMIC_CONFIG.write().await = saved;
    let id = started.unwrap().0["jobId"].as_str().unwrap().to_owned();
    let row = ended(&db, &id).await;
    assert_eq!(
        row.state(),
        rig_jobs::State::Succeeded,
        "{:?}",
        row.error_detail
    );
    assert_eq!(row.owner(), Some(Owner::Site { user_id: OWNER }));
    assert_eq!(
        row.pipeline_input,
        Some(json!({
            "from": "picture",
            "picture": { "kind": "siteMedia", "url": url },
            "make": { "shape": "plain" }
        }))
    );
    let psd = pickup::fetched(&db, &rig_jobs::user_caller(OWNER), &id, pickup::Part::Psd)
        .await
        .unwrap();
    assert_eq!(psd.as_slice(), b"8BPS site");
    cache.remove();
    let _ = std::fs::remove_file(file);
    schema.drop().await;
}

#[tokio::test]
async fn a_record_that_names_no_input_or_an_unknown_owner_is_refused_cleanly() {
    let _slot = JOB_TESTS.lock().await;
    let Some(schema) = super::super::live::tests::schema("rig_input_refused").await else {
        return;
    };
    let db = schema.db.clone();
    for broken in [
        "pipeline_input = NULL",
        "owner_scope = 'tenant:1'",
        "pipeline_input = '{\"from\": \"elsewhere\"}'",
    ] {
        let (id, _) = claim(
            &db,
            &site_submission(None),
            Phase::Plain,
            Stage::Decomposing,
            1,
            Default::default(),
        )
        .await
        .unwrap();
        db.execute_unprepared(&format!("UPDATE rig_jobs SET {broken} WHERE id = '{id}'"))
            .await
            .unwrap();
        assert!(recorded(&db, &id).await.is_err(), "{broken}");
        let work = run_recorded(db.clone(), id.clone(), providers());
        live::run_job(db.clone(), id.clone(), work, meta_of(&site_picture())).await;
        let row = rig_jobs::get(&db, &id).await.unwrap().unwrap();
        assert_eq!(row.state(), rig_jobs::State::Failed, "{broken}");
        assert_eq!(row.result_ref, None);
    }
    schema.drop().await;
}

#[tokio::test]
async fn a_job_run_from_its_record_is_cancelled_where_it_stands() {
    let _slot = JOB_TESTS.lock().await;
    let Some(schema) = without_persona("rig_input_cancel").await else {
        return;
    };
    let db = schema.db.clone();
    let owner = Owner::Site { user_id: 7 };
    // The picture is a pipe nothing writes to yet: the run waits in reading it.
    let stem = format!("{:064x}", uuid::Uuid::new_v4().as_u128());
    let url = format!("/api/phantasi/image-cache/{}/{stem}.png", &stem[..2]);
    let pipe = ImageCacheService::new()
        .local_path_for_public_url(&url)
        .unwrap();
    std::fs::create_dir_all(pipe.parent().unwrap()).unwrap();
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&pipe)
            .status()
            .unwrap()
            .success()
    );
    let picture = PictureRef::SiteMedia { url };
    let claim = claim_of(&owner, None, &picture, Make::Plain, Holds::AsMade);
    let (id, _) = start_claimed(db.clone(), claim, providers(), Default::default())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    let asked = rig_jobs::request_cancel(&db, &owner.caller(), &id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(asked.state(), rig_jobs::State::Running);
    let row = ended(&db, &id).await;
    assert_eq!(row.state(), rig_jobs::State::Cancelled);
    // What the cancelled run was reading is let go.
    let writer = pipe.clone();
    std::thread::spawn(move || std::fs::write(writer, b"").ok())
        .join()
        .unwrap();
    let _ = std::fs::remove_file(pipe);
    schema.drop().await;
}

const MASTER: &str = "/media/assets/m/portrait.png";

fn site_picture() -> PictureRef {
    PictureRef::SiteMedia { url: MASTER.into() }
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

/// What the site's persona page asks for a plain bust, as its record keeps it.
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
async fn the_site_records_its_owner_and_input_beside_its_request_and_key() {
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
    // What was asked, in the page's terms, and its key's digest of it.
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
    update(&id, |job| job.stage = Stage::Done);
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
    // Claimed and running on a process whose rows name neither owner scope
    // nor input: its row and key as that process writes them.
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
    // Its page reads it as it is; the same submission under its key is
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

fn png_of(picture: image::DynamicImage) -> Vec<u8> {
    let mut bytes = Vec::new();
    picture
        .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
        .unwrap();
    bytes
}

fn grey(width: u32, height: u32) -> Vec<u8> {
    png_of(image::DynamicImage::ImageLuma8(image::GrayImage::new(
        width, height,
    )))
}

#[tokio::test]
async fn a_supplied_picture_is_taken_only_as_an_upload_may_be() {
    let owner = isolated("supplied");
    let storage = owner.storage();
    let refused = |bytes: Vec<u8>| {
        let storage = storage.clone();
        async move {
            let (status, body) = supply_picture(&storage, Bytes::from(bytes))
                .await
                .expect_err("refused");
            (
                status,
                body.0["error"].as_str().unwrap_or_default().to_owned(),
            )
        }
    };
    // Too large, not a picture, a type the pipeline does not take.
    assert_eq!(
        refused(vec![0; MAX_PORTRAIT_BYTES + 1]).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        refused(b"not a picture".to_vec()).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        refused(b"GIF89a\x01\x00\x01\x00".to_vec()).await.0,
        StatusCode::BAD_REQUEST
    );
    // Past the media library's edge, or its pixels within the edge.
    assert_eq!(refused(grey(8193, 600)).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(refused(grey(4100, 4100)).await.0, StatusCode::BAD_REQUEST);
    // Shorter than the pipeline's canvas needs on either side.
    let (status, error) = refused(grey(511, 900)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(error.contains("512"), "{error}");
    // A 16-bit RGBA picture within the pixel limit whose pixels decode past
    // the decode allowance: refused as too large, as one past the edge is.
    let deep = png_of(image::DynamicImage::ImageRgba16(image::ImageBuffer::new(
        3000, 3000,
    )));
    assert!(media::validate_bytes(&deep, "image/png", MAX_PORTRAIT_BYTES).is_ok());
    let (status, error) = refused(deep).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(error.contains("exceed the supported limit"), "{error}");
    // Its header reads, its body is cut short.
    let whole = portrait();
    assert_eq!(
        refused(whole[..whole.len() - 40].to_vec()).await.0,
        StatusCode::BAD_REQUEST
    );
    assert!(!supplied_root(&storage).exists(), "nothing refused is kept");

    // Two supplying the same picture at once both have it, kept once.
    let (a, b) = tokio::join!(
        supply_picture(&storage, Bytes::from(whole.clone())),
        supply_picture(&storage, Bytes::from(whole.clone()))
    );
    let supplied = a.unwrap();
    assert_eq!(supplied, b.unwrap());
    let kept: Vec<_> = std::fs::read_dir(supplied_root(&storage))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(kept, [format!("{}.png", sha256_hex(&whole))]);
    // A file there with the same content counts as the picture kept.
    let path = supplied_path(&storage, &sha256_hex(&whole), "image/png").unwrap();
    let before = std::fs::metadata(&path).unwrap().modified().unwrap();
    std::thread::sleep(Duration::from_millis(20));
    supply_picture(&storage, Bytes::from(whole.clone()))
        .await
        .unwrap();
    assert_eq!(
        std::fs::metadata(&path).unwrap().modified().unwrap(),
        before
    );
    let picture = PictureRef::from(supplied);
    let loaded = load_picture(&owner, &picture).await.unwrap();
    assert_eq!(loaded.bytes.as_ref(), whole.as_slice());
    // Another owner does not have it, and a name that walks paths names nothing.
    assert!(
        load_picture(&isolated("supplied-other"), &picture)
            .await
            .is_err()
    );
    let walking = PictureRef::Supplied {
        sha256: "../../etc/passwd".into(),
        media_type: "image/png".into(),
    };
    assert!(load_picture(&owner, &walking).await.is_err());
    // A kept file that is no longer what its name says is not used.
    std::fs::write(&path, grey(600, 600)).unwrap();
    assert!(load_picture(&owner, &picture).await.is_err());
    let _ = std::fs::remove_dir_all(storage.root());
}

#[tokio::test]
async fn only_the_sites_own_jobs_read_the_sites_media() {
    let _slot = JOB_TESTS.lock().await;
    let Some(schema) = without_persona("rig_input_site_media").await else {
        return;
    };
    let db = schema.db.clone();
    let (picture, file) = in_site_media(&portrait());
    let site = Owner::Site { user_id: 7 };
    assert!(load_picture(&site, &picture).await.is_ok());
    let owner = isolated("site-media");
    let (status, _) = load_picture(&owner, &picture).await.unwrap_err();
    assert_eq!(status, StatusCode::BAD_REQUEST);
    // A record of another owner's that names the site's media is run to a
    // failure, not to the site's picture.
    let claim = claim_of(&owner, None, &picture, Make::Plain, Holds::AsMade);
    let (id, _) = start_claimed(db.clone(), claim, providers(), Default::default())
        .await
        .unwrap();
    let row = ended(&db, &id).await;
    assert_eq!(row.state(), rig_jobs::State::Failed);
    let _ = std::fs::remove_file(file);
    schema.drop().await;
}

#[tokio::test]
async fn supplied_pictures_no_job_names_go_once_they_are_old() {
    let _slot = JOB_TESTS.lock().await;
    let Some(schema) = without_persona("rig_input_prune").await else {
        return;
    };
    let db = schema.db.clone();
    let owner = isolated("prune");
    let storage = owner.storage();
    let named = supply_picture(&storage, Bytes::from(portrait()))
        .await
        .unwrap();
    let unnamed = supply_picture(&storage, Bytes::from(portrait()))
        .await
        .unwrap();
    let fresh = supply_picture(&storage, Bytes::from(portrait()))
        .await
        .unwrap();
    let file = |picture: &SuppliedPicture| {
        let PictureRef::Supplied { sha256, media_type } = PictureRef::from(picture.clone()) else {
            unreachable!()
        };
        supplied_path(&storage, &sha256, &media_type).unwrap()
    };
    // The first two were supplied long ago; a job names the first.
    let long_ago = std::time::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
    for picture in [&named, &unnamed] {
        std::fs::File::options()
            .write(true)
            .open(file(picture))
            .unwrap()
            .set_modified(long_ago)
            .unwrap();
    }
    let claim = claim_of(
        &owner,
        None,
        &PictureRef::from(named.clone()),
        Make::Plain,
        Holds::AsMade,
    );
    let (id, _) = claim_for(
        &db,
        claim,
        Phase::Plain,
        Stage::Decomposing,
        1,
        Default::default(),
    )
    .await
    .unwrap();
    super::super::end(&db, &id, rig_jobs::Ending::Cancelled).await;
    prune_supplied(&db, &owner).await;
    assert!(file(&named).exists(), "a job names it");
    assert!(
        !file(&unnamed).exists(),
        "no job names it, supplied long ago"
    );
    assert!(
        file(&fresh).exists(),
        "supplied just now, for a start yet to claim"
    );
    let _ = std::fs::remove_dir_all(storage.root());
    schema.drop().await;
}
