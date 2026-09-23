use super::*;
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use serde_json::json;
use test_support::{Fixture, png};

#[tokio::test]
async fn postgres_writer_fencing_and_delete_retry() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let payload = validate_bytes(&png(), "image/png", 1024 * 1024).unwrap();
    let token = Uuid::new_v4();
    let ctx = MediaContext::site(MediaActor::admin(1).unwrap(), MediaSource::Upload);
    let row = assets::insert_staging(
        &f.db,
        &ctx,
        &payload,
        "test.png",
        None,
        MediaExposure::Private,
        token,
        600,
    )
    .await
    .unwrap();
    let key = row.storage_key.unwrap();
    f.service.store().stage_bytes(token, &png()).await.unwrap();
    f.db.execute_unprepared(&format!(
        "UPDATE media_assets SET write_lease_until = NOW() - interval '1 second' WHERE id = {}",
        row.id
    ))
    .await
    .unwrap();
    assert_eq!(
        f.service
            .renew_write_lease(&f.db, row.id, token)
            .await
            .unwrap_err(),
        MediaError::NotReady
    );
    f.service.recover_expired(&f.db, 16).await.unwrap();
    assert_eq!(
        f.service
            .commit_staged(&f.db, row.id, token, &key, &content_path(row.id))
            .await
            .unwrap_err(),
        MediaError::NotReady
    );
    assert!(
        f.service
            .store()
            .final_checksum(&key)
            .await
            .unwrap()
            .is_none()
    );
    let image = f.image().await;
    // Simulate process death after marking deleting, before unlink.
    f.db.execute_unprepared(&format!(
        "UPDATE media_assets SET state = 'deleting' WHERE id = {}",
        image.id
    ))
    .await
    .unwrap();
    maintenance::retry_deletions(&f.service, &f.db, 16)
        .await
        .unwrap();
    assert_eq!(
        assets::find_by_id(&f.db, image.id)
            .await
            .unwrap()
            .unwrap()
            .state
            .as_deref(),
        Some("deleted")
    );
    assert!(
        f.service
            .store()
            .final_checksum(&storage_key(image.public_id, "png").unwrap())
            .await
            .unwrap()
            .is_none()
    );
    f.close().await;
}

#[tokio::test]
async fn postgres_result_references_and_mailbox_rollback_together() {
    use crate::services::ai_task_registry::*;
    let Some(f) = Fixture::new().await else {
        return;
    };
    let image = f.image().await;
    let task = PersistedAiTask {
        runtime_id: "test".into(),
        subject_id: 1,
        owner_id: 1,
        tapp_id: "test".into(),
        idempotency_key: None,
        request_hash: [0; 32],
        retain_until: chrono::Utc::now().timestamp() + 900,
        snapshot: AiTaskSnapshot {
            task_id: "test-result".into(),
            status: AiTaskStatus::Completed,
            operation: myriad_tapp_contract::manifest::TappAiOperation::Image,
            delivery: AiTaskDelivery::Result,
            created_at: "now".into(),
            updated_at: "now".into(),
            result: Some(json!({"url": image.content_path})),
            error: None,
            usage: serde_json::from_value(json!({
                "calls": {"limit": 10, "used": 0, "remaining": 10, "resetsAt": "x"},
                "tokens": {"limit": 10, "used": 0, "remaining": 10, "resetsAt": "x"},
                "cooldown": {"requiredSeconds": 0, "remainingSeconds": 0},
                "restricted": false, "unlimited": false, "role": "user"
            }))
            .unwrap(),
        },
    };
    f.db.execute_unprepared("CREATE FUNCTION fail_media_ref() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected media ref failure'; END $$; CREATE TRIGGER fail_media_ref BEFORE INSERT ON media_references FOR EACH ROW EXECUTE FUNCTION fail_media_ref()").await.unwrap();
    assert!(
        crate::services::ai_task_runtime::persist_terminal_task(&f.db, &task)
            .await
            .is_err()
    );
    let counts = f.db.query_one_raw(Statement::from_string(DatabaseBackend::Postgres,
        "SELECT (SELECT COUNT(*) FROM tapp_runtime_registry)::bigint AS tasks, (SELECT COUNT(*) FROM tapp_runtime_mailbox)::bigint AS messages")).await.unwrap().unwrap();
    assert_eq!(counts.try_get::<i64>("", "tasks").unwrap(), 0);
    assert_eq!(counts.try_get::<i64>("", "messages").unwrap(), 0);
    f.db.execute_unprepared("DROP TRIGGER fail_media_ref ON media_references")
        .await
        .unwrap();
    crate::services::ai_task_runtime::persist_terminal_task(&f.db, &task)
        .await
        .unwrap();
    assert_eq!(
        f.service.delete(&f.db, image.id).await.unwrap_err(),
        MediaError::InUse
    );
    f.close().await;
}

#[tokio::test]
async fn postgres_html_publish_and_stickers_protect_assets() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let image = f.image().await;
    let txn = f.db.begin().await.unwrap();
    let (_, body) = publish_cited_media(
        &txn,
        &[],
        None,
        &format!("<img src='{}'>", image.content_path),
    )
    .await
    .unwrap();
    assert!(body.contains("/media/assets/"));
    bind_note_published(&txn, 71, None, &body, &[])
        .await
        .unwrap();
    let layout = json!({"standard": [], "free": [{"type": "sticker", "config": {"imageUrl": image.content_path}}]});
    let rewritten = bind_and_publish_dashboard_layout(&txn, &layout.to_string(), &[])
        .await
        .unwrap();
    assert!(rewritten.contains("/media/assets/"));
    txn.commit().await.unwrap();
    assert_eq!(
        f.service.delete(&f.db, image.id).await.unwrap_err(),
        MediaError::InUse
    );
    assert_eq!(
        f.service.unpublish(&f.db, image.id).await.unwrap_err(),
        MediaError::PublicInUse
    );
    f.close().await;
}

#[tokio::test]
async fn postgres_upgrade_resumes_over_1000_and_preserves_cached_citations() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let legacy_root = f.service.store().root().join("legacy");
    let paths = LegacyPaths {
        federation_root: legacy_root.join("federation"),
        cache_images: legacy_root.join("cache"),
    };
    let hash = "a".repeat(64);
    let url = format!("/api/phantasi/image-cache/aa/{hash}.png");
    let source = paths.cache_images.join("aa").join(format!("{hash}.png"));
    tokio::fs::create_dir_all(source.parent().unwrap())
        .await
        .unwrap();
    tokio::fs::write(&source, png()).await.unwrap();
    f.db.execute_raw(Statement::from_sql_and_values(DatabaseBackend::Postgres,
        "INSERT INTO phantasi_note_docs(user_id, title, content_md) SELECT 1, 'old note', $1 FROM generate_series(1,1001)",
        [format!("<img src='{url}'>").into()])).await.unwrap();
    let first = upgrade::advance(&f.db, f.service.store(), &paths, &[], false)
        .await
        .unwrap();
    assert!(!first.complete);
    let mut progress = upgrade::advance(&f.db, f.service.store(), &paths, &[], false)
        .await
        .unwrap();
    assert!(progress.error.is_none(), "{:?}", progress.error);
    assert_eq!(upgrade::status(&f.db).await.unwrap().after, progress.after);
    let migrated_id = resolve_asset_id(&f.db, &url).await.unwrap().unwrap();
    assert!(
        !assets::find_by_id(&f.db, migrated_id)
            .await
            .unwrap()
            .unwrap()
            .references_complete
    );
    // Recreate the store handle as after a process restart; progress is only in DB.
    let resumed_store = MediaStore::new(f.service.store().root().to_path_buf());
    for _ in 0..80 {
        if progress.complete {
            break;
        }
        progress = upgrade::advance(&f.db, &resumed_store, &paths, &[], false)
            .await
            .unwrap();
        assert!(
            progress.error.is_none(),
            "phase {} cursor {}: {:?}",
            progress.phase,
            progress.after,
            progress.error
        );
    }
    assert!(progress.complete);
    assert_eq!(active_count(&f.db, migrated_id).await.unwrap(), 1001);
    assert!(
        assets::find_by_id(&f.db, migrated_id)
            .await
            .unwrap()
            .unwrap()
            .references_complete
    );
    let count_before = progress.scanned;
    assert_eq!(
        upgrade::advance(&f.db, &resumed_store, &paths, &[], false)
            .await
            .unwrap()
            .scanned,
        count_before
    );
    // Removing the cache volume must not remove historical media bytes.
    tokio::fs::remove_dir_all(&paths.cache_images)
        .await
        .unwrap();
    let row = assets::find_by_id(&f.db, migrated_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        tokio::fs::read(resumed_store.final_path(&row.storage_key.unwrap()).unwrap())
            .await
            .unwrap(),
        png()
    );
    assert_eq!(
        f.service.delete(&f.db, migrated_id).await.unwrap_err(),
        MediaError::InUse
    );
    f.close().await;
}

#[tokio::test]
async fn postgres_upgrade_failure_is_deferred_and_retries() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let paths = LegacyPaths {
        federation_root: f.service.store().root().join("legacy"),
        cache_images: f.service.store().root().join("cache"),
    };
    f.db.execute_unprepared("INSERT INTO media_assets(kind,url,mime,name,size) VALUES ('upload','/media/federation/1/missing.png','image/png','missing.png',0)").await.unwrap();
    let mut failed = upgrade::advance(&f.db, f.service.store(), &paths, &[], false)
        .await
        .unwrap();
    for _ in 0..50 {
        if failed.next_retry_at.is_some() {
            break;
        }
        failed = upgrade::advance(&f.db, f.service.store(), &paths, &[], false)
            .await
            .unwrap();
    }
    assert_eq!(failed.error.as_deref(), Some("MEDIA_MISSING"));
    assert_eq!(failed.after, "");
    assert!(!failed.complete);
    tokio::fs::create_dir_all(paths.federation_root.join("1"))
        .await
        .unwrap();
    tokio::fs::write(paths.federation_root.join("1/missing.png"), png())
        .await
        .unwrap();
    let retried = drive_upgrade(&f, &paths, chrono::Utc::now().timestamp() + 3600).await;
    assert!(retried.complete);
    assert!(retried.error.is_none());
    assert_eq!(retried.pending_failures, 0);
    f.close().await;
}

#[tokio::test]
async fn postgres_persona_url_rewrite_preserves_generation_and_public_avatar() {
    use crate::services::agent::merope;
    let Some(f) = Fixture::new().await else {
        return;
    };
    let portrait = f.image().await;
    let avatar = f.image().await;
    f.db.execute_raw(Statement::from_sql_and_values(DatabaseBackend::Postgres,
        "INSERT INTO agent_persona(id,name,personality,portrait_asset_id,avatar_asset_id,avatar_generation,updated_at) VALUES ('site','Test','Test',$1,$2,'{\"fingerprint\":\"keep\"}',NOW())",
        [portrait.content_path.clone().into(), avatar.content_path.clone().into()])).await.unwrap();
    let txn = f.db.begin().await.unwrap();
    let portrait_url = publish_local_url(&txn, &portrait.content_path, &[])
        .await
        .unwrap();
    let avatar_url = publish_local_url(&txn, &avatar.content_path, &[])
        .await
        .unwrap();
    let persona = merope::get_persona_on(&txn).await.unwrap().unwrap();
    let saved = merope::rewrite_persona_media_urls(
        &txn,
        persona,
        Some(portrait_url),
        Some(avatar_url.clone()),
    )
    .await
    .unwrap();
    bind_persona(
        &txn,
        saved.portrait_asset_id.as_deref(),
        saved.avatar_asset_id.as_deref(),
        saved.visual_profile.as_ref(),
        &[],
    )
    .await
    .unwrap();
    txn.commit().await.unwrap();
    assert_eq!(saved.avatar_asset_id.as_deref(), Some(avatar_url.as_str()));
    assert_eq!(
        saved.avatar_generation,
        Some(json!({"fingerprint": "keep"}))
    );
    assert_eq!(
        f.service.delete(&f.db, avatar.id).await.unwrap_err(),
        MediaError::InUse
    );
    assert!(matches!(
        resolve_public_asset(
            &f.db,
            f.service.store(),
            avatar.public_id,
            avatar_url.rsplit('/').next().unwrap()
        )
        .await
        .unwrap(),
        ServeOutcome::File(_)
    ));
    f.close().await;
}

#[tokio::test]
async fn postgres_channel_reference_failure_never_admits_run() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let user_id = 910071;
    f.db.execute_unprepared("INSERT INTO users(id,username,is_admin,auth_provider) VALUES (910071,'media-channel-test',TRUE,'local')").await.unwrap();
    let image = f.image().await;
    f.db.execute_unprepared("CREATE FUNCTION reject_channel_media() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected media ref failure'; END $$; CREATE TRIGGER reject_channel_media BEFORE INSERT ON media_references FOR EACH ROW EXECUTE FUNCTION reject_channel_media()").await.unwrap();
    let claims = crate::middleware::auth::Claims {
        sub: user_id.to_string(),
        username: "media-channel-test".into(),
        is_admin: true,
        is_owner: false,
        exp: chrono::Utc::now().timestamp() + 60,
        iat: chrono::Utc::now().timestamp(),
        tv: 0,
    };
    let req = crate::api::agent::ProcessRequest {
        input: "Inspect the attached image".into(),
        context: Some(crate::api::agent::ProcessContext {
            custom_data: Some(json!({"attachments": [{"url": image.content_path}]})),
            ..Default::default()
        }),
    };
    assert_eq!(
        crate::services::agent::run_hub::user_executing_run_count(user_id).await,
        0
    );
    let result = crate::api::agent::start_process_run(f.db.clone(), claims, req).await;
    match result {
        Err(error) => assert_eq!(error.0.code(), Some("MEDIA_STORE_FAILED")),
        Ok(run) => {
            run.abort_execution().await;
            panic!("failed media reference must not start a run");
        }
    }
    assert_eq!(
        crate::services::agent::run_hub::user_executing_run_count(user_id).await,
        0
    );
    assert_eq!(active_count(&f.db, image.id).await.unwrap(), 0);
    f.close().await;
}

#[tokio::test]
async fn postgres_automatic_upgrade_starts_retries_resumes_and_stops() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let paths = LegacyPaths {
        federation_root: f.service.store().root().join("old"),
        cache_images: f.service.store().root().join("cache"),
    };
    f.db.execute_unprepared("INSERT INTO media_assets(kind,url,mime,name,size) VALUES ('upload','/media/federation/1/automatic.png','image/png','automatic.png',0)").await.unwrap();
    let now = chrono::Utc::now().timestamp();
    // A second replica must return immediately while an admin/worker owns the job.
    let lock = f.db.begin().await.unwrap();
    lock.execute_unprepared(
        "SELECT pg_advisory_xact_lock(hashtextextended('media:upgrade:v2', 0))",
    )
    .await
    .unwrap();
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            upgrade::automatic_step(&f.db, f.service.store(), &paths, &[], now)
        )
        .await
        .unwrap()
        .unwrap()
        .is_none()
    );
    lock.rollback().await.unwrap();
    // No admin request creates or advances this job.
    let mut progress = upgrade::UpgradeProgress::default();
    for _ in 0..40 {
        progress = upgrade::automatic_step(&f.db, f.service.store(), &paths, &[], now)
            .await
            .unwrap()
            .unwrap();
        if progress.next_retry_at.is_some() {
            break;
        }
    }
    assert_eq!(progress.error.as_deref(), Some("MEDIA_MISSING"));
    assert_eq!(progress.consecutive_failures, 1);
    assert_eq!(progress.next_retry_at, Some(now + 60));
    let cursor = (
        progress.pass,
        progress.phase,
        progress.after.clone(),
        progress.scanned,
    );
    assert!(
        upgrade::automatic_step(&f.db, f.service.store(), &paths, &[], now + 59)
            .await
            .unwrap()
            .is_none()
    );
    // A new handle reads durable backoff and cursor, as a restarted process would.
    let resumed = MediaStore::new(f.service.store().root().to_path_buf());
    let second = drive_upgrade(&f, &paths, now + 60).await;
    assert_eq!(
        (second.pass, second.phase, second.after.clone()),
        (cursor.0, cursor.1, cursor.2)
    );
    assert!(second.scanned > cursor.3);
    assert_eq!(second.consecutive_failures, 2);
    assert_eq!(second.next_retry_at, Some(now + 180));
    tokio::fs::create_dir_all(paths.federation_root.join("1"))
        .await
        .unwrap();
    tokio::fs::write(paths.federation_root.join("1/automatic.png"), png())
        .await
        .unwrap();
    for _ in 0..40 {
        progress = upgrade::automatic_step(&f.db, &resumed, &paths, &[], now + 180)
            .await
            .unwrap()
            .unwrap();
        if progress.complete {
            break;
        }
    }
    assert!(progress.complete);
    assert_eq!(progress.consecutive_failures, 0);
    assert!(progress.error.is_none());
    assert!(
        upgrade::automatic_step(&f.db, &resumed, &paths, &[], now + 3600)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        upgrade::status(&f.db).await.unwrap().scanned,
        progress.scanned
    );
    assert!(
        paths.federation_root.join("1/automatic.png").exists(),
        "source is retained"
    );
    f.close().await;
}

async fn drive_upgrade(f: &Fixture, paths: &LegacyPaths, now: i64) -> upgrade::UpgradeProgress {
    let mut result = upgrade::UpgradeProgress::default();
    for _ in 0..100 {
        result = upgrade::automatic_step(&f.db, f.service.store(), paths, &[], now)
            .await
            .unwrap()
            .unwrap();
        if result.complete || result.next_retry_at.is_some() {
            break;
        }
    }
    result
}

#[tokio::test]
async fn postgres_regression_brew_and_phantasi_aliases_of_one_file() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let paths = LegacyPaths {
        federation_root: f.service.store().root().join("old"),
        cache_images: f.service.store().root().join("cache"),
    };
    let hash = "a".repeat(64);
    let brew = format!("/api/brew/image-cache/aa/{hash}.png");
    let phantasi = format!("/api/phantasi/image-cache/aa/{hash}.png");
    tokio::fs::create_dir_all(paths.cache_images.join("aa"))
        .await
        .unwrap();
    tokio::fs::write(paths.cache_images.join(format!("aa/{hash}.png")), png())
        .await
        .unwrap();
    f.db.execute_raw(Statement::from_sql_and_values(DatabaseBackend::Postgres,
        "INSERT INTO phantasi_note_docs(user_id,title,content_md) VALUES (1,'old mixed aliases',$1)",
        [format!("![old]({brew})\n![new]({phantasi})").into()])).await.unwrap();
    let progress = drive_upgrade(&f, &paths, chrono::Utc::now().timestamp()).await;
    let result = (
        progress.complete,
        progress.error.clone(),
        progress.error_source.clone(),
    );
    f.close().await;
    assert!(
        result.0,
        "same physical cache file must migrate: {result:?}"
    );
}

#[tokio::test]
async fn postgres_regression_stored_private_sticker_is_repaired() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let paths = LegacyPaths {
        federation_root: f.service.store().root().join("old"),
        cache_images: f.service.store().root().join("cache"),
    };
    migration::record_job(
        &f.db,
        "upgrade",
        "platform_media_v2",
        None,
        "copied",
        "verified",
        "switched",
        None,
        Some(
            &serde_json::to_string(&upgrade::UpgradeProgress {
                complete: true,
                ..Default::default()
            })
            .unwrap(),
        ),
    )
    .await
    .unwrap();
    let image = f.image().await;
    let layout =
        json!({"standard":[],"free":[{"type":"sticker","config":{"imageUrl":image.content_path}}]});
    f.db.execute_raw(Statement::from_sql_and_values(DatabaseBackend::Postgres,
        "INSERT INTO configurations(key,value,updated_at) VALUES ('dashboard_layout',$1::json,NOW())",
        [json!(layout.to_string()).to_string().into()])).await.unwrap();
    let progress = drive_upgrade(&f, &paths, chrono::Utc::now().timestamp()).await;
    let row = assets::find_by_id(&f.db, image.id).await.unwrap().unwrap();
    let cfg =
        f.db.query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT value::text AS value FROM configurations WHERE key='dashboard_layout'",
        ))
        .await
        .unwrap()
        .unwrap();
    let value: String = cfg.try_get("", "value").unwrap();
    let evidence = (progress.complete, row.exposure, value);
    f.close().await;
    assert!(
        evidence.0
            && evidence.1.as_deref() == Some("public")
            && evidence.2.contains("/media/assets/"),
        "completed upgrade must fix public sticker: {evidence:?}"
    );
}

#[tokio::test]
async fn postgres_regression_missing_file_does_not_starve_other_valid_assets() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let paths = LegacyPaths {
        federation_root: f.service.store().root().join("old"),
        cache_images: f.service.store().root().join("cache"),
    };
    f.db.execute_unprepared("INSERT INTO media_assets(kind,url,mime,name,size) VALUES ('upload','/media/federation/1/missing.png','image/png','missing.png',0), ('upload','/media/federation/1/valid.png','image/png','valid.png',0)").await.unwrap();
    tokio::fs::create_dir_all(paths.federation_root.join("1"))
        .await
        .unwrap();
    tokio::fs::write(paths.federation_root.join("1/valid.png"), png())
        .await
        .unwrap();
    f.db.execute_unprepared("INSERT INTO phantasi_note_docs(user_id,title,content_md) VALUES (1,'healthy note','![image](/media/federation/1/valid.png)')").await.unwrap();
    let progress = drive_upgrade(&f, &paths, chrono::Utc::now().timestamp()).await;
    assert!(
        active_count(&f.db, 2).await.unwrap() > 0,
        "healthy consumers must also bind before retries"
    );
    let valid = assets::find_by_id(&f.db, 2).await.unwrap().unwrap();
    let evidence = (progress.error, progress.error_source, valid.state);
    assert!(!progress.complete);
    assert_eq!(progress.pending_failures, 1);
    assert!(
        !valid.references_complete,
        "unresolved consumers retain deletion protection"
    );
    assert!(f.service.delete(&f.db, 2).await.is_err());
    tokio::fs::write(paths.federation_root.join("1/missing.png"), png())
        .await
        .unwrap();
    let repaired = drive_upgrade(&f, &paths, progress.next_retry_at.unwrap()).await;
    assert!(repaired.complete);
    assert_eq!(repaired.pending_failures, 0);
    f.close().await;
    assert_eq!(
        evidence.2.as_deref(),
        Some("ready"),
        "missing cache must not starve healthy file: {evidence:?}"
    );
}

#[tokio::test]
async fn postgres_upgrade_repairs_preexisting_duplicate_cache_catalog() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let paths = LegacyPaths {
        federation_root: f.service.store().root().join("old"),
        cache_images: f.service.store().root().join("cache"),
    };
    let hash = "b".repeat(64);
    let brew = format!("/api/brew/image-cache/bb/{hash}.png");
    let phantasi = format!("/api/phantasi/image-cache/bb/{hash}.png");
    tokio::fs::create_dir_all(paths.cache_images.join("bb"))
        .await
        .unwrap();
    tokio::fs::write(paths.cache_images.join(format!("bb/{hash}.png")), png())
        .await
        .unwrap();
    // Old discovery committed both rows before the copy pass got stuck.
    for url in [&brew, &phantasi] {
        f.db.execute_raw(Statement::from_sql_and_values(DatabaseBackend::Postgres,
            "INSERT INTO media_assets(kind,url,mime,name,size) VALUES ('upload',$1,'image/png','cache.png',0)",
            [url.clone().into()])).await.unwrap();
    }
    let progress = drive_upgrade(&f, &paths, chrono::Utc::now().timestamp()).await;
    assert!(progress.complete, "{:?}", progress.error);
    let owner = resolve_asset_id(&f.db, &brew).await.unwrap().unwrap();
    assert_eq!(
        resolve_asset_id(&f.db, &phantasi).await.unwrap(),
        Some(owner)
    );
    for id in [1, 2] {
        let row = assets::find_by_id(&f.db, id).await.unwrap().unwrap();
        assert_eq!(row.state.as_deref(), Some("ready"));
        assert_eq!(
            tokio::fs::read(
                f.service
                    .store()
                    .final_path(row.storage_key.as_deref().unwrap())
                    .unwrap()
            )
            .await
            .unwrap(),
            png()
        );
    }
    // Old completed runs could have only the brew alias. Repair without reading
    // the legacy file again, even after that cache has been evicted.
    f.db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM media_url_aliases WHERE local_path = $1",
        [phantasi.clone().into()],
    ))
    .await
    .unwrap();
    tokio::fs::remove_dir_all(&paths.cache_images)
        .await
        .unwrap();
    upgrade::advance(&f.db, f.service.store(), &paths, &[], true)
        .await
        .unwrap();
    let repaired = drive_upgrade(&f, &paths, chrono::Utc::now().timestamp()).await;
    assert!(repaired.complete);
    assert_eq!(
        resolve_asset_id(&f.db, &phantasi).await.unwrap(),
        Some(owner)
    );
    assert!(matches!(
        resolve_alias_or_legacy(&f.db, f.service.store(), &paths, &phantasi, true)
            .await
            .unwrap(),
        ServeOutcome::File(_)
    ));
    f.close().await;
}

#[tokio::test]
async fn postgres_upgrade_sticker_failure_rolls_back_publication_and_retries() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let paths = LegacyPaths {
        federation_root: f.service.store().root().join("old"),
        cache_images: f.service.store().root().join("cache"),
    };
    let image = f.image().await;
    let layout =
        json!({"standard":[],"free":[{"type":"sticker","config":{"imageUrl":image.content_path}}]});
    f.db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO configurations(key,value,updated_at) VALUES ('dashboard_layout',$1,NOW())",
        [json!(layout.to_string()).into()],
    ))
    .await
    .unwrap();
    f.db.execute_unprepared("CREATE FUNCTION reject_upgrade_layout() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected layout update failure'; END $$; CREATE TRIGGER reject_upgrade_layout BEFORE UPDATE ON configurations FOR EACH ROW EXECUTE FUNCTION reject_upgrade_layout()").await.unwrap();
    let progress = drive_upgrade(&f, &paths, chrono::Utc::now().timestamp()).await;
    assert!(!progress.complete);
    assert_eq!(progress.pending_failures, 2);
    assert_eq!(
        assets::find_by_id(&f.db, image.id)
            .await
            .unwrap()
            .unwrap()
            .exposure
            .as_deref(),
        Some("private")
    );
    assert_eq!(active_count(&f.db, image.id).await.unwrap(), 0);
    let cfg =
        f.db.query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT value FROM configurations WHERE key='dashboard_layout'",
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        cfg.try_get::<serde_json::Value>("", "value").unwrap(),
        json!(layout.to_string())
    );
    f.db.execute_unprepared("DROP TRIGGER reject_upgrade_layout ON configurations")
        .await
        .unwrap();
    let repaired = drive_upgrade(&f, &paths, progress.next_retry_at.unwrap()).await;
    assert!(repaired.complete);
    assert_eq!(repaired.pending_failures, 0);
    assert_eq!(
        assets::find_by_id(&f.db, image.id)
            .await
            .unwrap()
            .unwrap()
            .exposure
            .as_deref(),
        Some("public")
    );
    assert!(active_count(&f.db, image.id).await.unwrap() > 0);
    f.close().await;
}

#[tokio::test]
async fn postgres_rss_shared_assets_are_protected_and_new_feed_writes_are_atomic() {
    use crate::models::entities::phantasi_items;
    use sea_orm::{ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter};
    let Some(f) = Fixture::new().await else {
        return;
    };
    let paths = LegacyPaths {
        federation_root: f.service.store().root().join("old"),
        cache_images: f.service.store().root().join("cache"),
    };
    tokio::fs::create_dir_all(paths.federation_root.join("1"))
        .await
        .unwrap();
    tokio::fs::write(paths.federation_root.join("1/shared.png"), png())
        .await
        .unwrap();
    f.db.execute_unprepared("INSERT INTO media_assets(kind,url,mime,name,size) VALUES ('upload','/media/federation/1/shared.png','image/png','shared.png',0);
        INSERT INTO phantasi_sources(id,user_id,name,url,feed_type) VALUES (1,1,'RSS','https://example.com/feed','rss');
        INSERT INTO phantasi_items(source_id,guid,title,link,published_at,fetched_at,content) VALUES (1,'old','old','https://example.com/old',NOW(),NOW(),'<img src=\"/media/federation/1/shared.png\">');
        INSERT INTO phantasi_note_docs(user_id,title,content_md) VALUES (1,'shared','![shared](/media/federation/1/shared.png)')").await.unwrap();
    let progress = drive_upgrade(&f, &paths, chrono::Utc::now().timestamp()).await;
    assert!(progress.complete, "{:?}", progress.error);
    assert_eq!(active_count(&f.db, 1).await.unwrap(), 2);
    let txn = f.db.begin().await.unwrap();
    bind_note_draft(&txn, 1, 0, None, "", &[]).await.unwrap();
    txn.commit().await.unwrap();
    assert_eq!(
        f.service.delete(&f.db, 1).await.unwrap_err(),
        MediaError::InUse
    );
    let item = |guid: &str| phantasi_items::ActiveModel {
        source_id: Set(1),
        guid: Set(guid.into()),
        title: Set(guid.into()),
        link: Set(format!("https://example.com/{guid}")),
        image: Set(Some("/media/federation/1/shared.png".into())),
        published_at: Set(chrono::Utc::now().fixed_offset()),
        fetched_at: Set(chrono::Utc::now().fixed_offset()),
        ..Default::default()
    };
    crate::services::phantasi_scheduler::insert_feed_items(&f.db, vec![item("new")])
        .await
        .unwrap();
    assert_eq!(active_count(&f.db, 1).await.unwrap(), 2);
    f.db.execute_unprepared("CREATE FUNCTION reject_rss_ref() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected rss bind failure'; END $$; CREATE TRIGGER reject_rss_ref BEFORE INSERT ON media_references FOR EACH ROW EXECUTE FUNCTION reject_rss_ref()").await.unwrap();
    assert!(
        crate::services::phantasi_scheduler::insert_feed_items(&f.db, vec![item("rollback")])
            .await
            .is_err()
    );
    assert!(
        phantasi_items::Entity::find()
            .filter(phantasi_items::Column::Guid.eq("rollback"))
            .one(&f.db)
            .await
            .unwrap()
            .is_none()
    );
    f.db.execute_unprepared("DROP TRIGGER reject_rss_ref ON media_references")
        .await
        .unwrap();
    let txn = f.db.begin().await.unwrap();
    clear_rss_source(&txn, 1).await.unwrap();
    txn.execute_unprepared("DELETE FROM phantasi_sources WHERE id=1")
        .await
        .unwrap();
    txn.commit().await.unwrap();
    assert_eq!(active_count(&f.db, 1).await.unwrap(), 0);
    f.service.delete(&f.db, 1).await.unwrap();
    f.close().await;
}

#[tokio::test]
async fn postgres_upgrade_defers_locked_note_and_binds_latest_edit() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let paths = LegacyPaths {
        federation_root: f.service.store().root().join("old"),
        cache_images: f.service.store().root().join("cache"),
    };
    tokio::fs::create_dir_all(paths.federation_root.join("1"))
        .await
        .unwrap();
    tokio::fs::write(paths.federation_root.join("1/old.png"), png())
        .await
        .unwrap();
    f.db.execute_unprepared("INSERT INTO media_assets(kind,url,mime,name,size) VALUES ('upload','/media/federation/1/old.png','image/png','old.png',0);
        INSERT INTO phantasi_note_docs(user_id,title,content_md) VALUES (1,'editable','![old](/media/federation/1/old.png)')").await.unwrap();
    let new_image = f.image().await;
    let now = chrono::Utc::now().timestamp();
    for _ in 0..40 {
        let p = upgrade::automatic_step(&f.db, f.service.store(), &paths, &[], now)
            .await
            .unwrap()
            .unwrap();
        if p.pass == 1 && p.phase == 1 {
            break;
        }
    }
    let edit = f.db.begin().await.unwrap();
    let body = format!("![new]({})", new_image.content_path);
    edit.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE phantasi_note_docs SET content_md=$1 WHERE id=1",
        [body.clone().into()],
    ))
    .await
    .unwrap();
    bind_note_draft(&edit, 1, 0, None, &body, &[])
        .await
        .unwrap();
    let p = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        upgrade::automatic_step(&f.db, f.service.store(), &paths, &[], now),
    )
    .await
    .unwrap()
    .unwrap()
    .unwrap();
    assert!(
        p.pending_failures > 0,
        "locked row must be queued, not silently skipped"
    );
    // History snapshots protect the prior image under the same transaction;
    // deletion must serialize with the edit rather than bypass its asset lock.
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_millis(100),
            f.service.delete(&f.db, 1)
        )
        .await
        .is_err()
    );
    edit.commit().await.unwrap();
    let p = drive_upgrade(&f, &paths, now).await;
    assert!(!p.complete);
    let p = drive_upgrade(&f, &paths, p.next_retry_at.unwrap()).await;
    assert!(p.complete, "{:?}", p.error);
    assert!(
        active_count(&f.db, 1).await.unwrap() > 0,
        "historical revision retains old image"
    );
    assert_eq!(active_count(&f.db, new_image.id).await.unwrap(), 1);
    assert_eq!(
        f.service.delete(&f.db, 1).await.unwrap_err(),
        MediaError::InUse
    );
    f.close().await;
}

#[tokio::test]
async fn postgres_retry_work_is_bounded_by_failures_not_history_size() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let paths = LegacyPaths {
        federation_root: f.service.store().root().join("old"),
        cache_images: f.service.store().root().join("cache"),
    };
    f.db.execute_unprepared("INSERT INTO phantasi_note_docs(user_id,title,content_md) SELECT 1,'healthy-'||n,'' FROM generate_series(1,1200) n;
        INSERT INTO media_assets(kind,url,mime,name,size) VALUES ('upload','/media/federation/1/missing.png','image/png','missing.png',0)").await.unwrap();
    let p = drive_upgrade(&f, &paths, chrono::Utc::now().timestamp()).await;
    assert_eq!(p.pending_failures, 1);
    assert!(p.retrying);
    let again = drive_upgrade(&f, &paths, p.next_retry_at.unwrap()).await;
    assert_eq!(
        again.scanned - p.scanned,
        1,
        "1200 healthy records must not be reprocessed"
    );
    assert_eq!(again.pending_failures, 1);
    // Deleted work items must retire instead of becoming a permanent failure.
    f.db.execute_unprepared("DELETE FROM media_assets WHERE id=1")
        .await
        .unwrap();
    let retired = drive_upgrade(&f, &paths, again.next_retry_at.unwrap()).await;
    assert!(retired.complete);
    f.close().await;
}

#[tokio::test]
async fn postgres_retry_discovers_new_copy_dependencies_after_edit() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let paths = LegacyPaths {
        federation_root: f.service.store().root().join("old"),
        cache_images: f.service.store().root().join("cache"),
    };
    f.db.execute_unprepared("INSERT INTO phantasi_note_docs(user_id,title,content_md) VALUES (1,'edit','![missing](/media/federation/1/first.png)')").await.unwrap();
    let p = drive_upgrade(&f, &paths, chrono::Utc::now().timestamp()).await;
    // A pending edit can reveal a new asset behind the original scan cursor.
    f.db.execute_unprepared("UPDATE phantasi_note_docs SET content_md='![new](/media/federation/1/second.png)' WHERE id=1").await.unwrap();
    tokio::fs::create_dir_all(paths.federation_root.join("1"))
        .await
        .unwrap();
    for name in ["first.png", "second.png"] {
        tokio::fs::write(paths.federation_root.join("1").join(name), png())
            .await
            .unwrap();
    }
    let repaired = drive_upgrade(&f, &paths, p.next_retry_at.unwrap()).await;
    assert!(repaired.complete, "{:?}", repaired.error);
    let second = resolve_asset_id(&f.db, "/media/federation/1/second.png")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(active_count(&f.db, second).await.unwrap(), 1);
    f.close().await;
}

// Executed only as a child of the crash test, with an isolated schema/root.
#[tokio::test]
#[ignore = "subprocess crash harness"]
async fn media_upgrade_crash_child() {
    let schema = std::env::var("MYRIAD_MEDIA_CRASH_SCHEMA").unwrap();
    let root = std::path::PathBuf::from(std::env::var("MYRIAD_MEDIA_CRASH_ROOT").unwrap());
    let mut options =
        sea_orm::ConnectOptions::new(std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL").unwrap());
    options
        .set_schema_search_path(schema)
        .max_connections(1)
        .sqlx_logging(false);
    let db = sea_orm::Database::connect(options).await.unwrap();
    let pid = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT pg_backend_pid() AS pid",
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<i32>("", "pid")
        .unwrap();
    tokio::fs::write(root.join("crash.pid"), pid.to_string())
        .await
        .unwrap();
    let store = MediaStore::new(root.clone());
    let paths = LegacyPaths {
        federation_root: root.join("old"),
        cache_images: root.join("cache"),
    };
    upgrade::automatic_step(&db, &store, &paths, &[], chrono::Utc::now().timestamp())
        .await
        .unwrap();
}

#[tokio::test]
async fn postgres_process_kill_after_copy_resumes_without_new_identity() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let root = f.service.store().root();
    let paths = LegacyPaths {
        federation_root: root.join("old"),
        cache_images: root.join("cache"),
    };
    tokio::fs::create_dir_all(paths.federation_root.join("1"))
        .await
        .unwrap();
    tokio::fs::write(paths.federation_root.join("1/crash.png"), png())
        .await
        .unwrap();
    f.db.execute_unprepared("INSERT INTO media_assets(kind,url,mime,name,size) VALUES ('upload','/media/federation/1/crash.png','image/png','crash.png',0)").await.unwrap();
    let now = chrono::Utc::now().timestamp();
    for _ in 0..30 {
        let p = upgrade::automatic_step(&f.db, f.service.store(), &paths, &[], now)
            .await
            .unwrap()
            .unwrap();
        if p.pass == 1 && p.phase == 0 {
            break;
        }
    }
    let row = assets::find_by_id(&f.db, 1).await.unwrap().unwrap();
    let public_id = row.public_id.unwrap();
    let key = storage_key(public_id, "png").unwrap();
    let before = upgrade::status(&f.db).await.unwrap();
    // Simulate a prior kill during the streaming copy. This token belongs only
    // to this migration; another writer's partial file must survive cleanup.
    f.service
        .store()
        .stage_bytes(public_id, b"incomplete copy")
        .await
        .unwrap();
    let other_token = Uuid::new_v4();
    f.service
        .store()
        .stage_bytes(other_token, b"another writer")
        .await
        .unwrap();
    f.db.execute_unprepared("CREATE FUNCTION pause_after_media_copy() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.state='ready' THEN PERFORM pg_sleep(3); END IF; RETURN NEW; END $$; CREATE TRIGGER pause_after_media_copy BEFORE UPDATE ON media_assets FOR EACH ROW EXECUTE FUNCTION pause_after_media_copy()").await.unwrap();
    let schema: String =
        f.db.query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT current_schema() AS schema",
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "schema")
        .unwrap();
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "services::media::integration_tests::media_upgrade_crash_child",
            "--ignored",
            "--nocapture",
        ])
        .env("MYRIAD_MEDIA_CRASH_SCHEMA", schema)
        .env("MYRIAD_MEDIA_CRASH_ROOT", root)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut paused = false;
    while std::time::Instant::now() < deadline {
        if let Ok(pid) = tokio::fs::read_to_string(root.join("crash.pid")).await {
            let pid: i32 = pid.parse().unwrap();
            let row = f.db.query_one_raw(Statement::from_sql_and_values(DatabaseBackend::Postgres,
                "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE pid=$1 AND wait_event='PgSleep') AS paused", [pid.into()])).await.unwrap().unwrap();
            if row.try_get::<bool>("", "paused").unwrap() {
                paused = true;
                break;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let _ = child.kill();
    child.wait().unwrap();
    assert!(
        paused,
        "child must reach the database switch after copying bytes"
    );
    // DDL waits for the killed connection's sleeping statement to finish and
    // roll back. No test rollback or cooperative cancellation in the child.
    f.db.execute_unprepared("DROP TRIGGER pause_after_media_copy ON media_assets")
        .await
        .unwrap();
    assert_eq!(
        tokio::fs::read(f.service.store().final_path(&key).unwrap())
            .await
            .unwrap(),
        png()
    );
    let row = assets::find_by_id(&f.db, 1).await.unwrap().unwrap();
    assert!(row.state.is_none());
    assert_eq!(row.public_id, Some(public_id));
    assert_eq!(
        upgrade::status(&f.db).await.unwrap().scanned,
        before.scanned
    );
    let recovered = drive_upgrade(&f, &paths, now).await;
    assert!(recovered.complete);
    let row = assets::find_by_id(&f.db, 1).await.unwrap().unwrap();
    assert_eq!(row.public_id, Some(public_id));
    assert_eq!(row.storage_key.as_deref(), Some(key.as_str()));
    assert!(!f.service.store().temp_exists(public_id).await);
    assert!(f.service.store().temp_exists(other_token).await);
    assert!(paths.federation_root.join("1/crash.png").exists());
    f.close().await;
}

#[tokio::test]
async fn postgres_upgrade_does_not_rebind_completed_concurrent_delivery() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let paths = LegacyPaths {
        federation_root: f.service.store().root().join("old"),
        cache_images: f.service.store().root().join("cache"),
    };
    let image = f.image().await;
    let token = Uuid::new_v4();
    f.db.execute_raw(Statement::from_sql_and_values(DatabaseBackend::Postgres,
        "INSERT INTO federation_activities(id,activity_id,user_id,activity_type,object_json,is_local,published_at) VALUES (1,'https://example.com/activities/1',1,'Create',$1,TRUE,NOW())",
        [json!({"attachment":[{"url":image.content_path}]}).into()])).await.unwrap();
    f.db.execute_raw(Statement::from_sql_and_values(DatabaseBackend::Postgres,
        "INSERT INTO federation_delivery_queue(id,activity_id,target_inbox,target_domain,status,attempts,max_attempts,lease_token,created_at) VALUES (1,1,'https://peer.test/inbox','peer.test','delivering',1,3,$1,NOW())", [token.into()])).await.unwrap();
    let now = chrono::Utc::now().timestamp();
    for _ in 0..40 {
        let p = upgrade::automatic_step(&f.db, f.service.store(), &paths, &[], now)
            .await
            .unwrap()
            .unwrap();
        if p.pass == 2 && p.phase == 7 {
            break;
        }
    }
    let delivery = f.db.begin().await.unwrap();
    delivery
        .execute_unprepared("SELECT id FROM federation_delivery_queue WHERE id=1 FOR UPDATE")
        .await
        .unwrap();
    let p = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        upgrade::automatic_step(&f.db, f.service.store(), &paths, &[], now),
    )
    .await
    .unwrap()
    .unwrap()
    .unwrap();
    assert!(p.pending_failures > 0);
    assert!(
        crate::federation::delivery::mark_delivery_delivered_if_owned(&delivery, 1, token)
            .await
            .unwrap()
    );
    delivery.commit().await.unwrap();
    let p = drive_upgrade(&f, &paths, now).await;
    let done = drive_upgrade(&f, &paths, p.next_retry_at.unwrap()).await;
    assert!(done.complete);
    let row = f.db.query_one_raw(Statement::from_string(DatabaseBackend::Postgres,"SELECT count(*)::bigint AS n FROM media_references WHERE consumer_type='federation_outbox' AND consumer_id='1'")).await.unwrap().unwrap();
    assert_eq!(
        row.try_get::<i64>("", "n").unwrap(),
        0,
        "delivered rows must leave the deferred queue without stale references"
    );
    assert_eq!(
        active_count(&f.db, image.id).await.unwrap(),
        1,
        "activity reference still protects the attachment"
    );
    f.close().await;
}

#[tokio::test]
async fn postgres_catalog_paginates_filters_and_reads_only_live_reference_labels() {
    use crate::services::media_catalog::{MediaListQuery, list_assets};
    let Some(f) = Fixture::new().await else {
        return;
    };
    f.db.execute_unprepared(
        r#"
INSERT INTO media_assets (id, kind, url, mime, name, created_at, state)
SELECT n, CASE WHEN n % 2 = 0 THEN 'generated' ELSE 'upload' END,
'/test/' || n, CASE WHEN n % 2 = 0 THEN 'image/png' ELSE 'application/octet-stream' END,
CASE WHEN n % 2 = 0 THEN 'Sky 100%_' || n || '.png' ELSE 'Photo-' || n || '.JPG' END,
'2026-09-22 01:02:03.123456+00'::timestamptz, 'ready' FROM generate_series(1, 105) n;
INSERT INTO media_assets (id, kind, url, mime, name, state) VALUES
(106,'upload','/staging','image/png','staging','staging'),
(107,'upload','/deleted','image/png','deleted','deleted'),
(108,'upload','/deleting','image/png','deleting','deleting');
INSERT INTO media_references (asset_id, consumer_type, consumer_id, slot, expires_at) VALUES
(105,'note_draft','1','a',NULL), (105,'note_draft','2','b',NULL),
(105,'note_history','3','c',NOW() - interval '1 second'),
(105,'rss_item','4','d',NOW() + interval '1 day'),
(105,'sticker','5','e',NOW() - interval '1 second');
"#,
    )
    .await
    .unwrap();
    let first = list_assets(&f.db, &MediaListQuery::default())
        .await
        .unwrap();
    assert_eq!(first.total, Some(105));
    assert_eq!(first.items.len(), 48);
    assert_eq!(first.items[0].id, 105);
    let mut labels = first.items[0].references.clone();
    labels.sort();
    assert_eq!(labels, ["articles", "notes"]);
    let mut ids: Vec<_> = first.items.iter().map(|a| a.id).collect();
    let mut cursor = first.next_cursor;
    while let Some(next) = cursor {
        let page = list_assets(
            &f.db,
            &MediaListQuery {
                before_created_at: Some(next.created_at),
                before_id: Some(next.id),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(page.total, None);
        ids.extend(page.items.iter().map(|a| a.id));
        cursor = page.next_cursor;
    }
    assert_eq!(ids, (1..=105).rev().collect::<Vec<_>>());
    let filtered = list_assets(
        &f.db,
        &MediaListQuery {
            kind: Some("generated".into()),
            format: Some("png".into()),
            query: Some("SKY 100%_".into()),
            limit: Some(100),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(filtered.total, Some(52));
    assert_eq!(filtered.items.len(), 52);
    assert!(filtered.items.iter().all(|a| a.id % 2 == 0));
    let jpeg = list_assets(
        &f.db,
        &MediaListQuery {
            format: Some("jpeg".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(jpeg.total, Some(53));
    let empty = list_assets(
        &f.db,
        &MediaListQuery {
            query: Some("100%Z".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(empty.total, Some(0));
    assert!(empty.items.is_empty());
    assert!(empty.next_cursor.is_none());
    f.close().await;
}

#[tokio::test]
async fn postgres_report_catalog_projects_summary_and_preserves_full_detail() {
    use crate::services::tapp_reports::*;
    let Some(f) = Fixture::new().await else {
        return;
    };
    f.db.execute_unprepared(r#"
INSERT INTO platform_reports (user_id, platform, metadata, report, created_at) VALUES
(1, 'steam', '{}', json_build_object('summary', 'Latest', 'card_visuals', repeat('x', 100000)), '2026-09-22'),
(1, 'github', '{}', '{"summary":null}', '2026-09-21'),
(2, 'steam', '{}', '{"summary":"Another user"}', '2026-09-23');
"#).await.unwrap();
    let rows = list_user_platform_reports(&f.db, 1).await.unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].summary, "Latest");
    assert_eq!(rows[1].summary, "");
    let compact = platform_report_list_item(&rows[0]);
    assert!(compact.to_string().len() < 200);
    let full = get_user_platform_report(&f.db, 1, rows[0].id)
        .await
        .unwrap();
    let payload = platform_report_payload(&full);
    assert_eq!(
        payload["content"]["card_visuals"].as_str().unwrap().len(),
        100000
    );
    assert!(payload.get("card_visuals").is_none());
    assert!(
        get_user_platform_report(&f.db, 2, rows[0].id)
            .await
            .is_err()
    );
    f.close().await;
}

#[tokio::test]
async fn postgres_recent_activity_uses_history_projection_without_snapshots() {
    use crate::api::profile::{ActivityQuery, get_recent_activities};
    use axum::extract::{Query, State};
    let Some(f) = Fixture::new().await else {
        return;
    };
    f.db.execute_unprepared(r#"
UPDATE users SET is_owner = TRUE WHERE id = 1;
INSERT INTO metadata_history (user_id, platform_name, changed_fields, old_data, new_data, change_date) VALUES
(1, 'steam', '["games", "playtime"]', json_build_object('large', repeat('x', 100000)), '{}', '2026-09-22'),
(1, 'steam', '["games"]', '{}', '{}', '2026-09-22'),
(2, 'github', '["repos"]', '{}', '{}', '2026-09-23');
ALTER TABLE metadata_history DROP COLUMN old_data, DROP COLUMN new_data;
"#).await.unwrap();
    // Removing the unused columns makes an accidental full-entity SELECT fail.
    let (status, axum::Json(body)) = get_recent_activities(
        Query(ActivityQuery { limit: Some(10) }),
        State(f.db.clone()),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(body["count"], 1);
    assert_eq!(body["activities"][0]["platform_name"], "steam");
    assert_eq!(body["activities"][0]["change_count"], 3);
    f.close().await;
}

#[tokio::test]
async fn wallpaper_publication_and_references_commit_together() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let image = f.image().await;
    let txn = f.db.begin().await.unwrap();
    let url = bind_and_publish_wallpaper(&txn, &image.content_path, &[])
        .await
        .unwrap();
    assert!(url.starts_with("/media/assets/"));
    txn.rollback().await.unwrap();
    let row = assets::find_by_id(&f.db, image.id).await.unwrap().unwrap();
    assert_eq!(row.exposure.as_deref(), Some("private"));
    let txn = f.db.begin().await.unwrap();
    let url = bind_and_publish_wallpaper(&txn, &image.content_path, &[])
        .await
        .unwrap();
    txn.commit().await.unwrap();
    let filename = url.rsplit('/').next().unwrap();
    let outcome = resolve_public_asset(&f.db, f.service.store(), image.public_id, filename)
        .await
        .unwrap();
    let ServeOutcome::File(file) = outcome else {
        panic!("published wallpaper must be readable")
    };
    assert_eq!(tokio::fs::read(file.path).await.unwrap(), png());
    assert!(matches!(
        f.service.delete(&f.db, image.id).await,
        Err(MediaError::InUse)
    ));
    let txn = f.db.begin().await.unwrap();
    bind_and_publish_wallpaper(&txn, "", &[]).await.unwrap();
    txn.commit().await.unwrap();
    assert_eq!(
        f.service.delete(&f.db, image.id).await.unwrap(),
        DeleteOutcome::Deleted
    );
    f.close().await;
}

#[tokio::test]
async fn generated_portrait_and_sticker_urls_serve_real_bytes_after_publication() {
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    let Some(f) = Fixture::new().await else {
        return;
    };
    for filename in ["portrait.png", "sticker.png", "note.png"] {
        let (image, _) = f
            .service
            .persist_ready_bytes(
                &f.db,
                MediaContext::site(MediaActor::admin(1).unwrap(), MediaSource::Generated),
                NewMediaBytes {
                    bytes: png().into(),
                    claimed_mime: "image/png".into(),
                    filename: filename.into(),
                    max_bytes: 1024 * 1024,
                    derived_from_id: None,
                    exposure: MediaExposure::Private,
                },
            )
            .await
            .unwrap();
        let txn = f.db.begin().await.unwrap();
        let public = publish_local_url(&txn, &image.catalog_url(), &[])
            .await
            .unwrap();
        txn.commit().await.unwrap();
        let catalog = crate::services::media_catalog::list_assets(&f.db, &Default::default())
            .await
            .unwrap();
        let item = catalog
            .items
            .iter()
            .find(|item| item.id == image.id)
            .unwrap();
        assert_eq!(item.public_path.as_deref(), Some(public.as_str()));
        let outcome = resolve_public_asset(
            &f.db,
            f.service.store(),
            image.public_id,
            public.rsplit('/').next().unwrap(),
        )
        .await
        .unwrap();
        let response = crate::api::media_public::send_media_outcome(
            Request::builder().uri(&public).body(Body::empty()).unwrap(),
            outcome,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["content-type"], "image/png");
        assert_eq!(
            &to_bytes(response.into_body(), 1024 * 1024).await.unwrap()[..],
            png()
        );
    }
    f.close().await;
}
