//! Shared outbound HTTP safety for Myriad (SSRF-resistant client builder).
//!
//! Workspace crate so `api`, `services`, and `federation` share one implementation
//! without depending on each other. Policy:
//! - only `http`/`https`
//! - no URL credentials
//! - resolve + pin DNS to public (globally routable) addresses
//! - mixed DNS: drop non-public records, fail only when none remain
//! - optional trusted-proxy path skips local DNS pin
//! - redirects disabled
//! - response bodies read with an explicit byte cap
//!
//! Lab-only: `MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND` may allow private/loopback.

use reqwest::{redirect::Policy, Client, ClientBuilder};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use url::Url;

/// Short-lived host-keyed Client cache (MYR-046).
///
/// Each client still has DNS pin + timeout baked in at build time; we only reuse
/// when host, resolved addrs, timeout, and user-agent match. Caps growth so a
/// long process that fans out to many federated hosts does not retain clients forever.
const CLIENT_CACHE_TTL: Duration = Duration::from_secs(60);
const CLIENT_CACHE_MAX: usize = 64;

struct CachedClient {
    addresses: Vec<SocketAddr>,
    timeout: Duration,
    user_agent: Option<String>,
    client: Client,
    built_at: Instant,
}

fn client_cache() -> &'static Mutex<HashMap<String, CachedClient>> {
    static CACHE: OnceLock<Mutex<HashMap<String, CachedClient>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn take_cached_client(
    host: &str,
    addresses: &[SocketAddr],
    timeout: Duration,
    user_agent: Option<&str>,
) -> Option<Client> {
    let mut guard = client_cache().lock().ok()?;
    let now = Instant::now();
    // Drop expired entries when we grow large.
    if guard.len() > CLIENT_CACHE_MAX / 2 {
        guard.retain(|_, e| now.duration_since(e.built_at) < CLIENT_CACHE_TTL);
    }
    let entry = guard.get(host)?;
    if now.duration_since(entry.built_at) >= CLIENT_CACHE_TTL {
        return None;
    }
    if entry.timeout != timeout {
        return None;
    }
    if entry.user_agent.as_deref() != user_agent {
        return None;
    }
    if entry.addresses.as_slice() != addresses {
        return None;
    }
    Some(entry.client.clone())
}

fn store_cached_client(
    host: &str,
    addresses: Vec<SocketAddr>,
    timeout: Duration,
    user_agent: Option<&str>,
    client: Client,
) {
    let Ok(mut guard) = client_cache().lock() else {
        return;
    };
    if guard.len() >= CLIENT_CACHE_MAX {
        // Evict oldest-ish: drop all expired first, then clear if still full.
        let now = Instant::now();
        guard.retain(|_, e| now.duration_since(e.built_at) < CLIENT_CACHE_TTL);
        if guard.len() >= CLIENT_CACHE_MAX {
            guard.clear();
        }
    }
    guard.insert(
        host.to_string(),
        CachedClient {
            addresses,
            timeout,
            user_agent: user_agent.map(str::to_string),
            client,
            built_at: Instant::now(),
        },
    );
}

fn is_public_ipv4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_unspecified()
        || ip.is_multicast()
        || a == 0
        || (a == 100 && (64..=127).contains(&b))
        || (a == 192 && b == 0 && c == 0)
        || (a == 192 && b == 88 && c == 99)
        || (a == 198 && (b == 18 || b == 19))
        || a >= 240)
}

fn is_public_ipv6(ip: Ipv6Addr) -> bool {
    if let Some(embedded) = ip.to_ipv4() {
        return is_public_ipv4(embedded);
    }
    let segments = ip.segments();
    !(ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_unique_local()
        || ip.is_multicast()
        || (segments[0] & 0xffc0) == 0xfe80
        || (segments[0] & 0xffc0) == 0xfec0
        || segments[0] == 0x2002
        || (segments[0] == 0x2001 && matches!(segments[1], 0x0000 | 0x0db8)))
}

pub fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_public_ipv4(ip),
        IpAddr::V6(ip) => is_public_ipv6(ip),
    }
}

/// Local dual-instance federation lab only.
///
/// When `MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND=1` (or `true`/`yes`), outbound
/// HTTP may target loopback/private addresses so two real backends on one machine
/// can federate over HTTP.
///
/// **Hard fail-closed in production:** if `ENVIRONMENT=production`, this always
/// returns `false` even when the lab flag is set (misconfig must not disable SSRF).
pub fn federation_lab_private_outbound_enabled() -> bool {
    if is_production_environment() {
        // Never honor lab SSRF bypass in production (even if the env flag is set).
        // Callers that log may detect the misconfig; we stay fail-closed here without
        // depending on tracing (this crate is leaf / no tracing dep).
        let _ = lab_flag_raw_enabled();
        return false;
    }
    lab_flag_raw_enabled()
}

fn is_production_environment() -> bool {
    std::env::var("ENVIRONMENT")
        .ok()
        .is_some_and(|v| v.trim().eq_ignore_ascii_case("production"))
}

fn lab_flag_raw_enabled() -> bool {
    match std::env::var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND") {
        Ok(v) => matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => false,
    }
}

pub fn validate_outbound_header(name: &reqwest::header::HeaderName) -> Result<(), String> {
    if matches!(
        name.as_str(),
        "host"
            | "connection"
            | "content-length"
            | "transfer-encoding"
            | "upgrade"
            | "proxy-authorization"
            | "proxy-connection"
            | "te"
            | "trailer"
    ) {
        return Err(format!("HTTP header is not allowed: {name}"));
    }
    Ok(())
}

/// Validate and pin DNS for an outbound HTTP target.
///
/// Every resolved address must be globally routable. The resulting client has
/// redirects disabled and its DNS result pinned, closing both redirect and DNS
/// rebinding variants of SSRF. Callers that need redirects must validate each
/// hop and build a fresh client for that hop.
pub async fn build_public_http_client(
    url: &str,
    timeout: Duration,
    user_agent: Option<&str>,
) -> Result<(Url, Client), String> {
    let parsed = Url::parse(url).map_err(|_| "Invalid URL".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("Only HTTP and HTTPS URLs are allowed".to_string());
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("URL credentials are not allowed".to_string());
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| "URL has no host".to_string())?;
    let port = parsed
        .port_or_known_default()
        .ok_or_else(|| "URL has no usable port".to_string())?;

    let addresses: Vec<SocketAddr> = if let Ok(ip) = host.parse::<IpAddr>() {
        vec![SocketAddr::new(ip, port)]
    } else {
        tokio::net::lookup_host((host, port))
            .await
            .map_err(|_| "DNS resolution failed".to_string())?
            .collect()
    };

    let mut addresses = select_outbound_addresses(addresses)?;

    // Stable order for cache key equality.
    addresses.sort_unstable();

    if let Some(client) = take_cached_client(host, &addresses, timeout, user_agent) {
        return Ok((parsed, client));
    }

    let mut builder: ClientBuilder = Client::builder()
        .timeout(timeout)
        .redirect(Policy::none())
        .resolve_to_addrs(host, &addresses);
    if let Some(user_agent) = user_agent {
        builder = builder.user_agent(user_agent);
    }
    let client = builder
        .build()
        .map_err(|_| "HTTP client error".to_string())?;
    store_cached_client(host, addresses, timeout, user_agent, client.clone());
    Ok((parsed, client))
}

/// Keep public addresses when DNS returns a mix of routable and poisoned/private
/// records. Fail only when nothing public remains.
///
/// The previous "any non-public address fails the whole request" check broke
/// declared-API egress in China: polluted A/AAAA answers often include one
/// loopback/CGN/6to4 record alongside a real Cloudflare/AWS address.
fn select_outbound_addresses(addresses: Vec<SocketAddr>) -> Result<Vec<SocketAddr>, String> {
    if addresses.is_empty() {
        return Err("DNS resolution returned no addresses".to_string());
    }
    if federation_lab_private_outbound_enabled() {
        return Ok(addresses);
    }
    let public: Vec<SocketAddr> = addresses
        .into_iter()
        .filter(|address| is_public_ip(address.ip()))
        .collect();
    if public.is_empty() {
        return Err("Target resolves to no public addresses".to_string());
    }
    Ok(public)
}

/// Build a client that sends through a trusted egress proxy.
///
/// Local DNS pin is skipped: the proxy resolves the hostname. That restores the
/// China egress path (`http_client` proxy / `HTTP_PROXY`) which
/// `build_public_http_client` removed when it started failing closed on
/// polluted local DNS. IP literals are still required to be public.
pub async fn build_public_http_client_via_proxy(
    url: &str,
    timeout: Duration,
    user_agent: Option<&str>,
    proxy: reqwest::Proxy,
) -> Result<(Url, Client), String> {
    let parsed = Url::parse(url).map_err(|_| "Invalid URL".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("Only HTTP and HTTPS URLs are allowed".to_string());
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("URL credentials are not allowed".to_string());
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| "URL has no host".to_string())?;
    if let Ok(ip) = host.parse::<IpAddr>() {
        if !federation_lab_private_outbound_enabled() && !is_public_ip(ip) {
            return Err(format!("Target is a non-public address: {ip}"));
        }
    }

    let mut builder: ClientBuilder = Client::builder()
        .timeout(timeout)
        .redirect(Policy::none())
        .proxy(proxy);
    if let Some(user_agent) = user_agent {
        builder = builder.user_agent(user_agent);
    }
    let client = builder
        .build()
        .map_err(|_| "HTTP client error".to_string())?;
    Ok((parsed, client))
}

/// Names the failure mode without ever putting the URL, host or credential
/// into the message — the string reaches sandboxed callers as payload.
///
/// One flattened string made every director drop look identical in the log
/// while all of them were in fact the request timeout.
fn body_read_error(error: reqwest::Error) -> String {
    if error.is_timeout() {
        "Response timed out".to_string()
    } else if error.is_connect() {
        "Connection closed while reading response".to_string()
    } else if error.is_decode() {
        "Failed to decode response".to_string()
    } else {
        "Failed to read response".to_string()
    }
}

/// Read an HTTP body without ever buffering more than the declared limit.
pub async fn read_limited_body(
    mut response: reqwest::Response,
    max_bytes: usize,
) -> Result<Vec<u8>, String> {
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes as u64)
    {
        return Err(format!("Response exceeds {max_bytes} bytes"));
    }
    let mut body =
        Vec::with_capacity(response.content_length().unwrap_or(0).min(max_bytes as u64) as usize);
    while let Some(chunk) = response.chunk().await.map_err(body_read_error)? {
        if body.len().saturating_add(chunk.len()) > max_bytes {
            return Err(format!("Response exceeds {max_bytes} bytes"));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// Serialize every test that touches the lab env flag (async-aware).
///
/// `MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND` is process-global while cargo runs
/// tests in parallel threads, so **readers must hold this too, not only
/// writers**: a test asserting that a private target is refused will otherwise
/// observe another test's open window and see the connect attempt succeed.
///
/// Not `cfg(test)` because a dependency's test-only items are invisible to
/// dependent crates. The lock is per test binary, which is the scope that
/// shares the environment.
pub async fn tests_lab_env_lock() -> tokio::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_public_address_ranges() {
        for ip in [
            "127.0.0.1",
            "10.0.0.1",
            "169.254.169.254",
            "100.64.0.1",
            "198.18.0.1",
            "::1",
            "fc00::1",
            "fe80::1",
            "::ffff:127.0.0.1",
        ] {
            assert!(!is_public_ip(ip.parse().unwrap()), "accepted {ip}");
        }
    }

    #[test]
    fn accepts_globally_routable_addresses() {
        assert!(is_public_ip("1.1.1.1".parse().unwrap()));
        assert!(is_public_ip("2606:4700:4700::1111".parse().unwrap()));
    }

    #[tokio::test]
    async fn mixed_dns_keeps_public_addresses() {
        let _guard = tests_lab_env_lock().await;
        std::env::remove_var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND");
        let mixed = vec![
            "127.0.0.1:443".parse().unwrap(),
            "1.1.1.1:443".parse().unwrap(),
            "100.64.0.1:443".parse().unwrap(),
            "[2606:4700:4700::1111]:443".parse().unwrap(),
        ];
        let selected = select_outbound_addresses(mixed).expect("public addrs");
        let ips: Vec<IpAddr> = selected.into_iter().map(|addr| addr.ip()).collect();
        assert_eq!(
            ips,
            vec![
                "1.1.1.1".parse::<IpAddr>().unwrap(),
                "2606:4700:4700::1111".parse::<IpAddr>().unwrap(),
            ]
        );
    }

    #[tokio::test]
    async fn all_private_dns_is_rejected() {
        let _guard = tests_lab_env_lock().await;
        std::env::remove_var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND");
        let private = vec![
            "127.0.0.1:443".parse().unwrap(),
            "10.0.0.1:443".parse().unwrap(),
        ];
        let error = select_outbound_addresses(private).expect_err("private-only");
        assert!(error.contains("no public addresses"));
    }

    #[tokio::test]
    async fn refuses_literal_internal_targets() {
        let _guard = tests_lab_env_lock().await;
        std::env::remove_var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND");
        let result = build_public_http_client(
            "http://169.254.169.254/latest/meta-data/",
            Duration::from_secs(1),
            None,
        )
        .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn lab_flag_allows_loopback_http_client() {
        let _guard = tests_lab_env_lock().await;
        let prev_env = std::env::var("ENVIRONMENT").ok();
        std::env::remove_var("ENVIRONMENT"); // lab only outside production
        std::env::set_var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND", "1");
        let result =
            build_public_http_client("http://127.0.0.1:18080/inbox", Duration::from_secs(1), None)
                .await;
        std::env::remove_var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND");
        if let Some(v) = prev_env {
            std::env::set_var("ENVIRONMENT", v);
        }
        assert!(
            result.is_ok(),
            "lab private outbound should allow loopback: {:?}",
            result.err()
        );
    }

    #[tokio::test]
    async fn lab_flag_env_parsing() {
        let _guard = tests_lab_env_lock().await;
        let prev_env = std::env::var("ENVIRONMENT").ok();
        std::env::remove_var("ENVIRONMENT");
        std::env::remove_var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND");
        assert!(!federation_lab_private_outbound_enabled());
        std::env::set_var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND", "1");
        assert!(federation_lab_private_outbound_enabled());
        std::env::set_var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND", "true");
        assert!(federation_lab_private_outbound_enabled());
        std::env::set_var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND", "0");
        assert!(!federation_lab_private_outbound_enabled());
        // Production hard-disables lab flag even when set.
        std::env::set_var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND", "1");
        std::env::set_var("ENVIRONMENT", "production");
        assert!(!federation_lab_private_outbound_enabled());
        std::env::remove_var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND");
        std::env::remove_var("ENVIRONMENT");
        if let Some(v) = prev_env {
            std::env::set_var("ENVIRONMENT", v);
        }
    }

    #[test]
    fn rejects_hop_by_hop_and_routing_headers() {
        assert!(validate_outbound_header(&reqwest::header::HOST).is_err());
        assert!(validate_outbound_header(&reqwest::header::CONNECTION).is_err());
        assert!(validate_outbound_header(&reqwest::header::AUTHORIZATION).is_ok());
    }
}
