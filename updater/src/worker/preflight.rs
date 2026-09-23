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
//! - diverged → requires `allow_diverged` / `allow_risk` (release→release and commit→commit only)
//! - **commit/dev → release**: dev/preview builds are ephemeral and routinely diverge from
//!   (or sit ahead of) the release line, so moving to a release is always allowed — no
//!   downgrade/diverged gate, and unknown ancestry does **not** require `allow_unknown`.
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

use crate::SUPPORTED_RELEASE_SCHEMA;
use crate::config::DbMode;
use crate::docker::network_allowlist::{
    NetworkAllowlist, UPDATE_RECREATE_SERVICES, find_disallowed_attachments, networks_for_services,
};
use crate::env_file::EnvFile;
use crate::error::{Result, UpdaterError};
use crate::release::{CommitRelation, Manifest};
use crate::version::{DeployTag, DeployTagKind, MyriadVersion, UpdateMode};
use crate::worker::Worker;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct PreflightReport {
    pub from_version: Option<DeployTag>,
    /// Tag actually written to MYRIAD_TAG (commit mode: always `dev-<sha>`).
    pub target: DeployTag,
    /// Full source commit for the target image set.
    pub target_commit_sha: Option<String>,
    pub backend_image_id: String,
    pub frontend_image_id: String,
    /// Pulled local image id of the proxy image shipped by this release, when the
    /// release actually ships one (proxy keeps its own `PROXY_TAG` cadence).
    #[serde(default)]
    pub proxy_image_id: Option<String>,
    /// Tag to write into `PROXY_TAG` when this release ships a proxy image.
    #[serde(default)]
    pub proxy_target_tag: Option<String>,
    /// `PROXY_TAG` value before the swap, restored on rollback when it changed.
    #[serde(default)]
    pub previous_proxy_tag: Option<String>,
    pub compose: crate::deployment::PreparedCompose,
    pub estimated_seconds: u32,
}

/// Local image identities resolved by [`prepare_images`].
struct ResolvedImages {
    backend_image_id: String,
    frontend_image_id: String,
    proxy_image_id: Option<String>,
    proxy_target_tag: Option<String>,
    previous_proxy_tag: Option<String>,
}

/// Operator confirmation flags. `allow_risk` is a backward-compatible umbrella that
/// enables all non-downgrade risk gates when true.
#[derive(Debug, Clone, Copy, Default)]
pub struct RiskFlags {
    pub allow_downgrade: bool,
    pub allow_diverged: bool,
    pub allow_unknown: bool,
    pub allow_irreversible: bool,
    pub allow_compose_override: bool,
}

impl RiskFlags {
    /// Build from granular flags + optional umbrella `allow_risk`.
    pub fn from_api(
        allow_downgrade: bool,
        allow_risk: bool,
        allow_diverged: Option<bool>,
        allow_unknown: Option<bool>,
        allow_irreversible: Option<bool>,
        allow_compose_override: Option<bool>,
    ) -> Self {
        Self {
            allow_downgrade: allow_downgrade || allow_risk,
            allow_diverged: allow_diverged.unwrap_or(allow_risk),
            allow_unknown: allow_unknown.unwrap_or(allow_risk),
            allow_irreversible: allow_irreversible.unwrap_or(allow_risk),
            allow_compose_override: allow_compose_override.unwrap_or(allow_risk),
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
            // Self-host / dev-channel often installs formal v* tags from Docker Hub when
            // GitHub release.json is missing (private repo, no token, asset not published).
            // Fall back without an extra allow gate — digests/cosign/min_from are skipped
            // on this path (same as pre-hardening behavior). Cosign hard-failures still
            // do not fall back (try_fetch_release_manifest returns Err).
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

/// True only when the running updater reports a parseable release version that
/// is older than the release floor.
///
/// `self_version()` is the image's `MYRIAD_VERSION`: release builds embed a
/// `vX.Y.Z` semver, but CI commit builds embed `dev-<sha>`, which is not a
/// [`MyriadVersion`] and cannot be ordered against a semver floor. An
/// unparseable (commit/dev) self version must not hard-fail the upgrade; it is
/// treated as "not below the floor", matching `worker::check`.
fn updater_below_min(self_version: &str, min: &MyriadVersion) -> bool {
    MyriadVersion::parse(self_version).is_ok_and(|v| v.older_than(min))
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

    if updater_below_min(crate::self_version(), &manifest.updater.min_updater_version) {
        return Err(UpdaterError::Precondition(format!(
            "this updater ({}) is older than required min_updater_version {}; self-update first",
            crate::self_version(),
            manifest.updater.min_updater_version
        )));
    }

    // Bundled deployments must meet the release floor; external deployments are
    // logged and left to the operator (see `enforce_min_pg_version`).
    enforce_min_pg_version(
        worker.cli().db_mode,
        &worker.cli().pgdata,
        &manifest.postgres.min_pg_version,
    )?;

    let st = worker.state().read_updater()?;
    let from_version = st.current_version.clone();

    let mut is_downgrade = false;

    // Semver direction when both sides are releases.
    if let (Some(curr), Some(tgt)) = (&from_version, target.as_release()) {
        match curr.as_release() {
            Some(curr_rel) => {
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
            }
            _ => {
                // Current is a `dev-<sha>` or branch tip while the target is a formal
                // release. Moving to a release is always allowed: dev/preview builds
                // are ephemeral and routinely diverge from (or sit ahead of) the
                // release line, so the downgrade/diverged direction gates do not apply.
                // Only a no-op (target already at the same commit) is rejected.
                match gh.compare_deploy_to_ref(Some(curr), target.as_str()).await {
                    Ok(Some(f)) if matches!(f.relation, CommitRelation::Identical) => {
                        return Err(UpdaterError::Precondition(format!(
                            "target {} points at the same git commit as current {}",
                            target.as_str(),
                            curr
                        )));
                    }
                    _ => {}
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

    // min_from only when upgrading between releases.
    if let (Some(curr), Some(min_from)) = (&from_version, &manifest.min_from_version)
        && let (Some(curr_rel), Some(tgt_rel)) = (curr.as_release(), target.as_release())
    {
        let is_upgrade = curr_rel.older_than(&tgt_rel);
        if is_upgrade && curr_rel.older_than(min_from) {
            return Err(UpdaterError::Precondition(format!(
                "current version {curr} is older than min_from_version {min_from}; \
                     upgrade to an intermediate release first"
            )));
        }
    }

    let backend = manifest
        .image("backend")
        .ok_or_else(|| UpdaterError::Precondition("manifest lacks backend image".into()))?;
    let frontend = manifest
        .image("frontend")
        .ok_or_else(|| UpdaterError::Precondition("manifest lacks frontend image".into()))?;

    let (resolved, compose) = prepare_images(
        &worker,
        Some(&manifest),
        [&backend.r#ref, &frontend.r#ref],
        target,
        &risk,
    )
    .await?;
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
        from_version,
        target: target.clone(),
        target_commit_sha,
        backend_image_id: resolved.backend_image_id,
        frontend_image_id: resolved.frontend_image_id,
        proxy_image_id: resolved.proxy_image_id,
        proxy_target_tag: resolved.proxy_target_tag,
        previous_proxy_tag: resolved.previous_proxy_tag,
        compose,
        estimated_seconds: estimated,
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

    // Semver when both sides are releases (no GitHub needed).
    if let (Some(curr), Some(tgt)) = (&from_version, target.as_release()) {
        match curr.as_release() {
            Some(curr_rel) => {
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
            }
            _ => {
                // Commit/branch → release: dev/preview builds are ephemeral and
                // routinely diverge from the release line, so moving to a release is
                // always allowed (no downgrade/diverged gate). Only a no-op (same
                // commit) is rejected when GitHub ancestry is available.
                if worker.github_commit_metadata_enabled() {
                    if let Ok(gh) = worker.github_client() {
                        match gh.compare_deploy_to_ref(Some(curr), target.as_str()).await {
                            Ok(Some(f)) => {
                                if matches!(f.relation, CommitRelation::Identical) {
                                    return Err(UpdaterError::Precondition(format!(
                                        "target {} points at the same git commit as current {}",
                                        target.as_str(),
                                        curr
                                    )));
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    if is_downgrade {
        require_downgrade(risk.allow_downgrade, target.as_str())?;
        warn!(to = %target, "preflight: explicit release DOWNgrade allowed (Docker Hub path)");
    }

    // No manifest: cannot enforce min_from_version / irreversible / min_updater_version.
    let (backend_repo, frontend_repo) = worker.image_repos_required()?;
    let tag = release.as_str();
    let backend_ref = format!("{backend_repo}:{tag}");
    let frontend_ref = format!("{frontend_repo}:{tag}");

    let (resolved, compose) =
        prepare_images(&worker, None, [&backend_ref, &frontend_ref], target, &risk).await?;

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
        from_version,
        target: target.clone(),
        target_commit_sha,
        backend_image_id: resolved.backend_image_id,
        frontend_image_id: resolved.frontend_image_id,
        proxy_image_id: resolved.proxy_image_id,
        proxy_target_tag: resolved.proxy_target_tag,
        previous_proxy_tag: resolved.previous_proxy_tag,
        compose,
        estimated_seconds: 60,
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

    if let Some(compare_ref) = compare_ref.as_deref() {
        // Only runs when GitHub resolved the target; otherwise we skip ancestry entirely.
        let gh = worker.github_client()?;
        match gh
            .compare_deploy_to_ref(from_version.as_ref(), compare_ref)
            .await
        {
            Ok(Some(f)) => {
                is_downgrade = f.is_downgrade();
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

    let (backend_repo, frontend_repo) = worker.image_repos_required()?;
    let tag = effective.as_str();
    let backend_ref = format!("{backend_repo}:{tag}");
    let frontend_ref = format!("{frontend_repo}:{tag}");

    let (resolved, compose) =
        prepare_images(&worker, None, [&backend_ref, &frontend_ref], &effective, &risk).await?;
    Ok(PreflightReport {
        from_version,
        target: effective,
        target_commit_sha,
        backend_image_id: resolved.backend_image_id,
        frontend_image_id: resolved.frontend_image_id,
        proxy_image_id: resolved.proxy_image_id,
        proxy_target_tag: resolved.proxy_target_tag,
        previous_proxy_tag: resolved.previous_proxy_tag,
        compose,
        estimated_seconds: 60,
    })
}

/// All target sources use the same checks, pulls and local image identities.
async fn prepare_images(
    worker: &Arc<Worker>,
    manifest: Option<&Manifest>,
    images: [&str; 2],
    target: &DeployTag,
    risk: &RiskFlags,
) -> Result<(ResolvedImages, crate::deployment::PreparedCompose)> {
    check_env_keys(worker, manifest)?;
    check_disk(worker)?;
    crate::worker::preflight_env::check_local_environment(worker).await?;
    for (role, image) in ["backend", "frontend"].into_iter().zip(images) {
        if image.ends_with(":latest") {
            return Err(UpdaterError::Precondition(format!(
                "image ref must use immutable tag, got: {image}"
            )));
        }
        let pulled = worker
            .pull_image(image)
            .await
            .map_err(|error| UpdaterError::Precondition(format!("pull {role} {image}: {error}")))?;
        if let Some(expected) = manifest.and_then(|manifest| manifest.image(role))
            && !digest_matches(&pulled, &expected.digest)
        {
            return Err(UpdaterError::Precondition(format!(
                "{role} digest mismatch: pulled {pulled}, expected {}",
                expected.digest
            )));
        }
    }
    // The proxy keeps its own `PROXY_TAG` cadence: it is only pulled/swapped when
    // the target release actually ships one (manifest carries `images.proxy`). The
    // Docker Hub fallback and commit mode leave the proxy image untouched.
    let (proxy_image_id, proxy_target_tag, previous_proxy_tag) =
        match manifest.and_then(|manifest| manifest.image("proxy")) {
            Some(proxy) => {
                if proxy.r#ref.ends_with(":latest") {
                    return Err(UpdaterError::Precondition(format!(
                        "image ref must use immutable tag, got: {}",
                        proxy.r#ref
                    )));
                }
                let pulled = worker
                    .pull_image(&proxy.r#ref)
                    .await
                    .map_err(|error| UpdaterError::Precondition(format!("pull proxy {}: {error}", proxy.r#ref)))?;
                if !digest_matches(&pulled, &proxy.digest) {
                    return Err(UpdaterError::Precondition(format!(
                        "proxy digest mismatch: pulled {pulled}, expected {}",
                        proxy.digest
                    )));
                }
                let target_tag = image_ref_tag(&proxy.r#ref).ok_or_else(|| {
                    UpdaterError::Precondition(format!("proxy image ref has no tag: {}", proxy.r#ref))
                })?;
                let previous = EnvFile::load(&worker.cli().env_file)
                    .ok()
                    .and_then(|env| env.get("PROXY_TAG").map(str::to_owned));
                (
                    Some(worker.docker().image_id(&proxy.r#ref).await?),
                    Some(target_tag),
                    previous,
                )
            }
            None => (None, None, None),
        };
    let runner = crate::worker::update::build_compose_runner_pub(worker).await?;
    let (prepared, candidate, compose_changed) =
        crate::deployment::prepare(worker, &runner, images[0], target).await?;
    // Fail closed when the deployment compose will be overwritten but the operator
    // did not acknowledge it: the on-disk compose differs from what the updater last
    // wrote (or there is no baseline yet).
    if compose_changed && !risk.allow_compose_override {
        return Err(UpdaterError::Precondition(
            "the deployment compose will be overwritten; re-submit with \
             allow_compose_override=true (or allow_risk=true)"
                .into(),
        ));
    }
    check_compose_networks(worker, images[0], &candidate).await?;
    Ok((
        ResolvedImages {
            backend_image_id: worker.docker().image_id(images[0]).await?,
            frontend_image_id: worker.docker().image_id(images[1]).await?,
            proxy_image_id,
            proxy_target_tag,
            previous_proxy_tag,
        },
        prepared,
    ))
}

/// Tag portion of an image reference (`repo:tag` → `tag`). The repo may contain a
/// host:port prefix; only the final `:` separates the tag.
fn image_ref_tag(image_ref: &str) -> Option<String> {
    image_ref.rsplit_once(':').map(|(_, tag)| tag.to_string())
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

/// Manifest `env.required` keys that apply to this deployment's DB mode.
///
/// A bundled postgres owns `POSTGRES_PASSWORD`; an external-DB deployment does not
/// have it, so the release contract must not demand it there (mirrors the
/// no-manifest branch below and the external short-circuit in [`check_disk`]).
fn applicable_required_env_keys(manifest: &Manifest, db_mode: DbMode) -> Vec<&str> {
    manifest
        .env
        .required
        .iter()
        .map(String::as_str)
        .filter(|k| !(db_mode.is_external() && *k == "POSTGRES_PASSWORD"))
        .collect()
}

fn check_env_keys(worker: &Worker, manifest: Option<&Manifest>) -> Result<()> {
    let env = EnvFile::load(&worker.cli().env_file)?;
    let mut missing = Vec::new();
    if let Some(manifest) = manifest {
        for k in applicable_required_env_keys(manifest, worker.cli().db_mode) {
            if env.get(k).is_none() {
                missing.push(k.to_string());
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
async fn check_compose_networks(
    worker: &Arc<Worker>,
    backend_image: &str,
    compose: &crate::docker::ComposeRunner,
) -> Result<()> {
    let allow = NetworkAllowlist::resolve(Some(worker.cli().env_file.as_path()))?;
    let config = compose.config_json().await.map_err(|e| {
        UpdaterError::Precondition(format!("compose config for network preflight failed: {e}"))
    })?;

    // Topology / volume / project-label contract — inspect only; does not alter compose.
    let workers = worker.docker().worker_support(backend_image).await?;
    crate::worker::preflight_env::check_compose_contract(
        worker,
        &config,
        compose.project(),
        workers,
    )
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
            // Names were checked against the fixed Compose identities above.
            let service = name.strip_prefix("myriad-").unwrap_or(name);
            if !allow.allows_service(service, net_name) {
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

/// Bundled PGDATA: read `PG_VERSION` and refuse updates below the release floor.
///
/// The floor describes the PostgreSQL major shipped in the bundled compose
/// image, so it is only enforceable where the updater owns `PGDATA`. An external
/// deployment (`MYRIAD_DB_MODE=external`) points at an operator-managed server
/// the updater cannot read, and release manifests always declare
/// `min_pg_version`, so refusing here would block every external deployment.
/// Log the floor and leave the operator's server unverified instead.
pub(crate) fn enforce_min_pg_version(
    db_mode: crate::config::DbMode,
    pgdata: &std::path::Path,
    min_pg_version: &str,
) -> Result<()> {
    let min = match parse_pg_major(min_pg_version) {
        Ok(None) => return Ok(()),
        Ok(Some(m)) => m,
        Err(reason) => {
            return Err(UpdaterError::Precondition(format!(
                "release min_pg_version is invalid ({reason}): {min_pg_version:?}"
            )));
        }
    };
    if db_mode.is_external() {
        // External DB is outside compose: there is no PGDATA to probe, so the
        // floor is advisory here. Refusing would block every external
        // deployment because release manifests always declare min_pg_version.
        warn!(
            min_pg = min,
            "preflight: release requires PostgreSQL >= {min}; db_mode=external has no \
             PGDATA to probe, leaving the operator's server unverified"
        );
        return Ok(());
    }
    let running = read_pgdata_major(pgdata)?;
    if running < min {
        return Err(UpdaterError::Precondition(format!(
            "PostgreSQL {running} is below release min_pg_version {min}; upgrade the bundled database first"
        )));
    }
    Ok(())
}

/// `Ok(None)` = no floor. `Ok(Some(n))` = required major. `Err` = illegal spec.
pub(crate) fn parse_pg_major(raw: &str) -> std::result::Result<Option<u32>, &'static str> {
    let s = raw.trim();
    if s.is_empty() || s.eq_ignore_ascii_case("unbounded") {
        return Ok(None);
    }
    if !s.chars().all(|c| c.is_ascii_digit()) {
        return Err("not a PostgreSQL major");
    }
    s.parse().map(Some).map_err(|_| "not a PostgreSQL major")
}

fn read_pgdata_major(pgdata: &std::path::Path) -> Result<u32> {
    // PostgreSQL <= 17 is commonly mounted with the data directory itself as
    // the host root (`pgdata/PG_VERSION`). PostgreSQL 18's official image
    // instead sets PGDATA to `/var/lib/postgresql/18/docker` while the
    // Compose volume targets `/var/lib/postgresql`; on the host that becomes
    // `pgdata/18/docker/PG_VERSION`. Support both layouts without scanning
    // arbitrary descendants of the data volume.
    let root = pgdata.join("PG_VERSION");
    if let Some(major) = read_pg_version_file(&root)? {
        return Ok(major);
    }

    let entries = std::fs::read_dir(pgdata).map_err(|e| {
        UpdaterError::Precondition(format!(
            "cannot read {}: {e}; refusing update without a PostgreSQL major",
            root.display()
        ))
    })?;
    let mut nested = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| {
            UpdaterError::Precondition(format!(
                "cannot inspect PostgreSQL data directory {}: {e}",
                pgdata.display()
            ))
        })?;
        let file_type = entry.file_type().map_err(|e| {
            UpdaterError::Precondition(format!(
                "cannot inspect PostgreSQL data directory entry {}: {e}",
                entry.path().display()
            ))
        })?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !file_type.is_dir() || !name.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let path = entry.path().join("docker/PG_VERSION");
        if let Some(major) = read_pg_version_file(&path)? {
            nested.push((path, major));
        }
    }

    match nested.as_slice() {
        [(_path, major)] => Ok(*major),
        [] => Err(UpdaterError::Precondition(format!(
            "cannot read {}: no PG_VERSION found in the legacy root or PostgreSQL versioned layout; refusing update without a PostgreSQL major",
            root.display()
        ))),
        entries => Err(UpdaterError::Precondition(format!(
            "multiple PostgreSQL PG_VERSION files found under {} ({}); refusing update until the active PGDATA is unambiguous",
            pgdata.display(),
            entries
                .iter()
                .map(|(path, _)| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

/// Read one supported PG_VERSION location. `Ok(None)` means the candidate is
/// absent; malformed or unreadable candidates remain hard precondition errors.
fn read_pg_version_file(path: &std::path::Path) -> Result<Option<u32>> {
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(UpdaterError::Precondition(format!(
                "cannot read {}: {error}; refusing update without a PostgreSQL major",
                path.display()
            )));
        }
    };
    parse_pg_major(&raw)
        .ok()
        .flatten()
        .map(Some)
        .ok_or_else(|| {
            UpdaterError::Precondition(format!(
                "{} is not a PostgreSQL major version: {:?}",
                path.display(),
                raw.trim()
            ))
        })
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
        let r = RiskFlags::from_api(false, true, None, None, None, None);
        assert!(r.allow_downgrade);
        assert!(r.allow_diverged);
        assert!(r.allow_unknown);
        assert!(r.allow_irreversible);
        assert!(r.allow_compose_override);
    }

    #[test]
    fn granular_flags_override_umbrella_defaults() {
        let r = RiskFlags::from_api(true, false, Some(true), Some(false), None, Some(false));
        assert!(r.allow_downgrade);
        assert!(r.allow_diverged);
        assert!(!r.allow_unknown);
        assert!(!r.allow_irreversible);
        assert!(!r.allow_compose_override);
    }
}

#[cfg(test)]
mod min_pg_version_tests {
    use super::*;
    use crate::config::DbMode;

    #[test]
    fn bundled_below_floor_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("PG_VERSION"), "15\n").unwrap();
        let err = enforce_min_pg_version(DbMode::Bundled, dir.path(), "16").unwrap_err();
        assert!(matches!(err, UpdaterError::Precondition(_)), "got {err}");
        assert!(err.to_string().contains("min_pg_version"));
    }

    #[test]
    fn bundled_at_floor_is_ok() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("PG_VERSION"), "16\n").unwrap();
        enforce_min_pg_version(DbMode::Bundled, dir.path(), "16").unwrap();
    }

    #[test]
    fn bundled_postgres_18_versioned_layout_is_ok() {
        let dir = tempfile::tempdir().unwrap();
        let pgdata = dir.path().join("18/docker");
        std::fs::create_dir_all(&pgdata).unwrap();
        std::fs::write(pgdata.join("PG_VERSION"), "18\n").unwrap();
        enforce_min_pg_version(DbMode::Bundled, dir.path(), "16").unwrap();
    }

    #[test]
    fn bundled_unreadable_pg_version_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let err = enforce_min_pg_version(DbMode::Bundled, dir.path(), "16").unwrap_err();
        assert!(
            matches!(err, UpdaterError::Precondition(_)),
            "missing PG_VERSION must not warn-and-continue, got {err}"
        );
    }

    #[test]
    fn empty_or_unbounded_min_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        enforce_min_pg_version(DbMode::Bundled, dir.path(), "").unwrap();
        enforce_min_pg_version(DbMode::Bundled, dir.path(), "unbounded").unwrap();
    }

    #[test]
    fn illegal_min_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let err = enforce_min_pg_version(DbMode::Bundled, dir.path(), "latest").unwrap_err();
        assert!(matches!(err, UpdaterError::Precondition(_)), "got {err}");
        assert!(err.to_string().contains("invalid"));
    }

    #[test]
    fn external_declared_floor_is_logged_not_enforced() {
        // The updater cannot probe an operator-managed server, and every release
        // manifest declares a floor, so external mode must proceed rather than
        // refusing the update.
        let dir = tempfile::tempdir().unwrap();
        enforce_min_pg_version(DbMode::External, dir.path(), "16").unwrap();
    }

    #[test]
    fn external_still_refuses_an_illegal_floor() {
        // A malformed manifest is rejected before db_mode is considered.
        let dir = tempfile::tempdir().unwrap();
        let err = enforce_min_pg_version(DbMode::External, dir.path(), "latest").unwrap_err();
        assert!(matches!(err, UpdaterError::Precondition(_)), "got {err}");
        assert!(err.to_string().contains("invalid"), "got {err}");
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
    fn digest_matches_accepts_suffix() {
        assert!(digest_matches(
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        ));
        assert!(!digest_matches("sha256:abc", "sha256:def"));
    }
}

#[cfg(test)]
mod external_db_tests {
    use super::*;
    use crate::config::DbMode;
    use serde_json::json;

    #[test]
    fn external_mode_skips_pgdata_disk_check() {
        assert!(should_skip_pgdata_disk_check(DbMode::External));
        assert!(!should_skip_pgdata_disk_check(DbMode::Bundled));
    }

    fn manifest_with_required(required: &[&str]) -> Manifest {
        let json = json!({
            "schema_version": 1,
            "version": "v0.5.1",
            "channel": "stable",
            "released_at": "2026-09-18T00:00:00Z",
            "images": {
                "backend": { "ref": "example/backend:v0.5.1", "digest": format!("sha256:{}", "0".repeat(64)) },
                "frontend": { "ref": "example/frontend:v0.5.1", "digest": format!("sha256:{}", "1".repeat(64)) }
            },
            "env": { "required": required, "new": [], "removed": [] },
            "migrations": { "irreversible": false, "estimated_seconds": 30 },
            "updater": { "min_updater_version": "v0.5.0" },
            "postgres": { "min_pg_version": "16" },
            "notes_url": "https://example.com/releases/v0.5.1"
        });
        Manifest::from_json(&serde_json::to_vec(&json).expect("fixture serializes"))
            .expect("manifest fixture is valid")
    }

    /// A release manifest generated by release.yml lists POSTGRES_PASSWORD, but an
    /// external-DB deployment never has it. Preflight must not demand it there.
    #[test]
    fn external_mode_drops_postgres_password_from_manifest_required() {
        let manifest = manifest_with_required(&["POSTGRES_PASSWORD", "JWT_SECRET", "CORS_ORIGINS"]);
        assert_eq!(
            applicable_required_env_keys(&manifest, DbMode::Bundled),
            ["POSTGRES_PASSWORD", "JWT_SECRET", "CORS_ORIGINS"]
        );
        assert_eq!(
            applicable_required_env_keys(&manifest, DbMode::External),
            ["JWT_SECRET", "CORS_ORIGINS"]
        );
    }

    #[test]
    fn commit_build_self_version_does_not_trip_min_updater_version() {
        let min = MyriadVersion::parse("v0.5.0").expect("valid floor");
        assert!(updater_below_min("v0.4.9", &min));
        assert!(!updater_below_min("v0.5.0", &min));
        assert!(!updater_below_min("v0.5.1", &min));
        // CI commit/dev builds are not release semver; they must not hard-fail
        // a release preflight (regression: dev-<full sha> previously errored).
        assert!(!updater_below_min("dev-1485875", &min));
        assert!(!updater_below_min(
            "dev-1485875bfbffc186a0092cc89e6a746f5ea315d1",
            &min
        ));
    }
}
