//! release.json fetching, parsing, and validation.

pub mod cosign;
pub mod dockerhub;
pub mod github;
pub mod manifest;

pub use cosign::{CosignPolicy, VerifyOutcome};
pub use dockerhub::{
    commit_upgrade_direction, is_cross_kind_deploy, pushed_at_for_tag, select_dev_channel_tip,
    CommitUpgradeDirection, DockerBuild, DockerHubClient,
};
pub use github::{
    deploy_tag_to_git_ref, CommitInfo, CommitRelation, Freshness, GithubClient, Release,
};
pub use manifest::{ImageRef, Manifest};
