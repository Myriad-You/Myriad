// Platform data fetching service
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

use super::bilibili_utils::{generate_bilibili_cookie, get_random_china_ip, get_random_user_agent};
use crate::services::http_client::{get_global_client, GitHubApiUrl};

pub struct PlatformFetcher {
    client: reqwest::Client,
}

// Bilibili 数据结构
#[derive(Debug, Serialize, Deserialize)]
pub struct BilibiliUserInfo {
    pub mid: i64,
    pub name: String,
    pub face: String,
    pub sign: String,
    pub level: i32,
    pub following: i64,
    pub follower: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BilibiliFavorite {
    pub id: i64,
    pub title: String,
    pub cover: String,
    pub intro: String,
    pub media_count: i32,
    pub fav_state: i32,
    pub videos: Vec<BilibiliFavoriteVideo>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BilibiliFavoriteVideo {
    pub bvid: String,
    pub title: String,
    pub cover: String,
    pub intro: String,
    pub upper_name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BilibiliBangumi {
    pub season_id: i64,
    pub title: String,
    pub cover: String,
    pub season_type: i32, // 1: 动画, 2: 电影, 3: 纪录片, 4: 国创, 5: 电视剧
    pub progress: String,
    pub badge: String,
}

// Steam 数据结构
#[derive(Debug, Serialize, Deserialize)]
pub struct SteamUserInfo {
    pub steamid: String,
    pub personaname: String,
    pub profileurl: String,
    pub avatar: String,
    pub avatarfull: String,
    pub timecreated: Option<i64>,
    pub communityvisibilitystate: Option<i32>,
    pub personastate: i32,
    pub personastate_label: String,
    pub lastlogoff: Option<i64>,
    pub gameid: Option<String>,
    pub gameextrainfo: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SteamGame {
    pub appid: i64,
    pub name: String,
    pub playtime_forever: i32, // 分钟
    pub playtime_2weeks: Option<i32>,
    pub img_icon_url: String,
    pub img_logo_url: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SteamWishlistItem {
    pub appid: i64,
    pub name: String,
    pub capsule: String,
    pub review_score: i32,
    pub review_desc: String,
    pub priority: i32,
}

const BANGUMI_API_BASE: &str = "https://api.bgm.tv";
const DEFAULT_BANGUMI_USER_AGENT: &str = "haru/Myriad";

fn steam_persona_state_label(state: i32) -> &'static str {
    match state {
        0 => "offline",
        1 => "online",
        2 => "busy",
        3 => "away",
        4 => "snooze",
        5 => "looking_to_trade",
        6 => "looking_to_play",
        _ => "unknown",
    }
}

fn optional_json_string(value: &serde_json::Value) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| value.as_i64().map(|value| value.to_string()))
}

impl PlatformFetcher {
    pub async fn new() -> Self {
        Self {
            client: get_global_client().await,
        }
    }

    // ==================== Bilibili API ====================

    /// 获取 Bilibili 用户基本信息
    pub async fn fetch_bilibili_user(&self, uid: i64) -> Result<BilibiliUserInfo> {
        // 使用不需要WBI签名的旧API端点
        let url = format!("https://api.bilibili.com/x/space/acc/info?mid={}", uid);

        // IP 伪装
        let client_ip = get_random_china_ip();
        let proxy_ip = get_random_china_ip();
        let forwarded_for = format!("{}, {}", client_ip, proxy_ip);

        let response: serde_json::Value = self
            .client
            .get(&url)
            .header("User-Agent", get_random_user_agent())
            .header("Referer", "https://www.bilibili.com")
            .header("Origin", "https://www.bilibili.com")
            .header("Accept", "application/json, text/plain, */*")
            .header("Accept-Language", "zh-CN,zh;q=0.9,en-US;q=0.8,en;q=0.7")
            .header("Cookie", generate_bilibili_cookie())
            .header("X-Forwarded-For", forwarded_for)
            .header("X-Real-IP", client_ip)
            .send()
            .await?
            .json()
            .await?;

        if response["code"].as_i64() != Some(0) {
            return Err(anyhow!("Bilibili API error: {}", response["message"]));
        }

        let data = &response["data"];
        Ok(BilibiliUserInfo {
            mid: data["mid"].as_i64().unwrap_or(0),
            name: data["name"].as_str().unwrap_or("").to_string(),
            face: data["face"].as_str().unwrap_or("").to_string(),
            sign: data["sign"].as_str().unwrap_or("").to_string(),
            level: data["level"].as_i64().unwrap_or(0) as i32,
            following: data["following"].as_i64().unwrap_or(0),
            follower: data["follower"].as_i64().unwrap_or(0),
        })
    }

    /// 获取收藏夹内容
    async fn fetch_favorite_videos(&self, fav_id: i64) -> Result<Vec<BilibiliFavoriteVideo>> {
        let url = format!(
            "https://api.bilibili.com/x/v3/fav/resource/list?media_id={}&pn=1&ps=20",
            fav_id
        );

        // IP 伪装
        let client_ip = get_random_china_ip();
        let proxy_ip = get_random_china_ip();
        let forwarded_for = format!("{}, {}", client_ip, proxy_ip);

        let response: serde_json::Value = self
            .client
            .get(&url)
            .header("User-Agent", get_random_user_agent())
            .header("Referer", "https://www.bilibili.com")
            .header("Accept", "application/json, text/plain, */*")
            .header("Accept-Language", "zh-CN,zh;q=0.9,en-US;q=0.8,en;q=0.7")
            .header("Cookie", generate_bilibili_cookie())
            .header("X-Forwarded-For", forwarded_for)
            .header("X-Real-IP", client_ip)
            .send()
            .await?
            .json()
            .await?;

        if response["code"].as_i64() != Some(0) {
            return Ok(Vec::new()); // 如果获取失败，返回空列表而不是错误
        }

        let medias = response["data"]["medias"]
            .as_array()
            .cloned()
            .unwrap_or_default();

        let videos: Vec<BilibiliFavoriteVideo> = medias
            .iter()
            .filter_map(|media| {
                Some(BilibiliFavoriteVideo {
                    bvid: media["bvid"].as_str()?.to_string(),
                    title: media["title"].as_str()?.to_string(),
                    cover: media["cover"].as_str().unwrap_or("").to_string(),
                    intro: media["intro"].as_str().unwrap_or("").to_string(),
                    upper_name: media["upper"]["name"].as_str().unwrap_or("").to_string(),
                })
            })
            .collect();

        Ok(videos)
    }

    /// 获取 Bilibili 收藏夹列表（包含内容）
    pub async fn fetch_bilibili_favorites(&self, uid: i64) -> Result<Vec<BilibiliFavorite>> {
        let url = format!(
            "https://api.bilibili.com/x/v3/fav/folder/created/list-all?up_mid={}",
            uid
        );

        // IP 伪装
        let client_ip = get_random_china_ip();
        let proxy_ip = get_random_china_ip();
        let forwarded_for = format!("{}, {}", client_ip, proxy_ip);

        let response: serde_json::Value = self
            .client
            .get(&url)
            .header("User-Agent", get_random_user_agent())
            .header("Referer", "https://www.bilibili.com")
            .header("Accept", "application/json, text/plain, */*")
            .header("Accept-Language", "zh-CN,zh;q=0.9,en-US;q=0.8,en;q=0.7")
            .header("Cookie", generate_bilibili_cookie())
            .header("X-Forwarded-For", forwarded_for)
            .header("X-Real-IP", client_ip)
            .send()
            .await?
            .json()
            .await?;

        if response["code"].as_i64() != Some(0) {
            return Err(anyhow!("Bilibili API error: {}", response["message"]));
        }

        let list = response["data"]["list"]
            .as_array()
            .ok_or_else(|| anyhow!("Invalid response format"))?;

        let mut favorites: Vec<BilibiliFavorite> = Vec::new();

        for item in list.iter().take(5) {
            // 只获取前5个收藏夹
            if let Some(fav_id) = item["id"].as_i64() {
                let videos = self.fetch_favorite_videos(fav_id).await.unwrap_or_default();

                favorites.push(BilibiliFavorite {
                    id: fav_id,
                    title: item["title"].as_str().unwrap_or("").to_string(),
                    cover: item["cover"].as_str().unwrap_or("").to_string(),
                    intro: item["intro"].as_str().unwrap_or("").to_string(),
                    media_count: item["media_count"].as_i64().unwrap_or(0) as i32,
                    fav_state: item["fav_state"].as_i64().unwrap_or(0) as i32,
                    videos,
                });

                // 避免请求过快 - 增加延迟到800-1200毫秒
                let delay = 800 + (rand::random::<u64>() % 400);
                tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
            }
        }

        Ok(favorites)
    }

    /// 获取 Bilibili 追番列表（动画）
    pub async fn fetch_bilibili_bangumi(
        &self,
        uid: i64,
        bangumi_type: i32,
    ) -> Result<Vec<BilibiliBangumi>> {
        // 使用 vmid 参数获取追番数据
        // type: 1=番剧(动画), 2=电影, 3=纪录片, 4=国创, 5=电视剧, 7=综艺
        // 注意:参数顺序很重要,follow_status参数会导致-400错误
        let url = format!(
            "https://api.bilibili.com/x/space/bangumi/follow/list?vmid={}&pn=1&ps=15&type={}",
            uid, bangumi_type
        );

        tracing::info!(
            "Fetching Bilibili bangumi type {} from: {}",
            bangumi_type,
            url
        );

        // IP 伪装 - 关键改进
        let client_ip = get_random_china_ip();
        let proxy_ip = get_random_china_ip();
        let forwarded_for = format!("{}, {}", client_ip, proxy_ip);

        let http_response = self
            .client
            .get(&url)
            .header("User-Agent", get_random_user_agent())
            .header("Referer", format!("https://space.bilibili.com/{}", uid))
            .header("Cookie", generate_bilibili_cookie())
            .header("X-Forwarded-For", forwarded_for)
            .header("X-Real-IP", client_ip)
            .send()
            .await?;

        // 先获取文本以调试
        let response_text = http_response.text().await?;
        tracing::debug!(
            "Bilibili bangumi raw response (first 500 chars): {}",
            &response_text.chars().take(500).collect::<String>()
        );

        let response: serde_json::Value = serde_json::from_str(&response_text)?;

        // 打印完整响应以调试
        tracing::info!(
            "Bilibili bangumi API response for type {}: {}",
            bangumi_type,
            serde_json::to_string_pretty(&response).unwrap_or_default()
        );

        if response["code"].as_i64() != Some(0) {
            tracing::warn!(
                "Bilibili bangumi API returned code: {}, message: {}",
                response["code"],
                response["message"]
            );
            return Ok(Vec::new()); // 返回空列表而不是错误
        }

        // 检查 data 字段是否存在
        let data = match response.get("data") {
            Some(d) => d,
            None => {
                tracing::warn!("Bilibili bangumi response missing 'data' field");
                return Ok(Vec::new());
            }
        };

        // 检查 list 字段
        let list = match data.get("list") {
            Some(l) => match l.as_array() {
                Some(arr) => arr,
                None => {
                    tracing::warn!("Bilibili bangumi 'list' is not an array, got: {:?}", l);
                    return Ok(Vec::new());
                }
            },
            None => {
                tracing::warn!(
                    "Bilibili bangumi data missing 'list' field, data: {:?}",
                    data
                );
                return Ok(Vec::new());
            }
        };

        let bangumi: Vec<BilibiliBangumi> = list
            .iter()
            .filter_map(|item| {
                Some(BilibiliBangumi {
                    season_id: item["season_id"].as_i64()?,
                    title: item["title"].as_str()?.to_string(),
                    cover: item["cover"].as_str().unwrap_or("").to_string(),
                    season_type: bangumi_type,
                    progress: item["progress"].as_str().unwrap_or("").to_string(),
                    badge: item["badge"].as_str().unwrap_or("").to_string(),
                })
            })
            .collect();

        tracing::info!(
            "Bilibili bangumi type {} fetched: {} items",
            bangumi_type,
            bangumi.len()
        );
        Ok(bangumi)
    }

    /// 获取所有 Bilibili 追番/追剧数据
    pub async fn fetch_all_bilibili_bangumi(&self, uid: i64) -> Result<Vec<BilibiliBangumi>> {
        let mut all_bangumi = Vec::new();

        // 1: 番剧(动画), 2: 电影
        // 移除 3: 纪录片, 4: 国创, 5: 电视剧 以减少请求数量
        for bangumi_type in [1, 2] {
            match self.fetch_bilibili_bangumi(uid, bangumi_type).await {
                Ok(mut items) => all_bangumi.append(&mut items),
                Err(e) => tracing::warn!(
                    "Failed to fetch Bilibili bangumi type {}: {}",
                    bangumi_type,
                    e
                ),
            }
            // 避免请求过快 - 增加延迟到1.5-2.5秒之间
            let delay = 1500 + (rand::random::<u64>() % 1000);
            tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
        }

        Ok(all_bangumi)
    }

    // ==================== Steam API ====================

    /// 获取 Steam 用户信息
    pub async fn fetch_steam_user(&self, api_key: &str, steam_id: &str) -> Result<SteamUserInfo> {
        let url = format!(
            "https://api.steampowered.com/ISteamUser/GetPlayerSummaries/v2/?key={}&steamids={}",
            api_key, steam_id
        );

        let response: serde_json::Value = self.client.get(&url).send().await?.json().await?;

        let players = response["response"]["players"]
            .as_array()
            .ok_or_else(|| anyhow!("No player data found"))?;

        if players.is_empty() {
            return Err(anyhow!("Steam user not found"));
        }

        let player = &players[0];
        Ok(SteamUserInfo {
            steamid: player["steamid"].as_str().unwrap_or("").to_string(),
            personaname: player["personaname"].as_str().unwrap_or("").to_string(),
            profileurl: player["profileurl"].as_str().unwrap_or("").to_string(),
            avatar: player["avatar"].as_str().unwrap_or("").to_string(),
            avatarfull: player["avatarfull"].as_str().unwrap_or("").to_string(),
            timecreated: player["timecreated"].as_i64(),
            communityvisibilitystate: player["communityvisibilitystate"]
                .as_i64()
                .map(|value| value as i32),
            personastate: player["personastate"].as_i64().unwrap_or(0) as i32,
            personastate_label: steam_persona_state_label(
                player["personastate"].as_i64().unwrap_or(0) as i32,
            )
            .to_string(),
            lastlogoff: player["lastlogoff"].as_i64(),
            gameid: optional_json_string(&player["gameid"]),
            gameextrainfo: optional_json_string(&player["gameextrainfo"]),
        })
    }

    /// 获取 Steam 游戏库
    pub async fn fetch_steam_games(&self, api_key: &str, steam_id: &str) -> Result<Vec<SteamGame>> {
        let url = format!(
            "https://api.steampowered.com/IPlayerService/GetOwnedGames/v1/?key={}&steamid={}&include_appinfo=1&include_played_free_games=1",
            api_key, steam_id
        );

        let response: serde_json::Value = self.client.get(&url).send().await?.json().await?;

        let games = response["response"]["games"]
            .as_array()
            .ok_or_else(|| anyhow!("No games data found"))?;

        let game_list: Vec<SteamGame> = games
            .iter()
            .filter_map(|game| {
                Some(SteamGame {
                    appid: game["appid"].as_i64()?,
                    name: game["name"].as_str()?.to_string(),
                    playtime_forever: game["playtime_forever"].as_i64().unwrap_or(0) as i32,
                    playtime_2weeks: game["playtime_2weeks"].as_i64().map(|v| v as i32),
                    img_icon_url: game["img_icon_url"].as_str().unwrap_or("").to_string(),
                    img_logo_url: game["img_logo_url"].as_str().unwrap_or("").to_string(),
                })
            })
            .collect();

        Ok(game_list)
    }

    /// 获取近两周游玩总时长（分钟）
    /// 使用 GetRecentlyPlayedGames，仅返回最近游玩的游戏，负载远小于整库
    pub async fn fetch_steam_recent_playtime(&self, api_key: &str, steam_id: &str) -> Result<i32> {
        let url = format!(
            "https://api.steampowered.com/IPlayerService/GetRecentlyPlayedGames/v1/?key={}&steamid={}",
            api_key, steam_id
        );

        let response: serde_json::Value = self.client.get(&url).send().await?.json().await?;

        let total_minutes = response["response"]["games"]
            .as_array()
            .map(|games| {
                games
                    .iter()
                    .map(|game| game["playtime_2weeks"].as_i64().unwrap_or(0) as i32)
                    .sum()
            })
            .unwrap_or(0);

        Ok(total_minutes)
    }

    /// 获取 Steam 愿望单
    pub async fn fetch_steam_wishlist(&self, steam_id: &str) -> Result<Vec<SteamWishlistItem>> {
        let url = format!(
            "https://store.steampowered.com/wishlist/profiles/{}/wishlistdata/",
            steam_id
        );

        let response: serde_json::Value = self.client.get(&url).send().await?.json().await?;

        let mut wishlist = Vec::new();

        if let Some(obj) = response.as_object() {
            for (appid_str, item) in obj {
                if let Ok(appid) = appid_str.parse::<i64>() {
                    wishlist.push(SteamWishlistItem {
                        appid,
                        name: item["name"].as_str().unwrap_or("").to_string(),
                        capsule: item["capsule"].as_str().unwrap_or("").to_string(),
                        review_score: item["review_score"].as_i64().unwrap_or(0) as i32,
                        review_desc: item["review_desc"].as_str().unwrap_or("").to_string(),
                        priority: item["priority"].as_i64().unwrap_or(0) as i32,
                    });
                }
            }
        }

        // 按优先级排序
        wishlist.sort_by_key(|a| a.priority);

        Ok(wishlist)
    }

    // ==================== GitHub API ====================

    /// 获取 GitHub 用户信息（包含粉丝数、仓库数等）
    pub async fn fetch_github_user(
        &self,
        username: &str,
        token: Option<&str>,
    ) -> Result<serde_json::Value> {
        let url = GitHubApiUrl::user_url(username).await;
        let mut request = self
            .client
            .get(&url)
            .header("User-Agent", "Myriad")
            .header("Accept", "application/vnd.github.v3+json");

        if let Some(token) = token {
            request = request.header("Authorization", format!("token {}", token));
        }

        let response = request.send().await?;

        if !response.status().is_success() {
            return Err(anyhow!("GitHub API error: {}", response.status()));
        }

        Ok(response.json().await?)
    }

    /// 获取 GitHub 用户的所有公开仓库
    pub async fn fetch_github_repos(
        &self,
        username: &str,
        token: Option<&str>,
    ) -> Result<Vec<serde_json::Value>> {
        let mut all_repos = Vec::new();
        let mut page: u32 = 1;

        loop {
            let url = GitHubApiUrl::user_repos_url(username, page).await;
            let mut request = self
                .client
                .get(&url)
                .header("User-Agent", "Myriad")
                .header("Accept", "application/vnd.github.v3+json");

            if let Some(token) = token {
                request = request.header("Authorization", format!("token {}", token));
            }

            let response = request.send().await?;

            if !response.status().is_success() {
                return Err(anyhow!("GitHub API error: {}", response.status()));
            }

            let repos: Vec<serde_json::Value> = response.json().await?;

            if repos.is_empty() {
                break;
            }

            all_repos.extend(repos);
            page += 1;

            // GitHub API 限流保护
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        }

        Ok(all_repos)
    }

    /// 获取 GitHub 贡献日历数据（通过爬取用户页面）
    pub async fn fetch_github_contributions(
        &self,
        username: &str,
        _token: Option<&str>,
    ) -> Result<Vec<serde_json::Value>> {
        tracing::info!("🔄 Fetching GitHub contributions for: {}", username);

        // 获取用户的贡献图表 SVG 片段
        let url = format!("https://github.com/users/{}/contributions", username);

        let response = self
            .client
            .get(&url)
            .header(
                "User-Agent",
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
            )
            .header("Accept", "text/html,application/xhtml+xml")
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(anyhow!(
                "Failed to fetch GitHub contributions: {}",
                response.status()
            ));
        }

        let html = response.text().await?;

        tracing::debug!(
            "Fetched GitHub contributions HTML, length: {} bytes",
            html.len()
        );

        // 解析 SVG 中的 <rect> 或 <td> 标签提取贡献数据
        // GitHub 可能使用 rect 或 table 格式
        let mut contributions = Vec::new();

        // 使用更稳健的解析策略：先匹配标签，再提取属性
        // 这样可以忽略属性顺序和中间的其他属性
        let tag_re = regex::Regex::new(r#"<(?:rect|td)([^>]+)>"#)?;
        let date_re = regex::Regex::new(r#"data-date="([0-9]{4}-[0-9]{2}-[0-9]{2})""#)?;
        let level_re = regex::Regex::new(r#"data-level="(\d+)""#)?;

        for cap in tag_re.captures_iter(&html) {
            let attrs = cap.get(1).map(|m| m.as_str()).unwrap_or("");

            // 必须同时包含 data-date 和 data-level
            if let (Some(date_cap), Some(level_cap)) =
                (date_re.captures(attrs), level_re.captures(attrs))
            {
                let date = date_cap.get(1).map(|m| m.as_str()).unwrap_or("");
                let level = level_cap
                    .get(1)
                    .and_then(|m| m.as_str().parse::<i64>().ok())
                    .unwrap_or(0);

                // 将 level (0-4) 转换为近似的贡献数
                let count = match level {
                    0 => 0,
                    1 => 2,
                    2 => 5,
                    3 => 8,
                    _ => 12,
                };

                contributions.push(serde_json::json!({
                    "date": date,
                    "count": count
                }));
            }
        }

        tracing::debug!("Extracted {} contribution days", contributions.len());

        if contributions.is_empty() {
            // 保存HTML用于调试
            let debug_path = "cache/debug_github_contributions.html";
            if let Err(e) = std::fs::write(debug_path, &html) {
                tracing::warn!("Failed to write debug HTML: {}", e);
            } else {
                tracing::warn!("No contribution data found. HTML saved to {}", debug_path);
            }

            tracing::warn!(
                "No contribution data found. HTML preview: {}",
                &html.chars().take(500).collect::<String>()
            );
            return Err(anyhow!("No contribution data found in HTML"));
        }

        // ✅ 返回完整的贡献历史数据（365天），而非截断
        // 前端会在显示热力图时只取最近60天，但计算总贡献数需要完整数据
        let total_days = contributions.len();
        let total_contributions: i64 = contributions
            .iter()
            .filter_map(|c| c.get("count").and_then(|v| v.as_i64()))
            .sum();

        tracing::info!(
            "✅ Returning {} contribution days with total {} contributions",
            total_days,
            total_contributions
        );
        Ok(contributions)
    }

    // ==================== Netease Cloud Music API ====================
    // ✅ 已重构：使用统一的 NeteaseService 服务层
    // - 自动享受防封技术（IP伪装、随机User-Agent）
    // - 支持大歌单（1000+首歌曲）
    // - VIP歌曲检测
    // - 缓存和限流保护

    /// 获取网易云音乐用户的喜欢列表（我喜欢的音乐）- 使用统一服务层
    pub async fn fetch_netease_liked_songs(&self, user_id: i64) -> Result<Vec<serde_json::Value>> {
        let netease_service = crate::services::netease_service::NeteaseService::new();
        netease_service.fetch_user_liked_songs(user_id).await
    }

    /// 获取网易云音乐用户基本信息（用于验证）- 使用统一服务层
    pub async fn fetch_netease_user(&self, user_id: i64) -> Result<serde_json::Value> {
        let netease_service = crate::services::netease_service::NeteaseService::new();
        netease_service.fetch_user_info(user_id).await
    }

    // ==================== Bangumi API ====================

    fn bangumi_user_agent(user_agent: Option<&str>) -> &str {
        user_agent
            .filter(|ua| !ua.trim().is_empty())
            .unwrap_or(DEFAULT_BANGUMI_USER_AGENT)
    }

    fn bangumi_request(
        &self,
        url: &str,
        access_token: Option<&str>,
        user_agent: Option<&str>,
    ) -> reqwest::RequestBuilder {
        let mut request = self
            .client
            .get(url)
            .header("User-Agent", Self::bangumi_user_agent(user_agent))
            .header("Accept", "application/json");

        if let Some(token) = access_token.filter(|token| !token.trim().is_empty()) {
            request = request.header("Authorization", format!("Bearer {}", token));
        }

        request
    }

    pub async fn fetch_bangumi_me(
        &self,
        access_token: &str,
        user_agent: Option<&str>,
    ) -> Result<serde_json::Value> {
        let url = format!("{}/v0/me", BANGUMI_API_BASE);
        let response = self
            .bangumi_request(&url, Some(access_token), user_agent)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(anyhow!("Bangumi API error: {}", response.status()));
        }

        Ok(response.json().await?)
    }

    pub async fn fetch_bangumi_user(
        &self,
        username: &str,
        access_token: Option<&str>,
        user_agent: Option<&str>,
    ) -> Result<serde_json::Value> {
        let encoded_username = urlencoding::encode(username);
        let url = format!("{}/v0/users/{}", BANGUMI_API_BASE, encoded_username);
        let response = self
            .bangumi_request(&url, access_token, user_agent)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(anyhow!("Bangumi API error: {}", response.status()));
        }

        Ok(response.json().await?)
    }

    pub async fn fetch_bangumi_collections(
        &self,
        username: &str,
        access_token: Option<&str>,
        user_agent: Option<&str>,
    ) -> Result<Vec<serde_json::Value>> {
        const PAGE_LIMIT: usize = 50;
        const MAX_ITEMS: usize = 1000;

        let mut collections = Vec::new();
        let mut offset = 0usize;
        let encoded_username = urlencoding::encode(username);

        loop {
            let url = format!(
                "{}/v0/users/{}/collections?limit={}&offset={}",
                BANGUMI_API_BASE, encoded_username, PAGE_LIMIT, offset
            );
            let response = self
                .bangumi_request(&url, access_token, user_agent)
                .send()
                .await?;

            if !response.status().is_success() {
                return Err(anyhow!(
                    "Bangumi collections API error: {}",
                    response.status()
                ));
            }

            let page: serde_json::Value = response.json().await?;
            let mut data = page
                .get("data")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

            if data.is_empty() {
                break;
            }

            let page_len = data.len();
            collections.append(&mut data);

            if page_len < PAGE_LIMIT || collections.len() >= MAX_ITEMS {
                break;
            }

            offset += PAGE_LIMIT;
            tokio::time::sleep(tokio::time::Duration::from_millis(350)).await;
        }

        collections.truncate(MAX_ITEMS);
        Ok(collections)
    }

    // ==================== X (Twitter) API v2 ====================

    const X_API_BASE: &'static str = "https://api.x.com/2";

    fn x_auth_header(bearer_token: &str) -> String {
        let token = bearer_token.trim();
        if token.to_ascii_lowercase().starts_with("bearer ") {
            token.to_string()
        } else {
            format!("Bearer {}", token)
        }
    }

    /// 通过用户名查找 X 用户（App Bearer Token）
    pub async fn fetch_x_user_by_username(
        &self,
        username: &str,
        bearer_token: &str,
    ) -> Result<serde_json::Value> {
        let username = username.trim().trim_start_matches('@');
        if username.is_empty() {
            return Err(anyhow!("X username is required"));
        }
        if bearer_token.trim().is_empty() {
            return Err(anyhow!("X bearer token is required"));
        }

        let encoded = urlencoding::encode(username);
        let url = format!(
            "{}/users/by/username/{}?user.fields=created_at,description,entities,id,location,name,pinned_tweet_id,profile_image_url,protected,public_metrics,url,username,verified,verified_type",
            Self::X_API_BASE,
            encoded
        );

        let response = self
            .client
            .get(&url)
            .header("Authorization", Self::x_auth_header(bearer_token))
            .header("User-Agent", "Myriad")
            .header("Accept", "application/json")
            .send()
            .await?;

        let status = response.status();
        let body: serde_json::Value = response.json().await?;

        if !status.is_success() {
            let detail = body
                .get("detail")
                .or_else(|| body.get("title"))
                .or_else(|| body.pointer("/errors/0/message"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error");
            return Err(anyhow!("X API error ({}): {}", status, detail));
        }

        body.get("data")
            .cloned()
            .ok_or_else(|| anyhow!("X user not found: @{}", username))
    }

    /// 获取用户时间线推文（最多 max_results，分页拉取）
    pub async fn fetch_x_user_tweets(
        &self,
        user_id: &str,
        bearer_token: &str,
        max_results: usize,
    ) -> Result<Vec<serde_json::Value>> {
        if user_id.trim().is_empty() {
            return Err(anyhow!("X user id is required"));
        }
        if bearer_token.trim().is_empty() {
            return Err(anyhow!("X bearer token is required"));
        }

        const PAGE_SIZE: usize = 100;
        let mut tweets = Vec::new();
        let mut pagination_token: Option<String> = None;
        let target = max_results.clamp(5, 200);

        loop {
            let page = (target - tweets.len()).clamp(5, PAGE_SIZE);
            let mut url = format!(
                "{}/users/{}/tweets?max_results={}&tweet.fields=created_at,public_metrics,entities,lang,possibly_sensitive,source,conversation_id,in_reply_to_user_id,referenced_tweets&exclude=retweets,replies",
                Self::X_API_BASE,
                urlencoding::encode(user_id),
                page
            );
            if let Some(token) = &pagination_token {
                url.push_str(&format!("&pagination_token={}", urlencoding::encode(token)));
            }

            let response = self
                .client
                .get(&url)
                .header("Authorization", Self::x_auth_header(bearer_token))
                .header("User-Agent", "Myriad")
                .header("Accept", "application/json")
                .send()
                .await?;

            let status = response.status();
            let body: serde_json::Value = response.json().await?;

            if !status.is_success() {
                // 时间线权限不足时返回空列表，由上层决定是否告警
                if tweets.is_empty() {
                    let detail = body
                        .get("detail")
                        .or_else(|| body.get("title"))
                        .or_else(|| body.pointer("/errors/0/message"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown error");
                    return Err(anyhow!("X tweets API error ({}): {}", status, detail));
                }
                break;
            }

            let mut page_data = body
                .get("data")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

            if page_data.is_empty() {
                break;
            }

            tweets.append(&mut page_data);

            pagination_token = body
                .pointer("/meta/next_token")
                .and_then(|v| v.as_str())
                .map(str::to_string);

            if tweets.len() >= target || pagination_token.is_none() {
                break;
            }

            tokio::time::sleep(tokio::time::Duration::from_millis(350)).await;
        }

        tweets.truncate(target);
        Ok(tweets)
    }

    /// 获取用户关注列表（最多 max_results，分页拉取；免费档不可用，需要付费额度）
    pub async fn fetch_x_user_following(
        &self,
        user_id: &str,
        bearer_token: &str,
        max_results: usize,
    ) -> Result<Vec<serde_json::Value>> {
        if user_id.trim().is_empty() {
            return Err(anyhow!("X user id is required"));
        }
        if bearer_token.trim().is_empty() {
            return Err(anyhow!("X bearer token is required"));
        }

        const PAGE_SIZE: usize = 1000;
        let mut following = Vec::new();
        let mut pagination_token: Option<String> = None;
        let target = max_results.clamp(1, 5000);

        loop {
            let page = (target - following.len()).clamp(1, PAGE_SIZE);
            // 只取下游（SmartFilter/报告卡）实际消费的字段，控制响应体积
            let mut url = format!(
                "{}/users/{}/following?max_results={}&user.fields=description,id,name,profile_image_url,public_metrics,username,verified",
                Self::X_API_BASE,
                urlencoding::encode(user_id),
                page
            );
            if let Some(token) = &pagination_token {
                url.push_str(&format!("&pagination_token={}", urlencoding::encode(token)));
            }

            let response = self
                .client
                .get(&url)
                .header("Authorization", Self::x_auth_header(bearer_token))
                .header("User-Agent", "Myriad")
                .header("Accept", "application/json")
                .send()
                .await?;

            let status = response.status();
            let body: serde_json::Value = response.json().await?;

            if !status.is_success() {
                // 后续分页失败时保留已拉到的部分，由上层决定是否告警
                if following.is_empty() {
                    let detail = body
                        .get("detail")
                        .or_else(|| body.get("title"))
                        .or_else(|| body.pointer("/errors/0/message"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown error");
                    return Err(anyhow!("X following API error ({}): {}", status, detail));
                }
                break;
            }

            let mut page_data = body
                .get("data")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

            if page_data.is_empty() {
                break;
            }

            following.append(&mut page_data);

            pagination_token = body
                .pointer("/meta/next_token")
                .and_then(|v| v.as_str())
                .map(str::to_string);

            if following.len() >= target || pagination_token.is_none() {
                break;
            }

            tokio::time::sleep(tokio::time::Duration::from_millis(350)).await;
        }

        following.truncate(target);
        Ok(following)
    }

    /// 聚合抓取：用户资料 + 时间线 + 关注列表（仅 App Bearer，不走用户 OAuth）
    pub async fn fetch_x_profile_bundle(
        &self,
        username: &str,
        bearer_token: &str,
    ) -> Result<serde_json::Value> {
        let user = self
            .fetch_x_user_by_username(username, bearer_token)
            .await?;

        let user_id = user
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("X user response missing id"))?
            .to_string();

        // 时间线与关注列表相互独立（不同端点、各自限额），并发拉取
        let (tweets_result, following_result) = tokio::join!(
            self.fetch_x_user_tweets(&user_id, bearer_token, 100),
            self.fetch_x_user_following(&user_id, bearer_token, 1000),
        );

        let tweets = match tweets_result {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!("X tweets fetch failed for {}: {}", username, e);
                Vec::new()
            }
        };

        let following = match following_result {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!("X following fetch failed for {}: {}", username, e);
                Vec::new()
            }
        };

        Ok(serde_json::json!({
            "user": user,
            "tweets": tweets,
            "following": following,
        }))
    }

    // ==================== Discord API v10 (user OAuth) ====================

    const DISCORD_API_BASE: &'static str = "https://discord.com/api/v10";
    const DISCORD_TOKEN_URL: &'static str = "https://discord.com/api/oauth2/token";

    fn discord_auth_header(access_token: &str) -> String {
        let token = access_token.trim();
        if token.to_ascii_lowercase().starts_with("bearer ") {
            token.to_string()
        } else {
            format!("Bearer {}", token)
        }
    }

    /// 刷新 Discord access token（需要 App client_id / client_secret + refresh_token）
    ///
    /// 返回 (access_token, refresh_token_opt, expires_at_unix_opt)
    pub async fn refresh_discord_token(
        &self,
        refresh_token: &str,
        client_id: &str,
        client_secret: &str,
    ) -> Result<(String, Option<String>, Option<i64>)> {
        let refresh_token = refresh_token.trim();
        let client_id = client_id.trim();
        let client_secret = client_secret.trim();
        if refresh_token.is_empty() || client_id.is_empty() || client_secret.is_empty() {
            return Err(anyhow!(
                "Discord refresh requires refresh_token, client_id, and client_secret"
            ));
        }

        let resp = self
            .client
            .post(Self::DISCORD_TOKEN_URL)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
                ("client_id", client_id),
                ("client_secret", client_secret),
            ])
            .send()
            .await?;

        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!(
                "Discord token refresh failed ({}): {}",
                status,
                body.chars().take(300).collect::<String>()
            ));
        }

        let json: serde_json::Value = serde_json::from_str(&body)
            .map_err(|e| anyhow!("Discord token refresh JSON parse error: {}", e))?;

        let access = json
            .get("access_token")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow!("Discord refresh response missing access_token"))?
            .to_string();

        let new_refresh = json
            .get("refresh_token")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        let expires_in = json.get("expires_in").and_then(|v| v.as_i64()).unwrap_or(0);
        let expires_at = if expires_in > 0 {
            Some(chrono::Utc::now().timestamp() + expires_in)
        } else {
            None
        };

        Ok((access, new_refresh, expires_at))
    }

    /// 若 token 将过期且具备 refresh 条件则刷新；否则原样返回 access_token。
    ///
    /// 返回 (access_token, refresh_token_opt, expires_at_unix_opt, did_refresh)
    pub async fn ensure_discord_access_token(
        &self,
        access_token: &str,
        refresh_token: Option<&str>,
        expires_at: Option<i64>,
        client_id: Option<&str>,
        client_secret: Option<&str>,
    ) -> Result<(String, Option<String>, Option<i64>, bool)> {
        let access_token = access_token.trim();
        if access_token.is_empty() {
            return Err(anyhow!("Discord access token is required"));
        }

        let now = chrono::Utc::now().timestamp();
        let needs_refresh = match expires_at {
            Some(exp) => exp <= now + 60,
            // 无过期信息时不主动刷新，交给 API 401 后由上层处理
            None => false,
        };

        if !needs_refresh {
            return Ok((
                access_token.to_string(),
                refresh_token.map(|s| s.to_string()),
                expires_at,
                false,
            ));
        }

        let refresh = refresh_token.map(str::trim).filter(|s| !s.is_empty());
        let cid = client_id.map(str::trim).filter(|s| !s.is_empty());
        let secret = client_secret.map(str::trim).filter(|s| !s.is_empty());

        match (refresh, cid, secret) {
            (Some(rt), Some(cid), Some(secret)) => {
                let (access, new_refresh, new_exp) =
                    self.refresh_discord_token(rt, cid, secret).await?;
                Ok((
                    access,
                    new_refresh.or_else(|| Some(rt.to_string())),
                    new_exp,
                    true,
                ))
            }
            _ => {
                tracing::warn!(
                    "Discord access token expired/expiring but refresh credentials incomplete"
                );
                Ok((
                    access_token.to_string(),
                    refresh_token.map(|s| s.to_string()),
                    expires_at,
                    false,
                ))
            }
        }
    }

    async fn discord_get_json(&self, path: &str, access_token: &str) -> Result<serde_json::Value> {
        let url = format!("{}{}", Self::DISCORD_API_BASE, path);
        let mut last_err = None;

        for attempt in 0..3 {
            let resp = self
                .client
                .get(&url)
                .header("Authorization", Self::discord_auth_header(access_token))
                .header(
                    "User-Agent",
                    "Myriad (compatible; +https://github.com/myriad-you/Myriad)",
                )
                .send()
                .await;

            let resp = match resp {
                Ok(r) => r,
                Err(e) => {
                    last_err = Some(anyhow!("Discord request failed: {}", e));
                    break;
                }
            };

            let status = resp.status();
            if status.as_u16() == 429 {
                let retry_after = resp
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse::<f64>().ok())
                    .unwrap_or(1.0)
                    .clamp(0.5, 10.0);
                tracing::warn!(
                    "Discord rate limited on {}, retry after {:.1}s (attempt {})",
                    path,
                    retry_after,
                    attempt + 1
                );
                tokio::time::sleep(tokio::time::Duration::from_secs_f64(retry_after)).await;
                continue;
            }

            let body = resp.text().await.unwrap_or_default();
            if status.as_u16() == 401 {
                return Err(anyhow!(
                    "Discord unauthorized (token expired or missing scopes identify/guilds/connections): {}",
                    body.chars().take(200).collect::<String>()
                ));
            }
            if !status.is_success() {
                return Err(anyhow!(
                    "Discord API {} failed ({}): {}",
                    path,
                    status,
                    body.chars().take(300).collect::<String>()
                ));
            }

            return serde_json::from_str(&body)
                .map_err(|e| anyhow!("Discord JSON parse error on {}: {}", path, e));
        }

        Err(last_err.unwrap_or_else(|| anyhow!("Discord request failed after retries")))
    }

    pub async fn fetch_discord_me(&self, access_token: &str) -> Result<serde_json::Value> {
        self.discord_get_json("/users/@me", access_token).await
    }

    pub async fn fetch_discord_guilds(&self, access_token: &str) -> Result<Vec<serde_json::Value>> {
        // with_counts=true 让每个 guild 附带 approximate_member_count /
        // approximate_presence_count —— 同一 scope（guilds）下的额外有效信号，
        // 用来按真实规模/在线人数排序社区，无需任何新权限。
        let value = self
            .discord_get_json("/users/@me/guilds?with_counts=true", access_token)
            .await?;
        Ok(value.as_array().cloned().unwrap_or_default())
    }

    pub async fn fetch_discord_connections(
        &self,
        access_token: &str,
    ) -> Result<Vec<serde_json::Value>> {
        let value = self
            .discord_get_json("/users/@me/connections", access_token)
            .await?;
        Ok(value.as_array().cloned().unwrap_or_default())
    }

    /// 聚合：画像 + 服务器 + 第三方连接
    pub async fn fetch_discord_profile_bundle(
        &self,
        access_token: &str,
    ) -> Result<serde_json::Value> {
        let user = self.fetch_discord_me(access_token).await?;

        let guilds = match self.fetch_discord_guilds(access_token).await {
            Ok(g) => g,
            Err(e) => {
                tracing::warn!("Discord guilds fetch failed: {}", e);
                Vec::new()
            }
        };

        let connections = match self.fetch_discord_connections(access_token).await {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("Discord connections fetch failed: {}", e);
                Vec::new()
            }
        };

        Ok(serde_json::json!({
            "user": user,
            "guilds": guilds,
            "connections": connections,
        }))
    }

    // ==================== MyAnimeList dual-mode ====================
    //
    // Mode A (default / easy): public load.json — username only (Sakurairo-style)
    //   GET https://myanimelist.net/animelist/{username}/load.json?status=7&order=5
    //   GET https://myanimelist.net/mangalist/{username}/load.json?status=7&order=5
    // Mode B (optional enhance): official API v2 + X-MAL-CLIENT-ID
    //   Prefer Mode B when client_id is present and non-empty.
    // load.json entries are normalized to official node + list_status shape.

    const MAL_API_BASE: &'static str = "https://api.myanimelist.net/v2";
    const MAL_SITE_BASE: &'static str = "https://myanimelist.net";
    const MAL_BROWSER_UA: &'static str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
    const MAL_MAX_ITEMS: usize = 1000;
    const MAL_PAGE_DELAY_MS: u64 = 250;

    /// Non-empty client_id → use official API; otherwise load.json.
    fn mal_official_client_id(client_id: Option<&str>) -> Option<&str> {
        client_id.map(str::trim).filter(|s| !s.is_empty())
    }

    fn mal_official_request(&self, url: &str, client_id: &str) -> reqwest::RequestBuilder {
        self.client
            .get(url)
            .header("X-MAL-CLIENT-ID", client_id.trim())
            .header("User-Agent", "Myriad")
            .header("Accept", "application/json")
    }

    fn mal_public_request(&self, url: &str) -> reqwest::RequestBuilder {
        self.client
            .get(url)
            .header("User-Agent", Self::MAL_BROWSER_UA)
            .header("Accept", "application/json,text/javascript,*/*;q=0.01")
            .header("Referer", "https://myanimelist.net/")
            .header("X-Requested-With", "XMLHttpRequest")
    }

    /// load.json status int → official API string status
    fn mal_status_int_to_str(status: i64, is_manga: bool) -> &'static str {
        match status {
            1 => {
                if is_manga {
                    "reading"
                } else {
                    "watching"
                }
            }
            2 => "completed",
            3 => "on_hold",
            4 => "dropped",
            6 => {
                if is_manga {
                    "plan_to_read"
                } else {
                    "plan_to_watch"
                }
            }
            _ => "unknown",
        }
    }

    fn mal_unix_to_iso(ts: i64) -> Option<String> {
        if ts <= 0 {
            return None;
        }
        chrono::DateTime::from_timestamp(ts, 0).map(|dt| dt.to_rfc3339())
    }

    /// 将 load.json 的缩略图升级为较大尺寸（去掉 /r/WxH 与 query）
    fn mal_upgrade_image(path: &str) -> (String, String) {
        let medium = path.to_string();
        let large = {
            let mut s = path.to_string();
            // e.g. https://cdn.myanimelist.net/r/192x272/images/anime/...jpg?s=...
            if let Some(idx) = s.find("/r/") {
                if let Some(rest) = s[idx + 3..].find('/') {
                    let after = idx + 3 + rest;
                    s = format!("{}{}", &s[..idx], &s[after..]);
                }
            }
            if let Some(q) = s.find('?') {
                s.truncate(q);
            }
            s
        };
        let large = if large.is_empty() {
            medium.clone()
        } else {
            large
        };
        (medium, large)
    }

    fn mal_boolish(v: &serde_json::Value) -> bool {
        match v {
            serde_json::Value::Bool(b) => *b,
            serde_json::Value::Number(n) => n.as_i64().unwrap_or(0) != 0,
            serde_json::Value::String(s) => {
                let s = s.trim();
                s == "1" || s.eq_ignore_ascii_case("true")
            }
            _ => false,
        }
    }

    /// 将 load.json 动画条目规范化为官方 v2 node + list_status
    fn normalize_mal_anime_item(raw: &serde_json::Value) -> Option<serde_json::Value> {
        let id = raw.get("anime_id").and_then(|v| v.as_i64())?;
        let title = raw
            .get("anime_title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let title_eng = raw
            .get("anime_title_eng")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let image = raw
            .get("anime_image_path")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let (medium, large) = if image.is_empty() {
            (String::new(), String::new())
        } else {
            Self::mal_upgrade_image(image)
        };
        let num_episodes = raw
            .get("anime_num_episodes")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let genres = raw
            .get("genres")
            .cloned()
            .unwrap_or_else(|| serde_json::json!([]));
        let status_int = raw.get("status").and_then(|v| v.as_i64()).unwrap_or(0);
        let score = raw.get("score").and_then(|v| v.as_i64()).unwrap_or(0);
        let num_watched = raw
            .get("num_watched_episodes")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let is_rewatching = raw
            .get("is_rewatching")
            .map(Self::mal_boolish)
            .unwrap_or(false);
        let updated_at = raw
            .get("updated_at")
            .and_then(|v| v.as_i64())
            .and_then(Self::mal_unix_to_iso)
            .unwrap_or_default();

        let mut node = serde_json::json!({
            "id": id,
            "title": title,
            "num_episodes": num_episodes,
            "genres": genres,
        });
        if !title_eng.is_empty() {
            node["alternative_titles"] = serde_json::json!({ "en": title_eng });
        }
        if !medium.is_empty() {
            node["main_picture"] = serde_json::json!({
                "medium": medium,
                "large": large,
            });
        }

        Some(serde_json::json!({
            "node": node,
            "list_status": {
                "status": Self::mal_status_int_to_str(status_int, false),
                "score": score,
                "num_episodes_watched": num_watched,
                "is_rewatching": is_rewatching,
                "updated_at": updated_at,
            }
        }))
    }

    /// 将 load.json 漫画条目规范化为官方 v2 node + list_status
    fn normalize_mal_manga_item(raw: &serde_json::Value) -> Option<serde_json::Value> {
        let id = raw.get("manga_id").and_then(|v| v.as_i64())?;
        let title = raw
            .get("manga_title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        // live load.json uses manga_english; keep manga_title_eng as alias
        let title_eng = raw
            .get("manga_english")
            .or_else(|| raw.get("manga_title_eng"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let image = raw
            .get("manga_image_path")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let (medium, large) = if image.is_empty() {
            (String::new(), String::new())
        } else {
            Self::mal_upgrade_image(image)
        };
        let num_chapters = raw
            .get("manga_num_chapters")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let num_volumes = raw
            .get("manga_num_volumes")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let genres = raw
            .get("genres")
            .cloned()
            .unwrap_or_else(|| serde_json::json!([]));
        let status_int = raw.get("status").and_then(|v| v.as_i64()).unwrap_or(0);
        let score = raw.get("score").and_then(|v| v.as_i64()).unwrap_or(0);
        let num_chapters_read = raw
            .get("num_read_chapters")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let num_volumes_read = raw
            .get("num_read_volumes")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let is_rereading = raw
            .get("is_rereading")
            .map(Self::mal_boolish)
            .unwrap_or(false);
        let updated_at = raw
            .get("updated_at")
            .and_then(|v| v.as_i64())
            .and_then(Self::mal_unix_to_iso)
            .unwrap_or_default();

        let mut node = serde_json::json!({
            "id": id,
            "title": title,
            "num_chapters": num_chapters,
            "num_volumes": num_volumes,
            "genres": genres,
        });
        if !title_eng.is_empty() {
            node["alternative_titles"] = serde_json::json!({ "en": title_eng });
        }
        if !medium.is_empty() {
            node["main_picture"] = serde_json::json!({
                "medium": medium,
                "large": large,
            });
        }

        Some(serde_json::json!({
            "node": node,
            "list_status": {
                "status": Self::mal_status_int_to_str(status_int, true),
                "score": score,
                "num_chapters_read": num_chapters_read,
                "num_volumes_read": num_volumes_read,
                "is_rereading": is_rereading,
                "updated_at": updated_at,
            }
        }))
    }

    /// 分页抓取 load.json（status=7 全部，order=5 最近更新），返回原始条目数组
    async fn fetch_mal_load_json_raw(
        &self,
        username: &str,
        list_kind: &str, // "animelist" | "mangalist"
        max_items: usize,
    ) -> Result<Vec<serde_json::Value>> {
        let username = username.trim();
        if username.is_empty() {
            return Err(anyhow!("MyAnimeList username is required"));
        }
        if list_kind != "animelist" && list_kind != "mangalist" {
            return Err(anyhow!("invalid MAL list kind: {}", list_kind));
        }

        let encoded = urlencoding::encode(username);
        let mut items = Vec::new();
        let mut offset: usize = 0;

        loop {
            let url = format!(
                "{}/{}/{}/load.json?status=7&order=5&offset={}",
                Self::MAL_SITE_BASE,
                list_kind,
                encoded,
                offset
            );

            let response = self.mal_public_request(&url).send().await?;
            let status = response.status();
            let body: serde_json::Value = response.json().await.map_err(|e| {
                anyhow!(
                    "MAL load.json returned non-JSON for {} ({}): {}",
                    username,
                    list_kind,
                    e
                )
            })?;

            if !status.is_success() {
                let detail = body
                    .pointer("/errors/0/message")
                    .or_else(|| body.get("message"))
                    .or_else(|| body.get("error"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown error");
                if items.is_empty() {
                    return Err(anyhow!(
                        "MAL load.json error for {}/{} ({}): {}",
                        username,
                        list_kind,
                        status,
                        detail
                    ));
                }
                tracing::warn!(
                    "MAL load.json pagination stopped for {}/{} at {}: {}",
                    username,
                    list_kind,
                    status,
                    detail
                );
                break;
            }

            let page = match body.as_array() {
                Some(arr) => arr.clone(),
                None => {
                    if items.is_empty() {
                        return Err(anyhow!(
                            "MAL load.json unexpected response for {}/{}: not an array",
                            username,
                            list_kind
                        ));
                    }
                    tracing::warn!(
                        "MAL load.json non-array page for {}/{}; stopping",
                        username,
                        list_kind
                    );
                    break;
                }
            };

            if page.is_empty() {
                break;
            }

            let page_len = page.len();
            items.extend(page);
            offset += page_len;

            if items.len() >= max_items {
                break;
            }

            // load.json 单页通常约 300；若不足一页可认为结束
            if page_len < 100 {
                break;
            }

            tokio::time::sleep(tokio::time::Duration::from_millis(Self::MAL_PAGE_DELAY_MS)).await;
        }

        items.truncate(max_items);
        Ok(items)
    }

    fn synthesize_mal_user(
        username: &str,
        anime_list: &[serde_json::Value],
        manga_list: &[serde_json::Value],
    ) -> serde_json::Value {
        fn count_status(list: &[serde_json::Value], statuses: &[&str]) -> i64 {
            list.iter()
                .filter(|e| {
                    e.pointer("/list_status/status")
                        .and_then(|v| v.as_str())
                        .map(|s| statuses.contains(&s))
                        .unwrap_or(false)
                })
                .count() as i64
        }

        fn mean_score(list: &[serde_json::Value]) -> Option<f64> {
            let scores: Vec<f64> = list
                .iter()
                .filter_map(|e| {
                    e.pointer("/list_status/score")
                        .and_then(|v| v.as_i64())
                        .filter(|&s| s > 0)
                        .map(|s| s as f64)
                })
                .collect();
            if scores.is_empty() {
                None
            } else {
                Some(scores.iter().sum::<f64>() / scores.len() as f64)
            }
        }

        let anime_mean = mean_score(anime_list);
        let manga_mean = mean_score(manga_list);

        let mut user = serde_json::json!({
            "name": username,
            "anime_statistics": {
                "num_items": anime_list.len() as i64,
                "num_items_watching": count_status(anime_list, &["watching"]),
                "num_items_completed": count_status(anime_list, &["completed"]),
                "num_items_on_hold": count_status(anime_list, &["on_hold"]),
                "num_items_dropped": count_status(anime_list, &["dropped"]),
                "num_items_plan_to_watch": count_status(anime_list, &["plan_to_watch"]),
            },
            "manga_statistics": {
                "num_items": manga_list.len() as i64,
                "num_items_reading": count_status(manga_list, &["reading"]),
                "num_items_completed": count_status(manga_list, &["completed"]),
                "num_items_on_hold": count_status(manga_list, &["on_hold"]),
                "num_items_dropped": count_status(manga_list, &["dropped"]),
                "num_items_plan_to_read": count_status(manga_list, &["plan_to_read"]),
            },
        });

        if let Some(m) = anime_mean {
            user["anime_statistics"]["mean_score"] =
                serde_json::json!((m * 100.0).round() / 100.0);
        }
        if let Some(m) = manga_mean {
            user["manga_statistics"]["mean_score"] =
                serde_json::json!((m * 100.0).round() / 100.0);
        }

        user
    }

    // ---------- Mode B: official API v2 ----------

    /// 官方 API：获取 MAL 用户资料（公开字段 + 动画/漫画统计）
    async fn fetch_mal_user_official(
        &self,
        username: &str,
        client_id: &str,
    ) -> Result<serde_json::Value> {
        let username = username.trim();
        let client_id = client_id.trim();
        if username.is_empty() {
            return Err(anyhow!("MyAnimeList username is required"));
        }
        if client_id.is_empty() {
            return Err(anyhow!("MyAnimeList client_id is required for official API"));
        }

        let encoded = urlencoding::encode(username);
        let url = format!(
            "{}/users/{}?fields=id,name,picture,gender,birthday,location,joined_at,anime_statistics,manga_statistics",
            Self::MAL_API_BASE,
            encoded
        );

        let response = self.mal_official_request(&url, client_id).send().await?;
        let status = response.status();
        let body: serde_json::Value = response.json().await?;

        if !status.is_success() {
            let detail = body
                .get("message")
                .or_else(|| body.get("error"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error");
            return Err(anyhow!("MAL API error ({}): {}", status, detail));
        }

        Ok(body)
    }

    async fn fetch_mal_list_paginated_official(
        &self,
        list_url: &str,
        client_id: &str,
        max_items: usize,
    ) -> Result<Vec<serde_json::Value>> {
        let mut items = Vec::new();
        let mut next_url = Some(list_url.to_string());

        while let Some(url) = next_url {
            let response = self.mal_official_request(&url, client_id).send().await?;
            let status = response.status();
            let body: serde_json::Value = response.json().await?;

            if !status.is_success() {
                if items.is_empty() {
                    let detail = body
                        .get("message")
                        .or_else(|| body.get("error"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown error");
                    return Err(anyhow!("MAL list API error ({}): {}", status, detail));
                }
                tracing::warn!("MAL list pagination stopped at {}: {}", status, url);
                break;
            }

            let mut page_data = body
                .get("data")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

            if page_data.is_empty() {
                break;
            }

            items.append(&mut page_data);

            next_url = body
                .pointer("/paging/next")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(str::to_string);

            if items.len() >= max_items || next_url.is_none() {
                break;
            }

            tokio::time::sleep(tokio::time::Duration::from_millis(Self::MAL_PAGE_DELAY_MS)).await;
        }

        items.truncate(max_items);
        Ok(items)
    }

    async fn fetch_mal_anime_list_official(
        &self,
        username: &str,
        client_id: &str,
    ) -> Result<Vec<serde_json::Value>> {
        let encoded = urlencoding::encode(username.trim());
        let url = format!(
            "{}/users/{}/animelist?fields=list_status{{status,score,num_episodes_watched,is_rewatching,updated_at,start_date,finish_date}},node{{id,title,main_picture,alternative_titles,media_type,num_episodes,status,start_season,mean,genres,nsfw}}&limit=100&nsfw=true&sort=list_updated_at",
            Self::MAL_API_BASE,
            encoded
        );
        self.fetch_mal_list_paginated_official(&url, client_id, Self::MAL_MAX_ITEMS)
            .await
    }

    async fn fetch_mal_manga_list_official(
        &self,
        username: &str,
        client_id: &str,
    ) -> Result<Vec<serde_json::Value>> {
        let encoded = urlencoding::encode(username.trim());
        let url = format!(
            "{}/users/{}/mangalist?fields=list_status{{status,score,num_volumes_read,num_chapters_read,is_rereading,updated_at,start_date,finish_date}},node{{id,title,main_picture,alternative_titles,media_type,num_volumes,num_chapters,status,mean,genres,nsfw}}&limit=100&nsfw=true&sort=list_updated_at",
            Self::MAL_API_BASE,
            encoded
        );
        self.fetch_mal_list_paginated_official(&url, client_id, Self::MAL_MAX_ITEMS)
            .await
    }

    async fn fetch_mal_profile_bundle_official(
        &self,
        username: &str,
        client_id: &str,
    ) -> Result<serde_json::Value> {
        let user = self.fetch_mal_user_official(username, client_id).await?;

        let anime_list = match self.fetch_mal_anime_list_official(username, client_id).await {
            Ok(list) => list,
            Err(e) => {
                tracing::warn!("MAL official anime list fetch failed for {}: {}", username, e);
                Vec::new()
            }
        };

        let manga_list = match self.fetch_mal_manga_list_official(username, client_id).await {
            Ok(list) => list,
            Err(e) => {
                tracing::warn!("MAL official manga list fetch failed for {}: {}", username, e);
                Vec::new()
            }
        };

        Ok(serde_json::json!({
            "user": user,
            "anime_list": anime_list,
            "manga_list": manga_list,
        }))
    }

    // ---------- Mode A: public load.json ----------

    /// load.json：验证用户名可访问（探测公开动画列表第一页）
    async fn fetch_mal_user_public(&self, username: &str) -> Result<serde_json::Value> {
        let username = username.trim();
        if username.is_empty() {
            return Err(anyhow!("MyAnimeList username is required"));
        }

        // max≈一页大小，避免分页；统计仅为抽样，验证通过即可
        let raw = self
            .fetch_mal_load_json_raw(username, "animelist", 300)
            .await?;
        let anime_list: Vec<serde_json::Value> = raw
            .iter()
            .filter_map(Self::normalize_mal_anime_item)
            .collect();

        Ok(Self::synthesize_mal_user(username, &anime_list, &[]))
    }

    async fn fetch_mal_anime_list_public(
        &self,
        username: &str,
    ) -> Result<Vec<serde_json::Value>> {
        let raw = self
            .fetch_mal_load_json_raw(username, "animelist", Self::MAL_MAX_ITEMS)
            .await?;
        Ok(raw
            .iter()
            .filter_map(Self::normalize_mal_anime_item)
            .collect())
    }

    async fn fetch_mal_manga_list_public(
        &self,
        username: &str,
    ) -> Result<Vec<serde_json::Value>> {
        let raw = self
            .fetch_mal_load_json_raw(username, "mangalist", Self::MAL_MAX_ITEMS)
            .await?;
        Ok(raw
            .iter()
            .filter_map(Self::normalize_mal_manga_item)
            .collect())
    }

    async fn fetch_mal_profile_bundle_public(&self, username: &str) -> Result<serde_json::Value> {
        let username = username.trim();
        if username.is_empty() {
            return Err(anyhow!("MyAnimeList username is required"));
        }

        let anime_list = match self.fetch_mal_anime_list_public(username).await {
            Ok(list) => list,
            Err(e) => {
                tracing::warn!("MAL public anime list fetch failed for {}: {}", username, e);
                Vec::new()
            }
        };

        tokio::time::sleep(tokio::time::Duration::from_millis(Self::MAL_PAGE_DELAY_MS)).await;

        let manga_list = match self.fetch_mal_manga_list_public(username).await {
            Ok(list) => list,
            Err(e) => {
                tracing::warn!("MAL public manga list fetch failed for {}: {}", username, e);
                Vec::new()
            }
        };

        if anime_list.is_empty() && manga_list.is_empty() {
            // 再探测一次用户名是否有效（空公开列表 vs 无效用户）
            self.fetch_mal_load_json_raw(username, "animelist", 1)
                .await?;
        }

        let user = Self::synthesize_mal_user(username, &anime_list, &manga_list);

        Ok(serde_json::json!({
            "user": user,
            "anime_list": anime_list,
            "manga_list": manga_list,
        }))
    }

    // ---------- Dual-mode public API ----------
    // client_id present → official API; else load.json (username only).

    /// 验证用户（有 Client ID 走官方 API，否则探测 load.json）
    pub async fn fetch_mal_user(
        &self,
        username: &str,
        client_id: Option<&str>,
    ) -> Result<serde_json::Value> {
        if let Some(cid) = Self::mal_official_client_id(client_id) {
            tracing::debug!("MAL verify via official API for {}", username);
            return self.fetch_mal_user_official(username, cid).await;
        }
        tracing::debug!("MAL verify via load.json for {}", username);
        self.fetch_mal_user_public(username).await
    }

    /// 获取用户动画列表（双模式）
    pub async fn fetch_mal_anime_list(
        &self,
        username: &str,
        client_id: Option<&str>,
    ) -> Result<Vec<serde_json::Value>> {
        if let Some(cid) = Self::mal_official_client_id(client_id) {
            return self.fetch_mal_anime_list_official(username, cid).await;
        }
        self.fetch_mal_anime_list_public(username).await
    }

    /// 获取用户漫画列表（双模式）
    pub async fn fetch_mal_manga_list(
        &self,
        username: &str,
        client_id: Option<&str>,
    ) -> Result<Vec<serde_json::Value>> {
        if let Some(cid) = Self::mal_official_client_id(client_id) {
            return self.fetch_mal_manga_list_official(username, cid).await;
        }
        self.fetch_mal_manga_list_public(username).await
    }

    /// 聚合抓取：用户资料 + 动画/漫画列表（双模式）
    /// - `client_id` 有值 → 官方 API（字段更全、分页更稳）
    /// - 否则 → 公开 load.json（仅需用户名）
    pub async fn fetch_mal_profile_bundle(
        &self,
        username: &str,
        client_id: Option<&str>,
    ) -> Result<serde_json::Value> {
        let username = username.trim();
        if username.is_empty() {
            return Err(anyhow!("MyAnimeList username is required"));
        }

        if let Some(cid) = Self::mal_official_client_id(client_id) {
            tracing::info!("MAL profile bundle via official API for {}", username);
            return self.fetch_mal_profile_bundle_official(username, cid).await;
        }

        tracing::info!("MAL profile bundle via load.json for {}", username);
        self.fetch_mal_profile_bundle_public(username).await
    }

    // ==================== Xbox (OpenXBL) ====================
    //
    // Xbox Live 不提供游玩时长，报告走"成就向"叙事：
    // Gamerscore、每个游戏的成就进度、最近游玩的作品。

    async fn openxbl_get(&self, url: &str, api_key: &str) -> Result<serde_json::Value> {
        let response = self
            .client
            .get(url)
            .header("X-Authorization", api_key)
            .header("Accept", "application/json")
            .send()
            .await?;
        let status = response.status();
        let body: serde_json::Value = response.json().await?;
        if !status.is_success() {
            let detail = body
                .get("error")
                .and_then(|v| v.as_str())
                .or_else(|| body.get("message").and_then(|v| v.as_str()))
                .unwrap_or("unknown error");
            return Err(anyhow!("OpenXBL API error ({}): {}", status, detail));
        }
        // OpenXBL 统一包装为 { content: {...}, code: 200 }，解包后再交给上层解析
        Ok(body.get("content").cloned().unwrap_or(body))
    }

    /// Gamertag → XUID（现代 gamertag 可含 #suffix，搜索时去掉）
    async fn resolve_xbox_xuid(&self, gamertag: &str, api_key: &str) -> Result<String> {
        let search_term = gamertag.split('#').next().unwrap_or(gamertag).trim();
        let url = format!(
            "https://xbl.io/api/v2/search/{}",
            urlencoding::encode(search_term)
        );
        let body = self.openxbl_get(&url, api_key).await?;

        let person = body
            .get("people")
            .and_then(|v| v.as_array())
            .and_then(|a| a.first())
            .ok_or_else(|| anyhow!("Xbox player not found: {}", gamertag))?;

        person
            .get("xuid")
            .and_then(|v| {
                v.as_str()
                    .map(|s| s.to_string())
                    .or_else(|| v.as_u64().map(|n| n.to_string()))
            })
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow!("Xbox search result missing xuid"))
    }

    /// 聚合抓取：档案 + 成就标题列表
    pub async fn fetch_xbox_profile_bundle(
        &self,
        gamertag: &str,
        api_key: &str,
    ) -> Result<serde_json::Value> {
        let gamertag = gamertag.trim();
        let api_key = api_key.trim();
        if gamertag.is_empty() {
            return Err(anyhow!("Xbox gamertag is required"));
        }
        if api_key.is_empty() {
            return Err(anyhow!("OpenXBL API key is required"));
        }

        let xuid = self.resolve_xbox_xuid(gamertag, api_key).await?;

        let profile = self
            .openxbl_get(&format!("https://xbl.io/api/v2/account/{xuid}"), api_key)
            .await
            .unwrap_or_else(|e| {
                tracing::warn!("Xbox account fetch failed for {}: {}", gamertag, e);
                serde_json::json!({})
            });

        // 玩家全部游戏的成就进度（OpenXBL 单次返回全部 titles，无分页）
        let achievements = self
            .openxbl_get(
                &format!("https://xbl.io/api/v2/achievements/player/{xuid}"),
                api_key,
            )
            .await?;

        Ok(serde_json::json!({
            "gamertag": gamertag,
            "xuid": xuid,
            "profile": profile,
            "achievements": achievements,
        }))
    }

    // ==================== PlayStation (PSN) ====================
    //
    // 同样没有时长数据，报告走"奖杯向"叙事：
    // 奖杯等级、白金数、每个游戏的奖杯完成度、最近有奖杯动态的作品。

    async fn psn_get(&self, url: &str, access_token: &str) -> Result<serde_json::Value> {
        let response = self
            .client
            .get(url)
            .header("Authorization", format!("Bearer {access_token}"))
            .header("Accept", "application/json")
            .send()
            .await?;
        let status = response.status();
        let body: serde_json::Value = response.json().await?;
        if !status.is_success() {
            let detail = body
                .pointer("/error/message")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error");
            return Err(anyhow!("PSN API error ({}): {}", status, detail));
        }
        Ok(body)
    }

    /// Online ID → accountId + 搜索返回的公开资料
    async fn resolve_psn_account(
        &self,
        online_id: &str,
        access_token: &str,
    ) -> Result<(String, serde_json::Value)> {
        let url = format!(
            "https://m.np.playstation.com/api/search/v1/users?searchTerm={}",
            urlencoding::encode(online_id.trim())
        );
        let body = self.psn_get(&url, access_token).await?;

        let metadata = body
            .get("domains")
            .and_then(|v| v.as_array())
            .and_then(|a| {
                a.iter()
                    .find(|d| d.get("domain").and_then(|x| x.as_str()) == Some("SocialAllAccounts"))
            })
            .and_then(|d| d.get("results"))
            .and_then(|v| v.as_array())
            .and_then(|a| a.first())
            .and_then(|r| r.get("socialMetadata"))
            .cloned()
            .ok_or_else(|| anyhow!("PSN player not found: {}", online_id))?;

        let account_id = metadata
            .get("accountId")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow!("PSN search result missing accountId"))?
            .to_string();

        Ok((account_id, metadata))
    }

    /// 聚合抓取：奖杯摘要 + 全部游戏的奖杯标题列表
    pub async fn fetch_psn_profile_bundle(
        &self,
        online_id: &str,
        npsso: &str,
    ) -> Result<serde_json::Value> {
        let online_id = online_id.trim();
        if online_id.is_empty() {
            return Err(anyhow!("PSN online ID is required"));
        }
        if npsso.trim().is_empty() {
            return Err(anyhow!("PSN NPSSO is required"));
        }

        let access_token = crate::api::game_presence::get_psn_access_token(npsso)
            .await
            .map_err(|e| anyhow!(e))?;

        let (account_id, social_metadata) =
            self.resolve_psn_account(online_id, &access_token).await?;

        let trophy_summary = self
            .psn_get(
                &format!(
                    "https://m.np.playstation.com/api/trophy/v1/users/{account_id}/trophySummary"
                ),
                &access_token,
            )
            .await
            .unwrap_or_else(|e| {
                tracing::warn!("PSN trophy summary fetch failed for {}: {}", online_id, e);
                serde_json::json!({})
            });

        // 奖杯标题分页拉取（每页上限 250，最多 800 个游戏足够画像用）
        let mut trophy_titles: Vec<serde_json::Value> = Vec::new();
        let mut offset = 0usize;
        const PAGE: usize = 250;
        const MAX_TITLES: usize = 800;
        loop {
            let url = format!(
                "https://m.np.playstation.com/api/trophy/v1/users/{account_id}/trophyTitles?limit={PAGE}&offset={offset}"
            );
            let body = match self.psn_get(&url, &access_token).await {
                Ok(b) => b,
                Err(e) => {
                    if trophy_titles.is_empty() {
                        return Err(e);
                    }
                    tracing::warn!("PSN trophyTitles pagination stopped at {}: {}", offset, e);
                    break;
                }
            };
            let page = body
                .get("trophyTitles")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let page_len = page.len();
            trophy_titles.extend(page);

            let total = body
                .get("totalItemCount")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as usize;
            offset += page_len;
            if page_len == 0 || offset >= total || offset >= MAX_TITLES {
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(250)).await;
        }

        Ok(serde_json::json!({
            "online_id": online_id,
            "account_id": account_id,
            "social_metadata": social_metadata,
            "trophy_summary": trophy_summary,
            "trophy_titles": trophy_titles,
        }))
    }
}

// ==================== X 分享文案工具（无网络） ====================

/// 免费账号常用上限；Premium 可更长，这里作为默认安全截断阈值
pub const X_SHARE_DEFAULT_MAX_LEN: usize = 280;

/// 组装分享文案：优先 `text`，否则 `title` + `summary`，再拼 hashtags 与 url
pub fn compose_x_share_text(
    text: Option<&str>,
    title: Option<&str>,
    summary: Option<&str>,
    url: Option<&str>,
    hashtags: &[String],
    max_len: usize,
) -> String {
    let max_len = if max_len == 0 {
        X_SHARE_DEFAULT_MAX_LEN
    } else {
        max_len
    };

    let mut parts: Vec<String> = Vec::new();

    let main = text
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            let t = title.map(str::trim).filter(|s| !s.is_empty());
            let s = summary.map(str::trim).filter(|s| !s.is_empty());
            match (t, s) {
                (Some(t), Some(s)) => Some(format!("{}\n\n{}", t, s)),
                (Some(t), None) => Some(t.to_string()),
                (None, Some(s)) => Some(s.to_string()),
                (None, None) => None,
            }
        });

    if let Some(main) = main {
        parts.push(main);
    }

    let tags: Vec<String> = hashtags
        .iter()
        .map(|t| t.trim().trim_start_matches('#').trim())
        .filter(|t| !t.is_empty())
        .map(|t| format!("#{}", t.replace(' ', "")))
        .collect();
    if !tags.is_empty() {
        parts.push(tags.join(" "));
    }

    if let Some(u) = url.map(str::trim).filter(|s| !s.is_empty()) {
        parts.push(u.to_string());
    }

    let composed = parts.join("\n\n");
    truncate_x_share_text(&composed, max_len)
}

/// 按 Unicode 标量截断，避免切在组合字符中间出乱码；末尾加 …
pub fn truncate_x_share_text(text: &str, max_len: usize) -> String {
    if max_len == 0 {
        return String::new();
    }
    let count = text.chars().count();
    if count <= max_len {
        return text.to_string();
    }
    if max_len == 1 {
        return "…".to_string();
    }
    let keep = max_len - 1;
    let truncated: String = text.chars().take(keep).collect();
    format!("{}…", truncated.trim_end())
}

/// 生成 Web Intent 链接（无需 API 写权限，打开浏览器即可发帖）
pub fn build_x_intent_url(text: &str, url: Option<&str>) -> String {
    let mut params = vec![("text", text.to_string())];
    if let Some(u) = url.map(str::trim).filter(|s| !s.is_empty()) {
        // Intent 支持独立 url 参数；若 text 里已含链接也可只放 text
        params.push(("url", u.to_string()));
    }
    let query = params
        .iter()
        .map(|(k, v)| format!("{}={}", k, urlencoding::encode(v)))
        .collect::<Vec<_>>()
        .join("&");
    format!("https://x.com/intent/tweet?{}", query)
}

#[cfg(test)]
mod x_share_tests {
    use super::*;

    #[test]
    fn compose_prefers_explicit_text() {
        let s = compose_x_share_text(
            Some("  hello  "),
            Some("title"),
            Some("summary"),
            Some("https://example.com"),
            &["Myriad".into(), "#Rust".into()],
            280,
        );
        assert!(s.starts_with("hello"));
        assert!(s.contains("#Myriad"));
        assert!(s.contains("#Rust"));
        assert!(s.contains("https://example.com"));
    }

    #[test]
    fn compose_from_title_summary() {
        let s = compose_x_share_text(None, Some("周报"), Some("本周写了 X 接入"), None, &[], 280);
        assert_eq!(s, "周报\n\n本周写了 X 接入");
    }

    #[test]
    fn truncate_respects_unicode() {
        let s = truncate_x_share_text("你好世界ABC", 3);
        assert_eq!(s.chars().count(), 3);
        assert!(s.ends_with('…'));
    }

    #[test]
    fn intent_url_encodes_text() {
        let url = build_x_intent_url("hello world #test", Some("https://ex.com/a b"));
        assert!(url.starts_with("https://x.com/intent/tweet?"));
        assert!(url.contains("text=hello%20world"));
        assert!(url.contains("url=https"));
    }
}
