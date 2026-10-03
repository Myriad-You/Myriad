//! HTTP(S) URL rules for manifest fields. Parsed with `url::Url`.

use crate::contract_rules::{
    HTTP_URL_SCHEMES, MAX_HTTP_URL_LEN, REMOTE_MEDIA_RESERVED_SUFFIXES,
    REMOTE_MEDIA_WILDCARD_DENIED_SUFFIXES,
};

pub fn validate_http_url(value: &str, field: &str) -> Result<(), String> {
    if value.len() > MAX_HTTP_URL_LEN {
        return Err(format!("Tapp {field} is too long"));
    }
    let parsed = url::Url::parse(value).map_err(|_| format!("Invalid Tapp {field}"))?;
    if !HTTP_URL_SCHEMES.contains(&parsed.scheme()) {
        return Err(format!("Tapp {field} must be an HTTP(S) URL"));
    }
    Ok(())
}

/// Stricter URL rules for host-mediated navigation targets (`openUrls`).
/// HTTPS only, except http on loopback hosts for local development.
pub fn validate_open_url_target(value: &str, field: &str) -> Result<url::Url, String> {
    if value.len() > MAX_HTTP_URL_LEN {
        return Err(format!("Tapp {field} is too long"));
    }
    if value
        .chars()
        .any(|ch| ch.is_control() || ch.is_whitespace())
    {
        return Err(format!(
            "Tapp {field} must not contain whitespace or control characters"
        ));
    }
    let parsed = url::Url::parse(value).map_err(|_| format!("Invalid Tapp {field}"))?;
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(format!("Tapp {field} must not include URL credentials"));
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| format!("Tapp {field} must include a hostname"))?;
    let is_loopback =
        host.eq_ignore_ascii_case("localhost") || host == "127.0.0.1" || host == "::1";
    match parsed.scheme() {
        "https" => {}
        "http" if is_loopback => {}
        "http" => {
            return Err(format!(
                "Tapp {field} must use HTTPS (http is only allowed for localhost)"
            ));
        }
        _ => return Err(format!("Tapp {field} must be an HTTPS URL")),
    }
    if parsed.cannot_be_a_base() {
        return Err(format!("Tapp {field} must be an absolute hierarchical URL"));
    }
    Ok(parsed)
}

/// `openUrls` entries with `match: same-origin` declare a rooted relative path.
/// The host resolves it against its own page origin at open time, so one package
/// can deep-link the station's pages on any self-hosted domain. No origin is
/// declared, so nothing here can escape to another host.
pub fn validate_same_origin_open_url_path(value: &str, field: &str) -> Result<(), String> {
    if value.len() > MAX_HTTP_URL_LEN {
        return Err(format!("Tapp {field} is too long"));
    }
    if value
        .chars()
        .any(|ch| ch.is_control() || ch.is_whitespace())
    {
        return Err(format!(
            "Tapp {field} must not contain whitespace or control characters"
        ));
    }
    if !value.starts_with('/') {
        return Err(format!(
            "Tapp {field} for match=same-origin must be a rooted relative path (e.g. /journal)"
        ));
    }
    if value.starts_with("//") {
        return Err(format!(
            "Tapp {field} for match=same-origin must not be protocol-relative"
        ));
    }
    if value.contains('#') {
        return Err(format!("Tapp {field} must not include a #fragment"));
    }
    if value.contains('\\') {
        return Err(format!(
            "Tapp {field} for match=same-origin must not contain path traversal"
        ));
    }
    // Inspect the raw path before URL parsing removes dot segments. Query
    // values are data, so traversal-like text there does not change the path.
    let path = value.split('?').next().unwrap_or(value);
    let bytes = path.as_bytes();
    if bytes.iter().enumerate().any(|(index, &byte)| {
        byte == b'%'
            && (index + 2 >= bytes.len()
                || !bytes[index + 1].is_ascii_hexdigit()
                || !bytes[index + 2].is_ascii_hexdigit())
    }) {
        return Err(format!("Tapp {field} has invalid path encoding"));
    }
    let decoded = percent_encoding::percent_decode_str(path)
        .decode_utf8()
        .map_err(|_| format!("Tapp {field} has invalid path encoding"))?;
    if decoded.split('/').any(|segment| segment == "..")
        || decoded.contains('\\')
        || decoded.chars().any(|ch| ch.is_ascii_control())
    {
        return Err(format!(
            "Tapp {field} for match=same-origin must not contain path traversal or control characters"
        ));
    }
    Ok(())
}

fn is_under_suffix(host: &str, suffix: &str) -> bool {
    host == suffix || host.ends_with(&format!(".{suffix}"))
}

/// `remoteMedia` entry: `cdn.example.com` or `*.example.com`, written lowercase.
///
/// The value is emitted verbatim into the sandbox CSP as `https://<entry>`, so the
/// charset stays strict (no scheme, port, path, quotes, spaces or semicolons).
/// `*.example.com` follows CSP semantics: subdomains only, not `example.com` itself.
pub fn validate_remote_media_host(value: &str, field: &str) -> Result<(), String> {
    let (wildcard, host) = match value.strip_prefix("*.") {
        Some(rest) => (true, rest),
        None => (false, value),
    };
    if host.is_empty() || host.len() > 253 {
        return Err(format!(
            "Tapp {field} must be a hostname of at most 253 characters"
        ));
    }
    let labels: Vec<&str> = host.split('.').collect();
    let label_ok = |label: &&str| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    };
    if !labels.iter().all(label_ok) {
        return Err(format!(
            "Tapp {field} must be a lowercase hostname (cdn.example.com) or *.example.com, without scheme, port or path; use punycode for IDN"
        ));
    }
    if labels.len() < 2 {
        return Err(format!(
            "Tapp {field} must be a public multi-label hostname"
        ));
    }
    // A numeric last label means an IPv4 literal (or a shorthand form of one).
    if labels
        .last()
        .is_some_and(|label| label.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(format!("Tapp {field} must not be an IP address"));
    }
    if REMOTE_MEDIA_RESERVED_SUFFIXES
        .iter()
        .any(|suffix| is_under_suffix(host, suffix))
    {
        return Err(format!("Tapp {field} must be a public hostname"));
    }
    if wildcard
        && REMOTE_MEDIA_WILDCARD_DENIED_SUFFIXES
            .iter()
            .any(|suffix| host == *suffix || suffix.ends_with(&format!(".{host}")))
    {
        return Err(format!(
            "Tapp {field} must not wildcard a shared hosting domain; list the exact host instead"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_url_accepts_http_and_https_only() {
        assert!(validate_http_url("https://example.com/x", "homepage").is_ok());
        assert!(validate_http_url("http://example.com/x", "homepage").is_ok());
        assert!(validate_http_url("ftp://example.com", "homepage").is_err());
        assert!(
            validate_http_url(&format!("https://x/{}", "a".repeat(3_000)), "homepage").is_err()
        );
    }

    #[test]
    fn open_url_target_https_or_loopback_http() {
        assert!(validate_open_url_target("https://docs.example.com/a", "openUrls[0].url").is_ok());
        assert!(validate_open_url_target("http://localhost:3000/x", "openUrls[0].url").is_ok());
        assert!(validate_open_url_target("http://example.com/x", "openUrls[0].url").is_err());
        assert!(
            validate_open_url_target("https://user:pass@example.com/", "openUrls[0].url").is_err()
        );
        assert!(validate_open_url_target("https://example.com/a b", "openUrls[0].url").is_err());
    }

    #[test]
    fn same_origin_open_url_accepts_rooted_paths_only() {
        for ok in [
            "/",
            "/journal",
            "/journal/notes?x=1",
            "/journal/notes?next=/a/../b",
            "/journal/%E7%AC%94%E8%AE%B0",
            "/journal/100%25",
            "/journal/v1..2",
            "/journal/a%2Fb",
        ] {
            assert!(
                validate_same_origin_open_url_path(ok, "openUrls[0].url").is_ok(),
                "{ok}"
            );
        }
        for bad in [
            "",
            "journal",
            "//evil.example",
            "https://example.com/journal",
            "/journal#frag",
            "/journal/../evil",
            "/journal/%2e%2e/",
            "/journal/.%2e/",
            "/journal/%2E%2E/config",
            "/journal/%2e%2e%2fconfig",
            "/journal%2f..%2fconfig",
            "/journal\\evil",
            "/journal/%5cconfig",
            "/journal/%00",
            "/journal/%0a",
            "/journal/%7f",
            "/journal/%",
            "/journal/%zz",
            "/journal/%ff",
            "/a b",
        ] {
            assert!(
                validate_same_origin_open_url_path(bad, "openUrls[0].url").is_err(),
                "{bad:?} should be rejected"
            );
        }
    }

    #[test]
    fn remote_media_host_accepts_exact_and_wildcard_public_hosts() {
        for ok in [
            "act-webstatic.mihoyo.com",
            "*.mihoyo.com",
            "*.miyoushe.com",
            "i0.hdslb.com",
            "xn--fiqs8s.example.org",
            "someone.github.io",
        ] {
            assert!(
                validate_remote_media_host(ok, "remoteMedia[0]").is_ok(),
                "{ok}"
            );
        }
    }

    #[test]
    fn remote_media_host_rejects_csp_injection_and_non_public_targets() {
        for bad in [
            "",
            "*.",
            "*",
            "*.com",
            "com",
            "https://cdn.example.com",
            "cdn.example.com/path",
            "cdn.example.com:443",
            "CDN.example.com",
            "cdn.example.com;",
            "cdn.example.com 'unsafe-inline'",
            "a.*.example.com",
            "**.example.com",
            "-cdn.example.com",
            "cdn..example.com",
            "127.0.0.1",
            "10.0.0.1",
            "[::1]",
            "localhost",
            "img.localhost",
            "nas.local",
            "svc.internal",
            "router.home.arpa",
            "*.github.io",
            "*.amazonaws.com",
            "*.s3.amazonaws.com",
            "*.workers.dev",
            "中文.example.com",
        ] {
            assert!(
                validate_remote_media_host(bad, "remoteMedia[0]").is_err(),
                "{bad:?} should be rejected"
            );
        }
    }

    #[test]
    fn url_module_does_not_name_reqwest() {
        let src = include_str!("urls.rs");
        let production = src.split("#[cfg(test)]").next().expect("source");
        assert!(
            production.contains("url::Url"),
            "tapp-contract URL validation must parse with url::Url"
        );
        assert!(
            !production.contains("reqwest::"),
            "tapp-contract URL validation must not import an HTTP client URL type"
        );
    }
}
