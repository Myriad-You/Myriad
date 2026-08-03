//! Pure image-proxy URL helpers for profile/export/library surfaces.
//!
//! Workspace crate: host allowlist is loaded from repo-root
//! `shared/image_proxy_hosts.json` (same file the frontend uses). HTTP handlers
//! must not reimplement this list.

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

/// 浏览器里**必须**走站内代理的 CDN（真·防盗链 / 无 Referer 就 403）。
///
/// 注意与 `api/proxy::is_allowed_domain` 不同：
/// - **needs_image_proxy**：自动改写 API 出口时用（窄；名单见 shared JSON）
/// - **is_allowed_domain**：`/api/proxy/image` egress allowlist（更宽的显式 host
///   exact/suffix 表；**禁止**任意公网 `.jpg` / 路径回退，见 MYR-007）
pub fn needs_image_proxy(url: &str) -> bool {
    let u = url.to_ascii_lowercase();
    let hosts = image_proxy_hosts();
    if hosts.markers.iter().any(|m| u.contains(&m.to_ascii_lowercase())) {
        return true;
    }
    if let Some(extra) = hosts.akamai_and_contains.as_deref() {
        if u.contains("akamaihd.net") && u.contains(&extra.to_ascii_lowercase()) {
            return true;
        }
    }
    false
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
        absolute
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
    use super::{needs_image_proxy, normalize_json_media_urls, proxy_image_url};
    use serde_json::json;

    #[test]
    fn shared_hosts_file_is_loaded() {
        assert!(needs_image_proxy("https://i0.hdslb.com/x.jpg"));
        assert!(needs_image_proxy(
            "https://steamcdn-a.akamaihd.net/steamcommunity/public/images/avatars/a.jpg"
        ));
        assert!(!needs_image_proxy("https://avatars.githubusercontent.com/u/1"));
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
        ] {
            assert_eq!(proxy_image_url(raw), raw, "{raw}");
            assert!(!needs_image_proxy(raw), "{raw}");
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
    fn recursive_normalize_rewrites_nested_covers() {
        let mut v = json!({
            "avatar": "https://i0.hdslb.com/bfs/face/a.jpg",
            "library_items": [
                { "cover": "https://cdn.cloudflare.steamstatic.com/steam/apps/1/header.jpg", "title": "G" }
            ],
            "name": "keep"
        });
        normalize_json_media_urls(&mut v);
        assert!(v["avatar"].as_str().unwrap().starts_with("/api/proxy/image"));
        assert!(v["library_items"][0]["cover"]
            .as_str()
            .unwrap()
            .starts_with("/api/proxy/image"));
        assert_eq!(v["name"], "keep");
    }
}

