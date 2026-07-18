use axum::{extract::ConnectInfo, http::HeaderMap};
use std::net::{IpAddr, SocketAddr};

fn parse_forwarded_for(headers: &HeaderMap) -> Option<IpAddr> {
    headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        // Myriad proxy rewrites XFF to a single resolved client IP. Prefer the
        // rightmost hop if a chain is still present (closest to our edge).
        .and_then(|value| {
            value
                .split(',')
                .rev()
                .find_map(|entry| entry.trim().parse::<IpAddr>().ok())
        })
}

fn parse_real_ip(headers: &HeaderMap) -> Option<IpAddr> {
    headers
        .get("x-real-ip")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<IpAddr>().ok())
}

pub fn trusted_proxy_headers_enabled() -> bool {
    std::env::var("TRUST_PROXY_HEADERS")
        .ok()
        .is_some_and(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes"
            )
        })
}

pub fn client_ip_from_parts(
    headers: &HeaderMap,
    peer_ip: Option<IpAddr>,
    trust_proxy_headers: bool,
) -> Option<IpAddr> {
    if trust_proxy_headers {
        // Prefer X-Real-IP: the Myriad proxy sets a single resolved client IP there.
        // Then XFF, then the TCP peer (usually the docker network address of proxy).
        parse_real_ip(headers)
            .or_else(|| parse_forwarded_for(headers))
            .or(peer_ip)
    } else {
        peer_ip
    }
}

pub fn extract_client_ip(request: &axum::extract::Request) -> Option<IpAddr> {
    let peer_ip = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(address)| address.ip());
    client_ip_from_parts(request.headers(), peer_ip, trusted_proxy_headers_enabled())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn ignores_forwarded_headers_unless_proxy_trust_is_enabled() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("203.0.113.10, 198.51.100.4"),
        );
        let peer = "192.0.2.7".parse().unwrap();

        assert_eq!(
            client_ip_from_parts(&headers, Some(peer), false),
            Some(peer)
        );
    }

    #[test]
    fn trusted_proxy_uses_rightmost_forwarded_address() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("203.0.113.10, 198.51.100.4"),
        );

        assert_eq!(
            client_ip_from_parts(&headers, None, true),
            Some("198.51.100.4".parse().unwrap())
        );
    }

    #[test]
    fn trusted_proxy_prefers_x_real_ip_over_xff() {
        let mut headers = HeaderMap::new();
        headers.insert("x-real-ip", HeaderValue::from_static("203.0.113.50"));
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("198.51.100.4"),
        );
        let peer = "10.0.0.2".parse().unwrap();

        assert_eq!(
            client_ip_from_parts(&headers, Some(peer), true),
            Some("203.0.113.50".parse().unwrap())
        );
    }

    #[test]
    fn trusted_proxy_falls_back_to_peer_without_headers() {
        let headers = HeaderMap::new();
        let peer = "10.0.0.2".parse().unwrap();

        assert_eq!(
            client_ip_from_parts(&headers, Some(peer), true),
            Some(peer)
        );
    }

    #[test]
    fn trusted_proxy_skips_malformed_xff_tokens() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("not-an-ip, 203.0.113.10"),
        );

        assert_eq!(
            client_ip_from_parts(&headers, None, true),
            Some("203.0.113.10".parse().unwrap())
        );
    }
}
