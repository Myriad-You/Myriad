//! One-time WebSocket tickets for Tapp-attributed federation subscriptions.
//!
//! Domain mint/consume lives in [`crate::services::tapp_ws_ticket`]. This module
//! owns Axum route handlers and HTTP error mapping. Federation WS gateway should
//! call services (or the re-exported consume adapter) rather than reimplementing
//! ticket storage.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use sea_orm::DatabaseConnection;
use serde::Serialize;
use serde_json::json;

use crate::middleware::auth::Claims;
use crate::services::permission_service::TappPermission;
use crate::error::HttpError;
use crate::services::tapp_ws_ticket::{self, WsTicketError, WsTicketMintIdentity};

use super::RuntimeGrantContext;

pub use crate::services::tapp_ws_ticket::{
    ConsumedWsTicket, WsTicketKind, TAPP_WS_TICKET_QUERY,
};

// Force the public name into the non-test binary so renames stay intentional.
const _: &str = TAPP_WS_TICKET_QUERY;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WsTicketResponse {
    pub ticket: String,
    pub expires_at: String,
}

fn ticket_http_error(err: WsTicketError) -> HttpError {
    let status =
        StatusCode::from_u16(err.status_hint()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    HttpError::from((
        status,
        Json(json!({
            "error": err.message(),
            "code": err.code(),
        })),
    ))
}

async fn mint_ticket(
    db: &DatabaseConnection,
    grant: &RuntimeGrantContext,
    kind: WsTicketKind,
    resource_id: String,
) -> Result<Json<WsTicketResponse>, HttpError> {
    grant.require(TappPermission::FederationMessage)?;

    let minted = tapp_ws_ticket::mint_ws_ticket(
        db,
        &WsTicketMintIdentity {
            runtime_id: grant.runtime_id().to_string(),
            tapp_id: grant.tapp_id().to_string(),
            owner_id: grant.owner_id(),
            subject_id: grant.subject_id(),
        },
        kind,
        &resource_id,
    )
    .await
    .map_err(ticket_http_error)?;

    Ok(Json(WsTicketResponse {
        ticket: minted.ticket,
        expires_at: minted.expires_at.to_rfc3339(),
    }))
}

/// POST /api/federation/channels/{channel_id}/ws-ticket
pub async fn mint_channel_ws_ticket(
    State(db): State<DatabaseConnection>,
    grant: RuntimeGrantContext,
    Path(channel_id): Path<String>,
) -> Result<Json<WsTicketResponse>, HttpError> {
    mint_ticket(&db, &grant, WsTicketKind::Channel, channel_id).await
}

/// POST /api/federation/rooms/{room_id}/ws-ticket
pub async fn mint_room_ws_ticket(
    State(db): State<DatabaseConnection>,
    grant: RuntimeGrantContext,
    Path(room_id): Path<String>,
) -> Result<Json<WsTicketResponse>, HttpError> {
    mint_ticket(&db, &grant, WsTicketKind::Room, room_id).await
}

/// Consume a one-time ticket for a federation WebSocket upgrade.
///
/// Thin adapter: parse subject from Claims, map domain errors to [`HttpError`].
/// Prefer [`tapp_ws_ticket::consume_ws_ticket`] from non-HTTP layers
/// (`federation::ws_gateway` already does). Kept for path-stable public API.
#[allow(dead_code)]
pub async fn consume_ws_ticket(
    db: &DatabaseConnection,
    ticket: &str,
    claims: &Claims,
    expected_kind: WsTicketKind,
    expected_resource_id: &str,
) -> Result<ConsumedWsTicket, HttpError> {
    let subject_id: i32 = claims
        .sub
        .parse()
        .map_err(|_| ticket_http_error(WsTicketError::InvalidSubject))?;
    tapp_ws_ticket::consume_ws_ticket(db, ticket, subject_id, expected_kind, expected_resource_id)
        .await
        .map_err(ticket_http_error)
}
