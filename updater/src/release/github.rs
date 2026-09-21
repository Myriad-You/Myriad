//! Minimal GitHub Release API client. Only needs:
//!   - list releases for a repo (filter by channel)
//!   - download release.json asset from a specific release
//!
//! Respects ETags via a small file cache under state/cache/.

use std::path::PathBuf;
use std::time::Duration;

use reqwest::Client;
use reqwest::header::{ACCEPT, AUTHORIZATION, HeaderMap, HeaderValue, IF_NONE_MATCH, USER_AGENT};
use serde::Deserialize;

use crate::config::{Channel, SecretString};
use crate::error::{Result, UpdaterError};
use crate::release::{CosignPolicy, Manifest, VerifyOutcome, cosign};
use crate::state::atomic;

pub struct GithubClient {
    repo: String,
    token: Option<SecretString>,
    client: Client,
    cache_dir: PathBuf,
    cosign_policy: CosignPolicy,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Release {
    pub tag_name: String,
    pub name: Option<String>,
    pub prerelease: bool,
    pub draft: bool,
    pub assets: Vec<Asset>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Asset {
    pub url: String,
    pub name: String,
    pub browser_download_url: String,
    #[serde(default)]
    pub size: u64,
}

impl GithubClient {
    /// Templates come from the selected source revision, including dev builds
    /// and v0.5.3, which has no Compose release asset.
    pub async fn compose_template(&self, revision: &str, external: bool) -> Result<Vec<u8>> {
        let variant = if external { "external" } else { "bundled" };
        let key: String = url::form_urlencoded::byte_serialize(revision.as_bytes()).collect();
        let cache = self.cache_dir.join(format!("compose-{key}-{variant}.yaml"));
        if let Some(bytes) = crate::state::read_existing(&cache)? {
            return Ok(bytes);
        }
        let path = if external {
            "docs/deployment/examples/docker-compose.external-db.example.yml"
        } else {
            "docker-compose.yml"
        };
        let response = self
            .client
            .get(format!(
                "https://api.github.com/repos/{}/contents/{path}?ref={key}",
                self.repo
            ))
            .headers(self.auth_headers())
            .header(ACCEPT, "application/vnd.github.raw+json")
            .send()
            .await
            .map_err(|e| UpdaterError::Github(format!("fetch deployment template: {e}")))?
            .error_for_status()
            .map_err(|e| UpdaterError::Github(format!("fetch deployment template: {e}")))?;
        let bytes = response
            .bytes()
            .await
            .map_err(|e| UpdaterError::Github(e.to_string()))?
            .to_vec();
        atomic::write_atomic_bytes(&cache, &bytes)?;
        Ok(bytes)
    }

    /// True when an error is the expected "no access / private / missing" class that
    /// commit-mode should treat as "use Docker Hub" rather than a hard failure.
    pub fn is_expected_unauthenticated_failure(err: &UpdaterError) -> bool {
        let s = err.to_string();
        // GitHub returns 404 for private repos without a token (repo "not found").
        s.contains("404")
            || s.contains("401")
            || s.contains("403")
            || s.contains("Not Found")
            || s.contains("Bad credentials")
            || s.contains("Requires authentication")
            || s.contains("API rate limit")
    }

    /// True when `release.json` cannot be obtained and callers may fall back to Docker Hub.
    ///
    /// Cosign failures and invalid downloaded JSON must **not** fall back — fail closed.
    /// Used by release preflight, proxy-update, and self-update.
    pub fn is_release_json_unavailable(err: &UpdaterError) -> bool {
        match err {
            UpdaterError::Github(_) | UpdaterError::Io(_) => true,
            // Cosign enforce returns Precondition("cosign: ...") — never fall back.
            UpdaterError::Precondition(msg) if msg.starts_with("cosign:") => false,
            // Manifest::from_json / validate after a successful download — fail closed.
            UpdaterError::Json(_) | UpdaterError::Precondition(_) => false,
            other => {
                // Network / client build oddities may surface as Internal(anyhow).
                Self::is_expected_unauthenticated_failure(other)
                    || other.to_string().to_ascii_lowercase().contains("timeout")
                    || other
                        .to_string()
                        .to_ascii_lowercase()
                        .contains("connection")
            }
        }
    }

    pub fn new(
        repo: impl Into<String>,
        token: Option<SecretString>,
        cache_dir: PathBuf,
        cosign_policy: CosignPolicy,
    ) -> Result<Self> {
        let client = Client::builder()
            .user_agent("myriad-updater")
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| UpdaterError::Github(format!("client build: {e}")))?;
        Ok(Self {
            repo: repo.into(),
            token,
            client,
            cache_dir,
            cosign_policy,
        })
    }

    fn auth_headers(&self) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(USER_AGENT, HeaderValue::from_static("myriad-updater"));
        h.insert(
            ACCEPT,
            HeaderValue::from_static("application/vnd.github+json"),
        );
        if let Some(t) = &self.token
            && let Ok(v) = HeaderValue::from_str(&format!("Bearer {}", t.expose()))
        {
            h.insert(AUTHORIZATION, v);
        }
        h
    }

    /// Returns releases newest-first.
    pub async fn list_releases(&self) -> Result<Vec<Release>> {
        let url = format!(
            "https://api.github.com/repos/{}/releases?per_page=20",
            self.repo
        );
        let resp = self
            .client
            .get(&url)
            .headers(self.auth_headers())
            .send()
            .await
            .map_err(|e| UpdaterError::Github(format!("GET releases: {e}")))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(UpdaterError::Github(format!(
                "GET releases failed: {status} {body}"
            )));
        }
        resp.json::<Vec<Release>>()
            .await
            .map_err(|e| UpdaterError::Github(format!("decode releases: {e}")))
    }

    /// True when a release tag belongs to the given channel filter.
    ///
    /// Myriad ships formal `vX.Y.Z` tags (non-prerelease) as the primary train.
    /// **Stable**: formal only (no prerelease, no `*-preview.*` tags).
    /// **Preview**: all non-draft releases (formal + prerelease / preview-marked)
    /// so the Preview channel still sees subsequent formal versions.
    pub fn release_matches_channel(tag: &str, prerelease: bool, channel: Channel) -> bool {
        match channel {
            Channel::Stable => !prerelease && !is_marked(tag, "preview"),
            // Drafts are already filtered by list_releases_for_channel.
            Channel::Preview => true,
        }
    }

    /// Pick the newest release matching the requested channel (and ignoring drafts).
    pub async fn latest_for_channel(&self, channel: Channel) -> Result<Option<Release>> {
        let releases = self.list_releases_for_channel(channel, 1).await?;
        Ok(releases.into_iter().next())
    }

    /// Releases for a channel, newest first (drafts excluded).
    pub async fn list_releases_for_channel(
        &self,
        channel: Channel,
        limit: u32,
    ) -> Result<Vec<Release>> {
        let limit = limit.clamp(1, 50) as usize;
        let releases = self.list_releases().await?;
        Ok(releases
            .into_iter()
            .filter(|r| {
                !r.draft && Self::release_matches_channel(&r.tag_name, r.prerelease, channel)
            })
            .take(limit)
            .collect())
    }

    /// Resolve any git ref (branch, tag, full/short sha) to a commit.
    pub async fn resolve_commit(&self, rev: &str) -> Result<CommitInfo> {
        let rev = rev.trim();
        if rev.is_empty() {
            return Err(UpdaterError::InvalidInput("empty git ref".into()));
        }
        // Percent-encode so tags like `v0.1.0` and shas are safe in the path.
        let enc = urlencoding_minimal(rev);
        let url = format!("https://api.github.com/repos/{}/commits/{}", self.repo, enc);
        let resp = self
            .client
            .get(&url)
            .headers(self.auth_headers())
            .send()
            .await
            .map_err(|e| UpdaterError::Github(format!("GET commits/{rev}: {e}")))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(UpdaterError::Github(format!(
                "GET commits/{rev} failed: {status} {body}"
            )));
        }
        let raw: GhCommit = resp
            .json()
            .await
            .map_err(|e| UpdaterError::Github(format!("decode commit: {e}")))?;
        let sha = raw.sha;
        if sha.len() < 7 {
            return Err(UpdaterError::Github(format!(
                "unexpected short sha from GitHub: {sha}"
            )));
        }
        let committed_at = raw
            .commit
            .committer
            .as_ref()
            .and_then(|c| c.date.clone())
            .or_else(|| raw.commit.author.as_ref().and_then(|a| a.date.clone()));
        Ok(CommitInfo {
            sha: sha.clone(),
            short_sha: sha[..7].to_string(),
            message: raw.commit.message.lines().next().unwrap_or("").to_string(),
            html_url: raw.html_url,
            committed_at,
        })
    }

    /// Latest commit on a branch (alias of [`resolve_commit`] for readability).
    pub async fn latest_commit_on_branch(&self, branch: &str) -> Result<CommitInfo> {
        self.resolve_commit(branch).await
    }

    /// List recent commits on a branch (newest first). Used by the UI commit picker.
    pub async fn list_commits(&self, branch: &str, limit: u32) -> Result<Vec<CommitInfo>> {
        let limit = limit.clamp(1, 50);
        let url = format!(
            "https://api.github.com/repos/{}/commits?sha={}&per_page={}",
            self.repo,
            urlencoding_minimal(branch),
            limit
        );
        let resp = self
            .client
            .get(&url)
            .headers(self.auth_headers())
            .send()
            .await
            .map_err(|e| UpdaterError::Github(format!("GET commits?sha={branch}: {e}")))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(UpdaterError::Github(format!(
                "GET commits?sha={branch} failed: {status} {body}"
            )));
        }
        let raw: Vec<GhCommit> = resp
            .json()
            .await
            .map_err(|e| UpdaterError::Github(format!("decode commit list: {e}")))?;
        Ok(raw
            .into_iter()
            .filter_map(|c| {
                if c.sha.len() < 7 {
                    return None;
                }
                let committed_at = c
                    .commit
                    .committer
                    .as_ref()
                    .and_then(|p| p.date.clone())
                    .or_else(|| c.commit.author.as_ref().and_then(|p| p.date.clone()));
                Some(CommitInfo {
                    sha: c.sha.clone(),
                    short_sha: c.sha[..7].to_string(),
                    message: c.commit.message.lines().next().unwrap_or("").to_string(),
                    html_url: c.html_url,
                    committed_at,
                })
            })
            .collect())
    }

    /// Compare two refs via GitHub: is `head` ahead/behind/identical/diverged relative to `base`?
    ///
    /// This is ancestry-based (merge-base), **not** wall-clock time.  
    /// `status == "ahead"` means `head` has commits that `base` does not → `head` is "newer"
    /// along that line of history.
    pub async fn compare(&self, base: &str, head: &str) -> Result<CompareResult> {
        let url = format!(
            "https://api.github.com/repos/{}/compare/{}...{}",
            self.repo,
            urlencoding_minimal(base),
            urlencoding_minimal(head)
        );
        let resp = self
            .client
            .get(&url)
            .headers(self.auth_headers())
            .send()
            .await
            .map_err(|e| UpdaterError::Github(format!("GET compare {base}...{head}: {e}")))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(UpdaterError::Github(format!(
                "GET compare {base}...{head} failed: {status} {body}"
            )));
        }
        let raw: GhCompare = resp
            .json()
            .await
            .map_err(|e| UpdaterError::Github(format!("decode compare: {e}")))?;
        Ok(CompareResult {
            status: CommitRelation::parse(&raw.status),
            ahead_by: raw.ahead_by,
            behind_by: raw.behind_by,
            base_sha: raw.base_commit.map(|c| c.sha).unwrap_or_default(),
            merge_base_sha: raw.merge_base_commit.map(|c| c.sha).unwrap_or_default(),
        })
    }

    /// Compare current deploy tag vs a target git ref; returns None if current cannot be resolved.
    ///
    /// `target_ref` may be a bare branch/sha, a formal `vX.Y.Z` tag, or a Docker
    /// `dev-<sha>` tag — the latter is normalized via [`deploy_tag_to_git_ref`].
    pub async fn compare_deploy_to_ref(
        &self,
        current: Option<&crate::version::DeployTag>,
        target_ref: &str,
    ) -> Result<Option<Freshness>> {
        // Normalize Docker Hub commit tags (`dev-abc1234`) → bare sha so GitHub
        // `/commits/{ref}` resolves. Passing `dev-…` literally 404s and dropped
        // ancestry for the whole dev-channel tip path.
        let target_ref = match crate::version::DeployTag::parse(target_ref.trim()) {
            Ok(t) => deploy_tag_to_git_ref(&t),
            Err(_) => target_ref.trim().to_string(),
        };
        let Some(curr) = current else {
            // No recorded version → anything is "newer".
            let tip = self.resolve_commit(&target_ref).await?;
            return Ok(Some(Freshness {
                relation: CommitRelation::Ahead,
                ahead_by: 1,
                behind_by: 0,
                current_sha: None,
                target_sha: Some(tip.sha),
                current_ref: None,
                target_ref: target_ref.to_string(),
            }));
        };
        let current_ref = deploy_tag_to_git_ref(curr);
        let curr_info = match self.resolve_commit(&current_ref).await {
            Ok(i) => i,
            Err(e) => {
                if Self::is_expected_unauthenticated_failure(&e) {
                    tracing::info!(
                        err = %e,
                        tag = %curr,
                        "current deploy tag not resolvable on GitHub (private/no access); relation unknown"
                    );
                } else {
                    tracing::warn!(
                        err = %e,
                        tag = %curr,
                        "could not resolve current deploy tag to a git commit"
                    );
                }
                return Ok(None);
            }
        };
        let tip = self.resolve_commit(&target_ref).await?;
        if curr_info.sha == tip.sha {
            return Ok(Some(Freshness {
                relation: CommitRelation::Identical,
                ahead_by: 0,
                behind_by: 0,
                current_sha: Some(curr_info.sha),
                target_sha: Some(tip.sha),
                current_ref: Some(current_ref),
                target_ref: target_ref.to_string(),
            }));
        }
        let cmp = self.compare(&curr_info.sha, &tip.sha).await?;
        Ok(Some(Freshness {
            relation: cmp.status,
            ahead_by: cmp.ahead_by,
            behind_by: cmp.behind_by,
            current_sha: Some(curr_info.sha),
            target_sha: Some(tip.sha),
            current_ref: Some(current_ref),
            target_ref: target_ref.to_string(),
        }))
    }

    /// Download release.json for the given tag, optionally verifying the cosign signature.
    /// Uses ETag/If-None-Match cache for the manifest blob.
    pub async fn fetch_manifest(&self, tag: &str) -> Result<Manifest> {
        let release = self.get_release_by_tag(tag).await?;
        let manifest_asset = release
            .assets
            .iter()
            .find(|a| a.name == "release.json")
            .ok_or_else(|| {
                UpdaterError::Github(format!("release {tag} has no release.json asset"))
            })?;
        let bytes = self.download_with_cache(tag, manifest_asset).await?;

        // 1) parse + structural validation
        let manifest = Manifest::from_json(&bytes)?;

        // 2) cosign verification (governed by policy)
        if !matches!(self.cosign_policy, CosignPolicy::Off) {
            let outcome = self.verify_cosign(tag, &release, &bytes).await;
            if let Err(e) = cosign::enforce(&outcome, self.cosign_policy) {
                return Err(UpdaterError::Precondition(format!("cosign: {e}")));
            }
        }

        Ok(manifest)
    }

    /// Fetch .sig + .pem siblings for `release.json` and ask cosign to verify them.
    /// Returns `VerifyOutcome::Skipped` if the assets are missing — the policy layer decides
    /// whether that's acceptable.
    async fn verify_cosign(
        &self,
        tag: &str,
        release: &Release,
        manifest_bytes: &[u8],
    ) -> VerifyOutcome {
        let sig_asset = release.assets.iter().find(|a| a.name == "release.json.sig");
        let pem_asset = release.assets.iter().find(|a| a.name == "release.json.pem");
        let (Some(sig), Some(pem)) = (sig_asset, pem_asset) else {
            return VerifyOutcome::Skipped;
        };

        // Write the manifest + sig + pem to the cache dir so cosign can `--signature path`.
        let mp = self.cache_dir.join(format!("release-{tag}.json"));
        let sp = self.cache_dir.join(format!("release-{tag}.sig"));
        let cp = self.cache_dir.join(format!("release-{tag}.pem"));
        if let Err(e) = atomic::write_atomic_bytes(&mp, manifest_bytes) {
            return VerifyOutcome::Failed(format!("write cache manifest: {e}"));
        }
        let sig_bytes = match self.fetch_asset_bytes(sig).await {
            Ok(b) => b,
            Err(e) => return VerifyOutcome::Failed(format!("download sig: {e}")),
        };
        let pem_bytes = match self.fetch_asset_bytes(pem).await {
            Ok(b) => b,
            Err(e) => return VerifyOutcome::Failed(format!("download cert: {e}")),
        };
        if let Err(e) = atomic::write_atomic_bytes(&sp, &sig_bytes) {
            return VerifyOutcome::Failed(format!("write sig: {e}"));
        }
        if let Err(e) = atomic::write_atomic_bytes(&cp, &pem_bytes) {
            return VerifyOutcome::Failed(format!("write cert: {e}"));
        }

        cosign::verify(&mp, &sp, &cp, &self.repo).await
    }

    /// One-shot asset download (no cache; used for short-lived .sig/.pem).
    async fn fetch_asset_bytes(&self, asset: &Asset) -> Result<Vec<u8>> {
        let mut headers = self.auth_headers();
        headers.insert(ACCEPT, HeaderValue::from_static("application/octet-stream"));
        let resp = self
            .client
            .get(&asset.url)
            .headers(headers)
            .send()
            .await
            .map_err(|e| UpdaterError::Github(format!("download {}: {e}", asset.name)))?;
        if !resp.status().is_success() {
            return Err(UpdaterError::Github(format!(
                "download {} failed: {}",
                asset.name,
                resp.status()
            )));
        }
        Ok(resp
            .bytes()
            .await
            .map_err(|e| UpdaterError::Github(format!("read body: {e}")))?
            .to_vec())
    }

    async fn get_release_by_tag(&self, tag: &str) -> Result<Release> {
        let url = format!(
            "https://api.github.com/repos/{}/releases/tags/{}",
            self.repo, tag
        );
        let resp = self
            .client
            .get(&url)
            .headers(self.auth_headers())
            .send()
            .await
            .map_err(|e| UpdaterError::Github(format!("GET release by tag: {e}")))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(UpdaterError::Github(format!(
                "GET release {tag} failed: {status} {body}"
            )));
        }
        resp.json::<Release>()
            .await
            .map_err(|e| UpdaterError::Github(format!("decode release: {e}")))
    }

    async fn download_with_cache(&self, tag: &str, asset: &Asset) -> Result<Vec<u8>> {
        std::fs::create_dir_all(&self.cache_dir)?;
        let cache_path = self.cache_dir.join(format!("release-{tag}.json"));
        let etag_path = self.cache_dir.join(format!("release-{tag}.etag"));

        let mut headers = self.auth_headers();
        // Use the GitHub API asset URL, not browser_download_url. The browser URL is
        // not token-authenticated reliably for private repositories.
        headers.insert(ACCEPT, HeaderValue::from_static("application/octet-stream"));
        if let Ok(etag) = std::fs::read_to_string(&etag_path)
            && let Ok(v) = HeaderValue::from_str(etag.trim())
        {
            headers.insert(IF_NONE_MATCH, v);
        }

        let resp = self
            .client
            .get(&asset.url)
            .headers(headers)
            .send()
            .await
            .map_err(|e| UpdaterError::Github(format!("download manifest: {e}")))?;

        if resp.status() == reqwest::StatusCode::NOT_MODIFIED
            && crate::probe::filesystem::path_is_present(&cache_path)?
        {
            return Ok(std::fs::read(&cache_path)?);
        }
        if !resp.status().is_success() {
            return Err(UpdaterError::Github(format!(
                "download {} failed: {}",
                asset.name,
                resp.status()
            )));
        }

        let etag = resp
            .headers()
            .get(reqwest::header::ETAG)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| UpdaterError::Github(format!("read body: {e}")))?
            .to_vec();
        atomic::write_atomic_bytes(&cache_path, &bytes)?;
        if let Some(e) = etag {
            atomic::write_atomic_bytes(&etag_path, e.as_bytes())?;
        }
        Ok(bytes)
    }
}

fn is_marked(tag: &str, marker: &str) -> bool {
    tag.contains(&format!("-{marker}."))
}

#[cfg(test)]
mod channel_filter_tests {
    use super::*;
    use crate::config::Channel;

    #[test]
    fn stable_excludes_prerelease_and_preview_tags() {
        assert!(GithubClient::release_matches_channel(
            "v1.0.0",
            false,
            Channel::Stable
        ));
        assert!(!GithubClient::release_matches_channel(
            "v1.0.0-preview.1",
            true,
            Channel::Stable
        ));
        assert!(!GithubClient::release_matches_channel(
            "v1.0.0",
            true,
            Channel::Stable
        ));
    }

    #[test]
    fn preview_matches_formal_and_prerelease() {
        assert!(GithubClient::release_matches_channel(
            "v1.0.0-preview.20260101",
            true,
            Channel::Preview
        ));
        assert!(GithubClient::release_matches_channel(
            "v1.0.0-rc.1",
            true,
            Channel::Preview
        ));
        // Formal train is what Myriad actually ships — must still match preview.
        assert!(GithubClient::release_matches_channel(
            "v1.0.0",
            false,
            Channel::Preview
        ));
        assert!(GithubClient::release_matches_channel(
            "v0.3.21",
            false,
            Channel::Preview
        ));
    }
}

/// Map a running deploy tag to a GitHub ref we can resolve.
/// - `v1.2.3` → tag `v1.2.3`
/// - `dev-abc1234` → sha `abc1234`
/// - `main`/`preview`/`beta` → branch name
pub fn deploy_tag_to_git_ref(tag: &crate::version::DeployTag) -> String {
    use crate::version::DeployTagKind;
    match tag.kind() {
        DeployTagKind::Release => tag.as_str().to_string(),
        DeployTagKind::Commit => tag.commit_sha().unwrap_or(tag.as_str()).to_string(),
        DeployTagKind::Branch => tag.as_str().to_string(),
    }
}

/// Minimal path-segment encoding for refs (keep `/` out; encode `#?%` etc.).
fn urlencoding_minimal(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push_str(&format!("{b:02X}"));
            }
        }
    }
    out
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CommitInfo {
    pub sha: String,
    pub short_sha: String,
    pub message: String,
    pub html_url: String,
    pub committed_at: Option<String>,
}

/// Result of GitHub compare base...head (ancestry).
#[derive(Debug, Clone)]
pub struct CompareResult {
    pub status: CommitRelation,
    pub ahead_by: u32,
    pub behind_by: u32,
    pub base_sha: String,
    pub merge_base_sha: String,
}

/// How `head` (target) sits relative to `base` (current).
#[derive(Debug, Clone, Copy, Eq, PartialEq, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CommitRelation {
    /// head has commits base does not → target is newer along history.
    Ahead,
    /// head is missing commits that base has → target is older.
    Behind,
    /// same commit.
    Identical,
    /// both sides have unique commits.
    Diverged,
    #[default]
    Unknown,
}

impl CommitRelation {
    pub fn parse(s: &str) -> Self {
        match s {
            "ahead" => Self::Ahead,
            "behind" => Self::Behind,
            "identical" => Self::Identical,
            "diverged" => Self::Diverged,
            _ => Self::Unknown,
        }
    }

    /// True when moving from base → head is an upgrade (target has new work).
    pub fn is_upgrade(self) -> bool {
        matches!(self, Self::Ahead | Self::Diverged)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ahead => "ahead",
            Self::Behind => "behind",
            Self::Identical => "identical",
            Self::Diverged => "diverged",
            Self::Unknown => "unknown",
        }
    }
}

/// Human/UI-facing freshness of target vs currently running deploy.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Freshness {
    pub relation: CommitRelation,
    pub ahead_by: u32,
    pub behind_by: u32,
    pub current_sha: Option<String>,
    pub target_sha: Option<String>,
    pub current_ref: Option<String>,
    pub target_ref: String,
}

impl Freshness {
    /// Whether applying the target is considered an upgrade (target has new commits).
    pub fn is_upgrade(&self) -> bool {
        match self.relation {
            CommitRelation::Ahead => true,
            // Diverged with new commits on target still counts as "can move forward"
            // (UI will require allow_risk for non-linear history).
            CommitRelation::Diverged => self.ahead_by > 0,
            CommitRelation::Identical | CommitRelation::Behind => false,
            CommitRelation::Unknown => false,
        }
    }

    /// Target is strictly older along history (safe to label as downgrade).
    pub fn is_downgrade(&self) -> bool {
        match self.relation {
            CommitRelation::Behind => true,
            // Pure rewind on a diverged graph (no unique commits on target).
            CommitRelation::Diverged => self.behind_by > 0 && self.ahead_by == 0,
            _ => false,
        }
    }

    /// Back-compat alias used by older call sites.
    pub fn update_available(&self) -> bool {
        self.is_upgrade()
    }
}

#[derive(Debug, Deserialize)]
struct GhCommit {
    sha: String,
    html_url: String,
    commit: GhCommitInner,
}

#[derive(Debug, Deserialize)]
struct GhCommitInner {
    message: String,
    #[serde(default)]
    author: Option<GhCommitPerson>,
    #[serde(default)]
    committer: Option<GhCommitPerson>,
}

#[derive(Debug, Deserialize)]
struct GhCommitPerson {
    #[serde(default)]
    date: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GhCompare {
    status: String,
    #[serde(default)]
    ahead_by: u32,
    #[serde(default)]
    behind_by: u32,
    #[serde(default)]
    base_commit: Option<GhCommitSha>,
    #[serde(default)]
    merge_base_commit: Option<GhCommitSha>,
}

#[derive(Debug, Deserialize)]
struct GhCommitSha {
    sha: String,
}
