//! Optional server-side Tapp attribution for host-proxied Phantasi / speech / federation REST.
//!
//! Phantasi, speech, and federation are host capabilities that Tapp sandboxes reach
//! through the same REST routes the host UI uses. When a request carries the
//! `x-tapp-runtime-grant` header, this middleware validates the grant, enforces
//! the permission mapped to the matched route, applies per-(subject, tapp,
//! operation class) rate limits on write-ish methods, and attributes the call
//! to the issuing Tapp runtime. Requests without the header (host UI traffic)
//! pass through unchanged — no new rate limit is applied.
//!
//! # Domain maps
//!
//! Route → permission facts and write rate-limit gating live in
//! [`crate::services::tapp_host_attribution`] (fixture-driven). This module is
//! the Axum edge only: JWT/Claims, grant validation, rate-limit HTTP mapping,
//! and logging.

use axum::{
    Json,
    extract::{MatchedPath, Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use sea_orm::DatabaseConnection;
use serde_json::json;

use crate::middleware::auth::{Claims, authenticate_request};
use crate::services::permission_service::TappPermission;
use crate::services::tapp_host_attribution::{
    self, error_codes, federation_permission, phantasi_permission, speech_permission,
};

use super::common::check_rate_limit;
use super::runtime_grant::{RUNTIME_GRANT_HEADER, validate_runtime_grant};

type PermissionMapper = fn(&str, &str) -> Option<TappPermission>;

fn attribution_error(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(json!({
            "error": message,
            "code": code
        })),
    )
        .into_response()
}

async fn attribute_host_request(
    db: &DatabaseConnection,
    mut req: Request,
    next: Next,
    mapper: PermissionMapper,
) -> Response {
    let Some(token) = req
        .headers()
        .get(RUNTIME_GRANT_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
    else {
        return next.run(req).await;
    };

    // Speech and federation routes run behind auth_middleware and already carry
    // Claims (with the subject parsed at that boundary); Phantasi routes resolve
    // identity per-handler, so this is their auth boundary: the same full
    // current-session check, never a bare JWT signature check.
    let claims: Claims = match req.extensions().get::<Claims>().cloned() {
        Some(claims) => claims,
        None => match authenticate_request(req.headers(), db).await {
            Ok(claims) => claims,
            Err(response) if response.status() == StatusCode::UNAUTHORIZED => {
                return attribution_error(
                    StatusCode::UNAUTHORIZED,
                    error_codes::UNAUTHENTICATED,
                    "A Tapp-attributed request requires an authenticated subject",
                );
            }
            Err(response) => return *response,
        },
    };

    let grant = match validate_runtime_grant(db, &token, &claims).await {
        Ok(grant) => grant,
        Err(error) => return error.into_response(),
    };

    let matched_path = req
        .extensions()
        .get::<MatchedPath>()
        .map(|path| path.as_str().to_string());
    let Some(permission) = matched_path
        .as_deref()
        .and_then(|path| mapper(req.method().as_str(), path))
    else {
        return attribution_error(
            StatusCode::FORBIDDEN,
            error_codes::PATH_NOT_ALLOWED,
            "This host route is not available to Tapp runtimes",
        );
    };
    if let Err(error) = grant.require(permission) {
        return error.into_response();
    }

    // Coarse per-(subject, tapp, operation class) limits on write-ish methods.
    // Host UI traffic never reaches this branch (no grant header above).
    if let Some(operation) = tapp_host_attribution::host_attribution_rate_limit_operation(
        req.method().as_str(),
        permission,
    ) {
        if let Err(error) =
            check_rate_limit(db, grant.subject_id(), grant.tapp_id(), operation).await
        {
            return error.into_response();
        }
    }

    tracing::info!(
        tapp_id = %grant.tapp_id(),
        runtime_id = %grant.runtime_id(),
        subject_id = grant.subject_id(),
        permission = permission.as_str(),
        path = matched_path.as_deref().unwrap_or_default(),
        method = %req.method(),
        "[TAPP] Host route attributed to Tapp runtime"
    );
    req.extensions_mut().insert(grant);
    next.run(req).await
}

/// Middleware for `/api/speech`: enforce and attribute grant-bearing requests.
pub async fn speech_host_attribution(
    State(db): State<DatabaseConnection>,
    req: Request,
    next: Next,
) -> Response {
    attribute_host_request(&db, req, next, speech_permission).await
}

/// Middleware for `/api/phantasi`: enforce and attribute grant-bearing requests.
pub async fn phantasi_host_attribution(
    State(db): State<DatabaseConnection>,
    req: Request,
    next: Next,
) -> Response {
    attribute_host_request(&db, req, next, phantasi_permission).await
}

/// Middleware for `/api/federation`: enforce and attribute grant-bearing requests.
pub async fn federation_host_attribution(
    State(db): State<DatabaseConnection>,
    req: Request,
    next: Next,
) -> Response {
    attribute_host_request(&db, req, next, federation_permission).await
}
