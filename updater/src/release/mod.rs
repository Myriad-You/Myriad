//! release.json fetching, parsing, and validation.

pub mod cosign;
pub mod dev_signature;
pub mod dockerhub;
pub mod github;
pub mod manifest;

pub use cosign::{CosignPolicy, VerifyOutcome};
pub use dockerhub::{
    commit_upgrade_direction, commit_upgrade_direction_ex, is_cross_kind_deploy, pushed_at_for_tag,
    same_commit_identity, same_deploy_identity, select_component_tip, select_dev_channel_tip,
    select_dev_channel_tip_for, CommitUpgradeDirection, ComponentTag, DockerBuild, DockerHubClient,
};
pub use github::{
    deploy_tag_to_git_ref, CommitInfo, CommitRelation, Freshness, GithubClient, Release,
};
pub use manifest::{ImageRef, Manifest};
