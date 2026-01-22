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
        wishlist.sort_by(|a, b| a.priority.cmp(&b.priority));

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
    #[allow(dead_code)]
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

    /// 计算 GitHub 用户的总 star 数
    #[allow(dead_code)]
    pub async fn fetch_github_stats(
        &self,
        username: &str,
        token: Option<&str>,
    ) -> Result<serde_json::Value> {
        // 获取用户信息
        let user_info = self.fetch_github_user(username, token).await?;

        // 获取所有仓库
        let repos = self.fetch_github_repos(username, token).await?;

        // 计算总 star 数
        let total_stars: i64 = repos
            .iter()
            .filter_map(|repo| repo["stargazers_count"].as_i64())
            .sum();

        // 计算总 fork 数
        let total_forks: i64 = repos
            .iter()
            .filter_map(|repo| repo["forks_count"].as_i64())
            .sum();

        // 统计编程语言
        let mut languages: std::collections::HashMap<String, i32> =
            std::collections::HashMap::new();
        for repo in &repos {
            if let Some(lang) = repo["language"].as_str() {
                *languages.entry(lang.to_string()).or_insert(0) += 1;
            }
        }

        Ok(serde_json::json!({
            "username": user_info["login"],
            "name": user_info["name"],
            "bio": user_info["bio"],
            "avatar_url": user_info["avatar_url"],
            "followers": user_info["followers"],
            "following": user_info["following"],
            "public_repos": user_info["public_repos"],
            "total_stars": total_stars,
            "total_forks": total_forks,
            "top_languages": languages,
            "repos": repos,
            "contribution_calendar": match self.fetch_github_contributions(username, token).await {
                Ok(calendar) => {
                    tracing::info!("✓ GitHub contributions fetched: {} days", calendar.len());
                    Some(calendar)
                },
                Err(e) => {
                    tracing::warn!("⚠ Failed to fetch GitHub contributions: {}", e);
                    None
                }
            },
        }))
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
}
