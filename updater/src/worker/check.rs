//! Availability checks and auto-install of a clear upgrade.

use std::sync::Arc;

use chrono::Utc;
use tracing::{info, warn};

use crate::config::Channel;
use crate::error::{Result, UpdaterError};
use crate::release::{
    commit_upgrade_direction_ex, is_cross_kind_deploy, pushed_at_for_tag,
    select_dev_channel_tip_for, DockerBuild, GithubClient, Manifest,
};
use crate::state::LatestAvailable;
use crate::version::{commit_branch_for_channel, DeployTag, MyriadVersion, UpdateMode};
use crate::worker::{AvailableInfo, Command, Worker};

impl Worker {
    /// Shared safety gate for auto-install: only clear upgrades on the current
    /// channel/mode. Downgrade / diverged / irreversible need human confirm.
    ///
    /// **Commit/dev mode**: `relation=unknown` does **not** block auto-install when
    /// `is_upgrade` is true (build-time newer is enough). Release channels still
    /// reject unknown. Applies to **all** channels (stable / preview / commit).
    ///
    /// Dev channel may surface a formal `vX.Y.Z` tip; that tip is cached/installed
    /// via the release path even though prefs `update_mode` remains `commit`.
    fn auto_install_target_ok(
        &self,
        install_mode: UpdateMode,
        irreversible: bool,
    ) -> Result<Option<DeployTag>> {
        if !self.auto_install_enabled() {
            return Ok(None);
        }
        if self.state.read_current_job()?.is_some() {
            return Ok(None);
        }
        if self.refuse_update_if_stuck().is_err() {
            return Ok(None);
        }
        let st = self.state.read_updater()?;
        let Some(la) = st.latest_available.as_ref() else {
            return Ok(None);
        };
        if la.mode != install_mode {
            return Ok(None);
        }
        let effective = self.effective_mode();
        let prefs_allow = match effective {
            // Release-channel prefs only auto-install release tips.
            UpdateMode::Release => install_mode == UpdateMode::Release,
            // Dev/commit prefs: commit tips, or formal release tips discovered as tip.
            UpdateMode::Commit => {
                install_mode == UpdateMode::Commit
                    || (install_mode == UpdateMode::Release && la.version.is_release())
            }
        };
        if !prefs_allow {
            return Ok(None);
        }
        if !auto_install_latest_ok(
            install_mode,
            la.is_upgrade,
            la.is_downgrade,
            la.relation.as_deref(),
            la.requires_self_update,
            irreversible,
        ) {
            return Ok(None);
        }
        let target = la.version.clone();
        if st.current_version.as_ref() == Some(&target) {
            return Ok(None);
        }
        Ok(Some(target))
    }

    async fn dispatch_auto_install(
        self: &Arc<Self>,
        target: DeployTag,
        mode: UpdateMode,
    ) -> Result<()> {
        info!(
            target = %target,
            mode = %mode,
            channel = %self.effective_channel(),
            "auto_install: dispatching clear upgrade"
        );
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(Command::Update {
                target: target.clone(),
                mode,
                allow_downgrade: false,
                allow_risk: false,
                allow_tag_install: false,
                allow_diverged: None,
                allow_unknown: None,
                allow_irreversible: None,
                idempotency_key: Some(format!(
                    "auto-install-{}-{}",
                    mode.as_str(),
                    target.as_str()
                )),
                actor: Some("auto-install".into()),
                reply: tx,
            })
            .await
            .map_err(|_| UpdaterError::Conflict)?;
        let _ = rx.await.map_err(|_| {
            UpdaterError::Precondition("worker dropped auto_install reply".into())
        })??;
        Ok(())
    }

    /// Auto-install a clear release upgrade on the current channel (stable or preview).
    pub(crate) async fn maybe_auto_install_release(
        self: Arc<Self>,
        manifest: &Manifest,
    ) -> Result<()> {
        let Some(target) =
            self.auto_install_target_ok(UpdateMode::Release, manifest.migrations.irreversible)?
        else {
            return Ok(());
        };
        // Prefer the just-fetched manifest version when it matches the cache.
        let target = if target.as_str() == manifest.version.as_str() {
            DeployTag::from_release(manifest.version.clone())
        } else {
            target
        };
        self.dispatch_auto_install(target, UpdateMode::Release)
            .await
    }

    /// Auto-install a clear commit/dev tip upgrade on the current channel.
    /// Formal release tips discovered under commit prefs install via the release path
    /// (preflight/API also re-resolve mode from `target.is_release()`).
    pub(crate) async fn maybe_auto_install_commit(self: Arc<Self>, tag: &DeployTag) -> Result<()> {
        let install_mode = if tag.is_release() {
            UpdateMode::Release
        } else {
            UpdateMode::Commit
        };
        let Some(target) = self.auto_install_target_ok(install_mode, false)? else {
            return Ok(());
        };
        // Prefer the tip just reported by the check when it matches cache.
        let target = if target.as_str() == tag.as_str() {
            tag.clone()
        } else {
            target
        };
        self.dispatch_auto_install(target, install_mode).await
    }

    pub(crate) async fn handle_check_updates(
        self: Arc<Self>,
        channel_override: Option<String>,
        mode_override: Option<UpdateMode>,
    ) -> Result<Option<AvailableInfo>> {
        // Ephemeral overrides for this check only — never write prefs here.
        // A caller may redundantly send the currently saved values; that is still
        // the canonical check and must refresh status(). Only a genuinely different
        // preview request is kept out of the shared availability cache.
        let saved_channel = self.effective_channel();
        let saved_mode = self.effective_mode();
        let (channel, mode, persist_cache) =
            resolve_check_request(&saved_channel, saved_mode, channel_override, mode_override);
        match mode {
            UpdateMode::Release => self.check_release_available(&channel, persist_cache).await,
            UpdateMode::Commit => self.check_commit_available(&channel, persist_cache).await,
        }
    }

    async fn check_release_available(
        self: Arc<Self>,
        channel: &str,
        persist_cache: bool,
    ) -> Result<Option<AvailableInfo>> {
        let gh = self.github_client()?;
        let ch: Channel = channel.parse().unwrap_or(self.config.channel);
        let Some(rel) = gh.latest_for_channel(ch).await? else {
            if persist_cache {
                let mut st = self.state.read_updater()?;
                st.last_checked_at = Some(Utc::now());
                st.latest_available = None;
                self.state.write_updater(&st)?;
            }
            return Ok(None);
        };
        let manifest = gh.fetch_manifest(&rel.tag_name).await?;

        let self_v = MyriadVersion::parse(crate::self_version()).ok();
        let requires_self_update = manifest.updater.self_update_required
            || self_v
                .as_ref()
                .is_some_and(|v| v.older_than(&manifest.updater.min_updater_version));
        let target_tag = DeployTag::from_release(manifest.version.clone());
        let current_state = self.state.read_updater().ok();
        let current = current_state
            .as_ref()
            .and_then(|s| s.current_version.clone());
        // Prefer semver when both are releases; otherwise git ancestry.
        let (is_upgrade, is_downgrade, relation) = match (
            current.as_ref().and_then(|c| c.as_release()),
            target_tag.as_release(),
        ) {
            (None, _) => (Some(true), Some(false), Some("ahead".into())),
            (Some(c), Some(tgt)) if c.as_str() == tgt.as_str() => {
                (Some(false), Some(false), Some("identical".into()))
            }
            (Some(c), Some(tgt)) if c.older_than(&tgt) => {
                (Some(true), Some(false), Some("ahead".into()))
            }
            (Some(c), Some(tgt)) if tgt.older_than(&c) => {
                (Some(false), Some(true), Some("behind".into()))
            }
            _ => {
                // Cross-mode or non-orderable: try git compare.
                match gh
                    .compare_deploy_to_ref(current.as_ref(), target_tag.as_str())
                    .await
                {
                    Ok(Some(f)) => (
                        Some(f.is_upgrade()),
                        Some(f.is_downgrade()),
                        Some(f.relation.as_str().to_string()),
                    ),
                    _ => (None, None, Some("unknown".into())),
                }
            }
        };
        let target_commit_sha = match manifest.commit_sha.clone() {
            some @ Some(_) => some,
            None => gh
                .resolve_commit(&rel.tag_name)
                .await
                .ok()
                .map(|info| info.sha),
        };
        let cached = LatestAvailable {
            version: target_tag,
            channel: manifest.channel.clone(),
            mode: UpdateMode::Release,
            source: Some("github".to_string()),
            seen_at: Utc::now(),
            commit_sha: target_commit_sha,
            current_commit_sha: current_state.and_then(|s| s.current_commit_sha),
            relation,
            ahead_by: None,
            behind_by: None,
            is_upgrade,
            is_downgrade,
            requires_self_update,
            min_updater_version: Some(manifest.updater.min_updater_version.clone()),
            notes_url: manifest.notes_url.clone(),
        };

        if persist_cache {
            let mut st = self.state.read_updater()?;
            st.last_checked_at = Some(Utc::now());
            st.latest_available = Some(cached);
            self.state.write_updater(&st)?;
        }
        Ok(Some(AvailableInfo::Release(manifest)))
    }

    async fn check_commit_available(
        self: Arc<Self>,
        channel: &str,
        persist_cache: bool,
    ) -> Result<Option<AvailableInfo>> {
        let branch = commit_branch_for_channel(channel);

        // Dev channel tip = newest Docker Hub common build by pushed_at among BOTH
        // dev-* commits and formal vX.Y.Z releases. Time wins; no prefer-dev filter.
        match self.clone().handle_list_builds(25).await {
            Ok(builds) if !builds.is_empty() => {
                return self
                    .finish_dev_channel_tip_from_builds(branch, builds, persist_cache)
                    .await;
            }
            Ok(_) => {
                info!(
                    %branch,
                    "commit check: Docker Hub has no common builds; falling back to GitHub branch tip"
                );
            }
            Err(docker_error) => {
                if !self.github_commit_metadata_enabled() {
                    return Err(UpdaterError::DockerHub(format!(
                        "Docker Hub commit discovery failed ({docker_error}); \
                         GITHUB_TOKEN not set for branch-tip fallback"
                    )));
                }
                warn!(
                    err = %docker_error,
                    %branch,
                    "commit check: Docker Hub list failed; falling back to GitHub branch tip"
                );
            }
        }

        // No usable Docker Hub tip — GitHub branch tip only (when token present).
        if !self.github_commit_metadata_enabled() {
            return Err(UpdaterError::DockerHub(
                "Docker Hub has no common immutable frontend/backend build and \
                 GITHUB_TOKEN is unset (cannot resolve branch tip)"
                    .into(),
            ));
        }
        self.check_github_branch_tip_available(branch, persist_cache)
            .await
    }

    /// Finish availability for a Docker Hub tip (commit or formal release).
    async fn finish_dev_channel_tip_from_builds(
        self: Arc<Self>,
        branch: &str,
        builds: Vec<DockerBuild>,
        persist_cache: bool,
    ) -> Result<Option<AvailableInfo>> {
        let state_now = self.state.read_updater()?;
        let current = state_now.current_version.as_ref().map(|c| c.as_str());
        let current_commit_sha = state_now.current_commit_sha.as_deref();

        // Skip tip when it is the same artifact as the running deploy (e.g. v0.3.21
        // vs dev-<same-sha>, or short vs full dev-sha). Otherwise a re-tagged sibling
        // becomes "tip" and either thrash-upgrades or blocks seeing a later build.
        let Some(build) = select_dev_channel_tip_for(&builds, current, current_commit_sha) else {
            if persist_cache {
                let mut state = state_now;
                state.last_checked_at = Some(Utc::now());
                state.latest_available = None;
                self.state.write_updater(&state)?;
            }
            // List was non-empty but every common build is the running identity.
            return Ok(None);
        };
        // Clone tip fields we need past the shared builds borrow.
        let tip_tag = build.tag.clone();
        let tip_kind = build.kind;
        let tip_pushed = build.pushed_at.clone();
        let tip_short_sha = build.short_sha.clone();
        let tip_backend_url = build.backend_url.clone();

        let tag = DeployTag::parse(&tip_tag)?;
        let current_pushed = current.and_then(|c| pushed_at_for_tag(&builds, c));

        // Cross-kind: push time / semver is primary (no ancestry). Same-kind commits may use git.
        let cross_kind = is_cross_kind_deploy(tag.as_str(), current);
        let freshness = if !cross_kind && !tag.is_release() && self.github_commit_metadata_enabled()
        {
            match self.github_client() {
                Ok(gh) => {
                    // Always pass a git-resolvable ref (strip dev- prefix); bare
                    // `dev-<sha>` 404s on the GitHub commits API and drops ancestry.
                    let target_ref = crate::release::deploy_tag_to_git_ref(&tag);
                    match gh
                        .compare_deploy_to_ref(state_now.current_version.as_ref(), &target_ref)
                        .await
                    {
                        Ok(f) => f,
                        Err(e) => {
                            warn!(err = %e, "commit freshness compare failed");
                            None
                        }
                    }
                }
                Err(e) => {
                    warn!(err = %e, "github client unavailable for ancestry");
                    None
                }
            }
        } else {
            None
        };

        let direction = commit_upgrade_direction_ex(
            tag.as_str(),
            current,
            tip_pushed.as_deref(),
            current_pushed,
            if cross_kind { None } else { freshness.as_ref() },
            current_commit_sha,
            Some(tip_short_sha.as_str()),
        );

        info!(
            target = %tag,
            kind = tip_kind,
            %branch,
            is_upgrade = direction.is_upgrade,
            is_downgrade = direction.is_downgrade,
            relation = direction.relation,
            target_pushed = ?tip_pushed,
            current_pushed = ?current_pushed,
            cross_kind,
            "dev-channel tip from Docker Hub (push-time; commits + formal releases)"
        );

        if !direction.is_upgrade && !direction.is_downgrade {
            if persist_cache {
                let mut state = state_now;
                state.last_checked_at = Some(Utc::now());
                state.latest_available = None;
                self.state.write_updater(&state)?;
            }
            return Ok(None);
        }

        // Formal release tip → release path (manifest / auto-install / preflight).
        if tag.is_release() {
            return self
                .finish_dev_channel_release_tip(
                    branch,
                    tag,
                    tip_backend_url.as_str(),
                    &direction,
                    state_now,
                    persist_cache,
                )
                .await;
        }

        // Commit tip: optional GitHub metadata enrichment.
        let mut full_sha = tip_short_sha.clone();
        let mut message = "Docker Hub common frontend/backend build".to_string();
        let mut notes_url = tip_backend_url.clone();
        let source = "dockerhub";
        if self.github_commit_metadata_enabled() {
            if let Ok(gh) = self.github_client() {
                if let Ok(info) = gh.resolve_commit(&tip_short_sha).await {
                    full_sha = info.sha;
                    message = info.message;
                    notes_url = info.html_url;
                }
            }
        }

        let cached = LatestAvailable {
            version: tag.clone(),
            channel: branch.to_string(),
            mode: UpdateMode::Commit,
            source: Some(source.to_string()),
            seen_at: Utc::now(),
            commit_sha: Some(full_sha.clone()),
            current_commit_sha: freshness
                .as_ref()
                .and_then(|f| f.current_sha.clone())
                .or_else(|| state_now.current_commit_sha.clone()),
            relation: Some(direction.relation.to_string()),
            ahead_by: freshness.as_ref().map(|f| f.ahead_by),
            behind_by: freshness.as_ref().map(|f| f.behind_by),
            is_upgrade: Some(direction.is_upgrade),
            is_downgrade: Some(direction.is_downgrade),
            requires_self_update: false,
            min_updater_version: None,
            notes_url: notes_url.clone(),
        };
        if persist_cache {
            let mut state = state_now;
            state.last_checked_at = Some(Utc::now());
            state.latest_available = Some(cached);
            self.state.write_updater(&state)?;
        }

        Ok(Some(AvailableInfo::Commit {
            tag,
            full_sha,
            message,
            branch: branch.to_string(),
            notes_url,
            source: source.to_string(),
            freshness,
            is_upgrade: Some(direction.is_upgrade),
            is_downgrade: Some(direction.is_downgrade),
            relation: Some(direction.relation.to_string()),
        }))
    }

    /// Formal release discovered as dev-channel tip: cache + AvailableInfo use release path.
    async fn finish_dev_channel_release_tip(
        self: Arc<Self>,
        branch: &str,
        tag: DeployTag,
        tip_backend_url: &str,
        direction: &crate::release::CommitUpgradeDirection,
        state_now: crate::state::UpdaterStateFile,
        persist_cache: bool,
    ) -> Result<Option<AvailableInfo>> {
        // Prefer full release.json when GitHub is reachable.
        if let Ok(gh) = self.github_client() {
            match gh.fetch_manifest(tag.as_str()).await {
                Ok(manifest) => {
                    let self_v = MyriadVersion::parse(crate::self_version()).ok();
                    let requires_self_update = manifest.updater.self_update_required
                        || self_v
                            .as_ref()
                            .is_some_and(|v| v.older_than(&manifest.updater.min_updater_version));
                    let target_commit_sha = match manifest.commit_sha.clone() {
                        some @ Some(_) => some,
                        None => gh
                            .resolve_commit(tag.as_str())
                            .await
                            .ok()
                            .map(|info| info.sha),
                    };
                    let cached = LatestAvailable {
                        version: tag,
                        channel: branch.to_string(),
                        mode: UpdateMode::Release,
                        source: Some("dockerhub".to_string()),
                        seen_at: Utc::now(),
                        commit_sha: target_commit_sha,
                        current_commit_sha: state_now.current_commit_sha.clone(),
                        relation: Some(direction.relation.to_string()),
                        ahead_by: None,
                        behind_by: None,
                        is_upgrade: Some(direction.is_upgrade),
                        is_downgrade: Some(direction.is_downgrade),
                        requires_self_update,
                        min_updater_version: Some(manifest.updater.min_updater_version.clone()),
                        notes_url: manifest.notes_url.clone(),
                    };
                    if persist_cache {
                        let mut state = state_now;
                        state.last_checked_at = Some(Utc::now());
                        state.latest_available = Some(cached);
                        self.state.write_updater(&state)?;
                    }
                    return Ok(Some(AvailableInfo::Release(manifest)));
                }
                Err(e) if GithubClient::is_release_json_unavailable(&e) => {
                    warn!(err = %e, target = %tag, "release manifest unavailable; tag install requires confirmation");
                }
                Err(e) => return Err(e),
            }
        }

        // No manifest: still surface the formal release tip with release-mode cache so
        // status / auto-install use the release path (install re-resolves from tag).
        let cached = LatestAvailable {
            version: tag.clone(),
            channel: branch.to_string(),
            mode: UpdateMode::Release,
            source: Some("dockerhub".to_string()),
            seen_at: Utc::now(),
            commit_sha: None,
            current_commit_sha: state_now.current_commit_sha.clone(),
            relation: Some(direction.relation.to_string()),
            ahead_by: None,
            behind_by: None,
            is_upgrade: Some(direction.is_upgrade),
            is_downgrade: Some(direction.is_downgrade),
            requires_self_update: false,
            min_updater_version: None,
            notes_url: tip_backend_url.to_string(),
        };
        if persist_cache {
            let mut state = state_now;
            state.last_checked_at = Some(Utc::now());
            state.latest_available = Some(cached);
            self.state.write_updater(&state)?;
        }
        // Synthetic commit-shaped payload so /available still returns a tip without
        // release.json; mode field in cache remains Release for auto-install.
        Ok(Some(AvailableInfo::Commit {
            tag,
            full_sha: String::new(),
            message: "Docker Hub formal release build (dev-channel tip)".to_string(),
            branch: branch.to_string(),
            notes_url: tip_backend_url.to_string(),
            source: "dockerhub".to_string(),
            freshness: None,
            is_upgrade: Some(direction.is_upgrade),
            is_downgrade: Some(direction.is_downgrade),
            relation: Some(direction.relation.to_string()),
        }))
    }

    /// GitHub branch tip only — used when Docker Hub has no common builds.
    async fn check_github_branch_tip_available(
        self: Arc<Self>,
        branch: &str,
        persist_cache: bool,
    ) -> Result<Option<AvailableInfo>> {
        let gh = self.github_client()?;
        let info = match gh.latest_commit_on_branch(branch).await {
            Ok(i) => i,
            Err(e) => {
                if GithubClient::is_expected_unauthenticated_failure(&e) {
                    info!(
                        err = %e,
                        %branch,
                        "commit lookup: GitHub access denied/not found; no Docker Hub tip either"
                    );
                } else {
                    warn!(err = %e, %branch, "commit lookup failed");
                }
                return Err(e);
            }
        };
        let tag = DeployTag::parse(&format!("dev-{}", info.short_sha))?;
        let notes_url = info.html_url.clone();

        let st_now = self.state.read_updater()?;
        let freshness = match gh
            .compare_deploy_to_ref(st_now.current_version.as_ref(), branch)
            .await
        {
            Ok(f) => f,
            Err(e) => {
                warn!(err = %e, "commit freshness compare failed");
                None
            }
        };
        if let Some(ref f) = freshness {
            info!(
                relation = f.relation.as_str(),
                ahead = f.ahead_by,
                behind = f.behind_by,
                current = ?f.current_sha,
                target = ?f.target_sha,
                "commit freshness vs branch tip (no Docker Hub builds)"
            );
        }

        let direction = commit_upgrade_direction_ex(
            tag.as_str(),
            st_now.current_version.as_ref().map(|c| c.as_str()),
            None,
            None,
            freshness.as_ref(),
            st_now.current_commit_sha.as_deref(),
            Some(info.sha.as_str()),
        );
        info!(
            target = %tag,
            is_upgrade = direction.is_upgrade,
            is_downgrade = direction.is_downgrade,
            relation = direction.relation,
            "commit check: GitHub branch tip only (no Docker Hub common builds)"
        );

        if !direction.is_upgrade && !direction.is_downgrade {
            if persist_cache {
                let mut st = self.state.read_updater()?;
                st.last_checked_at = Some(Utc::now());
                st.latest_available = None;
                self.state.write_updater(&st)?;
            }
            return Ok(None);
        }

        let cached = LatestAvailable {
            version: tag.clone(),
            channel: branch.to_string(),
            mode: UpdateMode::Commit,
            source: Some("github".to_string()),
            seen_at: Utc::now(),
            commit_sha: Some(info.sha.clone()),
            current_commit_sha: freshness
                .as_ref()
                .and_then(|f| f.current_sha.clone())
                .or_else(|| st_now.current_commit_sha.clone()),
            relation: Some(direction.relation.to_string()),
            ahead_by: freshness.as_ref().map(|f| f.ahead_by),
            behind_by: freshness.as_ref().map(|f| f.behind_by),
            is_upgrade: Some(direction.is_upgrade),
            is_downgrade: Some(direction.is_downgrade),
            requires_self_update: false,
            min_updater_version: None,
            notes_url: notes_url.clone(),
        };
        if persist_cache {
            let mut st = self.state.read_updater()?;
            st.last_checked_at = Some(Utc::now());
            st.latest_available = Some(cached);
            self.state.write_updater(&st)?;
        }
        Ok(Some(AvailableInfo::Commit {
            tag,
            full_sha: info.sha,
            message: info.message,
            branch: branch.to_string(),
            notes_url,
            source: "github".to_string(),
            freshness,
            is_upgrade: Some(direction.is_upgrade),
            is_downgrade: Some(direction.is_downgrade),
            relation: Some(direction.relation.to_string()),
        }))
    }
}

/// Resolve the channel/mode for one check. Matching explicit values still
/// refresh the canonical availability cache (they are not a preview override).
pub(crate) fn resolve_check_request(
    saved_channel: &str,
    saved_mode: UpdateMode,
    channel_override: Option<String>,
    mode_override: Option<UpdateMode>,
) -> (String, UpdateMode, bool) {
    let channel = channel_override
        .map(|channel| channel.trim().to_ascii_lowercase())
        .filter(|channel| !channel.is_empty())
        .unwrap_or_else(|| saved_channel.to_string());
    let mode = mode_override.unwrap_or(saved_mode);
    let persist_cache = channel == saved_channel && mode == saved_mode;
    (channel, mode, persist_cache)
}

/// Pure auto-install gate used by the worker (and unit tests).
///
/// - **Release**: only clear, low-risk upgrades (`is_upgrade`, relation not
///   unknown/diverged/behind/identical).
/// - **Commit/dev**: `is_upgrade` is enough; `relation=unknown` is allowed
///   (build publish time / different tip). Still blocks behind/diverged/identical.
pub fn auto_install_latest_ok(
    mode: UpdateMode,
    is_upgrade: Option<bool>,
    is_downgrade: Option<bool>,
    relation: Option<&str>,
    requires_self_update: bool,
    irreversible: bool,
) -> bool {
    if irreversible {
        return false;
    }
    if requires_self_update {
        return false;
    }
    if is_upgrade != Some(true) || is_downgrade == Some(true) {
        return false;
    }
    match mode {
        UpdateMode::Release => !matches!(
            relation,
            Some("diverged") | Some("unknown") | Some("behind") | Some("identical")
        ),
        UpdateMode::Commit => {
            // unknown is explicitly allowed for commit/dev (Docker Hub / no ancestry).
            !matches!(
                relation,
                Some("diverged") | Some("behind") | Some("identical")
            )
        }
    }
}

#[cfg(test)]
mod check_request_tests {
    use super::*;

    #[test]
    fn matching_explicit_values_refresh_the_canonical_cache() {
        let (channel, mode, persist_cache) = resolve_check_request(
            "preview",
            UpdateMode::Commit,
            Some("preview".to_string()),
            Some(UpdateMode::Commit),
        );

        assert_eq!(channel, "preview");
        assert_eq!(mode, UpdateMode::Commit);
        assert!(persist_cache);
    }

    #[test]
    fn genuine_override_does_not_replace_saved_availability() {
        let (_, _, persist_cache) = resolve_check_request(
            "stable",
            UpdateMode::Release,
            Some("preview".to_string()),
            Some(UpdateMode::Commit),
        );

        assert!(!persist_cache);
    }

    #[test]
    fn omitted_values_use_and_refresh_saved_preferences() {
        let (channel, mode, persist_cache) =
            resolve_check_request("stable", UpdateMode::Release, None, None);

        assert_eq!(channel, "stable");
        assert_eq!(mode, UpdateMode::Release);
        assert!(persist_cache);
    }
}

#[cfg(test)]
mod auto_install_gate_tests {
    use super::*;

    #[test]
    fn commit_mode_allows_upgrade_with_unknown_relation() {
        assert!(auto_install_latest_ok(
            UpdateMode::Commit,
            Some(true),
            Some(false),
            Some("unknown"),
            false,
            false,
        ));
    }

    #[test]
    fn commit_mode_allows_upgrade_with_ahead_from_push_time() {
        assert!(auto_install_latest_ok(
            UpdateMode::Commit,
            Some(true),
            Some(false),
            Some("ahead"),
            false,
            false,
        ));
    }

    #[test]
    fn commit_mode_blocks_downgrade_and_diverged() {
        assert!(!auto_install_latest_ok(
            UpdateMode::Commit,
            Some(false),
            Some(true),
            Some("behind"),
            false,
            false,
        ));
        assert!(!auto_install_latest_ok(
            UpdateMode::Commit,
            Some(true),
            Some(false),
            Some("diverged"),
            false,
            false,
        ));
    }

    #[test]
    fn release_mode_still_blocks_unknown() {
        assert!(!auto_install_latest_ok(
            UpdateMode::Release,
            Some(true),
            Some(false),
            Some("unknown"),
            false,
            false,
        ));
        assert!(auto_install_latest_ok(
            UpdateMode::Release,
            Some(true),
            Some(false),
            Some("ahead"),
            false,
            false,
        ));
    }
}
