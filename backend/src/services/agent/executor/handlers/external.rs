//! 外部集成能力处理器
//!
//! 处理 http.fetch, hitokoto.get, weather.get 等外部 API 集成类能力。
//! 纯参数/体积分/MCP 过滤见 [`crate::services::agent::external_pure`]。

use super::HandlerContext;
use crate::services::agent::external_pure::{
    classify_outbound_fetch, compress_and_truncate_text, first_i64_param, first_string_param,
    hitokoto_type, http_body_size_error, http_fetch_method, match_mcp_capability_id, mcp_arguments,
    optional_string_param, parse_http_body_value, sanitize_http_headers, scrape_max_length,
    scrape_selector, scrape_should_skip_tag,
};
use crate::services::data_paths::platform_filtered_file;
use crate::services::fetcher::PlatformFetcher;
use crate::services::outbound_security;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::Duration;

const FETCH_USER_AGENT: &str = "Myriad Agent/1.0 (http.fetch)";
const SCRAPE_USER_AGENT: &str = "Mozilla/5.0 (compatible; Myriad/1.0)";

/// 构建仅允许公网目标的 HTTP 客户端（DNS 钉扎、禁止重定向 → 防 SSRF）
async fn public_client(
    url: &str,
    timeout: Duration,
    user_agent: &str,
) -> Result<(url::Url, reqwest::Client), String> {
    outbound_security::build_public_http_client(url, timeout, Some(user_agent))
        .await
        .map_err(|_e| "Invalid URL".to_string())
}

/// Fixed-host public APIs: no redirects, short timeout (not for user-supplied URLs).
fn fixed_host_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| {
            tracing::error!(error = %e, "HTTP client build failed");
            "HTTP client error".to_string()
        })
}

/// Default success body cap for fixed-host JSON APIs (hitokoto / weather / etc.).
const FIXED_HOST_JSON_MAX: usize = 512 * 1024;
const FIXED_HOST_ERR_MAX: usize = 64 * 1024;

async fn limited_json(response: reqwest::Response, max: usize) -> Result<Value, String> {
    let bytes = outbound_security::read_limited_body(response, max)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "Failed to read response");
            "Failed to read response".to_string()
        })?;
    serde_json::from_slice(&bytes).map_err(|e| {
        tracing::warn!(error = %e, "Failed to parse JSON");
        "Failed to parse JSON".to_string()
    })
}

async fn limited_error_text(response: reqwest::Response) -> String {
    outbound_security::read_limited_body(response, FIXED_HOST_ERR_MAX)
        .await
        .map(|b| String::from_utf8_lossy(&b).to_string())
        .unwrap_or_default()
}

fn reqwest_fetch_error(label: &str, error: reqwest::Error) -> String {
    tracing::warn!(%error, label, "outbound fetch failed");
    if let Some(status) = error.status() {
        return format!("{label} (HTTP {})", status.as_u16());
    }
    if error.is_timeout() {
        return format!("{label}: timed out");
    }
    if error.is_connect() {
        return format!("{label}: could not connect");
    }
    classify_outbound_fetch(label, &error.to_string())
}

fn display_fetch_error(label: &str, error: impl std::fmt::Display) -> String {
    let detail = error.to_string();
    tracing::warn!(error = %detail, label, "outbound fetch failed");
    classify_outbound_fetch(label, &detail)
}

/// 执行外部集成能力
pub async fn execute(
    capability_id: &str,
    params: &HashMap<String, Value>,
    _ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    match capability_id {
        "http.fetch" => execute_http_fetch(params).await,
        "hitokoto.get" => execute_hitokoto_get(params).await,
        "notion.query" => execute_notion_query(params).await,
        "bilibili.user" => execute_bilibili_user(params).await,
        "bilibili.video" => execute_bilibili_video(params).await,
        "bangumi.user" => execute_bangumi_user(params).await,
        "bangumi.collections" => execute_bangumi_collections(params).await,
        "steam.user" => execute_steam_user(params).await,
        "steam.game" => execute_steam_game(params).await,
        "proxy.image" => execute_proxy_image(params).await,
        "weather.get" => execute_weather_get(params).await,
        "netease.song" => execute_netease_song(params).await,
        "netease.playlist.detail" => execute_netease_playlist_detail(params).await,
        "web.scrape" => execute_web_scrape(params).await,
        cap_id if cap_id.starts_with("mcp.") => execute_mcp_tool(cap_id, params).await,
        _ => Err(format!("Unknown external capability: {}", capability_id)),
    }
}

// HTTP 通用

async fn execute_http_fetch(params: &HashMap<String, Value>) -> Result<Value, String> {
    let url = params
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or("Missing URL parameter")?;

    let method = http_fetch_method(params);

    let (target_url, client) =
        public_client(url, Duration::from_secs(30), FETCH_USER_AGENT).await?;
    let extra_headers = sanitize_http_headers(params.get("headers"));
    let response = match method {
        "POST" => {
            let body = params.get("body").cloned().unwrap_or(json!({}));
            let mut request = client.post(target_url).json(&body);
            for (name, value) in &extra_headers {
                request = request.header(name.as_str(), value.as_str());
            }
            request.send().await.map_err(|e| {
                tracing::warn!(error = %e, "HTTP request failed");
                "HTTP request failed".to_string()
            })?
        }
        _ => {
            let mut request = client.get(target_url);
            for (name, value) in &extra_headers {
                request = request.header(name.as_str(), value.as_str());
            }
            request.send().await.map_err(|e| {
                tracing::warn!(error = %e, "HTTP request failed");
                "HTTP request failed".to_string()
            })?
        }
    };

    let status = response.status().as_u16();
    // Stream-capped read: no full buffer when Content-Length is absent.
    let max = crate::services::agent::external_pure::HTTP_FETCH_MAX_BODY_BYTES as usize;
    let body_bytes = outbound_security::read_limited_body(response, max)
        .await
        .map_err(|e| {
            if e.contains("exceeds") {
                http_body_size_error()
            } else {
                tracing::warn!(error = %e, "Failed to read response");
                "Failed to read response".to_string()
            }
        })?;
    let body = String::from_utf8_lossy(&body_bytes).to_string();
    let data = parse_http_body_value(&body);

    Ok(json!({
        "status": status,
        "data": data
    }))
}

// 一言

async fn execute_hitokoto_get(params: &HashMap<String, Value>) -> Result<Value, String> {
    let hitokoto_type = hitokoto_type(params);

    let host = crate::api::config::HITOKOTO_BUILTIN_HOSTS[0];
    let url = match hitokoto_type {
        Some(t) => {
            let encoded = urlencoding::encode(t);
            format!("https://{host}/?c={encoded}")
        }
        None => format!("https://{host}/"),
    };

    let client = fixed_host_client()?;
    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|error| reqwest_fetch_error("Failed to fetch hitokoto", error))?;

    let data: Value = limited_json(response, FIXED_HOST_JSON_MAX).await?;

    Ok(json!({
        "content": data.get("hitokoto").and_then(|v| v.as_str()).unwrap_or(""),
        "from": data.get("from").and_then(|v| v.as_str()).unwrap_or(""),
        "from_who": data.get("from_who"),
        "type": data.get("type").and_then(|v| v.as_str()).unwrap_or("")
    }))
}

// Notion

async fn execute_notion_query(params: &HashMap<String, Value>) -> Result<Value, String> {
    let api_key =
        std::env::var("NOTION_API_KEY").map_err(|_| "Notion is not configured".to_string())?;

    let database_id = first_string_param(params, &["databaseId", "database_id"])
        .ok_or("Missing databaseId parameter")?;
    let filter = params.get("filter").cloned().unwrap_or(json!({}));

    let client = fixed_host_client()?;
    let url = format!("https://api.notion.com/v1/databases/{}/query", database_id);

    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Notion-Version", "2022-06-28")
        .header("Content-Type", "application/json")
        .json(&json!({ "filter": filter }))
        .send()
        .await
        .map_err(|e| {
            tracing::error!("Notion API request failed: {e}");
            "Failed to fetch Notion".to_string()
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = limited_error_text(response).await;
        tracing::error!("Notion API returned {status}: {body}");
        return Err("Failed to fetch Notion".to_string());
    }

    let data: Value = limited_json(response, FIXED_HOST_JSON_MAX).await?;

    let results_count = data
        .get("results")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);

    Ok(json!({
        "success": true,
        "database_id": database_id,
        "results": data.get("results").cloned().unwrap_or(json!([])),
        "count": results_count,
        "hasMore": data.get("has_more").and_then(|v| v.as_bool()).unwrap_or(false)
    }))
}

// Bilibili

async fn execute_bilibili_user(params: &HashMap<String, Value>) -> Result<Value, String> {
    let uid = first_string_param(params, &["uid"]).ok_or("Missing uid parameter")?;

    let url = format!("https://api.bilibili.com/x/space/acc/info?mid={}", uid);
    let client = fixed_host_client()?;

    let response = client
        .get(&url)
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .await
        .map_err(|error| reqwest_fetch_error("Failed to fetch Bilibili user", error))?;

    let data: Value = limited_json(response, FIXED_HOST_JSON_MAX).await?;

    if data.get("code").and_then(|v| v.as_i64()) == Some(0) {
        let user_data = data.get("data").cloned().unwrap_or(json!({}));
        Ok(json!({
            "uid": uid,
            "userInfo": user_data,
            "name": user_data.get("name"),
            "face": user_data.get("face"),
            "sign": user_data.get("sign"),
            "level": user_data.get("level"),
            "follower": user_data.get("follower"),
            "following": user_data.get("following")
        }))
    } else {
        Err(classify_outbound_fetch(
            "Failed to fetch Bilibili user",
            data.get("message")
                .and_then(|value| value.as_str())
                .unwrap_or(""),
        ))
    }
}

async fn execute_bilibili_video(params: &HashMap<String, Value>) -> Result<Value, String> {
    let bvid = first_string_param(params, &["bvid"]);
    let aid = first_i64_param(params, &["aid"]);

    let url = if let Some(bvid) = bvid {
        format!(
            "https://api.bilibili.com/x/web-interface/view?bvid={}",
            bvid
        )
    } else if let Some(aid) = aid {
        format!("https://api.bilibili.com/x/web-interface/view?aid={}", aid)
    } else {
        return Err("Missing bvid or aid parameter".to_string());
    };

    let client = fixed_host_client()?;
    let response = client
        .get(&url)
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .await
        .map_err(|error| reqwest_fetch_error("Failed to fetch Bilibili video", error))?;

    let data: Value = limited_json(response, FIXED_HOST_JSON_MAX).await?;

    if data.get("code").and_then(|v| v.as_i64()) == Some(0) {
        let video = data.get("data").cloned().unwrap_or(json!({}));
        Ok(json!({
            "bvid": video.get("bvid"),
            "aid": video.get("aid"),
            "title": video.get("title"),
            "desc": video.get("desc"),
            "pic": video.get("pic"),
            "owner": video.get("owner"),
            "stat": video.get("stat"),
            "duration": video.get("duration")
        }))
    } else {
        Err(classify_outbound_fetch(
            "Failed to fetch Bilibili video",
            data.get("message")
                .and_then(|value| value.as_str())
                .unwrap_or(""),
        ))
    }
}

// Bangumi

async fn execute_bangumi_user(params: &HashMap<String, Value>) -> Result<Value, String> {
    let username = optional_string_param(params, "username").ok_or("Missing username parameter")?;
    let access_token = optional_string_param(params, "access_token");
    let user_agent = optional_string_param(params, "user_agent");
    let fetcher = PlatformFetcher::new().await;

    let user = fetcher
        .fetch_bangumi_user(&username, access_token.as_deref(), user_agent.as_deref())
        .await
        .map_err(|error| display_fetch_error("Failed to fetch Bangumi user", error))?;

    Ok(json!({
        "username": user.get("username"),
        "nickname": user.get("nickname"),
        "avatar": user.get("avatar"),
        "sign": user.get("sign"),
        "userInfo": user
    }))
}

async fn execute_bangumi_collections(params: &HashMap<String, Value>) -> Result<Value, String> {
    let username = optional_string_param(params, "username").ok_or("Missing username parameter")?;
    let access_token = optional_string_param(params, "access_token");
    let user_agent = optional_string_param(params, "user_agent");
    let fetcher = PlatformFetcher::new().await;

    let collections = fetcher
        .fetch_bangumi_collections(&username, access_token.as_deref(), user_agent.as_deref())
        .await
        .map_err(|error| display_fetch_error("Failed to fetch Bangumi collections", error))?;
    let total = collections.len();

    Ok(json!({
        "username": username,
        "items": collections,
        "total": total
    }))
}

// Steam

async fn execute_steam_user(_params: &HashMap<String, Value>) -> Result<Value, String> {
    if let Ok(content) = tokio::fs::read_to_string(platform_filtered_file("steam")).await {
        if let Ok(data) = serde_json::from_str::<Value>(&content) {
            return Ok(json!({
                "userInfo": data,
                "cached_data": data,
                "source": "local_cache"
            }));
        }
    }

    Err("No local Steam cache. Sync Steam on the platform page first.".to_string())
}

// 图片代理

async fn execute_proxy_image(params: &HashMap<String, Value>) -> Result<Value, String> {
    let url = params
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or("Missing url parameter")?;
    let platform = params
        .get("platform")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    // Hotlink hosts → /api/proxy/image; other http:// and // upgrade to https.
    let proxy_url = myriad_image_proxy::proxy_image_url(url);

    Ok(json!({
        "originalUrl": url,
        "proxyUrl": proxy_url,
        "platform": platform,
        "cached": false,
        "message": "Use proxyUrl for display (proxied only when host needs it)"
    }))
}

// 天气

async fn execute_weather_get(params: &HashMap<String, Value>) -> Result<Value, String> {
    let city =
        first_string_param(params, &["city", "location", "q"]).ok_or("Missing city parameter")?;

    let url = format!("https://wttr.in/{}?format=j1", urlencoding::encode(&city));
    let client = fixed_host_client()?;

    let response = client
        .get(&url)
        .header("User-Agent", "curl/7.68.0")
        .send()
        .await
        .map_err(|error| reqwest_fetch_error("Failed to fetch weather", error))?;

    let data: Value = limited_json(response, FIXED_HOST_JSON_MAX).await?;

    let current = data
        .get("current_condition")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .ok_or("No current weather data")?;

    Ok(json!({
        "city": city,
        "temperature": current.get("temp_C").and_then(|v| v.as_str()),
        "feelsLike": current.get("FeelsLikeC").and_then(|v| v.as_str()),
        "humidity": current.get("humidity").and_then(|v| v.as_str()),
        "weather": current.get("weatherDesc")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.get("value"))
            .and_then(|v| v.as_str()),
        "windSpeed": current.get("windspeedKmph").and_then(|v| v.as_str()),
        "visibility": current.get("visibility").and_then(|v| v.as_str())
    }))
}

// 网易云音乐

async fn execute_netease_song(params: &HashMap<String, Value>) -> Result<Value, String> {
    let song_id = first_i64_param(params, &["songId", "song_id"]).ok_or("Missing songId")?;
    let include_lyrics = params
        .get("includeLyrics")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let url = format!("https://music.163.com/api/song/detail?ids=[{}]", song_id);
    let client = fixed_host_client()?;

    let response = client
        .get(&url)
        .header("User-Agent", "Mozilla/5.0")
        .header("Referer", "https://music.163.com")
        .send()
        .await
        .map_err(|error| reqwest_fetch_error("Failed to fetch Netease song", error))?;

    let data: Value = limited_json(response, FIXED_HOST_JSON_MAX).await?;

    let song = data
        .get("songs")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .cloned()
        .unwrap_or(json!({}));

    let mut result = json!({
        "songId": song_id,
        "song": song
    });

    if include_lyrics {
        let lyrics_url = format!("https://music.163.com/api/song/lyric?id={}&lv=1", song_id);
        if let Ok(lyrics_resp) = client.get(&lyrics_url).send().await {
            if let Ok(lyrics_data) = lyrics_resp.json::<Value>().await {
                result["lyrics"] = lyrics_data
                    .get("lrc")
                    .and_then(|v| v.get("lyric"))
                    .cloned()
                    .unwrap_or(json!(""));
            }
        }
    }

    Ok(result)
}

async fn execute_netease_playlist_detail(params: &HashMap<String, Value>) -> Result<Value, String> {
    let playlist_id =
        first_i64_param(params, &["playlistId", "playlist_id"]).ok_or("Missing playlistId")?;

    let url = format!(
        "https://music.163.com/api/playlist/detail?id={}",
        playlist_id
    );
    let client = fixed_host_client()?;

    let response = client
        .get(&url)
        .header("User-Agent", "Mozilla/5.0")
        .header("Referer", "https://music.163.com")
        .send()
        .await
        .map_err(|error| reqwest_fetch_error("Failed to fetch Netease playlist", error))?;

    let data: Value = limited_json(response, FIXED_HOST_JSON_MAX).await?;

    let result = data.get("result").cloned().unwrap_or(json!({}));

    Ok(json!({
        "playlistId": playlist_id,
        "name": result.get("name"),
        "description": result.get("description"),
        "coverUrl": result.get("coverImgUrl"),
        "trackCount": result.get("trackCount"),
        "playCount": result.get("playCount"),
        "creator": result.get("creator").and_then(|c| c.get("nickname")),
        "tracks": result.get("tracks")
    }))
}

// Steam 游戏详情

/// Steam 游戏详情查询
async fn execute_steam_game(params: &HashMap<String, Value>) -> Result<Value, String> {
    let app_id = first_i64_param(params, &["appId", "app_id"]).ok_or("Missing appId")?;

    let url = format!(
        "https://store.steampowered.com/api/appdetails?appids={}",
        app_id
    );
    let client = fixed_host_client()?;

    match client.get(&url).send().await {
        Ok(response) => {
            if let Ok(data) = limited_json(response, FIXED_HOST_JSON_MAX).await {
                let app_data = data
                    .get(app_id.to_string())
                    .and_then(|v| v.get("data"))
                    .cloned()
                    .unwrap_or(json!({}));

                return Ok(json!({
                    "appId": app_id,
                    "name": app_data.get("name"),
                    "description": app_data.get("short_description"),
                    "developers": app_data.get("developers"),
                    "publishers": app_data.get("publishers"),
                    "genres": app_data.get("genres"),
                    "categories": app_data.get("categories"),
                    "headerImage": app_data.get("header_image"),
                    "price": app_data.get("price_overview"),
                    "releaseDate": app_data.get("release_date"),
                    "platforms": app_data.get("platforms"),
                    "metacritic": app_data.get("metacritic")
                }));
            }
            Err("Failed to fetch game details".to_string())
        }
        Err(error) => Err(reqwest_fetch_error("Failed to fetch Steam game", error)),
    }
}

// Web Scrape

/// 抓取外部网页并提取可读文本内容
async fn execute_web_scrape(params: &HashMap<String, Value>) -> Result<Value, String> {
    let url = params
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or("Missing url parameter")?;

    // SSRF 防护：公网 DNS 钉扎、禁止重定向（不再 follow 到内网）
    let (target_url, client) =
        public_client(url, Duration::from_secs(15), SCRAPE_USER_AGENT).await?;

    let selector_str = scrape_selector(params);
    let max_length = scrape_max_length(params);

    let response = client
        .get(target_url)
        .header("Accept", "text/html,application/xhtml+xml,*/*")
        .send()
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "Fetch failed");
            "Fetch failed".to_string()
        })?;

    let status = response.status().as_u16();
    if status >= 400 {
        return Err(format!("HTTP {status}"));
    }

    // Stream-capped (do not `.text()` then reject — still OOMs on huge pages).
    let max_html = crate::services::agent::external_pure::WEB_SCRAPE_MAX_HTML_BYTES;
    let html_bytes = outbound_security::read_limited_body(response, max_html)
        .await
        .map_err(|e| {
            if e.contains("exceeds") {
                "Page too large (>5MB)".to_string()
            } else {
                tracing::warn!(error = %e, "Read failed");
                "Read failed".to_string()
            }
        })?;
    let html = String::from_utf8_lossy(&html_bytes).to_string();

    let document = scraper::Html::parse_document(&html);

    // 提取标题
    let title = scraper::Selector::parse("title")
        .ok()
        .and_then(|s| document.select(&s).next())
        .map(|el| el.text().collect::<String>().trim().to_string());

    // Extract text; skip nodes whose direct parent is script|style|noscript|svg|iframe.
    let sel = scraper::Selector::parse(selector_str)
        .map_err(|_| format!("Invalid CSS selector: {}", selector_str))?;

    let text: String = document
        .select(&sel)
        .flat_map(|el| {
            el.descendants().filter_map(|node| {
                match node.value() {
                    scraper::node::Node::Text(t) => {
                        // 检查父元素是否为应跳过的标签
                        let parent_tag = node
                            .parent()
                            .and_then(|p| p.value().as_element())
                            .map(|e| e.name());
                        if let Some(tag) = parent_tag {
                            if scrape_should_skip_tag(tag) {
                                return None;
                            }
                        }
                        let s = t.trim();
                        if s.is_empty() {
                            None
                        } else {
                            Some(s.to_string())
                        }
                    }
                    _ => None,
                }
            })
        })
        .collect::<Vec<_>>()
        .join(" ");

    let (text, truncated) = compress_and_truncate_text(&text, max_length);

    // 提取 meta description 作为额外上下文
    let description = scraper::Selector::parse("meta[name=description]")
        .ok()
        .and_then(|s| document.select(&s).next())
        .and_then(|el| el.value().attr("content"))
        .map(|s| s.to_string());

    Ok(json!({
        "url": url,
        "title": title,
        "description": description,
        "content": text,
        "length": text.len(),
        "truncated": truncated
    }))
}

// MCP Tool Dispatch

/// 调用 MCP 服务器工具
async fn execute_mcp_tool(
    capability_id: &str,
    params: &HashMap<String, Value>,
) -> Result<Value, String> {
    // capability_id 格式: "mcp.{server_id}.{tool_name}"。server_id 可以带 `.`，
    // 必须对着已广告的 (server, tool) 对精确匹配，不能 split_once。
    let manager =
        crate::services::agent::mcp::get_mcp_manager().ok_or("MCP manager not initialized")?;
    let advertised: Vec<(String, String)> = manager
        .list_tools()
        .await
        .into_iter()
        .map(|(server_id, tool)| (server_id, tool.name))
        .collect();
    let (server_id, tool_name) = match_mcp_capability_id(capability_id, &advertised)?;

    // Executor-only context keys must never cross the MCP trust boundary or
    // violate tools that declare `additionalProperties: false`.
    let args = mcp_arguments(params);
    manager.call_tool(&server_id, &tool_name, args).await
}
