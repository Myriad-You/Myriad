//! Keep login cookies working when the site is opened over plain HTTP at a
//! host other than its public one (a LAN IP, `localhost` on the machine
//! running Docker Desktop).
//!
//! Cookies get `Secure` when the public site URL is `https://`
//! ([`crate::oauth_url_builder::SiteConfig::is_production`]). A browser
//! drops a `Secure` cookie set by a plain-HTTP page, except on `localhost`,
//! so opening `http://192.168.x.x` signed nobody in. This layer strips
//! `Secure` from the response's cookies only when the request is neither
//! for the public host nor reported as HTTPS by the proxy. Requests for the
//! public host keep `Secure` even when an outer TLS proxy is not listed in
//! `PROXY_TRUSTED_UPSTREAMS` (the Myriad proxy then reports `http`).

use axum::extract::Request;
use axum::http::uri::Authority;
use axum::http::{HeaderMap, HeaderValue, header};
use axum::middleware::Next;
use axum::response::Response;

pub async fn cookie_secure_middleware(req: Request, next: Next) -> Response {
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let proto = req
        .headers()
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let mut response = next.run(req).await;
    if !response.headers().contains_key(header::SET_COOKIE) {
        return response;
    }
    let base_url = crate::oauth_url_builder::SiteConfig::get_base_url().await;
    if keep_secure(&base_url, host.as_deref(), proto.as_deref()) {
        return response;
    }
    strip_secure(response.headers_mut());
    response
}

/// Whether `Secure` set from the public URL is right for this request.
fn keep_secure(base_url: &str, host: Option<&str>, forwarded_proto: Option<&str>) -> bool {
    let Ok(public) = url::Url::parse(base_url) else {
        return true;
    };
    if public.scheme() != "https" {
        return true;
    }
    if forwarded_proto.is_some_and(|p| p.trim().eq_ignore_ascii_case("https")) {
        return true;
    }
    let Some(host) = host.and_then(|h| h.parse::<Authority>().ok()) else {
        return true;
    };
    public
        .host_str()
        .is_none_or(|public_host| public_host.eq_ignore_ascii_case(host.host()))
}

fn strip_secure(headers: &mut HeaderMap) {
    let cookies: Vec<HeaderValue> = headers
        .get_all(header::SET_COOKIE)
        .iter()
        .cloned()
        .collect();
    headers.remove(header::SET_COOKIE);
    for cookie in cookies {
        let value = cookie
            .to_str()
            .ok()
            .and_then(|s| HeaderValue::from_str(&without_secure(s)).ok())
            .unwrap_or(cookie);
        headers.append(header::SET_COOKIE, value);
    }
}

fn without_secure(cookie: &str) -> String {
    cookie
        .split(';')
        .map(str::trim)
        .filter(|attr| !attr.eq_ignore_ascii_case("secure"))
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const PUBLIC: &str = "https://myriad.example";

    #[test]
    fn lan_http_drops_secure() {
        assert!(!keep_secure(PUBLIC, Some("192.168.1.20"), Some("http")));
        assert!(!keep_secure(PUBLIC, Some("192.168.1.20:8080"), None));
        assert!(!keep_secure(PUBLIC, Some("localhost"), Some("http")));
    }

    #[test]
    fn public_host_keeps_secure_even_if_proxy_says_http() {
        assert!(keep_secure(PUBLIC, Some("myriad.example"), Some("http")));
        assert!(keep_secure(PUBLIC, Some("MYRIAD.example:443"), None));
    }

    #[test]
    fn https_on_another_host_keeps_secure() {
        assert!(keep_secure(PUBLIC, Some("other.example"), Some("https")));
    }

    #[test]
    fn http_public_url_or_missing_host_is_untouched() {
        assert!(keep_secure("http://localhost", Some("192.168.1.20"), None));
        assert!(keep_secure(PUBLIC, None, Some("http")));
    }

    #[test]
    fn strips_only_the_secure_attribute() {
        assert_eq!(
            without_secure("auth_token=a; Path=/; HttpOnly; SameSite=Lax; Max-Age=60; Secure"),
            "auth_token=a; Path=/; HttpOnly; SameSite=Lax; Max-Age=60"
        );
        assert_eq!(without_secure("secure_x=1; Path=/"), "secure_x=1; Path=/");
    }

    #[test]
    fn rewrites_every_cookie() {
        let mut headers = HeaderMap::new();
        for c in ["a=1; Path=/; Secure", "b=2; Path=/; HttpOnly; Secure"] {
            headers.append(header::SET_COOKIE, HeaderValue::from_static(c));
        }
        strip_secure(&mut headers);
        let cookies: Vec<_> = headers
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|v| v.to_str().unwrap())
            .collect();
        assert_eq!(cookies, ["a=1; Path=/", "b=2; Path=/; HttpOnly"]);
    }
}
