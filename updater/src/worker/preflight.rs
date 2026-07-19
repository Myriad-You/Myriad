//! Preflight checks (spec §6 pre-check). Run BEFORE entering maintenance mode so failures
//! never cause downtime.
//!
//! Two modes:
//! - **Release**: prefer GitHub `release.json` (digests, cosign, min_from_version). When the
//!   manifest is unavailable (404 / no token / network / missing asset), fall back to pulling
//!   `BACKEND_IMAGE`/`FRONTEND_IMAGE` tagged with the release version from Docker Hub — same
//!   image naming as commit mode. Cosign/schema failures still hard-fail (no silent skip).
//! - **Commit**: resolve image tags to `dev-<sha>` (never persist branch tips), pull images.
//!
//! Direction gates (fail-closed):
//! - pure upgrade → ok
//! - pure downgrade → requires `allow_downgrade`
//! - diverged → requires `allow_diverged` / `allow_risk`
//! - **commit/dev**: unknown ancestry does **not** require `allow_unknown` (build-time
//!   newer / different tag is enough). Release + full manifest still treats unknown as risk;
//!   release Docker Hub fallback (no git compare) matches commit without ancestry.
//! - irreversible migration + downgrade → requires both flags (manifest path only)

use std::sync::Arc;

use tracing::{info, warn};

use crate::env_file::EnvFile;
use crate::error::{Result, UpdaterError};
use crate::release::{CommitRelation, Manifest};
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
/// enables all non-downgrade risk gates when true.
#[derive(Debug, Clone, Copy, Default)]
pub struct RiskFlags {
    pub allow_downgrade: bool,
    pub allow_diverged: bool,
    pub allow_unknown: bool,
    pub allow_irreversible: bool,
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
        }
    }
}

pub async fn run(
    worker: Arc<Worker>,
    target: &DeployTag,
    mode: UpdateMode,
    risk: RiskFlags,
) -> Result<PreflightReport> {
    // Route by target shape, not only by UI mode:
    // - formal releases always use release.json / manifest path
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
    match effective_mode {
        UpdateMode::Release => run_release(worker, target, risk).await,
        UpdateMode::Commit => run_commit(worker, target, risk).await,
    }
}

async fn run_release(
    worker: Arc<Worker>,
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
        Ok(Some((gh, manifest))) => {
            run_release_with_manifest(worker, target, risk, gh, manifest).await
        }
        Ok(None) => {
            warn!(
                target = %release,
                "preflight(release): GitHub release.json unavailable; verifying via Docker Hub images"
            );
            run_release_via_dockerhub(worker, target, risk).await
        }
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
) -> Result<Option<(crate::release::GithubClient, Manifest)>> {
    let gh = match worker.github_client() {
        Ok(gh) => gh,
        Err(e) => {
            warn!(err = %e, "preflight(release): cannot build GitHub client");
            return Ok(None);
        }
    };
    match gh.fetch_manifest(tag).await {
        Ok(manifest) => Ok(Some((gh, manifest))),
        Err(e) if github_release_json_unavailable(&e) => {
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

/// True when the error means release.json cannot be obtained (fall back to Docker Hub).
/// Cosign failures and invalid downloaded JSON must **not** fall back — fail closed.
fn github_release_json_unavailable(err: &UpdaterError) -> bool {
    match err {
        UpdaterError::Github(_) | UpdaterError::Io(_) => true,
        // Cosign enforce returns Precondition("cosign: ...") — never fall back.
        UpdaterError::Precondition(msg) if msg.starts_with("cosign:") => false,
        // Manifest::from_json / validate after a successful download — fail closed.
        UpdaterError::Json(_) | UpdaterError::Precondition(_) => false,
        other => {
            // Network / client build oddities may surface as Internal(anyhow).
            crate::release::GithubClient::is_expected_unauthenticated_failure(other)
                || other.to_string().to_ascii_lowercase().contains("timeout")
                || other.to_string().to_ascii_lowercase().contains("connection")
        }
    }
}

async fn run_release_with_manifest(
    worker: Arc<Worker>,
    target: &DeployTag,
    risk: RiskFlags,
    gh: crate::release::GithubClient,
    manifest: Manifest,
) -> Result<PreflightReport> {
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
    let frontend_pulled = worker
        .docker_pull_with_mirror(&frontend.r#ref)
        .await
        .map_err(|e| UpdaterError::Precondition(format!("pull frontend: {e}")))?;

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
    target: &DeployTag,
    risk: RiskFlags,
) -> Result<PreflightReport> {
    let release = target.as_release().ok_or_else(|| {
        UpdaterError::InvalidInput(format!(
            "release mode requires a vX.Y.Z target, got {}",
            target.as_str()
        ))
    })?;

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
    let frontend_pulled = worker
        .docker_pull_with_mirror(&frontend_ref)
        .await
        .map_err(|e| {
            UpdaterError::Precondition(format!(
                "pull frontend {frontend_ref}: {e} (is release {tag} published on Docker Hub?)"
            ))
        })?;

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

    let from_version = worker.state().read_updater()?.current_version.clone();

    // Prefer GitHub for full-SHA normalization and ancestry when a token is present.
    // Private source repos without GITHUB_TOKEN are the normal self-host case: treat
    // immutable `dev-<sha>` tags as Docker Hub–verified (pull both images) and skip
    // ancestry — do NOT require allow_unknown just because GitHub is private.
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
    let frontend_pulled = worker
        .docker_pull_with_mirror(&frontend_ref)
        .await
        .map_err(|e| {
            UpdaterError::Precondition(format!(
                "pull frontend {frontend_ref}: {e} (is the commit built by CI?)"
            ))
        })?;

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
        for k in [
            "MYRIAD_TAG",
            "POSTGRES_PASSWORD",
            "JWT_SECRET",
            "BACKEND_IMAGE",
            "FRONTEND_IMAGE",
        ] {
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
    if let Ok(stat) = nix::sys::statvfs::statvfs(&worker.cli().pgdata) {
        let block = stat.fragment_size();
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

fn digest_matches(pulled: &str, expected: &str) -> bool {
    pulled == expected || pulled.ends_with(expected)
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
        assert!(github_release_json_unavailable(&err));
    }

    #[test]
    fn github_missing_release_json_asset_is_unavailable() {
        let err = UpdaterError::Github("release v0.3.3 has no release.json asset".into());
        assert!(github_release_json_unavailable(&err));
    }

    #[test]
    fn github_401_is_unavailable() {
        let err = UpdaterError::Github("GET release v1.0.0 failed: 401 Unauthorized".into());
        assert!(github_release_json_unavailable(&err));
    }

    #[test]
    fn io_errors_are_unavailable() {
        let err = UpdaterError::Io(std::io::Error::new(
            std::io::ErrorKind::ConnectionReset,
            "connection reset",
        ));
        assert!(github_release_json_unavailable(&err));
    }

    #[test]
    fn cosign_failure_must_not_fall_back() {
        let err = UpdaterError::Precondition(
            "cosign: signature verification failed: no matching signatures".into(),
        );
        assert!(!github_release_json_unavailable(&err));
    }

    #[test]
    fn invalid_manifest_json_must_not_fall_back() {
        let err = UpdaterError::Json(serde_json::from_str::<serde_json::Value>("not-json").unwrap_err());
        assert!(!github_release_json_unavailable(&err));
    }

    #[test]
    fn manifest_validation_precondition_must_not_fall_back() {
        let err = UpdaterError::Precondition("manifest missing images.backend".into());
        assert!(!github_release_json_unavailable(&err));
    }

    #[test]
    fn digest_matches_accepts_suffix() {
        assert!(digest_matches(
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        ));
        assert!(!digest_matches("sha256:abc", "sha256:def"));
    }
}
