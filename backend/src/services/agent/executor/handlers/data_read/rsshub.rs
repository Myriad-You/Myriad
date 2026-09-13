use super::super::HandlerContext;
use super::search::calculate_fuzzy_score;
use serde_json::{Value, json};
use std::collections::HashMap;

pub(super) async fn execute_brew_discover(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let url = params.get("url").and_then(|v| v.as_str());
    let query = params.get("query").and_then(|v| v.as_str());
    let auto_verify = params
        .get("autoVerify")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let mut feeds = Vec::new();
    let mut discovery_methods = Vec::new();

    // 策略1: 如果提供了 URL，直接尝试解析
    if let Some(feed_url) = url {
        discovery_methods.push("direct_url");
        match try_parse_feed(feed_url).await {
            Ok(feed_info) => {
                feeds.push(feed_info);
            }
            Err(e) => {
                tracing::warn!("Failed to parse feed URL {}: {}", feed_url, e);
            }
        }
    }

    // 策略2: 如果提供了查询词，从 RSSHub 路由中搜索
    if let Some(search_query) = query {
        discovery_methods.push("rsshub_routes");

        let rsshub_results = query_rsshub_routes(search_query).await?;

        for mut route in rsshub_results {
            if auto_verify {
                if let Some(route_url) = route.get("url").and_then(|v| v.as_str()) {
                    match try_parse_feed(route_url).await {
                        Ok(verified_info) => {
                            route["verified"] = json!(true);
                            route["title"] =
                                verified_info.get("title").cloned().unwrap_or(json!(null));
                            route["itemCount"] =
                                verified_info.get("itemCount").cloned().unwrap_or(json!(0));
                            route["feedType"] = verified_info
                                .get("feedType")
                                .cloned()
                                .unwrap_or(json!("unknown"));
                        }
                        Err(_) => {
                            route["verified"] = json!(false);
                            route["verifyError"] = json!("Unable to reach or parse this RSS feed");
                        }
                    }
                }
            }
            feeds.push(route);
        }

        // 策略3: 如果查询词本身就是 URL，尝试网站 RSS 自动发现
        if looks_like_url(search_query) {
            discovery_methods.push("website_autodiscover");
            if let Ok(discovered) = discover_rss_from_website(search_query).await {
                for feed in discovered {
                    if !feeds.iter().any(|f| f.get("url") == feed.get("url")) {
                        feeds.push(feed);
                    }
                }
            }
        }

        // 策略4: 若仍为空且有 analyzer，用已有知识推断 RSS URL（无实时联网）
        if feeds.is_empty() && ctx.ai_analyzer.is_some() {
            discovery_methods.push("ai_web_search");
            if let Ok(ai_results) = ai_search_rss_feeds(search_query, ctx).await {
                for feed in ai_results {
                    feeds.push(feed);
                }
            }
        }
    }

    if url.is_none() && query.is_none() {
        return Err("Missing url or query".to_string());
    }

    let found = !feeds.is_empty();
    let verified_count = feeds
        .iter()
        .filter(|f| f.get("verified") == Some(&json!(true)))
        .count();

    Ok(json!({
        "found": found,
        "feeds": feeds,
        "total": feeds.len(),
        "verifiedCount": verified_count,
        "discoveryMethods": discovery_methods,
        "suggestions": if !found {
            json!({
                "message": crate::services::agent::response_agent::no_rss_found(),
                "tips": [
                    "Try a more specific keyword",
                    "Provide an RSS/Atom URL directly",
                    "You can let AI search the web"
                ],
                "aiSearchPrompt": format!(
                    "Search for an RSS feed URL for \"{}\"",
                    query.unwrap_or("")
                )
            })
        } else {
            json!(null)
        }
    }))
}

/// 用 AI 已有知识推断 RSS 地址（无实时联网）
async fn ai_search_rss_feeds(query: &str, ctx: &HandlerContext<'_>) -> Result<Vec<Value>, String> {
    let analyzer = ctx
        .ai_analyzer
        .ok_or(myriad_agent_rules::AI_PROVIDER_NOT_CONFIGURED)?;

    let search_query = format!("{} RSS feed URL", query);

    // 使用 AI 推断常见 RSS 地址（注意：AI 没有实时联网能力，依赖已有知识）
    let prompt = format!(
        "From your knowledge, infer likely RSS/Atom feed URLs for \"{}\".\n\n\
        Rules:\n\
        1. Prefer known feed patterns on common platforms (WordPress /feed/, GitHub .atom, Reddit .rss, etc.)\n\
        2. RSSHub (rsshub.app) routes are allowed\n\
        3. Only return URLs you are fairly confident about\n\
        4. Return a JSON array: [{{\"url\": \"...\", \"name\": \"...\", \"confidence\": \"high|medium\"}}]\n\n\
        Return the JSON array only.",
        search_query
    );

    let result = analyzer.analyze(&prompt).await.map_err(|error| {
        tracing::error!(%error, "AI search failed");
        "AI search failed".to_string()
    })?;

    let mut feeds = Vec::new();

    let url_re =
        regex::Regex::new(r#"https?://[^\s<>"')\]]+(?:rss|feed|atom|xml)[^\s<>"')\]]*"#).unwrap();

    // 从 AI 响应中提取 RSS URL
    for cap in url_re.captures_iter(&result) {
        let url = cap.get(0).map(|m| m.as_str()).unwrap_or("");
        if !url.is_empty()
            && !feeds
                .iter()
                .any(|f: &Value| f.get("url") == Some(&json!(url)))
        {
            feeds.push(json!({
                "url": url,
                "name": "",
                "description": "Found by AI web search",
                "source": "ai_web_search",
                "verified": false
            }));
        }
    }

    tracing::info!(
        query = %query,
        found = feeds.len(),
        "[Agent] AI RSS search completed"
    );

    Ok(feeds)
}

fn looks_like_url(s: &str) -> bool {
    let s_lower = s.trim().to_lowercase();
    s_lower.starts_with("http://")
        || s_lower.starts_with("https://")
        || s_lower.starts_with("www.")
        || (s_lower.contains('.') && !s_lower.contains(' ') && s_lower.len() < 100)
}

async fn try_parse_feed(url: &str) -> Result<Value, String> {
    let (target, client) = crate::services::outbound_security::build_public_http_client(
        url,
        std::time::Duration::from_secs(10),
        Some("Mozilla/5.0 (compatible; MyriadBot/1.0)"),
    )
    .await
    .map_err(|_e| "Invalid URL".to_string())?;

    let response = client.get(target).send().await.map_err(|e| {
        tracing::warn!(error = %e, "Request failed");
        "Request failed".to_string()
    })?;

    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }

    let bytes = crate::services::outbound_security::read_limited_body(response, 2 * 1024 * 1024)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "Failed to read response");
            "Failed to read response".to_string()
        })?;
    let content = String::from_utf8_lossy(&bytes).to_string();

    // 检测 Feed 类型
    let feed_type = if content.contains("<rss") {
        "rss"
    } else if content.contains("<feed") {
        "atom"
    } else {
        "unknown"
    };

    // 提取标题
    let title_re = regex::Regex::new(r"<title[^>]*>(?:<!\[CDATA\[)?(.*?)(?:\]\]>)?</title>").ok();
    let title = title_re
        .and_then(|r| r.captures(&content))
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
        .unwrap_or_default();

    // 统计条目数量
    let item_count = content.matches("<item>").count() + content.matches("<entry>").count();

    Ok(json!({
        "url": url,
        "title": title,
        "feedType": feed_type,
        "itemCount": item_count,
        "verified": true
    }))
}

/// 查询 RSSHub 路由 - 支持缓存和远程获取
async fn query_rsshub_routes(query: &str) -> Result<Vec<Value>, String> {
    let query_lower = query.to_lowercase();
    let mut results = Vec::new();

    // 尝试读取本地缓存
    let routes_data = load_rsshub_routes_cache().await;

    if let Some(routes) = routes_data {
        // 在路由中搜索匹配项
        if let Some(routes_array) = routes.as_array() {
            for route in routes_array {
                // 获取路由信息
                let name = route.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let path = route.get("path").and_then(|v| v.as_str()).unwrap_or("");
                let description = route
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let requires_config = route
                    .get("requiresConfig")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let config_params = route.get("configParams").and_then(|v| v.as_array());

                // Skip requiresConfig (parser: extra `:` in target). `configParams` is not in cache JSON.
                if requires_config {
                    continue;
                }
                if let Some(params) = config_params {
                    let has_sensitive_param = params.iter().any(|p| {
                        let param_name = p.as_str().unwrap_or("");
                        param_name.contains("cookie")
                            || param_name.contains("token")
                            || param_name.contains("key")
                            || param_name.contains("secret")
                            || param_name.contains("password")
                    });
                    if has_sensitive_param {
                        continue;
                    }
                }

                // 模糊匹配
                let name_lower = name.to_lowercase();
                let desc_lower = description.to_lowercase();
                let path_lower = path.to_lowercase();

                let name_score = calculate_fuzzy_score(&query_lower, &name_lower);
                let desc_score = calculate_fuzzy_score(&query_lower, &desc_lower) * 0.7;
                let path_score = calculate_fuzzy_score(&query_lower, &path_lower) * 0.5;

                let max_score = name_score.max(desc_score).max(path_score);

                if max_score > 0.3 {
                    results.push(json!({
                        "name": name,
                        "path": path,
                        "url": format!("https://rsshub.app{}", path),
                        "description": description,
                        "source": "rsshub",
                        "score": max_score,
                        "verified": false  // 需要实际验证 URL 是否可用
                    }));
                }
            }
        }
    }

    // 按得分排序
    results.sort_by(|a, b| {
        let score_a = a.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let score_b = b.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0);
        score_b
            .partial_cmp(&score_a)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // 限制返回数量
    results.truncate(10);

    Ok(results)
}

/// 加载 RSSHub 路由缓存
async fn load_rsshub_routes_cache() -> Option<Value> {
    use crate::services::data_paths::paths;

    // 尝试读取本地缓存文件
    let cache_path = paths().rsshub_routes_cache();
    if let Ok(content) = tokio::fs::read_to_string(&cache_path).await {
        if let Ok(data) = serde_json::from_str::<Value>(&content) {
            // 检查缓存是否过期（7天）
            if let Some(cached_at) = data.get("cachedAt").and_then(|v| v.as_i64()) {
                let now = chrono::Utc::now().timestamp();
                if now - cached_at < 7 * 24 * 3600 {
                    return data.get("routes").cloned();
                }
            }
        }
    }

    // 缓存不存在或已过期，尝试从远程获取
    fetch_rsshub_routes().await.ok()
}

/// Fetch DIYGod/RSSHub `lib/radar-rules.js` from GitHub; on failure return [].
async fn fetch_rsshub_routes() -> Result<Value, String> {
    use crate::services::data_paths::paths;

    let radar_url = "https://raw.githubusercontent.com/DIYgod/RSSHub/master/lib/radar-rules.js";

    let (target, client) = crate::services::outbound_security::build_public_http_client(
        radar_url,
        std::time::Duration::from_secs(30),
        Some("Myriad Agent/1.0 (rsshub-routes)"),
    )
    .await
    .map_err(|_e| "Invalid URL".to_string())?;

    // 尝试获取 radar-rules（这是一个 JS 文件，包含路由规则）
    match client.get(target).send().await {
        Ok(response) if response.status().is_success() => {
            if let Ok(bytes) =
                crate::services::outbound_security::read_limited_body(response, 5 * 1024 * 1024)
                    .await
            {
                let content = String::from_utf8_lossy(&bytes).to_string();
                // 解析 radar-rules.js 提取路由信息
                let routes =
                    crate::services::agent::data_read_pure::parse_rsshub_radar_rules(&content);

                // 缓存到本地
                let cache_data = json!({
                    "cachedAt": chrono::Utc::now().timestamp(),
                    "source": "radar-rules",
                    "routes": routes
                });

                let cache_path = paths().rsshub_routes_cache();
                let _ = tokio::fs::create_dir_all(paths().cache.clone()).await;
                let _ = tokio::fs::write(
                    &cache_path,
                    serde_json::to_string_pretty(&cache_data)
                        .unwrap_or_else(|_| cache_data.to_string()),
                )
                .await;

                return Ok(routes);
            }
        }
        _ => {}
    }

    // 如果获取失败，返回空数组
    Ok(json!([]))
}

async fn discover_rss_from_website(url: &str) -> Result<Vec<Value>, String> {
    let normalized_url = if url.starts_with("http://") || url.starts_with("https://") {
        url.to_string()
    } else if url.starts_with("www.") || looks_like_url(url) {
        format!("https://{}", url)
    } else {
        return Err("This URL is invalid".to_string());
    };

    let mut feeds = Vec::new();

    let (target, client) = crate::services::outbound_security::build_public_http_client(
        &normalized_url,
        std::time::Duration::from_secs(10),
        Some("Mozilla/5.0 (compatible; MyriadBot/1.0)"),
    )
    .await
    .map_err(|_e| "Invalid URL".to_string())?;

    let response = client.get(target).send().await.map_err(|e| {
        tracing::warn!(error = %e, "Request failed");
        "Request failed".to_string()
    })?;

    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }

    let bytes = crate::services::outbound_security::read_limited_body(response, 2 * 1024 * 1024)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "Failed to read response");
            "Failed to read response".to_string()
        })?;
    let html = String::from_utf8_lossy(&bytes).to_string();

    // 查找 RSS/Atom 链接
    let rss_re = regex::Regex::new(
        r#"<link[^>]*rel=[\"']alternate[\"'][^>]*type=[\"']application/(rss|atom)\+xml[\"'][^>]*href=[\"']([^\"']+)[\"']"#
    ).unwrap();

    let base_url = reqwest::Url::parse(&normalized_url).ok();

    for cap in rss_re.captures_iter(&html) {
        if let Some(href) = cap.get(2) {
            let feed_url = if href.as_str().starts_with("http") {
                href.as_str().to_string()
            } else if let Some(ref base) = base_url {
                base.join(href.as_str())
                    .map(|u| u.to_string())
                    .unwrap_or_default()
            } else {
                continue;
            };

            if let Ok(feed_info) = try_parse_feed(&feed_url).await {
                feeds.push(json!({
                    "url": feed_url,
                    "title": feed_info.get("title"),
                    "feedType": feed_info.get("feedType"),
                    "itemCount": feed_info.get("itemCount"),
                    "source": "website_autodiscover",
                    "verified": true
                }));
            }
        }
    }

    Ok(feeds)
}
