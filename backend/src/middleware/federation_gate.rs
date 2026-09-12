//! Closes the federation HTTP surface when the egress-location gate is shut.
//!
//! Matching is by path prefix rather than per route on purpose. The federation
//! router already applies its auth and host-attribution layers at Router level
//! for the same reason (see `api::federation::rooms_and_router::router`): a
//! route added later inherits the gate instead of silently shipping an open
//! endpoint because a per-route layer was forgotten.
//!
//! The response is 404, not 503. A blocked instance is not "temporarily
//! unavailable" — to a remote peer it simply does not federate, and 404 does not
//! invite the retry-with-backoff that 503 does.

use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

/// Path prefixes owned entirely by federation. A match requires at least one
/// character after the prefix, so SEO `/library` and `/reports` (and trailing
/// slash) stay reachable. `/brew` is not a remainder-sibling of `/brew/articles/`.
const FEDERATION_PATH_PREFIXES: &[&str] = &[
    "/api/federation/",
    "/api/tapp/federation/",
    "/api/admin/federation/",
    // AP actor, avatar, inbox, outbox, followers, following.
    "/users/",
    // Dereferenceable AP object ids emitted by `generate_activity_id` and the
    // Create activities that embed them.
    "/activities/",
    "/notes/",
    "/reports/",
    "/tapps/",
    "/library/",
    "/brew/articles/",
];

/// Exact paths (mount `/media/federation`; children are `FEDERATION_NESTED_PREFIXES`).
const FEDERATION_EXACT_PATHS: &[&str] = &[
    "/.well-known/webfinger",
    "/.well-known/nodeinfo",
    "/nodeinfo/2.1",
    "/inbox",
    "/media/federation",
];

/// Prefixes matched with no minimum remainder (the nested media service).
const FEDERATION_NESTED_PREFIXES: &[&str] = &["/media/federation/"];

pub(crate) fn is_federation_path(path: &str) -> bool {
    if FEDERATION_EXACT_PATHS.contains(&path) {
        return true;
    }
    if FEDERATION_NESTED_PREFIXES
        .iter()
        .any(|prefix| path.starts_with(prefix))
    {
        return true;
    }
    FEDERATION_PATH_PREFIXES
        .iter()
        .any(|prefix| path.len() > prefix.len() && path.starts_with(prefix))
}

/// Refuse this request?
///
/// Pure so tests can call it without touching process-wide `federation_enabled()`.
pub(crate) fn should_refuse(path: &str, federation_enabled: bool) -> bool {
    !federation_enabled && is_federation_path(path)
}

pub async fn federation_gate_middleware(req: Request, next: Next) -> Response {
    let enabled = crate::services::federation_gate::federation_enabled();
    let path = req.uri().path();
    if !should_refuse(path, enabled) {
        return next.run(req).await;
    }

    tracing::debug!(
        path,
        "federation request refused: egress-location gate is closed"
    );
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "error": "Not Found",
            "message": "Federation is disabled on this instance",
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::{is_federation_path, should_refuse};

    /// An open gate is fully transparent — no path is refused.
    #[test]
    fn open_gate_refuses_nothing() {
        for path in ["/api/federation/timeline", "/users/alice", "/inbox", "/"] {
            assert!(!should_refuse(path, true), "{path} must pass when enabled");
        }
    }

    /// A closed gate takes the federation surface and nothing else.
    #[test]
    fn closed_gate_refuses_only_federation() {
        assert!(should_refuse("/api/federation/timeline", false));
        assert!(should_refuse("/users/alice/inbox", false));
        assert!(should_refuse("/.well-known/webfinger", false));
        assert!(!should_refuse("/api/config", false));
        assert!(!should_refuse("/library", false));
        assert!(!should_refuse("/", false));
    }

    #[test]
    fn matches_federation_api_surface() {
        for path in [
            "/api/federation/timeline",
            "/api/federation/public/limits",
            "/api/federation/avatar-cache/abc.png",
            "/api/tapp/federation/feed",
            "/api/tapp/federation/rooms-feed",
            "/api/admin/federation/domain-move",
        ] {
            assert!(is_federation_path(path), "{path} must be gated");
        }
    }

    #[test]
    fn matches_activitypub_surface() {
        for path in [
            "/.well-known/webfinger",
            "/.well-known/nodeinfo",
            "/nodeinfo/2.1",
            "/inbox",
            "/users/alice",
            "/users/alice/inbox",
            "/users/alice/outbox",
            "/users/alice/followers",
            "/activities/1",
            "/notes/1",
            "/reports/1",
            "/tapps/1",
            "/library/1",
            "/brew/articles/1",
            "/media/federation",
            "/media/federation/2026/pic.png",
        ] {
            assert!(is_federation_path(path), "{path} must be gated");
        }
    }

    /// SEO `/library` and `/reports` are one segment above object ids; `/brew`
    /// is two (`/brew/articles/{id}`). Closed gate must not take SPA paths.
    #[test]
    fn spares_sibling_non_federation_routes() {
        for path in [
            "/library",
            "/library/",
            "/reports",
            "/reports/",
            "/brew",
            "/brew/",
            "/brew/item/42",
            "/tapp",
            "/tapp/detail/42",
            "/tapp/store",
            "/api/tapp/reports",
            "/api/config",
            "/health",
            "/",
            "/assets/index.js",
        ] {
            assert!(!is_federation_path(path), "{path} must stay reachable");
        }
    }

    /// This middleware matches the raw request path, so it is only sound if the
    /// router does not reach a federation handler through some other spelling of
    /// the same path. If axum matched a percent-decoded path, `/%61pi/...` would
    /// run the handler while `is_federation_path` saw the unmatched raw form —
    /// a silent bypass of the whole gate. Pin the behaviour it depends on.
    #[tokio::test]
    async fn percent_encoded_paths_do_not_reach_federation_routes() {
        use axum::{body::Body, http::Request, routing::get, Router};
        use tower::ServiceExt;

        let app = Router::new().route(
            "/api/federation/timeline",
            get(|| async { "reached the handler" }),
        );

        for spelling in [
            "/%61pi/federation/timeline",
            "/api/%66ederation/timeline",
            "/api/federation/../federation/timeline",
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(spelling)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                axum::http::StatusCode::NOT_FOUND,
                "{spelling} reached a federation route the gate would not have matched"
            );
        }
    }

    /// `/users` is federation-only, but the bare collection path is not a route.
    #[test]
    fn bare_prefix_without_remainder_is_not_matched() {
        assert!(!is_federation_path("/users"));
        assert!(!is_federation_path("/users/"));
        assert!(!is_federation_path("/activities/"));
    }
}
