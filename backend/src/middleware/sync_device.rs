//! `sync_device_auth`: the gate in front of the routes a device uses to sync
//! her (`/api/agent/sync/*`) and of her link (`/api/agent/link`).
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
//! (`myriad_persona_sync::hlc::check_ahead`). The site's clock here is real
//! time when the site has measured its own clock and can trust it
//! (`services::server_clock`): a site whose host is behind does not refuse
//! a device that is right. Without such a measurement it is this host's
//! clock, as before. `hello` requires it;
//! `leave` never refuses it, so a device with a wrong clock can still leave.
//! The link does not read it at all: nothing on the link is ordered by a
//! device's clock.

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
use crate::services::server_clock;
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
/// than it may be. Carries the site's time it was compared with, where that
/// time came from (`serverTimeSource`: `clock`, this host's clock, or
/// `sntp`/`https`, real time by the site's measurement of its clock) and the
/// measurement itself (`serverClock`), so the device can tell by how much
/// and which side is wrong.
pub fn clock_ahead(ahead: hlc::Ahead, server_time: i64, source: &str) -> Response {
    (
        StatusCode::CONFLICT,
        Json(json!({
            "error": "This device's clock is ahead of the site's; set it right and sync again",
            "code": "clock_ahead",
            "serverTime": server_time,
            "serverTimeSource": source,
            "serverClock": server_clock::shown(),
            "aheadMs": ahead.ahead_ms,
            "maxAheadMs": hlc::MAX_AHEAD_MS,
        })),
    )
        .into_response()
}

/// `rev` against real time as the site knows it from this host's clock at
/// `host_micros` and its last check of that clock
/// ([`server_clock::real_now_at`]): Ok, or the time and source it was too
/// far ahead of.
fn check_clock(
    rev: &Rev,
    host_micros: i64,
    last: Option<(server_clock::Check, std::time::Duration)>,
) -> Result<(), (hlc::Ahead, i64, &'static str)> {
    let (now, source) = server_clock::real_now_at(host_micros, last);
    hlc::check_ahead(rev, now).map_err(|ahead| (ahead, now, source))
}

/// How a gate takes the device's clock.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ClockRule {
    /// Refused when too far ahead.
    Check,
    /// Taken as it is, even far ahead (leaving).
    AheadToo,
    /// Not read (the link).
    Ignore,
}

/// The clock in `req`, if it has one, or the answer refusing it.
fn clock_of(req: &Request, rule: ClockRule) -> Result<Option<Rev>, Response> {
    if rule == ClockRule::Ignore {
        return Ok(None);
    }
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
    if rule == ClockRule::Check {
        check_clock(
            &rev,
            chrono::Utc::now().timestamp_micros(),
            server_clock::last(),
        )
        .map_err(|(ahead, now, source)| clock_ahead(ahead, now, source))?;
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
    gate(db, req, next, ClockRule::Check).await
}

/// The gate for `leave`: the same, except that a clock too far ahead does
/// not keep a device from giving its token back.
pub async fn sync_device_leave_auth(
    State(db): State<DatabaseConnection>,
    req: Request,
    next: Next,
) -> Response {
    gate(db, req, next, ClockRule::AheadToo).await
}

/// The gate for her link: the same device checks, before the upgrade, and no
/// clock.
pub async fn sync_device_link_auth(
    State(db): State<DatabaseConnection>,
    req: Request,
    next: Next,
) -> Response {
    gate(db, req, next, ClockRule::Ignore).await
}

async fn gate(db: DatabaseConnection, mut req: Request, next: Next, rule: ClockRule) -> Response {
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
    let clock = match clock_of(&req, rule) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::server_clock::{Check, MAX_AGE, Measurement, Source};
    use std::time::Duration;

    const MINUTE_US: i64 = 60 * 1_000_000;
    /// Real time.
    const REAL: i64 = 1_761_200_000_000_000;

    fn rev_at(micros: i64) -> Rev {
        Rev::new((micros / 1000) as u64, 0, "dev_test").unwrap()
    }

    /// The site's host 10 minutes behind, as a check measured it.
    fn behind(trusted: bool, age: Duration) -> Option<(Check, Duration)> {
        Some((
            Check {
                checked_at_ms: (REAL - 10 * MINUTE_US) / 1000,
                measured: Some(Measurement {
                    offset_ms: -10 * 60 * 1000,
                    source: Source::Https,
                    trusted,
                }),
                threshold_ms: 60_000,
            },
            age,
        ))
    }

    #[test]
    fn a_right_device_passes_a_site_whose_host_is_behind() {
        let host = REAL - 10 * MINUTE_US;
        let fresh = behind(true, Duration::from_secs(60));
        assert!(check_clock(&rev_at(REAL), host, fresh).is_ok());
        // Without the measurement it was refused (the incident).
        assert!(check_clock(&rev_at(REAL), host, None).is_err());
    }

    #[test]
    fn a_device_ahead_of_real_time_is_still_refused() {
        let host = REAL - 10 * MINUTE_US;
        let fresh = behind(true, Duration::from_secs(60));
        let (ahead, now, source) =
            check_clock(&rev_at(REAL + 6 * MINUTE_US), host, fresh).unwrap_err();
        assert_eq!((now, source), (REAL, "https"), "real time, and from where");
        assert!(ahead.ahead_ms >= 6 * 60 * 1000 - 1);
    }

    #[test]
    fn without_a_trusted_fresh_measurement_the_hosts_clock_as_before() {
        let host = REAL - 10 * MINUTE_US;
        for last in [
            None,
            behind(false, Duration::from_secs(60)),
            behind(true, MAX_AGE + Duration::from_secs(1)),
        ] {
            let (_, now, source) = check_clock(&rev_at(REAL), host, last).unwrap_err();
            assert_eq!((now, source), (host, "clock"));
        }
    }

    #[tokio::test]
    async fn clock_ahead_says_where_the_sites_time_came_from() {
        let ahead = hlc::check_ahead(&rev_at(REAL + 6 * MINUTE_US), REAL).unwrap_err();
        let response = clock_ahead(ahead, REAL, "https");
        assert_eq!(response.status(), StatusCode::CONFLICT);
        let body = axum::body::to_bytes(response.into_body(), 1 << 16)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["code"], "clock_ahead");
        assert_eq!(body["serverTime"], REAL);
        assert_eq!(body["serverTimeSource"], "https");
        assert!(body.get("serverClock").is_some(), "{body}");
        assert_eq!(body["maxAheadMs"], 300_000);
    }
}
