//! Preflight checks (spec §6 pre-check). Run BEFORE entering maintenance mode so failures
//! never cause downtime.
//!
//! Two modes:
//! - **Release**: prefer GitHub `release.json` (digests, cosign, min_from_version). When the
//!   manifest is unavailable (404 / no token / network / missing asset), require tag consent to pull
//!   `BACKEND_IMAGE`/`FRONTEND_IMAGE` tagged with the release version from Docker Hub — same
//!   image naming as commit mode. Cosign/schema failures still hard-fail (no silent skip).
//! - **Commit**: resolve to `dev-<sha>`, pull and verify CI image signatures; missing signatures require consent.
//!
//! Direction gates (fail-closed):
//! - pure upgrade → ok
//! - pure downgrade → requires `allow_downgrade`
//! - diverged → requires `allow_diverged` / `allow_risk`
//! - **commit/dev**: unknown ancestry does **not** require `allow_unknown` (build-time
//!   newer / different tag is enough). Release + full manifest still treats unknown as risk;
//!   release Docker Hub fallback (no git compare) matches commit without ancestry.
//! - irreversible migration + downgrade → requires both flags (manifest path only)
//!
//! Local gates (before image pull when possible):
//! - env keys, pgdata disk (bundled)
//! - **local environment**: tag vars, writable state/.env, Docker API (via docker-guard)
//! - **compose contract**: required services / container_name / bundled pgdata bind /
//!   running Compose project labels (no orchestration change — inspect only)
//! - **compose network allowlist** (docker-guard parity)

use std::sync::Arc;

use tracing::{info, warn};

use crate::docker::network_allowlist::{
    find_disallowed_attachments, networks_for_services, NetworkAllowlist, UPDATE_RECREATE_SERVICES,
};
use crate::env_file::EnvFile;
use crate::error::{Result, UpdaterError};
use crate::release::{CommitRelation, Manifest};
use crate::state::UpdateTrust;
use crate::version::{DeployTag, DeployTagKind, MyriadVersion, UpdateMode};
use crate::worker::Worker;
use crate::SUPPORTED_RELEASE_SCHEMA;

pub struct PreflightReport {
    /// Present only for release-mode updates.
    pub manifest: Option<Manifest>,
    pub from_version: Option<DeployTag>,
    /// Tag actually written to MYRIAD_TAG (commit mode: always `dev-<sha>`).
    pub target: DeployTag,
    /// Full source commit for the target image set.
    pub target_commit_sha: Option<String>,
    pub backend_digest: String,
    pub frontend_digest: String,
    pub estimated_seconds: u32,
    pub is_downgrade: bool,
    pub is_diverged: bool,
}

/// Operator confirmation flags. `allow_risk` is a backward-compatible umbrella that
/// enables downgrade/ancestry/migration exceptions, never tag-install consent.
#[derive(Debug, Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct RiskFlags {
    pub allow_downgrade: bool,
    pub allow_diverged: bool,
    pub allow_unknown: bool,
    pub allow_irreversible: bool,
    pub allow_tag_install: bool,
}

impl RiskFlags {
    /// Build from granular flags + optional umbrella `allow_risk`.
    pub fn from_api(
        allow_downgrade: bool,
        allow_risk: bool,
        allow_diverged: Option<bool>,
        allow_unknown: Option<bool>,
        allow_irreversible: Option<bool>,
    ) -> Self {
        Self {
            allow_downgrade: allow_downgrade || allow_risk,
            allow_diverged: allow_diverged.unwrap_or(allow_risk),
            allow_unknown: allow_unknown.unwrap_or(allow_risk),
            allow_irreversible: allow_irreversible.unwrap_or(allow_risk),
            allow_tag_install: false,
        }
    }
}

pub(crate) fn new_trust(path: &str, verification: &str, reason: Option<String>) -> UpdateTrust {
    UpdateTrust {
        trust_path: path.into(),
        verification: verification.into(),
        reason,
        commit_sha: None,
        backend: None,
        frontend: None,
    }
}

pub(crate) fn record_trust(worker: &Worker, job_id: &str, trust: UpdateTrust) -> Result<()> {
    let mut job = worker.state().read_job(job_id)?;
    let line = format!(
        "audit: update_trust job={} evidence={}",
        job_id,
        serde_json::to_string(&trust)?
    );
    job.trust = Some(trust);
    worker.state().write_job(&job)?;
    worker.state().append_audit(&line)?;
    worker.state().append_history(&line)?;
    Ok(())
}

fn record_image(
    worker: &Worker,
    job_id: &str,
    component: &str,
    image: &str,
    digest: &str,
) -> Result<()> {
    let mut trust = worker
        .state()
        .read_job(job_id)?
        .trust
        .ok_or_else(|| UpdaterError::State("missing update trust context".into()))?;
    let evidence = Some(crate::release::ImageRef {
        r#ref: image.into(),
        digest: digest.into(),
    });
    match component {
        "backend" => trust.backend = evidence,
        "frontend" => trust.frontend = evidence,
        _ => return Err(UpdaterError::State("unknown image component".into())),
    }
    record_trust(worker, job_id, trust)
}

fn require_tag_install(risk: RiskFlags, reason: &str) -> Result<()> {
    if risk.allow_tag_install {
        Ok(())
    } else {
        Err(UpdaterError::TagInstallRequired(reason.into()))
    }
}

fn require_formal_tag(target: &DeployTag) -> Result<()> {
    if target
        .as_release()
        .is_some_and(|v| v.semver().pre.is_empty())
    {
        Ok(())
    } else {
        Err(UpdaterError::InvalidInput(
            "Docker Hub formal tag path requires vX.Y.Z without a prerelease suffix".into(),
        ))
    }
}

pub(crate) fn validate_image_repo(repo: &str) -> Result<()> {
    // A repository, never a caller-controlled tag/digest/URL or a whitespace-bearing value.
    let valid = regex::Regex::new(r"^(?:[a-z0-9.-]+(?::[0-9]+)?/)?[a-z0-9]+(?:[._-][a-z0-9]+)*(?:/[a-z0-9]+(?:[._-][a-z0-9]+)*)*$").unwrap();
    if valid.is_match(repo) {
        Ok(())
    } else {
        Err(UpdaterError::Precondition(
            "BACKEND_IMAGE / FRONTEND_IMAGE must be repository names without a tag or digest"
                .into(),
        ))
    }
}

pub async fn run(
    worker: Arc<Worker>,
    job_id: &str,
    target: &DeployTag,
    mode: UpdateMode,
    risk: RiskFlags,
) -> Result<PreflightReport> {
    // Route by target shape, not only by UI mode:
    // - formal releases always use release preflight (manifest first, explicit tag path otherwise)
    // - dev-<sha> / branch tips always use commit path
    // (Dev channel may list both; never install a release tag as a commit.)
    let effective_mode = if target.is_release() {
        UpdateMode::Release
    } else if mode == UpdateMode::Release {
        return Err(UpdaterError::InvalidInput(format!(
            "release mode requires a vX.Y.Z target, got {} (use mode=commit for CI tags)",
            target.as_str()
        )));
    } else {
        UpdateMode::Commit
    };
    let report = match effective_mode {
        UpdateMode::Release => run_release(worker.clone(), job_id, target, risk).await,
        UpdateMode::Commit => run_commit(worker.clone(), job_id, target, risk).await,
    }?;
    let mut trust = worker
        .state()
        .read_job(job_id)?
        .trust
        .ok_or_else(|| UpdaterError::State("missing completed preflight evidence".into()))?;
    trust.commit_sha = report.target_commit_sha.clone();
    record_trust(&worker, job_id, trust)?;
    Ok(report)
}

async fn run_release(
    worker: Arc<Worker>,
    job_id: &str,
    target: &DeployTag,
    risk: RiskFlags,
) -> Result<PreflightReport> {
    let release = target.as_release().ok_or_else(|| {
        UpdaterError::InvalidInput(format!(
            "release mode requires a vX.Y.Z target, got {}",
            target.as_str()
        ))
    })?;

    info!(target = %release, "preflight(release): fetching GitHub release.json");
    match try_fetch_release_manifest(worker.as_ref(), release.as_str()).await {
        Ok(Some((gh, manifest, verification))) => {
            run_release_with_manifest(worker, job_id, target, risk, gh, manifest, verification)
                .await
        }
        Ok(None) => run_release_via_dockerhub(worker, job_id, target, risk).await,
        Err(e) => Err(e),
    }
}

/// Attempt to download + verify `release.json`.
///
/// - `Ok(Some)` — verified manifest ready for the full release path
/// - `Ok(None)` — GitHub release unavailable (404/401/network/no asset); caller may fall back
/// - `Err` — hard failure (cosign, invalid manifest body); do **not** fall back
async fn try_fetch_release_manifest(
    worker: &Worker,
    tag: &str,
) -> Result<Option<(crate::release::GithubClient, Manifest, &'static str)>> {
    let gh = match worker.github_client() {
        Ok(gh) => gh,
        Err(e) => {
            warn!(err = %e, "preflight(release): cannot build GitHub client");
            return Ok(None);
        }
    };
    match gh.fetch_manifest_with_verification(tag).await {
        Ok((manifest, verification)) => Ok(Some((gh, manifest, verification))),
        Err(e) if crate::release::GithubClient::is_release_json_unavailable(&e) => {
            warn!(
                err = %e,
                tag = %tag,
                "preflight(release): GitHub release.json unavailable"
            );
            Ok(None)
        }
        Err(e) => Err(e),
    }
}

async fn run_release_with_manifest(
    worker: Arc<Worker>,
    job_id: &str,
    target: &DeployTag,
    risk: RiskFlags,
    gh: crate::release::GithubClient,
    manifest: Manifest,
    verification: &str,
) -> Result<PreflightReport> {
    record_trust(
        &worker,
        job_id,
        new_trust("github_release", verification, None),
    )?;
    if manifest.version.as_str() != target.as_str() {
        return Err(UpdaterError::Precondition(
            "release manifest version does not match requested target".into(),
        ));
    }
    if manifest.schema_version > SUPPORTED_RELEASE_SCHEMA {
        return Err(UpdaterError::Precondition(format!(
            "release schema_version {} exceeds updater support {}; upgrade updater first",
            manifest.schema_version, SUPPORTED_RELEASE_SCHEMA
        )));
    }

    let self_v = MyriadVersion::parse(crate::self_version()).map_err(|e| {
        UpdaterError::Precondition(format!(
            "could not parse own updater version {:?}: {e}",
            crate::self_version()
        ))
    })?;
    if self_v.older_than(&manifest.updater.min_updater_version) {
        return Err(UpdaterError::Precondition(format!(
            "this updater ({}) is older than required min_updater_version {}; self-update first",
            self_v, manifest.updater.min_updater_version
        )));
    }

    // postgres.min_pg_version is advisory until we can probe a live major reliably
    // without Docker exec. Surface it so operators see the requirement in logs.
    if !manifest.postgres.min_pg_version.is_empty()
        && manifest.postgres.min_pg_version != "unbounded"
    {
        warn!(
            min_pg = %manifest.postgres.min_pg_version,
            "preflight: release requires PostgreSQL >= {}; updater does not auto-probe PG major — \
             ensure your DB meets this before applying migrations",
            manifest.postgres.min_pg_version
        );
    }

    let st = worker.state().read_updater()?;
    let from_version = st.current_version.clone();

    let mut is_downgrade = false;
    let mut is_diverged = false;

    // Semver direction when both sides are releases.
    if let (Some(curr), Some(tgt)) = (&from_version, target.as_release()) {
        if let Some(curr_rel) = curr.as_release() {
            if curr_rel.as_str() == tgt.as_str() {
                return Err(UpdaterError::Precondition(format!(
                    "target {tgt} is already the running release version"
                )));
            } else if tgt.older_than(&curr_rel) {
                is_downgrade = true;
            } else if !curr_rel.older_than(&tgt) {
                // Non-orderable prerelease edge cases → require unknown confirmation.
                require_flag(
                    risk.allow_unknown,
                    &format!(
                        "cannot order {curr_rel} vs {tgt} by semver; re-submit with \
                         allow_unknown=true (or allow_risk=true)"
                    ),
                )?;
            }
        } else {
            // Current is commit/branch while target is release — use git compare when possible.
            match gh.compare_deploy_to_ref(Some(curr), target.as_str()).await {
                Ok(Some(f)) => {
                    is_downgrade = f.is_downgrade();
                    is_diverged = matches!(f.relation, CommitRelation::Diverged);
                    if matches!(f.relation, CommitRelation::Identical) {
                        return Err(UpdaterError::Precondition(format!(
                            "target {} points at the same git commit as current {}",
                            target.as_str(),
                            curr
                        )));
                    }
                    if matches!(f.relation, CommitRelation::Unknown) {
                        require_flag(
                            risk.allow_unknown,
                            &format!(
                                "cannot determine whether {} is newer than current {}; \
                                 re-submit with allow_unknown=true (or allow_risk=true)",
                                target.as_str(),
                                curr
                            ),
                        )?;
                    }
                }
                Ok(None) => {
                    require_flag(
                        risk.allow_unknown,
                        &format!(
                            "cannot resolve current deploy {curr} to a git commit for comparison \
                             with release {}; re-submit with allow_unknown=true (or allow_risk=true)",
                            target.as_str()
                        ),
                    )?;
                }
                Err(e) => {
                    require_flag(
                        risk.allow_unknown,
                        &format!(
                            "git compare failed ({e}); refusing update without known direction. \
                             Fix GitHub access or re-submit with allow_unknown=true (or allow_risk=true)"
                        ),
                    )?;
                }
            }
        }
    }

    if is_downgrade {
        require_downgrade(risk.allow_downgrade, target.as_str())?;
        if manifest.migrations.irreversible {
            require_flag(
                risk.allow_irreversible,
                &format!(
                    "target release {} declares irreversible migrations; downgrade refused \
                     unless allow_irreversible=true (or allow_risk=true)",
                    target.as_str()
                ),
            )?;
            warn!(
                to = %target,
                "preflight: irreversible migration + downgrade allowed"
            );
        }
        warn!(to = %target, "preflight: explicit release DOWNgrade allowed");
    }
    if is_diverged {
        require_flag(
            risk.allow_diverged,
            &format!(
                "target {} diverged from current history; re-submit with allow_diverged=true \
                 (or allow_risk=true)",
                target.as_str()
            ),
        )?;
    }

    // min_from only when upgrading between releases.
    if let (Some(curr), Some(min_from)) = (&from_version, &manifest.min_from_version) {
        if let (Some(curr_rel), Some(tgt_rel)) = (curr.as_release(), target.as_release()) {
            let is_upgrade = curr_rel.older_than(&tgt_rel);
            if is_upgrade && curr_rel.older_than(min_from) {
                return Err(UpdaterError::Precondition(format!(
                    "current version {curr} is older than min_from_version {min_from}; \
                     upgrade to an intermediate release first"
                )));
            }
        }
    }

    check_env_keys(worker.as_ref(), Some(&manifest))?;
    check_disk(worker.as_ref())?;
    crate::worker::preflight_env::check_local_environment(&worker).await?;
    check_compose_networks(&worker).await?;

    let backend = manifest
        .image("backend")
        .ok_or_else(|| UpdaterError::Precondition("manifest lacks backend image".into()))?;
    let frontend = manifest
        .image("frontend")
        .ok_or_else(|| UpdaterError::Precondition("manifest lacks frontend image".into()))?;

    for img in [&backend.r#ref, &frontend.r#ref] {
        if img.ends_with(":latest") {
            return Err(UpdaterError::Precondition(format!(
                "image ref must use immutable tag, got: {img}"
            )));
        }
    }

    let backend_pulled = worker
        .docker_pull_with_mirror(&backend.r#ref)
        .await
        .map_err(|e| UpdaterError::Precondition(format!("pull backend: {e}")))?;
    record_image(&worker, job_id, "backend", &backend.r#ref, &backend_pulled)?;
    let frontend_pulled = worker
        .docker_pull_with_mirror(&frontend.r#ref)
        .await
        .map_err(|e| UpdaterError::Precondition(format!("pull frontend: {e}")))?;
    record_image(
        &worker,
        job_id,
        "frontend",
        &frontend.r#ref,
        &frontend_pulled,
    )?;

    if !digest_matches(&backend_pulled, &backend.digest) {
        return Err(UpdaterError::Precondition(format!(
            "backend digest mismatch: pulled {backend_pulled}, expected {}",
            backend.digest
        )));
    }
    if !digest_matches(&frontend_pulled, &frontend.digest) {
        return Err(UpdaterError::Precondition(format!(
            "frontend digest mismatch: pulled {frontend_pulled}, expected {}",
            frontend.digest
        )));
    }

    let estimated = manifest.migrations.estimated_seconds;
    let target_commit_sha = match manifest.commit_sha.clone() {
        some @ Some(_) => some,
        None => match gh.resolve_commit(target.as_str()).await {
            Ok(info) => Some(info.sha),
            Err(e) => {
                warn!(target = %target, err = %e, "release manifest lacks commit_sha and tag resolution failed");
                None
            }
        },
    };
    Ok(PreflightReport {
        manifest: Some(manifest),
        from_version,
        target: target.clone(),
        target_commit_sha,
        backend_digest: backend_pulled,
        frontend_digest: frontend_pulled,
        estimated_seconds: estimated,
        is_downgrade,
        is_diverged,
    })
}

/// Release install without `release.json`: pull formal `vX.Y.Z` images from Docker Hub
/// using the same repo naming as commit mode (`BACKEND_IMAGE` / `FRONTEND_IMAGE` + tag).
///
/// Digests come from the pull; cosign and manifest digest equality are skipped.
/// Missing images hard-fail with a clear Docker Hub error (no silent install).
async fn run_release_via_dockerhub(
    worker: Arc<Worker>,
    job_id: &str,
    target: &DeployTag,
    risk: RiskFlags,
) -> Result<PreflightReport> {
    let release = target.as_release().ok_or_else(|| {
        UpdaterError::InvalidInput(format!(
            "release mode requires a vX.Y.Z target, got {}",
            target.as_str()
        ))
    })?;

    require_formal_tag(target)?;
    let reason = "GitHub release.json unavailable; no manifest digest, Cosign or migration compatibility guarantees";
    record_trust(
        &worker,
        job_id,
        new_trust("dockerhub_tag", "unsigned", Some(reason.into())),
    )?;
    require_tag_install(risk, reason)?;

    let from_version = worker.state().read_updater()?.current_version.clone();
    let mut is_downgrade = false;
    let mut is_diverged = false;

    // Semver when both sides are releases (no GitHub needed).
    if let (Some(curr), Some(tgt)) = (&from_version, target.as_release()) {
        if let Some(curr_rel) = curr.as_release() {
            if curr_rel.as_str() == tgt.as_str() {
                return Err(UpdaterError::Precondition(format!(
                    "target {tgt} is already the running release version"
                )));
            } else if tgt.older_than(&curr_rel) {
                is_downgrade = true;
            } else if !curr_rel.older_than(&tgt) {
                require_flag(
                    risk.allow_unknown,
                    &format!(
                        "cannot order {curr_rel} vs {tgt} by semver; re-submit with \
                         allow_unknown=true (or allow_risk=true)"
                    ),
                )?;
            }
        } else {
            // Commit/branch → release without reliable git compare (no release.json path).
            // Mirror commit mode: do not require allow_unknown solely for missing ancestry.
            if worker.github_commit_metadata_enabled() {
                if let Ok(gh) = worker.github_client() {
                    match gh.compare_deploy_to_ref(Some(curr), target.as_str()).await {
                        Ok(Some(f)) => {
                            is_downgrade = f.is_downgrade();
                            is_diverged = matches!(f.relation, CommitRelation::Diverged);
                            if matches!(f.relation, CommitRelation::Identical) {
                                return Err(UpdaterError::Precondition(format!(
                                    "target {} points at the same git commit as current {}",
                                    target.as_str(),
                                    curr
                                )));
                            }
                            if matches!(f.relation, CommitRelation::Unknown) {
                                info!(
                                    target = %target,
                                    current = %curr,
                                    "preflight(release/dh): unknown git relation; proceeding \
                                     without allow_unknown (no release.json)"
                                );
                            }
                        }
                        Ok(None) => {
                            info!(
                                target = %target,
                                current = %curr,
                                "preflight(release/dh): cannot resolve current deploy to git; \
                                 proceeding (semver/tag differ is sufficient without release.json)"
                            );
                        }
                        Err(e) => {
                            warn!(
                                target = %target,
                                err = %e,
                                "preflight(release/dh): git compare failed; proceeding without allow_unknown"
                            );
                        }
                    }
                }
            } else {
                info!(
                    target = %target,
                    current = %curr,
                    "preflight(release/dh): GITHUB_TOKEN unset; skipping git ancestry for \
                     commit→release (image pull verifies tags exist)"
                );
            }
        }
    }

    if is_downgrade {
        require_downgrade(risk.allow_downgrade, target.as_str())?;
        warn!(to = %target, "preflight: explicit release DOWNgrade allowed (Docker Hub path)");
    }
    if is_diverged {
        require_flag(
            risk.allow_diverged,
            &format!(
                "target {} diverged from current history; re-submit with allow_diverged=true \
                 (or allow_risk=true)",
                target.as_str()
            ),
        )?;
    }

    // No manifest: cannot enforce min_from_version / irreversible / min_updater_version.
    check_env_keys(worker.as_ref(), None)?;
    check_disk(worker.as_ref())?;
    crate::worker::preflight_env::check_local_environment(&worker).await?;
    check_compose_networks(&worker).await?;

    let (backend_repo, frontend_repo) = worker.image_repos_required()?;
    let tag = release.as_str();
    let backend_ref = format!("{backend_repo}:{tag}");
    let frontend_ref = format!("{frontend_repo}:{tag}");

    for img in [&backend_ref, &frontend_ref] {
        if img.ends_with(":latest") {
            return Err(UpdaterError::Precondition(format!(
                "image ref must use immutable tag, got: {img}"
            )));
        }
    }

    info!(
        backend = %backend_ref,
        frontend = %frontend_ref,
        "preflight(release): pulling release images via Docker Hub"
    );

    let backend_pulled = worker
        .docker_pull_with_mirror(&backend_ref)
        .await
        .map_err(|e| {
            UpdaterError::Precondition(format!(
                "pull backend {backend_ref}: {e} (is release {tag} published on Docker Hub?)"
            ))
        })?;
    record_image(&worker, job_id, "backend", &backend_ref, &backend_pulled)?;
    let frontend_pulled = worker
        .docker_pull_with_mirror(&frontend_ref)
        .await
        .map_err(|e| {
            UpdaterError::Precondition(format!(
                "pull frontend {frontend_ref}: {e} (is release {tag} published on Docker Hub?)"
            ))
        })?;

    record_image(&worker, job_id, "frontend", &frontend_ref, &frontend_pulled)?;
    // Optional commit_sha when GitHub is reachable but only the release asset was missing.
    let target_commit_sha = if worker.github_commit_metadata_enabled() {
        match worker.github_client() {
            Ok(gh) => match gh.resolve_commit(target.as_str()).await {
                Ok(info) => Some(info.sha),
                Err(e) => {
                    warn!(
                        target = %target,
                        err = %e,
                        "preflight(release/dh): could not resolve release tag to commit_sha"
                    );
                    None
                }
            },
            Err(_) => None,
        }
    } else {
        None
    };

    Ok(PreflightReport {
        manifest: None,
        from_version,
        target: target.clone(),
        target_commit_sha,
        backend_digest: backend_pulled,
        frontend_digest: frontend_pulled,
        estimated_seconds: 60,
        is_downgrade,
        is_diverged,
    })
}

async fn run_commit(
    worker: Arc<Worker>,
    job_id: &str,
    target: &DeployTag,
    risk: RiskFlags,
) -> Result<PreflightReport> {
    if target.is_release() {
        return Err(UpdaterError::InvalidInput(format!(
            "commit mode expects dev-<sha> or branch tip, got release tag {}",
            target.as_str()
        )));
    }
    info!(target = %target, kind = ?target.kind(), "preflight(commit): resolving + pulling");
    record_trust(
        &worker,
        job_id,
        new_trust("dockerhub_commit", "pending", None),
    )?;

    let from_version = worker.state().read_updater()?.current_version.clone();

    // Prefer GitHub for full-SHA normalization and ancestry when a token is present.
    // Private source repositories need no GitHub access for image signature verification.
    // Missing ancestry alone does not require allow_unknown; signatures are checked below.
    let git_ref = crate::release::deploy_tag_to_git_ref(target);
    let (effective, target_commit_sha, compare_ref) = if !worker.github_commit_metadata_enabled() {
        if target.kind() != DeployTagKind::Commit {
            return Err(UpdaterError::Precondition(format!(
                "GITHUB_TOKEN is not set and target {} is a mutable branch tip; select an \
                 immutable dev-<sha> build from Docker Hub (commit mode without GitHub)",
                target.as_str()
            )));
        }
        info!(
            target = %target,
            "preflight(commit): GITHUB_TOKEN unset; verifying dev tag via Docker Hub image pulls"
        );
        (target.clone(), target.commit_sha().map(str::to_owned), None)
    } else {
        let gh = worker.github_client()?;
        match gh.resolve_commit(&git_ref).await {
            Ok(tip) => {
                let effective = DeployTag::parse(&format!("dev-{}", tip.short_sha))?;
                info!(
                    requested = %target,
                    effective = %effective,
                    full_sha = %tip.sha,
                    "preflight(commit): normalized target to immutable dev-sha tag"
                );
                (effective, Some(tip.sha.clone()), Some(tip.sha))
            }
            Err(error) if target.kind() == DeployTagKind::Commit => {
                // Access failures on private repos are expected; other errors still warn.
                if crate::release::GithubClient::is_expected_unauthenticated_failure(&error) {
                    info!(
                        target = %target,
                        err = %error,
                        "preflight(commit): GitHub unavailable; verifying tag by pulling images"
                    );
                } else {
                    warn!(
                        target = %target,
                        err = %error,
                        "preflight(commit): GitHub resolve failed; verifying tag by pulling images"
                    );
                }
                (target.clone(), target.commit_sha().map(str::to_owned), None)
            }
            Err(error) => {
                return Err(UpdaterError::Precondition(format!(
                    "cannot resolve mutable git ref {git_ref} for target {}: {error}; select an \
                     immutable dev-<sha> build from Docker Hub instead",
                    target.as_str()
                )));
            }
        }
    };

    // Confirmation retries bind the resolved commit, never a moving branch name.
    let mut job = worker.state().read_job(job_id)?;
    job.to_version = Some(effective.clone());
    worker.state().write_job(&job)?;

    if from_version.as_ref() == Some(&effective) {
        return Err(UpdaterError::Precondition(format!(
            "target {} is already running",
            effective.as_str()
        )));
    }

    let mut is_downgrade = false;
    let mut is_diverged = false;

    if let Some(compare_ref) = compare_ref.as_deref() {
        // Only runs when GitHub resolved the target; otherwise we skip ancestry entirely.
        let gh = worker.github_client()?;
        match gh
            .compare_deploy_to_ref(from_version.as_ref(), compare_ref)
            .await
        {
            Ok(Some(f)) => {
                is_downgrade = f.is_downgrade();
                is_diverged = matches!(f.relation, CommitRelation::Diverged);
                info!(
                    relation = f.relation.as_str(),
                    ahead = f.ahead_by,
                    behind = f.behind_by,
                    "preflight(commit): freshness"
                );
                match f.relation {
                    CommitRelation::Identical => {
                        return Err(UpdaterError::Precondition(format!(
                            "target {} is already the running commit",
                            effective.as_str()
                        )));
                    }
                    CommitRelation::Behind => {
                        require_downgrade(risk.allow_downgrade, effective.as_str())?;
                    }
                    CommitRelation::Diverged => {
                        require_flag(
                            risk.allow_diverged,
                            &format!(
                                "target {} diverged from current (ahead {}, behind {}). \
                             Re-submit with allow_diverged=true (or allow_risk=true)",
                                effective.as_str(),
                                f.ahead_by,
                                f.behind_by
                            ),
                        )?;
                    }
                    CommitRelation::Unknown => {
                        // Dev/commit channel: unknown ancestry is not a hard stop.
                        // Upgrade direction is primarily build publish time / different tag.
                        info!(
                            target = %effective,
                            "preflight(commit): unknown git relation; proceeding without allow_unknown"
                        );
                    }
                    CommitRelation::Ahead => {}
                }
            }
            Ok(None) => {
                // Current deploy not resolvable on GitHub (private/no token path is normal).
                info!(
                    target = %effective,
                    "preflight(commit): cannot resolve current deploy to git; proceeding \
                     (build-time / different tag is sufficient for commit mode)"
                );
            }
            Err(e) => {
                warn!(
                    target = %effective,
                    err = %e,
                    "preflight(commit): git compare failed; proceeding without allow_unknown"
                );
            }
        }
    }

    if is_downgrade {
        warn!(to = %effective, "preflight: explicit commit DOWNgrade allowed");
    }

    check_env_keys(worker.as_ref(), None)?;
    check_disk(worker.as_ref())?;
    crate::worker::preflight_env::check_local_environment(&worker).await?;
    check_compose_networks(&worker).await?;

    let (backend_repo, frontend_repo) = worker.image_repos_required()?;
    let tag = effective.as_str();
    let backend_ref = format!("{backend_repo}:{tag}");
    let frontend_ref = format!("{frontend_repo}:{tag}");

    for img in [&backend_ref, &frontend_ref] {
        if img.ends_with(":latest") {
            return Err(UpdaterError::Precondition(format!(
                "image ref must use immutable tag, got: {img}"
            )));
        }
    }

    let backend_pulled = worker
        .docker_pull_with_mirror(&backend_ref)
        .await
        .map_err(|e| {
            UpdaterError::Precondition(format!(
                "pull backend {backend_ref}: {e} (is the commit built by CI?)"
            ))
        })?;
    record_image(&worker, job_id, "backend", &backend_ref, &backend_pulled)?;
    let frontend_pulled = worker
        .docker_pull_with_mirror(&frontend_ref)
        .await
        .map_err(|e| {
            UpdaterError::Precondition(format!(
                "pull frontend {frontend_ref}: {e} (is the commit built by CI?)"
            ))
        })?;

    record_image(&worker, job_id, "frontend", &frontend_ref, &frontend_pulled)?;
    let expected_sha = target_commit_sha
        .as_deref()
        .ok_or_else(|| UpdaterError::Precondition("commit target has no SHA".into()))?;
    let source_repo = &worker.config().github_repo;
    let backend_signature = crate::release::dev_signature::verify(
        &backend_repo,
        &backend_pulled,
        source_repo,
        "backend",
        expected_sha,
    )
    .await?;
    let frontend_signature = crate::release::dev_signature::verify(
        &frontend_repo,
        &frontend_pulled,
        source_repo,
        "frontend",
        expected_sha,
    )
    .await?;
    let signed_commit =
        crate::release::dev_signature::pair_commit(&backend_signature, &frontend_signature)?;
    let mut trust = worker
        .state()
        .read_job(job_id)?
        .trust
        .ok_or_else(|| UpdaterError::State("missing commit trust context".into()))?;
    trust.commit_sha = signed_commit.clone().or(target_commit_sha.clone());
    trust.verification = if signed_commit.is_some() {
        "verified"
    } else {
        "unsigned"
    }
    .into();
    trust.trust_path = if signed_commit.is_some() {
        "signed_commit"
    } else {
        "dockerhub_commit"
    }
    .into();
    trust.reason = signed_commit
        .is_none()
        .then(|| "Development image signature missing; commit origin is not authenticated".into());
    record_trust(&worker, job_id, trust)?;
    if signed_commit.is_none() {
        require_tag_install(risk, "Development image signature missing; install legacy unsigned commit build only after confirmation")?;
    }
    let target_commit_sha = signed_commit.or(target_commit_sha);

    Ok(PreflightReport {
        manifest: None,
        from_version,
        target: effective,
        target_commit_sha,
        backend_digest: backend_pulled,
        frontend_digest: frontend_pulled,
        estimated_seconds: 60,
        is_downgrade,
        is_diverged,
    })
}

fn require_downgrade(allowed: bool, target: &str) -> Result<()> {
    if allowed {
        return Ok(());
    }
    Err(UpdaterError::Precondition(format!(
        "target {target} is older than current (downgrade). \
         Re-submit with allow_downgrade=true after operator confirmation. \
         Warning: schema/data may not fully reverse."
    )))
}

fn require_flag(allowed: bool, msg: &str) -> Result<()> {
    if allowed {
        return Ok(());
    }
    Err(UpdaterError::Precondition(msg.into()))
}

fn check_env_keys(worker: &Worker, manifest: Option<&Manifest>) -> Result<()> {
    let env = EnvFile::load(&worker.cli().env_file)?;
    let mut missing = Vec::new();
    if let Some(manifest) = manifest {
        for k in &manifest.env.required {
            if env.get(k).is_none() {
                missing.push(k.clone());
            }
        }
        for ne in &manifest.env.new {
            if ne.required && env.get(&ne.name).is_none() && ne.default.is_none() {
                missing.push(ne.name.clone());
            }
        }
    } else {
        // Baseline keys for non-manifest paths (commit + Docker Hub release fallback).
        let mut keys: Vec<&str> = vec![
            "MYRIAD_TAG",
            "JWT_SECRET",
            "BACKEND_IMAGE",
            "FRONTEND_IMAGE",
        ];
        if worker.cli().db_mode.is_external() {
            // Official external-DB deploy has no local postgres service / password.
            keys.push("DATABASE_URL");
        } else {
            keys.push("POSTGRES_PASSWORD");
        }
        for k in keys {
            if env.get(k).is_none() {
                missing.push(k.to_string());
            }
        }
    }
    if !missing.is_empty() {
        return Err(UpdaterError::Precondition(format!(
            "missing required env keys: {}",
            missing.join(", ")
        )));
    }
    Ok(())
}

fn check_disk(worker: &Worker) -> Result<()> {
    if worker.cli().db_mode.is_external() {
        // External Postgres: no local pgdata snapshot; skip size/path requirements.
        warn!("db_mode=external; skipping pgdata disk preflight");
        return Ok(());
    }
    // Updates snapshot pgdata; missing path must fail preflight, not pass silently.
    crate::probe::filesystem::require_pgdata(&worker.cli().pgdata)?;
    if let Ok(stat) = nix::sys::statvfs::statvfs(&worker.cli().pgdata) {
        let block = stat.fragment_size();
        // fsblkcnt_t width differs by OS (Linux CI flags same-type cast as needless).
        #[allow(clippy::unnecessary_cast)]
        let avail = block * (stat.blocks_available() as u64);
        let pgdata_size = fs_size(&worker.cli().pgdata).unwrap_or(0);
        let need = pgdata_size + (pgdata_size / 2) + (1024 * 1024 * 1024);
        if avail < need {
            return Err(UpdaterError::Precondition(format!(
                "insufficient disk for snapshot: have {} bytes, need ~{}",
                avail, need
            )));
        }
    }
    Ok(())
}

/// Fail closed when compose would attach update/rollback services to a network
/// docker-guard will reject — otherwise `compose up` fails after stop/snapshot and
/// rollback hits the same error (site stuck down).
async fn check_compose_networks(worker: &Arc<Worker>) -> Result<()> {
    let allow = NetworkAllowlist::resolve(Some(worker.cli().env_file.as_path()));
    let compose = crate::worker::update::build_compose_runner_pub(worker)
        .await
        .map_err(|e| {
            UpdaterError::Precondition(format!(
                "cannot build compose runner for network preflight: {e}"
            ))
        })?;

    let config = compose.config_json().await.map_err(|e| {
        UpdaterError::Precondition(format!("compose config for network preflight failed: {e}"))
    })?;

    // Topology / volume / project-label contract — inspect only; does not alter compose.
    crate::worker::preflight_env::check_compose_contract(worker, &config, compose.project())
        .await?;

    let mut services: Vec<&str> = UPDATE_RECREATE_SERVICES.to_vec();
    if worker.cli().db_mode.is_external() {
        services.retain(|s| *s != "postgres");
    }

    let attachments = networks_for_services(&config, compose.project(), &services);
    if attachments.is_empty() {
        return Err(UpdaterError::Precondition(
            "compose config lists no networks for backend/frontend \
             (and postgres when bundled); refusing update"
                .into(),
        ));
    }

    let bad = find_disallowed_attachments(&allow, &attachments);
    if !bad.is_empty() {
        let detail = bad
            .iter()
            .map(|(svc, net)| format!("{svc}→{net}"))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(UpdaterError::Precondition(format!(
            "compose network(s) outside the Myriad docker-guard allowlist: {detail}. \
             Allowed names: {}. Fix MYRIAD_DOCKER_NETWORK / MYRIAD_ADMIN_NETWORK / \
             MYRIAD_DOCKER_GUARD_NETWORK (and matching compose `networks.*.name`) so they \
             match before retrying — otherwise update and rollback both fail at compose up.",
            allow.describe()
        )));
    }

    // docker-guard cannot create networks; confirm each required name already exists
    // and inspect Name still matches the allowlist (connect path uses inspect Name).
    let mut missing = Vec::new();
    let mut mismatched = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for (_svc, name) in &attachments {
        if !seen.insert(name.clone()) {
            continue;
        }
        match worker.docker().raw().inspect_network(name, None).await {
            Ok(info) => {
                let actual = info.name.unwrap_or_else(|| name.clone());
                let actual = actual.trim_start_matches('/');
                if !allow.contains(actual) {
                    mismatched.push(format!("{name} (inspect Name={actual})"));
                }
            }
            Err(_) => missing.push(name.clone()),
        }
    }
    if !missing.is_empty() {
        return Err(UpdaterError::Precondition(format!(
            "required Docker network(s) missing: {}. \
             docker-guard cannot create networks; create them (or run an initial \
             `docker compose up` that creates allowlisted nets) before updating. \
             Allowed names: {}.",
            missing.join(", "),
            allow.describe()
        )));
    }
    if !mismatched.is_empty() {
        return Err(UpdaterError::Precondition(format!(
            "Docker network Name outside allowlist: {}. Allowed: {}.",
            mismatched.join(", "),
            allow.describe()
        )));
    }

    // Running containers on a non-allowlisted net also break recreate (connect/disconnect).
    check_running_container_networks(worker, &allow).await?;

    info!(
        allowlist = %allow.describe(),
        attachments = attachments.len(),
        "preflight: compose networks within docker-guard allowlist"
    );
    Ok(())
}

async fn check_running_container_networks(worker: &Worker, allow: &NetworkAllowlist) -> Result<()> {
    // Fixed container_name values from production compose (skip leftover postgres when external).
    let containers = crate::worker::preflight_env::running_check_containers(worker.cli().db_mode);
    let mut bad = Vec::new();
    for name in containers {
        let info = match worker.docker().raw().inspect_container(name, None).await {
            Ok(info) => info,
            Err(_) => continue, // not present yet — compose up will create
        };
        let Some(networks) = info.network_settings.and_then(|ns| ns.networks) else {
            continue;
        };
        for net_name in networks.keys() {
            if !allow.contains(net_name) {
                bad.push(format!("{name}→{net_name}"));
            }
        }
    }
    if bad.is_empty() {
        return Ok(());
    }
    Err(UpdaterError::Precondition(format!(
        "running container(s) attached to network(s) outside the Myriad allowlist: {}. \
         Allowed: {}. Reattach to allowlisted nets (or recreate the stack) before updating; \
         otherwise compose up / rollback will fail with the same denial.",
        bad.join(", "),
        allow.describe()
    )))
}

/// Pure helper used by unit tests — mirrors external short-circuit in [`check_disk`].
#[cfg(test)]
pub(crate) fn should_skip_pgdata_disk_check(db_mode: crate::config::DbMode) -> bool {
    db_mode.is_external()
}

fn digest_matches(pulled: &str, expected: &str) -> bool {
    let pulled = pulled.strip_prefix("sha256:").unwrap_or(pulled);
    let expected = expected.strip_prefix("sha256:").unwrap_or(expected);
    expected.len() == 64 && expected.bytes().all(|b| b.is_ascii_hexdigit()) && pulled == expected
}

fn fs_size(p: &std::path::Path) -> std::io::Result<u64> {
    let out = std::process::Command::new("du")
        .args(["-sb", &p.to_string_lossy()])
        .output()?;
    if !out.status.success() {
        return Ok(0);
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0))
}

#[cfg(test)]
mod risk_flag_tests {
    use super::*;

    #[test]
    fn tag_install_consent_is_independent_of_other_risk_flags() {
        let umbrella = RiskFlags::from_api(false, true, None, None, None);
        assert!(matches!(
            require_tag_install(umbrella, "no manifest"),
            Err(UpdaterError::TagInstallRequired(_))
        ));
        let tag_only = RiskFlags {
            allow_tag_install: true,
            ..Default::default()
        };
        assert!(require_tag_install(tag_only, "no manifest").is_ok());
        assert!(require_downgrade(tag_only.allow_downgrade, "v1.0.0").is_err());
        assert!(!tag_only.allow_irreversible);
    }

    #[test]
    fn formal_tag_path_rejects_dev_branch_and_prerelease() {
        assert!(require_formal_tag(&DeployTag::parse("v1.2.3").unwrap()).is_ok());
        for tag in ["dev-abcdef0", "preview", "v1.2.3-beta.1"] {
            assert!(
                require_formal_tag(&DeployTag::parse(tag).unwrap()).is_err(),
                "{tag}"
            );
        }
    }

    #[test]
    fn repository_config_cannot_inject_a_tag_digest_or_url() {
        for repo in [
            "docker.io/org/backend",
            "org/frontend",
            "localhost:5000/org/backend",
        ] {
            assert!(validate_image_repo(repo).is_ok(), "{repo}");
        }
        for repo in [
            "org/backend:v1",
            "org/backend@sha256:abc",
            "https://registry/org/backend",
            " org/backend",
            "org/backend\n",
            "org/../backend",
            "",
        ] {
            assert!(validate_image_repo(repo).is_err(), "{repo:?}");
        }
    }

    #[test]
    fn allow_risk_umbrellas_granular_flags() {
        let r = RiskFlags::from_api(false, true, None, None, None);
        assert!(r.allow_downgrade);
        assert!(r.allow_diverged);
        assert!(r.allow_unknown);
        assert!(r.allow_irreversible);
    }

    #[test]
    fn granular_flags_override_umbrella_defaults() {
        let r = RiskFlags::from_api(true, false, Some(true), Some(false), None);
        assert!(r.allow_downgrade);
        assert!(r.allow_diverged);
        assert!(!r.allow_unknown);
        assert!(!r.allow_irreversible);
    }
}

#[cfg(test)]
mod github_manifest_fallback_tests {
    use super::*;

    #[test]
    fn github_404_is_unavailable_for_dockerhub_fallback() {
        let err = UpdaterError::Github(
            "GET release v0.3.3 failed: 404 Not Found {\"message\":\"Not Found\"}".into(),
        );
        assert!(crate::release::GithubClient::is_release_json_unavailable(
            &err
        ));
    }

    #[test]
    fn github_missing_release_json_asset_is_unavailable() {
        let err = UpdaterError::Github("release v0.3.3 has no release.json asset".into());
        assert!(crate::release::GithubClient::is_release_json_unavailable(
            &err
        ));
    }

    #[test]
    fn github_401_is_unavailable() {
        let err = UpdaterError::Github("GET release v1.0.0 failed: 401 Unauthorized".into());
        assert!(crate::release::GithubClient::is_release_json_unavailable(
            &err
        ));
    }

    #[test]
    fn io_errors_are_unavailable() {
        let err = UpdaterError::Io(std::io::Error::new(
            std::io::ErrorKind::ConnectionReset,
            "connection reset",
        ));
        assert!(crate::release::GithubClient::is_release_json_unavailable(
            &err
        ));
    }

    #[test]
    fn cosign_failure_must_not_fall_back() {
        let err = UpdaterError::Precondition(
            "cosign: signature verification failed: no matching signatures".into(),
        );
        assert!(!crate::release::GithubClient::is_release_json_unavailable(
            &err
        ));
    }

    #[test]
    fn invalid_manifest_json_must_not_fall_back() {
        let err =
            UpdaterError::Json(serde_json::from_str::<serde_json::Value>("not-json").unwrap_err());
        assert!(!crate::release::GithubClient::is_release_json_unavailable(
            &err
        ));
    }

    #[test]
    fn manifest_validation_precondition_must_not_fall_back() {
        let err = UpdaterError::Precondition("manifest missing images.backend".into());
        assert!(!crate::release::GithubClient::is_release_json_unavailable(
            &err
        ));
    }

    #[test]
    fn digest_matches_accepts_full_hex_but_rejects_short_suffix() {
        assert!(digest_matches(
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        ));
        assert!(!digest_matches("sha256:abc", "sha256:def"));
        assert!(!digest_matches(&format!("sha256:{}", "a".repeat(64)), "a"));
        assert!(!digest_matches(&format!("sha256:{}", "a".repeat(64)), ""));
    }
}

#[cfg(test)]
mod external_db_tests {
    use super::*;
    use crate::config::DbMode;

    #[test]
    fn external_mode_skips_pgdata_disk_check() {
        assert!(should_skip_pgdata_disk_check(DbMode::External));
        assert!(!should_skip_pgdata_disk_check(DbMode::Bundled));
    }
}
