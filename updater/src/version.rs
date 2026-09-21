//! Version / deploy-tag parsing and comparison.
//!
//! Two families of tags are managed by the updater:
//!
//! 1. **Release** — `v`-prefixed semver (`v1.2.3`, `v1.2.3-beta.1`). Used with
//!    GitHub Release + `release.json`.
//! 2. **Commit / branch tip** — CI dev images from `docker-publish.yml`:
//!    - `dev-<shortsha>` (7–40 hex) for a specific commit
//!    - bare branch names `main` / `preview` / `beta` for the latest image on that branch
//!
//! [`MyriadVersion`] is the strict release-only type (manifests, min_from_version).
//! [`DeployTag`] is the broader type stored in state / jobs / MYRIAD_TAG.

use once_cell::sync::Lazy;
use regex::Regex;
use semver::Version;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

use crate::error::UpdaterError;

static VERSION_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^v(?P<sv>[0-9]+\.[0-9]+\.[0-9]+(?:-[a-z0-9.]+)?)$").unwrap());

static COMMIT_DEV_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^dev-(?P<sha>[0-9a-f]{7,40})$").unwrap());

static BARE_SHA_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[0-9a-f]{7,40}$").unwrap());

const BRANCH_TIPS: &[&str] = &["main", "preview", "beta"];

/// How the operator wants to consume updates.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum UpdateMode {
    /// GitHub Release + release.json (production path).
    #[default]
    Release,
    /// CI images tagged by commit / branch tip (dev / preview path).
    Commit,
}

impl UpdateMode {
    pub fn as_str(self) -> &'static str {
        match self {
            UpdateMode::Release => "release",
            UpdateMode::Commit => "commit",
        }
    }
}

impl fmt::Display for UpdateMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for UpdateMode {
    type Err = UpdaterError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "release" | "releases" | "version" | "semver" => Ok(UpdateMode::Release),
            "commit" | "commits" | "sha" | "dev" => Ok(UpdateMode::Commit),
            other => Err(UpdaterError::InvalidInput(format!(
                "unknown update mode: {other} (expected release|commit)"
            ))),
        }
    }
}

/// A validated `v`-prefixed Myriad release version.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct MyriadVersion {
    raw: String,
    inner: Version,
}

impl MyriadVersion {
    pub fn parse(s: &str) -> Result<Self, UpdaterError> {
        let caps = VERSION_RE
            .captures(s)
            .ok_or_else(|| UpdaterError::InvalidInput(format!("invalid version: {s}")))?;
        let sv = &caps["sv"];
        let inner = Version::parse(sv)
            .map_err(|e| UpdaterError::InvalidInput(format!("invalid version {s}: {e}")))?;
        Ok(Self {
            raw: s.to_string(),
            inner,
        })
    }

    pub fn as_str(&self) -> &str {
        &self.raw
    }

    pub fn semver(&self) -> &Version {
        &self.inner
    }

    /// True when this version is older than `other` according to semver precedence.
    pub fn older_than(&self, other: &MyriadVersion) -> bool {
        self.inner < other.inner
    }
}

impl fmt::Display for MyriadVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.raw)
    }
}

impl FromStr for MyriadVersion {
    type Err = UpdaterError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl Serialize for MyriadVersion {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(&self.raw)
    }
}

impl<'de> Deserialize<'de> for MyriadVersion {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        let s = String::deserialize(de)?;
        Self::parse(&s).map_err(serde::de::Error::custom)
    }
}

/// Kind of deploy tag.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum DeployTagKind {
    Release,
    /// `dev-<sha>` CI image for a specific commit.
    Commit,
    /// Branch-tip docker tag (`main` / `preview` / `beta`).
    Branch,
}

/// Any image tag the updater may write into `MYRIAD_TAG` / record in state.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct DeployTag {
    raw: String,
    kind: DeployTagKind,
}

impl DeployTag {
    pub fn parse(s: &str) -> Result<Self, UpdaterError> {
        let s = s.trim();
        if s.is_empty() {
            return Err(UpdaterError::InvalidInput("empty deploy tag".into()));
        }
        if let Ok(v) = MyriadVersion::parse(s) {
            return Ok(Self {
                raw: v.raw,
                kind: DeployTagKind::Release,
            });
        }
        if let Some(caps) = COMMIT_DEV_RE.captures(s) {
            let sha = caps["sha"].to_string();
            return Ok(Self {
                raw: format!("dev-{sha}"),
                kind: DeployTagKind::Commit,
            });
        }
        if BARE_SHA_RE.is_match(s) {
            // Normalize bare sha → docker-publish short tag `dev-<sha>`.
            // Prefer full string if already short (7–12); keep full 40-char as-is after prefix.
            let short = if s.len() > 12 { &s[..7] } else { s };
            return Ok(Self {
                raw: format!("dev-{short}"),
                kind: DeployTagKind::Commit,
            });
        }
        if BRANCH_TIPS.contains(&s) {
            return Ok(Self {
                raw: s.to_string(),
                kind: DeployTagKind::Branch,
            });
        }
        Err(UpdaterError::InvalidInput(format!(
            "invalid deploy tag: {s} (expected vX.Y.Z, dev-<sha>, bare sha, or main|preview|beta)"
        )))
    }

    pub fn from_release(v: MyriadVersion) -> Self {
        Self {
            raw: v.raw,
            kind: DeployTagKind::Release,
        }
    }

    pub fn as_str(&self) -> &str {
        &self.raw
    }

    pub fn kind(&self) -> DeployTagKind {
        self.kind
    }

    pub fn is_release(&self) -> bool {
        matches!(self.kind, DeployTagKind::Release)
    }

    pub fn as_release(&self) -> Option<MyriadVersion> {
        if self.is_release() {
            MyriadVersion::parse(&self.raw).ok()
        } else {
            None
        }
    }

    /// Extract sha fragment for commit tags (`dev-abc1234` → `abc1234`).
    pub fn commit_sha(&self) -> Option<&str> {
        match self.kind {
            DeployTagKind::Commit => self.raw.strip_prefix("dev-"),
            _ => None,
        }
    }

    /// Whether a runtime-reported version string (backend /health, frontend meta) matches
    /// this deploy target. Commit builds embed `dev-<fullsha>` which must match on prefix.
    pub fn matches_runtime_version(&self, reported: &str) -> bool {
        let reported = reported.trim();
        if reported == self.raw {
            return true;
        }
        match self.kind {
            DeployTagKind::Release => false,
            DeployTagKind::Commit => {
                let Some(sha) = self.commit_sha() else {
                    return false;
                };
                // reported often looks like `dev-<full40>`; tag may be short.
                reported == format!("dev-{sha}")
                    || reported
                        .strip_prefix("dev-")
                        .is_some_and(|r| r.starts_with(sha) || sha.starts_with(r))
            }
            DeployTagKind::Branch => {
                // We no longer persist branch tips as MYRIAD_TAG after success (normalized to
                // dev-<sha>). Matching a bare branch name is exact only.
                reported == self.raw
            }
        }
    }
}

impl fmt::Display for DeployTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.raw)
    }
}

impl FromStr for DeployTag {
    type Err = UpdaterError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl Serialize for DeployTag {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(&self.raw)
    }
}

impl<'de> Deserialize<'de> for DeployTag {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        let s = String::deserialize(de)?;
        Self::parse(&s).map_err(serde::de::Error::custom)
    }
}

impl From<MyriadVersion> for DeployTag {
    fn from(v: MyriadVersion) -> Self {
        Self::from_release(v)
    }
}

/// Map a channel/branch name to the git branch used for commit-mode tips.
pub fn commit_branch_for_channel(channel: &str) -> &'static str {
    match channel {
        "preview" => "preview",
        // stable / main / anything else → main
        _ => "main",
    }
}

/// Map UI/effective channel to a **release** channel for self-update lookups.
pub fn release_channel_name_for_self_update(channel: &str) -> &'static str {
    match channel {
        "preview" => "preview",
        _ => "stable",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_plain() {
        let v = MyriadVersion::parse("v1.2.3").unwrap();
        assert_eq!(v.as_str(), "v1.2.3");
    }

    #[test]
    fn accepts_pre_release() {
        let v = MyriadVersion::parse("v1.2.3-beta.4").unwrap();
        assert_eq!(v.semver().pre.as_str(), "beta.4");
    }

    #[test]
    fn rejects_garbage() {
        for bad in ["", "1.2.3", "vX.Y.Z", "v1.2", "v1.2.3-BETA"] {
            assert!(MyriadVersion::parse(bad).is_err(), "should reject {bad}");
        }
    }

    #[test]
    fn ordering() {
        let a = MyriadVersion::parse("v1.2.3").unwrap();
        let b = MyriadVersion::parse("v1.2.4").unwrap();
        assert!(a.older_than(&b));
        assert!(!b.older_than(&a));
    }

    #[test]
    fn deploy_tag_release_and_commit() {
        let r = DeployTag::parse("v1.2.3").unwrap();
        assert!(r.is_release());
        let c = DeployTag::parse("abc1234").unwrap();
        assert_eq!(c.as_str(), "dev-abc1234");
        assert_eq!(c.kind(), DeployTagKind::Commit);
        let d = DeployTag::parse("dev-deadbee").unwrap();
        assert_eq!(d.as_str(), "dev-deadbee");
        let b = DeployTag::parse("preview").unwrap();
        assert_eq!(b.kind(), DeployTagKind::Branch);
    }

    #[test]
    fn runtime_version_match_commit() {
        let t = DeployTag::parse("dev-abc1234").unwrap();
        assert!(t.matches_runtime_version("dev-abc1234"));
        assert!(t.matches_runtime_version("dev-abc1234ffffffffffffffffffffffffffffffff"));
        assert!(!t.matches_runtime_version("v1.2.3"));
    }

    #[test]
    fn update_mode_parse() {
        assert_eq!(
            "release".parse::<UpdateMode>().unwrap(),
            UpdateMode::Release
        );
        assert_eq!("commit".parse::<UpdateMode>().unwrap(), UpdateMode::Commit);
    }

    #[test]
    fn channel_mapping() {
        assert_eq!(commit_branch_for_channel("stable"), "main");
        assert_eq!(commit_branch_for_channel("main"), "main");
        assert_eq!(commit_branch_for_channel("preview"), "preview");
        assert_eq!(release_channel_name_for_self_update("main"), "stable");
        assert_eq!(release_channel_name_for_self_update("stable"), "stable");
        assert_eq!(release_channel_name_for_self_update("preview"), "preview");
    }
}

/// Docker tag syntax only. Version ordering and channel are selection preferences.
pub(crate) fn validate_image_tag(tag: &str) -> std::result::Result<(), String> {
    if tag.is_empty()
        || tag.len() > 128
        || !tag
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
        || matches!(tag.as_bytes()[0], b'.' | b'-')
    {
        return Err("invalid image tag".into());
    }
    Ok(())
}

#[cfg(test)]
mod image_tag_tests {
    #[test]
    fn versions_and_channels_do_not_restrict_image_tags() {
        for tag in [
            "v0.1.0",
            "v9.0.0",
            "preview",
            "main",
            "latest",
            "custom-build",
        ] {
            assert!(super::validate_image_tag(tag).is_ok());
        }
        for tag in ["", "../image", "foreign/repo:tag", "-flag", "a\nb"] {
            assert!(super::validate_image_tag(tag).is_err());
        }
    }
}
