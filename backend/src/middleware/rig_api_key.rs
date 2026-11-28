//! `rig_api_key_auth`: the gate in front of the rig API (`/rig-api/v1/*`).
//!
//! It takes only an API key (`Authorization: Bearer mrk_…`, see
//! [`services::rig_api::keys`](crate::services::rig_api::keys)) and never a
//! site session: a session JWT is not the shape of a key, and a key is not
//! a JWT, so neither side takes the other's credential. A request that
//! passes is counted against its tenant's per-minute limit; the tenant, its
//! limits and the key it came with go to the handler as [`ApiTenant`].
//!
//! Every refusal is an error body with a stable `code`: 401 for a missing,
//! malformed, unknown, revoked or expired key (with `WWW-Authenticate`), 403
//! for a disabled tenant, 429 `rig_api_rate_limited` with `Retry-After`
//! for a tenant over its requests per minute. Neither the key nor its hash
//! is logged or answered with.

use axum::{
    extract::{FromRequestParts, Request, State},
    http::{HeaderMap, HeaderName, HeaderValue, header, request::Parts},
    middleware::Next,
    response::Response,
};
use chrono::{DateTime, Utc};
use myriad_error::AppError;
use sea_orm::DatabaseConnection;

use crate::services::rig_api::keys::{self, Refused};
use crate::services::rig_api::quota::{self, Limits, RateCount};
use crate::services::rig_api::tenants::Tenant;

/// The tenant a rig API request comes from.
#[derive(Clone, Debug)]
pub struct ApiTenant {
    pub tenant: Tenant,
    /// The key it called with: its id and public prefix, never the key.
    pub key_id: String,
    pub key_prefix: String,
    pub key_expires_at: Option<DateTime<Utc>>,
    /// Its limits as they were when the request came in.
    pub limits: Limits,
}

/// Fails closed on a route the gate is not in front of.
impl<S: Send + Sync> FromRequestParts<S> for ApiTenant {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<ApiTenant>()
            .cloned()
            .ok_or_else(|| unauthorized(AppError::unauthorized("API key required")))
    }
}

const RATE_LIMIT_LIMIT: HeaderName = HeaderName::from_static("x-ratelimit-limit");
const RATE_LIMIT_REMAINING: HeaderName = HeaderName::from_static("x-ratelimit-remaining");
const RATE_LIMIT_RESET: HeaderName = HeaderName::from_static("x-ratelimit-reset");

fn with_headers(error: AppError, headers: Vec<(HeaderName, String)>) -> Response {
    let mut response = crate::error::app_error_response(error);
    for (name, value) in headers {
        if let Ok(value) = HeaderValue::from_str(&value) {
            response.headers_mut().insert(name, value);
        }
    }
    response
}

fn unauthorized(error: AppError) -> Response {
    with_headers(
        error,
        vec![(
            header::WWW_AUTHENTICATE,
            "Bearer realm=\"rig-api\"".to_owned(),
        )],
    )
}

fn unavailable() -> Response {
    crate::error::app_error_response(
        AppError::service_unavailable("Database error")
            .with_message("The API key could not be checked; try again."),
    )
}

/// The answer to a request over its tenant's requests per minute.
pub fn rate_limited(limit: i32, resets_at: DateTime<Utc>, retry_after_secs: u64) -> Response {
    with_headers(
        AppError::too_many_requests("API rate limit exceeded")
            .with_message(format!(
                "This tenant may make {limit} requests a minute; try again in \
                 {retry_after_secs} seconds."
            ))
            .with_details(serde_json::json!({
                "limit": limit,
                "resetsAt": resets_at,
                "retryAfterSeconds": retry_after_secs,
            })),
        vec![
            (header::RETRY_AFTER, retry_after_secs.to_string()),
            (RATE_LIMIT_LIMIT, limit.to_string()),
            (RATE_LIMIT_REMAINING, "0".to_owned()),
            (RATE_LIMIT_RESET, retry_after_secs.to_string()),
        ],
    )
}

/// The key in `Authorization: Bearer <key>`: `Ok(None)` without the header,
/// `Err` when the header is there but holds no bearer credential.
fn bearer(headers: &HeaderMap) -> Result<Option<&str>, ()> {
    let Some(value) = headers.get(header::AUTHORIZATION) else {
        return Ok(None);
    };
    let value = value.to_str().map_err(|_| ())?;
    let (scheme, credential) = value.split_once(' ').ok_or(())?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return Err(());
    }
    let credential = credential.trim();
    if credential.is_empty() {
        return Err(());
    }
    Ok(Some(credential))
}

fn refused(refused: Refused) -> Response {
    match refused {
        Refused::Malformed => unauthorized(
            AppError::unauthorized("Malformed API key")
                .with_message("An API key is `mrk_`, 12 characters, `_` and 43 characters."),
        ),
        Refused::Unknown => unauthorized(AppError::unauthorized("Invalid API key")),
        Refused::Revoked => unauthorized(AppError::unauthorized("API key revoked")),
        Refused::Expired => unauthorized(AppError::unauthorized("API key expired")),
        Refused::TenantDisabled => crate::error::app_error_response(
            AppError::forbidden("Tenant disabled")
                .with_message("This tenant is disabled; its keys are refused."),
        ),
        Refused::Db(error) => {
            tracing::error!(%error, "[rig-api] could not check an API key");
            unavailable()
        }
    }
}

pub async fn rig_api_key_auth(
    State(db): State<DatabaseConnection>,
    mut req: Request,
    next: Next,
) -> Response {
    let now = crate::clock::now();
    let presented = match bearer(req.headers()) {
        Ok(Some(key)) => key,
        Ok(None) => {
            return unauthorized(
                AppError::unauthorized("API key required")
                    .with_message("Send the key as `Authorization: Bearer <key>`."),
            );
        }
        Err(()) => return refused(Refused::Malformed),
    };
    let found = match keys::authenticate(&db, presented, now).await {
        Ok(found) => found,
        Err(why) => return refused(why),
    };
    let limits = match quota::limits_for(&db, found.tenant.overrides).await {
        Ok(limits) => limits,
        Err(error) => {
            tracing::error!(%error, tenant = %found.tenant.id, "[rig-api] could not read limits");
            return unavailable();
        }
    };
    let counted = match quota::count_request(&db, &found.tenant.id, limits.requests_per_minute, now)
        .await
    {
        Ok(counted) => counted,
        Err(error) => {
            tracing::error!(%error, tenant = %found.tenant.id, "[rig-api] could not count a request");
            return unavailable();
        }
    };
    let (used, resets_at) = match counted {
        RateCount::Counted {
            used, resets_at, ..
        } => (used, resets_at),
        RateCount::Limited {
            limit,
            resets_at,
            retry_after_secs,
        } => {
            tracing::info!(tenant = %found.tenant.id, "[rig-api] a tenant is over its rate limit");
            return rate_limited(limit, resets_at, retry_after_secs);
        }
    };
    if let Err(error) = keys::touch(&db, &found.key.id, now).await {
        tracing::warn!(%error, key = %found.key.id, "[rig-api] could not record a key as used");
    }
    req.extensions_mut().insert(ApiTenant {
        key_id: found.key.id,
        key_prefix: found.key.prefix,
        key_expires_at: found.key.expires_at,
        tenant: found.tenant,
        limits,
    });
    let mut response = next.run(req).await;
    for (name, value) in [
        (RATE_LIMIT_LIMIT, limits.requests_per_minute.to_string()),
        (
            RATE_LIMIT_REMAINING,
            (limits.requests_per_minute - used).max(0).to_string(),
        ),
        (
            RATE_LIMIT_RESET,
            quota::seconds_until(now, resets_at).to_string(),
        ),
    ] {
        if let Ok(value) = HeaderValue::from_str(&value) {
            response.headers_mut().insert(name, value);
        }
    }
    response
}
