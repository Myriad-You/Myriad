//! 前端取用：任务进度、结果拆层与关键形、取消，以及存档的列出、下载、删除、载入与重做拟合。

use super::{pickup::Part, *};

/// The owner, as the caller whose jobs these are.
async fn owner_caller(db: &DatabaseConnection, claims: &Claims) -> ApiResult<String> {
    require_merope_enabled().await?;
    let user_id = require_owner(claims, db).await?;
    Ok(rig_jobs::user_caller(user_id))
}

pub async fn get_turn_keyforms(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let caller = owner_caller(&db, &claims).await?;
    Ok(Json(pickup::status_for_its_page(&db, &caller, &id).await?))
}

/// Cancels a job: it stops where it stands, what it was waiting on let go,
/// and ends as cancelled. A job that has ended stays as it ended. Its status.
pub async fn cancel_turn_keyforms(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let caller = owner_caller(&db, &claims).await?;
    Ok(Json(cancel(&db, &caller, &id).await?))
}

/// A job's record as the generation service keeps it: what it was asked,
/// where it stands, each step with its time and compute, and what its paid
/// calls cost as the ledger has them, those that completed apart from those
/// that failed.
pub async fn get_turn_keyforms_record(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let caller = owner_caller(&db, &claims).await?;
    Ok(Json(record(&db, &caller, &id).await?))
}

pub(super) async fn record(db: &DatabaseConnection, caller: &str, id: &str) -> ApiResult<Value> {
    let job = rig_jobs::get(db, id)
        .await
        .map_err(internal_error)?
        .filter(|job| job.caller == caller)
        .ok_or_else(missing_job)?;
    let steps = rig_jobs::steps(db, id).await.map_err(internal_error)?;
    let spend = rig_jobs::spend(db, &job).await.map_err(internal_error)?;
    Ok(json!({
        "jobId": job.id,
        "kind": job.kind,
        "ledgerTaskId": job.ledger_task_id,
        "state": job.state().as_str(),
        "input": job.input,
        "progress": job.progress,
        "attempts": job.attempts,
        "runner": job.runner,
        "error": job.error_code.as_ref().map(|code| json!({
            "code": code,
            "message": job.error_message,
        })),
        "resultRef": job.result_ref,
        "cancelRequestedAt": job.cancel_requested_at,
        "createdAt": job.created_at,
        "startedAt": job.started_at,
        "endedAt": job.ended_at,
        "steps": steps.iter().map(|step| json!({
            "name": step.name,
            "state": step.state.as_str(),
            "attempts": step.attempts,
            "backend": step.backend,
            "backendTarget": step.backend_target,
            "startedAt": step.started_at,
            "endedAt": step.ended_at,
            "wallSeconds": step.wall_seconds,
            "gpuSeconds": step.gpu_seconds,
        })).collect::<Vec<_>>(),
        "spend": {
            "completed": calls_json(&spend.completed),
            "failed": calls_json(&spend.failed),
        },
    }))
}

/// Paid calls of one outcome, as the record reads them.
fn calls_json(calls: &rig_jobs::Calls) -> Value {
    json!({
        "calls": calls.calls,
        "inputTokens": calls.input_tokens,
        "outputTokens": calls.output_tokens,
        "pricedMicroUsd": calls.priced_micro_usd,
        "unpricedCalls": calls.unpriced_calls,
    })
}

pub(super) async fn cancel(db: &DatabaseConnection, caller: &str, id: &str) -> ApiResult<Value> {
    let job = rig_jobs::request_cancel(db, caller, id)
        .await
        .map_err(internal_error)?
        .filter(|job| !job.interrupted())
        .ok_or_else(missing_job)?;
    if !job.state().ended() {
        tracing::info!(job = %id, "turn keyforms cancel asked");
        live::tell_cancel(id);
    }
    Ok(pickup::job_status(&job))
}

/// The job a page opened or reloaded now picks up, if any (there is one
/// slot): one still running, whose progress shows again; one done while no
/// page was there, whose result is fetched as any job's; one that failed
/// with no page there to tell. The page takes it only when it is of the
/// portrait and set it shows, and then asks for it by its id.
pub async fn running_turn_keyforms(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> ApiResult<Json<Value>> {
    let caller = owner_caller(&db, &claims).await?;
    Ok(Json(
        json!({ "job": pickup::to_pick_up(&db, &caller).await? }),
    ))
}

pub async fn get_turn_keyforms_psd(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    let psd = finished(&db, &claims, &id, Part::Psd).await?;
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
    let keys = finished(&db, &claims, &id, Part::Keys).await?;
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

/// One of a finished job's two files, for the owner's page that fetches it.
async fn finished(
    db: &DatabaseConnection,
    claims: &Claims,
    id: &str,
    part: Part,
) -> ApiResult<Arc<Vec<u8>>> {
    let caller = owner_caller(db, claims).await?;
    pickup::fetched(db, &caller, id, part).await
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
    Query(beside): Query<Beside>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    require_merope_enabled().await?;
    let user_id = require_owner(&claims, &db).await?;
    let submission = kept_submission(user_id, &id, false, beside, &headers).await?;
    if let Some(job_id) = submission.given_before(&db, Phase::Load).await? {
        return Ok(Json(json!({ "jobId": job_id })));
    }
    let kept = archive::read_meta(&id).await.ok_or_else(missing_archive)?;
    require_master_match(
        &db,
        slot_of(kept.outfit_id.as_deref()),
        &kept.source_master_asset_id,
        kept.source_generation_fingerprint.as_deref(),
    )
    .await?;
    let (baked, keyforms) = archive::read_result(&kept).await.map_err(internal_error)?;
    let (job_id, new) = claim(
        &db,
        &submission,
        Phase::Load,
        Stage::Fitting,
        1,
        live::Targets::default(),
    )
    .await?;
    if new {
        update(&job_id, |job| {
            job.stage = Stage::Done;
            job.done = 1;
            job.psd = Some(Arc::new(baked));
            job.keyforms = Some(Arc::new(keyforms));
            job.archive_id = Some(kept.id.clone());
        });
        let result_ref = Some(archive::result_ref(&kept.id));
        // Written off the request: a database that fails it is tried again
        // for as long as it takes (`end`), and a page polls meanwhile.
        rig_jobs::hold(&job_id);
        let ending = (db.clone(), job_id.clone());
        tokio::spawn(async move {
            let (db, job_id) = ending;
            update_quietly(&job_id, |job| job.ending = true);
            end(&db, &job_id, rig_jobs::Ending::Succeeded { result_ref }).await;
            rig_jobs::let_go(&job_id);
        });
        tracing::info!(job = %job_id, archive = %kept.id, "turn keyforms loaded from an archive");
    }
    Ok(Json(json!({ "jobId": job_id })))
}

/// A kept job used again, as its caller submits it: of the kept job's slot
/// and master, fitted anew or handed over as it was.
async fn kept_submission(
    user_id: i32,
    archive_id: &str,
    refit: bool,
    beside: Beside,
    headers: &HeaderMap,
) -> ApiResult<Submission> {
    let kept = archive::read_meta(archive_id)
        .await
        .ok_or_else(missing_archive)?;
    Ok(Submission {
        user_id,
        idempotency_key: idempotency_key(headers)?,
        outfit_id: kept.outfit_id,
        source_master_asset_id: kept.source_master_asset_id,
        source_generation_fingerprint: kept.source_generation_fingerprint,
        asked: Asked::kept(archive_id, refit, beside),
    })
}

/// Fits a kept job again with the Space's current fit: no drawing, no
/// decomposition. Its master must still be the slot's current one, as the
/// import requires; the result is kept as a new archive and fetched as any job.
pub async fn refit_turn_archive(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
    Query(beside): Query<Beside>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    require_merope_enabled().await?;
    let user_id = require_owner(&claims, &db).await?;
    let submission = kept_submission(user_id, &id, true, beside, &headers).await?;
    if let Some(job_id) = submission.given_before(&db, Phase::Refit).await? {
        return Ok(Json(json!({ "jobId": job_id })));
    }
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
    let targets = live::Targets {
        space: Some(space_name.clone()),
        image: None,
    };
    let (job_id, new) = claim(&db, &submission, Phase::Refit, Stage::Fitting, 1, targets).await?;
    if !new {
        return Ok(Json(json!({ "jobId": job_id })));
    }
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
    let runs = db.clone();
    tokio::spawn(async move {
        let work = async {
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
            match (result, kept.figure.clone(), figure) {
                (Ok(made), Some(meta), Some(files)) => {
                    put_back(&task_id, &client, made, meta, files).await
                }
                (result, _, _) => result,
            }
            .map(|mut made| {
                made.files.keyforms = body_turn::with_body_keys(made.files.keyforms, body.as_ref());
                made
            })
        };
        live::run_job(runs, task_id.clone(), work, meta).await;
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
