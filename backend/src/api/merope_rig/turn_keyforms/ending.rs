//! 任务的结束：做完的存档、交给导入，主立绘已经换了的记成失败；取消的就此停下；结束的样子
//! 和它的步骤写进任务记录。

use super::*;

/// Ends a job: kept in the archive and handed out when the master is still
/// the one it was made for, else failed.
pub(super) async fn finish(
    db: &DatabaseConnection,
    task_id: &str,
    result: ApiResult<Made>,
    meta: ArchiveMeta,
) {
    let still_current = require_master_match(
        db,
        slot_of(meta.outfit_id.as_deref()),
        &meta.source_master_asset_id,
        meta.source_generation_fingerprint.as_deref(),
    )
    .await;
    match (result, still_current) {
        (Ok(made), Ok(_)) => {
            let meta = ArchiveMeta {
                master_media_type: made.master_media_type.clone(),
                redrawn: made.redrawn.clone(),
                figure: made.figure.clone(),
                body_motions: made
                    .figure
                    .is_some()
                    .then(|| body_turn::motions_of(&made.files.keyforms)),
                ..meta
            };
            let archive_id = match made.keep.then_some(()) {
                Some(()) => match archive::save(meta, &made.files).await {
                    Ok(id) => Some(id),
                    Err(error) => {
                        tracing::warn!(%error, job = %task_id, "turn keys could not be archived");
                        None
                    }
                },
                None => None,
            };
            let ArchiveFiles {
                keyforms, baked, ..
            } = made.files;
            let result_ref = archive_id.as_deref().map(archive::result_ref);
            update(task_id, |job| {
                job.stage = Stage::Done;
                job.psd = Some(Arc::new(baked));
                job.keyforms = Some(Arc::new(keyforms));
                job.archive_id = archive_id;
                tracing::info!(job = %job.id, seconds = job.started.elapsed().as_secs(), archive = ?job.archive_id, "turn keyforms done");
            });
            end(db, task_id, rig_jobs::Ending::Succeeded { result_ref }).await;
        }
        (Err((_, Json(error))), _) | (_, Err((_, Json(error)))) => {
            update(task_id, |job| {
                tracing::warn!(job = %job.id, ?error, "turn keyforms failed");
                job.stage = Stage::Failed;
                job.error = Some(error.clone());
            });
            let ending = rig_jobs::Ending::Failed {
                code: error["code"].as_str().map(str::to_owned),
                message: error["error"].as_str().map(str::to_owned),
                detail: error,
            };
            end(db, task_id, ending).await;
        }
    }
}

/// A cancelled job's end: what it was waiting on let go (`live::run_job`).
pub(super) async fn cancelled(db: &DatabaseConnection, id: &str) {
    update(id, |job| {
        tracing::info!(job = %job.id, seconds = job.started.elapsed().as_secs(), "turn keyforms cancelled");
        job.stage = Stage::Failed;
        job.error = Some(pickup::cancelled_error());
    });
    end(db, id, rig_jobs::Ending::Cancelled).await;
}

/// The job's end written to its record, with its steps.
pub(super) async fn end(db: &DatabaseConnection, id: &str, ending: rig_jobs::Ending) {
    live::write_steps(db, id).await;
    let progress = live::with_job(id, Job::progress).unwrap_or_else(|| json!({}));
    match rig_jobs::finish(db, id, ending, &progress).await {
        Ok(true) => {}
        Ok(false) => tracing::warn!(job = %id, "turn keyforms job had ended already"),
        Err(error) => tracing::warn!(%error, job = %id, "turn keyforms end could not be written"),
    }
}
