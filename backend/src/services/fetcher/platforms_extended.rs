// Extended platform fetch implementations (Discord, MAL, Xbox, PSN, YouTube, …).

use anyhow::{anyhow, Result};

use super::types::*;
use super::x_share::urlencoding_lite;

impl PlatformFetcher {
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

    // MyAnimeList dual-mode
    //
    // Mode A (default / easy): public load.json — username only (Sakurairo-style)
    // GET https://myanimelist.net/animelist/{username}/load.json?status=7&order=5
    // GET https://myanimelist.net/mangalist/{username}/load.json?status=7&order=5
    // Mode B (optional enhance): official API v2 + X-MAL-CLIENT-ID
    // Prefer Mode B when client_id is present and non-empty.
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
            user["anime_statistics"]["mean_score"] = serde_json::json!((m * 100.0).round() / 100.0);
        }
        if let Some(m) = manga_mean {
            user["manga_statistics"]["mean_score"] = serde_json::json!((m * 100.0).round() / 100.0);
        }

        user
    }

    // Mode B: official API v2

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
            return Err(anyhow!(
                "MyAnimeList client_id is required for official API"
            ));
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

        let anime_list = match self
            .fetch_mal_anime_list_official(username, client_id)
            .await
        {
            Ok(list) => list,
            Err(e) => {
                tracing::warn!(
                    "MAL official anime list fetch failed for {}: {}",
                    username,
                    e
                );
                Vec::new()
            }
        };

        let manga_list = match self
            .fetch_mal_manga_list_official(username, client_id)
            .await
        {
            Ok(list) => list,
            Err(e) => {
                tracing::warn!(
                    "MAL official manga list fetch failed for {}: {}",
                    username,
                    e
                );
                Vec::new()
            }
        };

        Ok(serde_json::json!({
            "user": user,
            "anime_list": anime_list,
            "manga_list": manga_list,
        }))
    }

    // Mode A: public load.json

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

    async fn fetch_mal_anime_list_public(&self, username: &str) -> Result<Vec<serde_json::Value>> {
        let raw = self
            .fetch_mal_load_json_raw(username, "animelist", Self::MAL_MAX_ITEMS)
            .await?;
        Ok(raw
            .iter()
            .filter_map(Self::normalize_mal_anime_item)
            .collect())
    }

    async fn fetch_mal_manga_list_public(&self, username: &str) -> Result<Vec<serde_json::Value>> {
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

    // Dual-mode public API
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

    // Xbox (OpenXBL)
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

    // PlayStation (PSN)
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

        let access_token = myriad_psn_auth::get_psn_access_token(npsso)
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

    // YouTube Data API v3 (API key, public only)

    /// Resolve channel + recent public uploads (+ stats). No OAuth / mine flows.
    ///
    /// `channel_identity` accepts UC… id, `@handle`, or bare handle / customUrl.
    /// Uses channels.list → uploads playlist → playlistItems → videos.list (never search.list).
    pub async fn fetch_youtube_channel_bundle(
        &self,
        api_key: &str,
        channel_identity: &str,
    ) -> Result<serde_json::Value> {
        let key = api_key.trim();
        let identity = channel_identity.trim();
        if key.is_empty() {
            return Err(anyhow!("YouTube API key is required"));
        }
        if identity.is_empty() {
            return Err(anyhow!("YouTube channel id or handle is required"));
        }

        let channel = self.fetch_youtube_channel(key, identity).await?;
        let channel_id = channel
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if channel_id.is_empty() {
            return Err(anyhow!("YouTube channel response missing id"));
        }

        let uploads_playlist = channel
            .pointer("/contentDetails/relatedPlaylists/uploads")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let mut playlist_items: Vec<serde_json::Value> = Vec::new();
        let mut video_ids: Vec<String> = Vec::new();
        if !uploads_playlist.is_empty() {
            playlist_items = self
                .fetch_youtube_playlist_items(key, &uploads_playlist, 12)
                .await
                .unwrap_or_else(|e| {
                    tracing::warn!("YouTube playlistItems failed: {}", e);
                    Vec::new()
                });
            for item in &playlist_items {
                if let Some(vid) = item
                    .pointer("/contentDetails/videoId")
                    .or_else(|| item.pointer("/snippet/resourceId/videoId"))
                    .and_then(|v| v.as_str())
                {
                    if !vid.is_empty() && !video_ids.iter().any(|x| x == vid) {
                        video_ids.push(vid.to_string());
                    }
                }
            }
        }

        let videos = if video_ids.is_empty() {
            Vec::new()
        } else {
            self.fetch_youtube_videos(key, &video_ids)
                .await
                .unwrap_or_else(|e| {
                    tracing::warn!("YouTube videos.list failed: {}", e);
                    Vec::new()
                })
        };

        Ok(serde_json::json!({
            "channel": channel,
            "uploads_playlist_id": uploads_playlist,
            "playlist_items": playlist_items,
            "videos": videos,
        }))
    }

    /// channels.list by id / forHandle / forUsername
    pub async fn fetch_youtube_channel(
        &self,
        api_key: &str,
        channel_identity: &str,
    ) -> Result<serde_json::Value> {
        let base = "https://www.googleapis.com/youtube/v3/channels";
        let part = "snippet,statistics,contentDetails,brandingSettings";
        let identity = channel_identity.trim();

        let url = if identity.starts_with("UC") && identity.len() >= 20 && !identity.contains(' ') {
            format!(
                "{base}?part={part}&id={}&key={}",
                urlencoding_lite(identity),
                urlencoding_lite(api_key)
            )
        } else {
            let handle = identity.trim_start_matches('@');
            // Prefer forHandle (modern); fall back to forUsername for legacy names
            format!(
                "{base}?part={part}&forHandle={}&key={}",
                urlencoding_lite(handle),
                urlencoding_lite(api_key)
            )
        };

        let resp = self.client.get(&url).send().await?;
        let status = resp.status();
        let body: serde_json::Value = resp.json().await?;
        if !status.is_success() {
            let msg = body
                .pointer("/error/message")
                .and_then(|v| v.as_str())
                .unwrap_or("YouTube channels.list failed");
            return Err(anyhow!("{} (HTTP {})", msg, status.as_u16()));
        }

        if let Some(item) = body
            .get("items")
            .and_then(|v| v.as_array())
            .and_then(|a| a.first())
        {
            return Ok(item.clone());
        }

        // Handle lookup failed — try forUsername once for bare custom names
        if !(identity.starts_with("UC") && identity.len() >= 20) {
            let handle = identity.trim_start_matches('@');
            let url2 = format!(
                "{base}?part={part}&forUsername={}&key={}",
                urlencoding_lite(handle),
                urlencoding_lite(api_key)
            );
            let resp2 = self.client.get(&url2).send().await?;
            let status2 = resp2.status();
            let body2: serde_json::Value = resp2.json().await?;
            if status2.is_success() {
                if let Some(item) = body2
                    .get("items")
                    .and_then(|v| v.as_array())
                    .and_then(|a| a.first())
                {
                    return Ok(item.clone());
                }
            }
        }

        Err(anyhow!(
            "YouTube channel not found for identity '{}'",
            identity
        ))
    }

    pub async fn fetch_youtube_playlist_items(
        &self,
        api_key: &str,
        playlist_id: &str,
        max_results: u32,
    ) -> Result<Vec<serde_json::Value>> {
        let max = max_results.clamp(1, 50);
        let url = format!(
            "https://www.googleapis.com/youtube/v3/playlistItems?part=snippet,contentDetails&playlistId={}&maxResults={}&key={}",
            urlencoding_lite(playlist_id),
            max,
            urlencoding_lite(api_key)
        );
        let resp = self.client.get(&url).send().await?;
        let status = resp.status();
        let body: serde_json::Value = resp.json().await?;
        if !status.is_success() {
            let msg = body
                .pointer("/error/message")
                .and_then(|v| v.as_str())
                .unwrap_or("YouTube playlistItems.list failed");
            return Err(anyhow!("{} (HTTP {})", msg, status.as_u16()));
        }
        Ok(body
            .get("items")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default())
    }

    pub async fn fetch_youtube_videos(
        &self,
        api_key: &str,
        video_ids: &[String],
    ) -> Result<Vec<serde_json::Value>> {
        if video_ids.is_empty() {
            return Ok(Vec::new());
        }
        // API allows up to 50 ids per call
        let chunk: Vec<&str> = video_ids.iter().take(50).map(|s| s.as_str()).collect();
        let ids = chunk.join(",");
        let url = format!(
            "https://www.googleapis.com/youtube/v3/videos?part=snippet,statistics,contentDetails&id={}&key={}",
            urlencoding_lite(&ids),
            urlencoding_lite(api_key)
        );
        let resp = self.client.get(&url).send().await?;
        let status = resp.status();
        let body: serde_json::Value = resp.json().await?;
        if !status.is_success() {
            let msg = body
                .pointer("/error/message")
                .and_then(|v| v.as_str())
                .unwrap_or("YouTube videos.list failed");
            return Err(anyhow!("{} (HTTP {})", msg, status.as_u16()));
        }
        Ok(body
            .get("items")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default())
    }
}
