//! 任务的结束：做完的存档进主人的存储、交给导入；站点设定的任务在主立绘已经换了时记成失败；
//! 取消的就此停下；结束的样子和它的步骤写进任务记录。

use super::*;

/// What a finished job's result must still be true of to be kept and handed out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Holds {
    /// The site persona's master in the job's slot is still the one the job
    /// was made from (`ArchiveMeta`'s), as the import requires: the site's
    /// persona flow.
    PersonaMaster,
    /// Nothing outside the job: its result is its owner's as made. For
    /// tests of an owner other than the site.
    #[cfg(test)]
    AsMade,
}

/// Ends a job: kept in its owner's archive and handed out when what it
/// `Holds` still does, else failed.
pub(super) async fn finish(
    db: &DatabaseConnection,
    task_id: &str,
    result: ApiResult<Made>,
    meta: ArchiveMeta,
) {
    let Some((holds, storage)) = live::with_job(task_id, |job| (job.holds, job.owner.storage()))
    else {
        // Only a job this process runs is ended here: without its entry
        // nothing says what it must hold or whose storage it goes to.
        tracing::error!(job = %task_id, "turn keyforms job ended without this process running it");
        let error = json!({
            "error": "The turn keys job is not run by this process",
            "code": "turn_keyforms_untracked"
        });
        let ending = rig_jobs::Ending::Failed {
            code: Some("turn_keyforms_untracked".into()),
            message: error["error"].as_str().map(str::to_owned),
            detail: error,
        };
        end(db, task_id, ending).await;
        return;
    };
    let still_current = match holds {
        Holds::PersonaMaster => require_master_match(
            db,
            slot_of(meta.outfit_id.as_deref()),
            &meta.source_master_asset_id,
            meta.source_generation_fingerprint.as_deref(),
        )
        .await
        .map(|_| ()),
        #[cfg(test)]
        Holds::AsMade => Ok(()),
    };
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
                Some(()) => match archive::save(&storage, meta, &made.files).await {
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
