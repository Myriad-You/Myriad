use super::*;
use crate::services::media::test_support::Fixture;
use serde_json::json;

const OLD_A: &str = "/media/federation/1/a.png";
const OLD_B: &str = "/media/federation/1/b.png";

async fn failed_upgrade(f: &Fixture) -> DataPaths {
    // Two missing files produce six failures in 0.6.1: copy, discovery,
    // binding. Both successful and failed catalogue rows retain their UUIDs.
    f.db.execute_unprepared(&format!(
        r#"
        CREATE TABLE media_migration_jobs (id SERIAL PRIMARY KEY, source_kind TEXT NOT NULL,
            source_key TEXT NOT NULL, error_code TEXT, cursor TEXT);
        CREATE TABLE media_url_aliases (local_path TEXT PRIMARY KEY, asset_id INTEGER NOT NULL);
        INSERT INTO media_assets (id,kind,url,mime,name,size,public_id)
            VALUES (101,'upload','{OLD_A}','image/png','a.png',0,gen_random_uuid()),
                   (102,'upload','{OLD_B}','image/png','b.png',0,gen_random_uuid());
        INSERT INTO media_migration_jobs (source_kind,source_key,cursor) VALUES
            ('upgrade','platform_media_v2','{{"revision":4,"complete":false,"retrying":true}}');
        INSERT INTO media_migration_jobs (source_kind,source_key,error_code,cursor) VALUES
            ('upgrade_failure','1:0:101','MEDIA_MISSING','media_assets:101'),
            ('upgrade_failure','1:0:102','MEDIA_MISSING','media_assets:102'),
            ('upgrade_failure','0:1:23',NULL,'phantasi_note_docs:23'),
            ('upgrade_failure','0:1:36',NULL,'phantasi_note_docs:36'),
            ('upgrade_failure','2:1:23','MEDIA_NOT_READY','phantasi_note_docs:23'),
            ('upgrade_failure','2:1:36','MEDIA_NOT_READY','phantasi_note_docs:36');
        INSERT INTO phantasi_note_docs (id,user_id,title,content_md) VALUES
            (23,1,'a','![a]({OLD_A})'),(36,1,'b','![b]({OLD_B})');
    "#
    ))
    .await
    .unwrap();
    let mut paths = DataPaths::from_env();
    paths.root = f.service.store().root().join("legacy-data");
    paths.media = f.service.store().root().to_path_buf();
    paths.cache_images = paths.media.join("legacy-cache");
    paths
}

async fn assert_untouched(f: &Fixture) {
    let row = f.db.query_one_raw(Statement::from_string(DatabaseBackend::Postgres,
        "SELECT (SELECT count(*) FROM media_assets WHERE id IN (101,102) AND state IS NULL)::int AS assets,
         (SELECT count(*) FROM media_migration_jobs WHERE source_kind = 'upgrade_failure' AND error_code IS DISTINCT FROM 'MEDIA_MISSING')::int AS failures"))
        .await.unwrap().unwrap();
    assert_eq!(row.try_get::<i32>("", "assets").unwrap(), 2);
    assert_eq!(row.try_get::<i32>("", "failures").unwrap(), 4);
}

#[tokio::test]
async fn postgres_missing_only_tail_automatically_migrates_and_retires() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let intact = f.image().await;
    let intact_key = super::super::assets::find_by_id(&f.db, intact.id)
        .await
        .unwrap()
        .unwrap()
        .storage_key
        .unwrap();
    let paths = failed_upgrade(&f).await;
    // Reproduce the production failure's federation phase (0:6 / 2:6).
    f.db.execute_unprepared(&format!(r#"
        INSERT INTO federation_activities (id,activity_id,activity_type,object_json,is_local,published_at)
        VALUES (23,'https://test/23','Create','{{"attachment":[{{"url":"{OLD_A}"}}]}}',TRUE,NOW()),
               (36,'https://test/36','Create','{{"attachment":[{{"url":"{OLD_B}"}}]}}',TRUE,NOW());
        UPDATE media_migration_jobs SET source_key = replace(source_key,':1:',':6:'),
            cursor = replace(cursor,'phantasi_note_docs:','federation_activities:')
            WHERE source_key LIKE '0:1:%' OR source_key LIKE '2:1:%';
    "#)).await.unwrap();
    let identity =
        f.db.query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT public_id FROM media_assets WHERE id = 101",
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<uuid::Uuid>("", "public_id")
        .unwrap();
    assert!(
        prepare_legacy_media_upgrade(&f.db, &paths, &[], false)
            .await
            .unwrap()
    );
    let row = f.db.query_one_raw(Statement::from_string(DatabaseBackend::Postgres,
        "SELECT (SELECT count(*) FROM media_migration_jobs WHERE source_kind = 'upgrade_failure' AND error_code = 'MEDIA_MISSING')::int AS failures,
        (SELECT cursor::jsonb ->> 'complete' FROM media_migration_jobs WHERE source_kind = 'upgrade') AS complete,
        (SELECT count(*) FROM media_assets WHERE id IN (101,102) AND state = 'missing' AND references_complete = FALSE)::int AS missing"))
        .await.unwrap().unwrap();
    assert_eq!(row.try_get::<i32>("", "failures").unwrap(), 6);
    assert_eq!(row.try_get::<String>("", "complete").unwrap(), "false");
    assert_eq!(row.try_get::<i32>("", "missing").unwrap(), 2);
    // Retrying the preflight is idempotent, including reclassified discovery jobs.
    assert!(
        prepare_legacy_media_upgrade(&f.db, &paths, &[], false)
            .await
            .unwrap()
    );
    migration::Migrator::up_after_media_preflight(&f.db, None, true)
        .await
        .unwrap();
    super::super::retire_legacy_media(&f.db).await.unwrap();
    let row = f.db.query_one_raw(Statement::from_string(DatabaseBackend::Postgres,
        "SELECT a.public_id,a.state,a.url,n.content_md, to_regclass('media_migration_jobs') IS NULL AS retired
         FROM media_assets a JOIN phantasi_note_docs n ON n.id=23 WHERE a.id=101"))
        .await.unwrap().unwrap();
    assert_eq!(
        row.try_get::<uuid::Uuid>("", "public_id").unwrap(),
        identity
    );
    assert_eq!(row.try_get::<String>("", "state").unwrap(), "missing");
    assert_eq!(
        row.try_get::<String>("", "content_md").unwrap(),
        format!("![a]({})", row.try_get::<String>("", "url").unwrap())
    );
    assert!(row.try_get::<bool>("", "retired").unwrap());
    let activity =
        f.db.query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT object_json FROM federation_activities WHERE id=23",
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<Value>("", "object_json")
        .unwrap();
    assert_eq!(
        activity["attachment"][0]["url"],
        row.try_get::<String>("", "url").unwrap()
    );
    assert_eq!(
        hash_path(&f.service.store().final_path(&intact_key).unwrap())
            .await
            .unwrap()
            .0,
        intact.size as u64
    );
    assert!(
        !prepare_legacy_media_upgrade(&f.db, &paths, &[], false)
            .await
            .unwrap()
    );
    f.close().await;
}

#[tokio::test]
async fn postgres_existing_source_or_copy_prevents_missing_acceptance() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    f.image().await;
    let paths = failed_upgrade(&f).await;
    let source = legacy_file(&paths, OLD_A).unwrap();
    tokio::fs::create_dir_all(source.parent().unwrap())
        .await
        .unwrap();
    tokio::fs::write(&source, b"still present").await.unwrap();
    let error = prepare_legacy_media_upgrade(&f.db, &paths, &[], true)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("marked missing is present"));
    assert_untouched(&f).await;
    // A valid permanent copy from a rolled-back writer must not be discarded either.
    tokio::fs::remove_file(&source).await.unwrap();
    let id =
        f.db.query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT public_id FROM media_assets WHERE id = 101",
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<uuid::Uuid>("", "public_id")
        .unwrap();
    let dest = f
        .service
        .store()
        .final_path(&urls::storage_key(id, "png").unwrap())
        .unwrap();
    tokio::fs::create_dir_all(dest.parent().unwrap())
        .await
        .unwrap();
    tokio::fs::write(dest, b"copied").await.unwrap();
    assert!(
        prepare_legacy_media_upgrade(&f.db, &paths, &[], true)
            .await
            .is_err()
    );
    assert_untouched(&f).await;
    f.close().await;
}

#[tokio::test]
async fn postgres_wrong_volume_and_corrupt_ready_media_are_refused() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let intact = f.image().await;
    let intact_key = super::super::assets::find_by_id(&f.db, intact.id)
        .await
        .unwrap()
        .unwrap()
        .storage_key
        .unwrap();
    let mut paths = failed_upgrade(&f).await;
    let correct = paths.media.clone();
    paths.media = correct.join("empty-volume");
    assert!(
        prepare_legacy_media_upgrade(&f.db, &paths, &[], true)
            .await
            .unwrap_err()
            .to_string()
            .contains("volume may be missing")
    );
    assert_untouched(&f).await;
    paths.media = correct;
    tokio::fs::write(
        f.service.store().final_path(&intact_key).unwrap(),
        b"corrupt",
    )
    .await
    .unwrap();
    assert!(
        prepare_legacy_media_upgrade(&f.db, &paths, &[], false)
            .await
            .unwrap_err()
            .to_string()
            .contains("does not match")
    );
    assert_untouched(&f).await;
    f.close().await;
}

#[tokio::test]
async fn postgres_all_missing_requires_operator_but_needs_no_sql_repair() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let paths = failed_upgrade(&f).await;
    assert!(
        prepare_legacy_media_upgrade(&f.db, &paths, &[], false)
            .await
            .unwrap_err()
            .to_string()
            .contains("MYRIAD_ACCEPT_MISSING_MEDIA=1")
    );
    assert_untouched(&f).await;
    assert!(
        prepare_legacy_media_upgrade(&f.db, &paths, &[], true)
            .await
            .unwrap()
    );
    f.close().await;
}

#[tokio::test]
async fn postgres_unrelated_or_unfinished_failures_do_not_get_reclassified() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    f.image().await;
    let paths = failed_upgrade(&f).await;
    for change in [
        "UPDATE media_migration_jobs SET error_code = 'MEDIA_STORE_FAILED' WHERE source_key = '2:1:36'",
        "UPDATE media_migration_jobs SET cursor = 'phantasi_note_docs:99' WHERE source_key = '2:1:36'",
        "UPDATE phantasi_note_docs SET content_md = 'no missing image' WHERE id = 36",
        "UPDATE phantasi_note_docs SET content_md = '![a](/media/federation/1/a.png) ![unknown](/media/federation/1/unknown.png)' WHERE id = 36",
    ] {
        let txn = f.db.begin().await.unwrap();
        txn.execute_unprepared(change).await.unwrap();
        txn.commit().await.unwrap();
        assert!(
            prepare_legacy_media_upgrade(&f.db, &paths, &[], true)
                .await
                .is_err()
        );
        assert_untouched(&f).await;
        f.db.execute_unprepared("UPDATE media_migration_jobs SET error_code = 'MEDIA_NOT_READY',cursor = 'phantasi_note_docs:36' WHERE source_key = '2:1:36'; UPDATE phantasi_note_docs SET content_md = '![b](/media/federation/1/b.png)' WHERE id = 36").await.unwrap();
    }
    f.db.execute_unprepared("UPDATE media_migration_jobs SET cursor = '{\"revision\":4,\"complete\":false,\"retrying\":false}' WHERE source_kind = 'upgrade'").await.unwrap();
    assert!(
        !prepare_legacy_media_upgrade(&f.db, &paths, &[], true)
            .await
            .unwrap()
    );
    assert_untouched(&f).await;
    assert!(
        migration::Migrator::up_after_media_preflight(&f.db, None, false)
            .await
            .is_err()
    );
    f.close().await;
}

#[test]
fn only_valid_local_paths_and_fixed_revision_four_phases_are_used() {
    let origins = vec!["https://own.example".to_string()];
    let paths = cited_paths(
        &json!({"content":"![a](https://own.example/media/federation/1/a.png) ![b](https://foreign.example/media/federation/1/b.png)",
        "nested":["/api/brew/image-cache/aa/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png"]}),
        &origins,
    );
    assert_eq!(paths.len(), 2);
    assert!(paths.contains(OLD_A));
    assert!(paths.iter().any(|path| path.starts_with("/api/phantasi/")));
    assert!(local_path("/media/federation/1/../secret", &origins).is_none());
    assert!(local_path("/media/federation/1/%2e%2e", &origins).is_none());
    assert!(consumer(13, "1").is_none());
    assert!(consumer(5, "JWT_SECRET").is_none());
    assert_eq!(consumer(6, "23").unwrap().0, "federation_activities");
}
