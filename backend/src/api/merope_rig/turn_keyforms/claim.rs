//! 领一个任务：调用方提交了什么（`Submission`），它的 `Idempotency-Key`，以及在任务记录
//! 上领下它——调用方的键给过的那个、同一张主立绘还在跑的那个，或者在同时能跑的上限内新开一个。

use super::*;

/// What a caller asks a job for: who, under which key, of which master, and
/// how (`Asked`). The job's record keeps it as its input.
pub(super) struct Submission {
    pub(super) user_id: i32,
    pub(super) idempotency_key: Option<String>,
    pub(super) outfit_id: Option<String>,
    pub(super) source_master_asset_id: String,
    pub(super) source_generation_fingerprint: Option<String>,
    pub(super) asked: Asked,
}

impl Submission {
    fn caller(&self) -> String {
        rig_jobs::user_caller(self.user_id)
    }

    fn input(&self, phase: Phase) -> Value {
        json!({
            "kind": phase.as_str(),
            "outfitId": self.outfit_id,
            "sourceMasterAssetId": self.source_master_asset_id,
            "sourceGenerationFingerprint": self.source_generation_fingerprint,
            "asked": self.asked,
        })
    }

    /// The job the caller's key gave before, when it was given for the same
    /// submission; refused when it was given for another.
    pub(super) async fn given_before(
        &self,
        db: &DatabaseConnection,
        phase: Phase,
    ) -> ApiResult<Option<String>> {
        let Some(key) = &self.idempotency_key else {
            return Ok(None);
        };
        match rig_jobs::by_key(db, &self.caller(), key)
            .await
            .map_err(internal_error)?
        {
            Some((id, digest)) if digest == rig_jobs::digest(&self.input(phase)) => Ok(Some(id)),
            Some(_) => Err(key_reused()),
            None => Ok(None),
        }
    }
}

fn key_reused() -> ApiError {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(json!({
            "error": "Idempotency-Key was already used for a different request",
            "code": "idempotency_key_reused"
        })),
    )
}

/// The caller's key for one submission, from its `Idempotency-Key` header.
pub(super) fn idempotency_key(headers: &HeaderMap) -> ApiResult<Option<String>> {
    let Some(value) = headers.get("idempotency-key") else {
        return Ok(None);
    };
    let key = value
        .to_str()
        .ok()
        .map(str::trim)
        .filter(|key| !key.is_empty() && key.len() <= 255)
        .ok_or_else(|| bad_request("Idempotency-Key must be 1 to 255 visible characters"))?;
    Ok(Some(key.to_owned()))
}

/// A job claimed for `submission`: a new one this process runs (true), the
/// one its key gave before, or the one already running for the same master
/// (a reload, a second tab); a conflict while as many jobs run as may.
pub(super) async fn claim(
    db: &DatabaseConnection,
    submission: &Submission,
    phase: Phase,
    stage: Stage,
    total: u8,
    targets: live::Targets,
) -> ApiResult<(String, bool)> {
    let id = uuid::Uuid::new_v4().simple().to_string();
    let input = submission.input(phase);
    let subject = format!(
        "{}|{}",
        submission.outfit_id.as_deref().unwrap_or(""),
        submission.source_master_asset_id
    );
    let job = Job::new(id.clone(), phase, stage, total, targets);
    let claimed = rig_jobs::claim(
        db,
        rig_jobs::NewJob {
            id: id.clone(),
            kind: phase.as_str().to_owned(),
            caller: submission.caller(),
            owner_user_id: Some(submission.user_id),
            idempotency_key: submission.idempotency_key.clone(),
            request_digest: rig_jobs::digest(&input),
            subject: Some(subject),
            input,
            progress: job.progress(),
            ledger_task_id: rig_jobs::ledger_task_id(&id),
            runner: live::RUNNER.clone(),
        },
        live::at_once(),
    )
    .await;
    match claimed {
        Ok(rig_jobs::Claimed::New(id)) => {
            live::track(job);
            Ok((id, true))
        }
        Ok(rig_jobs::Claimed::Existing(id)) => Ok((id, false)),
        Err(rig_jobs::ClaimError::Busy) => Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Turn keys are already being made",
                "code": "turn_keyforms_in_progress"
            })),
        )),
        Err(rig_jobs::ClaimError::KeyReused) => Err(key_reused()),
        Err(rig_jobs::ClaimError::Db(error)) => Err(internal_error(error)),
    }
}

/// A job tracked as this process's alone, for tests of the pipeline's
/// progress: the only one, as a job claimed in a slot of one.
#[cfg(test)]
pub(super) fn claim_job(
    outfit_id: &Option<String>,
    _master_asset_id: &str,
    stage: Stage,
    total: u8,
) -> Result<(String, bool), ApiError> {
    let id = uuid::Uuid::new_v4().simple().to_string();
    let phase = if outfit_id.is_some() {
        Phase::Figure
    } else {
        Phase::Bust
    };
    live::JOBS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clear();
    live::track(Job::new(
        id.clone(),
        phase,
        stage,
        total,
        live::Targets::default(),
    ));
    Ok((id, true))
}

/// `read` of the one job tests track (`claim_job`).
#[cfg(test)]
pub(super) fn the_job<T>(read: impl FnOnce(&Job) -> T) -> T {
    let jobs = live::JOBS.lock().unwrap();
    read(jobs.values().next().expect("a job"))
}
