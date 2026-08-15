//! One-time federation WebSocket tickets (registry namespace `federation_ws_ticket`).
//!
//! Browser WebSockets cannot send `X-Tapp-Runtime-Grant`. Runtimes mint a
//! short-lived, single-use ticket over REST (with the grant header), then present
//! it as `?tapp_ws_ticket=` on the channel/room upgrade. Host UI traffic omits
//! the ticket and continues to authenticate with Claims only.
//!
//! Domain lives in services so `federation::ws_gateway` does not import
//! `api::tapp_runtime` for consume. The API layer maps [`WsTicketError`] to Axum.

use chrono::Utc;
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::Duration;
use uuid::Uuid;

use crate::services::permission_service::TappPermission;
use crate::services::tapp_registry::{self as shared_registry, RegistryIdentity};

/// Query parameter name accepted by federation channel/room WS upgrades.
/// Keep in sync with `FederationWsQuery` and `federationApi.connect*Ws`.
pub const TAPP_WS_TICKET_QUERY: &str = "tapp_ws_ticket";

const WS_TICKET_NAMESPACE: &str = "federation_ws_ticket";
const WS_TICKET_TTL: Duration = Duration::from_secs(45);
const MAX_ACTIVE_TICKETS_PER_SUBJECT: usize = 32;
const TICKET_PREFIX: &str = "twt_";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum WsTicketKind {
    Channel,
    Room,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct StoredWsTicket {
    runtime_id: String,
    tapp_id: String,
    owner_id: i32,
    subject_id: i32,
    permissions: Vec<String>,
    kind: WsTicketKind,
    resource_id: String,
    expires_at: i64,
}

/// Context returned after a successful single-use ticket consume.
#[derive(Debug, Clone)]
pub struct ConsumedWsTicket {
    pub runtime_id: String,
    pub tapp_id: String,
    pub subject_id: i32,
    pub owner_id: i32,
    pub kind: WsTicketKind,
    pub resource_id: String,
}

/// Runtime identity snapshot used when minting a ticket (from a validated grant).
#[derive(Debug, Clone)]
pub struct WsTicketMintIdentity {
    pub runtime_id: String,
    pub tapp_id: String,
    pub owner_id: i32,
    pub subject_id: i32,
}

/// Successfully minted ticket payload (HTTP maps to JSON).
#[derive(Debug, Clone)]
pub struct MintedWsTicket {
    pub ticket: String,
    pub expires_at: chrono::DateTime<Utc>,
}

/// Domain errors for WS ticket mint/consume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WsTicketError {
    Unavailable,
    Invalid,
    InvalidSubject,
    SubjectMismatch,
    ScopeMismatch,
    PermissionDenied,
    InvalidScope,
    LimitExceeded,
}

impl WsTicketError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unavailable => "WS_TICKET_REGISTRY_UNAVAILABLE",
            Self::Invalid => "INVALID_WS_TICKET",
            Self::InvalidSubject => "INVALID_WS_TICKET_SUBJECT",
            Self::SubjectMismatch => "WS_TICKET_SUBJECT_MISMATCH",
            Self::ScopeMismatch => "WS_TICKET_SCOPE_MISMATCH",
            Self::PermissionDenied => "WS_TICKET_PERMISSION_DENIED",
            Self::InvalidScope => "INVALID_WS_TICKET_SCOPE",
            Self::LimitExceeded => "WS_TICKET_LIMIT_EXCEEDED",
        }
    }

    pub fn message(&self) -> &'static str {
        match self {
            Self::Unavailable => "WebSocket ticket registry is unavailable",
            Self::Invalid => {
                "WebSocket ticket is missing, invalid, expired, or already used"
            }
            Self::InvalidSubject => "Authenticated subject is invalid",
            Self::SubjectMismatch => {
                "WebSocket ticket subject does not match the authenticated user"
            }
            Self::ScopeMismatch => "WebSocket ticket is not valid for this channel or room",
            Self::PermissionDenied => "WebSocket ticket is missing federation:message",
            Self::InvalidScope => "Invalid channel or room id for WebSocket ticket",
            Self::LimitExceeded => "Too many active federation WebSocket tickets",
        }
    }

    pub fn status_hint(&self) -> u16 {
        match self {
            Self::Unavailable => 503,
            Self::Invalid | Self::InvalidSubject => 401,
            Self::SubjectMismatch | Self::ScopeMismatch | Self::PermissionDenied => 403,
            Self::InvalidScope => 400,
            Self::LimitExceeded => 429,
        }
    }
}

impl std::fmt::Display for WsTicketError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for WsTicketError {}

pub(crate) fn token_hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

pub(crate) fn new_ticket() -> String {
    format!(
        "{TICKET_PREFIX}{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    )
}

pub(crate) fn has_permission(permissions: &[String], required: TappPermission) -> bool {
    permissions.iter().any(|p| p == required.as_str())
}

/// Mint a one-time WS ticket for a validated runtime (caller must enforce
/// `federation:message` on the grant before calling).
pub async fn mint_ws_ticket(
    db: &DatabaseConnection,
    identity: &WsTicketMintIdentity,
    kind: WsTicketKind,
    resource_id: &str,
) -> Result<MintedWsTicket, WsTicketError> {
    if resource_id.is_empty() || resource_id.len() > 256 {
        return Err(WsTicketError::InvalidScope);
    }

    let ticket = new_ticket();
    let expires_at = Utc::now() + chrono::Duration::from_std(WS_TICKET_TTL).expect("valid TTL");
    let stored = StoredWsTicket {
        runtime_id: identity.runtime_id.clone(),
        tapp_id: identity.tapp_id.clone(),
        owner_id: identity.owner_id,
        subject_id: identity.subject_id,
        permissions: vec![TappPermission::FederationMessage.as_str().to_string()],
        kind,
        resource_id: resource_id.to_string(),
        expires_at: expires_at.timestamp(),
    };

    // Snapshot only the permission we required at mint. validate_runtime_grant
    // already rebinds the grant to current role/installation before this runs.
    let inserted = shared_registry::put_with_subject_limit(
        db,
        WS_TICKET_NAMESPACE,
        &token_hash(&ticket),
        RegistryIdentity {
            subject_id: Some(identity.subject_id),
            owner_id: Some(identity.owner_id),
            tapp_id: Some(identity.tapp_id.as_str()),
            runtime_id: Some(identity.runtime_id.as_str()),
        },
        &stored,
        expires_at.timestamp(),
        MAX_ACTIVE_TICKETS_PER_SUBJECT,
    )
    .await
    .map_err(|error| {
        tracing::error!(%error, "[TAPP] Failed to store federation WS ticket");
        WsTicketError::Unavailable
    })?;

    if !inserted {
        return Err(WsTicketError::LimitExceeded);
    }

    tracing::info!(
        runtime_id = %identity.runtime_id,
        tapp_id = %identity.tapp_id,
        subject_id = identity.subject_id,
        ?kind,
        resource_id = %resource_id,
        "[TAPP] Federation WS ticket minted"
    );

    Ok(MintedWsTicket {
        ticket,
        expires_at,
    })
}

/// Consume a one-time ticket for a federation WebSocket upgrade.
///
/// Fail closed: invalid, expired, reused, wrong-scope, or permission-missing
/// tickets return an error. Callers must not fall open to host identity.
pub async fn consume_ws_ticket(
    db: &DatabaseConnection,
    ticket: &str,
    subject_id: i32,
    expected_kind: WsTicketKind,
    expected_resource_id: &str,
) -> Result<ConsumedWsTicket, WsTicketError> {
    if ticket.is_empty() || !ticket.starts_with(TICKET_PREFIX) {
        return Err(WsTicketError::Invalid);
    }

    // Atomic take: single-use across replicas.
    let stored =
        shared_registry::take::<StoredWsTicket>(db, WS_TICKET_NAMESPACE, &token_hash(ticket))
            .await
            .map_err(|error| {
                tracing::error!(%error, "[TAPP] Federation WS ticket consume failed");
                WsTicketError::Unavailable
            })?
            .ok_or(WsTicketError::Invalid)?;

    if stored.subject_id != subject_id {
        return Err(WsTicketError::SubjectMismatch);
    }

    if stored.kind != expected_kind || stored.resource_id != expected_resource_id {
        return Err(WsTicketError::ScopeMismatch);
    }

    if !has_permission(&stored.permissions, TappPermission::FederationMessage) {
        return Err(WsTicketError::PermissionDenied);
    }

    if stored.expires_at <= Utc::now().timestamp() {
        return Err(WsTicketError::Invalid);
    }

    tracing::info!(
        runtime_id = %stored.runtime_id,
        tapp_id = %stored.tapp_id,
        subject_id = stored.subject_id,
        ?expected_kind,
        resource_id = %expected_resource_id,
        "[TAPP] Federation WS ticket consumed; connection attributed to Tapp runtime"
    );

    Ok(ConsumedWsTicket {
        runtime_id: stored.runtime_id,
        tapp_id: stored.tapp_id,
        subject_id: stored.subject_id,
        owner_id: stored.owner_id,
        kind: stored.kind,
        resource_id: stored.resource_id,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        has_permission, new_ticket, token_hash, StoredWsTicket, WsTicketError, WsTicketKind,
        TAPP_WS_TICKET_QUERY, TICKET_PREFIX,
    };
    use crate::services::permission_service::TappPermission;

    #[test]
    fn tickets_are_prefixed_and_hashed_stably() {
        let ticket = new_ticket();
        assert!(ticket.starts_with(TICKET_PREFIX));
        let hash = token_hash(&ticket);
        assert_eq!(hash.len(), 64);
        assert_eq!(hash, token_hash(&ticket));
        assert_ne!(hash, token_hash("twt_other"));
        // Query param name used by federationApi / ws_gateway must stay stable.
        assert_eq!(TAPP_WS_TICKET_QUERY, "tapp_ws_ticket");
    }

    #[test]
    fn federation_message_permission_is_required() {
        assert!(has_permission(
            &[TappPermission::FederationMessage.as_str().to_string()],
            TappPermission::FederationMessage
        ));
        assert!(!has_permission(
            &[TappPermission::FederationRead.as_str().to_string()],
            TappPermission::FederationMessage
        ));
    }

    #[test]
    fn stored_ticket_roundtrips_scope_fields() {
        let stored = StoredWsTicket {
            runtime_id: "rt_1".into(),
            tapp_id: "tapp_1".into(),
            owner_id: 1,
            subject_id: 2,
            permissions: vec!["federation:message".into()],
            kind: WsTicketKind::Channel,
            resource_id: "ch_abc".into(),
            expires_at: 1_700_000_000,
        };
        let value = serde_json::to_value(&stored).unwrap();
        let back: StoredWsTicket = serde_json::from_value(value).unwrap();
        assert_eq!(back.kind, WsTicketKind::Channel);
        assert_eq!(back.resource_id, "ch_abc");
        assert!(has_permission(
            &back.permissions,
            TappPermission::FederationMessage
        ));
    }

    #[test]
    fn error_codes_preserve_api_contract() {
        assert_eq!(
            WsTicketError::Unavailable.code(),
            "WS_TICKET_REGISTRY_UNAVAILABLE"
        );
        assert_eq!(WsTicketError::Invalid.code(), "INVALID_WS_TICKET");
        assert_eq!(
            WsTicketError::InvalidSubject.code(),
            "INVALID_WS_TICKET_SUBJECT"
        );
        assert_eq!(
            WsTicketError::SubjectMismatch.code(),
            "WS_TICKET_SUBJECT_MISMATCH"
        );
        assert_eq!(
            WsTicketError::ScopeMismatch.code(),
            "WS_TICKET_SCOPE_MISMATCH"
        );
        assert_eq!(
            WsTicketError::PermissionDenied.code(),
            "WS_TICKET_PERMISSION_DENIED"
        );
        assert_eq!(
            WsTicketError::InvalidScope.code(),
            "INVALID_WS_TICKET_SCOPE"
        );
        assert_eq!(
            WsTicketError::LimitExceeded.code(),
            "WS_TICKET_LIMIT_EXCEEDED"
        );
    }

    #[test]
    fn status_hints_match_http_contract() {
        assert_eq!(WsTicketError::Unavailable.status_hint(), 503);
        assert_eq!(WsTicketError::Invalid.status_hint(), 401);
        assert_eq!(WsTicketError::LimitExceeded.status_hint(), 429);
        assert_eq!(WsTicketError::InvalidScope.status_hint(), 400);
        assert_eq!(WsTicketError::PermissionDenied.status_hint(), 403);
    }
}
