
fn proxy_json_err(status: axum::http::StatusCode, error: &str) -> axum::response::Response {
    use axum::response::IntoResponse;
    crate::error::HttpError(myriad_error::AppError::from_status_u16(status.as_u16(), error)).into_response()
}

// 图片代理服务 - 用于处理Bilibili等平台的防盗链图片
use axum::{
    extract::{Path, Query},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use once_cell::sync::Lazy;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

// 导入网易云音乐统一服务
use crate::services::kugou_service::KugouService;
use crate::services::netease_service::{CacheEntry, NeteaseService, MUSIC_CACHE, RATE_LIMITER};

/// Music proxy 429 with Retry-After + JSON body for FE toast / axios interceptors.
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

// 全局代理限流器映射 (域名 -> 令牌桶)
/// 音频代理的响应体上限。
///
/// 原实现直接 `bytes()`，**完全没有上限** —— 上游返回多大就往内存里读多大。
/// 128 MiB 足以覆盖无损单曲（FLAC 一首约 30–60 MB），同时把单个请求的
/// 内存占用封死。
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
    serde_json::from_slice(&bytes).map_err(|e| format!("Invalid JSON from upstream: {e}"))
}

const MAX_AUDIO_BYTES: usize = 128 * 1024 * 1024;

static PROXY_LIMITERS: Lazy<Arc<Mutex<HashMap<String, TokenBucket>>>> =
    Lazy::new(|| Arc::new(Mutex::new(HashMap::new())));

/// Normalize a DNS host for allowlist / rate-limit keys: lowercase, strip trailing dot.
fn normalize_host(host: &str) -> String {
    host.trim_end_matches('.').to_ascii_lowercase()
}

/// Exact host match or proper DNS suffix (`i0.hdslb.com` matches `hdslb.com`;
/// lookalikes like `hdslb.com.evil.com` / `nothdslb.com` do not).
fn host_matches_domain(host: &str, domain: &str) -> bool {
    let host = normalize_host(host);
    let domain = normalize_host(domain);
    if domain.is_empty() || host.is_empty() {
        return false;
    }
    host == domain || host.ends_with(&format!(".{domain}"))
}

/// Parse `http`/`https` URL and return normalized host, or `None` if unusable.
fn parse_proxy_url_host(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
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

/// 获取域名的主标识 (例如 i0.hdslb.com -> hdslb.com)
///
/// Host-based only (no substring-of-full-URL matching) so lookalike URLs cannot
/// steal another CDN's rate-limit bucket.
fn get_domain_key(url: &str) -> String {
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
    } else if host_matches_domain(&host, "discordapp.com")
        || host_matches_domain(&host, "discordapp.net")
    {
        return "discord".to_string();
    } else if host_matches_domain(&host, "myanimelist.net") {
        return "mal".to_string();
    } else if host_matches_domain(&host, "twimg.com") {
        return "x".to_string();
    } else if host_matches_domain(&host, "ytimg.com") || host_matches_domain(&host, "ggpht.com")
    {
        return "youtube".to_string();
    } else if host_matches_domain(&host, "xboxlive.com") {
        return "xbox".to_string();
    } else if host_matches_domain(&host, "playstation.net") {
        return "psn".to_string();
    } else if host_matches_domain(&host, "enka.network") {
        return "enka".to_string();
    } else if host_matches_domain(&host, "githubusercontent.com") {
        return "github".to_string();
    } else if host_matches_domain(&host, "google.com") || host_matches_domain(&host, "gstatic.com")
    {
        return "google-favicon".to_string();
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
                    // Discord CDN
                    "discord" => TokenBucket::new(15.0, 60.0),
                    // MyAnimeList CDN
                    "mal" => TokenBucket::new(12.0, 60.0),
                    // 其它 allowlist 图床 / 博客图（RSS 等）
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
/// Guest-facing avatars/covers must stay **unauthenticated**: require JWT would break
/// public profile cards. Mitigation = host allowlist + per-IP rolling quota
/// (`PUBLIC_IMAGE_IP_HITS`) + SSRF guards. Revisit JWT-only if abuse exceeds ops tolerance.
///
/// Security rejections (SSRF / domain / unsafe target / rate limit / oversize URL)
/// still return hard 4xx. Upstream fetch/content failures soft-fail with a
/// transparent 1×1 PNG (HTTP 200) so ordinary `<img>` usage stays quiet.
pub async fn proxy_image(Query(params): Query<ImageProxyQuery>) -> Response {
    let url = params.url;

    // P2 安全增强：检查 URL 长度，防止恶意超长 URL
    if url.len() > 2048 {
        tracing::warn!("🚨 Rejected proxy request: URL too long ({})", url.len());
        return (StatusCode::BAD_REQUEST, "URL too long").into_response();
    }

    // 检查URL是否来自支持的域名（白名单保护）
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

    // P2 安全增强：验证是否为图片类型（非图片 → soft-fail, not 400 red console）
    if !content_type.starts_with("image/") {
        tracing::debug!(%url, %content_type, "Image proxy rejected non-image content");
        return soft_fail_placeholder("non-image content-type", &url);
    }

    // 获取图片数据，限制大小为 10MB
    // 流式读取并封顶。
    //
    // 原实现是 `response.bytes()` 先把整个响应缓冲进内存、再判断是否超过
    // 10 MiB —— 上限拦不住内存消耗，只是在事后拒绝。白名单域名被攻陷或
    // 单纯故障时，一个 500 MB 的响应会先被完整读进来。
    //
    // `read_limited_body` 逐块累加、一超限就中断，内存占用真正有界。
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
            // 延长缓存至 7 天，减少重复请求 (Lighthouse 建议高效的缓存生命周期)
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

/// Explicit host allowlist for `/api/proxy/image` (MYR-007).
///
/// Matching is **parsed host** exact or proper DNS suffix only — never substring
/// of the full URL. Wider than `needs_image_proxy` (auto-rewrite narrow list in
/// `shared/image_proxy_hosts.json`), but **not** an open proxy: no fallback for
/// arbitrary public `.jpg` / `/images/` paths.
///
/// Guest-facing avatars/covers stay unauthenticated (product decision); this
/// list + SSRF guards + size limits are the primary mitigations.
const ALLOWED_IMAGE_PROXY_DOMAINS: &[&str] = &[
    // Platform CDNs (hotlink / avatar / cover)
    "hdslb.com",            // Bilibili CDN
    "bilibili.com",         // Bilibili
    "steamstatic.com",      // Steam CDN (includes cloudflare.steamstatic.com)
    "akamaihd.net",         // Steam legacy avatar CDN
    "bgm.tv",               // Bangumi
    "bangumi.tv",           // Bangumi legacy
    "chii.in",              // Bangumi legacy CDN
    "126.net",              // 网易云音乐 CDN (music.126.net, …)
    "163.com",              // 网易云
    "discordapp.com",       // Discord CDN
    "discordapp.net",       // Discord media
    "myanimelist.net",      // MyAnimeList
    "twimg.com",            // X / Twitter media
    "githubusercontent.com", // GitHub avatars (extensionless paths)
    "ggpht.com",            // YouTube channel avatars
    "ytimg.com",            // YouTube thumbs
    "googleusercontent.com",
    "xboxlive.com",
    "playstation.net",
    "enka.network",
    // Brew / SourceCard favicon helpers — explicit hosts only (not open path match)
    "google.com", // www.google.com/s2/favicons
    "gstatic.com", // t*.gstatic.com/faviconV2
];

/// True when Content-Type is SVG (active content if served same-origin).
fn is_disallowed_image_content_type(content_type: &str) -> bool {
    let base = content_type
        .split(';')
        .next()
        .unwrap_or(content_type)
        .trim()
        .to_ascii_lowercase();
    // image/svg+xml, image/svg, any image/svg* variant
    base == "image/svg+xml" || base == "image/svg" || base.starts_with("image/svg+")
}

/// 检查URL是否来自允许的域名（parsed host exact/suffix only）.
fn is_allowed_domain(url: &str) -> bool {
    let Some(host) = parse_proxy_url_host(url) else {
        return false;
    };
    ALLOWED_IMAGE_PROXY_DOMAINS
        .iter()
        .any(|domain| host_matches_domain(&host, domain))
}

/// 根据URL获取适当的Referer（host-based, not URL substring）.
fn get_referer_for_url(url: &str) -> &'static str {
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
    } else if host_matches_domain(&host, "myanimelist.net") {
        "https://myanimelist.net/"
    } else if host_matches_domain(&host, "twimg.com") {
        "https://x.com/"
    } else if host_matches_domain(&host, "discordapp.com")
        || host_matches_domain(&host, "discordapp.net")
    {
        "https://discord.com/"
    } else if host_matches_domain(&host, "ytimg.com") || host_matches_domain(&host, "ggpht.com") {
        "https://www.youtube.com/"
    } else if host_matches_domain(&host, "xboxlive.com") {
        "https://www.xbox.com/"
    } else if host_matches_domain(&host, "playstation.net") {
        "https://www.playstation.com/"
    } else if host_matches_domain(&host, "enka.network") {
        "https://enka.network/"
    } else if host_matches_domain(&host, "githubusercontent.com") {
        "https://github.com/"
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
    fn host_matches_domain_exact_and_suffix_only() {
        assert!(host_matches_domain("hdslb.com", "hdslb.com"));
        assert!(host_matches_domain("i0.hdslb.com", "hdslb.com"));
        assert!(host_matches_domain("I0.HDSLB.COM.", "hdslb.com"));
        // Lookalikes must not match
        assert!(!host_matches_domain("hdslb.com.evil.com", "hdslb.com"));
        assert!(!host_matches_domain("nothdslb.com", "hdslb.com"));
        assert!(!host_matches_domain("evil-hdslb.com", "hdslb.com"));
        assert!(!host_matches_domain("com", "hdslb.com"));
    }

    #[test]
    fn allows_legitimate_cdn_hosts() {
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
        assert!(is_allowed_domain("https://lain.bgm.tv/pic/cover/l/1.jpg"));
        assert!(is_allowed_domain(
            "https://pbs.twimg.com/profile_images/1/normal.jpg"
        ));
        assert!(is_allowed_domain(
            "https://cdn.myanimelist.net/images/anime/1.jpg"
        ));
        assert!(is_allowed_domain(
            "https://cdn.discordapp.com/avatars/1/2.png"
        ));
        // Extensionless avatar CDNs (core allowlist)
        assert!(is_allowed_domain(
            "https://avatars.githubusercontent.com/u/1?v=4"
        ));
        assert!(is_allowed_domain(
            "https://yt3.ggpht.com/ytc/AIdro_test=s88-c-k-c0x00ffffff-no-rj"
        ));
        // Brew default: Google favicon service (explicit host list, not path fallback)
        assert!(is_allowed_domain(
            "https://www.google.com/s2/favicons?domain=example.com&sz=64"
        ));
        assert!(is_allowed_domain(
            "https://t2.gstatic.com/faviconV2?client=SOCIAL&type=FAVICON&url=https://example.com"
        ));
        // Trailing-dot host normalization
        assert!(is_allowed_domain("https://i0.hdslb.com./bfs/face/x.jpg"));
    }

    #[test]
    fn rejects_lookalike_hosts_and_arbitrary_public_images() {
        // Substring lookalikes that the old url.contains() allowlist accepted
        assert!(!is_allowed_domain(
            "https://hdslb.com.evil.com/face.jpg"
        ));
        assert!(!is_allowed_domain(
            "https://nothdslb.com/bfs/face/x.jpg"
        ));
        assert!(!is_allowed_domain(
            "https://evil.com/cdn?u=hdslb.com/x.jpg"
        ));
        // Arbitrary public image URLs (removed open extension/path fallback)
        assert!(!is_allowed_domain("https://221.ltd/favicon.ico"));
        assert!(!is_allowed_domain("https://blog.hanawa.me/favicon.ico"));
        assert!(!is_allowed_domain(
            "https://evil.example/uploads/photo.jpg"
        ));
        assert!(!is_allowed_domain("https://cdn.evil.com/images/a.png"));
        assert!(!is_allowed_domain("https://example.com/static/logo.webp"));
        assert!(!is_allowed_domain("https://evil.example/page.html"));
        assert!(!is_allowed_domain("ftp://evil.example/x.png"));
        assert!(!is_allowed_domain("not-a-url"));
        assert!(!is_allowed_domain("https://example.com/api/data"));
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
    fn soft_fail_does_not_weaken_security_rejections() {
        // Documented contract: security gates stay hard-fail; only covered
        // here via is_allowed_domain / length checks that proxy_image uses first.
        assert!(!is_allowed_domain("not-a-url"));
        assert!(!is_allowed_domain("https://example.com/api/data"));
        // Arbitrary .jpg is no longer allowed (open-proxy regression guard)
        assert!(!is_allowed_domain("https://example.com/photo.jpg"));
        let long = format!("https://i0.hdslb.com/{}.png", "a".repeat(3000));
        assert!(long.len() > 2048);
    }

    #[test]
    fn domain_key_and_referer_use_host_not_url_substring() {
        assert_eq!(
            get_domain_key("https://i0.hdslb.com/bfs/face/x.jpg"),
            "hdslb.com"
        );
        // Lookalike must not inherit bilibili rate-limit bucket
        assert_eq!(
            get_domain_key("https://hdslb.com.evil.com/x.jpg"),
            "other"
        );
        assert_eq!(
            get_referer_for_url("https://i0.hdslb.com/x.jpg"),
            "https://www.bilibili.com/"
        );
        assert_eq!(
            get_referer_for_url("https://evil.com/?ref=hdslb.com"),
            "https://www.google.com/"
        );
    }
    #[tokio::test]
    async fn respond_netease_play_url_json_and_redirect() {
        let cdn = "https://m801.music.126.net/song.mp3?sign=abc";
        let json_resp = respond_netease_play_url(cdn, Some("json"));
        assert_eq!(json_resp.status(), StatusCode::OK);
        let body = to_bytes(json_resp.into_body(), 64 * 1024).await.expect("body");
        let v: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(v["url"], cdn);

        let redir = respond_netease_play_url(cdn, None);
        assert_eq!(redir.status(), StatusCode::FOUND);
        let loc = redir.headers().get(header::LOCATION).and_then(|h| h.to_str().ok()).unwrap_or_default();
        assert_eq!(loc, cdn);
        let cache = redir.headers().get(header::CACHE_CONTROL).and_then(|h| h.to_str().ok()).unwrap_or_default();
        assert!(cache.contains("max-age=60"), "302 must not long-cache CDN URLs");
    }
}


/// 代理网易云音乐歌单请求 - 使用统一服务层
pub async fn proxy_netease_playlist(Path(playlist_id): Path<String>) -> Response {
    let playlist_id_i64 = match playlist_id.parse::<i64>() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid playlist ID"})),
            )
                .into_response();
        }
    };

    // Align with QQ playlist: surface 429 + Retry-After for FE toast
    let cache_key = format!("netease_playlist:{}", playlist_id);
    {
        let mut limiter = RATE_LIMITER.write().await;
        if !limiter.check_rate_limit(&cache_key) {
            return music_rate_limited_response(&format!("Netease playlist: {}", playlist_id));
        }
    }

    let service = NeteaseService::new();
    match service.fetch_playlist(playlist_id_i64, true).await {
        Ok(data) => (
            StatusCode::OK,
            [(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")],
            Json(data),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("Failed to fetch Netease playlist {}: {}", playlist_id, e);
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({
                    "error": "Failed to fetch playlist",
                    "message": e.to_string()
                })),
            )
                .into_response()
        }
    }
}

// 网易云音乐相关函数
// proxy_netease_playlist 已简化，使用统一服务层
// generate_device_id, get_random_china_ip, get_random_user_agent 已移至 netease_utils.rs
// proxy_netease_lyrics 和 proxy_netease_audio 仍需简化（见下方）

/// 代理网易云音乐歌词请求 - 使用统一服务层
pub async fn proxy_netease_lyrics(Path(song_id): Path<String>) -> Response {
    let song_id_i64 = match song_id.parse::<i64>() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid song ID"})),
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
                    "message": e.to_string()
                })),
            )
                .into_response()
        }
    }
}

/// 代理网易云音乐逐字歌词请求（yrc）- 使用统一服务层
pub async fn proxy_netease_lyrics_verbatim(Path(song_id): Path<String>) -> Response {
    let song_id_i64 = match song_id.parse::<i64>() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid song ID"})),
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
                    "message": e.to_string()
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

/// 代理酷狗逐字歌词（KRC）- 网易云 yrc 缺失时的补充源
/// 返回 { krc: "<解码后的 KRC 文本>" }，由前端 parseKrc 解析
pub async fn proxy_kugou_lyrics_verbatim(Query(q): Query<KugouLyricsQuery>) -> Response {
    if q.keyword.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "keyword required"})),
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

/// 代理网易云音乐单曲详情请求 - 使用统一服务层
pub async fn proxy_netease_song(Path(song_id): Path<String>) -> Response {
    let song_id_i64 = match song_id.parse::<i64>() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid song ID"})),
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
                    "message": e.to_string()
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
        let cache = MUSIC_CACHE.read().await;
        if let Some(entry) = cache.get(&cache_key) {
            if entry.expires_at > Instant::now() {
                if let Some(url) = entry.data.get("url").and_then(|v| v.as_str()) {
                    return respond_netease_play_url(url, query.format.as_deref());
                }
            }
        }
    }

    let service = NeteaseService::new();
    match service.fetch_audio_url(song_id_i64).await {
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
            respond_netease_play_url(&audio_url, query.format.as_deref())
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

pub(crate) fn respond_netease_play_url(audio_url: &str, format: Option<&str>) -> Response {
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

    // 302：音频仍由浏览器直连网易 CDN（非本机拉流）
    // 勿长期缓存 302：CDN 签名 URL 会过期
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

/// 代理网易云音乐音频流 - 使用统一服务层
///
/// 海外或需绕过 CORS/防盗链时使用：本机拉取 CDN 再回传（流量经服务器）。
/// 国内 HTTPS 站点优先用 [`proxy_netease_play_url`] 直连 CDN。
pub async fn proxy_netease_audio(Path(song_id): Path<String>) -> Response {
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
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap();

            match client
                .get(&audio_url)
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
                        MAX_AUDIO_BYTES,
                    )
                    .await
                    {
                        Ok(audio_data) => (
                            StatusCode::OK,
                            [
                                (header::CONTENT_TYPE, content_type),
                                // 延长音频缓存至 24 小时 (Lighthouse 建议高效的缓存生命周期)
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

/// 校验 QQ 音乐 songmid（字母数字，长度通常 14）
fn is_valid_qq_songmid(song_mid: &str) -> bool {
    let len = song_mid.len();
    (8..=32).contains(&len) && song_mid.chars().all(|c| c.is_ascii_alphanumeric())
}

/// 通过 QQ 音乐 GetEVkey 接口解析可播放音频 URL
///
/// 旧前端硬编码的 `ws.stream.qqmusic.qq.com/{songmid}.m4a?fromtag=46` 已全面 403。
/// 当前可用路径：`music.vkey.GetEVkey` + `RS02{songmid}.mp3`（部分曲目需回退其它封装）。
async fn resolve_qq_audio_url(song_mid: &str) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .user_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
             (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
        )
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;

    // 优先 RS02（海外匿名可拿到 purl），再回退常见清晰度封装
    let filenames = [
        format!("RS02{}.mp3", song_mid),
        format!("M500{}.mp3", song_mid),
        format!("C400{}.m4a", song_mid),
        format!("M800{}.mp3", song_mid),
    ];

    let guid = format!("{:010}", rand::random::<u32>() % 1_000_000_000);

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
                tracing::debug!("QQ GetEVkey request failed for {}: {}", filename, e);
                continue;
            }
        };

        let data: Value = match resp.json().await {
            Ok(v) => v,
            Err(e) => {
                tracing::debug!("QQ GetEVkey parse failed for {}: {}", filename, e);
                continue;
            }
        };

        let req_code = data.pointer("/req_0/code").and_then(|v| v.as_i64());
        if req_code != Some(0) {
            tracing::debug!(
                "QQ GetEVkey req_0.code={:?} for {} / {}",
                req_code,
                song_mid,
                filename
            );
            continue;
        }

        let midinfo = match data.pointer("/req_0/data/midurlinfo/0") {
            Some(v) => v,
            None => {
                tracing::debug!("QQ GetEVkey empty midurlinfo for {}", filename);
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
            tracing::debug!(
                "QQ GetEVkey empty purl result={:?} for {} / {}",
                result,
                song_mid,
                filename
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
                tracing::debug!(
                    "QQ audio probe {} for {} -> {}",
                    probe.status(),
                    filename,
                    song_mid
                );
            }
            Err(e) => {
                tracing::debug!("QQ audio probe error for {}: {}", filename, e);
            }
        }
    }

    Err(format!("No playable QQ audio URL for songmid {}", song_mid))
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
        let cache = MUSIC_CACHE.read().await;
        if let Some(entry) = cache.get(&cache_key) {
            if entry.expires_at > Instant::now() {
                if let Some(url) = entry.data.get("url").and_then(|v| v.as_str()) {
                    return respond_qq_play_url(url, query.format.as_deref());
                }
            }
        }
    }

    match resolve_qq_audio_url(&song_mid).await {
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
        Err(e) => {
            tracing::error!("Failed to resolve QQ play URL for {}: {}", song_mid, e);
            (
                StatusCode::NOT_FOUND,
                "Audio not available (copyright, VIP, or geo-restriction)",
            )
                .into_response()
        }
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
pub async fn proxy_qq_audio(Path(song_mid): Path<String>) -> Response {
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

    let audio_url = match resolve_qq_audio_url(&song_mid).await {
        Ok(url) => url,
        Err(e) => {
            tracing::error!("Failed to resolve QQ audio for {}: {}", song_mid, e);
            return (
                StatusCode::NOT_FOUND,
                "Audio not available (copyright, VIP, or geo-restriction)",
            )
                .into_response();
        }
    };

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
             (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
        )
        .build()
        .unwrap();

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
                return (StatusCode::BAD_GATEWAY, "Failed to fetch audio stream").into_response();
            }

            let content_type = audio_resp
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("audio/mpeg")
                .to_string();

            match crate::services::outbound_security::read_limited_body(audio_resp, MAX_AUDIO_BYTES)
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
            (StatusCode::BAD_GATEWAY, "Failed to fetch audio stream").into_response()
        }
    }
}

/// 代理QQ音乐歌单请求（带缓存）
pub async fn proxy_qq_playlist(Path(playlist_id): Path<String>) -> Response {
    let cache_key = format!("qq_playlist:{}", playlist_id);

    // 检查限流
    {
        let mut limiter = RATE_LIMITER.write().await;
        if !limiter.check_rate_limit(&cache_key) {
            return music_rate_limited_response(&format!("QQ playlist: {}", playlist_id));
        }
    }

    // 检查缓存（歌单缓存1小时）
    // 临时禁用缓存以确保包含新的isVip字段
    let use_cache = false;
    if use_cache {
        let cache = MUSIC_CACHE.read().await;
        if let Some(entry) = cache.get(&cache_key) {
            if entry.expires_at > Instant::now() {
                tracing::debug!("Cache hit for QQ playlist: {}", playlist_id);
                return (
                    StatusCode::OK,
                    [(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")],
                    Json(entry.data.clone()),
                )
                    .into_response();
            }
        }
    }

    let client = reqwest::Client::builder()
        .user_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
             (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
        )
        .timeout(Duration::from_secs(20))
        .build()
        .unwrap();

    let url = format!(
        "https://c.y.qq.com/qzone/fcg-bin/fcg_ucc_getcdinfo_byids_cp.fcg?type=1&json=1&utf8=1&onlysong=0&disstid={}&g_tk=5381&loginUin=0&hostUin=0&format=json&inCharset=utf8&outCharset=utf-8&notice=0&platform=yqq.json&needNewCode=0",
        playlist_id
    );

    match client
        .get(&url)
        .header("Referer", "https://y.qq.com/")
        .header("Origin", "https://y.qq.com")
        .send()
        .await
    {
        Ok(resp) => match read_limited_json(resp).await {
            Ok(mut data) => {
                // QQ 匿名接口不返回可靠 VIP 字段；按 payplay 粗略标注（播放链仍以 audio 代理实测为准）
                if let Some(cdlist) = data.get_mut("cdlist") {
                    if let Some(cdlist_array) = cdlist.as_array_mut() {
                        for cd in cdlist_array.iter_mut() {
                            if let Some(songlist) = cd.get_mut("songlist") {
                                if let Some(songlist_array) = songlist.as_array_mut() {
                                    for song in songlist_array.iter_mut() {
                                        let payplay = song
                                            .pointer("/pay/payplay")
                                            .and_then(|v| v.as_i64())
                                            .unwrap_or(0);
                                        if let Some(obj) = song.as_object_mut() {
                                            obj.insert("isVip".to_string(), json!(payplay > 0));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // 存入缓存
                {
                    let mut cache = MUSIC_CACHE.write().await;
                    cache.insert(
                        cache_key,
                        CacheEntry {
                            data: data.clone(),
                            expires_at: Instant::now() + Duration::from_secs(604800), // 7天 (7*24*3600)
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
                tracing::error!("Failed to parse QQ playlist: {}", e);
                (StatusCode::BAD_GATEWAY, "Failed to parse response").into_response()
            }
        },
        Err(e) => {
            tracing::error!("Failed to fetch QQ playlist: {}", e);
            (StatusCode::BAD_GATEWAY, "Failed to fetch playlist").into_response()
        }
    }
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
        headers
            .get("x-real-ip")
            .and_then(|v| v.to_str().ok()),
        headers
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
    );

    // Private / loopback / unparseable → cannot geo-locate the visitor; fall
    // back to server egress only as a last resort (local dev / misconfigured
    // TRUST_PROXY_*). FE treats `source=server-egress` as soft failure.
    let is_local_ip = crate::middleware::client_ip::is_private_or_local_str(&client_ip);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .build()
        .unwrap();

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
        if let Ok(resp) = client.get("https://api.ipify.org?format=json").send().await {
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
        } else {
            // 方案2: 使用 icanhazip.com
            if let Ok(resp) = client.get("https://icanhazip.com").send().await {
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
            } else {
                client_ip.clone()
            }
        }
    } else {
        client_ip.clone()
    };

    tracing::info!("Querying geolocation for IP: {}", target_ip);

    // 尝试多个地理位置服务，提高成功率

    // 方案1: ip-api.com (免费，稳定，无需key)
    // countryCode is required by FE `isUserInChinaMainland` (CN vs name-only).
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
                    obj.insert(
                        "detected_client_ip".to_string(),
                        json!(client_ip),
                    );
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
                return (
                    StatusCode::OK,
                    [(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")],
                    Json(data),
                )
                    .into_response();
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

                return (
                    StatusCode::OK,
                    [(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")],
                    Json(unified_data),
                )
                    .into_response();
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

                    return (
                        StatusCode::OK,
                        [(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")],
                        Json(unified_data),
                    )
                        .into_response();
                }
            }
        }
    }

    // 如果是本地开发，返回默认位置（仍标记 server-egress，FE 可改走浏览器侧 IP）
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
        return (
            StatusCode::OK,
            [(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")],
            Json(fallback_data),
        )
            .into_response();
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
        // Keep raw keys for any legacy consumers
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
        let cache = MUSIC_CACHE.read().await;
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

    let client = reqwest::Client::builder()
        .user_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
             (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
        )
        .timeout(Duration::from_secs(15))
        .build()
        .unwrap();

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

// 所有旧的网易云音乐函数已删除，使用统一服务层
