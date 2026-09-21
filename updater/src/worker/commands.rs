//! Thin worker command handlers: list / compare / dismiss.

use std::sync::Arc;

use tracing::info;

use crate::config::Channel;
use crate::error::{Result, UpdaterError};
use crate::release::{DockerBuild, GithubClient};
use crate::version::{DeployTag, commit_branch_for_channel};
use crate::worker::Worker;

impl Worker {
    pub(crate) async fn handle_list_commits(
        self: Arc<Self>,
        branch: String,
        limit: u32,
    ) -> Result<Vec<crate::release::CommitInfo>> {
        let branch = if branch.trim().is_empty() {
            commit_branch_for_channel(&self.effective_channel()).to_string()
        } else {
            commit_branch_for_channel(branch.trim()).to_string()
        };
        // Empty list → API/UI falls through to Docker Hub /builds (private repo, no token).
        if !self.github_commit_metadata_enabled() {
            info!(
                %branch,
                "commit list: GITHUB_TOKEN unset; returning empty (use Docker Hub builds)"
            );
            return Ok(Vec::new());
        }
        let gh = self.github_client()?;
        match gh.list_commits(&branch, limit).await {
            Ok(items) => Ok(items),
            Err(e) if GithubClient::is_expected_unauthenticated_failure(&e) => {
                info!(
                    err = %e,
                    %branch,
                    "commit list: GitHub unavailable; returning empty for Docker Hub fallback"
                );
                Ok(Vec::new())
            }
            Err(e) => Err(e),
        }
    }

    pub(crate) async fn handle_list_builds(
        self: Arc<Self>,
        limit: u32,
    ) -> Result<Vec<DockerBuild>> {
        let (backend, frontend) = self.image_repos_required()?;
        self.dockerhub_client()?
            .list_common_builds(&backend, &frontend, limit)
            .await
    }

    pub(crate) async fn handle_list_releases(
        self: Arc<Self>,
        channel: Option<String>,
        limit: u32,
    ) -> Result<Vec<crate::release::github::Release>> {
        let ch_name = channel.filter(|c| !c.trim().is_empty()).unwrap_or_else(|| {
            crate::version::release_channel_name_for_self_update(&self.effective_channel())
                .to_string()
        });
        let ch: Channel = ch_name.parse().unwrap_or(self.config.channel);
        let gh = self.github_client()?;
        gh.list_releases_for_channel(ch, limit).await
    }

    pub(crate) async fn handle_compare(
        self: Arc<Self>,
        from: Option<String>,
        to: String,
    ) -> Result<crate::release::Freshness> {
        let gh = self.github_client()?;
        let current = match from {
            Some(s) if !s.trim().is_empty() => Some(DeployTag::parse(s.trim())?),
            _ => self.state.read_updater()?.current_version,
        };
        gh.compare_deploy_to_ref(current.as_ref(), to.trim())
            .await?
            .ok_or_else(|| {
                UpdaterError::Precondition(
                    "could not resolve current deploy tag to a git commit for comparison".into(),
                )
            })
    }

    pub(crate) fn handle_dismiss_last_failed(self: Arc<Self>) -> Result<()> {
        let mut st = self.state.read_updater()?;
        if st.last_failed_update.is_none() {
            return Ok(());
        }
        st.last_failed_update = None;
        self.state.write_updater(&st)?;
        let _ = self
            .state
            .append_audit("audit: last_failed_update_dismissed");
        Ok(())
    }

    pub(crate) fn handle_dismiss_state_file(
        self: Arc<Self>,
        name: &str,
        audit: &str,
    ) -> Result<()> {
        let path = self.cli.state_dir.join(name);
        self.require_no_component_update()?;
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let _ = self.state.append_audit(audit);
        Ok(())
    }
}
