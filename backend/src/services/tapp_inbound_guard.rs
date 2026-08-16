//! Inbound `/tapi` pause and IP-fingerprint blocks.
//!
//! Raw client addresses are never stored. Auto-blocks trip after repeated
//! verify failures; owners can pause a Tapp's inbound surface or clear a block.

use crate::services::tapp_rate_limit::{
    RateLimitError, anonymous_subject_fingerprint, increment_named_limit,
};
use crate::services::tapp_registry::{self, RegistryIdentity};
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const BLOCK_NAMESPACE: &str = "route_block";
const AUTO_BLOCK_SECS: i64 = 3600;
const MANUAL_BLOCK_SECS: i64 = 30 * 24 * 3600;
const PAUSE_SECS: i64 = 10 * 365 * 24 * 3600;
const FAIL_LIMIT_TAPP: u32 = 25;
const FAIL_LIMIT_SITE: u32 = 80;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InboundGuardError {
    Database,
    NotFound,
}

impl InboundGuardError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Database => "DATABASE_ERROR",
            Self::NotFound => "ROUTE_BLOCK_NOT_FOUND",
        }
    }

    pub fn message(&self) -> &'static str {
        match self {
            Self::Database => "Database error",
            Self::NotFound => "Inbound block was not found",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InboundBlockRecord {
    pub kind: String,
    pub source: String,
    pub fingerprint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_id: Option<i32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InboundBlockView {
    pub fingerprint: String,
    pub source: String,
    pub scope: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InboundGuardStatus {
    pub paused: bool,
    pub blocks: Vec<InboundBlockView>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InboundDenial {
    Paused,
    Blocked { retry_after: u64 },
}

pub fn client_fingerprint(client_ip: Option<&str>) -> String {
    anonymous_subject_fingerprint(client_ip.unwrap_or("unresolved"))
}

fn record_id(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for (index, part) in parts.iter().enumerate() {
        if index > 0 {
            hasher.update([0]);
        }
        hasher.update(part.as_bytes());
    }
    hex::encode(hasher.finalize())
}

fn tapp_block_id(owner_id: i32, tapp_id: &str, fingerprint: &str) -> String {
    record_id(&["tapp", &owner_id.to_string(), tapp_id, "ip", fingerprint])
}

fn site_block_id(fingerprint: &str) -> String {
    record_id(&["site", "ip", fingerprint])
}

fn pause_id(owner_id: i32, tapp_id: &str) -> String {
    record_id(&["pause", &owner_id.to_string(), tapp_id])
}

/// Site-wide auto-block. Checked before resolving the public install so a
/// banned fingerprint cannot probe which Tapps exist.
pub async fn check_site_inbound_block(
    db: &DatabaseConnection,
    client_ip: Option<&str>,
) -> Result<Option<InboundDenial>, InboundGuardError> {
    let fingerprint = client_fingerprint(client_ip);
    if tapp_registry::get::<InboundBlockRecord>(db, BLOCK_NAMESPACE, &site_block_id(&fingerprint))
        .await
        .map_err(|_| InboundGuardError::Database)?
        .is_some()
    {
        return Ok(Some(InboundDenial::Blocked {
            retry_after: AUTO_BLOCK_SECS as u64,
        }));
    }
    Ok(None)
}

/// Pause / per-install block. Scoped to the public-install owner so a private
/// copy of the same `tapp_id` cannot freeze or unban the site inbound surface.
pub async fn check_tapp_inbound_access(
    db: &DatabaseConnection,
    owner_id: i32,
    tapp_id: &str,
    client_ip: Option<&str>,
) -> Result<Option<InboundDenial>, InboundGuardError> {
    let fingerprint = client_fingerprint(client_ip);
    let pause_record = pause_id(owner_id, tapp_id);
    let block_record = tapp_block_id(owner_id, tapp_id, &fingerprint);
    let (paused, blocked) = tokio::try_join!(
        tapp_registry::get::<InboundBlockRecord>(db, BLOCK_NAMESPACE, &pause_record),
        tapp_registry::get::<InboundBlockRecord>(db, BLOCK_NAMESPACE, &block_record),
    )
    .map_err(|_| InboundGuardError::Database)?;
    if paused.is_some() {
        return Ok(Some(InboundDenial::Paused));
    }
    if blocked.is_some() {
        return Ok(Some(InboundDenial::Blocked {
            retry_after: AUTO_BLOCK_SECS as u64,
        }));
    }
    Ok(None)
}

async fn write_block(
    db: &DatabaseConnection,
    record_id: &str,
    owner_id: Option<i32>,
    tapp_id: Option<&str>,
    fingerprint: &str,
    source: &str,
    ttl_secs: i64,
) -> Result<(), InboundGuardError> {
    let now = chrono::Utc::now().timestamp();
    tapp_registry::put(
        db,
        BLOCK_NAMESPACE,
        record_id,
        RegistryIdentity {
            subject_id: None,
            owner_id,
            tapp_id,
            runtime_id: None,
        },
        &InboundBlockRecord {
            kind: "ip".into(),
            source: source.into(),
            fingerprint: fingerprint.into(),
            owner_id,
        },
        now.saturating_add(ttl_secs),
    )
    .await
    .map_err(|_| InboundGuardError::Database)
}

fn should_block(result: Result<u32, RateLimitError>, limit: u32) -> bool {
    match result {
        Ok(count) => count >= limit,
        Err(RateLimitError::Exceeded { .. }) => true,
        Err(_) => false,
    }
}

pub async fn record_verify_failure(
    db: &DatabaseConnection,
    owner_id: i32,
    tapp_id: &str,
    client_ip: Option<&str>,
) -> Result<(), InboundGuardError> {
    let fingerprint = client_fingerprint(client_ip);
    let (tapp_count, site_count) = tokio::join!(
        increment_named_limit(
            db,
            format!("route.fail:{owner_id}:{tapp_id}:{fingerprint}"),
            tapp_id,
            "route.fail",
        ),
        increment_named_limit(
            db,
            format!("route.fail.site:{fingerprint}"),
            "_",
            "route.fail.site",
        ),
    );
    if should_block(tapp_count, FAIL_LIMIT_TAPP) {
        write_block(
            db,
            &tapp_block_id(owner_id, tapp_id, &fingerprint),
            Some(owner_id),
            Some(tapp_id),
            &fingerprint,
            "auto",
            AUTO_BLOCK_SECS,
        )
        .await?;
    }
    if should_block(site_count, FAIL_LIMIT_SITE) {
        write_block(
            db,
            &site_block_id(&fingerprint),
            None,
            None,
            &fingerprint,
            "auto",
            AUTO_BLOCK_SECS,
        )
        .await?;
    }
    Ok(())
}

pub async fn pause_inbound(
    db: &DatabaseConnection,
    owner_id: i32,
    tapp_id: &str,
) -> Result<(), InboundGuardError> {
    let now = chrono::Utc::now().timestamp();
    tapp_registry::put(
        db,
        BLOCK_NAMESPACE,
        &pause_id(owner_id, tapp_id),
        RegistryIdentity {
            subject_id: None,
            owner_id: Some(owner_id),
            tapp_id: Some(tapp_id),
            runtime_id: None,
        },
        &InboundBlockRecord {
            kind: "pause".into(),
            source: "manual".into(),
            fingerprint: String::new(),
            owner_id: Some(owner_id),
        },
        now.saturating_add(PAUSE_SECS),
    )
    .await
    .map_err(|_| InboundGuardError::Database)
}

pub async fn resume_inbound(
    db: &DatabaseConnection,
    owner_id: i32,
    tapp_id: &str,
) -> Result<(), InboundGuardError> {
    tapp_registry::delete(db, BLOCK_NAMESPACE, &pause_id(owner_id, tapp_id))
        .await
        .map(|_| ())
        .map_err(|_| InboundGuardError::Database)
}

pub async fn unblock_fingerprint(
    db: &DatabaseConnection,
    owner_id: i32,
    tapp_id: &str,
    fingerprint: &str,
) -> Result<(), InboundGuardError> {
    if fingerprint.is_empty() || fingerprint.len() > 128 {
        return Err(InboundGuardError::NotFound);
    }
    // Owners may only lift their own install's block. Site-wide auto-blocks
    // expire on their own so one Tapp manager cannot unban an attacker for
    // every inbound route on the host.
    let removed = tapp_registry::delete(
        db,
        BLOCK_NAMESPACE,
        &tapp_block_id(owner_id, tapp_id, fingerprint),
    )
    .await
    .map_err(|_| InboundGuardError::Database)?;
    if removed {
        Ok(())
    } else {
        Err(InboundGuardError::NotFound)
    }
}

pub async fn extend_block(
    db: &DatabaseConnection,
    owner_id: i32,
    tapp_id: &str,
    fingerprint: &str,
) -> Result<(), InboundGuardError> {
    if fingerprint.is_empty() || fingerprint.len() > 128 {
        return Err(InboundGuardError::NotFound);
    }
    write_block(
        db,
        &tapp_block_id(owner_id, tapp_id, fingerprint),
        Some(owner_id),
        Some(tapp_id),
        fingerprint,
        "manual",
        MANUAL_BLOCK_SECS,
    )
    .await
}

pub async fn guard_status(
    db: &DatabaseConnection,
    owner_id: i32,
    tapp_id: &str,
) -> Result<InboundGuardStatus, InboundGuardError> {
    let paused =
        tapp_registry::get::<InboundBlockRecord>(db, BLOCK_NAMESPACE, &pause_id(owner_id, tapp_id))
            .await
            .map_err(|_| InboundGuardError::Database)?
            .is_some();
    let rows = tapp_registry::list(db, BLOCK_NAMESPACE, None, Some(tapp_id))
        .await
        .map_err(|_| InboundGuardError::Database)?;
    let mut blocks = Vec::new();
    for row in rows {
        let Ok(record) = serde_json::from_value::<InboundBlockRecord>(row.payload) else {
            continue;
        };
        if record.kind != "ip" || record.fingerprint.is_empty() || record.owner_id != Some(owner_id)
        {
            continue;
        }
        blocks.push(InboundBlockView {
            fingerprint: record.fingerprint,
            source: record.source,
            scope: "tapp".into(),
        });
    }
    blocks.sort_by(|left, right| left.fingerprint.cmp(&right.fingerprint));
    Ok(InboundGuardStatus { paused, blocks })
}

#[cfg(test)]
mod tests {
    use super::{pause_id, site_block_id, tapp_block_id};

    #[test]
    fn block_ids_are_stable_owner_scoped_and_distinct() {
        let first = tapp_block_id(1, "com.example.app", "abc");
        assert_eq!(first, tapp_block_id(1, "com.example.app", "abc"));
        assert_ne!(first, tapp_block_id(2, "com.example.app", "abc"));
        assert_ne!(first, tapp_block_id(1, "com.example.other", "abc"));
        assert_ne!(first, site_block_id("abc"));
        assert_ne!(pause_id(1, "com.example.app"), first);
        assert_ne!(
            pause_id(1, "com.example.app"),
            pause_id(2, "com.example.app")
        );
        assert_eq!(first.len(), 64);
    }
}
