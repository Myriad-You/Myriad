//! 智能内容过滤器
//!
//! 新的过滤策略：
//! 1. 保留完整内容列表（不只是最近5个）
//! 2. 使用预置数据库进行初步分类
//! 3. 将同类内容合并为判断 + 代表性例子
//! 4. 未知内容保留完整信息

#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::cmp::Reverse;
use std::fs;
use std::path::Path;

use super::content_databases::{AnimeDatabase, ArtistDatabase, GameDatabase};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartFilteredData {
    pub platform: String,
    pub user_summary: UserSummary,
    pub content_analysis: ContentAnalysis,
    pub raw_unknown_content: Vec<UnknownContent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSummary {
    pub username: String,
    pub user_id: String,
    pub level: Option<String>,
    pub stats: UserStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserStats {
    pub follower_count: Option<i64>,
    pub following_count: Option<i64>,
    pub total_content: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ContentAnalysis {
    Bilibili(BilibiliAnalysis),
    Steam(SteamAnalysis),
    GitHub(GitHubAnalysis),
    Netease(NeteaseAnalysis),
    Bangumi(BangumiAnalysis),
    X(XAnalysis),
    Discord(DiscordAnalysis),
    Mal(MalAnalysis),
    Xbox(XboxAnalysis),
    Psn(PsnAnalysis),
}

/// Xbox 成就分析（Xbox Live 无游玩时长，走成就向叙事）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XboxAnalysis {
    pub gaming_summary: String,
    pub gamerscore: i64,
    pub games_count: usize,
    /// 带成就系统的游戏数（totalAchievements > 0）
    #[serde(default)]
    pub achievement_games: usize,
    /// 成就进度 100% 的游戏数
    pub completed_games: usize,
    pub total_achievements_earned: i64,
    /// 库内可解锁成就总数（仅统计有成就系统的作品）
    #[serde(default)]
    pub total_achievements_available: i64,
    /// 有成就系统的游戏的平均完成度（0-100）；无成就系统游戏不计入
    pub average_completion: f64,
    /// 0-100 综合硬核指数（完成度 + 全成就 + GS 规模）
    #[serde(default)]
    pub hardcore_score: i64,
    /// Silver / Gold 等
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_tier: Option<String>,
    /// 玩家头像（GameDisplayPicRaw）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar: Option<String>,
    /// 展示用 gamertag（优先 UniqueModernGamertag）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_gamertag: Option<String>,
    /// XboxOneRep（如 GoodPlayer）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reputation: Option<String>,
    /// 按最近游玩排序
    pub recent_titles: Vec<XboxTitleItem>,
    /// 按完成度排序（优先展示接近全成就的作品）
    pub top_completed_titles: Vec<XboxTitleItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XboxTitleItem {
    pub title_id: String,
    pub name: String,
    pub display_image: Option<String>,
    pub achievements_earned: i64,
    pub achievements_total: i64,
    pub gamerscore_earned: i64,
    pub gamerscore_total: i64,
    /// 成就完成度（0-100）
    pub progress: f64,
    pub last_played: Option<String>,
    /// 设备列表（PC / XboxOne / XboxSeries 等），卡片上可做轻量标签
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub devices: Vec<String>,
}

/// PSN 奖杯分析（同样无时长数据，走奖杯向叙事）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PsnAnalysis {
    pub trophy_summary_text: String,
    pub trophy_level: i64,
    pub platinum_count: i64,
    pub gold_count: i64,
    pub silver_count: i64,
    pub bronze_count: i64,
    /// 四色奖杯合计
    #[serde(default)]
    pub total_trophies: i64,
    pub games_count: usize,
    /// 奖杯进度 100% 的游戏数
    pub completed_games: usize,
    /// 平均奖杯完成度（0-100）
    pub average_progress: f64,
    /// 0-100 猎人指数（白金 + 等级 + 完成度）
    #[serde(default)]
    pub hardcore_score: i64,
    /// 玩家头像
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar: Option<String>,
    /// 展示用 Online ID
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_online_id: Option<String>,
    /// 是否 PS Plus（social 元数据里有时带）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_plus: Option<bool>,
    /// 按最近奖杯动态排序
    pub recent_titles: Vec<PsnTitleItem>,
    /// 按完成度排序
    pub top_completed_titles: Vec<PsnTitleItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PsnTitleItem {
    pub name: String,
    pub platform: String,
    pub icon_url: Option<String>,
    /// 奖杯完成度（0-100）
    pub progress: i64,
    pub earned_platinum: i64,
    pub earned_gold: i64,
    pub earned_silver: i64,
    pub earned_bronze: i64,
    pub last_updated: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XAnalysis {
    pub post_summary: String,
    // 账号本人的展示名/头像（概览卡 header 用）
    #[serde(default)]
    pub user_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_avatar: Option<String>,
    pub engagement_stats: XEngagementStats,
    // following 字段放在 posts 前面：prompt 会按 12000 字符截断，
    // 关注列表对兴趣分析的信号比推文正文更强，优先保留
    #[serde(default)]
    pub following_summary: String,
    #[serde(default)]
    pub following_sample: Vec<XFollowingItem>,
    pub recent_posts: Vec<XPostItem>,
    pub top_posts: Vec<XPostItem>,
    pub language_distribution: std::collections::HashMap<String, usize>,
}

/// Discord 社交身份分析（社区足迹 + 跨平台连接）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordAnalysis {
    pub community_summary: String,
    pub guild_stats: DiscordGuildStats,
    pub guilds_preview: Vec<DiscordGuildItem>,
    pub connections: Vec<DiscordConnectionItem>,
    pub identity_graph: DiscordIdentityGraph,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordGuildStats {
    pub guild_count: usize,
    pub owned_guild_count: usize,
    pub admin_guild_count: usize,
    pub manage_guild_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordGuildItem {
    pub id: String,
    pub name: String,
    pub icon_url: Option<String>,
    pub owner: bool,
    pub permissions_highlight: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordConnectionItem {
    pub r#type: String,
    pub name: String,
    pub id: String,
    pub verified: bool,
    pub visibility: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordIdentityGraph {
    pub linked_platforms: Vec<String>,
    pub cross_check: std::collections::HashMap<String, DiscordCrossCheck>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordCrossCheck {
    pub discord_linked: bool,
    pub myriad_configured: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_match: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name_match: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XEngagementStats {
    pub total_posts: usize,
    pub total_likes_received: i64,
    pub total_retweets_received: i64,
    pub total_replies_received: i64,
    pub total_impressions: i64,
    pub liked_posts_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XFollowingItem {
    pub username: String,
    pub name: String,
    pub description: String,
    pub follower_count: i64,
    pub verified: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_image_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XPostItem {
    pub id: String,
    pub text: String,
    pub created_at: Option<String>,
    pub like_count: i64,
    pub retweet_count: i64,
    pub reply_count: i64,
    pub impression_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BilibiliAnalysis {
    pub video_summary: String,
    pub anime_analysis: Vec<super::content_databases::anime_database::CategoryAnalysis>,
    pub recent_videos: Vec<VideoItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoItem {
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SteamAnalysis {
    pub game_summary: String,
    pub genre_analysis: Vec<super::content_databases::game_database::GameGenreAnalysis>,
    pub recent_games: Vec<GameItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameItem {
    pub name: String,
    pub playtime: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubAnalysis {
    pub repo_summary: String,
    pub language_distribution: std::collections::HashMap<String, usize>,
    pub recent_repos: Vec<RepoItem>,
    pub contribution_calendar: Option<Vec<ContributionDay>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContributionDay {
    pub date: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoItem {
    pub name: String,
    pub language: Option<String>,
    pub stars: Option<i64>,
    pub forks: Option<i64>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NeteaseAnalysis {
    pub music_summary: String,
    pub artist_analysis: super::content_databases::artist_database::MusicAnalysis,
    pub recent_songs: Vec<SongItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SongItem {
    pub title: String,
    pub artist: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BangumiAnalysis {
    pub collection_summary: String,
    pub subject_type_distribution: std::collections::HashMap<String, usize>,
    pub collection_type_distribution: std::collections::HashMap<String, usize>,
    pub tag_distribution: std::collections::HashMap<String, usize>,
    pub top_rated_subjects: Vec<BangumiSubjectItem>,
    pub watching_subjects: Vec<BangumiSubjectItem>,
    pub recent_updates: Vec<BangumiSubjectItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BangumiSubjectItem {
    pub subject_id: i64,
    pub title: String,
    pub subject_type: String,
    pub collection_type: String,
    pub rate: i64,
    pub cover: Option<String>,
    pub updated_at: Option<String>,
}

/// MyAnimeList 收藏分析（结构对齐 Bangumi，便于报告卡复用）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MalAnalysis {
    pub collection_summary: String,
    pub subject_type_distribution: std::collections::HashMap<String, usize>,
    pub collection_type_distribution: std::collections::HashMap<String, usize>,
    pub tag_distribution: std::collections::HashMap<String, usize>,
    pub top_rated_subjects: Vec<MalSubjectItem>,
    pub watching_subjects: Vec<MalSubjectItem>,
    pub recent_updates: Vec<MalSubjectItem>,
    pub mean_score: Option<f64>,
    pub days_watched: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MalSubjectItem {
    pub subject_id: i64,
    pub title: String,
    pub subject_type: String,
    pub collection_type: String,
    pub rate: i64,
    pub cover: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnknownContent {
    pub content_type: String,
    pub title: String,
    #[serde(skip_serializing_if = "std::collections::HashMap::is_empty")]
    #[serde(default)]
    pub metadata: std::collections::HashMap<String, String>,
}

pub struct SmartFilter;

impl SmartFilter {
    /// 处理所有平台数据并分别保存到各平台缓存文件
    /// 🚀 优化：分平台保存，减少不必要的克隆，添加数据量限制
    pub fn process_and_save_all(all_data: &Value) -> Result<(), Box<dyn std::error::Error>> {
        // 🚀 数据量限制常量
        const MAX_VIDEOS_FOR_FILTER: usize = 200;
        const MAX_SONGS_FOR_FILTER: usize = 3000;

        let cache_dir = Path::new("cache/platforms");
        fs::create_dir_all(cache_dir)?;

        let mut processed_count = 0;

        // 1. Process Bilibili
        if let Some(bilibili_data) = all_data.get("bilibili") {
            let mut process_data = serde_json::Map::new();

            // 适配数据结构: user -> user_info
            if let Some(user) = bilibili_data.get("user") {
                process_data.insert("user_info".to_string(), user.clone());
            }

            // 适配数据结构: favorites -> videos (提取所有视频，添加数量限制)
            if let Some(favorites) = bilibili_data.get("favorites").and_then(|v| v.as_array()) {
                let mut all_videos = Vec::new();
                'outer: for fav in favorites {
                    if let Some(vids) = fav.get("videos").and_then(|v| v.as_array()) {
                        for v in vids {
                            if all_videos.len() >= MAX_VIDEOS_FOR_FILTER {
                                tracing::debug!(
                                    "🚀 Limiting videos to {} for filter",
                                    MAX_VIDEOS_FOR_FILTER
                                );
                                break 'outer;
                            }
                            all_videos.push(v.clone());
                        }
                    }
                }
                process_data.insert("videos".to_string(), Value::Array(all_videos));
            }

            // 直接传递 bangumi
            if let Some(bangumi) = bilibili_data.get("bangumi") {
                process_data.insert("bangumi".to_string(), bangumi.clone());
            }

            match SmartFilter::filter("bilibili", &Value::Object(process_data)) {
                Ok(result) => {
                    Self::save_platform_cache_atomic("bilibili", &result)?;
                    processed_count += 1;
                }
                Err(e) => tracing::warn!("Bilibili filter failed: {}", e),
            }
        }

        // 2. Process Steam
        if let Some(steam_data) = all_data.get("steam") {
            let mut process_data = serde_json::Map::new();

            // 适配数据结构: user -> user_info
            if let Some(user) = steam_data.get("user") {
                process_data.insert("user_info".to_string(), user.clone());
            }

            // 适配数据结构: games -> owned_games.games
            if let Some(games) = steam_data.get("games") {
                process_data.insert(
                    "owned_games".to_string(),
                    serde_json::json!({ "games": games }),
                );
                process_data.insert(
                    "recently_played".to_string(),
                    serde_json::json!({ "games": games }),
                );
            }

            match SmartFilter::filter("steam", &Value::Object(process_data)) {
                Ok(result) => {
                    Self::save_platform_cache_atomic("steam", &result)?;
                    processed_count += 1;
                }
                Err(e) => tracing::warn!("Steam filter failed: {}", e),
            }
        }

        // 3. Process Netease
        if let Some(netease_data) = all_data.get("netease") {
            let mut process_data = serde_json::Map::new();

            // 适配数据结构: liked_songs -> playlists[0].tracks（添加数量限制）
            if let Some(liked_songs) = netease_data.get("liked_songs") {
                // 🚀 限制歌曲数量避免内存问题
                let limited_songs = if let Some(songs_array) = liked_songs.as_array() {
                    if songs_array.len() > MAX_SONGS_FOR_FILTER {
                        tracing::debug!(
                            "🚀 Limiting songs from {} to {} for filter",
                            songs_array.len(),
                            MAX_SONGS_FOR_FILTER
                        );
                        Value::Array(
                            songs_array
                                .iter()
                                .take(MAX_SONGS_FOR_FILTER)
                                .cloned()
                                .collect(),
                        )
                    } else {
                        liked_songs.clone()
                    }
                } else {
                    liked_songs.clone()
                };

                process_data.insert(
                    "playlists".to_string(),
                    serde_json::json!([{ "tracks": limited_songs }]),
                );
                process_data.insert("songs".to_string(), limited_songs);
            }

            // 传递 profile
            if let Some(profile) = netease_data.get("profile") {
                process_data.insert("profile".to_string(), profile.clone());
            }

            match SmartFilter::filter("netease", &Value::Object(process_data)) {
                Ok(result) => {
                    Self::save_platform_cache_atomic("netease", &result)?;
                    processed_count += 1;
                }
                Err(e) => {
                    tracing::warn!("Netease filter failed: {}", e);
                }
            }
        }

        // 4. Process GitHub
        if let Some(github_data) = all_data.get("github") {
            // GitHub 结构基本一致 (user, repos)
            match SmartFilter::filter("github", github_data) {
                Ok(result) => {
                    Self::save_platform_cache_atomic("github", &result)?;
                    processed_count += 1;
                }
                Err(e) => tracing::warn!("GitHub filter failed: {}", e),
            }
        }

        // 5. Process Bangumi
        if let Some(bangumi_data) = all_data.get("bangumi") {
            match SmartFilter::filter("bangumi", bangumi_data) {
                Ok(result) => {
                    Self::save_platform_cache_atomic("bangumi", &result)?;
                    processed_count += 1;
                }
                Err(e) => tracing::warn!("Bangumi filter failed: {}", e),
            }
        }

        if let Some(x_data) = all_data.get("x") {
            match SmartFilter::filter("x", x_data) {
                Ok(result) => {
                    Self::save_platform_cache_atomic("x", &result)?;
                    processed_count += 1;
                }
                Err(e) => tracing::warn!("X filter failed: {}", e),
            }
        }

        if let Some(discord_data) = all_data.get("discord") {
            match SmartFilter::filter("discord", discord_data) {
                Ok(result) => {
                    Self::save_platform_cache_atomic("discord", &result)?;
                    processed_count += 1;
                }
                Err(e) => tracing::warn!("Discord filter failed: {}", e),
            }
        }

        if let Some(mal_data) = all_data.get("mal") {
            match SmartFilter::filter("mal", mal_data) {
                Ok(result) => {
                    Self::save_platform_cache_atomic("mal", &result)?;
                    processed_count += 1;
                }
                Err(e) => tracing::warn!("MyAnimeList filter failed: {}", e),
            }
        }

        if let Some(xbox_data) = all_data.get("xbox") {
            match SmartFilter::filter("xbox", xbox_data) {
                Ok(result) => {
                    Self::save_platform_cache_atomic("xbox", &result)?;
                    processed_count += 1;
                }
                Err(e) => tracing::warn!("Xbox filter failed: {}", e),
            }
        }

        if let Some(psn_data) = all_data.get("psn") {
            match SmartFilter::filter("psn", psn_data) {
                Ok(result) => {
                    Self::save_platform_cache_atomic("psn", &result)?;
                    processed_count += 1;
                }
                Err(e) => tracing::warn!("PSN filter failed: {}", e),
            }
        }

        tracing::info!(
            "✓ Smart filtered data saved to {} platform files in cache/platforms/",
            processed_count
        );
        Ok(())
    }

    /// 🚀 原子性保存平台缓存（使用临时文件+重命名）
    fn save_platform_cache_atomic(
        platform: &str,
        data: &SmartFilteredData,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let cache_dir = Path::new("cache/platforms");
        fs::create_dir_all(cache_dir)?;

        let cache_file = cache_dir.join(format!("{}_filtered.json", platform));
        let temp_file = cache_dir.join(format!("{}_filtered.json.tmp", platform));

        // 使用 BufWriter 提高写入效率
        {
            let file = fs::File::create(&temp_file)?;
            let writer = std::io::BufWriter::with_capacity(65536, file); // 64KB buffer
            serde_json::to_writer_pretty(writer, data)?;
        }

        // 原子性重命名
        fs::rename(&temp_file, &cache_file).or_else(|_| {
            fs::copy(&temp_file, &cache_file)?;
            fs::remove_file(&temp_file)
        })?;

        tracing::debug!("✓ Saved {} filtered cache", platform);
        Ok(())
    }

    /// 智能过滤平台数据
    pub fn filter(platform: &str, raw_data: &Value) -> Result<SmartFilteredData, String> {
        match platform {
            "bilibili" => Self::filter_bilibili(raw_data),
            "steam" => Self::filter_steam(raw_data),
            "github" => Self::filter_github(raw_data),
            "netease" => Self::filter_netease(raw_data),
            "bangumi" => Self::filter_bangumi(raw_data),
            "x" => Self::filter_x(raw_data),
            "discord" => Self::filter_discord(raw_data),
            "mal" => Self::filter_mal(raw_data),
            "xbox" => Self::filter_xbox(raw_data),
            "psn" => Self::filter_psn(raw_data),
            _ => Err(format!("Unsupported platform: {}", platform)),
        }
    }

    /// Bilibili 智能过滤
    fn filter_bilibili(data: &Value) -> Result<SmartFilteredData, String> {
        // 如果缺少用户信息，使用默认值而不是报错
        let default_user_info = serde_json::json!({
            "name": "未知用户",
            "mid": 0,
            "level": 0,
            "follower": 0,
            "following": 0
        });
        let user_info = data.get("user_info").unwrap_or(&default_user_info);

        // 1. 用户摘要
        let user_summary = UserSummary {
            username: user_info
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("未知用户")
                .to_string(),
            user_id: user_info
                .get("mid")
                .and_then(|v| v.as_i64())
                .map(|i| i.to_string())
                .unwrap_or_default(),
            level: user_info
                .get("level")
                .and_then(|v| v.as_i64())
                .map(|l| format!("Lv{}", l)),
            stats: UserStats {
                follower_count: user_info.get("follower").and_then(|v| v.as_i64()),
                following_count: user_info.get("following").and_then(|v| v.as_i64()),
                total_content: 0, // 后续计算
            },
        };

        // 2. 收集所有视频信息
        let videos = data.get("videos").and_then(|v| v.as_array());
        let mut recent_videos = Vec::new();

        if let Some(vids) = videos {
            // 保留所有视频
            for video in vids {
                if let Some(title) = video.get("title").and_then(|v| v.as_str()) {
                    recent_videos.push(VideoItem {
                        title: title.to_string(),
                    });
                }
            }
        }

        // 3. 使用动画数据库分析番剧/电视剧/电影
        let bangumi = data.get("bangumi").and_then(|v| v.as_array());
        let mut watch_list = Vec::new();

        if let Some(items) = bangumi {
            for item in items {
                if let Some(title) = item.get("title").and_then(|v| v.as_str()) {
                    let author = item
                        .get("author")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    watch_list.push((title.to_string(), author));
                }
            }
        }

        let anime_db = AnimeDatabase::new();
        let anime_analysis = anime_db.analyze(watch_list.clone());

        // 找出未知的番剧内容
        let mut raw_unknown_content = Vec::new();
        for (title, _author) in watch_list.iter() {
            if anime_db.find(title).is_none() {
                let metadata = std::collections::HashMap::new();
                // metadata.insert("author".to_string(), author.clone()); // 不需要具体的metadata
                raw_unknown_content.push(UnknownContent {
                    content_type: "Bangumi".to_string(),
                    title: title.clone(),
                    metadata,
                });
            }
        }
        tracing::info!(
            "Bilibili: watch_list size: {}, unknown size: {}",
            watch_list.len(),
            raw_unknown_content.len()
        );

        let video_summary = format!(
            "基于收藏的 {} 个视频和追番的 {} 部作品分析",
            videos.map(|v| v.len()).unwrap_or(0),
            bangumi.map(|b| b.len()).unwrap_or(0)
        );

        let content_analysis = ContentAnalysis::Bilibili(BilibiliAnalysis {
            video_summary,
            anime_analysis,
            recent_videos,
        });

        Ok(SmartFilteredData {
            platform: "bilibili".to_string(),
            user_summary,
            content_analysis,
            raw_unknown_content,
        })
    }

    /// Steam 智能过滤
    fn filter_steam(data: &Value) -> Result<SmartFilteredData, String> {
        let user_info = data.get("user_info");

        // 1. 用户摘要
        let user_summary = UserSummary {
            username: user_info
                .and_then(|u| u.get("personaname"))
                .and_then(|v| v.as_str())
                .unwrap_or("未知用户")
                .to_string(),
            user_id: user_info
                .and_then(|u| u.get("steamid"))
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            level: None,
            stats: UserStats {
                follower_count: None,
                following_count: None,
                total_content: 0,
            },
        };

        // 2. 收集所有游戏信息
        let owned_games = data
            .get("owned_games")
            .and_then(|v| v.get("games"))
            .and_then(|v| v.as_array());
        let recently_played = data
            .get("recently_played")
            .and_then(|v| v.get("games"))
            .and_then(|v| v.as_array());

        let mut game_list = Vec::new();
        let mut recent_games = Vec::new();

        // 收集所有拥有的游戏
        if let Some(games) = owned_games {
            for game in games {
                if let Some(name) = game.get("name").and_then(|v| v.as_str()) {
                    let playtime = game
                        .get("playtime_forever")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                    game_list.push((name.to_string(), playtime));
                }
            }
        }

        // 收集最近玩的游戏
        if let Some(games) = recently_played {
            for game in games {
                if let Some(name) = game.get("name").and_then(|v| v.as_str()) {
                    let playtime = game
                        .get("playtime_forever")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                    recent_games.push(GameItem {
                        name: name.to_string(),
                        playtime,
                    });
                }
            }
        }

        // 3. 使用游戏数据库分析
        let game_db = GameDatabase::new();
        let game_analysis = game_db.analyze(game_list);

        // 收集未知的游戏内容
        let mut raw_unknown_content = Vec::new();
        for (name, playtime) in game_analysis.unknown_games {
            let mut metadata = std::collections::HashMap::new();
            metadata.insert("playtime".to_string(), playtime.to_string());
            raw_unknown_content.push(UnknownContent {
                content_type: "Game".to_string(),
                title: name,
                metadata,
            });
        }

        let content_analysis = ContentAnalysis::Steam(SteamAnalysis {
            game_summary: game_analysis.summary.clone(),
            genre_analysis: game_analysis.genre_analysis,
            recent_games,
        });

        Ok(SmartFilteredData {
            platform: "steam".to_string(),
            user_summary,
            content_analysis,
            raw_unknown_content,
        })
    }

    /// GitHub 智能过滤
    fn filter_github(data: &Value) -> Result<SmartFilteredData, String> {
        let user = data.get("user");

        // 1. 用户摘要
        let user_summary = UserSummary {
            username: user
                .and_then(|u| u.get("login"))
                .and_then(|v| v.as_str())
                .unwrap_or("未知用户")
                .to_string(),
            user_id: user
                .and_then(|u| u.get("id"))
                .and_then(|v| v.as_i64())
                .map(|i| i.to_string())
                .unwrap_or_default(),
            level: None,
            stats: UserStats {
                follower_count: user
                    .and_then(|u| u.get("followers"))
                    .and_then(|v| v.as_i64()),
                following_count: user
                    .and_then(|u| u.get("following"))
                    .and_then(|v| v.as_i64()),
                total_content: 0,
            },
        };

        // 2. 收集所有仓库信息
        let repos = data.get("repos").and_then(|v| v.as_array());
        let mut recent_repos = Vec::new();
        let mut language_distribution = std::collections::HashMap::new();

        if let Some(repo_list) = repos {
            // 保留所有仓库
            for repo in repo_list {
                if let Some(name) = repo.get("name").and_then(|v| v.as_str()) {
                    let language = repo
                        .get("language")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let stars = repo.get("stargazers_count").and_then(|v| v.as_i64());
                    let forks = repo.get("forks_count").and_then(|v| v.as_i64());
                    let description = repo
                        .get("description")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    recent_repos.push(RepoItem {
                        name: name.to_string(),
                        language: language.clone(),
                        stars,
                        forks,
                        description,
                    });

                    // 统计编程语言
                    if let Some(lang) = language {
                        *language_distribution.entry(lang).or_insert(0) += 1;
                    }
                }
            }

            // 统计所有仓库的语言分布 (Wait, I was iterating twice before, now I can just do it once if I remove the limit)
            // Actually, the previous code iterated `take(10)` for `recent_repos` and then iterated ALL for `language_distribution`.
            // Now I iterate ALL for `recent_repos`, so I can do language distribution in the same loop.
            // But wait, the previous code had a second loop:
            // for repo in repo_list { ... }
            // If I merge them, I need to be careful.
            // Let's just remove the limit in the first loop and remove the second loop if it's redundant.
            // The first loop now iterates all repos.
            // So `language_distribution` is populated for all repos in the first loop.
            // The second loop is now redundant.
        }

        let repo_summary = format!(
            "拥有 {} 个仓库，主要使用 {}",
            repos.map(|r| r.len()).unwrap_or(0),
            language_distribution
                .iter()
                .map(|(k, v)| format!("{} ({})", k, v))
                .collect::<Vec<_>>()
                .join("、")
        );

        // 3. 提取贡献历史（直接从原始数据中获取）
        let contribution_calendar = data
            .get("contribution_calendar")
            .and_then(|v| v.as_array())
            .map(|calendar| {
                let contributions: Vec<ContributionDay> = calendar
                    .iter()
                    .filter_map(|day| {
                        let date = day.get("date").and_then(|v| v.as_str())?;
                        let count = day.get("count").and_then(|v| v.as_i64())?;
                        Some(ContributionDay {
                            date: date.to_string(),
                            count,
                        })
                    })
                    .collect();
                tracing::info!(
                    "📊 Extracted {} contribution days from GitHub data",
                    contributions.len()
                );
                contributions
            });

        let content_analysis = ContentAnalysis::GitHub(GitHubAnalysis {
            repo_summary,
            language_distribution,
            recent_repos,
            contribution_calendar,
        });

        Ok(SmartFilteredData {
            platform: "github".to_string(),
            user_summary,
            content_analysis,
            raw_unknown_content: vec![],
        })
    }

    /// 网易云音乐智能过滤
    fn filter_netease(data: &Value) -> Result<SmartFilteredData, String> {
        tracing::debug!("🎵 Processing Netease data...");

        let profile = data.get("profile");

        // 1. 用户摘要（使用更宽松的默认值）
        let user_summary = UserSummary {
            username: profile
                .and_then(|p| p.get("nickname"))
                .and_then(|v| v.as_str())
                .or_else(|| {
                    // 如果profile中没有nickname，尝试从其他可能的位置获取
                    data.get("user")
                        .and_then(|u| u.get("nickname"))
                        .and_then(|v| v.as_str())
                })
                .unwrap_or("网易云音乐用户")
                .to_string(),
            user_id: profile
                .and_then(|p| p.get("userId"))
                .and_then(|v| v.as_i64())
                .or_else(|| {
                    data.get("user")
                        .and_then(|u| u.get("userId"))
                        .and_then(|v| v.as_i64())
                })
                .map(|i| i.to_string())
                .unwrap_or_default(),
            level: profile
                .and_then(|p| p.get("level"))
                .and_then(|v| v.as_i64())
                .or_else(|| {
                    data.get("user")
                        .and_then(|u| u.get("level"))
                        .and_then(|v| v.as_i64())
                })
                .map(|l| format!("Lv{}", l)),
            stats: UserStats {
                follower_count: profile
                    .and_then(|p| p.get("followeds"))
                    .and_then(|v| v.as_i64()),
                following_count: profile
                    .and_then(|p| p.get("follows"))
                    .and_then(|v| v.as_i64()),
                total_content: 0,
            },
        };

        // 2. 收集所有歌曲信息（支持多种数据结构）
        let playlists = data.get("playlists").and_then(|v| v.as_array());
        let mut song_list = Vec::new();
        let mut recent_songs = Vec::new();

        if let Some(lists) = playlists {
            tracing::debug!("  - Found {} playlists", lists.len());
            for playlist in lists {
                if let Some(tracks) = playlist.get("tracks").and_then(|v| v.as_array()) {
                    tracing::debug!("  - Processing playlist with {} tracks", tracks.len());
                    for track in tracks {
                        if let Some(name) = track.get("name").and_then(|v| v.as_str()) {
                            // 尝试多种方式获取艺术家名称
                            let artist = track
                                .get("ar") // 标准字段
                                .and_then(|v| v.as_array())
                                .and_then(|arr| arr.first())
                                .and_then(|a| a.get("name"))
                                .and_then(|v| v.as_str())
                                .or_else(|| {
                                    // 备用字段：artists
                                    track
                                        .get("artists")
                                        .and_then(|v| v.as_array())
                                        .and_then(|arr| arr.first())
                                        .and_then(|a| a.get("name"))
                                        .and_then(|v| v.as_str())
                                })
                                .unwrap_or("未知艺术家");

                            song_list.push((name.to_string(), artist.to_string()));

                            // 保留所有歌曲
                            recent_songs.push(SongItem {
                                title: name.to_string(),
                                artist: artist.to_string(),
                            });
                        }
                    }
                }
            }
        } else {
            tracing::warn!("  ⚠️ No playlists found in Netease data");
        }

        if song_list.is_empty() {
            tracing::warn!("  ⚠️ No songs collected from Netease data");
        } else {
            tracing::info!("  ✓ Collected {} songs from Netease", song_list.len());
        }

        // 3. 使用歌手数据库分析
        let artist_db = ArtistDatabase::new();
        let music_analysis = artist_db.analyze(song_list);

        let content_analysis = ContentAnalysis::Netease(NeteaseAnalysis {
            music_summary: music_analysis.summary.clone(),
            artist_analysis: music_analysis,
            recent_songs,
        });

        Ok(SmartFilteredData {
            platform: "netease".to_string(),
            user_summary,
            content_analysis,
            raw_unknown_content: vec![],
        })
    }

    fn filter_bangumi(data: &Value) -> Result<SmartFilteredData, String> {
        let user = data.get("user");
        let collections = data
            .get("collections")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let user_summary = UserSummary {
            username: user
                .and_then(|u| u.get("nickname"))
                .and_then(|v| v.as_str())
                .or_else(|| {
                    user.and_then(|u| u.get("username"))
                        .and_then(|v| v.as_str())
                })
                .unwrap_or("Bangumi 用户")
                .to_string(),
            user_id: user
                .and_then(|u| u.get("id"))
                .and_then(|v| v.as_i64())
                .map(|id| id.to_string())
                .unwrap_or_default(),
            level: None,
            stats: UserStats {
                follower_count: None,
                following_count: None,
                total_content: collections.len(),
            },
        };

        let mut subject_type_distribution = std::collections::HashMap::new();
        let mut collection_type_distribution = std::collections::HashMap::new();
        let mut tag_distribution = std::collections::HashMap::new();
        let mut subjects = Vec::new();

        for collection in &collections {
            let subject_id = collection
                .get("subject_id")
                .and_then(|v| v.as_i64())
                .or_else(|| {
                    collection
                        .get("subject")
                        .and_then(|s| s.get("id"))
                        .and_then(|v| v.as_i64())
                })
                .unwrap_or(0);
            let subject_type = collection
                .get("subject_type")
                .and_then(|v| v.as_i64())
                .map(Self::bangumi_subject_type_label)
                .unwrap_or("unknown");
            let collection_type = collection
                .get("type")
                .and_then(|v| v.as_i64())
                .map(Self::bangumi_collection_type_label)
                .unwrap_or("unknown");
            let rate = collection.get("rate").and_then(|v| v.as_i64()).unwrap_or(0);
            let subject = collection.get("subject");
            let title = subject
                .and_then(|s| s.get("name_cn"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .or_else(|| subject.and_then(|s| s.get("name")).and_then(|v| v.as_str()))
                .unwrap_or("Unknown")
                .to_string();
            let cover = subject
                .and_then(|s| s.get("images"))
                .and_then(|images| {
                    images
                        .get("large")
                        .or_else(|| images.get("common"))
                        .or_else(|| images.get("medium"))
                })
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let updated_at = collection
                .get("updated_at")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            *subject_type_distribution
                .entry(subject_type.to_string())
                .or_insert(0) += 1;
            *collection_type_distribution
                .entry(collection_type.to_string())
                .or_insert(0) += 1;

            if let Some(tags) = collection.get("tags").and_then(|v| v.as_array()) {
                for tag in tags {
                    if let Some(tag) = tag.as_str().filter(|s| !s.is_empty()) {
                        *tag_distribution.entry(tag.to_string()).or_insert(0) += 1;
                    }
                }
            }

            subjects.push(BangumiSubjectItem {
                subject_id,
                title,
                subject_type: subject_type.to_string(),
                collection_type: collection_type.to_string(),
                rate,
                cover,
                updated_at,
            });
        }

        let mut top_rated_subjects = subjects.clone();
        top_rated_subjects.sort_by_key(|b| Reverse(b.rate));
        top_rated_subjects.truncate(20);

        let watching_subjects = subjects
            .iter()
            .filter(|item| item.collection_type == "doing")
            .take(20)
            .cloned()
            .collect::<Vec<_>>();

        let mut recent_updates = subjects.clone();
        recent_updates.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        recent_updates.truncate(20);

        let collection_summary = format!(
            "Bangumi 收藏 {} 个条目，其中看过/读过/玩过 {} 个，正在进行 {} 个",
            collections.len(),
            collection_type_distribution
                .get("done")
                .copied()
                .unwrap_or_default(),
            collection_type_distribution
                .get("doing")
                .copied()
                .unwrap_or_default()
        );

        Ok(SmartFilteredData {
            platform: "bangumi".to_string(),
            user_summary,
            content_analysis: ContentAnalysis::Bangumi(BangumiAnalysis {
                collection_summary,
                subject_type_distribution,
                collection_type_distribution,
                tag_distribution,
                top_rated_subjects,
                watching_subjects,
                recent_updates,
            }),
            raw_unknown_content: vec![],
        })
    }

    fn filter_mal(data: &Value) -> Result<SmartFilteredData, String> {
        let user = data.get("user");
        let anime_list = data
            .get("anime_list")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let manga_list = data
            .get("manga_list")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let user_summary = UserSummary {
            username: user
                .and_then(|u| u.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("MyAnimeList 用户")
                .to_string(),
            user_id: user
                .and_then(|u| u.get("id"))
                .and_then(|v| v.as_i64())
                .map(|id| id.to_string())
                .unwrap_or_default(),
            level: None,
            stats: UserStats {
                follower_count: None,
                following_count: None,
                total_content: anime_list.len() + manga_list.len(),
            },
        };

        let mut subject_type_distribution = std::collections::HashMap::new();
        let mut collection_type_distribution = std::collections::HashMap::new();
        let mut tag_distribution = std::collections::HashMap::new();
        let mut subjects = Vec::new();

        let parse_entry = |entry: &Value, media_kind: &str| -> Option<MalSubjectItem> {
            let node = entry.get("node")?;
            let list_status = entry.get("list_status");
            let subject_id = node.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
            let title = node
                .get("title")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .or_else(|| {
                    node.pointer("/alternative_titles/en")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                })
                .unwrap_or("Unknown")
                .to_string();
            let cover = node
                .pointer("/main_picture/medium")
                .or_else(|| node.pointer("/main_picture/large"))
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let raw_status = list_status
                .and_then(|s| s.get("status"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let collection_type = Self::mal_status_label(raw_status).to_string();
            let rate = list_status
                .and_then(|s| s.get("score"))
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let updated_at = list_status
                .and_then(|s| s.get("updated_at"))
                .and_then(|v| v.as_str())
                .map(str::to_string);

            Some(MalSubjectItem {
                subject_id,
                title,
                subject_type: media_kind.to_string(),
                collection_type,
                rate,
                cover,
                updated_at,
            })
        };

        for entry in &anime_list {
            if let Some(item) = parse_entry(entry, "anime") {
                *subject_type_distribution
                    .entry("anime".to_string())
                    .or_insert(0) += 1;
                *collection_type_distribution
                    .entry(item.collection_type.clone())
                    .or_insert(0) += 1;
                if let Some(genres) = entry
                    .get("node")
                    .and_then(|n| n.get("genres"))
                    .and_then(|v| v.as_array())
                {
                    for genre in genres {
                        if let Some(name) = genre
                            .get("name")
                            .and_then(|v| v.as_str())
                            .filter(|s| !s.is_empty())
                        {
                            *tag_distribution.entry(name.to_string()).or_insert(0) += 1;
                        }
                    }
                }
                subjects.push(item);
            }
        }

        for entry in &manga_list {
            if let Some(item) = parse_entry(entry, "manga") {
                *subject_type_distribution
                    .entry("manga".to_string())
                    .or_insert(0) += 1;
                *collection_type_distribution
                    .entry(item.collection_type.clone())
                    .or_insert(0) += 1;
                if let Some(genres) = entry
                    .get("node")
                    .and_then(|n| n.get("genres"))
                    .and_then(|v| v.as_array())
                {
                    for genre in genres {
                        if let Some(name) = genre
                            .get("name")
                            .and_then(|v| v.as_str())
                            .filter(|s| !s.is_empty())
                        {
                            *tag_distribution.entry(name.to_string()).or_insert(0) += 1;
                        }
                    }
                }
                subjects.push(item);
            }
        }

        let mut top_rated_subjects = subjects.clone();
        top_rated_subjects.sort_by_key(|b| Reverse(b.rate));
        top_rated_subjects.truncate(20);

        let watching_subjects = subjects
            .iter()
            .filter(|item| item.collection_type == "doing")
            .take(20)
            .cloned()
            .collect::<Vec<_>>();

        let mut recent_updates = subjects.clone();
        recent_updates.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        recent_updates.truncate(20);

        let mean_score = user
            .and_then(|u| u.pointer("/anime_statistics/mean_score"))
            .and_then(|v| v.as_f64());
        let days_watched = user
            .and_then(|u| u.pointer("/anime_statistics/num_days"))
            .and_then(|v| v.as_f64());

        let collection_summary = format!(
            "MyAnimeList 收藏 {} 部（动画 {} / 漫画 {}），完成 {} 部，正在进行 {} 部",
            subjects.len(),
            subject_type_distribution
                .get("anime")
                .copied()
                .unwrap_or_default(),
            subject_type_distribution
                .get("manga")
                .copied()
                .unwrap_or_default(),
            collection_type_distribution
                .get("done")
                .copied()
                .unwrap_or_default(),
            collection_type_distribution
                .get("doing")
                .copied()
                .unwrap_or_default()
        );

        Ok(SmartFilteredData {
            platform: "mal".to_string(),
            user_summary,
            content_analysis: ContentAnalysis::Mal(MalAnalysis {
                collection_summary,
                subject_type_distribution,
                collection_type_distribution,
                tag_distribution,
                top_rated_subjects,
                watching_subjects,
                recent_updates,
                mean_score,
                days_watched,
            }),
            raw_unknown_content: vec![],
        })
    }

    /// Xbox 成就过滤：OpenXBL achievements bundle → 成就向画像
    ///
    /// 可拿字段全部榨干：profile settings（头像/GS/等级/信誉/展示名）+
    /// titles（进度/封面/设备/最近游玩）。平均完成度只计「有成就系统」的作品，
    /// 避免 PC 商店无成就条目把均值压到接近 0。
    fn filter_xbox(data: &Value) -> Result<SmartFilteredData, String> {
        let fallback_gamertag = data
            .get("gamertag")
            .and_then(|v| v.as_str())
            .unwrap_or("Xbox 玩家")
            .to_string();
        let xuid = data
            .get("xuid")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        // profile settings → GS / 头像 / 展示名 / 账号档 / 信誉
        let mut gamerscore: i64 = 0;
        let mut avatar: Option<String> = None;
        let mut display_gamertag: Option<String> = None;
        let mut account_tier: Option<String> = None;
        let mut reputation: Option<String> = None;
        if let Some(settings) = data
            .pointer("/profile/profileUsers/0/settings")
            .and_then(|v| v.as_array())
        {
            for s in settings {
                let id = s.get("id").and_then(|v| v.as_str()).unwrap_or("");
                let val = s.get("value").and_then(|v| v.as_str()).unwrap_or("");
                if val.is_empty() {
                    continue;
                }
                match id {
                    "Gamerscore" => {
                        gamerscore = val.parse::<i64>().unwrap_or(0);
                    }
                    "GameDisplayPicRaw" | "PublicGamerpic" => {
                        if avatar.is_none() {
                            avatar = Some(Self::normalize_xbox_media_url(val));
                        }
                    }
                    "UniqueModernGamertag" | "ModernGamertag" | "Gamertag" => {
                        // 优先 UniqueModern（含 #suffix），已有更完整值则不覆盖
                        if display_gamertag
                            .as_ref()
                            .map(|g| !g.contains('#'))
                            .unwrap_or(true)
                            || id == "UniqueModernGamertag"
                        {
                            display_gamertag = Some(val.to_string());
                        }
                    }
                    "AccountTier" => account_tier = Some(val.to_string()),
                    "XboxOneRep" => reputation = Some(val.to_string()),
                    _ => {}
                }
            }
        }

        let gamertag = display_gamertag
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(fallback_gamertag);

        let raw_titles = data
            .pointer("/achievements/titles")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let titles: Vec<XboxTitleItem> = raw_titles
            .iter()
            .filter_map(|t| {
                let name = t.get("name").and_then(|v| v.as_str())?.to_string();
                // 过滤掉非游戏条目（如 App）
                if t.get("type").and_then(|v| v.as_str()) == Some("App") {
                    return None;
                }
                // 过滤明显启动器/壳应用
                let name_l = name.to_lowercase();
                if name_l.contains("launcher") || name_l.ends_with(" app") {
                    return None;
                }
                let ach = t.get("achievement");
                let earned = ach
                    .and_then(|a| a.get("currentAchievements"))
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                let total = ach
                    .and_then(|a| a.get("totalAchievements"))
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                let gs_earned = ach
                    .and_then(|a| a.get("currentGamerscore"))
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                let gs_total = ach
                    .and_then(|a| a.get("totalGamerscore"))
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                let progress = ach
                    .and_then(|a| a.get("progressPercentage"))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                let devices = t
                    .get("devices")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|d| d.as_str().map(str::to_string))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let display_image = t
                    .get("displayImage")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(Self::normalize_xbox_media_url);
                Some(XboxTitleItem {
                    title_id: t
                        .get("titleId")
                        .and_then(|v| {
                            v.as_str()
                                .map(str::to_string)
                                .or_else(|| v.as_u64().map(|n| n.to_string()))
                        })
                        .unwrap_or_default(),
                    name,
                    display_image,
                    achievements_earned: earned,
                    achievements_total: total,
                    gamerscore_earned: gs_earned,
                    gamerscore_total: gs_total,
                    progress,
                    last_played: t
                        .pointer("/titleHistory/lastTimePlayed")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    devices,
                })
            })
            .collect();

        let games_count = titles.len();
        // 只在「有成就系统」的作品上算完成度，避免无成就 PC 条目稀释均值
        let achievement_titles: Vec<&XboxTitleItem> = titles
            .iter()
            .filter(|t| t.achievements_total > 0 || t.gamerscore_total > 0)
            .collect();
        let achievement_games = achievement_titles.len();
        let completed_games = titles.iter().filter(|t| t.progress >= 100.0).count();
        let total_achievements_earned: i64 = titles.iter().map(|t| t.achievements_earned).sum();
        let total_achievements_available: i64 = achievement_titles
            .iter()
            .map(|t| t.achievements_total)
            .sum();
        let average_completion = if achievement_games > 0 {
            achievement_titles.iter().map(|t| t.progress).sum::<f64>() / achievement_games as f64
        } else {
            0.0
        };

        // 硬核指数 0-100：完成度主导 + 全成就密度 + GS 规模（log）+ 成就解锁量
        // 目标：轻度玩家（几十 GS）落在 10-30，中坚 40-70，猎人 80+
        let hardcore_score = {
            let completion_part = (average_completion * 0.45).clamp(0.0, 45.0);
            let complete_ratio = if achievement_games > 0 {
                completed_games as f64 / achievement_games as f64
            } else {
                0.0
            };
            let complete_part = (complete_ratio * 25.0).clamp(0.0, 25.0);
            let gs_part = if gamerscore > 0 {
                // log10(1+gs) / log10(1+100000) * 20 → 100k GS 打满 20 分
                let ratio =
                    ((1.0 + gamerscore as f64).ln() / (1.0_f64 + 100_000.0).ln()).clamp(0.0, 1.0);
                ratio * 20.0
            } else {
                0.0
            };
            let ach_part = if total_achievements_earned > 0 {
                // 500 成就打满 10 分
                ((total_achievements_earned as f64 / 500.0).min(1.0)) * 10.0
            } else {
                0.0
            };
            (completion_part + complete_part + gs_part + ach_part)
                .round()
                .clamp(0.0, 100.0) as i64
        };

        let mut recent_titles = titles.clone();
        // 最近游玩：有 last_played 的排前面；同时间优先有封面的
        recent_titles.sort_by(|a, b| {
            b.last_played
                .cmp(&a.last_played)
                .then_with(|| b.display_image.is_some().cmp(&a.display_image.is_some()))
        });
        recent_titles.truncate(20);

        // 完成度排序时只看有进度的游戏，避免一堆 0% 噪音
        let mut top_completed = titles;
        top_completed.retain(|t| t.achievements_earned > 0 || t.progress > 0.0);
        top_completed.sort_by(|a, b| {
            b.progress
                .partial_cmp(&a.progress)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(b.gamerscore_earned.cmp(&a.gamerscore_earned))
        });
        top_completed.truncate(20);

        let gaming_summary = format!(
            "Xbox Gamerscore {}，共 {} 款游戏（{} 款含成就），{} 款全成就，累计解锁 {}/{} 成就，平均完成度 {:.1}%",
            gamerscore,
            games_count,
            achievement_games,
            completed_games,
            total_achievements_earned,
            total_achievements_available,
            average_completion
        );

        Ok(SmartFilteredData {
            platform: "xbox".to_string(),
            user_summary: UserSummary {
                username: gamertag.clone(),
                user_id: xuid,
                level: None,
                stats: UserStats {
                    follower_count: None,
                    following_count: None,
                    total_content: games_count,
                },
            },
            content_analysis: ContentAnalysis::Xbox(XboxAnalysis {
                gaming_summary,
                gamerscore,
                games_count,
                achievement_games,
                completed_games,
                total_achievements_earned,
                total_achievements_available,
                average_completion,
                hardcore_score,
                account_tier,
                avatar,
                display_gamertag: Some(gamertag),
                reputation,
                recent_titles,
                top_completed_titles: top_completed,
            }),
            raw_unknown_content: vec![],
        })
    }

    /// PSN 奖杯过滤：trophyTitles bundle → 奖杯向画像
    /// PSN 奖杯过滤：trophySummary + trophyTitles + social_metadata → 奖杯向画像
    fn filter_psn(data: &Value) -> Result<SmartFilteredData, String> {
        let fallback_id = data
            .get("online_id")
            .and_then(|v| v.as_str())
            .unwrap_or("PSN 玩家")
            .to_string();
        let account_id = data
            .get("account_id")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        // social_metadata：头像 / 展示名 / Plus
        let social = data.get("social_metadata");
        let mut avatar: Option<String> = social
            .and_then(|s| {
                s.get("avatarUrl")
                    .or_else(|| s.get("avatar"))
                    .or_else(|| s.pointer("/avatarUrls/0/avatarUrl"))
            })
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(Self::normalize_https_media_url);
        let display_from_social = social
            .and_then(|s| s.get("onlineId").or_else(|| s.get("online_id")))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let is_plus = social.and_then(|s| {
            s.get("isPlus").or_else(|| s.get("plus")).and_then(|v| {
                v.as_bool().or_else(|| {
                    v.as_i64()
                        .map(|n| n != 0)
                        .or_else(|| v.as_str().map(|s| s == "true" || s == "1"))
                })
            })
        });
        // 有时头像在 profile 结构外
        if avatar.is_none() {
            avatar = data
                .pointer("/social_metadata/profilePictureUrls/0/profilePictureUrl")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(Self::normalize_https_media_url);
        }

        let online_id = display_from_social
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(fallback_id);

        let summary = data.get("trophy_summary");
        let trophy_level = summary
            .and_then(|s| s.get("trophyLevel"))
            .and_then(|v| {
                v.as_i64()
                    .or_else(|| v.as_str().and_then(|s| s.parse::<i64>().ok()))
            })
            .unwrap_or(0);
        let earned = summary.and_then(|s| s.get("earnedTrophies"));
        let count_of = |kind: &str| -> i64 {
            earned
                .and_then(|e| e.get(kind))
                .and_then(|v| v.as_i64())
                .unwrap_or(0)
        };
        let platinum_count = count_of("platinum");
        let gold_count = count_of("gold");
        let silver_count = count_of("silver");
        let bronze_count = count_of("bronze");
        let total_trophies = platinum_count + gold_count + silver_count + bronze_count;

        let raw_titles = data
            .get("trophy_titles")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let titles: Vec<PsnTitleItem> = raw_titles
            .iter()
            .filter_map(|t| {
                let name = t
                    .get("trophyTitleName")
                    .and_then(|v| v.as_str())?
                    .to_string();
                // 跳过隐藏/空壳
                if name.trim().is_empty() {
                    return None;
                }
                let earned = t.get("earnedTrophies");
                let earned_of = |kind: &str| -> i64 {
                    earned
                        .and_then(|e| e.get(kind))
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0)
                };
                let icon_url = t
                    .get("trophyTitleIconUrl")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(Self::normalize_https_media_url);
                Some(PsnTitleItem {
                    name,
                    platform: t
                        .get("trophyTitlePlatform")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    icon_url,
                    progress: t.get("progress").and_then(|v| v.as_i64()).unwrap_or(0),
                    earned_platinum: earned_of("platinum"),
                    earned_gold: earned_of("gold"),
                    earned_silver: earned_of("silver"),
                    earned_bronze: earned_of("bronze"),
                    last_updated: t
                        .get("lastUpdatedDateTime")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                })
            })
            .collect();

        let games_count = titles.len();
        let completed_games = titles.iter().filter(|t| t.progress >= 100).count();
        // 只对有进度或有奖杯的作品算平均完成度，避免 0% 占位稀释
        let progressed: Vec<&PsnTitleItem> = titles
            .iter()
            .filter(|t| {
                t.progress > 0
                    || t.earned_platinum + t.earned_gold + t.earned_silver + t.earned_bronze > 0
            })
            .collect();
        let average_progress = if !progressed.is_empty() {
            progressed.iter().map(|t| t.progress as f64).sum::<f64>() / progressed.len() as f64
        } else if games_count > 0 {
            titles.iter().map(|t| t.progress as f64).sum::<f64>() / games_count as f64
        } else {
            0.0
        };

        // 猎人指数：白金主导 + 等级 + 完成度 + 通关密度
        // 目标：0 白金轻度 ~10-30，数枚白金 40-70，双位数白金/高完成 80+
        let hardcore_score = {
            let plat_part = ((platinum_count as f64) * 4.0).min(40.0);
            let level_part = if trophy_level > 0 {
                // lv 1→~0, lv 100→~20, lv 400→~25 封顶
                ((trophy_level as f64).ln() / (400.0_f64).ln() * 25.0).clamp(0.0, 25.0)
            } else {
                0.0
            };
            let completion_part = (average_progress * 0.25).clamp(0.0, 25.0);
            let complete_ratio = if games_count > 0 {
                completed_games as f64 / games_count as f64
            } else {
                0.0
            };
            let complete_part = (complete_ratio * 10.0).clamp(0.0, 10.0);
            (plat_part + level_part + completion_part + complete_part)
                .round()
                .clamp(0.0, 100.0) as i64
        };

        let mut recent_titles = titles.clone();
        recent_titles.sort_by(|a, b| {
            b.last_updated
                .cmp(&a.last_updated)
                .then_with(|| b.icon_url.is_some().cmp(&a.icon_url.is_some()))
        });
        recent_titles.truncate(20);

        let mut top_completed = titles;
        // 完成度排序：有进度优先，白金优先
        top_completed.retain(|t| {
            t.progress > 0
                || t.earned_platinum + t.earned_gold + t.earned_silver + t.earned_bronze > 0
        });
        top_completed.sort_by(|a, b| {
            b.progress
                .cmp(&a.progress)
                .then(b.earned_platinum.cmp(&a.earned_platinum))
                .then(
                    (b.earned_gold + b.earned_silver + b.earned_bronze)
                        .cmp(&(a.earned_gold + a.earned_silver + a.earned_bronze)),
                )
        });
        top_completed.truncate(20);

        let trophy_summary_text = format!(
            "PSN 奖杯等级 {}，白金 {} / 金 {} / 银 {} / 铜 {}（共 {}），{} 款游戏，{} 款 100% 完成，平均完成度 {:.1}%",
            trophy_level,
            platinum_count,
            gold_count,
            silver_count,
            bronze_count,
            total_trophies,
            games_count,
            completed_games,
            average_progress
        );

        Ok(SmartFilteredData {
            platform: "psn".to_string(),
            user_summary: UserSummary {
                username: online_id.clone(),
                user_id: account_id,
                level: Some(format!("Lv.{trophy_level}")),
                stats: UserStats {
                    follower_count: None,
                    following_count: None,
                    total_content: games_count,
                },
            },
            content_analysis: ContentAnalysis::Psn(PsnAnalysis {
                trophy_summary_text,
                trophy_level,
                platinum_count,
                gold_count,
                silver_count,
                bronze_count,
                total_trophies,
                games_count,
                completed_games,
                average_progress,
                hardcore_score,
                avatar,
                display_online_id: Some(online_id),
                is_plus,
                recent_titles,
                top_completed_titles: top_completed,
            }),
            raw_unknown_content: vec![],
        })
    }

    fn filter_x(data: &Value) -> Result<SmartFilteredData, String> {
        let user = data.get("user").unwrap_or(&Value::Null);
        let username = user
            .get("username")
            .or_else(|| user.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let user_id = user
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let metrics = user.get("public_metrics").cloned().unwrap_or(Value::Null);
        let follower_count = metrics.get("followers_count").and_then(|v| v.as_i64());
        let following_count = metrics.get("following_count").and_then(|v| v.as_i64());
        let tweet_count_metric = metrics
            .get("tweet_count")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);

        let tweets = data
            .get("tweets")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let mut language_distribution = std::collections::HashMap::new();
        let mut total_likes = 0i64;
        let mut total_retweets = 0i64;
        let mut total_replies = 0i64;
        let mut total_impressions = 0i64;

        let mut post_items: Vec<XPostItem> = Vec::new();
        for tweet in &tweets {
            let id = tweet
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let text = tweet
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let created_at = tweet
                .get("created_at")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let pm = tweet.get("public_metrics").cloned().unwrap_or(Value::Null);
            let like_count = pm.get("like_count").and_then(|v| v.as_i64()).unwrap_or(0);
            let retweet_count = pm
                .get("retweet_count")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let reply_count = pm.get("reply_count").and_then(|v| v.as_i64()).unwrap_or(0);
            let impression_count = pm
                .get("impression_count")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);

            total_likes += like_count;
            total_retweets += retweet_count;
            total_replies += reply_count;
            total_impressions += impression_count;

            if let Some(lang) = tweet.get("lang").and_then(|v| v.as_str()) {
                *language_distribution.entry(lang.to_string()).or_insert(0) += 1;
            }

            post_items.push(XPostItem {
                id,
                text,
                created_at,
                like_count,
                retweet_count,
                reply_count,
                impression_count,
            });
        }

        let mut top_posts = post_items.clone();
        top_posts.sort_by(|a, b| {
            (b.like_count + b.retweet_count * 2).cmp(&(a.like_count + a.retweet_count * 2))
        });
        top_posts.truncate(10);

        let recent_posts: Vec<XPostItem> = post_items.into_iter().take(20).collect();
        let fetched_count = tweets.len();

        let post_summary = format!(
            "X 账号 @{} 共有约 {} 条帖子，抓取 {} 条时间线，累计获赞 {}，转推 {}，评论 {}",
            username, tweet_count_metric, fetched_count, total_likes, total_retweets, total_replies
        );

        // 关注列表：按粉丝数排序取样本，简介截断以控制 token
        const MAX_FOLLOWING_SAMPLE: usize = 50;
        const MAX_FOLLOWING_DESC_CHARS: usize = 80;

        let following = data
            .get("following")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let following_fetched = following.len();

        let mut following_sample: Vec<XFollowingItem> = following
            .iter()
            .map(|account| XFollowingItem {
                username: account
                    .get("username")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                name: account
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                description: account
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .chars()
                    .take(MAX_FOLLOWING_DESC_CHARS)
                    .collect(),
                follower_count: account
                    .pointer("/public_metrics/followers_count")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0),
                verified: account
                    .get("verified")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                profile_image_url: account
                    .get("profile_image_url")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(str::to_string),
            })
            .collect();
        following_sample.sort_by_key(|b| Reverse(b.follower_count));
        following_sample.truncate(MAX_FOLLOWING_SAMPLE);

        let following_summary = if following_fetched > 0 {
            format!(
                "共关注 {} 个账号（已抓取 {} 个），样本按粉丝数取前 {} 个；关注对象反映用户的兴趣圈层",
                following_count.unwrap_or(following_fetched as i64),
                following_fetched,
                following_sample.len()
            )
        } else {
            String::new()
        };

        let user_name = user
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(&username)
            .to_string();
        let user_avatar = user
            .get("profile_image_url")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string);

        Ok(SmartFilteredData {
            platform: "x".to_string(),
            user_summary: UserSummary {
                username,
                user_id,
                level: None,
                stats: UserStats {
                    follower_count,
                    following_count,
                    total_content: fetched_count,
                },
            },
            content_analysis: ContentAnalysis::X(XAnalysis {
                post_summary,
                user_name,
                user_avatar,
                following_summary,
                following_sample,
                engagement_stats: XEngagementStats {
                    total_posts: fetched_count,
                    total_likes_received: total_likes,
                    total_retweets_received: total_retweets,
                    total_replies_received: total_replies,
                    total_impressions,
                    // 已放弃用户 OAuth，不再抓 likes；字段保留兼容旧报告结构
                    liked_posts_count: 0,
                },
                recent_posts,
                top_posts,
                language_distribution,
            }),
            raw_unknown_content: vec![],
        })
    }

    fn filter_discord(data: &Value) -> Result<SmartFilteredData, String> {
        let user = data.get("user").unwrap_or(&Value::Null);

        let user_id = user
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let username = user
            .get("username")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let global_name = user
            .get("global_name")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
        let display_name = global_name.clone().unwrap_or_else(|| username.clone());

        let premium_type = user
            .get("premium_type")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let level = match premium_type {
            1 => Some("Nitro Classic".to_string()),
            2 => Some("Nitro".to_string()),
            3 => Some("Nitro Basic".to_string()),
            _ => None,
        };

        let guilds = data
            .get("guilds")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let connections_raw = data
            .get("connections")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let mut owned_guild_count = 0usize;
        let mut admin_guild_count = 0usize;
        let mut manage_guild_count = 0usize;
        let mut guilds_preview = Vec::new();

        for guild in &guilds {
            let id = guild
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let name = guild
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let owner = guild
                .get("owner")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if owner {
                owned_guild_count += 1;
            }

            let perms =
                Self::discord_permissions_highlight(guild.get("permissions").and_then(|v| {
                    v.as_str()
                        .and_then(|s| s.parse::<u64>().ok())
                        .or_else(|| v.as_u64())
                }));
            if perms.iter().any(|p| p == "ADMINISTRATOR") {
                admin_guild_count += 1;
            }
            if perms
                .iter()
                .any(|p| p == "MANAGE_GUILD" || p == "ADMINISTRATOR")
            {
                manage_guild_count += 1;
            }

            let icon_url = match (
                guild
                    .get("icon")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty()),
                id.is_empty(),
            ) {
                (Some(icon), false) => {
                    let ext = if icon.starts_with("a_") { "gif" } else { "png" };
                    Some(format!(
                        "https://cdn.discordapp.com/icons/{}/{}.{}",
                        id, icon, ext
                    ))
                }
                _ => None,
            };

            guilds_preview.push(DiscordGuildItem {
                id,
                name,
                icon_url,
                owner,
                permissions_highlight: perms,
            });
        }

        // 所有者 / 管理员优先展示
        guilds_preview.sort_by(|a, b| {
            b.owner
                .cmp(&a.owner)
                .then_with(|| {
                    let a_admin = a.permissions_highlight.iter().any(|p| p == "ADMINISTRATOR");
                    let b_admin = b.permissions_highlight.iter().any(|p| p == "ADMINISTRATOR");
                    b_admin.cmp(&a_admin)
                })
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        guilds_preview.truncate(30);

        let mut connections = Vec::new();
        let mut linked_platforms = Vec::new();
        for conn in &connections_raw {
            let conn_type = conn
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let name = conn
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let id = conn
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let verified = conn
                .get("verified")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let visibility = conn.get("visibility").and_then(|v| v.as_i64()).unwrap_or(0);

            if !conn_type.is_empty()
                && conn_type != "unknown"
                && !linked_platforms.iter().any(|p| p == &conn_type)
            {
                linked_platforms.push(conn_type.clone());
            }

            connections.push(DiscordConnectionItem {
                r#type: conn_type,
                name,
                id,
                verified,
                visibility,
            });
        }
        linked_platforms.sort();

        let verified_connection_count = connections.iter().filter(|c| c.verified).count();

        // 交叉校验：raw 中可注入 myriad_cross_refs（由 profile 拉取时写入）
        let cross_refs = data
            .get("myriad_cross_refs")
            .cloned()
            .unwrap_or(Value::Null);
        let steam_id_cfg = cross_refs
            .get("steam_id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty());
        let github_username_cfg = cross_refs
            .get("github_username")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty());

        let mut cross_check = std::collections::HashMap::new();

        let steam_conn = connections.iter().find(|c| c.r#type == "steam");
        cross_check.insert(
            "steam".to_string(),
            DiscordCrossCheck {
                discord_linked: steam_conn.is_some(),
                myriad_configured: steam_id_cfg.is_some(),
                id_match: match (steam_conn, steam_id_cfg) {
                    (Some(c), Some(sid)) => {
                        let sid_l = sid.to_lowercase();
                        Some(
                            (!c.id.is_empty() && c.id.eq_ignore_ascii_case(sid))
                                || (!c.name.is_empty() && c.name.eq_ignore_ascii_case(sid))
                                || c.id.to_lowercase().contains(&sid_l)
                                || sid_l.contains(&c.id.to_lowercase()),
                        )
                    }
                    _ => None,
                },
                name_match: None,
            },
        );

        let github_conn = connections.iter().find(|c| c.r#type == "github");
        cross_check.insert(
            "github".to_string(),
            DiscordCrossCheck {
                discord_linked: github_conn.is_some(),
                myriad_configured: github_username_cfg.is_some(),
                id_match: None,
                name_match: match (github_conn, github_username_cfg) {
                    (Some(c), Some(gh)) => {
                        let gh_l = gh.trim_start_matches('@').to_lowercase();
                        Some(
                            (!c.name.is_empty() && c.name.eq_ignore_ascii_case(&gh_l))
                                || (!c.id.is_empty() && c.id.eq_ignore_ascii_case(&gh_l)),
                        )
                    }
                    _ => None,
                },
            },
        );

        let community_summary = format!(
            "Discord 用户 {} 加入 {} 个服务器（自建 {}，管理权限 {}），绑定 {} 个第三方账号（已验证 {}）",
            display_name,
            guilds.len(),
            owned_guild_count,
            manage_guild_count,
            connections.len(),
            verified_connection_count
        );

        Ok(SmartFilteredData {
            platform: "discord".to_string(),
            user_summary: UserSummary {
                username: display_name,
                user_id,
                level,
                stats: UserStats {
                    follower_count: None,
                    following_count: None,
                    total_content: guilds.len(),
                },
            },
            content_analysis: ContentAnalysis::Discord(DiscordAnalysis {
                community_summary,
                guild_stats: DiscordGuildStats {
                    guild_count: guilds.len(),
                    owned_guild_count,
                    admin_guild_count,
                    manage_guild_count,
                },
                guilds_preview,
                connections,
                identity_graph: DiscordIdentityGraph {
                    linked_platforms,
                    cross_check,
                },
            }),
            raw_unknown_content: vec![],
        })
    }

    /// 从 Discord permissions 位掩码提取关注权限标签
    fn discord_permissions_highlight(permissions: Option<u64>) -> Vec<String> {
        let Some(bits) = permissions else {
            return Vec::new();
        };
        // https://discord.com/developers/docs/topics/permissions
        const ADMINISTRATOR: u64 = 1 << 3;
        const MANAGE_CHANNELS: u64 = 1 << 4;
        const MANAGE_GUILD: u64 = 1 << 5;
        const MANAGE_ROLES: u64 = 1 << 28;
        const MANAGE_MESSAGES: u64 = 1 << 13;
        const KICK_MEMBERS: u64 = 1 << 1;
        const BAN_MEMBERS: u64 = 1 << 2;

        let mut out = Vec::new();
        if bits & ADMINISTRATOR != 0 {
            out.push("ADMINISTRATOR".to_string());
            return out;
        }
        if bits & MANAGE_GUILD != 0 {
            out.push("MANAGE_GUILD".to_string());
        }
        if bits & MANAGE_CHANNELS != 0 {
            out.push("MANAGE_CHANNELS".to_string());
        }
        if bits & MANAGE_ROLES != 0 {
            out.push("MANAGE_ROLES".to_string());
        }
        if bits & MANAGE_MESSAGES != 0 {
            out.push("MANAGE_MESSAGES".to_string());
        }
        if bits & KICK_MEMBERS != 0 {
            out.push("KICK_MEMBERS".to_string());
        }
        if bits & BAN_MEMBERS != 0 {
            out.push("BAN_MEMBERS".to_string());
        }
        out
    }

    fn bangumi_subject_type_label(subject_type: i64) -> &'static str {
        match subject_type {
            1 => "book",
            2 => "anime",
            3 => "music",
            4 => "game",
            6 => "real",
            _ => "unknown",
        }
    }

    fn bangumi_collection_type_label(collection_type: i64) -> &'static str {
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
    fn mal_status_label(status: &str) -> &'static str {
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
    fn preprocess_platform_data(
        platform: &str,
        data: &Value,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let mut processed = data.clone();

        match platform {
            "bilibili" => {
                // 适配: user -> user_info
                if let Some(user) = data.get("user") {
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
    fn save_platform_cache(
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    #[test]
    fn test_smart_filter_integration() {
        // Read from the new split raw data files
        let raw_dir = Path::new("cache/raw");
        if !raw_dir.exists() {
            println!(
                "Skipping test: cache/raw directory not found at {:?}",
                raw_dir
            );
            return;
        }

        let mut all_data = serde_json::Map::new();

        // Load all platform files
        if let Ok(entries) = fs::read_dir(raw_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("json") {
                    if let Some(platform_name) = path.file_stem().and_then(|s| s.to_str()) {
                        if let Ok(content) = fs::read_to_string(&path) {
                            if let Ok(json) = serde_json::from_str(&content) {
                                all_data.insert(platform_name.to_string(), json);
                            }
                        }
                    }
                }
            }
        }

        if all_data.is_empty() {
            println!("Skipping test: No platform data files found in cache/raw");
            return;
        }

        let data = Value::Object(all_data);

        // 使用 process_and_save_all，它会分平台保存
        match SmartFilter::process_and_save_all(&data) {
            Ok(_) => println!("Successfully processed and saved all platform data"),
            Err(e) => println!("Failed to process platform data: {}", e),
        }

        // 验证分平台文件是否已创建
        let platforms_dir = Path::new("cache/platforms");
        if platforms_dir.exists() {
            println!("Platform filtered files:");
            if let Ok(entries) = fs::read_dir(platforms_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .map(|s| s.ends_with("_filtered.json"))
                        .unwrap_or(false)
                    {
                        println!("  - {:?}", path.file_name().unwrap());
                    }
                }
            }
        }
    }

    #[test]
    fn filter_x_builds_engagement_summary() {
        let raw = serde_json::json!({
            "user": {
                "id": "42",
                "username": "demo",
                "name": "Demo User",
                "public_metrics": {
                    "followers_count": 100,
                    "following_count": 10,
                    "tweet_count": 50
                }
            },
            "tweets": [
                {
                    "id": "1",
                    "text": "hello",
                    "lang": "en",
                    "created_at": "2026-01-01T00:00:00Z",
                    "public_metrics": {
                        "like_count": 5,
                        "retweet_count": 1,
                        "reply_count": 0,
                        "impression_count": 20
                    }
                },
                {
                    "id": "2",
                    "text": "world",
                    "lang": "en",
                    "created_at": "2026-01-02T00:00:00Z",
                    "public_metrics": {
                        "like_count": 50,
                        "retweet_count": 10,
                        "reply_count": 2,
                        "impression_count": 200
                    }
                }
            ]
        });

        let filtered = SmartFilter::filter("x", &raw).expect("filter x");
        assert_eq!(filtered.platform, "x");
        assert_eq!(filtered.user_summary.username, "demo");
        assert_eq!(filtered.user_summary.stats.follower_count, Some(100));

        match filtered.content_analysis {
            ContentAnalysis::X(analysis) => {
                assert_eq!(analysis.engagement_stats.total_posts, 2);
                assert_eq!(analysis.engagement_stats.total_likes_received, 55);
                assert_eq!(analysis.top_posts[0].id, "2");
                assert!(analysis.post_summary.contains("@demo"));
            }
            other => panic!("expected X analysis, got {:?}", other),
        }
    }

    #[test]
    fn test_filter_discord() {
        // ADMINISTRATOR = 1<<3 = 8
        let raw = serde_json::json!({
            "user": {
                "id": "123456789",
                "username": "haru",
                "global_name": "Haru",
                "premium_type": 2
            },
            "guilds": [
                {
                    "id": "g1",
                    "name": "Owned Server",
                    "owner": true,
                    "permissions": "8",
                    "icon": "abc"
                },
                {
                    "id": "g2",
                    "name": "Member Server",
                    "owner": false,
                    "permissions": "0"
                }
            ],
            "connections": [
                {
                    "type": "steam",
                    "name": "haru_steam",
                    "id": "76561198000000000",
                    "verified": true,
                    "visibility": 1
                },
                {
                    "type": "github",
                    "name": "octocat",
                    "id": "1",
                    "verified": true,
                    "visibility": 0
                }
            ],
            "myriad_cross_refs": {
                "steam_id": "76561198000000000",
                "github_username": "octocat"
            }
        });

        let filtered = SmartFilter::filter("discord", &raw).expect("filter discord");
        assert_eq!(filtered.platform, "discord");
        assert_eq!(filtered.user_summary.username, "Haru");
        assert_eq!(filtered.user_summary.user_id, "123456789");
        assert_eq!(filtered.user_summary.level.as_deref(), Some("Nitro"));
        assert_eq!(filtered.user_summary.stats.total_content, 2);

        match filtered.content_analysis {
            ContentAnalysis::Discord(analysis) => {
                assert_eq!(analysis.guild_stats.guild_count, 2);
                assert_eq!(analysis.guild_stats.owned_guild_count, 1);
                assert_eq!(analysis.guild_stats.admin_guild_count, 1);
                assert_eq!(analysis.guilds_preview[0].name, "Owned Server");
                assert!(analysis.guilds_preview[0]
                    .permissions_highlight
                    .contains(&"ADMINISTRATOR".to_string()));
                assert_eq!(analysis.connections.len(), 2);
                assert!(analysis
                    .identity_graph
                    .linked_platforms
                    .contains(&"steam".to_string()));
                let steam = analysis.identity_graph.cross_check.get("steam").unwrap();
                assert!(steam.discord_linked);
                assert!(steam.myriad_configured);
                assert_eq!(steam.id_match, Some(true));
                let github = analysis.identity_graph.cross_check.get("github").unwrap();
                assert_eq!(github.name_match, Some(true));
                assert!(analysis.community_summary.contains("Haru"));
            }
            other => panic!("expected Discord analysis, got {:?}", other),
        }
    }

    #[test]
    fn test_discord_permissions_highlight() {
        let admin = SmartFilter::discord_permissions_highlight(Some(8));
        assert_eq!(admin, vec!["ADMINISTRATOR".to_string()]);

        // MANAGE_GUILD | MANAGE_CHANNELS = 32 | 16 = 48
        let manage = SmartFilter::discord_permissions_highlight(Some(48));
        assert!(manage.contains(&"MANAGE_GUILD".to_string()));
        assert!(manage.contains(&"MANAGE_CHANNELS".to_string()));
        assert!(!manage.contains(&"ADMINISTRATOR".to_string()));
    }
}
