//! Pure image-proxy URL helpers for profile/export/library surfaces.
//!
//! Workspace crate: host allowlist is loaded from repo-root
//! `shared/image_proxy_hosts.json` (same file the frontend uses). HTTP handlers
//! must not reimplement this list.
//!
//! ## Dual-path product model
//! - **Display**: only hosts on the must-proxy list are rewritten to
//!   `/api/proxy/image?url=…`; all other https URLs stay original.
//! - **Server proxy**: `/api/proxy/image` accepts **the same narrow list**
//!   (parsed host exact or proper DNS suffix only). No open `.jpg` / path fallback.

use serde_json::Value;

/// 与仓库根 `shared/image_proxy_hosts.json` 对齐（前后端共用一份名单）。
/// path: crates/myriad-image-proxy/src → ../../../ = repo root
const IMAGE_PROXY_HOSTS_JSON: &str = include_str!("../../../shared/image_proxy_hosts.json");

#[derive(Debug, serde::Deserialize)]
struct ImageProxyHostsFile {
    markers: Vec<String>,
    #[serde(default)]
    akamai_and_contains: Option<String>,
}

fn image_proxy_hosts() -> &'static ImageProxyHostsFile {
    use once_cell::sync::Lazy;
    static HOSTS: Lazy<ImageProxyHostsFile> = Lazy::new(|| {
        serde_json::from_str(IMAGE_PROXY_HOSTS_JSON)
            .expect("shared/image_proxy_hosts.json must be valid")
    });
    &HOSTS
}

/// Normalize a DNS host for allowlist matching: lowercase, strip trailing dot.
pub fn normalize_host(host: &str) -> String {
    host.trim_end_matches('.').to_ascii_lowercase()
}

/// Exact host match or proper DNS suffix (`i0.hdslb.com` matches `hdslb.com`;
/// lookalikes like `hdslb.com.evil.com` / `nothdslb.com` do not).
pub fn host_matches_domain(host: &str, domain: &str) -> bool {
    let host = normalize_host(host);
    let domain = normalize_host(domain);
    if domain.is_empty() || host.is_empty() {
        return false;
    }
    host == domain || host.ends_with(&format!(".{domain}"))
}

/// Parse `http`/`https` URL and return normalized host, or `None` if unusable.
pub fn parse_proxy_url_host(url: &str) -> Option<String> {
    let absolute = if let Some(rest) = url.strip_prefix("//") {
        format!("https://{rest}")
    } else if let Some(rest) = url.strip_prefix("http://") {
        format!("https://{rest}")
    } else {
        url.to_string()
    };
    let parsed = url::Url::parse(&absolute).ok()?;
    match parsed.scheme() {
        "http" | "https" => {}
        _ => return None,
    }
    let host = parsed.host_str()?.trim();
    if host.is_empty() {
        return None;
    }
    Some(normalize_host(host))
}

/// 浏览器里**必须**走站内代理的 CDN（真·防盗链 / 无 Referer 就 403）。
///
/// Same narrow list as `/api/proxy/image` egress allowlist (dual-path model).
/// Matching is **parsed host** exact/suffix only — never `url.contains`.
pub fn needs_image_proxy(url: &str) -> bool {
    let Some(host) = parse_proxy_url_host(url) else {
        return false;
    };
    let hosts = image_proxy_hosts();
    if hosts.markers.iter().any(|m| host_matches_domain(&host, m)) {
        return true;
    }
    // Steam legacy avatar CDN: host under akamaihd.net and label/path implies Steam.
    if let Some(extra) = hosts.akamai_and_contains.as_deref() {
        let extra = extra.to_ascii_lowercase();
        if host_matches_domain(&host, "akamaihd.net")
            && (host.contains(&extra) || url.to_ascii_lowercase().contains(&extra))
        {
            // Prefer host-label check; still allow path containing "steam" on akamaihd.net
            // only when host is a proper akamaihd.net suffix (already enforced).
            return true;
        }
    }
    false
}

/// Egress allowlist for `GET /api/proxy/image` — identical to [`needs_image_proxy`].
///
/// Kept as a named alias so call sites document the dual-path contract:
/// server may only fetch what the FE would rewrite through the proxy.
#[inline]
pub fn is_allowed_proxy_url(url: &str) -> bool {
    needs_image_proxy(url)
}

/// 已是代理路径（相对 `/api/proxy/image…` 或绝对 `https://host/api/proxy/image…`）
fn is_already_proxied(url: &str) -> bool {
    url.starts_with("/api/proxy/image")
        || url.contains("://")
            && url
                .split_once("://")
                .and_then(|(_, rest)| rest.find('/').map(|i| &rest[i..]))
                .is_some_and(|path| path.starts_with("/api/proxy/image"))
}

/// 将图片 URL 转为可在浏览器稳定显示的地址（需要时包一层 `/api/proxy/image`）。
pub fn proxy_image_url(url: &str) -> String {
    let url = url.trim();
    if url.is_empty() {
        return String::new();
    }
    if is_already_proxied(url) {
        return url.to_string();
    }
    // protocol-relative / http → https，避免混合内容
    let absolute = if let Some(rest) = url.strip_prefix("//") {
        format!("https://{rest}")
    } else if let Some(rest) = url.strip_prefix("http://") {
        format!("https://{rest}")
    } else {
        url.to_string()
    };
    if needs_image_proxy(&absolute) {
        format!("/api/proxy/image?url={}", urlencoding::encode(&absolute))
    } else {
        url.to_string()
    }
}

/// 递归改写 JSON 中所有字符串字段（`proxy_image_url` 对非热链恒等）。
/// 用于报告 `card_visuals` / library 等出口，避免各平台手写散点。
pub fn normalize_json_media_urls(value: &mut Value) {
    match value {
        Value::String(s) => {
            let next = proxy_image_url(s);
            if next != *s {
                *s = next;
            }
        }
        Value::Array(items) => {
            for item in items {
                normalize_json_media_urls(item);
            }
        }
        Value::Object(map) => {
            for (_k, v) in map.iter_mut() {
                normalize_json_media_urls(v);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod proxy_image_url_tests {
    use super::{
        host_matches_domain, is_allowed_proxy_url, needs_image_proxy, normalize_json_media_urls,
        proxy_image_url,
    };
    use serde_json::json;

    #[test]
    fn host_matches_domain_exact_and_suffix_only() {
        assert!(host_matches_domain("hdslb.com", "hdslb.com"));
        assert!(host_matches_domain("i0.hdslb.com", "hdslb.com"));
        assert!(host_matches_domain("I0.HDSLB.COM.", "hdslb.com"));
        assert!(host_matches_domain("p1.music.126.net", "music.126.net"));
        assert!(!host_matches_domain("hdslb.com.evil.com", "hdslb.com"));
        assert!(!host_matches_domain("nothdslb.com", "hdslb.com"));
        assert!(!host_matches_domain("evil-hdslb.com", "hdslb.com"));
        assert!(!host_matches_domain("com", "hdslb.com"));
    }

    #[test]
    fn shared_hosts_file_is_loaded() {
        assert!(needs_image_proxy("https://i0.hdslb.com/x.jpg"));
        assert!(needs_image_proxy(
            "https://steamcdn-a.akamaihd.net/steamcommunity/public/images/avatars/a.jpg"
        ));
        assert!(!needs_image_proxy(
            "https://avatars.githubusercontent.com/u/1"
        ));
    }

    #[test]
    fn rejects_lookalike_hosts() {
        assert!(!needs_image_proxy("https://hdslb.com.evil.com/face.jpg"));
        assert!(!needs_image_proxy("https://nothdslb.com/bfs/face/x.jpg"));
        assert!(!needs_image_proxy("https://evil.com/cdn?u=hdslb.com/x.jpg"));
        assert!(!is_allowed_proxy_url("https://evil.com/uploads/photo.jpg"));
        assert!(!is_allowed_proxy_url("https://blog.example/favicon.ico"));
    }

    #[test]
    fn proxies_true_hotlink_cdns() {
        let cases = [
            "https://i1.hdslb.com/bfs/face/abc.jpg",
            "https://avatars.steamstatic.com/xxx_full.jpg",
            "https://cdn.cloudflare.steamstatic.com/steam/apps/1/header.jpg",
            "http://steamcdn-a.akamaihd.net/steamcommunity/public/images/avatars/a.jpg",
            "https://p1.music.126.net/cover.jpg",
            "https://lain.bgm.tv/pic/cover/l/1.jpg",
            "https://pbs.twimg.com/profile_images/1/normal.jpg",
            "https://cdn.myanimelist.net/images/anime/1.jpg",
        ];
        for raw in cases {
            let out = proxy_image_url(raw);
            assert!(
                out.starts_with("/api/proxy/image?url="),
                "expected proxy for {raw}, got {out}"
            );
            assert!(needs_image_proxy(raw), "needs_image_proxy false for {raw}");
            assert!(
                is_allowed_proxy_url(raw),
                "allowlist must match needs_image_proxy for {raw}"
            );
        }
    }

    #[test]
    fn does_not_auto_proxy_healthy_cdns() {
        for raw in [
            "https://avatars.githubusercontent.com/u/1?v=4",
            "https://i.ytimg.com/vi/abc/hqdefault.jpg",
            "https://cdn.discordapp.com/avatars/1/2.png",
            "https://enka.network/ui/UI_AvatarIcon_Ayaka.png",
            "https://images-eds-ssl.xboxlive.com/image?url=x",
            "https://ui-avatars.com/api/?name=A",
            "https://221.ltd/favicon.ico",
            "https://www.google.com/s2/favicons?domain=example.com&sz=64",
        ] {
            assert_eq!(proxy_image_url(raw), raw, "{raw}");
            assert!(!needs_image_proxy(raw), "{raw}");
            assert!(!is_allowed_proxy_url(raw), "{raw}");
        }
        assert_eq!(proxy_image_url(""), "");
    }

    #[test]
    fn does_not_double_proxy() {
        let once = proxy_image_url("https://i0.hdslb.com/bfs/face/x.jpg");
        assert_eq!(proxy_image_url(&once), once);
        let absolute = format!("https://dash.example.com{once}");
        assert_eq!(proxy_image_url(&absolute), absolute);
    }

    #[test]
    fn upgrades_protocol_relative_and_http() {
        let out = proxy_image_url("//i0.hdslb.com/bfs/face/x.jpg");
        assert!(out.contains("url=https%3A%2F%2Fi0.hdslb.com"));
        let out2 = proxy_image_url("http://i0.hdslb.com/bfs/face/x.jpg");
        assert!(out2.contains("url=https%3A%2F%2Fi0.hdslb.com"));
    }

    #[test]
    fn does_not_rewrite_non_media_http_strings_to_https() {
        assert_eq!(
            proxy_image_url("http://example.com/not-media"),
            "http://example.com/not-media"
        );
        assert_eq!(proxy_image_url("//example.com/page"), "//example.com/page");
        let mut v = json!({
            "note": "http://example.com/x",
            "count": 1
        });
        normalize_json_media_urls(&mut v);
        assert_eq!(v["note"], "http://example.com/x");
        assert_eq!(v["count"], 1);
    }

    #[test]
    fn recursive_normalize_rewrites_nested_covers() {
        let mut v = json!({
            "avatar": "https://i0.hdslb.com/bfs/face/a.jpg",
            "library_items": [
                { "cover": "https://cdn.cloudflare.steamstatic.com/steam/apps/1/header.jpg", "title": "G" }
            ],
            "name": "keep"
        });
        normalize_json_media_urls(&mut v);
        assert!(
            v["avatar"]
                .as_str()
                .unwrap()
                .starts_with("/api/proxy/image")
        );
        assert!(
            v["library_items"][0]["cover"]
                .as_str()
                .unwrap()
                .starts_with("/api/proxy/image")
        );
        assert_eq!(v["name"], "keep");
    }
}
