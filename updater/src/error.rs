//! Error types. We expose a few semantic categories at the boundary; internal code uses anyhow.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum UpdaterError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("json: {0}")]
    Json(#[from] serde_json::Error),

    #[error("docker: {0}")]
    Docker(String),

    #[error("github: {0}")]
    Github(String),

    #[error("docker hub: {0}")]
    DockerHub(String),

    #[error("config: {0}")]
    Config(String),

    #[error("state: {0}")]
    State(String),

    #[error("env-probe: {0}")]
    Probe(String),

    #[error("precondition failed: {0}")]
    Precondition(String),

    #[error("tag installation requires allow_tag_install=true and confirm_risk=true: {0}")]
    TagInstallRequired(String),

    #[error("unauthorized")]
    Unauthorized,

    #[error("conflict: another job is in progress")]
    Conflict,

    #[error("not found: {0}")]
    NotFound(String),

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("internal: {0}")]
    Internal(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, UpdaterError>;
