//! 领一个任务（`Claim`）：归谁、谁用哪个 `Idempotency-Key` 提交的、按调用方的说法要的是什么、
//! 步骤从什么跑（`JobInput`），以及在任务记录上领下它——调用方的键给过的那个、同一张图还在跑的
//! 那个，或者在同时能跑的上限内新开一个。站点设定的页面是调用方之一（`Submission`）。

use super::*;

/// A job as it is claimed: whose (its caller is its owner's), under which key, what was
/// asked in the caller's terms (its key's digest is of it, its page reads it
/// back), what joins a job already running, what its steps run from, and
/// what its result must still hold at its end.
pub(super) struct Claim {
    pub(super) owner: rig_jobs::Owner,
    pub(super) idempotency_key: Option<String>,
    pub(super) request: Value,
    pub(super) subject: String,
    pub(super) input: JobInput,
    pub(super) holds: Holds,
}

/// What the site's persona page asks a job for: who, under which key, of
/// which master, and how (`Asked`). The job's record keeps it as what was
/// asked; its steps run from the master's picture it names.
pub(super) struct Submission {
    pub(super) user_id: i32,
    pub(super) idempotency_key: Option<String>,
    pub(super) outfit_id: Option<String>,
    pub(super) source_master_asset_id: String,
    pub(super) source_generation_fingerprint: Option<String>,
    pub(super) asked: Asked,
}

impl Submission {
    fn owner(&self) -> rig_jobs::Owner {
        rig_jobs::Owner::Site {
            user_id: self.user_id,
        }
    }

    fn caller(&self) -> String {
        self.owner().caller()
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

    /// What the job's steps run from: the master's picture in the site's
    /// media, made as `phase` asks; or the kept job it uses again.
    pub(super) fn job_input(&self, phase: Phase) -> JobInput {
        if let Some((archive_id, refit)) = self.asked.kept_job() {
            return JobInput::Kept {
                archive_id: archive_id.to_owned(),
                refit,
            };
        }
        let make = match phase {
            Phase::Plain => Make::Plain,
            Phase::Figure | Phase::Head | Phase::Body => {
                let keys = self.asked.figure_keys();
                Make::Figure {
                    head: keys.head,
                    body: keys.body,
                }
            }
            Phase::Bust | Phase::Refit | Phase::Load => Make::Bust,
        };
        JobInput::Picture {
            picture: PictureRef::SiteMedia {
                url: self.source_master_asset_id.clone(),
            },
            make,
        }
    }

    /// The submission as the site's owner claims it: the site's job, its
    /// result held to the persona's master at its end.
    pub(super) fn claim(&self, phase: Phase) -> Claim {
        Claim {
            owner: self.owner(),
            idempotency_key: self.idempotency_key.clone(),
            request: self.input(phase),
            subject: format!(
                "{}|{}",
                self.outfit_id.as_deref().unwrap_or(""),
                self.source_master_asset_id
            ),
            input: self.job_input(phase),
            holds: Holds::PersonaMaster,
        }
    }

    /// The job the caller's key gave before, when it was given for the same
    /// submission and has not failed or been cancelled (the claim then lets
    /// the key go and starts anew); refused when it was given for another.
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
            // A key whose job failed or was cancelled is free for any request.
            Some(keyed) if !keyed.holds() => Ok(None),
            Some(keyed) if keyed.request_digest != rig_jobs::digest(&self.input(phase)) => {
                Err(key_reused())
            }
            Some(keyed) => Ok(Some(keyed.job_id)),
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
/// The same key from the same caller gives the same job for a day
/// (`rig_jobs::KEY_TTL`), however it ended unless it failed or was
/// cancelled: then the key asks anew. A submission that joins a job already
/// running for the same master keeps its key on that job. The same key with
/// another request is refused (422 `idempotency_key_reused`).
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
    claim_for(db, submission.claim(phase), phase, stage, total, targets).await
}

/// A job claimed as `claim` has it: a new one this process runs (true), the
/// one its key gave before, or the one already running for the same subject;
/// a conflict while as many jobs run as may. The record keeps whose it is
/// and what its steps run from.
pub(super) async fn claim_for(
    db: &DatabaseConnection,
    claim: Claim,
    phase: Phase,
    stage: Stage,
    total: u8,
    targets: live::Targets,
) -> ApiResult<(String, bool)> {
    let id = uuid::Uuid::new_v4().simple().to_string();
    let job = Job::new(
        id.clone(),
        phase,
        stage,
        total,
        targets,
        claim.owner.clone(),
        claim.holds,
    );
    let claimed = rig_jobs::claim(
        db,
        rig_jobs::NewJob {
            id: id.clone(),
            kind: phase.as_str().to_owned(),
            owner: claim.owner,
            idempotency_key: claim.idempotency_key,
            request_digest: rig_jobs::digest(&claim.request),
            subject: Some(claim.subject),
            input: claim.request,
            pipeline_input: claim.input.to_value(),
            progress: job.progress(),
            ledger_task_id: rig_jobs::ledger_task_id(&id),
            runner: live::runner(),
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
        // This process is going away: a start is refused, as any request
        // while it is down. A page polling a job here reads it running until
        // it is gone, so it never starts one again on this process.
        Err(rig_jobs::ClaimError::Stopping) => Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "Service unavailable", "code": "service_unavailable" })),
        )),
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
        rig_jobs::Owner::Site { user_id: 7 },
        Holds::PersonaMaster,
    ));
    Ok((id, true))
}

/// `read` of the one job tests track (`claim_job`).
#[cfg(test)]
pub(super) fn the_job<T>(read: impl FnOnce(&Job) -> T) -> T {
    let jobs = live::JOBS.lock().unwrap();
    read(jobs.values().next().expect("a job"))
}
