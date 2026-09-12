//! Unit tests for analytics helpers.

use chrono::NaiveDate;
use serde_json::json;

use super::admin_api::{
    i64_nonneg, normalize_import_event_name, normalize_import_event_path, normalize_import_path,
    parse_day_str, valid_visitor_hash, vid_from_query,
};
use super::intake_helpers::*;

#[test]
fn normalizes_root_and_known() {
    assert_eq!(normalize_path("/").as_deref(), Some("/"));
    assert_eq!(normalize_path("/library").as_deref(), Some("/library"));
    assert_eq!(
        normalize_path("/tapp/abc123?x=1").as_deref(),
        Some("/tapp/:id")
    );
}

#[test]
fn rejects_empty() {
    assert!(normalize_path("").is_none());
}

#[test]
fn vid_and_event() {
    assert!(is_valid_vid("0123456789abcdef"));
    assert!(!is_valid_vid("x"));
    assert_eq!(
        normalize_event_name("Login_Success").as_deref(),
        Some("login_success")
    );
    assert!(normalize_event_name("!!!").is_none());
    assert!(normalize_event_name("a").is_none());
    assert!(
        normalize_event_name("__engage__").is_none(),
        "reserved internal names rejected"
    );
    assert!(normalize_event_name("__custom").is_none());
    assert_eq!(normalize_target(""), "");
    assert_eq!(normalize_target("  My-Tapp_01  "), "my-tapp_01");
    assert_eq!(normalize_target("weather@github.com"), "weather@github.com");
    assert!(normalize_target("!!!").is_empty());
}

#[test]
fn parses_vid_from_query() {
    let uri = |s: &str| s.parse::<axum::http::Uri>().unwrap();
    assert_eq!(
        vid_from_query(&uri("/api/analytics/visitor?vid=0123456789abcdef")).as_deref(),
        Some("0123456789abcdef")
    );
    // 位置无关，且不会被前缀相同的键骗到
    assert_eq!(
        vid_from_query(&uri("/x?days=7&vid=abcdefghijklmnop&z=1")).as_deref(),
        Some("abcdefghijklmnop")
    );
    assert_eq!(vid_from_query(&uri("/x?myvid=nope")), None);
    assert_eq!(vid_from_query(&uri("/x")), None);
    assert_eq!(vid_from_query(&uri("/x?vid")), None);
    // 取到的值仍要过 is_valid_vid（过短不通过；指纹回退不在本函数）
    assert!(
        !is_valid_vid(&vid_from_query(&uri("/x?vid=short")).unwrap()),
        "too-short vid must not pass validation"
    );
}

#[test]
fn visitor_hash_prefers_vid() {
    let a = resolve_visitor_hash(Some("0123456789abcdef01"), None, "Mozilla");
    let b = resolve_visitor_hash(
        Some("0123456789abcdef01"),
        Some("1.2.3.4".parse().unwrap()),
        "Other",
    );
    assert_eq!(a, b);
    assert!(
        a.is_some(),
        "dev/default salt path must still hash visitors"
    );
}

/// Pure salt resolver tests — no process env mutation (safe under parallel tests).
#[test]
fn analytics_salt_production_fails_closed_without_env_or_jwt() {
    assert_eq!(
        resolve_analytics_salt(None, true, None),
        Err(AnalyticsSaltUnavailable)
    );
    assert_eq!(
        resolve_analytics_salt(Some("   "), true, None),
        Err(AnalyticsSaltUnavailable)
    );
}

#[test]
fn analytics_salt_production_derives_from_jwt_when_salt_empty() {
    // Compose often ships ENVIRONMENT=production + JWT_SECRET + empty ANALYTICS_SALT.
    assert_eq!(
        resolve_analytics_salt(None, true, Some("jwt-secret-value")).as_deref(),
        Ok("myriad-analytics-prod|jwt-secret-value")
    );
    assert_eq!(
        resolve_analytics_salt(Some(""), true, Some("jwt-secret-value")).as_deref(),
        Ok("myriad-analytics-prod|jwt-secret-value")
    );
}

#[test]
fn analytics_salt_production_accepts_configured() {
    assert_eq!(
        resolve_analytics_salt(Some("prod-unique-salt"), true, None).as_deref(),
        Ok("prod-unique-salt")
    );
    assert_eq!(
        resolve_analytics_salt(Some("  trimmed  "), true, None).as_deref(),
        Ok("trimmed")
    );
}

#[test]
fn analytics_salt_dev_uses_default_or_jwt() {
    assert_eq!(
        resolve_analytics_salt(None, false, None).as_deref(),
        Ok("myriad-analytics-v1")
    );
    assert_eq!(
        resolve_analytics_salt(None, false, Some("dev-jwt-abc")).as_deref(),
        Ok("myriad-analytics-dev|dev-jwt-abc")
    );
    // Explicit salt always wins over JWT derivation.
    assert_eq!(
        resolve_analytics_salt(Some("explicit"), false, Some("dev-jwt-abc")).as_deref(),
        Ok("explicit")
    );
}

#[test]
fn bots_detected() {
    assert!(is_bot_ua("Googlebot/2.1"));
    assert!(!is_bot_ua(
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36"
    ));
}

/// Empty allowlist uses narrow default (loopback + docker0), not full
/// RFC1918. Public peers and arbitrary private nets still cannot forge.
#[test]
fn country_headers_empty_allowlist_private_vs_public_peer() {
    use axum::http::{HeaderMap, HeaderValue};

    let mut headers = HeaderMap::new();
    headers.insert("cf-ipcountry", HeaderValue::from_static("JP"));
    headers.insert("x-country-code", HeaderValue::from_static("US"));
    let docker0_peer: std::net::IpAddr = "172.17.0.2".parse().unwrap();
    let loopback_peer: std::net::IpAddr = "127.0.0.1".parse().unwrap();
    let rfc1918_peer: std::net::IpAddr = "10.0.0.2".parse().unwrap();
    let public_peer: std::net::IpAddr = "192.0.2.7".parse().unwrap();

    assert!(
        country_from_headers_with_trust(&headers, Some(docker0_peer), true, &[]).is_some(),
        "empty allowlist + docker0 peer should honor CDN country"
    );
    assert!(
        country_from_headers_with_trust(&headers, Some(loopback_peer), true, &[]).is_some(),
        "empty allowlist + loopback peer should honor CDN country"
    );
    assert!(
        country_from_headers_with_trust(&headers, Some(rfc1918_peer), true, &[]).is_none(),
        "empty allowlist + arbitrary RFC1918 peer must not honor CDN country"
    );
    assert!(
        country_from_headers_with_trust(&headers, Some(public_peer), true, &[]).is_none(),
        "empty allowlist + public peer must ignore forged CDN country"
    );
    assert!(
        country_from_headers_with_trust(&headers, Some(docker0_peer), false, &[]).is_none(),
        "trust disabled must ignore country headers"
    );
}

#[test]
fn country_headers_honored_only_for_trusted_proxy_peer() {
    use axum::http::{HeaderMap, HeaderValue};

    let mut headers = HeaderMap::new();
    headers.insert("cf-ipcountry", HeaderValue::from_static("JP"));
    let peer: std::net::IpAddr = "10.0.0.2".parse().unwrap();
    let allow: [ipnet::IpNet; 1] = ["10.0.0.0/8".parse().unwrap()];
    let outside: std::net::IpAddr = "192.0.2.7".parse().unwrap();

    assert!(
        country_from_headers_with_trust(&headers, Some(peer), true, &allow).is_some(),
        "trusted peer + allowlist must accept CDN country"
    );
    assert!(
        country_from_headers_with_trust(&headers, Some(outside), true, &allow).is_none(),
        "peer outside allowlist must ignore country headers"
    );
    assert!(
        country_from_headers_with_trust(&headers, None, true, &allow).is_none(),
        "missing peer must not trust country headers"
    );
}

#[test]
fn parse_day_str_accepts_iso_and_trims() {
    assert_eq!(
        parse_day_str("2026-07-30"),
        Some(NaiveDate::from_ymd_opt(2026, 7, 30).unwrap())
    );
    assert_eq!(
        parse_day_str("  2026-01-01  "),
        Some(NaiveDate::from_ymd_opt(2026, 1, 1).unwrap())
    );
    assert!(parse_day_str("").is_none());
    assert!(parse_day_str("2026/07/30").is_none());
    assert!(parse_day_str("not-a-date").is_none());
    assert!(parse_day_str("2026-13-01").is_none());
}

#[test]
fn valid_visitor_hash_is_hex_length_bounded() {
    assert!(valid_visitor_hash("0123456789abcdef")); // 16 hex
    assert!(valid_visitor_hash(&"ab".repeat(16))); // 32 hex (sha16 style)
    assert!(!valid_visitor_hash("short"));
    assert!(!valid_visitor_hash("not-hex-zzzzzzzz"));
    assert!(!valid_visitor_hash(""));
    assert!(!valid_visitor_hash(&"a".repeat(65)));
}

#[test]
fn i64_nonneg_accepts_json_numbers() {
    assert_eq!(i64_nonneg(Some(&json!(0))), Some(0));
    assert_eq!(i64_nonneg(Some(&json!(42))), Some(42));
    assert_eq!(i64_nonneg(Some(&json!(u64::MAX))), None); // doesn't fit i64
    assert!(i64_nonneg(Some(&json!(-1))).is_none());
    assert!(i64_nonneg(Some(&json!("1"))).is_none());
    assert!(i64_nonneg(None).is_none());
}

#[test]
fn import_path_and_event_helpers() {
    assert_eq!(normalize_import_path(SITE_PATH).as_deref(), Some(SITE_PATH));
    assert_eq!(
        normalize_import_path("/library").as_deref(),
        Some("/library")
    );
    assert!(normalize_import_path("relative").is_none());

    assert_eq!(
        normalize_import_event_name(ENGAGE_MARKER).as_deref(),
        Some(ENGAGE_MARKER)
    );
    assert_eq!(
        normalize_import_event_name("Login_OK").as_deref(),
        Some("login_ok")
    );
    assert!(normalize_import_event_name("!!").is_none());

    assert_eq!(normalize_import_event_path("").as_deref(), Some(""));
    assert_eq!(
        normalize_import_event_path("/tapp/xyz").as_deref(),
        Some("/tapp/:id")
    );
}

#[test]
fn backup_format_constants_stable() {
    assert_eq!(ANALYTICS_BACKUP_FORMAT, "myriad-analytics-backup");
    assert_eq!(ANALYTICS_BACKUP_VERSION, 1);
}
