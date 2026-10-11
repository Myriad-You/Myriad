//! `sync_device_auth`: the gate in front of the routes a device uses to sync
//! her (`/api/agent/sync/*`).
//!
//! It accepts only a device token (`Authorization: Bearer mys_…`) and never
//! a session: those routes are for devices. The other way round holds by
//! construction, since every session route verifies a JWT signature and a
//! device token has none. A device of a previous site owner is revoked the
//! moment it is used. Neither the token nor its hash appears in any answer
//! or log.
//!
//! A request may carry the device's clock, `X-Sync-Clock: <rev>` (the
//! revision its next write would get). One more than five minutes ahead of
//! the site's clock is refused, 409 `clock_ahead` with the site's time, so
//! the device can say its clock is wrong and sync again once it is right
//! (`myriad_persona_sync::hlc::check_ahead`). `hello` and `role` require it;
//! `leave` never refuses it, so a device with a wrong clock can still leave.

use axum::{
    Json,
    extract::{Request, State},
    http::{StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use sea_orm::DatabaseConnection;
use serde_json::json;

use crate::services::persona_sync::devices::{self, Device, Refused};
use myriad_persona_sync::{Rev, hlc};

/// The device a sync request comes from.
#[derive(Debug, Clone)]
pub struct SyncDevice(pub Device);

/// The device's clock as the request gave it, already checked: not too far
/// ahead of the site's. None when the request gave none.
#[derive(Debug, Clone)]
pub struct SyncClock(pub Option<Rev>);

/// The header a device's clock comes in.
pub const SYNC_CLOCK_HEADER: &str = "x-sync-clock";

/// 409 `clock_ahead`: the device's clock is ahead of the site's by more
/// than it may be. Carries the site's time so the device can tell by how
/// much.
pub fn clock_ahead(ahead: hlc::Ahead, server_time: i64) -> Response {
    (
        StatusCode::CONFLICT,
        Json(json!({
            "error": "This device's clock is ahead of the site's; set it right and sync again",
            "code": "clock_ahead",
            "serverTime": server_time,
            "aheadMs": ahead.ahead_ms,
            "maxAheadMs": hlc::MAX_AHEAD_MS,
        })),
    )
        .into_response()
}

/// The clock in `req`, if it has one, or the answer refusing it. With
/// `ahead_too` a clock too far ahead is taken as it is (leaving).
fn clock_of(req: &Request, ahead_too: bool) -> Result<Option<Rev>, Response> {
    let Some(raw) = req.headers().get(SYNC_CLOCK_HEADER) else {
        return Ok(None);
    };
    let rev = raw
        .to_str()
        .ok()
        .and_then(|raw| raw.trim().parse::<Rev>().ok())
        .ok_or_else(|| {
            refuse(
                StatusCode::BAD_REQUEST,
                "X-Sync-Clock is a revision",
                "invalid_sync_clock",
            )
        })?;
    let now = chrono::Utc::now().timestamp_micros();
    if !ahead_too {
        hlc::check_ahead(&rev, now).map_err(|ahead| clock_ahead(ahead, now))?;
    }
    Ok(Some(rev))
}

fn refuse(status: StatusCode, error: &str, code: &str) -> Response {
    (status, Json(json!({ "error": error, "code": code }))).into_response()
}

pub async fn sync_device_auth(
    State(db): State<DatabaseConnection>,
    req: Request,
    next: Next,
) -> Response {
    gate(db, req, next, false).await
}

/// The gate for `leave`: the same, except that a clock too far ahead does
/// not keep a device from giving its token back.
pub async fn sync_device_leave_auth(
    State(db): State<DatabaseConnection>,
    req: Request,
    next: Next,
) -> Response {
    gate(db, req, next, true).await
}

async fn gate(db: DatabaseConnection, mut req: Request, next: Next, ahead_too: bool) -> Response {
    let Some(token) = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::trim)
        .map(str::to_string)
    else {
        return refuse(
            StatusCode::UNAUTHORIZED,
            "A device token is required",
            "device_token_required",
        );
    };
    let owner = match crate::services::principal::site_owner_id(&db).await {
        Ok(owner) => owner,
        Err(error) => {
            tracing::error!(%error, "[sync] could not resolve the site owner");
            return refuse(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not check the device",
                "internal_error",
            );
        }
    };
    let device = match devices::authenticate(&db, &token, owner).await {
        Ok(device) => device,
        Err(Refused::Unknown) => {
            return refuse(
                StatusCode::UNAUTHORIZED,
                "Unknown device token",
                "device_token_invalid",
            );
        }
        Err(Refused::Revoked) => {
            return refuse(
                StatusCode::UNAUTHORIZED,
                "This device was removed from the site",
                "device_revoked",
            );
        }
        Err(Refused::OwnerChanged) => {
            return refuse(
                StatusCode::FORBIDDEN,
                "The site has a new owner; this device was removed",
                "owner_changed",
            );
        }
        Err(Refused::NoOwner) => {
            return refuse(
                StatusCode::FORBIDDEN,
                "The site has no owner yet",
                "site_owner_required",
            );
        }
        Err(Refused::Db(error)) => {
            tracing::error!(%error, "[sync] could not check a device token");
            return refuse(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not check the device",
                "internal_error",
            );
        }
    };
    if let Err(error) = devices::seen(&db, &device.id).await {
        tracing::warn!(%error, device = %device.id, "[sync] could not record a device as seen");
    }
    let clock = match clock_of(&req, ahead_too) {
        Ok(clock) => clock,
        Err(refused) => {
            tracing::info!(device = %device.id, "[sync] a device's clock was refused");
            return refused;
        }
    };
    req.extensions_mut().insert(SyncDevice(device));
    req.extensions_mut().insert(SyncClock(clock));
    next.run(req).await
}
