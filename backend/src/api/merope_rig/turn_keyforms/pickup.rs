//! 页面不在时任务的去向：任务当初的选择，结果有没有被页面取走，失败有没有向页面报过。
//! 重新打开或刷新的页面据此接上任务——还在跑的，做完没被取走的，失败还没报过的。
//! 这些只记在内存里的任务槽上，进程重启后不留；结果和失败各只自动交给页面一次，
//! 刷新不会把同一份结果再预检一遍。

use serde::Serialize;

use super::*;

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

/// What a page has had of a job's end. Only a page asking for the job by its
/// id counts: `to_pick_up` answers pages of any portrait, and the one whose
/// job it is asks by id next.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Told {
    psd: bool,
    keys: bool,
    failure: bool,
}

/// One of the two files a finished job hands out.
#[derive(Clone, Copy, Debug)]
pub(super) enum Part {
    Psd,
    Keys,
}

impl Job {
    /// Whether a page opened now has something of this job to pick up: it
    /// still runs, it is done with a file no page has fetched, or it failed
    /// and no page was told. A page leaving is not the job's end; a result
    /// fetched or a failure told once is not handed out a second time, so
    /// that a reload does not preflight the same result again.
    fn awaits_a_page(&self) -> bool {
        match self.stage {
            Stage::Done => !(self.told.psd && self.told.keys),
            Stage::Failed => !self.told.failure,
            _ => true,
        }
    }
}

/// The status of the job a page opened now picks up, if any (there is one slot).
pub(super) fn to_pick_up() -> Option<Value> {
    let job = JOB.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    job.as_ref()
        .filter(|job| job.awaits_a_page())
        .map(served::job_status)
}

/// A job's status for the page asking by its id; a failure it reads is told.
pub(super) fn status_for_its_page(id: &str) -> ApiResult<Value> {
    let mut job = JOB.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let job = job
        .as_mut()
        .filter(|job| job.id == id)
        .ok_or_else(served::missing_job)?;
    job.told.failure |= job.stage == Stage::Failed;
    Ok(served::job_status(job))
}

/// One of a finished job's files for the page asking by its id, counted as fetched.
pub(super) fn fetched(id: &str, part: Part) -> ApiResult<Arc<Vec<u8>>> {
    let mut job = JOB.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let job = job
        .as_mut()
        .filter(|job| job.id == id)
        .ok_or_else(served::missing_job)?;
    let file = match part {
        Part::Psd => job.psd.clone(),
        Part::Keys => job.keyforms.clone(),
    }
    .ok_or_else(|| {
        (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "The turn keys are not ready",
                "code": "turn_keyforms_not_ready"
            })),
        )
    })?;
    match part {
        Part::Psd => job.told.psd = true,
        Part::Keys => job.told.keys = true,
    }
    Ok(file)
}

#[cfg(test)]
#[path = "pickup_tests.rs"]
mod tests;
