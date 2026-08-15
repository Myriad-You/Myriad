// SmartFilter cache load/save and token estimation.

use serde_json::Value;
use std::fs;
use std::path::Path;

use super::helpers::*;

impl SmartFilter {

    /// Keep API vocabulary labels for distribution histograms; type 6 stays "real".
    /// Library/report consumers map via `bangumi_label_to_library_type` / `bangumi_library_item_type`.
    pub(crate) fn bangumi_subject_type_label(subject_type: i64) -> &'static str {
        match subject_type {
            1 => "book",
            2 => "anime",
            3 => "music",
            4 => "game",
            6 => "real",
            _ => "unknown",
        }
    }

    pub(crate) fn bangumi_collection_type_label(collection_type: i64) -> &'static str {
        match collection_type {
            1 => "wish",
            2 => "done",
            3 => "doing",
            4 => "on_hold",
            5 => "dropped",
            _ => "unknown",
        }
    }

    /// 将 MAL list_status 映射为与 Bangumi 一致的 done/doing/wish 标签
    pub(crate) fn mal_status_label(status: &str) -> &'static str {
        match status {
            "completed" => "done",
            "watching" | "reading" => "doing",
            "plan_to_watch" | "plan_to_read" => "wish",
            "on_hold" => "on_hold",
            "dropped" => "dropped",
            _ => "unknown",
        }
    }

    /// 估算过滤后数据的 Token 大小
    pub fn estimate_token_size(filtered_data: &SmartFilteredData) -> usize {
        let json_str = serde_json::to_string(filtered_data).unwrap_or_default();
        // 粗略估算: 每4个字符 ≈ 1 token
        json_str.len() / 4
    }

    /// 处理单个平台数据并保存到独立缓存文件
    /// 优势：
    /// - 只处理需要的平台
    /// - 独立文件缓存，避免大文件读写
    /// - 支持并发处理不同平台
    pub fn process_and_save_single(
        platform: &str,
        platform_data: &Value,
    ) -> Result<SmartFilteredData, Box<dyn std::error::Error>> {
        tracing::info!("🔄 Processing single platform: {}", platform);

        // 预处理数据（根据平台适配数据结构）
        let process_data = Self::preprocess_platform_data(platform, platform_data)?;

        // 过滤数据
        let filtered_data = Self::filter(platform, &process_data)?;

        // 保存到独立缓存文件
        Self::save_platform_cache(platform, &filtered_data)?;

        tracing::info!("✓ Processed and cached {}", platform);
        Ok(filtered_data)
    }

    /// 预处理平台数据（适配数据结构）
    pub(crate) fn preprocess_platform_data(
        platform: &str,
        data: &Value,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let mut processed = data.clone();

        match platform {
            "bilibili" => {
                // 适配: user / user_info（旧缓存）-> user_info
                if let Some(user) = data.get("user").or_else(|| data.get("user_info")) {
                    if let Some(obj) = processed.as_object_mut() {
                        obj.insert("user_info".to_string(), user.clone());
                    }
                }

                // 适配: favorites -> videos (提取所有视频)
                if let Some(favorites) = data.get("favorites").and_then(|v| v.as_array()) {
                    let mut all_videos = Vec::new();
                    for fav in favorites {
                        if let Some(vids) = fav.get("videos").and_then(|v| v.as_array()) {
                            all_videos.extend_from_slice(vids);
                        }
                    }
                    if let Some(obj) = processed.as_object_mut() {
                        obj.insert("videos".to_string(), Value::Array(all_videos));
                    }
                }
            }
            "steam" => {
                // 适配: user -> user_info
                if let Some(user) = data.get("user") {
                    if let Some(obj) = processed.as_object_mut() {
                        obj.insert("user_info".to_string(), user.clone());
                    }
                }

                // 适配: games -> owned_games.games
                if let Some(games) = data.get("games") {
                    if let Some(obj) = processed.as_object_mut() {
                        obj.insert(
                            "owned_games".to_string(),
                            serde_json::json!({ "games": games }),
                        );
                        obj.insert(
                            "recently_played".to_string(),
                            serde_json::json!({ "games": games }),
                        );
                    }
                }
            }
            "netease" => {
                // 适配: liked_songs -> playlists[0].tracks 和 songs
                if let Some(liked_songs) = data.get("liked_songs") {
                    if let Some(obj) = processed.as_object_mut() {
                        obj.insert(
                            "playlists".to_string(),
                            serde_json::json!([{ "tracks": liked_songs }]),
                        );
                        obj.insert("songs".to_string(), liked_songs.clone());
                    }
                }
            }
            "github" => {
                // GitHub 数据通常不需要特殊预处理
            }
            "youtube" => {
                // YouTube 数据已按 { channel, videos, playlist_items } 保存
            }
            "bangumi" => {
                // Bangumi 数据已按 { user, collections } 保存，不需要特殊预处理
            }
            "x" => {
                // X 数据已按 { user, tweets } 保存（Intent 分享，不拉 likes）
            }
            "discord" => {
                // Discord 数据已按 { user, guilds, connections, myriad_cross_refs? } 保存
            }
            "mal" => {
                // MAL 数据已按 { user, anime_list, manga_list } 保存
            }
            "xbox" => {
                // Xbox 数据已按 { gamertag, xuid, profile, achievements } 保存
            }
            "psn" => {
                // PSN 数据已按 { online_id, account_id, social_metadata, trophy_summary, trophy_titles } 保存
            }
            _ => {}
        }

        Ok(processed)
    }

    /// 保存平台缓存到独立文件（使用原子写入）
    pub(crate) fn save_platform_cache(
        platform: &str,
        data: &SmartFilteredData,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Self::save_platform_cache_atomic(platform, data)
    }

    /// Xbox / MS 商店图：http → https，images-eds → images-eds-ssl，避免 HTTPS 页混合内容被拦
    pub fn normalize_xbox_media_url(url: &str) -> String {
        let mut u = Self::normalize_https_media_url(url);
        u = u.replace(
            "://images-eds.xboxlive.com",
            "://images-eds-ssl.xboxlive.com",
        );
        u
    }

    /// 通用媒体 URL：协议相对 / http 升 https（PSN 图标、头像同用）
    pub fn normalize_https_media_url(url: &str) -> String {
        let mut u = url.trim().to_string();
        if u.starts_with("//") {
            u = format!("https:{u}");
        } else if let Some(rest) = u.strip_prefix("http://") {
            u = format!("https://{rest}");
        }
        u
    }

    /// 从独立缓存文件加载平台数据
    pub fn load_platform_cache(
        platform: &str,
    ) -> Result<SmartFilteredData, Box<dyn std::error::Error>> {
        let cache_file = Path::new("cache/platforms").join(format!("{}_filtered.json", platform));

        if !cache_file.exists() {
            return Err(format!("Cache file not found for platform: {}", platform).into());
        }

        let content = fs::read_to_string(&cache_file)?;
        let data: SmartFilteredData = serde_json::from_str(&content)?;

        tracing::debug!("Loaded {} from cache", platform);
        Ok(data)
    }

    /// 检查平台缓存是否存在
    pub fn has_platform_cache(platform: &str) -> bool {
        let cache_file = Path::new("cache/platforms").join(format!("{}_filtered.json", platform));
        cache_file.exists()
    }

    /// 清除平台缓存
    pub fn clear_platform_cache(platform: &str) -> Result<(), Box<dyn std::error::Error>> {
        let cache_file = Path::new("cache/platforms").join(format!("{}_filtered.json", platform));
        if cache_file.exists() {
            fs::remove_file(&cache_file)?;
            tracing::info!("Cleared cache for {}", platform);
        }
        Ok(())
    }
}
