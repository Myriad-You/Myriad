use reqwest::{redirect::Policy, Client, ClientBuilder};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;
use url::Url;

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
/// can federate over HTTP. **Never enable in production.**
pub fn federation_lab_private_outbound_enabled() -> bool {
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
    let parsed = Url::parse(url).map_err(|error| format!("Invalid URL: {error}"))?;
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
            .map_err(|error| format!("DNS resolution failed: {error}"))?
            .collect()
    };

    if addresses.is_empty() {
        return Err("DNS resolution returned no addresses".to_string());
    }
    if !federation_lab_private_outbound_enabled() {
        if let Some(blocked) = addresses.iter().find(|address| !is_public_ip(address.ip())) {
            return Err(format!(
                "Target resolves to a non-public address: {}",
                blocked.ip()
            ));
        }
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
        .map_err(|error| format!("HTTP client error: {error}"))?;
    Ok((parsed, client))
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
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| format!("Failed to read response: {error}"))?
    {
        if body.len().saturating_add(chunk.len()) > max_bytes {
            return Err(format!("Response exceeds {max_bytes} bytes"));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// Test-only lock so concurrent tests do not race on
/// `MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND`.
#[cfg(test)]
pub fn tests_lab_env_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
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
    async fn refuses_literal_internal_targets() {
        let _guard = tests_lab_env_lock();
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
        let _guard = tests_lab_env_lock();
        std::env::set_var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND", "1");
        let result = build_public_http_client(
            "http://127.0.0.1:18080/inbox",
            Duration::from_secs(1),
            None,
        )
        .await;
        std::env::remove_var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND");
        assert!(
            result.is_ok(),
            "lab private outbound should allow loopback: {:?}",
            result.err()
        );
    }

    #[test]
    fn lab_flag_env_parsing() {
        let _guard = tests_lab_env_lock();
        std::env::remove_var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND");
        assert!(!federation_lab_private_outbound_enabled());
        std::env::set_var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND", "1");
        assert!(federation_lab_private_outbound_enabled());
        std::env::set_var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND", "true");
        assert!(federation_lab_private_outbound_enabled());
        std::env::set_var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND", "0");
        assert!(!federation_lab_private_outbound_enabled());
        std::env::remove_var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND");
    }

    #[test]
    fn rejects_hop_by_hop_and_routing_headers() {
        assert!(validate_outbound_header(&reqwest::header::HOST).is_err());
        assert!(validate_outbound_header(&reqwest::header::CONNECTION).is_err());
        assert!(validate_outbound_header(&reqwest::header::AUTHORIZATION).is_ok());
    }
}
