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
        "SELECT (SELECT COUNT(*) FROM runtime_registry)::bigint AS tasks, (SELECT COUNT(*) FROM runtime_mailbox)::bigint AS messages")).await.unwrap().unwrap();
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
    let (_, body) = normalize_cited_media(
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
    let portrait_url = normalize_local_url(&txn, &portrait.content_path, &[])
        .await
        .unwrap();
    let avatar_url = normalize_local_url(&txn, &avatar.content_path, &[])
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
        subject: crate::middleware::auth::AuthSubject::from_test_sub(&user_id.to_string()),
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
async fn postgres_rss_shared_assets_are_protected_and_new_feed_writes_are_atomic() {
    use crate::models::entities::phantasi_items;
    use sea_orm::{ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter};
    let Some(f) = Fixture::new().await else {
        return;
    };
    let image = f.image().await;
    let shared = f
        .service
        .publish(&f.db, image.id)
        .await
        .unwrap()
        .public_path
        .unwrap();
    f.db.execute_unprepared("INSERT INTO phantasi_sources(id,user_id,name,url,feed_type) VALUES (1,1,'RSS','https://example.com/feed','rss')").await.unwrap();
    let item = |guid: &str| phantasi_items::ActiveModel {
        source_id: Set(1),
        guid: Set(guid.into()),
        title: Set(guid.into()),
        link: Set(format!("https://example.com/{guid}")),
        image: Set(Some(shared.clone())),
        published_at: Set(chrono::Utc::now().fixed_offset()),
        fetched_at: Set(chrono::Utc::now().fixed_offset()),
        ..Default::default()
    };
    crate::services::phantasi_scheduler::insert_feed_items(&f.db, vec![item("old")])
        .await
        .unwrap();
    assert_eq!(active_count(&f.db, image.id).await.unwrap(), 1);
    assert_eq!(
        f.service.delete(&f.db, image.id).await.unwrap_err(),
        MediaError::InUse
    );
    crate::services::phantasi_scheduler::insert_feed_items(&f.db, vec![item("new")])
        .await
        .unwrap();
    assert_eq!(active_count(&f.db, image.id).await.unwrap(), 2);
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
    assert_eq!(active_count(&f.db, image.id).await.unwrap(), 0);
    f.service.delete(&f.db, image.id).await.unwrap();
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
INSERT INTO users (id, username) VALUES (2, 'report-other');
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
async fn postgres_recent_activity_reads_events_including_older_history() {
    use crate::api::profile::{ActivityQuery, get_recent_activities};
    use axum::extract::{Query, State};
    let Some(f) = Fixture::new().await else {
        return;
    };
    f.db.execute_unprepared(r#"
UPDATE users SET is_owner = TRUE WHERE id = 1;
INSERT INTO users (id, username) VALUES (2, 'history-other');
INSERT INTO metadata_history (user_id, platform_name, changed_fields, old_data, new_data, change_date) VALUES
(1, 'steam', '["games", "playtime"]', json_build_object('large', repeat('x', 100000)), '{}', '2026-09-22 10:00'),
(1, 'steam', '["games"]', '{}', '{}', '2026-09-22 09:00'),
(2, 'github', '["repos"]', '{}', '{}', '2026-09-23'),
(1, 'github', '["repos"]', '{}', '{}', '2026-09-24');
INSERT INTO activity_events (metadata_history_id, user_id, platform_name, event_type, title, changes,
    change_count, importance, occurred_at, created_at)
SELECT id, 1, 'github', 'suppressed', 'GitHub', '[]', 1, 0, change_date, NOW()
FROM metadata_history WHERE user_id = 1 AND platform_name = 'github';
ALTER TABLE metadata_history DROP COLUMN old_data, DROP COLUMN new_data;
"#).await.unwrap();
    // History from before activity events gets one event each; removing the
    // snapshot columns makes an accidental full-row read fail.
    crate::db::schema_check::rewrite_old_rows_for_test(&f.db)
        .await
        .unwrap();
    let (status, axum::Json(body)) = get_recent_activities(
        Query(ActivityQuery { limit: Some(10) }),
        State(f.db.clone()),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(body["count"], 2, "{body}");
    assert_eq!(body["activities"][0]["platform_name"], "steam");
    assert_eq!(body["activities"][0]["title"], "Steam");
    assert_eq!(body["activities"][0]["change_count"], 2);
    assert_eq!(body["activities"][1]["change_count"], 1);
    assert_eq!(
        body["activities"][0]["changes"][0]["metric"],
        "data_changes"
    );
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
        let public = f.service.publish(&f.db, image.id).await.unwrap().url;
        assert_eq!(public, image.catalog_url(), "publishing keeps the address");
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

async fn wallpaper_references(f: &Fixture) -> Vec<i32> {
    f.db.query_all_raw(Statement::from_string(
        DatabaseBackend::Postgres,
        "SELECT asset_id FROM media_references WHERE consumer_type = 'site_wallpaper' AND consumer_id = 'site' ORDER BY asset_id",
    ))
    .await
    .unwrap()
    .iter()
    .map(|row| row.try_get::<i32>("", "asset_id").unwrap())
    .collect()
}

#[tokio::test]
async fn wallpaper_saved_under_previous_origin_binds_only_local_media() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let current = ["https://new.example".to_string()];
    let image = f.image().await;
    let txn = f.db.begin().await.unwrap();
    let stored = bind_and_publish_wallpaper(
        &txn,
        &format!("https://old.example{}", image.content_path),
        &current,
    )
    .await
    .unwrap();
    txn.commit().await.unwrap();
    // The stale origin is dropped and the wallpaper is published and protected.
    assert!(stored.starts_with("/media/assets/"), "{stored}");
    assert_eq!(wallpaper_references(&f).await, vec![image.id]);
    assert!(matches!(
        f.service.delete(&f.db, image.id).await,
        Err(MediaError::InUse)
    ));
    // Re-saving the public URL under the old origin keeps the same binding.
    let txn = f.db.begin().await.unwrap();
    let again = bind_and_publish_wallpaper(&txn, &format!("https://old.example{stored}"), &current)
        .await
        .unwrap();
    txn.commit().await.unwrap();
    assert_eq!(again, stored);
    assert_eq!(wallpaper_references(&f).await, vec![image.id]);
    // Media-shaped URLs that are not media of this instance stay external and
    // must neither fail the save nor bind an unrelated asset.
    for external in [
        format!(
            "https://other.example/media/assets/{}/a.png",
            Uuid::new_v4()
        ),
        "https://other.example/media/federation/9/remote.png".to_string(),
        "https://other.example/api/media/2147483000/content".to_string(),
        "https://cdn.example/wallpaper.png".to_string(),
    ] {
        let txn = f.db.begin().await.unwrap();
        let kept = bind_and_publish_wallpaper(&txn, &external, &current)
            .await
            .unwrap();
        txn.commit().await.unwrap();
        assert_eq!(kept, external);
        assert!(wallpaper_references(&f).await.is_empty(), "{external}");
    }
    f.close().await;
}

/// Installation-shared storage lives under the owner, but a member writes it
/// with the image they generated: that image is theirs to protect.
#[tokio::test]
async fn postgres_storage_binds_media_as_its_writer() {
    use crate::services::tapp_storage::{record_storage_media, write_storage_value_as};
    let Some(f) = Fixture::new().await else {
        return;
    };
    f.db.execute_unprepared("INSERT INTO users (id, username) VALUES (2, 'media-member')")
        .await
        .unwrap();
    let image = f
        .service
        .create_from_bytes(
            &f.db,
            MediaContext::user(MediaActor::user(2).unwrap(), MediaSource::Generated).unwrap(),
            NewMediaBytes {
                bytes: png().into(),
                claimed_mime: "image/png".into(),
                filename: "generated".into(),
                max_bytes: 1024 * 1024,
                derived_from_id: None,
                exposure: MediaExposure::Public,
            },
        )
        .await
        .unwrap();
    let value = json!({ "image": image.url });
    write_storage_value_as(&f.db, 1, Some(1), "app", "_shared.a", value.clone())
        .await
        .unwrap();
    assert_eq!(references::active_count(&f.db, image.id).await.unwrap(), 0);
    write_storage_value_as(&f.db, 1, Some(2), "app", "_shared.b", value.clone())
        .await
        .unwrap();
    assert_eq!(references::active_count(&f.db, image.id).await.unwrap(), 1);

    // Direct writers record what the row holds when they call, not a copy.
    f.db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO tapp_storage (tapp_id, user_id, key, value, created_at, updated_at)
         VALUES ('app', 2, 'report', $1, NOW(), NOW())",
        [value.into()],
    ))
    .await
    .unwrap();
    let row =
        f.db.query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT id FROM tapp_storage WHERE key = 'report'",
        ))
        .await
        .unwrap()
        .unwrap();
    record_storage_media(&f.db, row.try_get("", "id").unwrap(), Some(2)).await;
    assert_eq!(references::active_count(&f.db, image.id).await.unwrap(), 2);
    f.close().await;
}

fn restored_entry(key: &str, value: serde_json::Value) -> crate::api::config::SettingsBackupEntry {
    crate::api::config::SettingsBackupEntry {
        key: key.into(),
        value,
        schema_version: 1,
        description: None,
        category: Some("general".into()),
        is_encrypted: Some(false),
        is_public: Some(false),
    }
}

async fn stored_config(f: &Fixture, key: &str) -> Option<serde_json::Value> {
    f.db.query_one_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT value FROM configurations WHERE key = $1",
        [key.into()],
    ))
    .await
    .unwrap()
    .map(|row| row.try_get::<serde_json::Value>("", "value").unwrap())
}

#[tokio::test]
async fn settings_restore_rebinds_wallpaper_and_stickers_in_the_config_transaction() {
    use crate::api::config::{
        RestoreWriteError, SETTINGS_BACKUP_FORMAT, SETTINGS_BACKUP_VERSION, SettingsBackup,
        UnresolvedRestoredMedia, build_settings_restore_plan, write_restored_configurations,
    };
    let Some(f) = Fixture::new().await else {
        return;
    };
    let wallpaper = f.image().await;
    let sticker = f.image().await;
    let dead_sticker = format!("/media/assets/{}/gone.png", Uuid::new_v4());
    let layout = json!({"standard":[],"free":[
        {"type":"sticker","config":{"imageUrl":sticker.content_path}},
        {"type":"sticker","config":{"imageUrl":dead_sticker}}
    ]});
    let txn = f.db.begin().await.unwrap();
    let unresolved = write_restored_configurations(
        &txn,
        vec![
            restored_entry("ui_wallpaper_url", json!(wallpaper.content_path)),
            restored_entry("dashboard_layout", json!(layout.to_string())),
        ],
        &[],
    )
    .await
    .ok()
    .unwrap();
    txn.commit().await.unwrap();
    assert_eq!(
        unresolved,
        vec![UnresolvedRestoredMedia {
            setting: "dashboard_layout".into(),
            url: dead_sticker.clone(),
        }]
    );
    // Stored as a save would: published, path-only public URLs. The dead
    // sticker is kept as it was in the backup.
    let stored_wallpaper = stored_config(&f, "ui_wallpaper_url").await.unwrap();
    assert!(
        stored_wallpaper
            .as_str()
            .is_some_and(|url| url.starts_with("/media/assets/")),
        "{stored_wallpaper}"
    );
    let stored_layout = stored_config(&f, "dashboard_layout").await.unwrap();
    let stored_layout = stored_layout.as_str().unwrap();
    assert!(
        !stored_layout.contains(&sticker.content_path),
        "{stored_layout}"
    );
    assert!(stored_layout.contains(&dead_sticker), "{stored_layout}");
    assert_eq!(wallpaper_references(&f).await, vec![wallpaper.id]);
    assert_eq!(active_count(&f.db, sticker.id).await.unwrap(), 1);
    for id in [wallpaper.id, sticker.id] {
        assert!(matches!(
            f.service.delete(&f.db, id).await,
            Err(MediaError::InUse)
        ));
    }

    // A wallpaper whose local media does not exist here does not fail the
    // restore: it is kept, left unbound and reported.
    let dead_wallpaper = format!("/media/assets/{}/wall-gone.png", Uuid::new_v4());
    let txn = f.db.begin().await.unwrap();
    let unresolved = write_restored_configurations(
        &txn,
        vec![
            restored_entry("restore_probe", json!("written")),
            restored_entry("ui_wallpaper_url", json!(dead_wallpaper)),
        ],
        &[],
    )
    .await
    .ok()
    .unwrap();
    txn.commit().await.unwrap();
    assert_eq!(
        unresolved,
        vec![UnresolvedRestoredMedia {
            setting: "ui_wallpaper_url".into(),
            url: dead_wallpaper.clone(),
        }]
    );
    assert_eq!(
        stored_config(&f, "restore_probe").await,
        Some(json!("written"))
    );
    assert_eq!(
        stored_config(&f, "ui_wallpaper_url").await,
        Some(json!(dead_wallpaper))
    );
    assert!(wallpaper_references(&f).await.is_empty());
    assert!(
        resolve_asset_id(&f.db, &dead_wallpaper)
            .await
            .unwrap()
            .is_none()
    );

    // A backup from before the asset store: its old address is reported too.
    let retired_wallpaper = "/media/federation/1/wall.png";
    let txn = f.db.begin().await.unwrap();
    let unresolved = write_restored_configurations(
        &txn,
        vec![restored_entry("ui_wallpaper_url", json!(retired_wallpaper))],
        &[],
    )
    .await
    .ok()
    .unwrap();
    // Not kept: the cases below start from the wallpaper restored above.
    txn.rollback().await.unwrap();
    assert_eq!(
        unresolved,
        vec![UnresolvedRestoredMedia {
            setting: "ui_wallpaper_url".into(),
            url: retired_wallpaper.into(),
        }]
    );
    assert!(wallpaper_references(&f).await.is_empty());

    // A wallpaper that saving would reject (unsafe scheme, private host) is
    // marked invalid by the restore plan and skipped: the current wallpaper
    // stays, nothing is published or bound, the rest of the backup restores.
    for unsafe_wallpaper in [
        "javascript:alert(1)",
        "data:image/png;base64,aaa",
        "http://127.0.0.1/wall.png",
    ] {
        let plan = build_settings_restore_plan(&SettingsBackup {
            format: SETTINGS_BACKUP_FORMAT.into(),
            version: SETTINGS_BACKUP_VERSION,
            product_version: None,
            exported_at: "2026-01-01T00:00:00Z".into(),
            contains_secrets: true,
            configurations: vec![
                restored_entry("restore_url_probe", json!(unsafe_wallpaper)),
                restored_entry("ui_wallpaper_url", json!(unsafe_wallpaper)),
            ],
            effective_config: Default::default(),
            user_preferences: Default::default(),
        });
        assert_eq!(plan.preview.invalid_keys, vec!["ui_wallpaper_url"]);
        let txn = f.db.begin().await.unwrap();
        let unresolved = write_restored_configurations(&txn, plan.entries, &[])
            .await
            .unwrap();
        txn.commit().await.unwrap();
        assert!(unresolved.is_empty());
        assert_eq!(
            stored_config(&f, "restore_url_probe").await,
            Some(json!(unsafe_wallpaper))
        );
        assert_eq!(
            stored_config(&f, "ui_wallpaper_url").await,
            Some(json!(dead_wallpaper))
        );
        assert!(wallpaper_references(&f).await.is_empty());
    }

    // Media that is catalogued here but not ready is not dead: the restore is
    // rejected like saving it would be, and nothing from the backup is written.
    let late_id = Uuid::new_v4();
    let late = format!("/media/assets/{late_id}/late.png");
    f.db.execute_unprepared(&format!("INSERT INTO media_assets(kind,url,mime,name,size,public_id,state,scope,source,exposure) VALUES ('upload','{late}','image/png','late.png',0,'{late_id}','missing','site','upload','private')")).await.unwrap();
    let txn = f.db.begin().await.unwrap();
    let result = write_restored_configurations(
        &txn,
        vec![
            restored_entry("restore_probe", json!("rolled back")),
            restored_entry("ui_wallpaper_url", json!(late)),
        ],
        &[],
    )
    .await;
    assert!(matches!(
        result,
        Err(RestoreWriteError::Media(MediaError::NotReady))
    ));
    txn.rollback().await.unwrap();
    assert_eq!(
        stored_config(&f, "restore_probe").await,
        Some(json!("written"))
    );
    assert_eq!(
        stored_config(&f, "ui_wallpaper_url").await,
        Some(json!(dead_wallpaper))
    );

    // Restoring an external wallpaper and an unset layout releases the assets.
    let txn = f.db.begin().await.unwrap();
    let unresolved = write_restored_configurations(
        &txn,
        vec![
            restored_entry("ui_wallpaper_url", json!("https://cdn.example/wall.png")),
            restored_entry("dashboard_layout", serde_json::Value::Null),
        ],
        &[],
    )
    .await
    .ok()
    .unwrap();
    txn.commit().await.unwrap();
    assert!(unresolved.is_empty());
    assert!(wallpaper_references(&f).await.is_empty());
    assert_eq!(active_count(&f.db, sticker.id).await.unwrap(), 0);
    assert_eq!(
        stored_config(&f, "ui_wallpaper_url").await,
        Some(json!("https://cdn.example/wall.png"))
    );
    f.close().await;
}

#[tokio::test]
async fn dashboard_save_protects_sticker_urls_under_the_site_origin() {
    use crate::api::config::{DashboardConfigPayload, save_dashboard_config};
    let Some(f) = Fixture::new().await else {
        return;
    };
    let origins = ["https://site.example".to_string()];
    let sticker = f.image().await;
    let absolute = format!("https://site.example{}", sticker.content_path);
    let payload = || DashboardConfigPayload {
        layout: Some(
            json!({"standard":[],"free":[{"type":"sticker","config":{"imageUrl":absolute}}]})
                .to_string(),
        ),
        layout_mode: None,
        title: None,
        custom_platforms: None,
        title_font: None,
        title_font_size: None,
        title_color: None,
        widget_theme: None,
    };
    // Without the site origin the absolute URL reads as external and escapes
    // deletion protection; this is what the handler used to pass.
    let (status, _) = save_dashboard_config(&f.db, payload(), &[]).await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(active_count(&f.db, sticker.id).await.unwrap(), 0);
    let (status, body) = save_dashboard_config(&f.db, payload(), &origins).await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(active_count(&f.db, sticker.id).await.unwrap(), 1);
    assert!(matches!(
        f.service.delete(&f.db, sticker.id).await,
        Err(MediaError::InUse)
    ));
    let saved = body.0["layout"].as_str().unwrap().to_string();
    assert!(!saved.contains("https://site.example"), "{saved}");
    assert!(saved.contains("/media/assets/"), "{saved}");
    assert_eq!(
        stored_config(&f, "dashboard_layout").await,
        Some(json!(saved))
    );
    f.close().await;
}

#[tokio::test]
async fn postgres_producer_key_is_released_after_delete_and_missing() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let ctx = || {
        MediaContext::site(MediaActor::admin(1).unwrap(), MediaSource::Generated)
            .with_producer_key("ai-task:t1:image")
    };
    let bytes = || NewMediaBytes {
        bytes: png().into(),
        claimed_mime: "image/png".into(),
        filename: "generated.png".into(),
        max_bytes: 1024 * 1024,
        derived_from_id: None,
        exposure: MediaExposure::Public,
    };
    let (first, created) = f
        .service
        .persist_ready_bytes(&f.db, ctx(), bytes())
        .await
        .unwrap();
    assert!(created);
    let (again, created) = f
        .service
        .persist_ready_bytes(&f.db, ctx(), bytes())
        .await
        .unwrap();
    assert!(!created);
    assert_eq!(again.id, first.id);
    f.service.delete(&f.db, first.id).await.unwrap();
    let (second, created) = f
        .service
        .persist_ready_bytes(&f.db, ctx(), bytes())
        .await
        .unwrap();
    assert!(created, "a deleted asset must not burn its producer key");
    assert_ne!(second.id, first.id);
    f.db.execute_unprepared(&format!(
        "UPDATE media_assets SET state = 'missing' WHERE id = {}",
        second.id
    ))
    .await
    .unwrap();
    let third = f
        .service
        .create_from_bytes(&f.db, ctx(), bytes())
        .await
        .unwrap();
    assert_ne!(third.id, second.id);
    f.close().await;
}

#[tokio::test]
async fn postgres_publishing_ids_requires_managing_private_assets() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    f.db.execute_unprepared("INSERT INTO users (id, username) VALUES (2, 'media-other')")
        .await
        .unwrap();
    let foreign = f
        .service
        .create_from_bytes(
            &f.db,
            MediaContext::user(MediaActor::user(2).unwrap(), MediaSource::Upload).unwrap(),
            NewMediaBytes {
                bytes: png().into(),
                claimed_mime: "image/png".into(),
                filename: "private.png".into(),
                max_bytes: 1024 * 1024,
                derived_from_id: None,
                exposure: MediaExposure::Private,
            },
        )
        .await
        .unwrap();
    // Request-supplied citations on a public consumer: another user's draft
    // reads as missing; the owner or an admin publishes it by citing it.
    let citations = Citations::urls(&[], &[foreign.url.clone()], |i| format!("a:{i}"));
    let cite_as = |actor: MediaActor| {
        let citations = citations.clone();
        let db = f.db.clone();
        async move {
            bind(
                &db,
                &Consumer::federation_activity("act"),
                &citations,
                Authority::Actor(&actor),
                Unresolved::Reject,
            )
            .await
        }
    };
    let user = MediaActor::user(1).unwrap();
    assert_eq!(
        cite_as(user.clone()).await.unwrap_err(),
        MediaError::Missing
    );
    let row = assets::find_by_id(&f.db, foreign.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.exposure.as_deref(), Some("private"));
    cite_as(MediaActor::user(2).unwrap()).await.unwrap();
    let row = assets::find_by_id(&f.db, foreign.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.exposure.as_deref(), Some("public"));
    // Already public: citing it needs no further right.
    cite_as(user).await.unwrap();
    f.close().await;
}

#[tokio::test]
async fn postgres_note_history_skips_dead_media_the_author_removed() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let live = f.image().await;
    let dead = format!("/api/phantasi/image-cache/ab/ab{}.png", "0".repeat(62));
    let body = format!("![live]({}) ![dead]({dead})", live.content_path);
    let doc: i32 = f
        .db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO phantasi_note_docs(user_id, title, content_md) VALUES (1, 'n', $1) RETURNING id",
            [body.into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "id")
        .unwrap();
    // The author removes the dead image; the trigger snapshots the old body.
    f.db.execute_unprepared(&format!(
        "UPDATE phantasi_note_docs SET content_md = 'clean', revision = 2 WHERE id = {doc}"
    ))
    .await
    .unwrap();
    cite::sync_note_history_refs(&f.db, doc, 0, &[])
        .await
        .unwrap();
    let refs = references::active_count(&f.db, live.id).await.unwrap();
    assert_eq!(refs, 1, "live media in history stays protected");
    f.close().await;
}

#[tokio::test]
async fn postgres_message_payload_binds_only_the_senders_media() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    f.db.execute_unprepared("INSERT INTO users (id, username) VALUES (2, 'media-other')")
        .await
        .unwrap();
    let upload = |owner: i32| {
        f.service.create_from_bytes(
            &f.db,
            MediaContext::user(MediaActor::user(owner).unwrap(), MediaSource::Channel).unwrap(),
            NewMediaBytes {
                bytes: png().into(),
                claimed_mime: "image/png".into(),
                filename: "inbound.png".into(),
                max_bytes: 1024 * 1024,
                derived_from_id: None,
                exposure: MediaExposure::Private,
            },
        )
    };
    let own = upload(1).await.unwrap();
    let foreign = upload(2).await.unwrap();
    let payload = json!({ "attachments": [
        { "url": own.content_path },
        { "url": foreign.content_path },
    ]});
    let sender = MediaActor::user(1).unwrap();
    cite::bind_run_input(&f.db, "run_a", &payload, &[], Some(&sender))
        .await
        .unwrap();
    assert_eq!(references::active_count(&f.db, own.id).await.unwrap(), 1);
    assert_eq!(
        references::active_count(&f.db, foreign.id).await.unwrap(),
        0,
        "a message must not pin another user's media"
    );
    cite::bind_run_input(&f.db, "run_guest", &payload, &[], None)
        .await
        .unwrap();
    assert_eq!(references::active_count(&f.db, own.id).await.unwrap(), 1);
    f.close().await;
}

#[tokio::test]
async fn postgres_note_cites_absolute_site_urls_under_configured_origins() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let image = f.image().await;
    let body = format!("![a](https://site.example{})", image.content_path);
    let doc: i32 = f
        .db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO phantasi_note_docs(user_id, title, content_md) VALUES (1, 'n', $1) RETURNING id",
            [body.clone().into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "id")
        .unwrap();
    let origins = vec!["https://site.example".to_string()];
    cite::bind_note_draft(&f.db, doc, 1, None, &body, &origins)
        .await
        .unwrap();
    assert_eq!(references::active_count(&f.db, image.id).await.unwrap(), 1);
    f.close().await;
}

#[tokio::test]
async fn postgres_share_image_and_favicon_are_published_and_bound() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    for key in ["site_og_image", "site_favicon"] {
        let image = f.image().await;
        let txn = f.db.begin().await.unwrap();
        let stored = cite::bind_and_publish_site_image(&txn, key, &image.content_path, &[])
            .await
            .unwrap();
        txn.commit().await.unwrap();
        assert!(stored.starts_with("/media/assets/"), "{key}: {stored}");
        let row = assets::find_by_id(&f.db, image.id).await.unwrap().unwrap();
        assert_eq!(row.exposure.as_deref(), Some("public"));
        assert_eq!(references::active_count(&f.db, image.id).await.unwrap(), 1);
        // A published site image cannot be made private under the crawler.
        assert_eq!(
            f.service.unpublish(&f.db, image.id).await.unwrap_err(),
            MediaError::PublicInUse
        );
    }
    f.close().await;
}

#[tokio::test]
async fn postgres_unpublish_waits_for_reference_scan_like_delete() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let image = f.image().await;
    f.service.publish(&f.db, image.id).await.unwrap();
    f.db.execute_unprepared(&format!(
        "UPDATE media_assets SET references_complete = FALSE WHERE id = {}",
        image.id
    ))
    .await
    .unwrap();
    assert_eq!(
        f.service.unpublish(&f.db, image.id).await.unwrap_err(),
        MediaError::PublicInUse
    );
    f.db.execute_unprepared(&format!(
        "UPDATE media_assets SET references_complete = TRUE WHERE id = {}",
        image.id
    ))
    .await
    .unwrap();
    f.service.unpublish(&f.db, image.id).await.unwrap();
    f.close().await;
}

#[tokio::test]
async fn postgres_one_permanent_address_for_private_and_public() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    f.db.execute_unprepared("INSERT INTO users (id, username) VALUES (2, 'media-other')")
        .await
        .unwrap();
    let image = f.image().await;
    assert!(image.url.starts_with("/media/assets/"), "{}", image.url);
    assert_eq!(image.catalog_url(), image.url);
    let file = image.url.rsplit('/').next().unwrap().to_string();
    let store = f.service.store();
    let anonymous = resolve_public_asset(&f.db, store, image.public_id, &file)
        .await
        .unwrap();
    assert!(matches!(anonymous, ServeOutcome::NotFound { .. }));
    let stranger = MediaActor::user(2).unwrap();
    let denied = resolve_private_asset(&f.db, store, image.public_id, &file, &stranger)
        .await
        .unwrap();
    assert!(matches!(denied, ServeOutcome::NotFound { .. }));
    let admin = MediaActor::admin(1).unwrap();
    match resolve_private_asset(&f.db, store, image.public_id, &file, &admin)
        .await
        .unwrap()
    {
        ServeOutcome::File(served) => assert_eq!(served.cache_control, NO_STORE),
        other => panic!("an admin reads any private asset: {other:?}"),
    }
    let published = f.service.publish(&f.db, image.id).await.unwrap();
    assert_eq!(published.url, image.url, "publishing never moves the asset");
    let private = f.service.unpublish(&f.db, image.id).await.unwrap();
    assert_eq!(private.url, image.url);
    f.close().await;
}

#[tokio::test]
async fn postgres_cited_cache_file_becomes_a_durable_asset() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let cache_root = f.service.store().root().join("cache");
    let cache = crate::services::image_cache::ImageCacheService::at(cache_root.clone());
    let hash = format!("cd{}", "0".repeat(62));
    let url = format!("/api/phantasi/image-cache/cd/{hash}.png");
    let source = cache_root.join("cd").join(format!("{hash}.png"));
    tokio::fs::create_dir_all(source.parent().unwrap())
        .await
        .unwrap();
    tokio::fs::write(&source, png()).await.unwrap();
    let txn = f.db.begin().await.unwrap();
    let id = cache_import::import_cached_citation(&txn, f.service.store(), &cache, &url)
        .await
        .unwrap()
        .expect("cached file imports");
    txn.commit().await.unwrap();
    // The cache path resolves to the asset through its derived public id.
    assert_eq!(resolve_asset_id(&f.db, &url).await.unwrap(), Some(id));
    let row = assets::find_by_id(&f.db, id).await.unwrap().unwrap();
    assert_eq!(row.state.as_deref(), Some("ready"));
    assert_eq!(row.exposure.as_deref(), Some("public"));
    assert_eq!(row.scope.as_deref(), Some("site"));
    assert_eq!(row.source.as_deref(), Some("import"));
    assert!(row.url.starts_with("/media/assets/"), "{}", row.url);
    assert!(row.references_complete, "new imports bind transactionally");
    // Evicting the cache no longer matters: both addresses serve the asset.
    tokio::fs::remove_file(&source).await.unwrap();
    let asset = assets::to_domain(row.clone(), 0).unwrap();
    let file = asset.url.rsplit('/').next().unwrap().to_string();
    assert!(matches!(
        resolve_public_asset(&f.db, f.service.store(), asset.public_id, &file)
            .await
            .unwrap(),
        ServeOutcome::File(_)
    ));
    match resolve_cached_image(&f.db, f.service.store(), &url)
        .await
        .unwrap()
    {
        ServeOutcome::File(served) => assert_eq!(served.etag, row.checksum_sha256),
        other => panic!("expected the imported asset, got {other:?}"),
    }
    // Unpublished, the asset is not served from the cache path either.
    f.service.unpublish(&f.db, id).await.unwrap();
    assert!(matches!(
        resolve_cached_image(&f.db, f.service.store(), &url)
            .await
            .unwrap(),
        ServeOutcome::NotFound { no_store: true }
    ));
    // A cache path whose file is gone is not importable.
    let txn = f.db.begin().await.unwrap();
    let gone = format!("/api/phantasi/image-cache/ef/ef{}.png", "0".repeat(62));
    assert_eq!(
        cache_import::import_cached_citation(&txn, f.service.store(), &cache, &gone)
            .await
            .unwrap(),
        None
    );
    txn.rollback().await.unwrap();
    f.close().await;
}

#[tokio::test]
async fn postgres_references_of_gone_consumers_are_pruned() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let image = f.image().await;
    let id = image.id;
    f.db.execute_unprepared(&format!(
        "INSERT INTO media_references (asset_id, consumer_type, consumer_id, slot, expires_at, created_at) VALUES
         ({id}, 'channel_message', 'agent_messages:987654', 'body:0', NULL, NOW()),
         ({id}, 'ai_task', 'old-task', 'result', NOW() - interval '2 days', NOW() - interval '3 days'),
         ({id}, 'channel_message', 'run_abc', 'inbound:0', NULL, NOW() - interval '2 days'),
         ({id}, 'ai_task', 'live-task', 'result', NOW() + interval '1 hour', NOW()),
         ({id}, 'tapp_storage', '424242', 'value:0', NULL, NOW()),
         ({id}, 'note_draft', '1', 'body:0', NULL, NOW())"
    ))
    .await
    .unwrap();
    assert_eq!(maintenance::prune_references(&f.db, 100).await.unwrap(), 4);
    assert_eq!(maintenance::prune_references(&f.db, 100).await.unwrap(), 0);
    assert_eq!(references::active_count(&f.db, id).await.unwrap(), 2);
    f.close().await;
}

/// 撤回修复之前撤回的帖子：已发布行没了、且有以原 Create 为对象的本地 Delete
/// 时，引用被释放；仍在发布的、没有 Delete 的、别人的 Delete、远端活动都不动。
#[tokio::test]
async fn postgres_references_of_withdrawn_publications_are_pruned() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let image = f.image().await;
    let id = image.id;
    f.db.execute_unprepared(&format!(
        r#"
        INSERT INTO users (id, username) VALUES (2, 'media-other');
        INSERT INTO federation_activities
            (activity_id, user_id, activity_type, object_type, object_json, is_local, published_at)
        VALUES
            ('https://s/a/withdrawn', 1, 'Create', 'Note', '{{}}', true, NOW()),
            ('https://s/a/withdrawn-obj', 1, 'Create', 'Note', '{{}}', true, NOW()),
            ('https://s/a/live', 1, 'Create', 'Note', '{{}}', true, NOW()),
            ('https://s/a/no-delete', 1, 'Create', 'Note', '{{}}', true, NOW()),
            ('https://s/a/foreign-delete', 1, 'Create', 'Note', '{{}}', true, NOW()),
            ('https://s/d/1', 1, 'Delete', 'note', '{{"object": "https://s/a/withdrawn"}}', true, NOW()),
            ('https://s/d/2', 1, 'Delete', 'note', '{{"object": {{"id": "https://s/a/withdrawn-obj"}}}}', true, NOW()),
            ('https://s/d/3', 1, 'Delete', 'note', '{{"object": "https://s/a/live"}}', true, NOW()),
            ('https://s/d/4', 2, 'Delete', 'note', '{{"object": "https://s/a/foreign-delete"}}', true, NOW());
        INSERT INTO federation_activities
            (activity_id, activity_type, object_type, object_json, is_local, published_at)
        VALUES ('https://r/a/remote', 'Create', 'Note', '{{}}', false, NOW());
        INSERT INTO federation_published_content
            (user_id, content_type, content_id, activity_id, visibility, published_at)
        VALUES (1, 'note', 'live', 'https://s/a/live', 'public', NOW());
        INSERT INTO media_references (asset_id, consumer_type, consumer_id, slot, expires_at, created_at) VALUES
            ({id}, 'federation_activity', 'https://s/a/withdrawn', 'attachment:0', NULL, NOW()),
            ({id}, 'federation_activity', 'https://s/a/withdrawn-obj', 'attachment:0', NULL, NOW()),
            ({id}, 'federation_activity', 'https://s/a/live', 'attachment:0', NULL, NOW()),
            ({id}, 'federation_activity', 'https://s/a/no-delete', 'attachment:0', NULL, NOW()),
            ({id}, 'federation_activity', 'https://s/a/foreign-delete', 'attachment:0', NULL, NOW()),
            ({id}, 'federation_activity', 'https://r/a/remote', 'attachment:0', NULL, NOW());
        "#
    ))
    .await
    .unwrap();
    assert_eq!(maintenance::prune_references(&f.db, 100).await.unwrap(), 2);
    assert_eq!(maintenance::prune_references(&f.db, 100).await.unwrap(), 0);
    assert_eq!(references::active_count(&f.db, id).await.unwrap(), 4);
    f.close().await;
}

#[tokio::test]
async fn postgres_feeds_never_publish_what_they_cite() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let private = f.image().await;
    let public = f.image().await;
    f.service.publish(&f.db, public.id).await.unwrap();
    // A subscribed feed names a private draft by its sequential id.
    let payload = json!({
        "content": format!(
            "<img src=\"{}\"> <img src=\"{}\">",
            content_path(private.id),
            public.url
        )
    });
    let txn = f.db.begin().await.unwrap();
    bind_rss_item(&txn, 9, &payload, &[]).await.unwrap();
    txn.commit().await.unwrap();
    let row = assets::find_by_id(&f.db, private.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        row.exposure.as_deref(),
        Some("private"),
        "a feed never publishes"
    );
    assert_eq!(
        references::active_count(&f.db, private.id).await.unwrap(),
        0
    );
    assert_eq!(references::active_count(&f.db, public.id).await.unwrap(), 1);
    f.close().await;
}

#[tokio::test]
async fn postgres_rolled_back_cache_import_is_reused_by_the_retry() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let cache_root = f.service.store().root().join("cache");
    let cache = crate::services::image_cache::ImageCacheService::at(cache_root.clone());
    let hash = format!("ab{}", "1".repeat(62));
    let url = format!("/api/phantasi/image-cache/ab/{hash}.png");
    let source = cache_root.join("ab").join(format!("{hash}.png"));
    tokio::fs::create_dir_all(source.parent().unwrap())
        .await
        .unwrap();
    tokio::fs::write(&source, png()).await.unwrap();
    let identity = |id| {
        let db = f.db.clone();
        async move {
            assets::find_by_id(&db, id)
                .await
                .unwrap()
                .unwrap()
                .public_id
        }
    };
    let txn = f.db.begin().await.unwrap();
    let first = cache_import::import_cached_citation(&txn, f.service.store(), &cache, &url)
        .await
        .unwrap()
        .unwrap();
    let first_identity = assets::find_by_id(&txn, first)
        .await
        .unwrap()
        .unwrap()
        .public_id;
    txn.rollback().await.unwrap();
    let txn = f.db.begin().await.unwrap();
    let second = cache_import::import_cached_citation(&txn, f.service.store(), &cache, &url)
        .await
        .unwrap()
        .unwrap();
    txn.commit().await.unwrap();
    assert_eq!(
        identity(second).await,
        first_identity,
        "same file, same identity"
    );
    f.close().await;
}

#[tokio::test]
async fn postgres_retiring_old_addresses_rewrites_content_then_drops_the_tables() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let federated = f.service.publish(&f.db, f.image().await.id).await.unwrap();
    let cached = f.service.publish(&f.db, f.image().await.id).await.unwrap();
    let hash = format!("44{}", "1".repeat(62));
    let old_file = "/media/federation/1/old.png";
    let cache_path = format!("/api/phantasi/image-cache/44/{hash}.png");
    let brew_path = format!("/api/brew/image-cache/44/{hash}.png");
    let uncached_brew = format!("/api/brew/image-cache/ab/ab{}.png", "2".repeat(62));
    let own = crate::services::media::configured_origins().await[0]
        .trim_end_matches('/')
        .to_string();
    f.db.execute_unprepared(&format!(
        "CREATE TABLE media_url_aliases (id SERIAL PRIMARY KEY, local_path TEXT NOT NULL,
             asset_id INTEGER NOT NULL REFERENCES media_assets(id) ON DELETE RESTRICT,
             created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP);
         CREATE TABLE media_migration_jobs (id SERIAL PRIMARY KEY, source_kind TEXT NOT NULL,
             source_key TEXT NOT NULL, cursor TEXT);
         INSERT INTO media_url_aliases (local_path, asset_id) VALUES
             ('{old_file}', {fed}), ('{cache_path}', {cache});
         UPDATE media_assets SET url = '{old_file}' WHERE id = {fed};
         INSERT INTO media_assets (kind, url, mime, name, size)
             VALUES ('upload', '/media/federation/1/never-copied.png', 'image/png', 'x.png', 0);
         INSERT INTO media_references (asset_id, consumer_type, consumer_id, slot)
             VALUES ({fed}, 'federation_outbox', '9', 'attachment:0');
         INSERT INTO phantasi_note_docs (id, user_id, title, content_md, image)
             VALUES (5, 1, 'n', '![a]({old_file}) ![b]({brew_path}) ![c]({uncached_brew}) ![d](https://old.example{old_file})', '{cache_path}');
         INSERT INTO configurations (key, value)
             VALUES ('ui_wallpaper_url', to_jsonb('{own}{old_file}'::text))
             ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value;",
        fed = federated.id,
        cache = cached.id,
    ))
    .await
    .unwrap();
    let history = || async {
        f.db.query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT count(*)::int AS n FROM phantasi_note_history",
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<i32>("", "n")
        .unwrap()
    };
    let history_before = history().await;

    let rewritten = retire_legacy_media(&f.db).await.unwrap();
    assert_eq!(rewritten, 3, "note body, note cover, wallpaper");
    let fed_url = federated.public_path.clone().unwrap();
    let cache_url = cached.public_path.clone().unwrap();
    let note =
        f.db.query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT content_md, image FROM phantasi_note_docs WHERE id = 5",
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        note.try_get::<String>("", "content_md").unwrap(),
        format!(
            "![a]({fed_url}) ![b]({cache_url}) ![c]({}) ![d](https://old.example{old_file})",
            uncached_brew.replace("/api/brew/", "/api/phantasi/")
        ),
        "another site's address is its file, not ours"
    );
    assert_eq!(note.try_get::<String>("", "image").unwrap(), cache_url);
    assert_eq!(history().await, history_before, "no history revision");
    assert_eq!(
        stored_config(&f, "ui_wallpaper_url").await,
        Some(json!(format!("{own}{fed_url}")))
    );
    let row = assets::find_by_id(&f.db, federated.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.url, fed_url);
    let leftovers =
        f.db.query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT (SELECT count(*) FROM media_assets WHERE state IS NULL)::int AS unmigrated,
                    (SELECT count(*) FROM media_assets WHERE state = 'missing')::int AS missing,
                    (SELECT count(*) FROM media_references
                      WHERE consumer_type = 'federation_outbox')::int AS outbox,
                    (to_regclass('media_url_aliases') IS NULL
                     AND to_regclass('media_migration_jobs') IS NULL) AS dropped",
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(leftovers.try_get::<i32>("", "unmigrated").unwrap(), 0);
    assert_eq!(leftovers.try_get::<i32>("", "missing").unwrap(), 1);
    assert_eq!(leftovers.try_get::<i32>("", "outbox").unwrap(), 0);
    assert!(leftovers.try_get::<bool>("", "dropped").unwrap());
    // Nothing left to retire on the next start.
    assert_eq!(retire_legacy_media(&f.db).await.unwrap(), 0);
    f.close().await;
}
