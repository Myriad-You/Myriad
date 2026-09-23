use myriad_error::AppError;
// 图片代理服务 - 用于处理Bilibili等平台的防盗链图片
use axum::{
    Json,
    extract::{Path, Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use once_cell::sync::Lazy;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock};

use crate::services::http_client::MEDIA_FETCH_CLIENT;
use crate::state::AppState;
use crate::services::keyed_lock::KeyedLocks;

// 网易云 / 酷狗服务与共享缓存、限流
use crate::services::kugou_service::KugouService;
use crate::services::music_player_view::{self, PlayerMusicSource, PlayerPlaylist};
use crate::services::netease_service::{CacheEntry, MUSIC_CACHE, NeteaseService, RATE_LIMITER};

/// Music proxy 429 with Retry-After + JSON body for FE toast / axios interceptors.
fn player_playlist_response(view: std::sync::Arc<PlayerPlaylist>) -> Response {
    (
        StatusCode::OK,
        [(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")],
        Json(view.as_ref()),
    )
        .into_response()
}

fn music_rate_limited_response(context: &str) -> Response {
    const RETRY_AFTER_SECS: u64 = 60;
    tracing::warn!("Rate limit exceeded for {}", context);
    (
        StatusCode::TOO_MANY_REQUESTS,
        [(header::RETRY_AFTER, RETRY_AFTER_SECS.to_string())],
        Json(json!({
            "error": "Too many requests",
            "message": format!(
                "Rate limit exceeded. Please try again in {} seconds.",
                RETRY_AFTER_SECS
            ),
            "retry_after": RETRY_AFTER_SECS
        })),
    )
        .into_response()
}

// 简单的令牌桶限流器
struct TokenBucket {
    tokens: f64,
    last_refill: Instant,
    refill_rate: f64, // 每秒生成的令牌数
    capacity: f64,    // 桶容量
}

impl TokenBucket {
    fn new(rate: f64, capacity: f64) -> Self {
        Self {
            tokens: capacity,
            last_refill: Instant::now(),
            refill_rate: rate,
            capacity,
        }
    }

    fn try_acquire(&mut self) -> Option<Duration> {
        self.refill();
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            None // Success
        } else {
            let needed = 1.0 - self.tokens;
            let wait_secs = needed / self.refill_rate;
            Some(Duration::from_secs_f64(wait_secs))
        }
    }

    fn refill(&mut self) {
        let now = Instant::now();
        let duration = now.duration_since(self.last_refill).as_secs_f64();
        if duration > 0.0 {
            let new_tokens = duration * self.refill_rate;
            self.tokens = (self.tokens + new_tokens).min(self.capacity);
            self.last_refill = now;
        }
    }
}

/// 上游 JSON 元数据的响应体上限。
///
/// 这些都是歌单/歌词/地理位置之类的小 JSON。`resp.json()` 会无界缓冲，
/// 上游被劫持或故障时是一条内存放大路径。
const MAX_UPSTREAM_JSON_BYTES: usize = 2 * 1024 * 1024;

/// 限长读取并解析 JSON。
///
/// 语义与 `resp.json::<Value>()` 一致（成功给 Value，失败给 Err），
/// 只是先把体积封顶。
async fn read_limited_json(resp: reqwest::Response) -> Result<Value, String> {
    let bytes =
        crate::services::outbound_security::read_limited_body(resp, MAX_UPSTREAM_JSON_BYTES)
            .await?;
    serde_json::from_slice(&bytes).map_err(|error| {
        tracing::error!(%error, "invalid JSON from music/geo upstream");
        "Invalid JSON from upstream".to_string()
    })
}

// 全局代理限流器映射（域名 → 令牌桶）
static PROXY_LIMITERS: Lazy<Arc<Mutex<HashMap<String, TokenBucket>>>> =
    Lazy::new(|| Arc::new(Mutex::new(HashMap::new())));

struct ClientGeoCacheEntry {
    data: Value,
    cached_at: Instant,
}

static CLIENT_GEO_CACHE: Lazy<RwLock<HashMap<String, ClientGeoCacheEntry>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));
static CLIENT_GEO_LOCKS: Lazy<KeyedLocks> = Lazy::new(KeyedLocks::default);
const CLIENT_GEO_CACHE_TTL: Duration = Duration::from_secs(600);

fn client_geo_ok(data: Value) -> Response {
    (
        StatusCode::OK,
        [(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")],
        Json(data),
    )
        .into_response()
}

async fn client_geo_cache_get(ip: &str) -> Option<Value> {
    let cache = CLIENT_GEO_CACHE.read().await;
    let entry = cache.get(ip)?;
    (entry.cached_at.elapsed() < CLIENT_GEO_CACHE_TTL).then(|| entry.data.clone())
}

async fn client_geo_cache_set(ip: &str, data: Value) {
    let mut cache = CLIENT_GEO_CACHE.write().await;
    cache.retain(|_, entry| entry.cached_at.elapsed() < CLIENT_GEO_CACHE_TTL);
    let geo_cap = crate::services::memory_profile::max_geo_cache_entries();
    let geo_bytes_cap = crate::services::memory_profile::max_geo_cache_bytes();
    let entry_bytes = 512usize;
    while cache.len() >= geo_cap || cache.len().saturating_mul(entry_bytes) >= geo_bytes_cap {
        let Some(oldest) = cache
            .iter()
            .min_by_key(|(_, entry)| entry.cached_at)
            .map(|(key, _)| key.clone())
        else {
            break;
        };
        cache.remove(&oldest);
    }
    cache.insert(
        ip.to_string(),
        ClientGeoCacheEntry {
            data,
            cached_at: Instant::now(),
        },
    );
}

async fn cache_and_ok(ip: &str, data: Value) -> Response {
    client_geo_cache_set(ip, data.clone()).await;
    client_geo_ok(data)
}

/// Host-based rate-limit key (parsed host exact/suffix; no full-URL substring).
fn get_domain_key(url: &str) -> String {
    use myriad_image_proxy::{host_matches_domain, parse_proxy_url_host};
    let Some(host) = parse_proxy_url_host(url) else {
        return "other".to_string();
    };
    if host_matches_domain(&host, "hdslb.com") {
        return "hdslb.com".to_string();
    } else if host_matches_domain(&host, "bilibili.com") {
        return "bilibili.com".to_string();
    } else if host_matches_domain(&host, "steamstatic.com")
        || host_matches_domain(&host, "akamaihd.net")
    {
        return "steamstatic.com".to_string();
    } else if host_matches_domain(&host, "bgm.tv")
        || host_matches_domain(&host, "bangumi.tv")
        || host_matches_domain(&host, "chii.in")
    {
        return "bangumi".to_string();
    } else if host_matches_domain(&host, "126.net") || host_matches_domain(&host, "163.com") {
        return "netease".to_string();
    } else if host_matches_domain(&host, "y.gtimg.cn") {
        return "qqmusic".to_string();
    } else if host_matches_domain(&host, "myanimelist.net") {
        return "mal".to_string();
    } else if host_matches_domain(&host, "twimg.com") {
        return "x".to_string();
    }
    "other".to_string()
}

/// 等待获取代理许可
/// 如果获取成功返回 Ok(()), 超时返回 Err(())
async fn wait_for_proxy_permit(url: &str) -> Result<(), ()> {
    let domain = get_domain_key(url);
    let start = Instant::now();
    let timeout = Duration::from_secs(15); // 最多等待15秒

    loop {
        let wait_duration = {
            let mut limiters = PROXY_LIMITERS.lock().await;
            let bucket = limiters.entry(domain.clone()).or_insert_with(|| {
                // 针对不同域名设置不同的限流策略
                match domain.as_str() {
                    // B站图片 CDN：页面常一次拉多张头像/封面；过低会排队到 15s 超时 → 429
                    "hdslb.com" | "bilibili.com" => TokenBucket::new(12.0, 48.0),
                    // Steam 通常比较宽松
                    "steamstatic.com" => TokenBucket::new(20.0, 100.0),
                    // Bangumi 封面 CDN
                    "bangumi" => TokenBucket::new(12.0, 60.0),
                    // 网易云
                    "netease" => TokenBucket::new(12.0, 60.0),
                    // QQ 音乐封面 CDN
                    "qqmusic" => TokenBucket::new(12.0, 60.0),
                    // MyAnimeList CDN
                    "mal" => TokenBucket::new(12.0, 60.0),
                    // twimg（domain key "x"）等未单列的 allowlist 域名
                    _ => TokenBucket::new(12.0, 48.0),
                }
            });

            bucket.try_acquire()
        };

        match wait_duration {
            None => return Ok(()), // 获取成功
            Some(d) => {
                // 检查是否会超时
                if start.elapsed() + d > timeout {
                    return Err(());
                }
                // 等待所需的时间（加上一点点缓冲）
                tokio::time::sleep(d + Duration::from_millis(10)).await;
            }
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct ImageProxyQuery {
    url: String,
}

/// Classic 1×1 transparent PNG (70 bytes). Soft-fail placeholder so ordinary
/// `<img>` loads do not paint Network/console red on dead favicons.
/// Base64: iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==
const TRANSPARENT_1X1_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0xDA, 0x63, 0xFC, 0xCF, 0xC0, 0x50,
    0x0F, 0x00, 0x04, 0x85, 0x01, 0x80, 0x84, 0xA9, 0x8C, 0x21, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
];

/// Soft-fail for upstream/content failures after security checks have passed.
/// Returns HTTP 200 + tiny transparent PNG so browser `<img>` does not log 502/4xx.
fn soft_fail_placeholder(reason: &str, url: &str) -> Response {
    tracing::debug!(%url, %reason, "Image proxy soft-fail: transparent placeholder");
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "image/png".to_string()),
            // Short public cache: dead favicons are common; avoid long-lived bad entries
            // if the remote recovers, but still dampen guest-page hammering.
            (header::CACHE_CONTROL, "public, max-age=300".to_string()),
            (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*".to_string()),
            // Lets intentional clients distinguish placeholders; <img> ignores this.
            (
                header::HeaderName::from_static("x-image-proxy"),
                "placeholder".to_string(),
            ),
        ],
        TRANSPARENT_1X1_PNG,
    )
        .into_response()
}

/// 代理图片请求，添加必要的Referer头
///
/// # Auth policy (product decision)
/// Guest-facing avatars/covers stay **unauthenticated**. Mitigation = hotlink allowlist
/// (`needs_image_proxy` / `shared/image_proxy_hosts.json`) + per-IP
/// `IMAGE_PROXY_MAX` (400/min) + `IP_HARD_CAP_MAX` + SSRF guards。非 allowlist 域名硬 403。
///
/// Security rejections (SSRF / domain / unsafe target / rate limit / oversize URL / SVG)
/// still return hard 4xx. Body oversize or unreadable is 413. Send/status/non-image
/// failures soft-fail with a transparent 1×1 PNG (HTTP 200).
pub async fn proxy_image(Query(params): Query<ImageProxyQuery>) -> Response {
    let url = params.url;

    // URL 长度上限 2048。
    if url.len() > 2048 {
        tracing::warn!("🚨 Rejected proxy request: URL too long ({})", url.len());
        return (StatusCode::BAD_REQUEST, "URL too long").into_response();
    }

    // Dual-path: only the narrow must-proxy host list (precise host match).
    if !is_allowed_domain(&url) {
        tracing::warn!(
            "🚨 Rejected proxy request: Domain not whitelisted - {}",
            url
        );
        return (
            StatusCode::FORBIDDEN,
            "Only images from supported platforms are allowed",
        )
            .into_response();
    }

    // SSRF 防护：阻止请求内网地址
    if crate::federation::types::is_internal_url(&url) {
        tracing::warn!(
            "🚨 Rejected proxy request: SSRF attempt to internal URL - {}",
            url
        );
        return (StatusCode::FORBIDDEN, "Cannot proxy internal URLs").into_response();
    }

    // 限流保护：等待获取令牌
    if wait_for_proxy_permit(&url).await.is_err() {
        tracing::warn!("🚨 Proxy rate limit exceeded (timeout) for URL: {}", url);
        return (
            StatusCode::TOO_MANY_REQUESTS,
            "Rate limit exceeded, please try again later",
        )
            .into_response();
    }

    // Resolve and pin the public target, and never follow an unvalidated redirect.
    let (target_url, client) = match crate::services::outbound_security::build_public_http_client(
        &url,
        std::time::Duration::from_secs(10),
        Some("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"),
    )
    .await
    {
        Ok(target) => target,
        Err(error) => {
            tracing::warn!(%url, %error, "Rejected unsafe image proxy target");
            return (StatusCode::FORBIDDEN, "Cannot proxy unsafe URLs").into_response();
        }
    };

    // 根据域名设置适当的Referer
    let referer = get_referer_for_url(&url);

    // 发起请求 — upstream failures soft-fail (no 502/504 console noise for <img>)
    let response = match client
        .get(target_url)
        .header("Referer", referer)
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(e) => {
            let reason = if e.is_timeout() {
                "upstream timeout"
            } else if e.is_connect() {
                "upstream connect failed"
            } else {
                "upstream request failed"
            };
            tracing::debug!(%url, error = %e, %reason, "Image proxy upstream error");
            return soft_fail_placeholder(reason, &url);
        }
    };

    if !response.status().is_success() {
        tracing::debug!(
            %url,
            status = %response.status(),
            "Image proxy target returned non-success"
        );
        return soft_fail_placeholder("upstream non-success status", &url);
    }

    // 获取内容类型
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("image/jpeg")
        .to_string();

    // SVG is active content when served same-origin — hard reject (not soft 200).
    if is_disallowed_image_content_type(&content_type) {
        tracing::warn!(%url, %content_type, "🚨 Image proxy rejected SVG content-type");
        return (
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "SVG images are not allowed through the image proxy",
        )
            .into_response();
    }

    // 非 `image/*` → soft-fail 占位图，不是 400。
    if !content_type.starts_with("image/") {
        tracing::debug!(%url, %content_type, "Image proxy rejected non-image content");
        return soft_fail_placeholder("non-image content-type", &url);
    }

    // `read_limited_body` 逐块累加、一超限就中断（MAX_IMAGE_BYTES = 10 MiB）。
    const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;
    let image_data = match crate::services::outbound_security::read_limited_body(
        response,
        MAX_IMAGE_BYTES,
    )
    .await
    {
        Ok(data) => data,
        Err(e) => {
            tracing::warn!(%url, error = %e, "🚨 Image proxy body rejected");
            return (
                StatusCode::PAYLOAD_TOO_LARGE,
                "Image size exceeds the 10MB limit or could not be read",
            )
                .into_response();
        }
    };

    // 返回图片
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, content_type),
            // Cache-Control max-age=604800
            (
                header::CACHE_CONTROL,
                "public, max-age=604800, immutable".to_string(),
            ),
            (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*".to_string()),
        ],
        image_data,
    )
        .into_response()
}

/// True when Content-Type is SVG (active content if served same-origin).
///
/// Residual: only `Content-Type` is checked; mislabeled SVG bodies are not sniffed.
fn is_disallowed_image_content_type(content_type: &str) -> bool {
    let base = content_type
        .split(';')
        .next()
        .unwrap_or(content_type)
        .trim()
        .to_ascii_lowercase();
    base == "image/svg+xml" || base == "image/svg" || base.starts_with("image/svg+")
}

/// Dual-path egress allowlist: same `needs_image_proxy` host list
/// (`shared/image_proxy_hosts.json`)。host 精确/后缀；akamaihd.net 另可用 path 含 steam。
fn is_allowed_domain(url: &str) -> bool {
    myriad_image_proxy::is_allowed_proxy_url(url)
}

/// 根据URL获取适当的Referer（host-based, not URL substring）.
fn get_referer_for_url(url: &str) -> &'static str {
    use myriad_image_proxy::{host_matches_domain, parse_proxy_url_host};
    let Some(host) = parse_proxy_url_host(url) else {
        return "https://www.google.com/";
    };
    if host_matches_domain(&host, "hdslb.com") || host_matches_domain(&host, "bilibili.com") {
        "https://www.bilibili.com/"
    } else if host_matches_domain(&host, "steamstatic.com")
        || host_matches_domain(&host, "akamaihd.net")
    {
        "https://store.steampowered.com/"
    } else if host_matches_domain(&host, "bgm.tv")
        || host_matches_domain(&host, "bangumi.tv")
        || host_matches_domain(&host, "chii.in")
    {
        "https://bgm.tv/"
    } else if host_matches_domain(&host, "126.net") || host_matches_domain(&host, "163.com") {
        "https://music.163.com/"
    } else if host_matches_domain(&host, "y.gtimg.cn") {
        "https://y.qq.com/"
    } else if host_matches_domain(&host, "myanimelist.net") {
        "https://myanimelist.net/"
    } else if host_matches_domain(&host, "twimg.com") {
        "https://x.com/"
    } else {
        "https://www.google.com/"
    }
}

#[cfg(test)]
mod image_proxy_tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::http::StatusCode;

    #[test]
    fn transparent_png_has_valid_signature_and_size() {
        assert_eq!(TRANSPARENT_1X1_PNG.len(), 70);
        assert_eq!(&TRANSPARENT_1X1_PNG[0..8], b"\x89PNG\r\n\x1a\n");
        // IHDR width/height = 1
        assert_eq!(&TRANSPARENT_1X1_PNG[16..24], &[0, 0, 0, 1, 0, 0, 0, 1]);
    }

    #[tokio::test]
    async fn soft_fail_placeholder_returns_200_png_with_short_cache() {
        let response =
            soft_fail_placeholder("upstream connect failed", "https://example.com/favicon.ico");
        assert_eq!(response.status(), StatusCode::OK);

        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok());
        assert_eq!(content_type, Some("image/png"));

        let cache = response
            .headers()
            .get(header::CACHE_CONTROL)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default();
        assert!(cache.contains("public"));
        assert!(cache.contains("max-age=300"));

        let proxy_marker = response
            .headers()
            .get("x-image-proxy")
            .and_then(|v| v.to_str().ok());
        assert_eq!(proxy_marker, Some("placeholder"));

        let body = to_bytes(response.into_body(), 1024).await.expect("body");
        assert_eq!(body.as_ref(), TRANSPARENT_1X1_PNG);
    }

    #[test]
    fn allows_narrow_hotlink_hosts_only() {
        assert!(is_allowed_domain(
            "https://i0.hdslb.com/bfs/face/example.jpg"
        ));
        assert!(is_allowed_domain(
            "https://avatars.steamstatic.com/xxx_full.jpg"
        ));
        assert!(is_allowed_domain(
            "https://cdn.cloudflare.steamstatic.com/steam/apps/1/header.jpg"
        ));
        assert!(is_allowed_domain(
            "http://steamcdn-a.akamaihd.net/steamcommunity/public/images/avatars/a.jpg"
        ));
        assert!(is_allowed_domain("https://p1.music.126.net/cover.jpg"));
        assert!(is_allowed_domain(
            "https://y.gtimg.cn/music/photo_new/T002R300x300M000003H4b1P4V0990.jpg"
        ));
        assert!(!is_allowed_domain("https://gtimg.cn/x.jpg"));
        assert!(is_allowed_domain("https://lain.bgm.tv/pic/cover/l/1.jpg"));
        assert!(is_allowed_domain(
            "https://pbs.twimg.com/profile_images/1/normal.jpg"
        ));
        assert!(is_allowed_domain(
            "https://cdn.myanimelist.net/images/anime/1.jpg"
        ));
        // Dual-path: non-hotlink display via original URL — proxy must refuse
        assert!(!is_allowed_domain("https://221.ltd/favicon.ico"));
        assert!(!is_allowed_domain("https://blog.hanawa.me/favicon.ico"));
        assert!(!is_allowed_domain(
            "https://www.google.com/s2/favicons?domain=example.com&sz=64"
        ));
        assert!(!is_allowed_domain(
            "https://avatars.githubusercontent.com/u/1?v=4"
        ));
        assert!(!is_allowed_domain(
            "https://cdn.discordapp.com/avatars/1/2.png"
        ));
        assert!(!is_allowed_domain("ftp://evil.example/x.png"));
        assert!(!is_allowed_domain("https://evil.example/page.html"));
    }

    #[test]
    fn rejects_lookalike_hosts_and_open_path_fallback() {
        assert!(!is_allowed_domain("https://hdslb.com.evil.com/face.jpg"));
        assert!(!is_allowed_domain("https://nothdslb.com/bfs/face/x.jpg"));
        assert!(!is_allowed_domain("https://evil.com/cdn?u=hdslb.com/x.jpg"));
        assert!(!is_allowed_domain("https://evil.example/uploads/photo.jpg"));
        assert!(!is_allowed_domain("https://cdn.evil.com/images/a.png"));
        assert!(!is_allowed_domain("https://example.com/static/logo.webp"));
    }

    #[test]
    fn rejects_svg_content_types() {
        assert!(is_disallowed_image_content_type("image/svg+xml"));
        assert!(is_disallowed_image_content_type("image/svg"));
        assert!(is_disallowed_image_content_type(
            "image/svg+xml; charset=utf-8"
        ));
        assert!(is_disallowed_image_content_type("IMAGE/SVG+XML"));
        assert!(!is_disallowed_image_content_type("image/png"));
        assert!(!is_disallowed_image_content_type("image/jpeg"));
        assert!(!is_disallowed_image_content_type("image/webp"));
        assert!(!is_disallowed_image_content_type("text/html"));
    }

    #[test]
    fn domain_key_and_referer_use_host_not_url_substring() {
        assert_eq!(
            get_domain_key("https://i0.hdslb.com/bfs/face/x.jpg"),
            "hdslb.com"
        );
        assert_eq!(get_domain_key("https://hdslb.com.evil.com/x.jpg"), "other");
        assert_eq!(
            get_referer_for_url("https://i0.hdslb.com/x.jpg"),
            "https://www.bilibili.com/"
        );
        assert_eq!(
            get_referer_for_url("https://evil.com/?ref=hdslb.com"),
            "https://www.google.com/"
        );
    }

    #[test]
    fn soft_fail_does_not_weaken_security_rejections() {
        // Documented contract: security gates stay hard-fail; only covered
        // here via is_allowed_domain / length checks that proxy_image uses first.
        assert!(!is_allowed_domain("not-a-url"));
        assert!(!is_allowed_domain("https://example.com/api/data"));
        // 非 allowlist 的 .jpg 也不放行
        assert!(!is_allowed_domain("https://example.com/photo.jpg"));
        let long = format!("https://i0.hdslb.com/{}.png", "a".repeat(3000));
        assert!(long.len() > 2048);
    }
    #[test]
    fn play_url_http_cache_never_outlives_remaining_server_ttl() {
        for format in [None, Some("json")] {
            let response = respond_netease_play_url(
                "https://music.126.net/song",
                format,
                Duration::from_secs(12),
            );
            assert_eq!(
                response.headers()[header::CACHE_CONTROL],
                "private, max-age=12"
            );
            for remaining in [Duration::ZERO, Duration::from_millis(999)] {
                let response =
                    respond_netease_play_url("https://music.126.net/song", format, remaining);
                assert_eq!(
                    response.headers()[header::CACHE_CONTROL],
                    "private, no-store"
                );
            }
        }
    }

    #[tokio::test]
    async fn play_url_cache_hit_does_not_restart_browser_ttl() {
        let id = "8999911262";
        MUSIC_CACHE.write().await.insert(
            format!("netease_play_url:{id}"),
            CacheEntry {
                data: json!({"url": "https://music.126.net/test.mp3"}),
                expires_at: Instant::now() + Duration::from_secs(20),
            },
        );
        let response = proxy_netease_play_url(
            Path(id.into()),
            Query(NeteasePlayUrlQuery {
                format: Some("json".into()),
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let policy = response.headers()[header::CACHE_CONTROL].to_str().unwrap();
        let age: u64 = policy
            .strip_prefix("private, max-age=")
            .unwrap()
            .parse()
            .unwrap();
        assert!(age <= 20);
    }

    #[tokio::test]
    async fn respond_netease_play_url_json_and_redirect() {
        let cdn = "https://m801.music.126.net/song.mp3?sign=abc";
        let json_resp = respond_netease_play_url(cdn, Some("json"), Duration::from_secs(300));
        assert_eq!(json_resp.status(), StatusCode::OK);
        let body = to_bytes(json_resp.into_body(), 64 * 1024)
            .await
            .expect("body");
        let v: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(v["url"], cdn);

        let redir = respond_netease_play_url(cdn, None, Duration::from_secs(300));
        assert_eq!(redir.status(), StatusCode::FOUND);
        let loc = redir
            .headers()
            .get(header::LOCATION)
            .and_then(|h| h.to_str().ok())
            .unwrap_or_default();
        assert_eq!(loc, cdn);
        let cache = redir
            .headers()
            .get(header::CACHE_CONTROL)
            .and_then(|h| h.to_str().ok())
            .unwrap_or_default();
        assert!(
            cache.contains("max-age=60"),
            "302 must not long-cache CDN URLs"
        );
    }
}

/// 代理网易云歌单；并发 miss 共享一次加载，只缓存播放器瘦队列。
pub async fn proxy_netease_playlist(Path(playlist_id): Path<String>) -> Response {
    let playlist_id = match playlist_id.parse::<i64>() {
        Ok(id) if id > 0 => id,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(AppError::public_json("Invalid playlist ID")),
            )
                .into_response();
        }
    };
    let id = playlist_id.to_string();
    let result =
        music_player_view::load_player_playlist(PlayerMusicSource::Netease, &id, || async {
            let key = format!("playlist:{playlist_id}");
            if !RATE_LIMITER.write().await.check_rate_limit(&key) {
                return Err(music_player_view::PlayerPlaylistError::RateLimited);
            }
            NeteaseService::new()
                .fetch_player_playlist(playlist_id)
                .await
                .map_err(|error| {
                    tracing::error!(playlist_id, %error, "Failed to fetch Netease playlist");
                    music_player_view::PlayerPlaylistError::FetchFailed
                })
        })
        .await;
    player_playlist_result(result)
}

fn player_playlist_result(
    result: Result<std::sync::Arc<PlayerPlaylist>, music_player_view::PlayerPlaylistError>,
) -> Response {
    match result {
        Ok(view) => player_playlist_response(view),
        Err(music_player_view::PlayerPlaylistError::RateLimited) => {
            music_rate_limited_response("playlist")
        }
        Err(music_player_view::PlayerPlaylistError::FetchFailed) => (
            StatusCode::BAD_GATEWAY,
            Json(json!({"error": "Failed to fetch playlist", "code": "playlist_fetch_failed"})),
        )
            .into_response(),
    }
}

// 网易云：playlist / lyrics / song / audio 走 NeteaseService。

/// 代理网易云歌词。`NeteaseService::fetch_lyrics`。
pub async fn proxy_netease_lyrics(Path(song_id): Path<String>) -> Response {
    let song_id_i64 = match song_id.parse::<i64>() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(AppError::public_json("Invalid song ID")),
            )
                .into_response();
        }
    };

    let service = NeteaseService::new();
    match service.fetch_lyrics(song_id_i64).await {
        Ok(data) => (
            StatusCode::OK,
            [(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")],
            Json(data),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("Failed to fetch Netease lyrics {}: {}", song_id, e);
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({
                    "error": "Failed to fetch lyrics",
                    "code": "lyrics_fetch_failed"
                })),
            )
                .into_response()
        }
    }
}

/// 代理网易云逐字歌词。`NeteaseService::fetch_lyrics_verbatim`。
pub async fn proxy_netease_lyrics_verbatim(Path(song_id): Path<String>) -> Response {
    let song_id_i64 = match song_id.parse::<i64>() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(AppError::public_json("Invalid song ID")),
            )
                .into_response();
        }
    };

    let service = NeteaseService::new();
    match service.fetch_lyrics_verbatim(song_id_i64).await {
        Ok(data) => (
            StatusCode::OK,
            [(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")],
            Json(data),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("Failed to fetch Netease verbatim lyrics {}: {}", song_id, e);
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({
                    "error": "Failed to fetch verbatim lyrics",
                    "code": "lyrics_fetch_failed"
                })),
            )
                .into_response()
        }
    }
}

/// 酷狗逐字歌词查询参数
#[derive(Deserialize)]
pub struct KugouLyricsQuery {
    /// 搜索关键词，建议「歌名 歌手」
    pub keyword: String,
    /// 歌曲时长（毫秒），用于在候选中挑最接近的版本；缺省 0 表示不匹配
    #[serde(default)]
    pub duration: i64,
}

/// 代理酷狗逐字歌词（KRC）
/// 返回 { krc: "<解码后的 KRC 文本>" }，由前端 parseKrc 解析
pub async fn proxy_kugou_lyrics_verbatim(Query(q): Query<KugouLyricsQuery>) -> Response {
    if q.keyword.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json("keyword required")),
        )
            .into_response();
    }

    let service = KugouService::new();
    match service.fetch_verbatim_lyrics(&q.keyword, q.duration).await {
        Ok(krc) => (
            StatusCode::OK,
            [(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")],
            Json(json!({ "krc": krc })),
        )
            .into_response(),
        Err(e) => {
            tracing::debug!("KuGou verbatim lyrics miss for {}: {}", q.keyword, e);
            // 未命中不是错误：返回空 krc，前端据此回退逐行
            (
                StatusCode::OK,
                [(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")],
                Json(json!({ "krc": "" })),
            )
                .into_response()
        }
    }
}

/// 代理网易云单曲详情。`NeteaseService::fetch_song_detail`。
pub async fn proxy_netease_song(Path(song_id): Path<String>) -> Response {
    let song_id_i64 = match song_id.parse::<i64>() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(AppError::public_json("Invalid song ID")),
            )
                .into_response();
        }
    };

    let service = NeteaseService::new();
    match service.fetch_song_detail(song_id_i64).await {
        Ok(data) => (
            StatusCode::OK,
            [(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")],
            Json(data),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("Failed to fetch Netease song {}: {}", song_id, e);
            (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "error": "Failed to fetch song detail",
                    "code": "song_fetch_failed"
                })),
            )
                .into_response()
        }
    }
}

/// 解析网易云可播放 HTTPS CDN URL（不转发音频字节）
///
/// GET /api/proxy/music/netease/play-url/{id}
/// - 默认 302 到 HTTPS CDN：浏览器/音频元素直连网易，流量不经本机
/// - `?format=json` 返回 `{ "url": "https://..." }`，便于调试或后续客户端解析
///
/// 与 `/audio/{id}` 全量代理的区别：本接口只解析临时链并升级 http→https，
/// 解决 HTTPS 站点上 outer/url 跳到 HTTP CDN 被 Mixed Content 拦截的问题。
#[derive(Debug, Deserialize)]
pub struct NeteasePlayUrlQuery {
    /// `json` 时返回 JSON；其它/缺省时 302 重定向到 CDN
    pub format: Option<String>,
}

pub async fn proxy_netease_play_url(
    Path(song_id): Path<String>,
    Query(query): Query<NeteasePlayUrlQuery>,
) -> Response {
    let song_id_i64 = match song_id.parse::<i64>() {
        Ok(id) => id,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, "Invalid song ID").into_response();
        }
    };

    let cache_key = format!("netease_play_url:{}", song_id);
    {
        let mut limiter = RATE_LIMITER.write().await;
        if !limiter.check_rate_limit(&cache_key) {
            return music_rate_limited_response(&format!("Netease play-url: {}", song_id));
        }
    }

    // 短缓存解析结果，减轻网易侧压力；CDN 链本身有时效，缓存不宜过长
    {
        let mut cache = MUSIC_CACHE.write().await;
        if let Some(entry) = cache.get(&cache_key) {
            if entry.expires_at > Instant::now() {
                if let Some(url) = entry.data.get("url").and_then(|v| v.as_str()) {
                    return respond_netease_play_url(
                        url,
                        query.format.as_deref(),
                        entry.expires_at.saturating_duration_since(Instant::now()),
                    );
                }
            }
        }
    }

    let service = NeteaseService::new();
    match service.fetch_audio_url(song_id_i64).await {
        Ok(audio_url) => {
            if audio_url.cache_until > Instant::now() {
                let mut cache = MUSIC_CACHE.write().await;
                cache.insert(
                    cache_key,
                    CacheEntry {
                        data: json!({ "url": audio_url.url }),
                        expires_at: audio_url.cache_until,
                    },
                );
            }
            respond_netease_play_url(
                &audio_url.url,
                query.format.as_deref(),
                audio_url
                    .cache_until
                    .saturating_duration_since(Instant::now()),
            )
        }
        Err(e) => {
            tracing::error!("Failed to resolve Netease play URL for {}: {}", song_id, e);
            (
                StatusCode::NOT_FOUND,
                "Audio not available (copyright or geo-restriction)",
            )
                .into_response()
        }
    }
}

pub(crate) fn respond_netease_play_url(
    audio_url: &str,
    format: Option<&str>,
    remaining: Duration,
) -> Response {
    let max_age = remaining
        .as_secs()
        .min(if format == Some("json") { 300 } else { 60 });
    let cache_control = if max_age == 0 {
        "private, no-store".to_string()
    } else {
        format!("private, max-age={max_age}")
    };
    if format == Some("json") {
        return (
            StatusCode::OK,
            [
                (header::CACHE_CONTROL, cache_control),
                (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*".to_string()),
            ],
            Json(json!({ "url": audio_url })),
        )
            .into_response();
    }

    // 302：音频仍由浏览器直连网易 CDN（非本机拉流）
    // 勿长期缓存 302：CDN 签名 URL 会过期
    (
        StatusCode::FOUND,
        [
            (header::LOCATION, audio_url.to_string()),
            (header::CACHE_CONTROL, cache_control),
            (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*".to_string()),
        ],
    )
        .into_response()
}

/// 代理网易云音频流。`NeteaseService::fetch_audio_url`。
///
/// 海外或需绕过 CORS/防盗链时使用：本机拉取 CDN 再回传（流量经服务器）。
/// 国内 HTTPS 站点优先用 [`proxy_netease_play_url`] 直连 CDN。
pub async fn proxy_netease_audio(
    State(state): State<AppState>,
    Path(song_id): Path<String>,
) -> Response {
    if !super::music_switch::music_proxy_enabled(state.db()).await {
        return super::music_switch::music_proxy_disabled_response();
    }
    let song_id_i64 = match song_id.parse::<i64>() {
        Ok(id) => id,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, "Invalid song ID").into_response();
        }
    };

    // Align with QQ /audio: music-layer Retry-After (not only play-url)
    let cache_key = format!("netease_audio:{}", song_id);
    {
        let mut limiter = RATE_LIMITER.write().await;
        if !limiter.check_rate_limit(&cache_key) {
            return music_rate_limited_response(&format!("Netease audio: {}", song_id));
        }
    }

    let service = NeteaseService::new();
    match service.fetch_audio_url(song_id_i64).await {
        Ok(audio_url) => {
            // 获取实际音频流
            let client = MEDIA_FETCH_CLIENT.clone();

            match client
                .get(&audio_url.url)
                .header("Referer", "https://music.163.com/")
                .header("Range", "bytes=0-")
                .send()
                .await
            {
                Ok(audio_resp) => {
                    let content_type = audio_resp
                        .headers()
                        .get(reqwest::header::CONTENT_TYPE)
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("audio/mpeg")
                        .to_string();

                    match crate::services::outbound_security::read_limited_body(
                        audio_resp,
                        crate::services::memory_profile::max_audio_bytes(),
                    )
                    .await
                    {
                        Ok(audio_data) => (
                            StatusCode::OK,
                            [
                                (header::CONTENT_TYPE, content_type),
                                // Cache-Control max-age=86400
                                (header::CACHE_CONTROL, "public, max-age=86400".to_string()),
                                (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*".to_string()),
                                (header::ACCEPT_RANGES, "bytes".to_string()),
                            ],
                            audio_data,
                        )
                            .into_response(),
                        Err(e) => {
                            tracing::error!(
                                "Failed to read audio data for song {}: {}",
                                song_id,
                                e
                            );
                            (StatusCode::BAD_GATEWAY, "Failed to read audio data").into_response()
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to fetch audio stream for song {}: {}", song_id, e);
                    (StatusCode::BAD_GATEWAY, "Failed to fetch audio stream").into_response()
                }
            }
        }
        Err(e) => {
            tracing::error!("Failed to get audio URL for song {}: {}", song_id, e);
            (
                StatusCode::NOT_FOUND,
                "Audio not available (copyright or geo-restriction)",
            )
                .into_response()
        }
    }
}

// QQ音乐相关函数

/// 校验 QQ 音乐 songmid（字母数字，长度 8..=32）。
fn is_valid_qq_songmid(song_mid: &str) -> bool {
    let len = song_mid.len();
    (8..=32).contains(&len) && song_mid.chars().all(|c| c.is_ascii_alphanumeric())
}

/// QQ 取链失败的分类。四个文件名封装会各试一次，取走得最远的那一类作为结论。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum QqAudioFailure {
    /// 请求 GetEVkey 本身失败（网络、超时、响应不可解析）。
    Unreachable,
    /// GetEVkey `req_0.code != 0`：QQ 拒绝匿名取链（需登录态、仅放行加密格式或风控），与单曲无关。
    UpstreamDenied,
    /// `code == 0` 但 `purl` 为空：该曲目没有可播放授权（版权、VIP 或地区）。
    NoLicense,
    /// 拿到了 `purl`，CDN 却不给音频。
    CdnUnavailable,
}

impl QqAudioFailure {
    const ALL: [Self; 4] = [
        Self::Unreachable,
        Self::UpstreamDenied,
        Self::NoLicense,
        Self::CdnUnavailable,
    ];

    /// 返回给前端的机器码，前端据此选择本地化文案。
    fn code(self) -> &'static str {
        match self {
            Self::Unreachable => "upstream_unreachable",
            Self::UpstreamDenied => "upstream_denied",
            Self::NoLicense => "song_unavailable",
            Self::CdnUnavailable => "cdn_unavailable",
        }
    }

    fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.code() == code)
    }

    fn message(self) -> &'static str {
        match self {
            Self::Unreachable => "QQ Music API is unreachable",
            Self::UpstreamDenied => {
                "QQ Music refused to issue a play URL (login required or anonymous access restricted); this is not specific to the song"
            }
            Self::NoLicense => "Audio not available (copyright, VIP, or geo-restriction)",
            Self::CdnUnavailable => "QQ Music CDN did not serve the audio",
        }
    }

    fn status(self) -> StatusCode {
        match self {
            Self::NoLicense => StatusCode::NOT_FOUND,
            Self::Unreachable | Self::UpstreamDenied | Self::CdnUnavailable => {
                StatusCode::BAD_GATEWAY
            }
        }
    }
}

#[derive(Debug)]
struct QqResolveError {
    failure: QqAudioFailure,
    /// 各次尝试的上游返回摘要，只进日志，不进响应。
    attempts: Vec<String>,
}

impl QqResolveError {
    fn record(&mut self, failure: QqAudioFailure, attempt: String) {
        self.failure = self.failure.max(failure);
        self.attempts.push(attempt);
    }
}

/// QQ 返回的 `msg` 以服务器出口 IP 开头（如 `1.2.3.4;invalidq;`），日志里去掉它。
fn qq_vkey_msg(data: &Value) -> String {
    let msg = data
        .pointer("/req_0/data/msg")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    msg.split(';')
        .filter(|part| !part.is_empty() && part.parse::<std::net::IpAddr>().is_err())
        .collect::<Vec<_>>()
        .join(";")
}

/// 失败结论短时缓存，避免浏览器重试与前端探测反复打 QQ 接口。
const QQ_AUDIO_FAILURE_TTL: Duration = Duration::from_secs(2 * 60);

fn qq_audio_failure_cache_key(song_mid: &str) -> String {
    format!("qq_audio_fail:{}", song_mid)
}

fn qq_audio_failure_response(failure: QqAudioFailure) -> Response {
    (
        failure.status(),
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({
            "error": failure.code(),
            "message": failure.message(),
        })),
    )
        .into_response()
}

/// 带负缓存的取链：命中失败缓存直接返回，失败时写入缓存并按分类记日志。
async fn resolve_qq_audio_url_cached(song_mid: &str) -> Result<String, QqAudioFailure> {
    let fail_key = qq_audio_failure_cache_key(song_mid);
    {
        let mut cache = MUSIC_CACHE.write().await;
        if let Some(entry) = cache.get(&fail_key) {
            if entry.expires_at > Instant::now() {
                if let Some(failure) = entry
                    .data
                    .get("failure")
                    .and_then(|v| v.as_str())
                    .and_then(QqAudioFailure::from_code)
                {
                    return Err(failure);
                }
            }
        }
    }

    match resolve_qq_audio_url(song_mid).await {
        Ok(url) => Ok(url),
        Err(err) => {
            tracing::warn!(
                failure = err.failure.code(),
                "Failed to resolve QQ audio for {}: {}",
                song_mid,
                err.attempts.join(" | ")
            );
            let mut cache = MUSIC_CACHE.write().await;
            cache.insert(
                fail_key,
                CacheEntry {
                    data: json!({ "failure": err.failure.code() }),
                    expires_at: Instant::now() + QQ_AUDIO_FAILURE_TTL,
                },
            );
            Err(err.failure)
        }
    }
}

/// 通过 QQ 音乐 GetEVkey 接口解析可播放音频 URL
///
/// `music.vkey.GetEVkey` + `RS02{songmid}.mp3`（部分曲目需回退其它封装）。
async fn resolve_qq_audio_url(song_mid: &str) -> Result<String, QqResolveError> {
    let client = MEDIA_FETCH_CLIENT.clone();

    // 优先 RS02（海外匿名可拿到 purl），再回退常见清晰度封装
    let filenames = [
        format!("RS02{}.mp3", song_mid),
        format!("M500{}.mp3", song_mid),
        format!("C400{}.m4a", song_mid),
        format!("M800{}.mp3", song_mid),
    ];

    let guid = format!("{:010}", rand::random::<u32>() % 1_000_000_000);
    let mut error = QqResolveError {
        failure: QqAudioFailure::Unreachable,
        attempts: Vec::with_capacity(filenames.len()),
    };

    for filename in &filenames {
        let payload = json!({
            "comm": { "ct": 24, "cv": 0, "uin": "0", "format": "json" },
            "req_0": {
                "module": "music.vkey.GetEVkey",
                "method": "CgiGetEVkey",
                "param": {
                    "guid": guid,
                    "songmid": [song_mid],
                    "filename": [filename],
                    "songtype": [0],
                    "uin": "0",
                    "loginflag": 1,
                    "platform": "20"
                }
            }
        });

        let resp = match client
            .post("https://u.y.qq.com/cgi-bin/musicu.fcg")
            .header("Referer", "https://y.qq.com/")
            .header("Origin", "https://y.qq.com")
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                error.record(
                    QqAudioFailure::Unreachable,
                    format!("{}: request failed: {}", filename, e),
                );
                continue;
            }
        };

        let data: Value = match read_limited_json(resp).await {
            Ok(v) => v,
            Err(e) => {
                error.record(
                    QqAudioFailure::Unreachable,
                    format!("{}: parse failed: {}", filename, e),
                );
                continue;
            }
        };

        let req_code = data.pointer("/req_0/code").and_then(|v| v.as_i64());
        if req_code != Some(0) {
            error.record(
                QqAudioFailure::UpstreamDenied,
                format!(
                    "{}: req_0.code={:?} msg={:?}",
                    filename,
                    req_code,
                    qq_vkey_msg(&data)
                ),
            );
            continue;
        }

        let midinfo = match data.pointer("/req_0/data/midurlinfo/0") {
            Some(v) => v,
            None => {
                error.record(
                    QqAudioFailure::NoLicense,
                    format!("{}: empty midurlinfo", filename),
                );
                continue;
            }
        };
        let purl = midinfo
            .get("purl")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if purl.is_empty() {
            let result = midinfo.get("result").and_then(|v| v.as_i64());
            error.record(
                QqAudioFailure::NoLicense,
                format!(
                    "{}: empty purl result={:?} msg={:?}",
                    filename,
                    result,
                    qq_vkey_msg(&data)
                ),
            );
            continue;
        }

        let sip = data
            .pointer("/req_0/data/sip/0")
            .and_then(|v| v.as_str())
            .unwrap_or("https://dl.stream.qqmusic.qq.com/");

        // 浏览器页面若为 HTTPS，需避免 http:// CDN 混合内容拦截
        let sip = if sip.starts_with("http://") {
            sip.replacen("http://", "https://", 1)
        } else if sip.starts_with("https://") {
            sip.to_string()
        } else {
            format!("https://{}", sip.trim_start_matches("//"))
        };

        let audio_url = format!("{}{}", sip, purl);

        // 轻量探测：purl 有时 result=0 但 CDN 404
        match client
            .get(&audio_url)
            .header("Referer", "https://y.qq.com/")
            .header("Range", "bytes=0-1023")
            .send()
            .await
        {
            Ok(probe) if probe.status().is_success() => {
                return Ok(audio_url);
            }
            Ok(probe) => {
                error.record(
                    QqAudioFailure::CdnUnavailable,
                    format!("{}: CDN probe {}", filename, probe.status()),
                );
            }
            Err(e) => {
                error.record(
                    QqAudioFailure::CdnUnavailable,
                    format!("{}: CDN probe error: {}", filename, e),
                );
            }
        }
    }

    Err(error)
}

#[cfg(test)]
mod qq_audio_failure_tests {
    use super::*;

    #[test]
    fn furthest_stage_wins() {
        let mut err = QqResolveError {
            failure: QqAudioFailure::Unreachable,
            attempts: Vec::new(),
        };
        err.record(QqAudioFailure::NoLicense, "a".into());
        err.record(QqAudioFailure::UpstreamDenied, "b".into());
        assert_eq!(err.failure, QqAudioFailure::NoLicense);
        err.record(QqAudioFailure::CdnUnavailable, "c".into());
        assert_eq!(err.failure, QqAudioFailure::CdnUnavailable);
    }

    #[test]
    fn failure_codes_round_trip() {
        for failure in QqAudioFailure::ALL {
            assert_eq!(QqAudioFailure::from_code(failure.code()), Some(failure));
        }
        assert_eq!(QqAudioFailure::from_code("nope"), None);
    }

    #[test]
    fn vkey_msg_drops_egress_ip() {
        let data =
            json!({ "req_0": { "data": { "msg": "203.0.113.7;必须请求加密文件;invalidq;" } } });
        assert_eq!(qq_vkey_msg(&data), "必须请求加密文件;invalidq");
        let v6 = json!({ "req_0": { "data": { "msg": "2001:db8::1;invalidq;" } } });
        assert_eq!(qq_vkey_msg(&v6), "invalidq");
    }
}

#[derive(Debug, Deserialize)]
pub struct QqPlayUrlQuery {
    /// `json` → `{ "url": "..." }`；默认 302 Location 到 CDN
    pub format: Option<String>,
}

/// 仅解析 QQ 临时播放链（302 / JSON），音频字节仍由浏览器直连 CDN。
/// GET /api/proxy/music/qq/play-url/{songmid}
///
/// 国内 HTTPS 站点优先用本端点；海外或需 CORS/频谱分析时用 [`proxy_qq_audio`]。
pub async fn proxy_qq_play_url(
    Path(song_mid): Path<String>,
    Query(query): Query<QqPlayUrlQuery>,
) -> Response {
    if !is_valid_qq_songmid(&song_mid) {
        return (StatusCode::BAD_REQUEST, "Invalid QQ songmid").into_response();
    }

    let cache_key = format!("qq_play_url:{}", song_mid);
    {
        let mut limiter = RATE_LIMITER.write().await;
        if !limiter.check_rate_limit(&cache_key) {
            return music_rate_limited_response(&format!("QQ play-url: {}", song_mid));
        }
    }

    // 短缓存解析结果；CDN 签名链有时效
    {
        let mut cache = MUSIC_CACHE.write().await;
        if let Some(entry) = cache.get(&cache_key) {
            if entry.expires_at > Instant::now() {
                if let Some(url) = entry.data.get("url").and_then(|v| v.as_str()) {
                    return respond_qq_play_url(url, query.format.as_deref());
                }
            }
        }
    }

    match resolve_qq_audio_url_cached(&song_mid).await {
        Ok(audio_url) => {
            {
                let mut cache = MUSIC_CACHE.write().await;
                cache.insert(
                    cache_key,
                    CacheEntry {
                        data: json!({ "url": audio_url }),
                        expires_at: Instant::now() + Duration::from_secs(5 * 60),
                    },
                );
            }
            respond_qq_play_url(&audio_url, query.format.as_deref())
        }
        Err(failure) => qq_audio_failure_response(failure),
    }
}

pub(crate) fn respond_qq_play_url(audio_url: &str, format: Option<&str>) -> Response {
    if format == Some("json") {
        return (
            StatusCode::OK,
            [
                (header::CACHE_CONTROL, "private, max-age=300"),
                (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*"),
            ],
            Json(json!({ "url": audio_url })),
        )
            .into_response();
    }

    // 302：音频仍由浏览器直连 QQ CDN（非本机拉流）
    (
        StatusCode::FOUND,
        [
            (header::LOCATION, audio_url.to_string()),
            (header::CACHE_CONTROL, "private, max-age=60".to_string()),
            (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*".to_string()),
        ],
    )
        .into_response()
}

/// 代理 QQ 音乐音频流
/// GET /api/proxy/music/qq/audio/{songmid}
///
/// 解析临时 vkey 后拉取音频并回传（与网易云海外代理一致，便于 CORS / 频谱分析）。
/// 国内 HTTPS 站点优先用 [`proxy_qq_play_url`] 直连 CDN。
pub async fn proxy_qq_audio(
    State(state): State<AppState>,
    Path(song_mid): Path<String>,
) -> Response {
    if !super::music_switch::music_proxy_enabled(state.db()).await {
        return super::music_switch::music_proxy_disabled_response();
    }
    if !is_valid_qq_songmid(&song_mid) {
        return (StatusCode::BAD_REQUEST, "Invalid QQ songmid").into_response();
    }

    let cache_key = format!("qq_audio:{}", song_mid);
    {
        let mut limiter = RATE_LIMITER.write().await;
        if !limiter.check_rate_limit(&cache_key) {
            return music_rate_limited_response(&format!("QQ audio: {}", song_mid));
        }
    }

    let audio_url = match resolve_qq_audio_url_cached(&song_mid).await {
        Ok(url) => url,
        Err(failure) => return qq_audio_failure_response(failure),
    };

    let client = MEDIA_FETCH_CLIENT.clone();

    match client
        .get(&audio_url)
        .header("Referer", "https://y.qq.com/")
        .header("Range", "bytes=0-")
        .send()
        .await
    {
        Ok(audio_resp) => {
            if !audio_resp.status().is_success() {
                tracing::error!(
                    "QQ CDN returned {} for songmid {}",
                    audio_resp.status(),
                    song_mid
                );
                return qq_audio_failure_response(QqAudioFailure::CdnUnavailable);
            }

            let content_type = audio_resp
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("audio/mpeg")
                .to_string();

            match crate::services::outbound_security::read_limited_body(
                audio_resp,
                crate::services::memory_profile::max_audio_bytes(),
            )
            .await
            {
                Ok(audio_data) => (
                    StatusCode::OK,
                    [
                        (header::CONTENT_TYPE, content_type),
                        (header::CACHE_CONTROL, "public, max-age=3600".to_string()),
                        (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*".to_string()),
                        (header::ACCEPT_RANGES, "bytes".to_string()),
                    ],
                    audio_data,
                )
                    .into_response(),
                Err(e) => {
                    tracing::error!("Failed to read QQ audio data for {}: {}", song_mid, e);
                    (StatusCode::BAD_GATEWAY, "Failed to read audio data").into_response()
                }
            }
        }
        Err(e) => {
            tracing::error!("Failed to fetch QQ audio stream for {}: {}", song_mid, e);
            qq_audio_failure_response(QqAudioFailure::CdnUnavailable)
        }
    }
}

/// Public QQ playlist metadata, without a QQ login cookie.
fn qq_playlist_request(client: &reqwest::Client, playlist_id: &str) -> reqwest::RequestBuilder {
    // reqwest 0.13 gates RequestBuilder::query behind an optional feature.
    // Keep query encoding in the existing url dependency instead.
    let mut url = url::Url::parse("https://c.y.qq.com/v8/fcg-bin/fcg_v8_playlist_cp.fcg")
        .expect("static QQ playlist URL must be valid");
    url.query_pairs_mut().extend_pairs([
        ("id", playlist_id),
        ("format", "json"),
        ("newsong", "1"),
        ("platform", "jqspaframe.json"),
    ]);
    client.get(url).header("Referer", "http://y.qq.com")
}

/// 代理 QQ 歌单。只缓存播放器瘦视图；播放地址仍由歌曲 MID 按需解析。
pub async fn proxy_qq_playlist(Path(playlist_id): Path<String>) -> Response {
    let playlist_id = match playlist_id.parse::<u64>() {
        Ok(id) if id > 0 => id.to_string(),
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(AppError::public_json("Invalid playlist ID")),
            )
                .into_response();
        }
    };
    let result =
        music_player_view::load_player_playlist(PlayerMusicSource::Qq, &playlist_id, || async {
            let key = format!("qq_playlist:{playlist_id}");
            if !RATE_LIMITER.write().await.check_rate_limit(&key) {
                return Err(music_player_view::PlayerPlaylistError::RateLimited);
            }
            let fetch = async {
                let response = qq_playlist_request(&MEDIA_FETCH_CLIENT, &playlist_id)
                    .send()
                    .await
                    .map_err(|error| error.to_string())?
                    .error_for_status()
                    .map_err(|error| error.to_string())?;
                let data = read_limited_json(response).await?;
                music_player_view::project_qq_player_playlist(&playlist_id, &data)
                    .map_err(|_| "Invalid QQ playlist response".to_string())
            }
            .await;
            fetch.map_err(|error| {
                tracing::error!(%error, "Failed to fetch QQ playlist");
                music_player_view::PlayerPlaylistError::FetchFailed
            })
        })
        .await;
    player_playlist_result(result)
}

/// 获取客户端真实 IP 地理位置信息
/// GET /api/proxy/client-geo
///
/// 这个端点会自动处理以下情况：
/// 1. 从请求头中提取客户端真实IP（支持反向代理）
/// 2. 如果是本地/内网IP，则查询服务器的公网IP位置
/// 3. 使用可靠的地理位置API获取坐标信息
pub async fn get_client_geo(
    headers: axum::http::HeaderMap,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
) -> Response {
    let client_ip = crate::middleware::client_ip::client_ip_from_parts(
        &headers,
        Some(addr.ip()),
        crate::middleware::client_ip::trusted_proxy_headers_enabled(),
    )
    .map(|ip| ip.to_string())
    .unwrap_or_else(|| addr.ip().to_string());

    tracing::info!(
        "Client IP detection: resolved={}, socket={}, x_real_ip={:?}, xff={:?}",
        client_ip,
        addr.ip(),
        headers.get("x-real-ip").and_then(|v| v.to_str().ok()),
        headers.get("x-forwarded-for").and_then(|v| v.to_str().ok())
    );

    let lock = CLIENT_GEO_LOCKS.get(&client_ip);
    let _guard = lock.lock().await;
    if let Some(cached) = client_geo_cache_get(&client_ip).await {
        tracing::debug!(ip = %client_ip, "client-geo cache hit");
        return client_geo_ok(cached);
    }

    // Private / loopback / unparseable → cannot geo-locate the visitor; fall
    // back to server egress only as a last resort (local dev / misconfigured
    // TRUST_PROXY_*). FE treats `source=server-egress` as soft failure.
    let is_local_ip = crate::middleware::client_ip::is_private_or_local_str(&client_ip);

    let client = MEDIA_FETCH_CLIENT.clone();

    // 如果是本地IP，需要获取服务器的公网IP，然后查询位置
    let used_server_egress = is_local_ip;
    let target_ip = if is_local_ip {
        tracing::warn!(
            "Detected local/private client IP {} (socket={}); proxy X-Real-IP may be missing or \
             TRUST_PROXY_HEADERS/TRUST_PROXY_PEERS misconfigured. Falling back to server egress IP.",
            client_ip,
            addr.ip()
        );

        // 方案1: 使用 ipify.org 获取服务器公网IP
        match client.get("https://api.ipify.org?format=json").send().await {
            Ok(resp) => {
                if let Ok(data) = read_limited_json(resp).await {
                    if let Some(ip) = data.get("ip").and_then(|v| v.as_str()) {
                        tracing::info!("Server public IP from ipify: {}", ip);
                        ip.to_string()
                    } else {
                        client_ip.clone()
                    }
                } else {
                    client_ip.clone()
                }
            }
            _ => {
                // 方案2: 使用 icanhazip.com
                match client.get("https://icanhazip.com").send().await {
                    Ok(resp) => {
                        // 期望的响应就是一行 IP。限到 1 KiB：这个端点被劫持或故障时
                        // 不该能把任意大小的响应读进内存。
                        if let Ok(bytes) =
                            crate::services::outbound_security::read_limited_body(resp, 1024).await
                        {
                            let text = String::from_utf8_lossy(&bytes);
                            let ip = text.trim().to_string();
                            tracing::info!("Server public IP from icanhazip: {}", ip);
                            ip
                        } else {
                            client_ip.clone()
                        }
                    }
                    _ => client_ip.clone(),
                }
            }
        }
    } else {
        client_ip.clone()
    };

    tracing::info!("Querying geolocation for IP: {}", target_ip);

    // 尝试多个地理位置服务，提高成功率

    // 方案1: ip-api.com (免费，稳定，无需key)
    // ip-api `fields` 含 countryCode；后续备用源不一定带这个键。
    let url1 = format!(
        "http://ip-api.com/json/{}?fields=status,lat,lon,city,country,countryCode,regionName",
        target_ip
    );

    if let Ok(resp) = client.get(&url1).send().await {
        if let Ok(mut data) = read_limited_json(resp).await {
            if data.get("status").and_then(|s| s.as_str()) == Some("success") {
                tracing::info!(
                    "Geolocation success via ip-api.com for IP {}: city={}, region={}, country={} ({})",
                    target_ip,
                    data.get("city")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown"),
                    data.get("regionName")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown"),
                    data.get("country")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown"),
                    data.get("countryCode")
                        .and_then(|v| v.as_str())
                        .unwrap_or("??")
                );
                // Include resolved lookup IP for weather/geo debugging.
                if let Some(obj) = data.as_object_mut() {
                    obj.insert("ip".to_string(), json!(target_ip));
                    obj.insert("detected_client_ip".to_string(), json!(client_ip));
                    // Alias snake_case for clients that prefer country_code.
                    if let Some(cc) = obj.get("countryCode").cloned() {
                        obj.insert("country_code".to_string(), cc);
                    }
                    if used_server_egress {
                        // Soft-fail marker: visitor IP was private; location is
                        // the server egress, not the user. FE should try browser
                        // IP services next.
                        obj.insert("source".to_string(), json!("server-egress"));
                        obj.insert("fallback".to_string(), json!("server-public-ip"));
                    } else {
                        obj.insert("source".to_string(), json!("client-ip"));
                    }
                }
                return cache_and_ok(&client_ip, data).await;
            }
        }
    }

    // 方案2: ipapi.co (备用)
    let url2 = format!("https://ipapi.co/{}/json/", target_ip);

    if let Ok(resp) = client.get(&url2).send().await {
        if let Ok(data) = read_limited_json(resp).await {
            if let (Some(lat), Some(lon)) = (
                data.get("latitude").and_then(|v| v.as_f64()),
                data.get("longitude").and_then(|v| v.as_f64()),
            ) {
                let country_code = data
                    .get("country_code")
                    .or_else(|| data.get("country"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                // 转换为统一格式
                let mut unified_data = json!({
                    "status": "success",
                    "lat": lat,
                    "lon": lon,
                    "city": data.get("city").and_then(|v| v.as_str()).unwrap_or(""),
                    "country": data.get("country_name").and_then(|v| v.as_str()).unwrap_or(""),
                    "countryCode": country_code,
                    "country_code": country_code,
                    "regionName": data.get("region").and_then(|v| v.as_str()).unwrap_or(""),
                    "ip": target_ip,
                    "detected_client_ip": client_ip,
                });
                if let Some(obj) = unified_data.as_object_mut() {
                    if used_server_egress {
                        obj.insert("source".to_string(), json!("server-egress"));
                        obj.insert("fallback".to_string(), json!("server-public-ip"));
                    } else {
                        obj.insert("source".to_string(), json!("client-ip"));
                    }
                }

                tracing::info!(
                    "Geolocation success via ipapi.co for IP {}: city={}, country={}",
                    target_ip,
                    data.get("city")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown"),
                    data.get("country_name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                );

                return cache_and_ok(&client_ip, unified_data).await;
            }
        }
    }

    // 方案3: geojs.io (第三备用)
    let url3 = format!("https://get.geojs.io/v1/ip/geo/{}.json", target_ip);

    if let Ok(resp) = client.get(&url3).send().await {
        if let Ok(data) = read_limited_json(resp).await {
            if let (Some(lat_str), Some(lon_str)) = (
                data.get("latitude").and_then(|v| v.as_str()),
                data.get("longitude").and_then(|v| v.as_str()),
            ) {
                if let (Ok(lat), Ok(lon)) = (lat_str.parse::<f64>(), lon_str.parse::<f64>()) {
                    // 转换为统一格式
                    let mut unified_data = json!({
                        "status": "success",
                        "lat": lat,
                        "lon": lon,
                        "city": data.get("city").and_then(|v| v.as_str()).unwrap_or(""),
                        "country": data.get("country").and_then(|v| v.as_str()).unwrap_or(""),
                        "regionName": data.get("region").and_then(|v| v.as_str()).unwrap_or(""),
                        "ip": target_ip,
                        "detected_client_ip": client_ip,
                    });
                    if let Some(obj) = unified_data.as_object_mut() {
                        if used_server_egress {
                            obj.insert("source".to_string(), json!("server-egress"));
                            obj.insert("fallback".to_string(), json!("server-public-ip"));
                        } else {
                            obj.insert("source".to_string(), json!("client-ip"));
                        }
                    }

                    tracing::info!(
                        "Geolocation success via geojs.io for IP {}: city={}, country={}",
                        target_ip,
                        data.get("city")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown"),
                        data.get("country")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown")
                    );

                    return cache_and_ok(&client_ip, unified_data).await;
                }
            }
        }
    }

    // 私网/回环/无法解析：各 geo 源都失败后回 lat/lon 0，source=server-egress
    if is_local_ip {
        tracing::warn!("All geolocation services failed for local IP, using default fallback");
        let fallback_data = json!({
            "status": "success",
            "lat": 0.0,
            "lon": 0.0,
            "city": "Localhost",
            "country": "Development",
            "regionName": "Local",
            "ip": client_ip,
            "detected_client_ip": client_ip,
            "source": "server-egress",
            "fallback": "server-public-ip",
        });
        return cache_and_ok(&client_ip, fallback_data).await;
    }

    // 所有方案都失败，返回错误
    tracing::error!("All geolocation services failed for IP: {}", target_ip);
    (
        StatusCode::SERVICE_UNAVAILABLE,
        "All geolocation services failed",
    )
        .into_response()
}

/// Unescape common HTML entities in QQ lyric text (nobase64=1 still entity-escapes).
fn unescape_qq_lyric_text(s: &str) -> String {
    s.replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&quot;", "\"")
        .replace("&#34;", "\"")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&#10;", "\n")
        .replace("&#13;", "\r")
}

/// Normalize QQ lyrics JSON: validate retcode, unescape lyric/trans, stable shape for FE.
fn normalize_qq_lyrics_payload(raw: &Value) -> Result<Value, String> {
    let retcode = raw
        .get("retcode")
        .or_else(|| raw.get("code"))
        .and_then(|v| v.as_i64())
        .unwrap_or(-1);
    if retcode != 0 {
        return Err(format!("QQ lyrics retcode={}", retcode));
    }

    let lyric_raw = raw
        .get("lyric")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    let trans_raw = raw
        .get("trans")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();

    let lyric = if lyric_raw.is_empty() {
        String::new()
    } else {
        unescape_qq_lyric_text(lyric_raw)
    };
    let trans = if trans_raw.is_empty() {
        String::new()
    } else {
        unescape_qq_lyric_text(trans_raw)
    };

    if lyric.is_empty() {
        return Err("QQ lyrics empty after normalize".to_string());
    }

    Ok(json!({
        "retcode": 0,
        "lyric": lyric,
        "trans": trans,
        "code": 0,
    }))
}

/// 代理QQ音乐歌词请求（带缓存）
///
/// 规范化：校验 retcode、HTML 实体 unescape、暴露 trans 翻译层。
pub async fn proxy_qq_lyrics(Path(song_mid): Path<String>) -> Response {
    let cache_key = format!("qq_lyrics_v2:{}", song_mid);

    // 检查限流
    {
        let mut limiter = RATE_LIMITER.write().await;
        if !limiter.check_rate_limit(&cache_key) {
            return music_rate_limited_response(&format!("QQ lyrics: {}", song_mid));
        }
    }

    // 检查缓存（歌词缓存24小时）
    {
        let mut cache = MUSIC_CACHE.write().await;
        if let Some(entry) = cache.get(&cache_key) {
            if entry.expires_at > Instant::now() {
                tracing::debug!("Cache hit for QQ lyrics: {}", song_mid);
                return (
                    StatusCode::OK,
                    [(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")],
                    Json(entry.data.clone()),
                )
                    .into_response();
            }
        }
    }

    let client = MEDIA_FETCH_CLIENT.clone();

    let url = format!(
        "https://c.y.qq.com/lyric/fcgi-bin/fcg_query_lyric_new.fcg?songmid={}&g_tk=5381&format=json&inCharset=utf8&outCharset=utf-8&nobase64=1",
        song_mid
    );

    match client
        .get(&url)
        .header("Referer", "https://y.qq.com/")
        .header("Origin", "https://y.qq.com")
        .send()
        .await
    {
        Ok(resp) => match read_limited_json(resp).await {
            Ok(raw) => match normalize_qq_lyrics_payload(&raw) {
                Ok(data) => {
                    {
                        let mut cache = MUSIC_CACHE.write().await;
                        cache.insert(
                            cache_key,
                            CacheEntry {
                                data: data.clone(),
                                expires_at: Instant::now() + Duration::from_secs(86400), // 24小时
                            },
                        );
                    }

                    (
                        StatusCode::OK,
                        [(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")],
                        Json(data),
                    )
                        .into_response()
                }
                Err(e) => {
                    tracing::warn!("QQ lyrics normalize failed for {}: {}", song_mid, e);
                    (
                        StatusCode::NOT_FOUND,
                        [(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")],
                        Json(json!({
                            "retcode": -1,
                            "error": e,
                            "lyric": "",
                            "trans": ""
                        })),
                    )
                        .into_response()
                }
            },
            Err(e) => {
                tracing::error!("Failed to parse QQ lyrics: {}", e);
                (StatusCode::BAD_GATEWAY, "Failed to parse response").into_response()
            }
        },
        Err(e) => {
            tracing::error!("Failed to fetch QQ lyrics: {}", e);
            (StatusCode::BAD_GATEWAY, "Failed to fetch lyrics").into_response()
        }
    }
}

#[cfg(test)]
mod qq_playlist_tests {
    use super::*;

    #[test]
    fn qq_playlist_request_uses_public_v8_endpoint_without_credentials() {
        let client = reqwest::Client::new();
        let request = qq_playlist_request(&client, "9780150005").build().unwrap();
        assert_eq!(request.method(), reqwest::Method::GET);
        assert_eq!(request.url().scheme(), "https");
        assert_eq!(request.url().host_str(), Some("c.y.qq.com"));
        assert_eq!(request.url().path(), "/v8/fcg-bin/fcg_v8_playlist_cp.fcg");
        let query: HashMap<_, _> = request.url().query_pairs().into_owned().collect();
        assert_eq!(query.len(), 4);
        assert_eq!(query.get("id").map(String::as_str), Some("9780150005"));
        assert_eq!(query.get("format").map(String::as_str), Some("json"));
        assert_eq!(query.get("newsong").map(String::as_str), Some("1"));
        assert_eq!(
            query.get("platform").map(String::as_str),
            Some("jqspaframe.json")
        );
        assert_eq!(
            request.headers()[reqwest::header::REFERER],
            "http://y.qq.com"
        );
        assert!(!request.headers().contains_key(reqwest::header::COOKIE));
        assert!(!request.headers().contains_key(reqwest::header::AUTHORIZATION));
    }

    #[tokio::test]
    async fn qq_playlist_rejects_invalid_ids_before_fetching() {
        for id in ["", "0", "-1", "abc", "99&newsong=0", "18446744073709551616"] {
            let response = proxy_qq_playlist(Path(id.to_string())).await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "id={id}");
        }
    }
}
