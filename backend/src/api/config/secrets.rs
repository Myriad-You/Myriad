//! Secret masking, credential persist rules, and URL sanitizers for config save/build.
use serde_json::Value;

/// Fixed-length secret mask shown to the UI (no real characters).
pub(crate) fn mask_secret_display_value() -> String {
    "••••••••".to_string()
}

/// True when the client re-submitted a masked secret (save must keep DB value).
pub(crate) fn is_masked_secret_value(value: &str) -> bool {
    let v = value.trim();
    if v.is_empty() {
        return false;
    }
    // Bullet and asterisk masks both count as masked.
    v.starts_with("••")
        || v.starts_with("**")
        || v == "********"
        || v == mask_secret_display_value()
        || (v.chars().all(|c| c == '•' || c == '*') && v.len() >= 4)
}

/// Form secret usable only when non-empty and not a mask.
pub(crate) fn form_secret_if_plaintext(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|s| !s.is_empty() && !is_masked_secret_value(s))
        .map(|s| s.to_string())
}

/// Persist a platform field into DB updates.
///
/// Semantics:
/// - masked (`••••` / `****…`) → skip (keep existing DB value)
/// - empty / whitespace → `null` (未配置). Do not persist `""`.
/// - non-empty plaintext → set new value
pub(crate) fn insert_platform_field(
    updates: &mut std::collections::HashMap<String, Value>,
    db_key: &str,
    value: &str,
) {
    if is_masked_secret_value(value) {
        return;
    }
    if value.trim().is_empty() {
        updates.insert(db_key.to_string(), Value::Null);
        return;
    }
    updates.insert(db_key.to_string(), Value::String(value.to_string()));
}

#[cfg(test)]
mod secret_persist_contract_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn masked_secret_keeps_db_value_empty_clears_to_null() {
        let mut updates = std::collections::HashMap::new();
        insert_platform_field(&mut updates, "github_token", "••••••••");
        assert!(updates.is_empty(), "mask must skip");
        insert_platform_field(&mut updates, "github_token", "   ");
        assert_eq!(updates.get("github_token"), Some(&Value::Null));
        insert_platform_field(&mut updates, "github_token", "plain-secret");
        assert_eq!(updates.get("github_token"), Some(&json!("plain-secret")));
        assert!(is_masked_secret_value("********"));
        assert!(!is_masked_secret_value(""));
    }
}

/// Extract a numeric playlist id from a bare id or a NetEase / QQ Music URL.
///
/// Config UI hints show full links (`?id=2884035`, `/playlist/8039305244`).
/// Unrecognized input is returned trimmed.
pub(crate) fn normalize_music_playlist_id(raw: &str) -> String {
    let s = raw.trim();
    if s.is_empty() {
        return String::new();
    }
    if s.chars().all(|c| c.is_ascii_digit()) {
        return s.to_string();
    }
    // NetEase / generic: id=123456
    if let Some(idx) = s.find("id=") {
        let rest = &s[idx + 3..];
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() {
            return digits;
        }
    }
    // QQ Music path: …/playlist/8039305244
    if let Some(idx) = s.find("/playlist/") {
        let rest = &s[idx + "/playlist/".len()..];
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() {
            return digits;
        }
    }
    // Fallback: longest consecutive digit run (len >= 5)
    let mut best = String::new();
    let mut cur = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() {
            cur.push(c);
        } else {
            if cur.len() > best.len() {
                best = cur.clone();
            }
            cur.clear();
        }
    }
    if cur.len() > best.len() {
        best = cur;
    }
    if best.len() >= 5 { best } else { s.to_string() }
}

/// Sanitize a wallpaper URL for persistence.
///
/// - Empty → empty (clear wallpaper)
/// - Absolute `http`/`https` only (no `data:`, `javascript:`, credentials)
/// - Same-origin path `/...` allowed
/// - Blocks loopback / private / link-local / special hostnames (visitor browsers
///   must not be pointed at intranet targets via public UI config)
///
/// Returns `None` when the value must not be stored as-is (caller should skip
/// the update rather than write a dangerous URL).
pub(crate) fn sanitize_wallpaper_url(raw: &str) -> Option<String> {
    let s = raw.trim();
    if s.is_empty() {
        return Some(String::new());
    }

    // Same-origin path only (single leading slash, not protocol-relative //)
    if s.starts_with('/') && !s.starts_with("//") {
        // Reject `/javascript:...` style smuggling
        if s.len() > 1 {
            let rest = &s[1..];
            if let Some(colon) = rest.find(':') {
                let scheme = &rest[..colon];
                if scheme
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '.' || c == '-')
                    && !scheme.is_empty()
                    && scheme
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_ascii_alphabetic())
                {
                    return None;
                }
            }
        }
        return Some(s.to_string());
    }

    let candidate = if let Some(rest) = s.strip_prefix("//") {
        format!("https://{rest}")
    } else {
        s.to_string()
    };

    let parsed = url::Url::parse(&candidate).ok()?;
    match parsed.scheme() {
        "http" | "https" => {}
        _ => return None,
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return None;
    }
    let host = parsed.host_str()?;
    if is_blocked_wallpaper_host(host) {
        return None;
    }
    Some(parsed.to_string())
}

fn is_blocked_wallpaper_host(host: &str) -> bool {
    let h = host
        .trim()
        .trim_matches(|c| c == '[' || c == ']')
        .to_ascii_lowercase();
    if h.is_empty() {
        return true;
    }
    if h == "localhost"
        || h == "0.0.0.0"
        || h == "::"
        || h == "::1"
        || h.ends_with(".localhost")
        || h.ends_with(".local")
        || h.ends_with(".internal")
        || h.ends_with(".arpa")
        || h.ends_with(".lan")
        || h.ends_with(".home")
        || h.ends_with(".corp")
    {
        return true;
    }

    if let Ok(ip) = h.parse::<std::net::IpAddr>() {
        return is_blocked_wallpaper_ip(ip);
    }
    false
}

fn is_blocked_wallpaper_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_documentation()
                // CGNAT 100.64.0.0/10
                || (v4.octets()[0] == 100 && (v4.octets()[1] & 0xc0) == 64)
                || v4.octets()[0] >= 224
        }
        std::net::IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return is_blocked_wallpaper_ip(std::net::IpAddr::V4(mapped));
            }
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_unique_local()
                || (v6.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

/// Same-origin path `/...` (not `//host`), rejecting `/javascript:...` smuggling.
fn sanitize_site_relative_path(s: &str) -> Option<String> {
    if !s.starts_with('/') || s.starts_with("//") {
        return None;
    }
    if s.len() > 1 {
        let rest = &s[1..];
        if let Some(colon) = rest.find(':') {
            let scheme = &rest[..colon];
            if !scheme.is_empty()
                && scheme
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_alphabetic())
                && scheme
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '.' || c == '-')
            {
                return None;
            }
        }
    }
    Some(s.to_string())
}

/// Favicon / logo: empty, same-origin path, http(s), or `data:image/*` (local upload).
/// Private hosts allowed (self-host LAN). No wallpaper-style host block.
pub(crate) fn sanitize_site_favicon_url(raw: &str) -> Option<String> {
    let s = raw.trim();
    if s.is_empty() {
        return Some(String::new());
    }
    if let Some(path) = sanitize_site_relative_path(s) {
        return Some(path);
    }
    // data:image/... only (uploaded favicon); reject data:text/html etc.
    if let Some(rest) = s.strip_prefix("data:") {
        let lower = rest.to_ascii_lowercase();
        if lower.starts_with("image/") {
            return Some(s.to_string());
        }
        return None;
    }
    sanitize_http_url_allow_private(s)
}

/// Google Search Console HTML-tag token. Empty clears. Accepts a bare token or a
/// pasted `<meta name="google-site-verification" content="...">`. Only
/// `[A-Za-z0-9_-]`, max 128 chars.
pub(crate) fn sanitize_google_site_verification(raw: &str) -> Option<String> {
    let s = raw.trim();
    if s.is_empty() {
        return Some(String::new());
    }
    let token = extract_google_site_verification_token(s);
    if token.is_empty() || token.len() > 128 {
        return None;
    }
    if !token
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return None;
    }
    Some(token)
}

fn extract_google_site_verification_token(s: &str) -> String {
    let lower = s.to_ascii_lowercase();
    let Some(idx) = lower.find("content=") else {
        return s.to_string();
    };
    let after = s[idx + "content=".len()..].trim_start();
    let Some(quote) = after.chars().next().filter(|c| *c == '"' || *c == '\'') else {
        return s.to_string();
    };
    let inner = &after[quote.len_utf8()..];
    match inner.find(quote) {
        Some(end) => inner[..end].trim().to_string(),
        None => s.to_string(),
    }
}

/// OG / share image: empty, path, http(s). No data: (crawlers need fetchable URLs).
pub(crate) fn sanitize_site_og_image_url(raw: &str) -> Option<String> {
    let s = raw.trim();
    if s.is_empty() {
        return Some(String::new());
    }
    if let Some(path) = sanitize_site_relative_path(s) {
        return Some(path);
    }
    sanitize_http_url_allow_private(s)
}

/// Tracker script URL (Umami): empty or http(s). Private hosts OK for self-host.
pub(crate) fn sanitize_umami_script_url(raw: &str) -> Option<String> {
    let s = raw.trim();
    if s.is_empty() {
        return Some(String::new());
    }
    sanitize_http_url_allow_private(s)
}

/// Outbound HTTP proxy: empty, or http(s)/socks* (LAN proxies are normal).
pub(crate) fn sanitize_proxy_url(raw: &str) -> Option<String> {
    let s = raw.trim();
    if s.is_empty() {
        return Some(String::new());
    }
    let parsed = url::Url::parse(s).ok()?;
    match parsed.scheme() {
        "http" | "https" | "socks5" | "socks5h" | "socks4" | "socks4a" => {}
        _ => return None,
    }
    parsed.host_str()?;
    Some(s.to_string())
}

/// API base URL (Gemini / GitHub / etc.): empty or http(s); private OK for reverse proxies.
///
/// Bare origins are stored **without** a trailing slash. `url::Url::to_string()`
/// normalizes `https://host` → `https://host/`; callers join paths onto the base,
/// so a trailing `/` would produce double slashes and break the UI persist tests.
pub(crate) fn sanitize_http_base_url(raw: &str) -> Option<String> {
    let s = raw.trim();
    if s.is_empty() {
        return Some(String::new());
    }
    let safe = sanitize_http_url_allow_private(s)?;
    Some(safe.trim_end_matches('/').to_string())
}

/// http(s) only; protocol-relative → https. **Does not** block private hosts.
fn sanitize_http_url_allow_private(raw: &str) -> Option<String> {
    let candidate = if let Some(rest) = raw.strip_prefix("//") {
        format!("https://{rest}")
    } else {
        raw.to_string()
    };
    let parsed = url::Url::parse(&candidate).ok()?;
    match parsed.scheme() {
        "http" | "https" => {}
        _ => return None,
    }
    parsed.host_str()?;
    Some(parsed.to_string())
}

/// Scheme/host policy of each URL-like setting, keyed by its `configurations`
/// key. The single source for saving the config bag and restoring a backup.
pub(crate) fn url_setting_sanitizer(db_key: &str) -> Option<fn(&str) -> Option<String>> {
    Some(match db_key {
        "ui_wallpaper_url" => sanitize_wallpaper_url,
        "site_favicon" => sanitize_site_favicon_url,
        "site_og_image" => sanitize_site_og_image_url,
        "google_site_verification" => sanitize_google_site_verification,
        "umami_script_url" => sanitize_umami_script_url,
        "proxy_url" => sanitize_proxy_url,
        "gemini_base_url" | "github_api_base_url" => sanitize_http_base_url,
        _ => return None,
    })
}

/// Apply a clearable URL field: empty writes empty; invalid skips the update.
pub(crate) fn insert_sanitized_clearable_url(
    updates: &mut std::collections::HashMap<String, serde_json::Value>,
    db_key: &str,
    raw: &str,
    sanitize: fn(&str) -> Option<String>,
) {
    match sanitize(raw) {
        Some(safe) => {
            updates.insert(db_key.to_string(), serde_json::Value::String(safe));
        }
        None => {
            tracing::warn!(
                key = db_key,
                value = %raw,
                "Rejecting config URL that failed scheme/format policy (keeping previous value)"
            );
        }
    }
}
