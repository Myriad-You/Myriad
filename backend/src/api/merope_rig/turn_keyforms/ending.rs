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

/// The pauses between tries at writing a job's end: growing, then at the
/// keeper's cadence for as long as the process lives.
const END_PAUSES: [Duration; 4] = [
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(4),
    Duration::from_secs(8),
];
const END_PAUSE_AFTER: Duration = Duration::from_secs(10);

/// The job's end written to its record, with its steps as far as they can
/// be. While the database fails it, it is tried again for as long as the
/// process lives, the lease kept meanwhile (`live::run_job`): a finished,
/// paid result is never left without its record. A process stopping gives
/// up; its job is let go as interrupted (`rig_jobs::release_at_exit`). A
/// success over a job failed as interrupted while it ran on is recorded all
/// the same: its result was paid for and kept.
///
/// An end the database refuses for what it says (`rig_jobs::refused_for_good`)
/// the same way `REFUSED_TRIES` times in a row would be refused forever: the
/// job is then written failed with nothing but a generic code, so that its
/// place is freed.
pub(super) async fn end(db: &DatabaseConnection, id: &str, ending: rig_jobs::Ending) {
    let progress = live::with_job(id, Job::progress).unwrap_or_else(|| json!({}));
    let mut ending = ending;
    let mut progress = progress;
    let mut tries = 0usize;
    let mut refused: Option<(String, usize)> = None;
    loop {
        // A step that cannot be written does not hold back the end.
        if let Err(error) = live::write_steps(db, id).await {
            tracing::warn!(%error, job = %id, "turn keyforms steps could not be written at its end");
        }
        match rig_jobs::finish(db, id, ending.clone(), &progress).await {
            Ok(rig_jobs::Ended::Written) => return,
            Ok(rig_jobs::Ended::Reconciled) => {
                tracing::warn!(job = %id, "turn keyforms job ended after it was taken for interrupted; its success is recorded");
                return;
            }
            Ok(rig_jobs::Ended::Already) => {
                tracing::info!(job = %id, "turn keyforms job had ended already");
                return;
            }
            Err(error) if rig_jobs::stopping() => {
                tracing::error!(%error, job = %id, "turn keyforms end could not be written before the process stopped");
                return;
            }
            Err(error) => {
                let said = error.to_string();
                refused = match refused.take() {
                    Some((before, times)) if before == said => Some((before, times + 1)),
                    _ if rig_jobs::refused_for_good(&error) => Some((said, 1)),
                    _ => None,
                };
                if refused
                    .as_ref()
                    .is_some_and(|(_, times)| *times >= REFUSED_TRIES)
                {
                    tracing::error!(%error, job = %id, "turn keyforms end refused for good; written as a bare failure");
                    ending = unwritable();
                    progress = json!({});
                    refused = None;
                    continue;
                }
                let pause = END_PAUSES.get(tries).copied().unwrap_or(END_PAUSE_AFTER);
                tries += 1;
                tracing::warn!(%error, job = %id, tries, "turn keyforms end could not be written; trying again");
                tokio::time::sleep(pause).await;
            }
        }
    }
}

/// How often an end refused the same way for good is tried before it is
/// written as a bare failure (`unwritable`).
const REFUSED_TRIES: usize = 3;

/// A job's end that could not be written as it was: failed, with nothing of
/// it but a generic code.
fn unwritable() -> rig_jobs::Ending {
    rig_jobs::Ending::Failed {
        code: Some("turn_keyforms_end_unwritable".into()),
        message: Some("The turn keys job ended, but how could not be recorded".into()),
        detail: json!({
            "error": "The turn keys job ended, but how could not be recorded",
            "code": "turn_keyforms_end_unwritable"
        }),
    }
}
