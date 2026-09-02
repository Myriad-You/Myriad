//! HTTP(S) URL rules for manifest fields. Parsed with `url::Url`.

use crate::contract_rules::{HTTP_URL_SCHEMES, MAX_HTTP_URL_LEN};

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
