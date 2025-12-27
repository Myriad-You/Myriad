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
use crate::services::netease_service::{CacheEntry, NeteaseService, MUSIC_CACHE, RATE_LIMITER};

// ===== 简单的令牌桶限流器 =====
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
static PROXY_LIMITERS: Lazy<Arc<Mutex<HashMap<String, TokenBucket>>>> =
    Lazy::new(|| Arc::new(Mutex::new(HashMap::new())));

/// 获取域名的主标识 (例如 i0.hdslb.com -> hdslb.com)
fn get_domain_key(url: &str) -> String {
    if url.contains("hdslb.com") {
        return "hdslb.com".to_string();
    } else if url.contains("bilibili.com") {
        return "bilibili.com".to_string();
    } else if url.contains("steamstatic.com") {
        return "steamstatic.com".to_string();
    } else if url.contains("126.net") || url.contains("163.com") {
        return "netease".to_string();
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
                    // B站图片CDN：允许突发，但限制持续速率
                    // 之前是无限制导致429，现在限制为每秒5个请求，突发30个
                    "hdslb.com" | "bilibili.com" => TokenBucket::new(5.0, 30.0),
                    // Steam通常比较宽松
                    "steamstatic.com" => TokenBucket::new(20.0, 100.0),
                    // 网易云
                    "netease" => TokenBucket::new(10.0, 50.0),
                    // 其他
                    _ => TokenBucket::new(5.0, 20.0),
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

/// 代理图片请求，添加必要的Referer头
/// ⚠️ P2: This endpoint is public but has domain whitelist protection
/// Consider adding authentication if abuse is detected
pub async fn proxy_image(Query(params): Query<ImageProxyQuery>) -> Response {
    let url = params.url;

    // ✅ P2 安全增强：检查 URL 长度，防止恶意超长 URL
    if url.len() > 2048 {
        tracing::warn!("🚨 Rejected proxy request: URL too long ({})", url.len());
        return (StatusCode::BAD_REQUEST, "URL too long").into_response();
    }

    // ✅ 检查URL是否来自支持的域名（白名单保护）
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

    // ✅ 限流保护：等待获取令牌
    if wait_for_proxy_permit(&url).await.is_err() {
        tracing::warn!("🚨 Proxy rate limit exceeded (timeout) for URL: {}", url);
        return (
            StatusCode::TOO_MANY_REQUESTS,
            "Rate limit exceeded, please try again later",
        )
            .into_response();
    }

    // 创建HTTP客户端
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .timeout(std::time::Duration::from_secs(10)) // ✅ 添加超时保护
        .build()
        .unwrap();

    // 根据域名设置适当的Referer
    let referer = get_referer_for_url(&url);

    // 发起请求
    let response = match client.get(&url).header("Referer", referer).send().await {
        Ok(resp) => resp,
        Err(e) => {
            tracing::error!("🚨 Image proxy failed - URL: {}, Error: {:?}", url, e);
            // 区分不同类型的错误
            if e.is_timeout() {
                tracing::error!("   ⏱️  Timeout: Image request exceeded 10s limit");
                return (StatusCode::GATEWAY_TIMEOUT, "Image request timeout").into_response();
            } else if e.is_connect() {
                tracing::error!("   🔌 Connection failed: Cannot reach image server");
                return (StatusCode::BAD_GATEWAY, "Cannot connect to image server").into_response();
            } else {
                tracing::error!("   ❌ Unknown error: {:?}", e);
                return (StatusCode::BAD_GATEWAY, "Failed to fetch image").into_response();
            }
        }
    };

    // 获取内容类型
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("image/jpeg")
        .to_string();

    // ✅ P2 安全增强：验证是否为图片类型
    if !content_type.starts_with("image/") {
        tracing::warn!("🚨 Rejected proxy request: Not an image ({})", content_type);
        return (StatusCode::BAD_REQUEST, "Only image content is allowed").into_response();
    }

    // 获取图片数据，限制大小为 10MB
    let image_data = match response.bytes().await {
        Ok(data) => {
            // ✅ P2 安全增强：限制图片大小，防止内存耗尽
            if data.len() > 10 * 1024 * 1024 {
                tracing::warn!(
                    "🚨 Rejected proxy request: Image too large ({} bytes)",
                    data.len()
                );
                return (
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "Image size exceeds 10MB limit",
                )
                    .into_response();
            }
            data
        }
        Err(e) => {
            tracing::error!("Failed to read image data: {}", e);
            return (StatusCode::BAD_GATEWAY, "Failed to read image data").into_response();
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

/// 检查URL是否来自允许的域名
fn is_allowed_domain(url: &str) -> bool {
    // 核心平台白名单（需要特殊 Referer 处理的）
    let core_domains = [
        "hdslb.com",                  // Bilibili CDN
        "bilibili.com",               // Bilibili
        "steamstatic.com",            // Steam CDN
        "cloudflare.steamstatic.com", // Steam Cloudflare CDN
        "music.126.net",              // 网易云音乐 CDN
    ];

    // 如果是核心平台，直接允许
    if core_domains.iter().any(|domain| url.contains(domain)) {
        return true;
    }

    // 对于其他 URL，检查是否是有效的图片 URL（支持 RSS 阅读器等场景）
    // 只允许 http:// 和 https:// 开头的 URL
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return false;
    }

    // 检查 URL 是否看起来像图片（通过扩展名或常见图片路径模式）
    let url_lower = url.to_lowercase();
    let image_extensions = [
        ".jpg", ".jpeg", ".png", ".gif", ".webp", ".svg", ".ico", ".bmp", ".avif",
    ];
    let image_patterns = [
        "/images/",
        "/image/",
        "/img/",
        "/uploads/",
        "/media/",
        "/assets/",
        "/static/",
        "/files/",
        "/wp-content/",
    ];

    // 如果 URL 包含图片扩展名或图片路径模式，允许代理
    image_extensions.iter().any(|ext| url_lower.contains(ext))
        || image_patterns
            .iter()
            .any(|pattern| url_lower.contains(pattern))
}

/// 根据URL获取适当的Referer
fn get_referer_for_url(url: &str) -> &'static str {
    if url.contains("hdslb.com") || url.contains("bilibili.com") {
        "https://www.bilibili.com/"
    } else if url.contains("steamstatic.com") {
        "https://store.steampowered.com/"
    } else if url.contains("music.126.net") {
        "https://music.163.com/"
    } else {
        "https://www.google.com/"
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

// ===== 网易云音乐相关函数 =====
// ✅ proxy_netease_playlist 已简化，使用统一服务层
// ✅ generate_device_id, get_random_china_ip, get_random_user_agent 已移至 netease_utils.rs
// ⚠️ proxy_netease_lyrics 和 proxy_netease_audio 仍需简化（见下方）

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

/// 代理网易云音乐音频流 - 使用统一服务层
pub async fn proxy_netease_audio(Path(song_id): Path<String>) -> Response {
    let song_id_i64 = match song_id.parse::<i64>() {
        Ok(id) => id,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, "Invalid song ID").into_response();
        }
    };

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

                    match audio_resp.bytes().await {
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

// ===== QQ音乐相关函数（保持不变）=====

/// 代理QQ音乐歌单请求（带缓存）
pub async fn proxy_qq_playlist(Path(playlist_id): Path<String>) -> Response {
    let cache_key = format!("qq_playlist:{}", playlist_id);

    // 检查限流
    {
        let mut limiter = RATE_LIMITER.write().await;
        if !limiter.check_rate_limit(&cache_key) {
            tracing::warn!("Rate limit exceeded for QQ playlist: {}", playlist_id);
            return (
                StatusCode::TOO_MANY_REQUESTS,
                Json(serde_json::json!({
                    "error": "Too many requests",
                    "message": "请求过于频繁，请稍后再试"
                })),
            )
                .into_response();
        }
    }

    // 检查缓存（歌单缓存1小时）
    // ⚠️ 临时禁用缓存以确保包含新的isVip字段
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
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .build()
        .unwrap();

    let url = format!(
        "https://c.y.qq.com/qzone/fcg-bin/fcg_ucc_getcdinfo_byids_cp.fcg?type=1&json=1&utf8=1&onlysong=0&disstid={}&g_tk=5381&loginUin=0&hostUin=0&format=json&inCharset=utf8&outCharset=utf-8&notice=0&platform=yqq.json&needNewCode=0",
        playlist_id
    );

    match client
        .get(&url)
        .header("Referer", "https://y.qq.com/")
        .send()
        .await
    {
        Ok(resp) => match resp.json::<Value>().await {
            Ok(mut data) => {
                // 为QQ音乐歌曲添加VIP标记（QQ音乐通常不区分VIP，都可播放）
                if let Some(cdlist) = data.get_mut("cdlist") {
                    if let Some(cdlist_array) = cdlist.as_array_mut() {
                        for cd in cdlist_array.iter_mut() {
                            if let Some(songlist) = cd.get_mut("songlist") {
                                if let Some(songlist_array) = songlist.as_array_mut() {
                                    for song in songlist_array.iter_mut() {
                                        // QQ音乐大部分歌曲免费，标记为非VIP
                                        song.as_object_mut()
                                            .unwrap()
                                            .insert("isVip".to_string(), json!(false));
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
    // 优先从请求头获取真实IP（处理反向代理的情况）
    let client_ip = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(|s| s.trim().to_string())
        .or_else(|| {
            headers
                .get("x-real-ip")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| addr.ip().to_string());

    tracing::info!(
        "Client IP detection: original={}, socket={}",
        client_ip,
        addr.ip()
    );

    // 检查是否为本地IP/内网IP
    let is_local_ip = client_ip == "127.0.0.1"
        || client_ip == "::1"
        || client_ip.starts_with("192.168.")
        || client_ip.starts_with("10.")
        || client_ip.starts_with("172.16.")
        || client_ip.starts_with("172.17.")
        || client_ip.starts_with("172.18.")
        || client_ip.starts_with("172.19.")
        || client_ip.starts_with("172.20.")
        || client_ip.starts_with("172.21.")
        || client_ip.starts_with("172.22.")
        || client_ip.starts_with("172.23.")
        || client_ip.starts_with("172.24.")
        || client_ip.starts_with("172.25.")
        || client_ip.starts_with("172.26.")
        || client_ip.starts_with("172.27.")
        || client_ip.starts_with("172.28.")
        || client_ip.starts_with("172.29.")
        || client_ip.starts_with("172.30.")
        || client_ip.starts_with("172.31.");

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .build()
        .unwrap();

    // 如果是本地IP，需要获取服务器的公网IP，然后查询位置
    let target_ip = if is_local_ip {
        tracing::info!(
            "Detected local/private IP: {}, fetching server public IP",
            client_ip
        );

        // 方案1: 使用 ipify.org 获取服务器公网IP
        if let Ok(resp) = client.get("https://api.ipify.org?format=json").send().await {
            if let Ok(data) = resp.json::<Value>().await {
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
                if let Ok(text) = resp.text().await {
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
    let url1 = format!(
        "http://ip-api.com/json/{}?fields=status,lat,lon,city,country,regionName",
        target_ip
    );

    if let Ok(resp) = client.get(&url1).send().await {
        if let Ok(data) = resp.json::<Value>().await {
            if data.get("status").and_then(|s| s.as_str()) == Some("success") {
                tracing::info!(
                    "Geolocation success via ip-api.com for IP {}: city={}, region={}, country={}",
                    target_ip,
                    data.get("city")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown"),
                    data.get("regionName")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown"),
                    data.get("country")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                );
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
        if let Ok(data) = resp.json::<Value>().await {
            if let (Some(lat), Some(lon)) = (
                data.get("latitude").and_then(|v| v.as_f64()),
                data.get("longitude").and_then(|v| v.as_f64()),
            ) {
                // 转换为统一格式
                let unified_data = json!({
                    "status": "success",
                    "lat": lat,
                    "lon": lon,
                    "city": data.get("city").and_then(|v| v.as_str()).unwrap_or(""),
                    "country": data.get("country_name").and_then(|v| v.as_str()).unwrap_or(""),
                    "regionName": data.get("region").and_then(|v| v.as_str()).unwrap_or("")
                });

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
        if let Ok(data) = resp.json::<Value>().await {
            if let (Some(lat_str), Some(lon_str)) = (
                data.get("latitude").and_then(|v| v.as_str()),
                data.get("longitude").and_then(|v| v.as_str()),
            ) {
                if let (Ok(lat), Ok(lon)) = (lat_str.parse::<f64>(), lon_str.parse::<f64>()) {
                    // 转换为统一格式
                    let unified_data = json!({
                        "status": "success",
                        "lat": lat,
                        "lon": lon,
                        "city": data.get("city").and_then(|v| v.as_str()).unwrap_or(""),
                        "country": data.get("country").and_then(|v| v.as_str()).unwrap_or(""),
                        "regionName": data.get("region").and_then(|v| v.as_str()).unwrap_or("")
                    });

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

    // 如果是本地开发，返回默认位置
    if is_local_ip {
        tracing::warn!("All geolocation services failed for local IP, using default fallback");
        let fallback_data = json!({
            "status": "success",
            "lat": 0.0,
            "lon": 0.0,
            "city": "Localhost",
            "country": "Development",
            "regionName": "Local"
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

/// 代理QQ音乐歌词请求（带缓存）
pub async fn proxy_qq_lyrics(Path(song_mid): Path<String>) -> Response {
    let cache_key = format!("qq_lyrics:{}", song_mid);

    // 检查限流
    {
        let mut limiter = RATE_LIMITER.write().await;
        if !limiter.check_rate_limit(&cache_key) {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                Json(serde_json::json!({"error": "Too many requests"})),
            )
                .into_response();
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
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .build()
        .unwrap();

    let url = format!(
        "https://c.y.qq.com/lyric/fcgi-bin/fcg_query_lyric_new.fcg?songmid={}&g_tk=5381&format=json&inCharset=utf8&outCharset=utf-8&nobase64=1",
        song_mid
    );

    match client
        .get(&url)
        .header("Referer", "https://y.qq.com/")
        .send()
        .await
    {
        Ok(resp) => match resp.json::<Value>().await {
            Ok(data) => {
                // 存入缓存
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

// ===== 所有旧的网易云音乐函数已删除，使用统一服务层 =====

/// 代理一言 (Hitokoto) API 请求
/// GET /api/proxy/hitokoto
///
/// 解决前端直接调用 hitokoto.cn 时的 CORS 问题
pub async fn proxy_hitokoto() -> Response {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .build()
        .unwrap();

    let url = "https://v1.hitokoto.cn/?c=d&c=i&c=k&encode=json";

    match client.get(url).send().await {
        Ok(resp) => {
            if !resp.status().is_success() {
                tracing::error!("Hitokoto API returned status: {}", resp.status());
                return (
                    StatusCode::BAD_GATEWAY,
                    Json(json!({"error": "Hitokoto API failed"})),
                )
                    .into_response();
            }

            match resp.json::<Value>().await {
                Ok(data) => {
                    tracing::debug!("Hitokoto data fetched successfully");
                    (
                        StatusCode::OK,
                        [
                            (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*"),
                            (header::CACHE_CONTROL, "public, max-age=600"), // 10分钟缓存
                        ],
                        Json(data),
                    )
                        .into_response()
                }
                Err(e) => {
                    tracing::error!("Failed to parse Hitokoto response: {}", e);
                    (
                        StatusCode::BAD_GATEWAY,
                        Json(json!({"error": "Failed to parse Hitokoto response"})),
                    )
                        .into_response()
                }
            }
        }
        Err(e) => {
            tracing::error!("Failed to fetch Hitokoto: {}", e);
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({"error": "Failed to fetch Hitokoto"})),
            )
                .into_response()
        }
    }
}
