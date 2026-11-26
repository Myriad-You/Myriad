//! 页面不在时任务的去向：任务当初的选择，结果有没有被页面取走，失败有没有向页面报过。
//! 重新打开或刷新的页面据此接上调用方最近的那个任务——还在跑的，做完没被取走的，失败还没
//! 报过的。这些记在任务记录上（`services::rig_jobs`），进程重启后还在；结果和失败各只自动
//! 交给页面一次，刷新不会把同一份结果再预检一遍。做完的结果在本进程的内存里，存了档的
//! 也能从存档取；两处都没有的（没存档、进程重启过或已经开始了下一个任务）不再交给页面。
//! 跑它的进程不在了而中断的任务，页面看到的和它不存在一样：在等它的页面照常按自己的选择
//! 重新开始，已经做完的生图和拆层从缓存里取。

use serde::Serialize;

use super::*;
use crate::services::rig_jobs::{JobRow, State as JobState};

/// What a job was asked for: the choices that decide what it draws,
/// decomposes and fits, and what the asking page puts on its result. A page
/// picking the job up shows and uses these, not what its own boxes say.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Asked {
    /// The head's turn keyed. Says nothing of a kept job used again.
    head: bool,
    /// A standing figure's body keyed. Says nothing of a kept job used again.
    body: bool,
    /// The bust split once, no keys.
    plain: bool,
    /// The kept job used instead of drawing and decomposing anew.
    from_archive: Option<FromArchive>,
    #[serde(flatten)]
    beside: Beside,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct FromArchive {
    id: String,
    /// Fitted again; handed over as it was otherwise.
    refit: bool,
}

/// What the asking page makes beside a job and puts on its result: the
/// redrawn expressions, the gesture arms. The job makes neither; it keeps the
/// choice for a page picking it up. None: the page did not say. It rides in
/// the query of the request that starts a job, not in its body: the body is
/// what the job does, and a kept job used again has none.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct Beside {
    expressions: Option<bool>,
    arms: Option<bool>,
}

impl Asked {
    /// What a started job does, as `start` reads its request: a bust split
    /// plain keys nothing, a bust has no body to key.
    pub(super) fn started(standing: bool, payload: &TurnKeyformsRequest, beside: Beside) -> Self {
        let plain = !standing && payload.plain;
        Self {
            head: payload.head && !plain,
            body: standing && payload.body,
            plain,
            from_archive: None,
            beside,
        }
    }

    /// A kept job used again, fitted anew or as it was.
    pub(super) fn kept(archive_id: &str, refit: bool, beside: Beside) -> Self {
        Self {
            from_archive: Some(FromArchive {
                id: archive_id.to_owned(),
                refit,
            }),
            beside,
            ..Self::default()
        }
    }
}

/// One of the two files a finished job hands out.
#[derive(Clone, Copy, Debug)]
pub(super) enum Part {
    Psd,
    Keys,
}

impl Part {
    /// The part as the job's record counts it delivered.
    fn name(self) -> &'static str {
        match self {
            Self::Psd => "psd",
            Self::Keys => "keys",
        }
    }
}

/// The failure of a job that was cancelled, as a page reads it.
pub(super) fn cancelled_error() -> Value {
    json!({ "error": "The turn keys job was cancelled", "code": "turn_keyforms_cancelled" })
}

/// A job's status as a page reads it: its stage `failed` when it was cancelled.
pub(super) fn job_status(job: &JobRow) -> Value {
    let progress = &job.progress;
    let (stage, error) = match job.state() {
        JobState::Cancelled => (json!("failed"), cancelled_error()),
        JobState::Succeeded => (json!("done"), Value::Null),
        JobState::Failed => (
            json!("failed"),
            job.error_detail
                .clone()
                .unwrap_or_else(|| json!({ "error": job.error_message, "code": job.error_code })),
        ),
        _ => (progress["stage"].clone(), Value::Null),
    };
    let seconds = (crate::clock::now() - job.created_at).num_seconds().max(0);
    json!({
        "jobId": job.id,
        "outfitId": job.input["outfitId"],
        "sourceMasterAssetId": job.input["sourceMasterAssetId"],
        "stage": stage,
        "phase": progress["phase"],
        "body": progress["body"],
        "done": progress["done"],
        "total": progress["total"],
        "seconds": seconds,
        "error": error,
        "archiveId": job.result_ref.as_deref().and_then(archive::of_result_ref),
        "asked": job.input["asked"],
    })
}

/// A finished job's two files: from this process, or from the archive its
/// record names. None: neither has them.
async fn result_of(job: &JobRow) -> Option<(Arc<Vec<u8>>, Arc<Vec<u8>>)> {
    if let Some(Some(files)) =
        live::with_job(&job.id, |live| live.psd.clone().zip(live.keyforms.clone()))
    {
        return Some(files);
    }
    let archive_id = job.result_ref.as_deref().and_then(archive::of_result_ref)?;
    let meta = archive::read_meta(archive_id).await?;
    let (baked, keyforms) = archive::read_result(&meta).await.ok()?;
    Some((Arc::new(baked), Arc::new(keyforms)))
}

/// Whether a page opened now has something of this job to pick up: it
/// still runs, it is done with a file no page has fetched and still to be
/// had, or it failed and no page was told. A page leaving is not the job's
/// end; a result fetched or a failure told once is not handed out a second
/// time, so that a reload does not preflight the same result again.
async fn awaits_a_page(job: &JobRow) -> bool {
    match job.state() {
        JobState::Succeeded => {
            !(job.delivered("psd") && job.delivered("keys")) && result_of(job).await.is_some()
        }
        JobState::Failed if job.interrupted() => false,
        JobState::Failed | JobState::Cancelled => !job.delivered("failure"),
        JobState::Queued | JobState::Running => true,
    }
}

/// The caller's job, as a page asking by its id may have it: none when it
/// is another caller's or was interrupted.
async fn job_of(db: &DatabaseConnection, caller: &str, id: &str) -> ApiResult<JobRow> {
    rig_jobs::get(db, id)
        .await
        .map_err(internal_error)?
        .filter(|job| job.caller == caller && !job.interrupted())
        .ok_or_else(served::missing_job)
}

/// The status of the job a page opened now picks up, if any: the caller's
/// latest, when it awaits a page.
pub(super) async fn to_pick_up(db: &DatabaseConnection, caller: &str) -> ApiResult<Option<Value>> {
    let Some(job) = rig_jobs::latest(db, caller).await.map_err(internal_error)? else {
        return Ok(None);
    };
    Ok(awaits_a_page(&job).await.then(|| job_status(&job)))
}

/// A job's status for the page asking by its id; a failure it reads is told.
pub(super) async fn status_for_its_page(
    db: &DatabaseConnection,
    caller: &str,
    id: &str,
) -> ApiResult<Value> {
    let job = job_of(db, caller, id).await?;
    if matches!(job.state(), JobState::Failed | JobState::Cancelled) && !job.delivered("failure") {
        rig_jobs::deliver(db, id, "failure")
            .await
            .map_err(internal_error)?;
    }
    Ok(job_status(&job))
}

/// One of a finished job's files for the page asking by its id, counted as fetched.
pub(super) async fn fetched(
    db: &DatabaseConnection,
    caller: &str,
    id: &str,
    part: Part,
) -> ApiResult<Arc<Vec<u8>>> {
    let job = job_of(db, caller, id).await?;
    if job.state() != JobState::Succeeded {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "The turn keys are not ready",
                "code": "turn_keyforms_not_ready"
            })),
        ));
    }
    let (psd, keys) = result_of(&job).await.ok_or_else(served::missing_job)?;
    rig_jobs::deliver(db, id, part.name())
        .await
        .map_err(internal_error)?;
    Ok(match part {
        Part::Psd => psd,
        Part::Keys => keys,
    })
}

#[cfg(test)]
#[path = "pickup_tests.rs"]
mod tests;
