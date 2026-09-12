// Core platform fetch implementations (Bilibili, Steam, GitHub, …).

use anyhow::{anyhow, Result};

use crate::services::bilibili_utils::{
    generate_bilibili_cookie, get_random_china_ip, get_random_user_agent,
};
use crate::services::http_client::{get_global_client, GitHubApiUrl};

use super::types::*;

const GITHUB_DESCRIPTION_MAX: usize = 500;
const GITHUB_LANGUAGE_MAX: usize = 64;

fn clip_github_text(value: &str, max_chars: usize) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.chars().take(max_chars).collect())
}

pub(crate) fn parse_github_repo_summary(body: &serde_json::Value) -> Result<GithubRepoSummary> {
    let stars = body
        .get("stargazers_count")
        .and_then(|value| value.as_i64())
        .filter(|count| *count >= 0)
        .ok_or_else(|| anyhow!("GitHub repo payload missing stargazers_count"))?;
    let forks = body
        .get("forks_count")
        .and_then(|value| value.as_i64())
        .filter(|count| *count >= 0)
        .unwrap_or(0);
    let description = body
        .get("description")
        .and_then(|value| value.as_str())
        .and_then(|value| clip_github_text(value, GITHUB_DESCRIPTION_MAX));
    let language = body
        .get("language")
        .and_then(|value| value.as_str())
        .and_then(|value| clip_github_text(value, GITHUB_LANGUAGE_MAX));
    Ok(GithubRepoSummary {
        stars,
        forks,
        description,
        language,
    })
}

impl PlatformFetcher {
    pub async fn new() -> Self {
        Self {
            client: get_global_client().await,
        }
    }

    // Bilibili API

    /// 带 IP 伪装的 B 站 GET，返回解析后的 JSON。
    async fn bilibili_get_json(&self, url: &str, referer: &str) -> Result<serde_json::Value> {
        let client_ip = get_random_china_ip();
        let proxy_ip = get_random_china_ip();
        let forwarded_for = format!("{}, {}", client_ip, proxy_ip);

        self.client
            .get(url)
            .header("User-Agent", get_random_user_agent())
            .header("Referer", referer)
            .header("Origin", "https://www.bilibili.com")
            .header("Accept", "application/json, text/plain, */*")
            .header("Accept-Language", "zh-CN,zh;q=0.9,en-US;q=0.8,en;q=0.7")
            .header("Cookie", generate_bilibili_cookie())
            .header("X-Forwarded-For", &forwarded_for)
            .header("X-Real-IP", &client_ip)
            .send()
            .await?
            .json()
            .await
            .map_err(Into::into)
    }

    /// 获取 Bilibili 用户基本信息。
    ///
    /// 优先 `x/web-interface/card`（含粉丝/关注）；
    /// 失败时回退 `space/acc/info` + `relation/stat`。
    pub async fn fetch_bilibili_user(&self, uid: i64) -> Result<BilibiliUserInfo> {
        match self.fetch_bilibili_user_via_card(uid).await {
            Ok(info) if !info.name.is_empty() || info.mid != 0 => {
                // card 有时粉丝为 0（字段缺失）；用 relation/stat 补全
                if info.follower == 0 && info.following == 0 {
                    if let Ok((follower, following)) = self.fetch_bilibili_relation_stat(uid).await
                    {
                        return Ok(BilibiliUserInfo {
                            follower,
                            following,
                            ..info
                        });
                    }
                }
                return Ok(info);
            }
            Ok(_) => {
                tracing::warn!(
                    "Bilibili card API returned empty user for mid={}, falling back",
                    uid
                );
            }
            Err(e) => {
                tracing::warn!(
                    "Bilibili card API failed for mid={}: {}; falling back to acc/info",
                    uid,
                    e
                );
            }
        }

        self.fetch_bilibili_user_via_acc_info(uid).await
    }

    /// 主路径：web-interface/card（name / face / level / fans / attention）
    async fn fetch_bilibili_user_via_card(&self, uid: i64) -> Result<BilibiliUserInfo> {
        let url = format!("https://api.bilibili.com/x/web-interface/card?mid={}", uid);
        let response = self
            .bilibili_get_json(&url, &format!("https://space.bilibili.com/{}", uid))
            .await?;

        if response["code"].as_i64() != Some(0) {
            return Err(anyhow!(
                "Bilibili card API error: {}",
                response["message"].as_str().unwrap_or("unknown")
            ));
        }

        let data = &response["data"];
        let card = &data["card"];
        let mid = card["mid"]
            .as_i64()
            .or_else(|| card["mid"].as_str().and_then(|s| s.parse::<i64>().ok()))
            .unwrap_or(uid);
        let level = card
            .pointer("/level_info/current_level")
            .and_then(|v| v.as_i64())
            .or_else(|| card["level"].as_i64())
            .unwrap_or(0) as i32;

        // 粉丝：data.follower 或 card.fans；关注：card.attention / card.friend
        let follower = data["follower"]
            .as_i64()
            .or_else(|| card["fans"].as_i64())
            .unwrap_or(0);
        let following = card["attention"]
            .as_i64()
            .or_else(|| card["friend"].as_i64())
            .or_else(|| data["following"].as_i64())
            .unwrap_or(0);

        Ok(BilibiliUserInfo {
            mid,
            name: card["name"].as_str().unwrap_or("").to_string(),
            face: card["face"].as_str().unwrap_or("").to_string(),
            sign: card["sign"].as_str().unwrap_or("").to_string(),
            level,
            following,
            follower,
        })
    }

    /// 关系计数：(follower, following)。acc/info 这两项常年是 0 或不存在。
    async fn fetch_bilibili_relation_stat(&self, uid: i64) -> Result<(i64, i64)> {
        let url = format!("https://api.bilibili.com/x/relation/stat?vmid={}", uid);
        let response = self
            .bilibili_get_json(&url, &format!("https://space.bilibili.com/{}", uid))
            .await?;

        if response["code"].as_i64() != Some(0) {
            return Err(anyhow!(
                "Bilibili relation/stat error: {}",
                response["message"].as_str().unwrap_or("unknown")
            ));
        }

        let data = &response["data"];
        Ok((
            data["follower"].as_i64().unwrap_or(0),
            data["following"].as_i64().unwrap_or(0),
        ))
    }

    /// 回退：旧 space/acc/info + relation/stat
    async fn fetch_bilibili_user_via_acc_info(&self, uid: i64) -> Result<BilibiliUserInfo> {
        let url = format!("https://api.bilibili.com/x/space/acc/info?mid={}", uid);
        let response = self
            .bilibili_get_json(&url, "https://www.bilibili.com")
            .await?;

        if response["code"].as_i64() != Some(0) {
            return Err(anyhow!(
                "Bilibili acc/info error: {}",
                response["message"].as_str().unwrap_or("unknown")
            ));
        }

        let data = &response["data"];
        let mut follower = data["follower"].as_i64().unwrap_or(0);
        let mut following = data["following"].as_i64().unwrap_or(0);

        // acc/info 已不再稳定返回粉丝/关注，用 relation/stat 补全
        if follower == 0 && following == 0 {
            if let Ok((f, g)) = self.fetch_bilibili_relation_stat(uid).await {
                follower = f;
                following = g;
            }
        }

        Ok(BilibiliUserInfo {
            mid: data["mid"].as_i64().unwrap_or(uid),
            name: data["name"].as_str().unwrap_or("").to_string(),
            face: data["face"].as_str().unwrap_or("").to_string(),
            sign: data["sign"].as_str().unwrap_or("").to_string(),
            level: data["level"].as_i64().unwrap_or(0) as i32,
            following,
            follower,
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

        // 只拉 type=1 番剧、type=2 电影。
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

    // Steam API

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
                let appid = game["appid"]
                    .as_i64()
                    .or_else(|| game["appid"].as_u64().map(|u| u as i64))?;
                let playtime_forever = game["playtime_forever"]
                    .as_i64()
                    .or_else(|| game["playtime_forever"].as_u64().map(|u| u as i64))
                    .or_else(|| {
                        game["playtime_forever"]
                            .as_f64()
                            .filter(|f| f.is_finite())
                            .map(|f| f.round() as i64)
                    })
                    .unwrap_or(0)
                    .max(0);
                let playtime_2weeks = game["playtime_2weeks"]
                    .as_i64()
                    .or_else(|| game["playtime_2weeks"].as_u64().map(|u| u as i64))
                    .map(|v| v.max(0) as i32);
                Some(SteamGame {
                    appid,
                    name: game["name"].as_str()?.to_string(),
                    playtime_forever: playtime_forever.min(i32::MAX as i64) as i32,
                    playtime_2weeks,
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

    // GitHub API

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

        if let Some(token) = token.map(str::trim).filter(|token| !token.is_empty()) {
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

            if let Some(token) = token.map(str::trim).filter(|token| !token.is_empty()) {
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

    /// 获取单个公开仓库（含 `stargazers_count`）。
    pub async fn fetch_github_repo(
        &self,
        owner: &str,
        repo: &str,
        token: Option<&str>,
    ) -> Result<serde_json::Value> {
        let url = GitHubApiUrl::repo_url(owner, repo).await;
        let mut request = self
            .client
            .get(&url)
            .header("User-Agent", "Myriad")
            .header("Accept", "application/vnd.github.v3+json");

        if let Some(token) = token.map(str::trim).filter(|token| !token.is_empty()) {
            request = request.header("Authorization", format!("token {}", token));
        }

        let response = request.send().await?;

        if !response.status().is_success() {
            return Err(anyhow!("GitHub API error: {}", response.status()));
        }

        Ok(response.json().await?)
    }

    /// 读取公开仓库摘要（star / fork / 描述 / 语言）。
    pub async fn fetch_github_repo_summary(
        &self,
        owner: &str,
        repo: &str,
        token: Option<&str>,
    ) -> Result<GithubRepoSummary> {
        let body = self.fetch_github_repo(owner, repo, token).await?;
        parse_github_repo_summary(&body)
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

        // Prefer real counts: data-count attr, or <tool-tip> "N contributions on …".
        // data-level is only intensity 0–4 — never treat it as contribution count.
        let mut contributions = Vec::new();

        let tag_re = regex::Regex::new(r#"<(?:rect|td)([^>]+)>"#)?;
        let date_re = regex::Regex::new(r#"data-date="([0-9]{4}-[0-9]{2}-[0-9]{2})""#)?;
        let level_re = regex::Regex::new(r#"data-level="(\d+)""#)?;
        let count_attr_re = regex::Regex::new(r#"data-count="(\d+)""#)?;
        let id_attr_re = regex::Regex::new(r#"\bid="([^"]+)""#)?;
        // tool-tip body: "12 contributions on January 15th." / "No contributions on …"
        let tip_for_re = regex::Regex::new(
            r#"(?is)<tool-tip[^>]*\bfor="([^"]+)"[^>]*>(?:\s*No\s+contributions|\s*(\d+)\s+contributions?)\s+on[^<]*</tool-tip>"#,
        )?;
        let mut tip_by_id: std::collections::HashMap<String, i64> =
            std::collections::HashMap::new();
        for cap in tip_for_re.captures_iter(&html) {
            let id = cap.get(1).map(|m| m.as_str()).unwrap_or("").to_string();
            let count = cap
                .get(2)
                .and_then(|m| m.as_str().parse::<i64>().ok())
                .unwrap_or(0);
            if !id.is_empty() {
                tip_by_id.insert(id, count);
            }
        }

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

                let id = id_attr_re
                    .captures(attrs)
                    .and_then(|c| c.get(1).map(|m| m.as_str().to_string()));

                let count = count_attr_re
                    .captures(attrs)
                    .and_then(|c| c.get(1))
                    .and_then(|m| m.as_str().parse::<i64>().ok())
                    .or_else(|| id.as_ref().and_then(|i| tip_by_id.get(i).copied()))
                    .unwrap_or_else(|| {
                        // Last resort: level as intensity 0–4 (not true commits).
                        // Conservative so total_contributions is not inflated.
                        level.clamp(0, 4)
                    });

                contributions.push(serde_json::json!({
                    "date": date,
                    "count": count,
                    "level": level
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

        // 返回 contributions 全量（总贡献数用完整序列求和）。
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

    /// 网易云喜欢列表。`NeteaseService::fetch_user_liked_songs`。
    pub async fn fetch_netease_liked_songs(&self, user_id: i64) -> Result<Vec<serde_json::Value>> {
        let netease_service = crate::services::netease_service::NeteaseService::new();
        netease_service.fetch_user_liked_songs(user_id).await
    }

    /// 网易云用户信息。`NeteaseService::fetch_user_info`。
    pub async fn fetch_netease_user(&self, user_id: i64) -> Result<serde_json::Value> {
        let netease_service = crate::services::netease_service::NeteaseService::new();
        netease_service.fetch_user_info(user_id).await
    }

    // Bangumi API

    pub(crate) fn bangumi_user_agent(user_agent: Option<&str>) -> &str {
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

    // X (Twitter) API v2

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
                // 已有页则保留部分结果后 break；第一页失败仍 Err。
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

    // Discord API v10 (user OAuth)

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
}
