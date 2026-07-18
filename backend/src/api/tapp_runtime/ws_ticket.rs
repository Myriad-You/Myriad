//! One-time WebSocket tickets for Tapp-attributed federation subscriptions.
//!
//! Browser WebSockets cannot send `X-Tapp-Runtime-Grant`. Tapp runtimes mint a
//! short-lived, single-use ticket over REST (with the grant header), then present
//! it as `?tapp_ws_ticket=` on the channel/room upgrade. Host UI traffic omits
//! the ticket and continues to authenticate with Claims only.

use axum::{
    extract::Path,
    http::StatusCode,
    Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::time::Duration;
use uuid::Uuid;

use crate::middleware::auth::Claims;
use crate::services::permission_service::TappPermission;

use super::{
    shared_registry::{self, RegistryIdentity},
    RuntimeGrantContext,
};

/// Query parameter name accepted by federation channel/room WS upgrades.
/// Keep in sync with `FederationWsQuery` and `federationApi.connect*Ws`.
pub const TAPP_WS_TICKET_QUERY: &str = "tapp_ws_ticket";

// Force the public name into the non-test binary so renames stay intentional.
const _: &str = TAPP_WS_TICKET_QUERY;

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



#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WsTicketResponse {
    pub ticket: String,
    pub expires_at: String,
}

fn token_hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

fn new_ticket() -> String {
    format!(
        "{TICKET_PREFIX}{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    )
}

fn api_error(status: StatusCode, code: &str, message: &str) -> (StatusCode, Json<Value>) {
    (
        status,
        Json(json!({
            "error": message,
            "code": code
        })),
    )
}

fn registry_unavailable() -> (StatusCode, Json<Value>) {
    api_error(
        StatusCode::SERVICE_UNAVAILABLE,
        "WS_TICKET_REGISTRY_UNAVAILABLE",
        "WebSocket ticket registry is unavailable",
    )
}

fn has_permission(permissions: &[String], required: TappPermission) -> bool {
    permissions.iter().any(|p| p == required.as_str())
}

async fn mint_ticket(
    grant: &RuntimeGrantContext,
    kind: WsTicketKind,
    resource_id: String,
) -> Result<Json<WsTicketResponse>, (StatusCode, Json<Value>)> {
    grant.require(TappPermission::FederationMessage)?;

    if resource_id.is_empty() || resource_id.len() > 256 {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "INVALID_WS_TICKET_SCOPE",
            "Invalid channel or room id for WebSocket ticket",
        ));
    }

    let db = shared_registry::database()
        .await
        .map_err(|_| registry_unavailable())?;

    let ticket = new_ticket();
    let expires_at = Utc::now() + chrono::Duration::from_std(WS_TICKET_TTL).expect("valid TTL");
    let stored = StoredWsTicket {
        runtime_id: grant.runtime_id().to_string(),
        tapp_id: grant.tapp_id().to_string(),
        owner_id: grant.owner_id(),
        subject_id: grant.subject_id(),
        permissions: vec![TappPermission::FederationMessage.as_str().to_string()],
        kind,
        resource_id: resource_id.clone(),
        expires_at: expires_at.timestamp(),
    };

    // Snapshot only the permission we required at mint. validate_runtime_grant
    // already rebinds the grant to current role/installation before this runs.
    let inserted = shared_registry::put_with_subject_limit(
        &db,
        WS_TICKET_NAMESPACE,
        &token_hash(&ticket),
        RegistryIdentity {
            subject_id: Some(grant.subject_id()),
            owner_id: Some(grant.owner_id()),
            tapp_id: Some(grant.tapp_id()),
            runtime_id: Some(grant.runtime_id()),
        },
        &stored,
        expires_at.timestamp(),
        MAX_ACTIVE_TICKETS_PER_SUBJECT,
    )
    .await
    .map_err(|error| {
        tracing::error!(%error, "[TAPP] Failed to store federation WS ticket");
        registry_unavailable()
    })?;

    if !inserted {
        return Err(api_error(
            StatusCode::TOO_MANY_REQUESTS,
            "WS_TICKET_LIMIT_EXCEEDED",
            "Too many active federation WebSocket tickets",
        ));
    }

    tracing::info!(
        runtime_id = %grant.runtime_id(),
        tapp_id = %grant.tapp_id(),
        subject_id = grant.subject_id(),
        ?kind,
        resource_id = %resource_id,
        "[TAPP] Federation WS ticket minted"
    );

    Ok(Json(WsTicketResponse {
        ticket,
        expires_at: expires_at.to_rfc3339(),
    }))
}

/// POST /api/federation/channels/{channel_id}/ws-ticket
pub async fn mint_channel_ws_ticket(
    grant: RuntimeGrantContext,
    Path(channel_id): Path<String>,
) -> Result<Json<WsTicketResponse>, (StatusCode, Json<Value>)> {
    mint_ticket(&grant, WsTicketKind::Channel, channel_id).await
}

/// POST /api/federation/rooms/{room_id}/ws-ticket
pub async fn mint_room_ws_ticket(
    grant: RuntimeGrantContext,
    Path(room_id): Path<String>,
) -> Result<Json<WsTicketResponse>, (StatusCode, Json<Value>)> {
    mint_ticket(&grant, WsTicketKind::Room, room_id).await
}

/// Consume a one-time ticket for a federation WebSocket upgrade.
///
/// Fail closed: invalid, expired, reused, wrong-scope, or permission-missing
/// tickets return an error. Callers must not fall open to host identity.
pub async fn consume_ws_ticket(
    ticket: &str,
    claims: &Claims,
    expected_kind: WsTicketKind,
    expected_resource_id: &str,
) -> Result<ConsumedWsTicket, (StatusCode, Json<Value>)> {
    if ticket.is_empty() || !ticket.starts_with(TICKET_PREFIX) {
        return Err(api_error(
            StatusCode::UNAUTHORIZED,
            "INVALID_WS_TICKET",
            "WebSocket ticket is missing, invalid, expired, or already used",
        ));
    }

    let subject_id: i32 = claims.sub.parse().map_err(|_| {
        api_error(
            StatusCode::UNAUTHORIZED,
            "INVALID_WS_TICKET_SUBJECT",
            "Authenticated subject is invalid",
        )
    })?;

    let db = shared_registry::database()
        .await
        .map_err(|_| registry_unavailable())?;

    // Atomic take: single-use across replicas.
    let stored = shared_registry::take::<StoredWsTicket>(&db, WS_TICKET_NAMESPACE, &token_hash(ticket))
        .await
        .map_err(|error| {
            tracing::error!(%error, "[TAPP] Federation WS ticket consume failed");
            registry_unavailable()
        })?
        .ok_or_else(|| {
            api_error(
                StatusCode::UNAUTHORIZED,
                "INVALID_WS_TICKET",
                "WebSocket ticket is missing, invalid, expired, or already used",
            )
        })?;

    if stored.subject_id != subject_id {
        return Err(api_error(
            StatusCode::FORBIDDEN,
            "WS_TICKET_SUBJECT_MISMATCH",
            "WebSocket ticket subject does not match the authenticated user",
        ));
    }

    if stored.kind != expected_kind || stored.resource_id != expected_resource_id {
        return Err(api_error(
            StatusCode::FORBIDDEN,
            "WS_TICKET_SCOPE_MISMATCH",
            "WebSocket ticket is not valid for this channel or room",
        ));
    }

    if !has_permission(&stored.permissions, TappPermission::FederationMessage) {
        return Err(api_error(
            StatusCode::FORBIDDEN,
            "WS_TICKET_PERMISSION_DENIED",
            "WebSocket ticket is missing federation:message",
        ));
    }

    if stored.expires_at <= Utc::now().timestamp() {
        return Err(api_error(
            StatusCode::UNAUTHORIZED,
            "INVALID_WS_TICKET",
            "WebSocket ticket is missing, invalid, expired, or already used",
        ));
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
        has_permission, new_ticket, token_hash, StoredWsTicket, WsTicketKind, TICKET_PREFIX,
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
        assert_eq!(super::TAPP_WS_TICKET_QUERY, "tapp_ws_ticket");
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
}
