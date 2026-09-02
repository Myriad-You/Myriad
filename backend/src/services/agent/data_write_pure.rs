//! Pure validation and projection for agent data_write handlers.
//!
//! Handlers keep DB/FS/HTTP. Domain owns:
//! - feed name sanitization
//! - subscribe URL scheme/host/IP policy (SSRF string rules)
//! - update_interval clamp
//! - platform write item cap
//! - multi-feed prioritization for brew.subscribe

pub use myriad_agent_rules::{
    clamp_update_interval_minutes, collect_subscribe_url_candidates, extract_and_prioritize_feeds,
    feed_priority_score, is_disallowed_subscribe_host, is_disallowed_subscribe_ip,
    platform_write_cap_error, platform_write_items_over_cap, sanitize_feed_name,
    take_feed_urls_to_try, validate_subscribe_url_policy, MAX_FEED_NAME_LEN, MAX_FEED_URLS,
    MAX_PLATFORM_WRITE_ITEMS, MAX_UPDATE_INTERVAL, MIN_UPDATE_INTERVAL,
};

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

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
        assert_eq!(
            platform_write_cap_error(),
            "Too many items to write at once"
        );
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
