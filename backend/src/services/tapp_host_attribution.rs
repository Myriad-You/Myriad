//! Host-proxied route → permission maps for Tapp attribution.
//!
//! Brew, speech, and federation are host capabilities that Tapp sandboxes reach
//! through the same REST routes the host UI uses. Domain maps live here so the
//! Axum middleware only validates grants, applies rate limits, and attributes
//! traffic — it does not own fixture indexing or route→permission facts.
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

use serde::Deserialize;

use crate::services::permission_service::TappPermission;
use crate::services::tapp_rate_limit::host_write_rate_limit_operation;

/// Compiled fixture path relative to this source file (repo
/// `docs/development/tapp/fixtures/host_route_permissions.json`).
const HOST_ROUTE_PERMISSIONS_JSON: &str =
    include_str!("../../../docs/development/tapp/fixtures/host_route_permissions.json");

/// Companion action → permission fixture (sandbox `PERMISSION_MAP` domains).
#[cfg(test)]
const ACTION_PERMISSIONS_JSON: &str =
    include_str!("../../../docs/development/tapp/fixtures/action_permissions.json");

/// Attribution error codes preserved at the HTTP edge.
pub mod error_codes {
    pub const UNAUTHENTICATED: &str = "TAPP_ATTRIBUTION_UNAUTHENTICATED";
    pub const PATH_NOT_ALLOWED: &str = "TAPP_HOST_PATH_NOT_ALLOWED";
}

/// Host domains that accept optional `x-tapp-runtime-grant` attribution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HostDomain {
    Speech,
    Brew,
    Federation,
}

impl HostDomain {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Speech => "speech",
            Self::Brew => "brew",
            Self::Federation => "federation",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "speech" => Some(Self::Speech),
            "brew" => Some(Self::Brew),
            "federation" => Some(Self::Federation),
            _ => None,
        }
    }

    pub const ALL: [Self; 3] = [Self::Speech, Self::Brew, Self::Federation];
}

#[derive(Debug, Deserialize)]
struct HostRouteFixture {
    routes: Vec<HostRouteEntry>,
}

/// One fixture row (domain + method + path + permission string).
#[derive(Debug, Clone, Deserialize)]
pub struct HostRouteEntry {
    pub domain: String,
    pub method: String,
    pub path: String,
    pub permission: String,
}

/// One compiled fixture row used for O(n) lookup (n is small, ~80 routes).
#[derive(Debug, Clone)]
pub struct CompiledHostRoute {
    pub method: String,
    pub path: String,
    pub permission: TappPermission,
}

/// method + matched path template → permission, keyed by host domain.
struct HostRouteIndex {
    speech: Vec<CompiledHostRoute>,
    brew: Vec<CompiledHostRoute>,
    federation: Vec<CompiledHostRoute>,
    /// Full fixture rows (for reverse coverage tests / introspection).
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
        match HostDomain::from_str(&entry.domain) {
            Some(HostDomain::Speech) => speech.push(compiled),
            Some(HostDomain::Brew) => brew.push(compiled),
            Some(HostDomain::Federation) => federation.push(compiled),
            None => panic!(
                "host_route_permissions.json: unknown domain {:?} (expected speech|brew|federation)",
                entry.domain
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

fn routes_slice(domain: HostDomain) -> &'static [CompiledHostRoute] {
    match domain {
        HostDomain::Speech => &HOST_ROUTE_INDEX.speech,
        HostDomain::Brew => &HOST_ROUTE_INDEX.brew,
        HostDomain::Federation => &HOST_ROUTE_INDEX.federation,
    }
}

fn lookup_permission(
    routes: &[CompiledHostRoute],
    method: &str,
    path: &str,
) -> Option<TappPermission> {
    routes
        .iter()
        .find(|route| route.method.eq_ignore_ascii_case(method) && route.path == path)
        .map(|route| route.permission)
}

/// Route → permission for a host domain (`method` is case-insensitive).
pub fn permission_for(domain: HostDomain, method: &str, path: &str) -> Option<TappPermission> {
    lookup_permission(routes_slice(domain), method, path)
}

/// Route → permission map for `/api/speech` (mirrors sandbox `speech.*` actions).
pub fn speech_permission(method: &str, path: &str) -> Option<TappPermission> {
    permission_for(HostDomain::Speech, method, path)
}

/// Route → permission map for `/api/brew` (mirrors sandbox `brewList.*` actions).
/// Paths that no sandbox handler exposes (WebSocket, RSSHub instance admin,
/// cache management, offline sync) stay unmapped so grant-bearing requests to
/// them are rejected.
pub fn brew_permission(method: &str, path: &str) -> Option<TappPermission> {
    permission_for(HostDomain::Brew, method, path)
}

/// Route → permission map for `/api/federation`.
///
/// Raw WebSocket *upgrade* paths stay unmapped for grant *headers*: browsers
/// cannot attach `X-Tapp-Runtime-Grant` to WS handshakes, so grant-bearing
/// header requests to `/ws` are rejected here. Tapp attribution for WS uses a
/// one-time ticket (`POST .../ws-ticket` + `?tapp_ws_ticket=` on upgrade).
pub fn federation_permission(method: &str, path: &str) -> Option<TappPermission> {
    permission_for(HostDomain::Federation, method, path)
}

/// Compiled routes for a domain (fixture partition).
pub fn routes_for_domain(domain: HostDomain) -> &'static [CompiledHostRoute] {
    routes_slice(domain)
}

/// All fixture rows in load order.
pub fn fixture_entries() -> &'static [HostRouteEntry] {
    &HOST_ROUTE_INDEX.entries
}

/// Safe / read methods are never counted against host write rate limits, even
/// when the route permission is a write-capable class (e.g. GET speech voices
/// shares `speech:tts` with POST TTS).
pub fn is_host_write_method(method: &str) -> bool {
    !matches!(
        method.to_ascii_uppercase().as_str(),
        "GET" | "HEAD" | "OPTIONS"
    )
}

/// Resolve the rate-limit operation class for an attributed host request, or
/// `None` when the call should not consume a host write quota (safe methods or
/// read-only permissions).
pub fn host_attribution_rate_limit_operation(
    method: &str,
    permission: TappPermission,
) -> Option<&'static str> {
    if !is_host_write_method(method) {
        return None;
    }
    host_write_rate_limit_operation(permission)
}

#[cfg(test)]
mod tests {
    use super::*;
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

    #[test]
    fn fixture_loads_and_indexes_all_host_domains() {
        assert!(
            !fixture_entries().is_empty(),
            "host_route_permissions.json must list at least one route"
        );
        for domain in HostDomain::ALL {
            assert!(
                !routes_for_domain(domain).is_empty(),
                "domain {} must have routes",
                domain.as_str()
            );
        }
    }

    #[test]
    fn every_fixture_route_matches_domain_mapper() {
        for entry in fixture_entries() {
            let domain = HostDomain::from_str(&entry.domain)
                .unwrap_or_else(|| panic!("unknown domain {}", entry.domain));
            let expected = TappPermission::from_str(&entry.permission)
                .unwrap_or_else(|| panic!("unknown permission {}", entry.permission));
            assert_eq!(
                permission_for(domain, &entry.method, &entry.path),
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
        let fixture: HostRouteFixture =
            serde_json::from_str(HOST_ROUTE_PERMISSIONS_JSON).expect("valid fixture");
        for domain in HostDomain::ALL {
            let expected: BTreeSet<(String, String)> = fixture
                .routes
                .iter()
                .filter(|r| r.domain == domain.as_str())
                .map(|r| (r.method.to_ascii_uppercase(), r.path.clone()))
                .collect();
            let actual: BTreeSet<(String, String)> = routes_for_domain(domain)
                .iter()
                .map(|r| (r.method.clone(), r.path.clone()))
                .collect();
            assert_eq!(
                actual, expected,
                "domain {}: mapper keys must equal fixture entries",
                domain.as_str()
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
                host_perms,
                action_perms,
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
            speech_permission("POST", "/api/speech/tts"),
            Some(TappPermission::SpeechTts)
        );
        assert_eq!(
            speech_permission("POST", "/api/speech/asr"),
            Some(TappPermission::SpeechAsr)
        );
        assert_eq!(
            speech_permission("GET", "/api/speech/voices"),
            Some(TappPermission::SpeechTts)
        );
        assert_eq!(speech_permission("GET", "/api/speech/cache/article"), None);
    }

    #[test]
    fn brew_routes_map_to_sandbox_permissions() {
        assert_eq!(
            brew_permission("GET", "/api/brew/items/{id}"),
            Some(TappPermission::BrewRead)
        );
        assert_eq!(
            brew_permission("POST", "/api/brew/items/{id}/read"),
            Some(TappPermission::BrewWrite)
        );
        assert_eq!(
            brew_permission("GET", "/api/brew/items/{id}/comments"),
            Some(TappPermission::BrewRead)
        );
        assert_eq!(
            brew_permission("POST", "/api/brew/items/{id}/comments"),
            Some(TappPermission::BrewCommentWrite)
        );
        assert_eq!(
            brew_permission("POST", "/api/brew/sources"),
            Some(TappPermission::BrewManage)
        );
        assert_eq!(brew_permission("GET", "/api/brew/ws"), None);
        assert_eq!(brew_permission("POST", "/api/brew/sync-states"), None);
        assert_eq!(brew_permission("GET", "/api/brew/rsshub/instances"), None);
    }

    #[test]
    fn federation_keys_rotate_and_cancel_pending_are_post() {
        // rotateKeys: 覆盖本地签名密钥并向 followers fan-out Update(Person)，
        // 是「外部可见广播」的持久副作用 → federation:post。
        assert_eq!(
            federation_permission("POST", "/api/federation/keys/rotate"),
            Some(TappPermission::FederationPost)
        );
        // delivery 管理：只改本用户投递队列状态，但直接影响外部可见广播是否
        // 送达（retry/cancel/dismiss/purge）→ federation:post。
        assert_eq!(
            federation_permission("POST", "/api/federation/delivery/cancel-pending"),
            Some(TappPermission::FederationPost)
        );
        assert_eq!(
            federation_permission("GET", "/api/federation/delivery/stats"),
            Some(TappPermission::FederationRead)
        );
        assert_eq!(
            federation_permission("GET", "/api/federation/delivery"),
            Some(TappPermission::FederationRead)
        );
        assert_eq!(
            federation_permission("POST", "/api/federation/delivery/retry-dead"),
            Some(TappPermission::FederationPost)
        );
        assert_eq!(
            federation_permission("POST", "/api/federation/delivery/{id}/retry"),
            Some(TappPermission::FederationPost)
        );
        assert_eq!(
            federation_permission("POST", "/api/federation/delivery/{id}/cancel"),
            Some(TappPermission::FederationPost)
        );
        assert_eq!(
            federation_permission("DELETE", "/api/federation/delivery/{id}"),
            Some(TappPermission::FederationPost)
        );
        assert_eq!(
            federation_permission("POST", "/api/federation/delivery/purge-dead"),
            Some(TappPermission::FederationPost)
        );
        assert_eq!(
            federation_permission("GET", "/api/federation/identity"),
            Some(TappPermission::FederationRead)
        );
    }

    #[test]
    fn federation_routes_map_to_frontend_permission_domains() {
        assert_eq!(
            federation_permission("GET", "/api/federation/timeline"),
            Some(TappPermission::FederationRead)
        );
        assert_eq!(
            federation_permission(
                "POST",
                "/api/federation/rooms/{room_id}/messages/{message_id}/pin"
            ),
            Some(TappPermission::FederationRoom)
        );
        assert_eq!(
            federation_permission("POST", "/api/federation/channels/{channel_id}/messages"),
            Some(TappPermission::FederationMessage)
        );
        assert_eq!(
            federation_permission("POST", "/api/federation/trust/block"),
            Some(TappPermission::FederationTrust)
        );
        assert_eq!(
            federation_permission("POST", "/api/federation/transfers/{transfer_id}/chunks"),
            Some(TappPermission::FederationFiles)
        );
    }

    #[test]
    fn federation_reads_and_writes_on_the_same_route_are_mapped_separately() {
        assert_eq!(
            federation_permission("GET", "/api/federation/channels/{channel_id}/messages"),
            Some(TappPermission::FederationRead)
        );
        assert_eq!(
            federation_permission("POST", "/api/federation/channels/{channel_id}/messages"),
            Some(TappPermission::FederationMessage)
        );
    }

    #[test]
    fn federation_unmapped_routes_reject_attributed_calls() {
        assert_eq!(
            federation_permission("GET", "/api/federation/channels/{channel_id}/ws"),
            None
        );
        assert_eq!(
            federation_permission("GET", "/api/federation/rooms/{room_id}/ws"),
            None
        );
        assert_eq!(
            federation_permission(
                "POST",
                "/api/federation/rooms/{room_id}/e2e/key-exchange"
            ),
            Some(TappPermission::FederationRoom)
        );
        assert_eq!(
            federation_permission("DELETE", "/api/federation/timeline"),
            None
        );
    }

    #[test]
    fn federation_ws_ticket_mint_routes_require_message_permission() {
        assert_eq!(
            federation_permission("POST", "/api/federation/channels/{channel_id}/ws-ticket"),
            Some(TappPermission::FederationMessage)
        );
        assert_eq!(
            federation_permission("POST", "/api/federation/rooms/{room_id}/ws-ticket"),
            Some(TappPermission::FederationMessage)
        );
    }

    #[test]
    fn federation_write_routes_are_bound_to_action_domains() {
        // 跨域归属（handoff 映射表）：每条写路由只绑定一个动作域权限。
        // Basic 域（interact/ring）路由绝不绑定 Elevated 权限，反之亦然。
        // interact 域
        assert_eq!(
            federation_permission("POST", "/api/federation/follow"),
            Some(TappPermission::FederationInteract)
        );
        assert_eq!(
            federation_permission("POST", "/api/federation/unfollow"),
            Some(TappPermission::FederationInteract)
        );
        assert_eq!(
            federation_permission("POST", "/api/federation/like"),
            Some(TappPermission::FederationInteract)
        );
        assert_eq!(
            federation_permission("POST", "/api/federation/announce"),
            Some(TappPermission::FederationInteract)
        );
        // post 域
        assert_eq!(
            federation_permission("POST", "/api/federation/publish"),
            Some(TappPermission::FederationPost)
        );
        assert_eq!(
            federation_permission("POST", "/api/federation/notes"),
            Some(TappPermission::FederationPost)
        );
        assert_eq!(
            federation_permission("POST", "/api/federation/media"),
            Some(TappPermission::FederationPost)
        );
        // channel 域
        assert_eq!(
            federation_permission("POST", "/api/federation/channels"),
            Some(TappPermission::FederationChannel)
        );
        assert_eq!(
            federation_permission("POST", "/api/federation/channels/{channel_id}/accept"),
            Some(TappPermission::FederationChannel)
        );
        // room 域
        assert_eq!(
            federation_permission("POST", "/api/federation/rooms/{room_id}/invite"),
            Some(TappPermission::FederationRoom)
        );
        assert_eq!(
            federation_permission("PUT", "/api/federation/rooms/{room_id}"),
            Some(TappPermission::FederationRoom)
        );
        // room sticker 路由与 action addRoomSticker/removeRoomSticker 同域
        // （federation:room）：host route 与 action fixture 必须 lockstep。
        assert_eq!(
            federation_permission("POST", "/api/federation/rooms/{room_id}/stickers"),
            Some(TappPermission::FederationRoom)
        );
        assert_eq!(
            federation_permission(
                "DELETE",
                "/api/federation/rooms/{room_id}/stickers/{sticker_id}"
            ),
            Some(TappPermission::FederationRoom)
        );
        // ring 域
        assert_eq!(
            federation_permission("POST", "/api/federation/rings"),
            Some(TappPermission::FederationRing)
        );
        assert_eq!(
            federation_permission("POST", "/api/federation/rings/{ring_id}/sync"),
            Some(TappPermission::FederationRing)
        );

        // 跨域负例：Elevated 路由不能落在 Basic 域权限上。
        for (method, path) in [
            ("POST", "/api/federation/publish"),
            ("POST", "/api/federation/notes"),
            ("POST", "/api/federation/channels"),
            ("POST", "/api/federation/rooms"),
        ] {
            let permission = federation_permission(method, path).unwrap();
            assert!(
                !matches!(
                    permission,
                    TappPermission::FederationInteract | TappPermission::FederationRing
                ),
                "{method} {path} must not resolve to a Basic federation domain"
            );
        }
        // 跨域负例：Basic 域路由不能落在 Elevated 权限上。
        for (method, path) in [
            ("POST", "/api/federation/follow"),
            ("POST", "/api/federation/bookmark"),
            ("POST", "/api/federation/rings"),
            ("POST", "/api/federation/rings/{ring_id}/peers"),
        ] {
            let permission = federation_permission(method, path).unwrap();
            assert!(
                !matches!(
                    permission,
                    TappPermission::FederationPost
                        | TappPermission::FederationChannel
                        | TappPermission::FederationRoom
                ),
                "{method} {path} must not resolve to an Elevated federation domain"
            );
        }
    }

    #[test]
    fn host_write_methods_are_rate_limited_by_permission_class() {
        assert_eq!(
            host_attribution_rate_limit_operation("POST", TappPermission::BrewWrite),
            Some("brew.write")
        );
        assert_eq!(
            host_attribution_rate_limit_operation("PUT", TappPermission::BrewManage),
            Some("brew.manage")
        );
        assert_eq!(
            host_attribution_rate_limit_operation("DELETE", TappPermission::BrewCommentWrite),
            Some("brew.commentWrite")
        );
        assert_eq!(
            host_attribution_rate_limit_operation("POST", TappPermission::FederationPost),
            Some("federation.post")
        );
        assert_eq!(
            host_attribution_rate_limit_operation("POST", TappPermission::FederationInteract),
            Some("federation.interact")
        );
        assert_eq!(
            host_attribution_rate_limit_operation("POST", TappPermission::FederationChannel),
            Some("federation.channel")
        );
        assert_eq!(
            host_attribution_rate_limit_operation("POST", TappPermission::FederationRoom),
            Some("federation.room")
        );
        assert_eq!(
            host_attribution_rate_limit_operation("POST", TappPermission::FederationRing),
            Some("federation.ring")
        );
        assert_eq!(
            host_attribution_rate_limit_operation("POST", TappPermission::FederationMessage),
            Some("federation.message")
        );
        assert_eq!(
            host_attribution_rate_limit_operation("POST", TappPermission::FederationTrust),
            Some("federation.trust")
        );
        assert_eq!(
            host_attribution_rate_limit_operation("POST", TappPermission::FederationFiles),
            Some("federation.files")
        );
        assert_eq!(
            host_attribution_rate_limit_operation("POST", TappPermission::SpeechTts),
            Some("speech.tts")
        );
        assert_eq!(
            host_attribution_rate_limit_operation("POST", TappPermission::SpeechAsr),
            Some("speech.asr")
        );
    }

    #[test]
    fn host_safe_methods_skip_rate_limit_even_for_write_permissions() {
        assert_eq!(
            host_attribution_rate_limit_operation("GET", TappPermission::SpeechTts),
            None
        );
        assert_eq!(
            host_attribution_rate_limit_operation("HEAD", TappPermission::BrewWrite),
            None
        );
        assert_eq!(
            host_attribution_rate_limit_operation("OPTIONS", TappPermission::FederationPost),
            None
        );
        // Case-insensitive method matching.
        assert_eq!(
            host_attribution_rate_limit_operation("get", TappPermission::SpeechTts),
            None
        );
    }

    #[test]
    fn host_read_permissions_never_rate_limited() {
        assert_eq!(
            host_attribution_rate_limit_operation("GET", TappPermission::BrewRead),
            None
        );
        assert_eq!(
            host_attribution_rate_limit_operation("POST", TappPermission::BrewRead),
            None
        );
        assert_eq!(
            host_attribution_rate_limit_operation("GET", TappPermission::FederationRead),
            None
        );
        assert_eq!(
            host_attribution_rate_limit_operation("POST", TappPermission::FederationRead),
            None
        );
    }

    #[test]
    fn fixture_write_routes_have_rate_limit_operation() {
        for entry in fixture_entries() {
            let method_upper = entry.method.to_ascii_uppercase();
            let permission = TappPermission::from_str(&entry.permission)
                .unwrap_or_else(|| panic!("unknown permission {}", entry.permission));
            let op = host_attribution_rate_limit_operation(&entry.method, permission);
            if matches!(method_upper.as_str(), "GET" | "HEAD" | "OPTIONS")
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

    #[test]
    fn error_codes_preserve_api_contract() {
        assert_eq!(error_codes::UNAUTHENTICATED, "TAPP_ATTRIBUTION_UNAUTHENTICATED");
        assert_eq!(error_codes::PATH_NOT_ALLOWED, "TAPP_HOST_PATH_NOT_ALLOWED");
    }
}
