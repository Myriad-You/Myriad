//! Optional server-side Tapp attribution for host-proxied legacy routes.
//!
//! Brew, speech, and federation are host capabilities that Tapp sandboxes reach
//! through the same REST routes the host UI uses. When a request carries the
//! `x-tapp-runtime-grant` header, this middleware validates the grant, enforces
//! the permission mapped to the matched route and attributes the call to the
//! issuing Tapp runtime. Requests without the header (host UI traffic) pass
//! through unchanged.

use axum::{
    extract::{MatchedPath, Request},
    http::{Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

use crate::middleware::auth::{verify_jwt_token, Claims};
use crate::services::permission_service::TappPermission;

use super::runtime_grant::{validate_runtime_grant, RUNTIME_GRANT_HEADER};

type PermissionMapper = fn(&Method, &str) -> Option<TappPermission>;

/// Route → permission map for `/api/speech`, mirroring the sandbox
/// `PERMISSION_MAP` (`speech.*` actions).
fn speech_permission(method: &Method, path: &str) -> Option<TappPermission> {
    match (method.as_str(), path) {
        ("POST", "/api/speech/tts") | ("POST", "/api/speech/tts/batch") => {
            Some(TappPermission::SpeechTts)
        }
        ("POST", "/api/speech/asr") => Some(TappPermission::SpeechAsr),
        ("GET", "/api/speech/status") | ("GET", "/api/speech/voices") => {
            Some(TappPermission::SpeechTts)
        }
        _ => None,
    }
}

/// Route → permission map for `/api/brew`, mirroring the sandbox
/// `PERMISSION_MAP` (`brewList.*` actions). Paths that no sandbox handler
/// exposes (WebSocket, RSSHub instance admin, cache management, offline sync)
/// stay unmapped so grant-bearing requests to them are rejected.
fn brew_permission(method: &Method, path: &str) -> Option<TappPermission> {
    use TappPermission::{BrewComment, BrewManage, BrewRead, BrewWrite};
    match (method.as_str(), path) {
        ("GET", "/api/brew/sources")
        | ("GET", "/api/brew/sources/{id}")
        | ("GET", "/api/brew/export-opml")
        | ("GET", "/api/brew/categories")
        | ("GET", "/api/brew/items")
        | ("GET", "/api/brew/items/{id}")
        | ("GET", "/api/brew/items/{id}/fulltext")
        | ("GET", "/api/brew/stats") => Some(BrewRead),
        ("POST", "/api/brew/items/{id}/read")
        | ("POST", "/api/brew/items/{id}/unread")
        | ("POST", "/api/brew/items/{id}/star")
        | ("POST", "/api/brew/items/{id}/unstar")
        | ("POST", "/api/brew/mark-all-read") => Some(BrewWrite),
        ("GET", "/api/brew/items/{id}/comments")
        | ("POST", "/api/brew/items/{id}/comments")
        | ("PUT", "/api/brew/comments/{id}")
        | ("DELETE", "/api/brew/comments/{id}")
        | ("GET", "/api/brew/comments/{id}/replies") => Some(BrewComment),
        ("POST", "/api/brew/sources")
        | ("PUT", "/api/brew/sources/{id}")
        | ("DELETE", "/api/brew/sources/{id}")
        | ("POST", "/api/brew/sources/{id}/refresh")
        | ("POST", "/api/brew/sources/discover")
        | ("POST", "/api/brew/import-opml")
        | ("POST", "/api/brew/categories")
        | ("PUT", "/api/brew/categories/{id}")
        | ("DELETE", "/api/brew/categories/{id}") => Some(BrewManage),
        _ => None,
    }
}

/// Route → permission map for `/api/federation`, mirroring frontend
/// `permissionConfig` / sandbox `PERMISSION_MAP` (`federation.*` actions).
/// Host-only paths (E2E key exchange, WebSocket upgrades) stay unmapped so
/// grant-bearing requests to them are rejected. Browser WebSockets cannot
/// carry custom headers and are therefore outside the grant path until a
/// ticket-based handshake exists; without a grant header they still
/// passthrough as host UI traffic.
fn federation_permission(method: &Method, path: &str) -> Option<TappPermission> {
    use TappPermission::{
        FederationFiles, FederationMessage, FederationRead, FederationTrust, FederationWrite,
    };
    match (method.as_str(), path) {
        // federation:read
        ("GET", "/api/federation/identity")
        | ("GET", "/api/federation/timeline")
        | ("GET", "/api/federation/following")
        | ("GET", "/api/federation/followers")
        | ("GET", "/api/federation/published")
        | ("GET", "/api/federation/channels")
        | ("GET", "/api/federation/channels/{channel_id}")
        | ("GET", "/api/federation/channels/{channel_id}/messages")
        | ("GET", "/api/federation/rooms")
        | ("GET", "/api/federation/rooms/{room_id}")
        | ("GET", "/api/federation/rooms/{room_id}/members")
        | ("GET", "/api/federation/rooms/{room_id}/messages")
        | ("GET", "/api/federation/rings")
        | ("GET", "/api/federation/rings/{ring_id}")
        | ("GET", "/api/federation/rings/{ring_id}/peers") => Some(FederationRead),

        // federation:write
        ("POST", "/api/federation/follow")
        | ("POST", "/api/federation/unfollow")
        | ("POST", "/api/federation/publish")
        | ("POST", "/api/federation/unpublish")
        | ("POST", "/api/federation/channels")
        | ("POST", "/api/federation/channels/{channel_id}/accept")
        | ("POST", "/api/federation/channels/{channel_id}/close")
        | ("POST", "/api/federation/rooms")
        | ("PUT", "/api/federation/rooms/{room_id}")
        | ("DELETE", "/api/federation/rooms/{room_id}")
        | ("POST", "/api/federation/rooms/{room_id}/invite")
        | ("DELETE", "/api/federation/rooms/{room_id}/members/{actor}")
        | ("POST", "/api/federation/rooms/{room_id}/leave")
        | ("POST", "/api/federation/rooms/{room_id}/messages/{message_id}/pin")
        | ("POST", "/api/federation/rings")
        | ("POST", "/api/federation/rings/{ring_id}/leave")
        | ("POST", "/api/federation/rings/{ring_id}/peers")
        | ("DELETE", "/api/federation/rings/{ring_id}/peers/{peer}")
        | ("POST", "/api/federation/rings/{ring_id}/sync") => Some(FederationWrite),

        // federation:message
        ("POST", "/api/federation/channels/{channel_id}/messages")
        | ("POST", "/api/federation/rooms/{room_id}/messages") => Some(FederationMessage),

        // federation:trust
        ("GET", "/api/federation/trust/policy")
        | ("GET", "/api/federation/trust/instances")
        | ("POST", "/api/federation/trust/update")
        | ("POST", "/api/federation/trust/block") => Some(FederationTrust),

        // federation:files
        ("GET", "/api/federation/channels/{channel_id}/transfers")
        | ("POST", "/api/federation/channels/{channel_id}/transfers")
        | ("GET", "/api/federation/transfers/{transfer_id}")
        | ("POST", "/api/federation/transfers/{transfer_id}/chunks")
        | ("POST", "/api/federation/transfers/{transfer_id}/cancel") => Some(FederationFiles),

        _ => None,
    }
}

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
    // Claims; Brew routes resolve identity per-handler, so fall back to the JWT
    // directly.
    let claims: Claims = match req.extensions().get::<Claims>().cloned() {
        Some(claims) => claims,
        None => match verify_jwt_token(req.headers()) {
            Ok(claims) => claims,
            Err(_) => {
                return attribution_error(
                    StatusCode::UNAUTHORIZED,
                    "TAPP_ATTRIBUTION_UNAUTHENTICATED",
                    "A Tapp-attributed request requires an authenticated subject",
                );
            }
        },
    };

    let grant = match validate_runtime_grant(&token, &claims).await {
        Ok(grant) => grant,
        Err(error) => return error.into_response(),
    };

    let matched_path = req
        .extensions()
        .get::<MatchedPath>()
        .map(|path| path.as_str().to_string());
    let Some(permission) = matched_path
        .as_deref()
        .and_then(|path| mapper(req.method(), path))
    else {
        return attribution_error(
            StatusCode::FORBIDDEN,
            "TAPP_HOST_PATH_NOT_ALLOWED",
            "This host route is not available to Tapp runtimes",
        );
    };
    if let Err(error) = grant.require(permission) {
        return error.into_response();
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
pub async fn speech_host_attribution(req: Request, next: Next) -> Response {
    attribute_host_request(req, next, speech_permission).await
}

/// Middleware for `/api/brew`: enforce and attribute grant-bearing requests.
pub async fn brew_host_attribution(req: Request, next: Next) -> Response {
    attribute_host_request(req, next, brew_permission).await
}

/// Middleware for `/api/federation`: enforce and attribute grant-bearing requests.
pub async fn federation_host_attribution(req: Request, next: Next) -> Response {
    attribute_host_request(req, next, federation_permission).await
}

#[cfg(test)]
mod tests {
    use super::{brew_permission, federation_permission, speech_permission};
    use crate::services::permission_service::TappPermission;
    use axum::http::Method;

    #[test]
    fn speech_routes_map_to_sandbox_permissions() {
        assert_eq!(
            speech_permission(&Method::POST, "/api/speech/tts"),
            Some(TappPermission::SpeechTts)
        );
        assert_eq!(
            speech_permission(&Method::POST, "/api/speech/asr"),
            Some(TappPermission::SpeechAsr)
        );
        assert_eq!(
            speech_permission(&Method::GET, "/api/speech/voices"),
            Some(TappPermission::SpeechTts)
        );
        // Cache management is host-only.
        assert_eq!(
            speech_permission(&Method::GET, "/api/speech/cache/article"),
            None
        );
    }

    #[test]
    fn brew_routes_map_to_sandbox_permissions() {
        assert_eq!(
            brew_permission(&Method::GET, "/api/brew/items/{id}"),
            Some(TappPermission::BrewRead)
        );
        assert_eq!(
            brew_permission(&Method::POST, "/api/brew/items/{id}/read"),
            Some(TappPermission::BrewWrite)
        );
        assert_eq!(
            brew_permission(&Method::POST, "/api/brew/items/{id}/comments"),
            Some(TappPermission::BrewComment)
        );
        assert_eq!(
            brew_permission(&Method::POST, "/api/brew/sources"),
            Some(TappPermission::BrewManage)
        );
        // Host-only routes stay unmapped and are denied for Tapp runtimes.
        assert_eq!(brew_permission(&Method::GET, "/api/brew/ws"), None);
        assert_eq!(
            brew_permission(&Method::POST, "/api/brew/sync-states"),
            None
        );
        assert_eq!(
            brew_permission(&Method::GET, "/api/brew/rsshub/instances"),
            None
        );
    }

    #[test]
    fn federation_routes_map_to_frontend_permission_domains() {
        assert_eq!(
            federation_permission(&Method::GET, "/api/federation/timeline"),
            Some(TappPermission::FederationRead)
        );
        assert_eq!(
            federation_permission(
                &Method::POST,
                "/api/federation/rooms/{room_id}/messages/{message_id}/pin"
            ),
            Some(TappPermission::FederationWrite)
        );
        assert_eq!(
            federation_permission(
                &Method::POST,
                "/api/federation/channels/{channel_id}/messages"
            ),
            Some(TappPermission::FederationMessage)
        );
        assert_eq!(
            federation_permission(&Method::POST, "/api/federation/trust/block"),
            Some(TappPermission::FederationTrust)
        );
        assert_eq!(
            federation_permission(
                &Method::POST,
                "/api/federation/transfers/{transfer_id}/chunks"
            ),
            Some(TappPermission::FederationFiles)
        );
    }

    #[test]
    fn federation_reads_and_writes_on_the_same_route_are_mapped_separately() {
        assert_eq!(
            federation_permission(
                &Method::GET,
                "/api/federation/channels/{channel_id}/messages"
            ),
            Some(TappPermission::FederationRead)
        );
        assert_eq!(
            federation_permission(
                &Method::POST,
                "/api/federation/channels/{channel_id}/messages"
            ),
            Some(TappPermission::FederationMessage)
        );
    }

    #[test]
    fn federation_unmapped_routes_reject_attributed_calls() {
        // WebSocket upgrades and E2E key exchange are host-UI only.
        assert_eq!(
            federation_permission(&Method::GET, "/api/federation/channels/{channel_id}/ws"),
            None
        );
        assert_eq!(
            federation_permission(
                &Method::POST,
                "/api/federation/rooms/{room_id}/e2e/key-exchange"
            ),
            None
        );
        // Method mismatches never fall back to a broader mapping.
        assert_eq!(
            federation_permission(&Method::DELETE, "/api/federation/timeline"),
            None
        );
    }
}
