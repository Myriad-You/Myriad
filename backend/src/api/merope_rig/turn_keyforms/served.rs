//! 前端取用：任务进度、结果拆层与关键形，以及存档的列出、下载、删除、载入与重做拟合。

use super::*;

pub async fn get_turn_keyforms(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    require_merope_enabled().await?;
    require_owner(&claims, &db).await?;
    let job = JOB.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let job = job
        .as_ref()
        .filter(|job| job.id == id)
        .ok_or_else(missing_job)?;
    Ok(Json(job_status(job)))
}

/// The job still running, if any (there is one slot): a page opened or
/// reloaded while it runs picks its progress up again.
pub async fn running_turn_keyforms(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> ApiResult<Json<Value>> {
    require_merope_enabled().await?;
    require_owner(&claims, &db).await?;
    let job = JOB.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let running = job
        .as_ref()
        .filter(|job| !matches!(job.stage, Stage::Done | Stage::Failed));
    Ok(Json(json!({ "job": running.map(job_status) })))
}

pub(super) fn job_status(job: &Job) -> Value {
    json!({
        "jobId": job.id,
        "outfitId": job.outfit_id,
        "sourceMasterAssetId": job.master_asset_id,
        "stage": job.stage.as_str(),
        "phase": job.phase.as_str(),
        "body": job.body.map(|body| json!({
            "motion": body.motion,
            "stage": body.stage.as_str(),
            "done": body.done,
            "total": body.total,
        })),
        "done": job.done,
        "total": job.total,
        "seconds": job.started.elapsed().as_secs(),
        "error": job.error,
        "archiveId": job.archive_id,
    })
}

pub async fn get_turn_keyforms_psd(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    let psd = finished(&db, &claims, &id, |job| job.psd.clone()).await?;
    Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            HeaderValue::from_static("image/vnd.adobe.photoshop"),
        )
        .header(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-store, private"),
        )
        .header(
            "x-see-through-canvas",
            HeaderValue::from_static(see_through::CANVAS),
        )
        .body(Body::from(psd.as_ref().clone()))
        .map_err(internal_error)
}

pub async fn get_turn_keyforms_keys(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    let keys = finished(&db, &claims, &id, |job| job.keyforms.clone()).await?;
    Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        )
        .header(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-store, private"),
        )
        .body(Body::from(keys.as_ref().clone()))
        .map_err(internal_error)
}

pub(super) async fn finished(
    db: &DatabaseConnection,
    claims: &Claims,
    id: &str,
    pick: impl FnOnce(&Job) -> Option<Arc<Vec<u8>>>,
) -> ApiResult<Arc<Vec<u8>>> {
    require_merope_enabled().await?;
    require_owner(claims, db).await?;
    let job = JOB.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let job = job
        .as_ref()
        .filter(|job| job.id == id)
        .ok_or_else(missing_job)?;
    pick(job).ok_or_else(|| {
        (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "The turn keys are not ready",
                "code": "turn_keyforms_not_ready"
            })),
        )
    })
}

/// The configured See-through Space, and its name.
pub(super) async fn space_client() -> ApiResult<(see_through::SeeThroughClient, String)> {
    let (token, space) = {
        let config = crate::GLOBAL_DYNAMIC_CONFIG.read().await;
        (
            see_through::configured_hf_token(&config),
            see_through::configured_space(&config),
        )
    };
    let token =
        token.ok_or_else(|| see_through_error(see_through::SeeThroughError::NotConfigured))?;
    let space_name = space.name().to_owned();
    let client = see_through::SeeThroughClient::new(token, space)
        .await
        .map_err(see_through_error)?;
    Ok((client, space_name))
}

/// The kept jobs, newest first, each with whether its master is still the
/// current one of its slot (only those can be fitted again and imported).
pub async fn list_turn_archives(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> ApiResult<Json<Value>> {
    require_merope_enabled().await?;
    require_owner(&claims, &db).await?;
    let mut archives = Vec::new();
    for meta in archive::list().await {
        let current = require_master_match(
            &db,
            slot_of(meta.outfit_id.as_deref()),
            &meta.source_master_asset_id,
            meta.source_generation_fingerprint.as_deref(),
        )
        .await
        .is_ok();
        let mut entry = serde_json::to_value(&meta).map_err(internal_error)?;
        entry["current"] = json!(current);
        archives.push(entry);
    }
    Ok(Json(
        json!({ "archives": archives, "keepPerMaster": archive::KEEP_PER_MASTER }),
    ))
}

/// A kept job as one zip.
pub async fn download_turn_archive(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    require_merope_enabled().await?;
    require_owner(&claims, &db).await?;
    archive::read_meta(&id).await.ok_or_else(missing_archive)?;
    let name = id.clone();
    let zip = tokio::task::spawn_blocking(move || archive::zip_blocking(&name))
        .await
        .map_err(internal_error)?
        .map_err(internal_error)?;
    Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/zip"),
        )
        .header(
            header::CONTENT_DISPOSITION,
            HeaderValue::from_str(&format!("attachment; filename=\"turn-keys-{id}.zip\""))
                .map_err(internal_error)?,
        )
        .header(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-store, private"),
        )
        .body(Body::from(zip))
        .map_err(internal_error)
}

pub async fn delete_turn_archive(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    require_merope_enabled().await?;
    require_owner(&claims, &db).await?;
    if !archive::delete(&id).await.map_err(internal_error)? {
        return Err(missing_archive());
    }
    Ok(Json(json!({ "deleted": id })))
}

/// A kept job handed to the importer again as it was: a finished job made of
/// its decomposition and keys, fetched as any job. For a job whose page went
/// away before it was imported; its master must still be the slot's current one.
pub async fn load_turn_archive(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    require_merope_enabled().await?;
    require_owner(&claims, &db).await?;
    let kept = archive::read_meta(&id).await.ok_or_else(missing_archive)?;
    require_master_match(
        &db,
        slot_of(kept.outfit_id.as_deref()),
        &kept.source_master_asset_id,
        kept.source_generation_fingerprint.as_deref(),
    )
    .await?;
    let (baked, keyforms) = archive::read_result(&kept).await.map_err(internal_error)?;
    let (job_id, new) = claim_job(
        &kept.outfit_id,
        &kept.source_master_asset_id,
        Stage::Done,
        1,
    )?;
    if new {
        update(&job_id, |job| {
            job.phase = Phase::Load;
            job.done = 1;
            job.psd = Some(Arc::new(baked));
            job.keyforms = Some(Arc::new(keyforms));
            job.archive_id = Some(kept.id.clone());
        });
        tracing::info!(job = %job_id, archive = %kept.id, "turn keyforms loaded from an archive");
    }
    Ok(Json(json!({ "jobId": job_id })))
}

/// Fits a kept job again with the Space's current fit: no drawing, no
/// decomposition. Its master must still be the slot's current one, as the
/// import requires; the result is kept as a new archive and fetched as any job.
pub async fn refit_turn_archive(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    require_merope_enabled().await?;
    require_owner(&claims, &db).await?;
    let kept = archive::read_meta(&id).await.ok_or_else(missing_archive)?;
    require_master_match(
        &db,
        slot_of(kept.outfit_id.as_deref()),
        &kept.source_master_asset_id,
        kept.source_generation_fingerprint.as_deref(),
    )
    .await?;
    let files = archive::read_files(&kept).await.map_err(internal_error)?;
    let (client, space_name) = space_client().await?;
    let (job_id, new) = claim_job(
        &kept.outfit_id,
        &kept.source_master_asset_id,
        Stage::Fitting,
        1,
    )?;
    if !new {
        return Ok(Json(json!({ "jobId": job_id })));
    }
    update(&job_id, |job| job.phase = Phase::Refit);
    tracing::info!(job = %job_id, archive = %kept.id, "turn keyforms fitted again from an archive");
    let meta = ArchiveMeta {
        id: archive::new_id(),
        created_at: crate::clock::now(),
        parent: Some(kept.id.clone()),
        space: space_name,
        bytes: 0,
        ..kept.clone()
    };
    let task_id = job_id.clone();
    tokio::spawn(async move {
        let pictures: [(Vec<u8>, String); 5] =
            std::iter::once((files.master.clone(), kept.master_media_type.clone()))
                .chain(
                    files
                        .turned
                        .iter()
                        .map(|png| (png.clone(), "image/png".to_string())),
                )
                .collect::<Vec<_>>()
                .try_into()
                .expect("five pictures");
        let figure = files.figure;
        // The body's keys are not fitted again: they go along as they were.
        let body = body_turn::body_of(&files.keyforms);
        let result = fit(&client, &files.decompositions, &pictures)
            .await
            .map(|output| Made {
                files: ArchiveFiles {
                    keyforms: output.keyforms,
                    baked: output.psd,
                    figure: None,
                    ..files
                },
                master_media_type: kept.master_media_type.clone(),
                redrawn: kept.redrawn.clone(),
                figure: None,
                keep: true,
            });
        // A figure keyed by its head goes back into the figure it was kept with.
        let result = match (result, kept.figure.clone(), figure) {
            (Ok(made), Some(meta), Some(files)) => put_back(&client, made, meta, files).await,
            (result, _, _) => result,
        }
        .map(|mut made| {
            made.files.keyforms = body_turn::with_body_keys(made.files.keyforms, body.as_ref());
            made
        });
        finish(&db, &task_id, result, meta).await;
    });
    Ok(Json(json!({ "jobId": job_id })))
}

pub(super) fn missing_archive() -> ApiError {
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "error": "No such turn keys archive", "code": "turn_archive_missing" })),
    )
}

pub(super) fn missing_job() -> ApiError {
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "error": "No such turn keys job", "code": "turn_keyforms_missing" })),
    )
}
