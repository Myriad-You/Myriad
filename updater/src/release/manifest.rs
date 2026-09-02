//! release.json schema (deserialization).
//!
//! Forward-compatible: unknown fields are ignored. Updaters must accept any
//! release.json with `schema_version <= SUPPORTED_RELEASE_SCHEMA` and reject higher.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{Result, UpdaterError};
use crate::version::MyriadVersion;
use crate::SUPPORTED_RELEASE_SCHEMA;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub schema_version: u32,
    pub version: MyriadVersion,
    /// Exact source commit used to build every image in this release.
    #[serde(default)]
    pub commit_sha: Option<String>,
    pub channel: String,
    pub released_at: DateTime<Utc>,
    #[serde(default)]
    pub min_from_version: Option<MyriadVersion>,
    pub images: HashMap<String, ImageRef>,
    pub env: EnvSpec,
    pub migrations: Migrations,
    pub updater: UpdaterReq,
    pub postgres: PostgresReq,
    pub notes_url: String,
    #[serde(default)]
    pub signature: Option<String>,
    /// Catch-all for forward-compat fields.
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageRef {
    pub r#ref: String,
    pub digest: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EnvSpec {
    #[serde(default)]
    pub required: Vec<String>,
    #[serde(default)]
    pub new: Vec<NewEnv>,
    #[serde(default)]
    pub removed: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewEnv {
    pub name: String,
    pub required: bool,
    #[serde(default)]
    pub default: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Migrations {
    pub irreversible: bool,
    pub estimated_seconds: u32,
    #[serde(default = "default_true")]
    pub requires_full_backup: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdaterReq {
    pub min_updater_version: MyriadVersion,
    #[serde(default)]
    pub self_update_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostgresReq {
    pub min_pg_version: String,
    /// Deprecated compatibility field. Missing or `unbounded` means there is
    /// no PostgreSQL upper bound.
    #[serde(default)]
    pub max_pg_version: Option<String>,
}

fn default_true() -> bool {
    true
}

impl Manifest {
    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        let m: Self = serde_json::from_slice(bytes)?;
        m.validate()?;
        Ok(m)
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema_version > SUPPORTED_RELEASE_SCHEMA {
            return Err(UpdaterError::Precondition(format!(
                "release.json schema_version {} > supported {}: please update the updater first",
                self.schema_version, SUPPORTED_RELEASE_SCHEMA
            )));
        }
        for required in ["backend", "frontend"] {
            if !self.images.contains_key(required) {
                return Err(UpdaterError::Precondition(format!(
                    "release.json missing required image: {required}"
                )));
            }
        }
        for (name, img) in &self.images {
            if !img.digest.starts_with("sha256:") || img.digest.len() != 71 {
                return Err(UpdaterError::Precondition(format!(
                    "image {name} has invalid digest: {}",
                    img.digest
                )));
            }
        }
        if let Some(sha) = &self.commit_sha {
            if sha.len() != 40 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(UpdaterError::Precondition(format!(
                    "release.json has invalid commit_sha: {sha}"
                )));
            }
        }
        Ok(())
    }

    pub fn image(&self, comp: &str) -> Option<&ImageRef> {
        self.images.get(comp)
    }
}

#[cfg(test)]
mod tests {
    use super::Manifest;
    use serde_json::json;

    fn manifest_json(postgres: serde_json::Value) -> Vec<u8> {
        let digest = format!("sha256:{}", "0".repeat(64));
        serde_json::to_vec(&json!({
            "schema_version": 1,
            "version": "v0.4.0",
            "channel": "stable",
            "released_at": "2026-07-15T00:00:00Z",
            "images": {
                "backend": { "ref": "example/backend:v0.4.0", "digest": digest },
                "frontend": { "ref": "example/frontend:v0.4.0", "digest": format!("sha256:{}", "1".repeat(64)) }
            },
            "env": { "required": [], "new": [], "removed": [] },
            "migrations": { "irreversible": false, "estimated_seconds": 30 },
            "updater": { "min_updater_version": "v0.4.0" },
            "postgres": postgres,
            "notes_url": "https://example.com/releases/v0.4.0"
        }))
        .expect("manifest fixture serializes")
    }

    #[test]
    fn postgres_upper_bound_is_optional() {
        let manifest = Manifest::from_json(&manifest_json(json!({
            "min_pg_version": "16"
        })))
        .expect("manifest without upper bound parses");
        assert!(manifest.postgres.max_pg_version.is_none());
    }

    #[test]
    fn unbounded_marker_keeps_old_release_payloads_compatible() {
        let manifest = Manifest::from_json(&manifest_json(json!({
            "min_pg_version": "16",
            "max_pg_version": "unbounded"
        })))
        .expect("compatibility marker parses");
        assert_eq!(
            manifest.postgres.max_pg_version.as_deref(),
            Some("unbounded")
        );
    }
}
