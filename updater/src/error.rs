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

    /// The on-disk compose differs from the updater's baseline (or there is none)
    /// and the operator has not acknowledged the overwrite.
    #[error(
        "precondition failed: the deployment compose will be replaced with the target version's template; \
         custom environment variables (including proxies) and other compose edits are not merged, \
         and additional compose files will be emptied; original files are backed up to state/compose-backup/; \
         re-submit with allow_compose_override=true (or allow_risk=true)"
    )]
    ComposeOverrideRequired,

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

impl UpdaterError {
    /// Stable machine-readable code for failures the UI can act on.
    pub fn code(&self) -> Option<&'static str> {
        match self {
            Self::ComposeOverrideRequired => Some("compose_override_required"),
            _ => None,
        }
    }
}

pub type Result<T> = std::result::Result<T, UpdaterError>;
