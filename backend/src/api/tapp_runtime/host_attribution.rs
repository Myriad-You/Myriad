//! Optional server-side Tapp attribution for host-proxied legacy routes.
//!
//! Brew, speech, and federation are host capabilities that Tapp sandboxes reach
//! through the same REST routes the host UI uses. When a request carries the
//! `x-tapp-runtime-grant` header, this middleware validates the grant, enforces
//! the permission mapped to the matched route, applies per-(subject, tapp,
//! operation class) rate limits on write-ish methods, and attributes the call
//! to the issuing Tapp runtime. Requests without the header (host UI traffic)
//! pass through unchanged — no new rate limit is applied.
//!
//! # Keeping maps consistent
//!
//! Route → permission facts live in the machine-readable fixture:
//! `docs/development/tapp/fixtures/host_route_permissions.json`.
//!
//! **Edit the fixture first**, then update sandbox `PERMISSION_MAP` /
//! `action_permissions.json` as needed. Unit tests (and the frontend
//! consistency test) enforce that:
//! - every fixture route is served by the domain mappers below;
//! - every mapper-covered route appears in the fixture;
//! - every permission string exists in [`TappPermission::from_str`].
//!
//! Comment-only sync is not enough: deliberate drift must fail CI.

use std::sync::LazyLock;

use axum::{
    extract::{MatchedPath, Request},
    http::{Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::json;

use crate::middleware::auth::{verify_jwt_token, Claims};
use crate::services::permission_service::TappPermission;

use super::common::{check_rate_limit, host_write_rate_limit_operation};
use super::runtime_grant::{validate_runtime_grant, RUNTIME_GRANT_HEADER};

/// Safe / read methods are never counted against host write rate limits, even
/// when the route permission is a write-capable class (e.g. GET speech voices
/// shares `speech:tts` with POST TTS).
fn is_host_write_method(method: &Method) -> bool {
    !matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

/// Resolve the rate-limit operation class for an attributed host request, or
/// `None` when the call should not consume a host write quota (safe methods or
/// read-only permissions).
fn host_attribution_rate_limit_operation(
    method: &Method,
    permission: TappPermission,
) -> Option<&'static str> {
    if !is_host_write_method(method) {
        return None;
    }
    host_write_rate_limit_operation(permission)
}

/// Compiled fixture path relative to this source file (repo
/// `docs/development/tapp/fixtures/host_route_permissions.json`).
const HOST_ROUTE_PERMISSIONS_JSON: &str =
    include_str!("../../../../docs/development/tapp/fixtures/host_route_permissions.json");

/// Companion action → permission fixture (sandbox `PERMISSION_MAP` domains).
#[cfg(test)]
const ACTION_PERMISSIONS_JSON: &str =
    include_str!("../../../../docs/development/tapp/fixtures/action_permissions.json");

type PermissionMapper = fn(&Method, &str) -> Option<TappPermission>;

#[derive(Debug, Deserialize)]
struct HostRouteFixture {
    routes: Vec<HostRouteEntry>,
}

#[derive(Debug, Clone, Deserialize)]
struct HostRouteEntry {
    domain: String,
    method: String,
    path: String,
    permission: String,
}

/// One compiled fixture row used for O(n) lookup (n is small, ~80 routes).
#[derive(Debug, Clone)]
struct CompiledHostRoute {
    method: String,
    path: String,
    permission: TappPermission,
}

/// method + matched path template → permission, keyed by host domain.
struct HostRouteIndex {
    speech: Vec<CompiledHostRoute>,
    brew: Vec<CompiledHostRoute>,
    federation: Vec<CompiledHostRoute>,
    /// Full fixture rows (for reverse coverage tests).
    #[cfg_attr(not(test), allow(dead_code))]
    entries: Vec<HostRouteEntry>,
}

static HOST_ROUTE_INDEX: LazyLock<HostRouteIndex> = LazyLock::new(load_host_route_index);

fn load_host_route_index() -> HostRouteIndex {
    let fixture: HostRouteFixture = serde_json::from_str(HOST_ROUTE_PERMISSIONS_JSON)
        .expect("host_route_permissions.json must be valid JSON");

    let mut speech = Vec::new();
    let mut brew = Vec::new();
    let mut federation = Vec::new();
    let mut seen = std::collections::BTreeSet::new();

    for entry in &fixture.routes {
        let permission = TappPermission::from_str(&entry.permission).unwrap_or_else(|| {
            panic!(
                "host_route_permissions.json: unknown permission {:?} for {} {}",
                entry.permission, entry.method, entry.path
            )
        });
        let method = entry.method.to_ascii_uppercase();
        let key = (method.clone(), entry.path.clone());
        if !seen.insert(key) {
            panic!(
                "host_route_permissions.json: duplicate route {} {}",
                entry.method, entry.path
            );
        }
        let compiled = CompiledHostRoute {
            method,
            path: entry.path.clone(),
            permission,
        };
        match entry.domain.as_str() {
            "speech" => speech.push(compiled),
            "brew" => brew.push(compiled),
            "federation" => federation.push(compiled),
            other => panic!(
                "host_route_permissions.json: unknown domain {other:?} (expected speech|brew|federation)"
            ),
        }
    }

    HostRouteIndex {
        speech,
        brew,
        federation,
        entries: fixture.routes,
    }
}

fn lookup_permission(
    routes: &[CompiledHostRoute],
    method: &Method,
    path: &str,
) -> Option<TappPermission> {
    let method = method.as_str();
    routes
        .iter()
        .find(|route| route.method.eq_ignore_ascii_case(method) && route.path == path)
        .map(|route| route.permission)
}

/// Route → permission map for `/api/speech`, loaded from
/// `host_route_permissions.json` (mirrors sandbox `speech.*` actions).
fn speech_permission(method: &Method, path: &str) -> Option<TappPermission> {
    lookup_permission(&HOST_ROUTE_INDEX.speech, method, path)
}

/// Route → permission map for `/api/brew`, loaded from
/// `host_route_permissions.json` (mirrors sandbox `brewList.*` actions).
/// Paths that no sandbox handler exposes (WebSocket, RSSHub instance admin,
/// cache management, offline sync) stay unmapped so grant-bearing requests to
/// them are rejected.
fn brew_permission(method: &Method, path: &str) -> Option<TappPermission> {
    lookup_permission(&HOST_ROUTE_INDEX.brew, method, path)
}

/// Route → permission map for `/api/federation`, loaded from
/// `host_route_permissions.json` (mirrors sandbox `federation.*` actions).
///
/// Raw WebSocket *upgrade* paths stay unmapped for grant *headers*: browsers
/// cannot attach `X-Tapp-Runtime-Grant` to WS handshakes, so grant-bearing
/// header requests to `/ws` are rejected here. Tapp attribution for WS uses a
/// one-time ticket (`POST .../ws-ticket` + `?tapp_ws_ticket=` on upgrade); see
/// `ws_ticket` and `federation::ws_gateway`. Host UI WS omits both the grant
/// header and the ticket query param and passes through as Claims-only.
/// E2E key exchange remains host-only / unmapped (not in the fixture).
fn federation_permission(method: &Method, path: &str) -> Option<TappPermission> {
    lookup_permission(&HOST_ROUTE_INDEX.federation, method, path)
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

    // Coarse per-(subject, tapp, operation class) limits on write-ish methods.
    // Host UI traffic never reaches this branch (no grant header above).
    if let Some(operation) = host_attribution_rate_limit_operation(req.method(), permission) {
        if let Err(error) =
            check_rate_limit(grant.subject_id(), grant.tapp_id(), operation).await
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
    use super::{
        brew_permission, federation_permission, host_attribution_rate_limit_operation,
        speech_permission, HostRouteFixture, ACTION_PERMISSIONS_JSON, HOST_ROUTE_INDEX,
        HOST_ROUTE_PERMISSIONS_JSON,
    };
    use crate::services::permission_service::TappPermission;
    use axum::http::Method;
    use serde::Deserialize;
    use std::collections::BTreeSet;

    #[derive(Debug, Deserialize)]
    struct ActionFixture {
        actions: Vec<ActionEntry>,
    }

    #[derive(Debug, Deserialize)]
    struct ActionEntry {
        domain: String,
        action: String,
        permission: String,
    }

    fn mapper_for_domain(domain: &str) -> fn(&Method, &str) -> Option<TappPermission> {
        match domain {
            "speech" => speech_permission,
            "brew" => brew_permission,
            "federation" => federation_permission,
            other => panic!("unknown domain {other}"),
        }
    }

    fn map_for_domain(domain: &str) -> &'static [super::CompiledHostRoute] {
        match domain {
            "speech" => &HOST_ROUTE_INDEX.speech,
            "brew" => &HOST_ROUTE_INDEX.brew,
            "federation" => &HOST_ROUTE_INDEX.federation,
            other => panic!("unknown domain {other}"),
        }
    }

    #[test]
    fn fixture_loads_and_indexes_all_host_domains() {
        assert!(
            !HOST_ROUTE_INDEX.entries.is_empty(),
            "host_route_permissions.json must list at least one route"
        );
        assert!(!HOST_ROUTE_INDEX.speech.is_empty());
        assert!(!HOST_ROUTE_INDEX.brew.is_empty());
        assert!(!HOST_ROUTE_INDEX.federation.is_empty());
    }

    #[test]
    fn every_fixture_route_matches_domain_mapper() {
        for entry in &HOST_ROUTE_INDEX.entries {
            let method = Method::from_bytes(entry.method.as_bytes())
                .unwrap_or_else(|_| panic!("invalid method {}", entry.method));
            let mapper = mapper_for_domain(&entry.domain);
            let expected = TappPermission::from_str(&entry.permission)
                .unwrap_or_else(|| panic!("unknown permission {}", entry.permission));
            assert_eq!(
                mapper(&method, &entry.path),
                Some(expected),
                "fixture route {} {} (domain {}) must map to {}",
                entry.method,
                entry.path,
                entry.domain,
                entry.permission
            );
        }
    }

    #[test]
    fn every_mapper_route_appears_in_fixture() {
        // Reverse coverage: maps are built only from the fixture, so keys must
        // match domain partitions. This still fails if load_host_route_index
        // ever gains a second source of routes.
        let fixture: HostRouteFixture =
            serde_json::from_str(HOST_ROUTE_PERMISSIONS_JSON).expect("valid fixture");
        for domain in ["speech", "brew", "federation"] {
            let expected: BTreeSet<(String, String)> = fixture
                .routes
                .iter()
                .filter(|r| r.domain == domain)
                .map(|r| (r.method.to_ascii_uppercase(), r.path.clone()))
                .collect();
            let actual: BTreeSet<(String, String)> = map_for_domain(domain)
                .iter()
                .map(|r| (r.method.clone(), r.path.clone()))
                .collect();
            assert_eq!(
                actual, expected,
                "domain {domain}: mapper keys must equal fixture entries"
            );
        }
    }

    #[test]
    fn fixture_permissions_exist_in_tapp_permission_enum() {
        let fixture: HostRouteFixture =
            serde_json::from_str(HOST_ROUTE_PERMISSIONS_JSON).expect("valid fixture");
        for entry in &fixture.routes {
            assert!(
                TappPermission::from_str(&entry.permission).is_some(),
                "permission {:?} is not in TappPermission::from_str ({} {})",
                entry.permission,
                entry.method,
                entry.path
            );
            let perm = TappPermission::from_str(&entry.permission).unwrap();
            assert_eq!(
                perm.as_str(),
                entry.permission.as_str(),
                "as_str round-trip for {}",
                entry.permission
            );
        }
    }

    #[test]
    fn action_fixture_permissions_exist_in_tapp_permission_enum() {
        let fixture: ActionFixture =
            serde_json::from_str(ACTION_PERMISSIONS_JSON).expect("valid action fixture");
        assert!(!fixture.actions.is_empty());
        for entry in &fixture.actions {
            assert!(
                TappPermission::from_str(&entry.permission).is_some(),
                "action {} permission {:?} missing from TappPermission::from_str",
                entry.action,
                entry.permission
            );
        }
    }

    #[test]
    fn host_and_action_fixtures_share_permission_string_set_per_domain() {
        // Every permission string used on a host-proxied route for a domain
        // must also appear on at least one sandbox action in that domain
        // (and vice versa for the domains we care about). Catches renames
        // applied on only one side of the stack.
        let host: HostRouteFixture =
            serde_json::from_str(HOST_ROUTE_PERMISSIONS_JSON).expect("valid host fixture");
        let actions: ActionFixture =
            serde_json::from_str(ACTION_PERMISSIONS_JSON).expect("valid action fixture");

        for domain in ["speech", "brew", "federation"] {
            let host_perms: BTreeSet<&str> = host
                .routes
                .iter()
                .filter(|r| r.domain == domain)
                .map(|r| r.permission.as_str())
                .collect();
            let action_perms: BTreeSet<&str> = actions
                .actions
                .iter()
                .filter(|a| a.domain == domain)
                .map(|a| a.permission.as_str())
                .collect();
            assert_eq!(
                host_perms, action_perms,
                "domain {domain}: host route permission set must equal action permission set.\n\
                 host-only: {:?}\naction-only: {:?}",
                host_perms
                    .difference(&action_perms)
                    .copied()
                    .collect::<Vec<_>>(),
                action_perms
                    .difference(&host_perms)
                    .copied()
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn action_fixture_entries_are_unique() {
        let fixture: ActionFixture =
            serde_json::from_str(ACTION_PERMISSIONS_JSON).expect("valid action fixture");
        let mut seen = BTreeSet::new();
        for entry in &fixture.actions {
            assert!(
                seen.insert(entry.action.as_str()),
                "duplicate action in action_permissions.json: {}",
                entry.action
            );
        }
    }

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
        // Raw WS upgrades stay unmapped for grant headers (ticket query is the
        // Tapp path). E2E key exchange remains host-UI only.
        assert_eq!(
            federation_permission(&Method::GET, "/api/federation/channels/{channel_id}/ws"),
            None
        );
        assert_eq!(
            federation_permission(&Method::GET, "/api/federation/rooms/{room_id}/ws"),
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

    #[test]
    fn federation_ws_ticket_mint_routes_require_message_permission() {
        assert_eq!(
            federation_permission(
                &Method::POST,
                "/api/federation/channels/{channel_id}/ws-ticket"
            ),
            Some(TappPermission::FederationMessage)
        );
        assert_eq!(
            federation_permission(&Method::POST, "/api/federation/rooms/{room_id}/ws-ticket"),
            Some(TappPermission::FederationMessage)
        );
    }

    #[test]
    fn host_write_methods_are_rate_limited_by_permission_class() {
        assert_eq!(
            host_attribution_rate_limit_operation(&Method::POST, TappPermission::BrewWrite),
            Some("brew.write")
        );
        assert_eq!(
            host_attribution_rate_limit_operation(&Method::PUT, TappPermission::BrewManage),
            Some("brew.manage")
        );
        assert_eq!(
            host_attribution_rate_limit_operation(&Method::DELETE, TappPermission::BrewComment),
            Some("brew.comment")
        );
        assert_eq!(
            host_attribution_rate_limit_operation(
                &Method::POST,
                TappPermission::FederationMessage
            ),
            Some("federation.message")
        );
        assert_eq!(
            host_attribution_rate_limit_operation(&Method::POST, TappPermission::FederationTrust),
            Some("federation.trust")
        );
        assert_eq!(
            host_attribution_rate_limit_operation(&Method::POST, TappPermission::FederationFiles),
            Some("federation.files")
        );
        assert_eq!(
            host_attribution_rate_limit_operation(&Method::POST, TappPermission::SpeechTts),
            Some("speech.tts")
        );
        assert_eq!(
            host_attribution_rate_limit_operation(&Method::POST, TappPermission::SpeechAsr),
            Some("speech.asr")
        );
    }

    #[test]
    fn host_safe_methods_skip_rate_limit_even_for_write_permissions() {
        // GET speech voices shares speech:tts with POST TTS but must not burn quota.
        assert_eq!(
            host_attribution_rate_limit_operation(&Method::GET, TappPermission::SpeechTts),
            None
        );
        assert_eq!(
            host_attribution_rate_limit_operation(&Method::HEAD, TappPermission::BrewWrite),
            None
        );
        assert_eq!(
            host_attribution_rate_limit_operation(&Method::OPTIONS, TappPermission::FederationWrite),
            None
        );
    }

    #[test]
    fn host_read_permissions_never_rate_limited() {
        assert_eq!(
            host_attribution_rate_limit_operation(&Method::GET, TappPermission::BrewRead),
            None
        );
        assert_eq!(
            host_attribution_rate_limit_operation(&Method::POST, TappPermission::BrewRead),
            None
        );
        assert_eq!(
            host_attribution_rate_limit_operation(&Method::GET, TappPermission::FederationRead),
            None
        );
        assert_eq!(
            host_attribution_rate_limit_operation(&Method::POST, TappPermission::FederationRead),
            None
        );
    }

    #[test]
    fn fixture_write_routes_have_rate_limit_operation() {
        // Every non-GET fixture route with a write-class permission must resolve
        // to a host rate-limit operation so grant-bearing mutations are covered.
        // Includes ws-ticket mint (POST + federation:message → federation.message).
        for entry in &HOST_ROUTE_INDEX.entries {
            let method = Method::from_bytes(entry.method.as_bytes())
                .unwrap_or_else(|_| panic!("invalid method {}", entry.method));
            let permission = TappPermission::from_str(&entry.permission)
                .unwrap_or_else(|| panic!("unknown permission {}", entry.permission));
            let op = host_attribution_rate_limit_operation(&method, permission);
            if matches!(method, Method::GET | Method::HEAD | Method::OPTIONS)
                || matches!(
                    permission,
                    TappPermission::BrewRead | TappPermission::FederationRead
                )
            {
                assert_eq!(
                    op, None,
                    "read path {} {} should not rate-limit",
                    entry.method, entry.path
                );
            } else {
                assert!(
                    op.is_some(),
                    "write path {} {} (permission {}) must map to a rate-limit operation",
                    entry.method,
                    entry.path,
                    entry.permission
                );
            }
        }
    }
}
