//! Automatic, resumable media upgrade with an optional admin retry endpoint.
//! No schema-startup I/O and no filesystem crawl: only catalogued or cited files.
use super::{LegacyPaths, MediaError, MediaStore, cite, migration};
use crate::models::entities::{media_assets, media_migration_jobs};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseBackend, DatabaseConnection,
    DatabaseTransaction, EntityTrait, QueryFilter, Set, Statement, TransactionTrait,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const JOB: &str = "platform_media_v2";
const BATCH: u64 = 50;
/// 3: backfill the `site_wallpaper` reference for existing wallpaper settings.
const REVISION: u32 = 3;
const WALLPAPER_KEY: &str = "ui_wallpaper_url";
const FAILURE_JOB: &str = "upgrade_failure";
const PHASES: &[(&str, &str, &str)] = &[
    ("media_assets", "id", "TRUE"),
    ("phantasi_note_docs", "id", "TRUE"),
    ("phantasi_items", "id", "TRUE"),
    (
        "phantasi_note_history",
        "(doc_id::text || ':' || revision::text)",
        "TRUE",
    ),
    ("agent_persona", "id", "TRUE"),
    ("configurations", "key", "key = 'dashboard_layout'"),
    ("federation_activities", "id", "is_local = TRUE"),
    (
        "federation_delivery_queue",
        "id",
        "status IN ('pending', 'delivering', 'failed')",
    ),
    (
        "tapp_runtime_registry",
        "record_id",
        "namespace = 'ai_task' AND expires_at > EXTRACT(EPOCH FROM NOW())::BIGINT",
    ),
    ("federation_channel_messages", "id", "is_encrypted = FALSE"),
    ("agent_messages", "id", "TRUE"),
    // Appended so earlier phase indices (and their failure keys) stay stable.
    ("configurations", "key", "key = 'ui_wallpaper_url'"),
];

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct UpgradeProgress {
    #[serde(default)]
    pub revision: u32,
    #[serde(default)]
    pub pending_failures: u64,
    #[serde(default)]
    pub retrying: bool,
    /// 0: discover citations, 1: copy catalog, 2: bind consumers.
    #[serde(default)]
    pub pass: u8,
    pub phase: usize,
    pub after: String,
    pub scanned: u64,
    pub complete: bool,
    pub error: Option<String>,
    #[serde(default)]
    pub error_source: Option<String>,
    #[serde(default)]
    pub consecutive_failures: u32,
    /// Unix seconds; shared by all replicas and retained across restarts.
    #[serde(default)]
    pub next_retry_at: Option<i64>,
}

pub async fn status(db: &impl ConnectionTrait) -> Result<UpgradeProgress, MediaError> {
    let job = media_migration_jobs::Entity::find()
        .filter(media_migration_jobs::Column::SourceKind.eq("upgrade"))
        .filter(media_migration_jobs::Column::SourceKey.eq(JOB))
        .one(db)
        .await?;
    match job.and_then(|row| row.cursor) {
        Some(cursor) => serde_json::from_str(&cursor)
            .map_err(|_| MediaError::invalid("Invalid media migration cursor")),
        None => Ok(UpgradeProgress::default()),
    }
}

async fn save(db: &impl ConnectionTrait, progress: &UpgradeProgress) -> Result<(), MediaError> {
    let cursor = serde_json::to_string(progress).map_err(|_| MediaError::StoreFailed)?;
    migration::record_job(
        db,
        "upgrade",
        JOB,
        None,
        if progress.complete {
            "copied"
        } else {
            "pending"
        },
        if progress.complete {
            "verified"
        } else {
            "pending"
        },
        if progress.complete {
            "switched"
        } else {
            "pending"
        },
        progress.error.as_deref(),
        Some(&cursor),
    )
    .await
}

pub async fn advance(
    db: &DatabaseConnection,
    store: &MediaStore,
    paths: &LegacyPaths,
    origins: &[String],
    restart: bool,
) -> Result<UpgradeProgress, MediaError> {
    advance_inner(
        db,
        store,
        paths,
        origins,
        Some(restart),
        chrono::Utc::now().timestamp(),
    )
    .await?
    .ok_or(MediaError::StoreFailed)
}

pub async fn configured_origins() -> Vec<String> {
    let mut origins = vec![crate::oauth_url_builder::SiteConfig::get_base_url().await];
    let config = crate::GLOBAL_CONFIG.read().await;
    origins.extend(
        config
            .base_url
            .iter()
            .chain(config.frontend_url.iter())
            .cloned(),
    );
    origins
}

/// One background tick. Busy, backed-off and completed jobs perform no work.
pub(super) async fn automatic_step(
    db: &DatabaseConnection,
    store: &MediaStore,
    paths: &LegacyPaths,
    origins: &[String],
    now: i64,
) -> Result<Option<UpgradeProgress>, MediaError> {
    advance_inner(db, store, paths, origins, None, now).await
}

async fn advance_inner(
    db: &DatabaseConnection,
    store: &MediaStore,
    paths: &LegacyPaths,
    origins: &[String],
    manual: Option<bool>,
    now: i64,
) -> Result<Option<UpgradeProgress>, MediaError> {
    let txn = db.begin().await?;
    if manual.is_none() {
        let row = txn.query_one_raw(Statement::from_string(DatabaseBackend::Postgres,
            "SELECT pg_try_advisory_xact_lock(hashtextextended('media:upgrade:v2', 0)) AS acquired")).await?
            .ok_or(MediaError::StoreFailed)?;
        if !row.try_get::<bool>("", "acquired")? {
            return Ok(None);
        }
    } else {
        txn.execute_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT pg_advisory_xact_lock(hashtextextended('media:upgrade:v2', 0))",
        ))
        .await?;
    }
    let restart = manual == Some(true);
    let mut progress = if restart {
        UpgradeProgress::default()
    } else {
        status(&txn).await?
    };
    if restart || progress.revision != REVISION {
        progress = UpgradeProgress {
            revision: REVISION,
            ..Default::default()
        };
        txn.execute_unprepared(
            "DELETE FROM media_migration_jobs WHERE source_kind = 'upgrade_failure'",
        )
        .await?;
        // Older completed jobs did not repair dashboard URLs or bind the site
        // wallpaper. Recheck their consumers before allowing migrated assets
        // to be deleted.
        txn.execute_unprepared("UPDATE media_assets SET references_complete = FALSE WHERE source = 'legacy' AND state = 'ready'").await?;
    }
    let saved_progress = progress.clone();
    if progress.complete {
        return Ok(manual.map(|_| progress));
    }
    if manual.is_none() && progress.next_retry_at.is_some_and(|at| at > now) {
        return Ok(None);
    }
    progress.error = None;
    progress.error_source = None;
    progress.next_retry_at = None;
    // Batch-level failures preserve the cursor; individual record failures use
    // nested savepoints and are retained without blocking later records.
    let batch = txn.begin().await?;
    let result = advance_on(&batch, store, paths, origins, &mut progress, now).await;
    match result {
        Ok(()) => {
            batch.commit().await?;
            if progress.pending_failures == 0 {
                progress.consecutive_failures = 0;
                progress.next_retry_at = None;
            }
        }
        Err(error) => {
            batch.rollback().await?;
            let failed_source = progress.error_source.clone();
            progress = saved_progress;
            progress.error_source = failed_source;
            progress.error = Some(error.code().to_string());
            progress.consecutive_failures = progress.consecutive_failures.saturating_add(1);
            let delay = (60_i64
                * (1_i64 << progress.consecutive_failures.saturating_sub(1).min(6)))
            .min(3600);
            progress.next_retry_at = Some(now.saturating_add(delay));
        }
    }
    save(&txn, &progress).await?;
    txn.commit().await?;
    Ok(Some(progress))
}

async fn advance_on(
    db: &DatabaseTransaction,
    store: &MediaStore,
    paths: &LegacyPaths,
    origins: &[String],
    progress: &mut UpgradeProgress,
    now: i64,
) -> Result<(), MediaError> {
    if progress.pass == 1 && progress.phase > 0 {
        progress.pass = 2;
        progress.phase = 1;
    }
    if progress.pass == 0 && progress.phase >= PHASES.len() {
        progress.pass = 1;
        progress.phase = 0;
        progress.after.clear();
        return Ok(());
    }
    let Some(&(table, key, filter)) = PHASES.get(progress.phase) else {
        refresh_failures(db, progress).await?;
        if progress.pending_failures > 0 {
            // Retry only durable work records. Failed bindings also enqueue
            // discovery; newly discovered assets enqueue their copy dependency.
            progress.retrying = true;
            progress.pass = 0;
            progress.phase = 0;
            progress.after.clear();
            progress.consecutive_failures = progress.consecutive_failures.saturating_add(1);
            let delay = (60_i64
                * (1_i64 << progress.consecutive_failures.saturating_sub(1).min(6)))
            .min(3600);
            progress.next_retry_at = Some(now.saturating_add(delay));
            return Ok(());
        }
        // Only migrated legacy rows are eligible. New writers already maintain
        // references transactionally; incomplete or missing imports stay protected.
        db.execute_unprepared("UPDATE media_assets SET references_complete = TRUE WHERE source = 'legacy' AND state = 'ready' AND references_complete = FALSE").await?;
        progress.complete = true;
        return Ok(());
    };
    let prefix = format!("{}:{}:", progress.pass, progress.phase);
    let rows = if progress.retrying {
        db.query_all_raw(Statement::from_sql_and_values(DatabaseBackend::Postgres,
            "SELECT substring(source_key FROM length($1) + 1) AS cursor FROM media_migration_jobs WHERE source_kind = 'upgrade_failure' AND starts_with(source_key, $1) AND source_key > $2 ORDER BY source_key LIMIT $3",
            [prefix.clone().into(), format!("{prefix}{}", progress.after).into(), (BATCH as i64).into()])).await?
    } else {
        // Select IDs without locking the whole batch. Each payload is read
        // under its own lock immediately before use, never from a stale snapshot.
        db.query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            scan_sql(table, key, filter),
            [progress.after.clone().into(), (BATCH as i64).into()],
        ))
        .await?
    };
    let started = std::time::Instant::now();
    let mut processed = 0;
    for row in &rows {
        let cursor: String = row.try_get("", "cursor")?;
        progress.error_source = Some(format!("{table}:{cursor}"));
        let record = db.begin().await?;
        // Short lock waits are deferred like other record errors. Business
        // writes and federation workers must not queue behind an entire batch.
        record
            .execute_unprepared("SET LOCAL lock_timeout = '250ms'")
            .await?;
        let result = async {
            let row = record.query_one_raw(Statement::from_sql_and_values(DatabaseBackend::Postgres,
                format!("SELECT to_jsonb(s) AS payload FROM {table} s WHERE ({filter}) AND {} FOR UPDATE NOWAIT", row_predicate(table, key)),
                [cursor.clone().into()])).await?;
            if let Some(row) = row {
                let payload: Value = row.try_get("", "payload")?;
                process_row(&record, store, paths, origins, progress.pass, progress.phase, table, &cursor, &payload).await?;
            }
            Ok::<_, MediaError>(())
        }.await;
        let failure_key = format!("{}:{}:{cursor}", progress.pass, progress.phase);
        match result {
            Ok(()) => {
                record.commit().await?;
                db.execute_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "DELETE FROM media_migration_jobs WHERE source_kind = $1 AND source_key = $2",
                    [FAILURE_JOB.into(), failure_key.into()],
                ))
                .await?;
            }
            Err(error) => {
                record.rollback().await?;
                if progress.pass == 2 {
                    // A concurrent edit may introduce a previously unknown asset.
                    migration::record_job(
                        db,
                        FAILURE_JOB,
                        &format!("0:{}:{cursor}", progress.phase),
                        None,
                        "pending",
                        "pending",
                        "pending",
                        None,
                        Some(&format!("{table}:{cursor}")),
                    )
                    .await?;
                }
                migration::record_job(
                    db,
                    FAILURE_JOB,
                    &failure_key,
                    None,
                    "pending",
                    "pending",
                    "pending",
                    Some(error.code()),
                    Some(&format!("{table}:{cursor}")),
                )
                .await?;
            }
        }
        progress.error_source = None;
        progress.after = cursor;
        progress.scanned += 1;
        processed += 1;
        // Yield only between records: cancelling a slow copy and rolling back
        // the entire batch would retry the same large files forever.
        if started.elapsed() >= std::time::Duration::from_secs(20) {
            break;
        }
    }
    if processed == rows.len() && rows.len() < BATCH as usize {
        progress.phase += 1;
        progress.after.clear();
    }
    refresh_failures(db, progress).await?;
    Ok(())
}

fn scan_sql(table: &str, key: &str, filter: &str) -> String {
    let (predicate, order) = if table == "phantasi_note_history" {
        ("(doc_id, revision) > (COALESCE(NULLIF(split_part($1, ':', 1), '')::bigint, 0), COALESCE(NULLIF(split_part($1, ':', 2), '')::bigint, 0))".into(), "doc_id, revision".into())
    } else if key == "id" && table != "agent_persona" {
        (
            format!("{key} > COALESCE(NULLIF($1, '')::bigint, 0)"),
            key.to_owned(),
        )
    } else {
        (format!("{key} > $1"), key.to_owned())
    };
    // Compare/order native primary keys so each page can use the existing index.
    format!(
        "SELECT {key}::text AS cursor FROM {table} s WHERE ({filter}) AND {predicate} ORDER BY {order} LIMIT $2"
    )
}

fn row_predicate(table: &str, key: &str) -> String {
    if table == "phantasi_note_history" {
        "doc_id = split_part($1, ':', 1)::bigint AND revision = split_part($1, ':', 2)::bigint"
            .into()
    } else if key == "id" && table != "agent_persona" {
        format!("{key} = $1::bigint")
    } else {
        format!("{key} = $1")
    }
}

async fn refresh_failures(
    db: &impl ConnectionTrait,
    progress: &mut UpgradeProgress,
) -> Result<(), MediaError> {
    let row = db.query_one_raw(Statement::from_string(DatabaseBackend::Postgres,
        "SELECT count(*)::bigint AS count FROM media_migration_jobs WHERE source_kind = 'upgrade_failure'"))
        .await?.ok_or(MediaError::StoreFailed)?;
    progress.pending_failures = row.try_get::<i64>("", "count")? as u64;
    let failure = media_migration_jobs::Entity::find()
        .filter(media_migration_jobs::Column::SourceKind.eq(FAILURE_JOB))
        .filter(media_migration_jobs::Column::ErrorCode.is_not_null())
        .one(db)
        .await?;
    progress.error = failure.as_ref().and_then(|row| row.error_code.clone());
    progress.error_source = failure.and_then(|row| row.cursor);
    Ok(())
}

async fn process_row(
    db: &impl ConnectionTrait,
    store: &MediaStore,
    paths: &LegacyPaths,
    origins: &[String],
    pass: u8,
    phase: usize,
    table: &str,
    cursor: &str,
    payload: &Value,
) -> Result<(), MediaError> {
    if pass == 0 && phase == 0 {
        db.execute_raw(Statement::from_sql_and_values(DatabaseBackend::Postgres,
                "UPDATE media_assets SET public_id = gen_random_uuid() WHERE id = $1 AND public_id IS NULL",
                [cursor.parse::<i32>().map_err(|_| MediaError::StoreFailed)?.into()])).await?;
    } else if pass == 0 {
        // RSS cache-only files retain their disposable cache lifecycle. Only
        // already catalogued assets need durable RSS references in pass 2.
        if table == "phantasi_items" && payload["content_md"].is_null() {
            return Ok(());
        }
        let layout = if is_wallpaper(table, cursor) {
            Some(Value::String(
                wallpaper_candidate(db, paths, origins, payload).await?,
            ))
        } else {
            parse_layout(table, &payload)
        };
        import_cited(
            db,
            store,
            paths,
            origins,
            layout.as_ref().unwrap_or(&payload),
            false,
        )
        .await?;
    } else if phase == 0 {
        let asset: media_assets::Model =
            serde_json::from_value(payload.clone()).map_err(|_| MediaError::StoreFailed)?;
        if let Ok(plan) = migration::plan_catalog_url(&asset.url, origins, paths) {
            if asset.state.is_none() || asset.state.as_deref() == Some("missing") {
                ensure_copied(store, db, &asset, &plan).await?;
            } else if asset.state.as_deref() == Some("ready")
                && asset.source.as_deref() == Some("legacy")
            {
                migration::register_aliases(
                    db,
                    &asset,
                    &plan,
                    asset
                        .checksum_sha256
                        .as_deref()
                        .ok_or(MediaError::StoreFailed)?,
                )
                .await?;
            }
        }
    } else {
        bind_row(db, store, paths, origins, table, &cursor, &payload).await?;
    }
    Ok(())
}

async fn ensure_copied(
    store: &MediaStore,
    db: &impl ConnectionTrait,
    row: &media_assets::Model,
    plan: &migration::CatalogPlan,
) -> Result<(), MediaError> {
    match migration::migrate_one(store, db, row, plan).await? {
        migration::Outcome::Copied { .. } | migration::Outcome::Already { .. } => Ok(()),
        migration::Outcome::Missing => Err(MediaError::Missing),
        migration::Outcome::Failed => Err(MediaError::StoreFailed),
    }
}

async fn import_cited(
    db: &impl ConnectionTrait,
    store: &MediaStore,
    paths: &LegacyPaths,
    origins: &[String],
    payload: &Value,
    copy: bool,
) -> Result<Vec<String>, MediaError> {
    let _ = store;
    let mut strings = Vec::new();
    cite::collect_strings(payload, &mut strings);
    let mut urls = Vec::new();
    for text in strings {
        if let Some(path) = super::urls::cite_local_path(&text, origins) {
            urls.push(path);
        }
        urls.extend(cite::extract_registered_paths(&text, origins));
    }
    urls.sort();
    urls.dedup();
    for url in &urls {
        let Ok(_plan) = migration::plan_catalog_url(url, origins, paths) else {
            continue;
        };
        let equivalent = super::legacy::cache_equivalent_path(url);
        let found = match cite::resolve_asset_id(db, url).await? {
            Some(id) => Some(id),
            None => match equivalent.as_deref() {
                Some(other) => cite::resolve_asset_id(db, other).await?,
                None => None,
            },
        };
        if let Some(id) = found {
            let row = super::assets::find_by_id(db, id)
                .await?
                .ok_or(MediaError::Missing)?;
            if copy && row.state.as_deref() != Some("ready") {
                return Err(MediaError::NotReady);
            }
            continue;
        }
        if copy {
            return Err(MediaError::NotReady);
        }
        let ext = std::path::Path::new(url)
            .extension()
            .and_then(|v| v.to_str())
            .unwrap_or("");
        let mime = match ext.to_ascii_lowercase().as_str() {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "mp4" => "video/mp4",
            "webm" => "video/webm",
            "mov" => "video/quicktime",
            _ => return Err(MediaError::invalid("Unsupported legacy media")),
        };
        let asset = media_assets::ActiveModel {
            kind: Set("upload".into()),
            url: Set(url.clone()),
            mime: Set(mime.into()),
            name: Set(url.rsplit('/').next().unwrap_or("media").into()),
            size: Set(0),
            created_at: Set(chrono::Utc::now().fixed_offset()),
            references_complete: Set(false),
            ..Default::default()
        }
        .insert(db)
        .await?;
        migration::record_job(
            db,
            FAILURE_JOB,
            &format!("1:0:{}", asset.id),
            None,
            "pending",
            "pending",
            "pending",
            None,
            Some(&format!("media_assets:{}", asset.id)),
        )
        .await?;
        // Identity commits in the discovery pass before any file copy. Retrying
        // a failed or interrupted copy therefore reuses the same public_id/key.
    }
    Ok(urls)
}

fn is_wallpaper(table: &str, cursor: &str) -> bool {
    table == "configurations" && cursor == WALLPAPER_KEY
}

fn stored_wallpaper(payload: &Value) -> String {
    parse_layout("configurations", payload)
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// The stored wallpaper, with a URL saved under a previous site origin reduced
/// to its path when that path is media of this instance: already catalogued, or
/// a legacy file present on disk awaiting import. Anything else stays external.
async fn wallpaper_candidate(
    db: &impl ConnectionTrait,
    paths: &LegacyPaths,
    origins: &[String],
    payload: &Value,
) -> Result<String, MediaError> {
    let url = stored_wallpaper(payload);
    if super::urls::cite_local_path(&url, origins).is_some() {
        return Ok(url);
    }
    let Some(path) = super::urls::media_shaped_path(&url) else {
        return Ok(url);
    };
    if cite::resolve_asset_id(db, &path).await?.is_some() {
        return Ok(path);
    }
    if let Ok(plan) = migration::plan_catalog_url(&path, origins, paths) {
        if tokio::fs::try_exists(&plan.disk).await.unwrap_or(false) {
            return Ok(path);
        }
    }
    Ok(url)
}

fn parse_layout(table: &str, payload: &Value) -> Option<Value> {
    // Config values may contain a JSON string wrapping the layout document.
    if table == "configurations" {
        let value = payload.get("value").unwrap_or(&Value::Null);
        let parsed = value
            .as_str()
            .and_then(|s| serde_json::from_str::<Value>(s).ok())
            .unwrap_or(value.clone());
        Some(
            parsed
                .as_str()
                .and_then(|s| serde_json::from_str::<Value>(s).ok())
                .unwrap_or(parsed),
        )
    } else {
        None
    }
}

async fn bind_row(
    db: &impl ConnectionTrait,
    store: &MediaStore,
    paths: &LegacyPaths,
    origins: &[String],
    table: &str,
    cursor: &str,
    payload: &Value,
) -> Result<(), MediaError> {
    let text = |key: &str| payload.get(key).and_then(Value::as_str);
    let id = || cursor.parse::<i32>().map_err(|_| MediaError::StoreFailed);
    let layout = parse_layout(table, payload);
    if table == "phantasi_items" && payload["content_md"].is_null() {
        return cite::bind_rss_item(db, id()?, payload, origins).await;
    }
    if is_wallpaper(table, cursor) {
        let stored = stored_wallpaper(payload);
        let candidate = wallpaper_candidate(db, paths, origins, payload).await?;
        import_cited(
            db,
            store,
            paths,
            origins,
            &Value::String(candidate.clone()),
            true,
        )
        .await?;
        // Same binder as saving the setting; clears stale references when unset.
        let published = cite::bind_and_publish_wallpaper(db, &candidate, origins).await?;
        if published != stored {
            db.execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "UPDATE configurations SET value = $1, updated_at = NOW() WHERE key = $2",
                [serde_json::json!(published).into(), WALLPAPER_KEY.into()],
            ))
            .await?;
        }
        return Ok(());
    }
    let urls = import_cited(
        db,
        store,
        paths,
        origins,
        layout.as_ref().unwrap_or(payload),
        true,
    )
    .await?;
    match table {
        "phantasi_note_docs" => {
            // Import history citations before the existing atomic draft/history binder.
            let history = db
                .query_all_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "SELECT snapshot FROM phantasi_note_history WHERE doc_id = $1",
                    [id()?.into()],
                ))
                .await?;
            for row in history {
                let value: Value = row.try_get("", "snapshot")?;
                import_cited(db, store, paths, origins, &value, true).await?;
            }
            cite::bind_note_draft(
                db,
                id()?,
                0,
                text("image"),
                text("content_md").unwrap_or(""),
                origins,
            )
            .await
        }
        "phantasi_items" => {
            cite::bind_note_published(
                db,
                id()?,
                text("image"),
                text("content_md").unwrap_or(""),
                origins,
            )
            .await
        }
        "phantasi_note_history" => {
            let snapshot = payload.get("snapshot").unwrap_or(&Value::Null);
            let refs = cite::references_from_fields(
                db,
                origins,
                snapshot.get("image").and_then(Value::as_str),
                snapshot
                    .get("content_md")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
                false,
            )
            .await?;
            cite::bind_consumer(
                db,
                "note_history",
                format!("{}:{}", payload["doc_id"], payload["revision"]),
                &refs,
            )
            .await
        }
        "agent_persona" => {
            let persona: crate::models::entities::agent_persona::Model =
                serde_json::from_value(payload.clone()).map_err(|_| MediaError::StoreFailed)?;
            let portrait = match persona.portrait_asset_id.as_deref() {
                Some(url) => Some(cite::publish_local_url(db, url, origins).await?),
                None => None,
            };
            let avatar = match persona.avatar_asset_id.as_deref() {
                Some(url) => Some(cite::publish_local_url(db, url, origins).await?),
                None => None,
            };
            let saved = crate::services::agent::merope::rewrite_persona_media_urls(
                db, persona, portrait, avatar,
            )
            .await
            .map_err(|error| {
                tracing::error!(%error, "media upgrade persona rewrite failed");
                MediaError::StoreFailed
            })?;
            cite::bind_persona(
                db,
                saved.portrait_asset_id.as_deref(),
                saved.avatar_asset_id.as_deref(),
                saved.visual_profile.as_ref(),
                origins,
            )
            .await
        }
        "configurations" => {
            let rewritten = cite::bind_and_publish_dashboard_layout(
                db,
                &layout.unwrap_or(Value::Null).to_string(),
                origins,
            )
            .await?;
            db.execute_raw(Statement::from_sql_and_values(DatabaseBackend::Postgres,
                "UPDATE configurations SET value = $1, updated_at = NOW() WHERE key = 'dashboard_layout'",
                [serde_json::json!(rewritten).into()])).await?;
            Ok(())
        }
        "tapp_runtime_registry" => {
            let task: crate::services::ai_task_registry::PersistedAiTask =
                serde_json::from_value(payload["payload"].clone())
                    .map_err(|_| MediaError::StoreFailed)?;
            cite::bind_ai_task(
                db,
                &task.snapshot.task_id,
                task.snapshot.result.as_ref().unwrap_or(&Value::Null),
                origins,
                chrono::DateTime::from_timestamp(task.retain_until, 0),
            )
            .await
        }
        "federation_delivery_queue" => {
            let activity = db
                .query_one_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "SELECT object_json FROM federation_activities WHERE id = $1",
                    [payload["activity_id"]
                        .as_i64()
                        .ok_or(MediaError::StoreFailed)?
                        .into()],
                ))
                .await?;
            let value = activity
                .map(|row| row.try_get::<Value>("", "object_json"))
                .transpose()?
                .unwrap_or(Value::Null);
            let urls = import_cited(db, store, paths, origins, &value, true).await?;
            let refs =
                cite::references_from_urls(db, origins, &urls, |i| format!("attachment:{i}"), true)
                    .await?;
            cite::bind_consumer(db, "federation_outbox", cursor, &refs).await
        }
        _ => {
            let public = table == "federation_activities";
            let refs = cite::references_from_urls(
                db,
                origins,
                &urls,
                |i| format!("attachment:{i}"),
                public,
            )
            .await?;
            let consumer = if public {
                "federation_activity"
            } else {
                "channel_message"
            };
            let identity = if public {
                text("activity_id").unwrap_or(cursor).to_owned()
            } else {
                format!("{table}:{cursor}")
            };
            cite::bind_consumer(db, consumer, identity, &refs).await
        }
    }
}
