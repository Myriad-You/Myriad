//! Hitokoto proxy + web content fetch (public, SSRF-hardened).

use axum::{
    extract::Query,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Deserialize)]
pub struct HitokotoQuery {
    /// 自定义一言 API 地址（可选），为空时用 `default_hitokoto_url()`（`v1.hitokoto.cn`）。
    pub url: Option<String>,
}

/// 代理一言 (Hitokoto) API 请求
/// GET /api/proxy/hitokoto?url=xxx
///
/// # Auth policy (product decision)
/// Remains **unauthenticated** for home-page widgets. Mitigation =
/// 1. **Host policy** — custom `url` goes through
/// `outbound_security::build_public_http_client` (public-routable only, DNS pin,
/// no redirects). This is **not** the image-proxy `shared/image_proxy_hosts.json`
/// list (that file is for media CDN rewrite only).
/// 2. **Per-IP quota** — `COMPUTE_MAX` (45 / 60s) compute-intensive rate limit.
/// Prefer tightening quota / outbound policy over mandatory JWT.
///
/// Catalog alignment (do not drift):
/// - Source ids: `api/config::HITOKOTO_SOURCE_IDS` ↔ FE `quote.ts`
/// - Builtin hosts / default URL: `HITOKOTO_BUILTIN_HOSTS` / `default_hitokoto_url`
/// The proxy still accepts any **SSRF-safe** public URL so `custom` sources work;
/// product security is outbound policy + rate limit, not a host-only allowlist.
///
/// 解决前端直接调用一言 API 时的 CORS 问题；
/// 支持通过 `url` 参数使用自定义 / 其他语言的一言源。
pub async fn proxy_hitokoto(Query(params): Query<HitokotoQuery>) -> Response {
    /// 一言响应是一小段 JSON；给足余量即可，不必按 MiB 计。
    const MAX_BODY: usize = 64 * 1024;

    let url = match params.url.as_deref().map(str::trim) {
        Some(custom) if !custom.is_empty() => custom.to_string(),
        _ => crate::api::config::default_hitokoto_url(),
    };

    // 未认证的任意 URL 出站：解析后逐个地址校验公网可路由、把 DNS 结果 pin 住、
    // 禁用重定向；响应体流式读取并限长。
    let (parsed, client) = match crate::services::outbound_security::build_public_http_client(
        &url,
        std::time::Duration::from_secs(10),
        Some("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36"),
    )
    .await
    {
        Ok(pair) => pair,
        Err(e) => {
            tracing::warn!("Rejected hitokoto proxy for unsafe url {}: {}", url, e);
            return (
                StatusCode::BAD_REQUEST,
                Json(AppError::public_json("Invalid or disallowed hitokoto url")),
            )
                .into_response();
        }
    };

    let resp = match client.get(parsed).send().await {
        Ok(resp) => resp,
        Err(e) => {
            tracing::error!("Failed to fetch Hitokoto: {}", e);
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({"error": "Failed to fetch Hitokoto", "code": "hitokoto_fetch_failed"})),
            )
                .into_response();
        }
    };

    if !resp.status().is_success() {
        tracing::error!("Hitokoto API returned status: {}", resp.status());
        return (
            StatusCode::BAD_GATEWAY,
            Json(json!({"error": "Hitokoto API failed", "code": "hitokoto_fetch_failed"})),
        )
            .into_response();
    }

    let body = match crate::services::outbound_security::read_limited_body(resp, MAX_BODY).await {
        Ok(body) => body,
        Err(e) => {
            tracing::error!("Failed to read Hitokoto response: {}", e);
            return (
                StatusCode::BAD_GATEWAY,
                Json(AppError::public_json(
                    "Hitokoto response too large or unreadable",
                )),
            )
                .into_response();
        }
    };

    match serde_json::from_slice::<Value>(&body) {
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
                Json(json!({"error": "Failed to parse Hitokoto response", "code": "hitokoto_fetch_failed"})),
            )
                .into_response()
        }
    }
}

// 网页内容抓取代理

#[derive(Debug, Deserialize)]
pub struct FetchWebContentQuery {
    url: String,
}

/// 抓取外部网页内容（用于阅读器显示网络搜索结果）
/// GET /api/proxy/fetch-content?url=xxx
///
/// 返回清理后的文章内容，适合在阅读器中显示
pub async fn fetch_web_content(Query(params): Query<FetchWebContentQuery>) -> Response {
    let url = &params.url;

    // 安全检查：URL 长度
    if url.len() > 2048 {
        return (
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json("URL too long")),
        )
            .into_response();
    }

    // 安全检查：必须是 http/https
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return (
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json("Invalid URL scheme")),
        )
            .into_response();
    }

    // SSRF 防护：阻止请求内网地址
    if crate::federation::types::is_internal_url(url) {
        tracing::warn!(url = %url, "[FetchWebContent] Blocked SSRF attempt to internal URL");
        return (
            StatusCode::FORBIDDEN,
            Json(AppError::public_json("Cannot fetch internal URLs")),
        )
            .into_response();
    }

    tracing::info!(url = %url, "[FetchWebContent] Fetching external content");

    let (target_url, client) = match crate::services::outbound_security::build_public_http_client(
        url,
        std::time::Duration::from_secs(15),
        Some("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"),
    )
    .await
    {
        Ok(target) => target,
        Err(error) => {
            tracing::warn!(url = %url, %error, "[FetchWebContent] Rejected unsafe target");
            return (
                StatusCode::FORBIDDEN,
                Json(AppError::public_json("Cannot fetch unsafe URLs")),
            )
                .into_response();
        }
    };

    match client.get(target_url).send().await {
        Ok(resp) => {
            if !resp.status().is_success() {
                tracing::warn!(url = %url, status = %resp.status(), "[FetchWebContent] HTTP error");
                return (
                    StatusCode::BAD_GATEWAY,
                    Json(json!({
                        "error": "Failed to fetch page",
                        "code": "page_fetch_failed",
                        "status": resp.status().as_u16(),
                        "fallbackUrl": url
                    })),
                )
                    .into_response();
            }

            // 获取内容类型
            let content_type = resp
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("text/html");

            // 只接受 `text/html` 或 `text/plain`
            if !content_type.contains("text/html") && !content_type.contains("text/plain") {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": "Not an HTML page",
                        "contentType": content_type,
                        "fallbackUrl": url
                    })),
                )
                    .into_response();
            }

            // 流式限长：`resp.text()` 会先缓冲整页再判断大小，上限拦不住内存消耗。
            const MAX_PAGE_BYTES: usize = 5 * 1024 * 1024;
            match crate::services::outbound_security::read_limited_body(resp, MAX_PAGE_BYTES)
                .await
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            {
                Ok(html) => {
                    // 提取文章内容
                    let extracted = extract_article_content(&html, url);

                    tracing::info!(
                        url = %url,
                        title_len = extracted.title.len(),
                        content_len = extracted.content.len(),
                        "[FetchWebContent] Content extracted successfully"
                    );

                    (
                        StatusCode::OK,
                        [
                            (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*"),
                            (header::CACHE_CONTROL, "public, max-age=3600"), // 1小时缓存
                        ],
                        Json(json!({
                            "success": true,
                            "url": url,
                            "title": extracted.title,
                            "content": extracted.content,
                            "excerpt": extracted.excerpt,
                            "author": extracted.author,
                            "publishedAt": extracted.published_at,
                            "siteName": extracted.site_name,
                            "fromWebSearch": true
                        })),
                    )
                        .into_response()
                }
                Err(e) => {
                    tracing::error!(url = %url, error = %e, "[FetchWebContent] Failed to read response");
                    (
                        StatusCode::BAD_GATEWAY,
                        Json(json!({
                            "error": "Failed to read page content",
                            "code": "page_fetch_failed",
                            "fallbackUrl": url
                        })),
                    )
                        .into_response()
                }
            }
        }
        Err(e) => {
            tracing::error!(url = %url, error = %e, "[FetchWebContent] Request failed");
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({
                    "error": "Failed to fetch page",
                    "code": "page_fetch_failed",
                    "fallbackUrl": url
                })),
            )
                .into_response()
        }
    }
}

/// 提取的文章内容
struct ExtractedArticle {
    title: String,
    content: String,
    excerpt: String,
    author: Option<String>,
    published_at: Option<String>,
    site_name: Option<String>,
}

/// 从 HTML 中提取文章内容
fn extract_article_content(html: &str, url: &str) -> ExtractedArticle {
    // 提取标题
    let title = extract_meta_content(html, "og:title")
        .or_else(|| extract_meta_content(html, "twitter:title"))
        .or_else(|| extract_tag_content(html, "title"))
        .unwrap_or_else(|| "Untitled".to_string());

    // 提取作者
    let author = extract_meta_content(html, "author")
        .or_else(|| extract_meta_content(html, "article:author"));

    // 提取发布时间
    let published_at = extract_meta_content(html, "article:published_time")
        .or_else(|| extract_meta_content(html, "datePublished"));

    // 提取站点名称
    let site_name =
        extract_meta_content(html, "og:site_name").or_else(|| extract_domain_from_url_simple(url));

    // 提取描述/摘要
    let excerpt = extract_meta_content(html, "og:description")
        .or_else(|| extract_meta_content(html, "description"))
        .or_else(|| extract_meta_content(html, "twitter:description"))
        .unwrap_or_default();

    // 提取正文内容
    let content = extract_main_content(html);

    ExtractedArticle {
        title,
        content,
        excerpt,
        author,
        published_at,
        site_name,
    }
}

/// 提取 meta 标签内容
fn extract_meta_content(html: &str, name: &str) -> Option<String> {
    // 匹配 <meta property="xxx" content="yyy"> 或 <meta name="xxx" content="yyy">
    let patterns = [
        format!(
            r#"<meta[^>]*property=["']{}["'][^>]*content=["']([^"']+)["']"#,
            regex::escape(name)
        ),
        format!(
            r#"<meta[^>]*name=["']{}["'][^>]*content=["']([^"']+)["']"#,
            regex::escape(name)
        ),
        format!(
            r#"<meta[^>]*content=["']([^"']+)["'][^>]*property=["']{}["']"#,
            regex::escape(name)
        ),
        format!(
            r#"<meta[^>]*content=["']([^"']+)["'][^>]*name=["']{}["']"#,
            regex::escape(name)
        ),
    ];

    for pattern in &patterns {
        if let Ok(re) = regex::Regex::new(pattern) {
            if let Some(caps) = re.captures(html) {
                if let Some(m) = caps.get(1) {
                    let content = m.as_str().trim();
                    if !content.is_empty() {
                        return Some(html_decode(content));
                    }
                }
            }
        }
    }

    None
}

/// 提取标签内容
fn extract_tag_content(html: &str, tag: &str) -> Option<String> {
    let pattern = format!(r#"<{}\s*[^>]*>([^<]+)</{}>""#, tag, tag);
    if let Ok(re) = regex::Regex::new(&pattern) {
        if let Some(caps) = re.captures(html) {
            if let Some(m) = caps.get(1) {
                let content = m.as_str().trim();
                if !content.is_empty() {
                    return Some(html_decode(content));
                }
            }
        }
    }
    None
}

/// 提取主要内容
fn extract_main_content(html: &str) -> String {
    // 移除 script 和 style 标签
    let html = remove_tags(html, "script");
    let html = remove_tags(&html, "style");
    let html = remove_tags(&html, "nav");
    let html = remove_tags(&html, "header");
    let html = remove_tags(&html, "footer");
    let html = remove_tags(&html, "aside");
    let html = remove_tags(&html, "noscript");

    // 尝试找到 article 标签
    if let Some(article) = extract_tag_block(&html, "article") {
        return clean_html_content(&article);
    }

    // 尝试找到 main 标签
    if let Some(main) = extract_tag_block(&html, "main") {
        return clean_html_content(&main);
    }

    // 尝试找到常见的内容容器
    let content_patterns = [
        r#"<div[^>]*class="[^"]*(?:article|content|post|entry|main)[^"]*"[^>]*>"#,
        r#"<div[^>]*id="[^"]*(?:article|content|post|entry|main)[^"]*"[^>]*>"#,
    ];

    for pattern in &content_patterns {
        if let Ok(re) = regex::Regex::new(pattern) {
            if let Some(m) = re.find(&html) {
                let start = m.start();
                if let Some(content) = extract_div_block(&html[start..]) {
                    let cleaned = clean_html_content(&content);
                    if cleaned.len() > 200 {
                        return cleaned;
                    }
                }
            }
        }
    }

    // 最后尝试提取 body 内容
    if let Some(body) = extract_tag_block(&html, "body") {
        return clean_html_content(&body);
    }

    // 如果都失败，清理整个 HTML
    clean_html_content(&html)
}

/// 移除指定标签及其内容
fn remove_tags(html: &str, tag: &str) -> String {
    let pattern = format!(r"(?is)<{}\b[^>]*>.*?</{}>", tag, tag);
    if let Ok(re) = regex::Regex::new(&pattern) {
        re.replace_all(html, "").to_string()
    } else {
        html.to_string()
    }
}

/// 提取标签块
fn extract_tag_block(html: &str, tag: &str) -> Option<String> {
    let start_pattern = format!(r"(?i)<{}\b[^>]*>", tag);
    let end_tag = format!("</{}>", tag);

    if let Ok(re) = regex::Regex::new(&start_pattern) {
        if let Some(m) = re.find(html) {
            let start = m.end();
            if let Some(end_pos) = html[start..].to_lowercase().find(&end_tag.to_lowercase()) {
                return Some(html[start..start + end_pos].to_string());
            }
        }
    }
    None
}

/// 提取 div 块（处理嵌套）
fn extract_div_block(html: &str) -> Option<String> {
    let mut depth = 0;
    let mut in_tag = false;
    let mut tag_start = 0;
    let mut content_start = 0;
    let chars: Vec<char> = html.chars().collect();

    for (i, &ch) in chars.iter().enumerate() {
        if ch == '<' {
            in_tag = true;
            tag_start = i;
        } else if ch == '>' && in_tag {
            in_tag = false;
            let tag_content: String = chars[tag_start..=i].iter().collect();
            let tag_lower = tag_content.to_lowercase();

            if tag_lower.starts_with("<div") {
                if depth == 0 {
                    content_start = i + 1;
                }
                depth += 1;
            } else if tag_lower.starts_with("</div") {
                depth -= 1;
                if depth == 0 {
                    return Some(chars[content_start..tag_start].iter().collect());
                }
            }
        }
    }
    None
}

/// 清理 HTML 内容，保留基本格式
fn clean_html_content(html: &str) -> String {
    // 保留段落结构：将 p, br, div 转换为换行
    let html = match regex::Regex::new(r"(?i)</p>|<br\s*/?>|</div>|</li>|</h[1-6]>") {
        Ok(re) => re.replace_all(html, "\n").to_string(),
        Err(_) => html.to_string(),
    };

    // 保留列表项标记
    let html = match regex::Regex::new(r"(?i)<li[^>]*>") {
        Ok(re) => re.replace_all(&html, "\n• ").to_string(),
        Err(_) => html,
    };

    // 移除所有 HTML 标签
    let html = match regex::Regex::new(r"<[^>]+>") {
        Ok(re) => re.replace_all(&html, "").to_string(),
        Err(_) => html,
    };

    // HTML 实体解码
    let html = html_decode(&html);

    // 合并多个空白字符
    let html = match regex::Regex::new(r"[ \t]+") {
        Ok(re) => re.replace_all(&html, " ").to_string(),
        Err(_) => html,
    };

    // 合并多个换行
    let html = match regex::Regex::new(r"\n\s*\n+") {
        Ok(re) => re.replace_all(&html, "\n\n").to_string(),
        Err(_) => html,
    };

    html.trim().to_string()
}

/// HTML 实体解码
fn html_decode(s: &str) -> String {
    s.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&#x27;", "'")
        .replace("&#x2F;", "/")
        .replace("&mdash;", "\u{2014}") // —
        .replace("&ndash;", "\u{2013}") // –
        .replace("&hellip;", "\u{2026}") // …
        .replace("&lsquo;", "\u{2018}") // '
        .replace("&rsquo;", "\u{2019}") // '
        .replace("&ldquo;", "\u{201C}") // "
        .replace("&rdquo;", "\u{201D}") // "
}

/// 从 URL 提取域名
fn extract_domain_from_url_simple(url: &str) -> Option<String> {
    let url = url
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("www.");

    url.split('/').next().map(|s| s.to_string())
}
use myriad_error::AppError;
