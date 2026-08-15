//! Pure validation and projection for agent data_write handlers.
//!
//! Handlers keep DB/FS/HTTP. Domain owns:
//! - feed name sanitization
//! - subscribe URL scheme/host/IP policy (SSRF string rules)
//! - update_interval clamp
//! - platform write item cap
//! - multi-feed prioritization for brew.subscribe

use serde_json::Value;
use std::cmp::Reverse;
use std::net::IpAddr;

/// 订阅源名称最大长度
pub const MAX_FEED_NAME_LEN: usize = 255;
/// 订阅 URL 最大尝试数
pub const MAX_FEED_URLS: usize = 10;
/// platform.write 单次最大写入条目数
pub const MAX_PLATFORM_WRITE_ITEMS: usize = 500;
/// update_interval 最小值（分钟）
pub const MIN_UPDATE_INTERVAL: i32 = 5;
/// update_interval 最大值（分钟）
pub const MAX_UPDATE_INTERVAL: i32 = 1440;

/// 清洗并验证用户提供的订阅源名称。
///
/// - 限制最大长度
/// - 去除首尾空白
/// - 拒绝纯空白字符串
pub fn sanitize_feed_name(name: &str) -> Result<String, String> {
    let trimmed: String = name.chars().take(MAX_FEED_NAME_LEN).collect();
    let trimmed = trimmed.trim().to_string();
    if trimmed.is_empty() {
        return Err("订阅源名称不能为空".to_string());
    }
    Ok(trimmed)
}

/// Clamp brew.subscribe update_interval minutes into contract bounds.
pub fn clamp_update_interval_minutes(raw: i32) -> i32 {
    raw.clamp(MIN_UPDATE_INTERVAL, MAX_UPDATE_INTERVAL)
}

/// Whether a platform.write items array exceeds the per-call cap.
pub fn platform_write_items_over_cap(count: usize) -> bool {
    count > MAX_PLATFORM_WRITE_ITEMS
}

pub fn platform_write_cap_error() -> String {
    format!("单次最多写入 {MAX_PLATFORM_WRITE_ITEMS} 条数据")
}

/// Disallowed hostname forms for subscribe URLs (before DNS).
pub fn is_disallowed_subscribe_host(host: &str) -> bool {
    host == "localhost" || host.ends_with(".local") || host.ends_with(".internal")
}

/// Whether a resolved IP must never be used as a subscribe target.
pub fn is_disallowed_subscribe_ip(ip: IpAddr) -> bool {
    if ip.is_loopback() || ip.is_unspecified() {
        return true;
    }
    match ip {
        IpAddr::V4(v4) => v4.is_private() || v4.is_link_local() || v4.octets()[0] == 169,
        IpAddr::V6(v6) => {
            // 阻止 IPv6 回环和链路本地
            v6.is_loopback() || (v6.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

/// Parse and apply pure URL policy for brew.subscribe (scheme + host string + IP literals).
///
/// Hostname DNS resolution remains in the handler (IO). When the host is an IP
/// literal it is checked here; hostname-only URLs pass if the host string is allowed.
pub fn validate_subscribe_url_policy(url: &str) -> Result<(), String> {
    let parsed = url::Url::parse(url).map_err(|_| format!("无效的 URL: {url}"))?;

    match parsed.scheme() {
        "http" | "https" => {}
        scheme => return Err(format!("不允许的 URL scheme: {scheme}")),
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| "URL 缺少 host".to_string())?;

    if is_disallowed_subscribe_host(host) {
        return Err("不允许访问内网地址".to_string());
    }

    // If host is already an IP literal, reject private ranges without DNS.
    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_disallowed_subscribe_ip(ip) {
            return match ip {
                IpAddr::V4(_) => Err("不允许访问内网地址".to_string()),
                IpAddr::V6(_) => Err("不允许访问内网 IPv6 地址".to_string()),
            };
        }
    }

    Ok(())
}

/// Score a candidate feed URL for brew.subscribe multi-source attempts.
///
/// Higher score is tried first: verified > official > HTTPS > known hosts; RSSHub demoted.
pub fn feed_priority_score(url: &str, verified: bool, source: &str) -> i32 {
    let mut score = 0;
    if verified {
        score += 100;
    }
    if source.contains("official") || url.contains("zhihu.com") {
        score += 50;
    }
    if url.starts_with("https://") {
        score += 20;
    }
    if url.contains("feedx.net") || url.contains("feedburner") {
        score += 30;
    }
    if url.contains("rsshub") {
        score -= 10;
    }
    score
}

/// Extract and prioritize feed URLs from a `feeds` JSON array.
///
/// Returns `(url, optional_name)` ordered by priority descending.
pub fn extract_and_prioritize_feeds(feeds: &Value) -> Vec<(String, Option<String>)> {
    let Some(feeds_arr) = feeds.as_array() else {
        return vec![];
    };

    let mut result: Vec<(String, Option<String>, i32)> = feeds_arr
        .iter()
        .filter_map(|f| {
            let url = f.get("url")?.as_str()?.to_string();
            let name = f
                .get("name")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let verified = f.get("verified").and_then(|v| v.as_bool()).unwrap_or(false);
            let source = f.get("source").and_then(|v| v.as_str()).unwrap_or("");
            let score = feed_priority_score(&url, verified, source);
            Some((url, name, score))
        })
        .collect();

    result.sort_by_key(|b| Reverse(b.2));
    result
        .into_iter()
        .map(|(url, name, _)| (url, name))
        .collect()
}

/// Collect subscribe candidate URLs from `feeds` or single `url` param.
pub fn collect_subscribe_url_candidates(
    feeds: Option<&Value>,
    single_url: Option<&str>,
) -> Result<Vec<(String, Option<String>)>, String> {
    let urls_to_try: Vec<(String, Option<String>)> = if let Some(feeds) = feeds {
        extract_and_prioritize_feeds(feeds)
    } else if let Some(url) = single_url {
        vec![(url.to_string(), None)]
    } else {
        return Err("缺少 url 或 feeds 参数".to_string());
    };

    if urls_to_try.is_empty() {
        return Err("没有可用的订阅源 URL".to_string());
    }
    Ok(urls_to_try)
}

/// Cap the number of URLs actually attempted.
pub fn take_feed_urls_to_try(urls: Vec<(String, Option<String>)>) -> Vec<(String, Option<String>)> {
    urls.into_iter().take(MAX_FEED_URLS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn sanitize_feed_name_trims_and_limits() {
        assert_eq!(sanitize_feed_name("  天利  ").unwrap(), "天利");
        assert!(sanitize_feed_name("   ").is_err());
        let long = "a".repeat(300);
        assert_eq!(
            sanitize_feed_name(&long).unwrap().chars().count(),
            MAX_FEED_NAME_LEN
        );
    }

    #[test]
    fn update_interval_and_platform_write_cap() {
        assert_eq!(clamp_update_interval_minutes(1), MIN_UPDATE_INTERVAL);
        assert_eq!(clamp_update_interval_minutes(9999), MAX_UPDATE_INTERVAL);
        assert_eq!(clamp_update_interval_minutes(30), 30);
        assert!(!platform_write_items_over_cap(10));
        assert!(platform_write_items_over_cap(MAX_PLATFORM_WRITE_ITEMS + 1));
        assert!(platform_write_cap_error().contains(&MAX_PLATFORM_WRITE_ITEMS.to_string()));
    }

    #[test]
    fn subscribe_url_policy_blocks_internal() {
        assert!(validate_subscribe_url_policy("https://example.com/rss.xml").is_ok());
        assert!(validate_subscribe_url_policy("ftp://example.com/x").is_err());
        assert!(validate_subscribe_url_policy("http://localhost/rss").is_err());
        assert!(validate_subscribe_url_policy("http://svc.local/rss").is_err());
        assert!(validate_subscribe_url_policy("http://192.168.0.1/rss").is_err());
        assert!(validate_subscribe_url_policy("http://127.0.0.1/rss").is_err());
        assert!(is_disallowed_subscribe_ip(IpAddr::V4(Ipv4Addr::new(
            10, 0, 0, 1
        ))));
        assert!(is_disallowed_subscribe_ip(IpAddr::V6(Ipv6Addr::LOCALHOST)));
        assert!(!is_disallowed_subscribe_ip(IpAddr::V4(Ipv4Addr::new(
            8, 8, 8, 8
        ))));
    }

    #[test]
    fn feed_priority_and_collect() {
        assert!(
            feed_priority_score("https://a.com", true, "official")
                > feed_priority_score("http://rsshub.app/x", false, "")
        );

        let feeds = json!([
            { "url": "http://rsshub.app/x", "verified": false, "source": "mirror" },
            { "url": "https://feedx.net/a.xml", "verified": true, "name": "A" },
            { "url": "https://zhihu.com/rss", "verified": false, "source": "official" }
        ]);
        let ranked = extract_and_prioritize_feeds(&feeds);
        assert_eq!(ranked[0].0, "https://feedx.net/a.xml");
        assert_eq!(ranked[0].1.as_deref(), Some("A"));

        let single = collect_subscribe_url_candidates(None, Some("https://ex.com/rss")).unwrap();
        assert_eq!(single.len(), 1);
        assert!(collect_subscribe_url_candidates(None, None).is_err());
        assert_eq!(
            take_feed_urls_to_try((0..20).map(|i| (format!("u{i}"), None)).collect()).len(),
            MAX_FEED_URLS
        );
    }
}
