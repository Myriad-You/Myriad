// SmartFilter process/save pipeline for platform raw payloads.

use serde_json::Value;
use std::cmp::Reverse;
use std::fs;
use std::path::Path;

use crate::services::content_databases::{AnimeDatabase, ArtistDatabase, GameDatabase};

use super::helpers::*;

impl SmartFilter {
    /// 处理所有平台数据并分别保存到各平台缓存文件
    /// 优化：分平台保存，减少不必要的克隆，添加数据量限制
    pub fn process_and_save_all(all_data: &Value) -> Result<(), Box<dyn std::error::Error>> {
        // 数据量限制常量
        const MAX_VIDEOS_FOR_FILTER: usize = 200;
        const MAX_SONGS_FOR_FILTER: usize = 3000;

        let cache_dir = Path::new("cache/platforms");
        fs::create_dir_all(cache_dir)?;

        let mut processed_count = 0;

        // 1. Process Bilibili
        if let Some(bilibili_data) = all_data.get("bilibili") {
            let mut process_data = serde_json::Map::new();

            // 适配: user / user_info（旧缓存）-> user_info
            if let Some(user) = bilibili_data
                .get("user")
                .or_else(|| bilibili_data.get("user_info"))
            {
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
                // 限制歌曲数量避免内存问题
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

        if let Some(youtube_data) = all_data.get("youtube") {
            match SmartFilter::filter("youtube", youtube_data) {
                Ok(result) => {
                    Self::save_platform_cache_atomic("youtube", &result)?;
                    processed_count += 1;
                }
                Err(e) => tracing::warn!("YouTube filter failed: {}", e),
            }
        }

        tracing::info!(
            "✓ Smart filtered data saved to {} platform files in cache/platforms/",
            processed_count
        );
        Ok(())
    }

    /// 原子性保存平台缓存（使用临时文件+重命名）
    pub(crate) fn save_platform_cache_atomic(
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
            "youtube" => Self::filter_youtube(raw_data),
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

    /// YouTube public channel + uploads sample (API key raw shape from fetcher)
    pub(crate) fn filter_youtube(data: &Value) -> Result<SmartFilteredData, String> {
        let channel = data
            .get("channel")
            .or_else(|| data.get("user"))
            .ok_or_else(|| "YouTube raw data missing channel".to_string())?;

        let channel_id = channel
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let title = channel
            .pointer("/snippet/title")
            .and_then(|v| v.as_str())
            .unwrap_or("YouTube channel")
            .to_string();
        let custom_url = channel
            .pointer("/snippet/customUrl")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let description = channel
            .pointer("/snippet/description")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let avatar = channel
            .pointer("/snippet/thumbnails/high/url")
            .or_else(|| channel.pointer("/snippet/thumbnails/medium/url"))
            .or_else(|| channel.pointer("/snippet/thumbnails/default/url"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let subscriber_count = channel
            .pointer("/statistics/subscriberCount")
            .and_then(json_nonneg_i64)
            .unwrap_or(0);
        let view_count = channel
            .pointer("/statistics/viewCount")
            .and_then(json_nonneg_i64)
            .unwrap_or(0);
        let video_count = channel
            .pointer("/statistics/videoCount")
            .and_then(json_nonneg_i64)
            .unwrap_or(0);

        let channel_url = if let Some(ref cu) = custom_url {
            let handle = cu.trim_start_matches('@');
            Some(format!("https://www.youtube.com/@{handle}"))
        } else if !channel_id.is_empty() {
            Some(format!("https://www.youtube.com/channel/{channel_id}"))
        } else {
            None
        };

        // Prefer videos[] with statistics; fall back to playlist_items snippet only
        let mut recent_videos: Vec<YouTubeVideoItem> = Vec::new();
        if let Some(videos) = data.get("videos").and_then(|v| v.as_array()) {
            for v in videos {
                let video_id = v.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string();
                if video_id.is_empty() {
                    continue;
                }
                let vtitle = v
                    .pointer("/snippet/title")
                    .and_then(|x| x.as_str())
                    .unwrap_or("Untitled")
                    .to_string();
                let cover = v
                    .pointer("/snippet/thumbnails/medium/url")
                    .or_else(|| v.pointer("/snippet/thumbnails/high/url"))
                    .or_else(|| v.pointer("/snippet/thumbnails/default/url"))
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                recent_videos.push(YouTubeVideoItem {
                    title: vtitle,
                    video_id: video_id.clone(),
                    cover,
                    view_count: v
                        .pointer("/statistics/viewCount")
                        .and_then(json_nonneg_i64),
                    like_count: v
                        .pointer("/statistics/likeCount")
                        .and_then(json_nonneg_i64),
                    comment_count: v
                        .pointer("/statistics/commentCount")
                        .and_then(json_nonneg_i64),
                    published_at: v
                        .pointer("/snippet/publishedAt")
                        .and_then(|x| x.as_str())
                        .map(|s| s.to_string()),
                    duration: v
                        .pointer("/contentDetails/duration")
                        .and_then(|x| x.as_str())
                        .map(|s| s.to_string()),
                    url: Some(format!("https://www.youtube.com/watch?v={video_id}")),
                });
            }
        } else if let Some(items) = data.get("playlist_items").and_then(|v| v.as_array()) {
            for item in items {
                let video_id = item
                    .pointer("/contentDetails/videoId")
                    .or_else(|| item.pointer("/snippet/resourceId/videoId"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                if video_id.is_empty() {
                    continue;
                }
                let vtitle = item
                    .pointer("/snippet/title")
                    .and_then(|x| x.as_str())
                    .unwrap_or("Untitled")
                    .to_string();
                let cover = item
                    .pointer("/snippet/thumbnails/medium/url")
                    .or_else(|| item.pointer("/snippet/thumbnails/high/url"))
                    .or_else(|| item.pointer("/snippet/thumbnails/default/url"))
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                recent_videos.push(YouTubeVideoItem {
                    title: vtitle,
                    video_id: video_id.clone(),
                    cover,
                    view_count: None,
                    like_count: None,
                    comment_count: None,
                    published_at: item
                        .pointer("/snippet/publishedAt")
                        .or_else(|| item.pointer("/contentDetails/videoPublishedAt"))
                        .and_then(|x| x.as_str())
                        .map(|s| s.to_string()),
                    duration: None,
                    url: Some(format!("https://www.youtube.com/watch?v={video_id}")),
                });
            }
        }

        // Cap sample size for filtered cache
        if recent_videos.len() > 24 {
            recent_videos.truncate(24);
        }

        let sample_n = recent_videos.len();
        // Empty public channel is a valid success (not a fetch failure).
        let video_summary = if video_count == 0 && sample_n == 0 {
            format!(
                "公开频道已解析，但暂无上传视频（订阅 {}，观看 {}）。空频道仍可生成报告。",
                subscriber_count, view_count
            )
        } else {
            format!(
                "{} 个公开视频，{} 位订阅者，累计 {} 次观看；已采样最近 {} 条上传",
                video_count, subscriber_count, view_count, sample_n
            )
        };

        let user_summary = UserSummary {
            username: title,
            user_id: channel_id,
            level: custom_url.clone(),
            stats: UserStats {
                follower_count: Some(subscriber_count),
                following_count: None,
                total_content: video_count.max(0) as usize,
            },
        };

        let content_analysis = ContentAnalysis::YouTube(YouTubeAnalysis {
            video_summary,
            subscriber_count,
            view_count,
            video_count,
            recent_videos,
            custom_url,
            avatar,
            channel_url,
            description,
        });

        Ok(SmartFilteredData {
            platform: "youtube".to_string(),
            user_summary,
            content_analysis,
            raw_unknown_content: vec![],
        })
    }

    /// Bilibili 智能过滤
    pub(crate) fn filter_bilibili(data: &Value) -> Result<SmartFilteredData, String> {
        // 如果缺少用户信息，使用默认值而不是报错
        let default_user_info = serde_json::json!({
            "name": "未知用户",
            "mid": 0,
            "level": 0,
            "follower": 0,
            "following": 0
        });
        // 兼容 preprocess 后的 user_info，以及未预处理的 user / user_info
        let user_info = data
            .get("user_info")
            .or_else(|| data.get("user"))
            .unwrap_or(&default_user_info);

        let video_count = data
            .get("videos")
            .and_then(|v| v.as_array())
            .map(|v| v.len())
            .unwrap_or(0);
        let bangumi_count = data
            .get("bangumi")
            .and_then(|v| v.as_array())
            .map(|v| v.len())
            .unwrap_or(0);

        // mid 可能是 number 或 string（card API）
        let user_id = user_info
            .get("mid")
            .and_then(|v| match v {
                Value::Number(n) => n.as_i64().map(|i| i.to_string()),
                Value::String(s) => {
                    let t = s.trim();
                    if t.is_empty() {
                        None
                    } else {
                        Some(t.to_string())
                    }
                }
                _ => None,
            })
            .unwrap_or_default();

        // 1. 用户摘要
        let user_summary = UserSummary {
            username: user_info
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("未知用户")
                .to_string(),
            user_id,
            level: user_info
                .get("level")
                .and_then(|v| v.as_i64())
                .or_else(|| {
                    user_info
                        .pointer("/level_info/current_level")
                        .and_then(|v| v.as_i64())
                })
                .map(|l| format!("Lv{}", l)),
            stats: UserStats {
                follower_count: user_info
                    .get("follower")
                    .and_then(|v| v.as_i64())
                    .or_else(|| user_info.get("fans").and_then(|v| v.as_i64())),
                following_count: user_info
                    .get("following")
                    .and_then(|v| v.as_i64())
                    .or_else(|| user_info.get("attention").and_then(|v| v.as_i64())),
                total_content: video_count + bangumi_count,
            },
        };

        // 2. 收集所有视频信息（保留 cover / bvid，供资料库分享卡片使用）
        let videos = data.get("videos").and_then(|v| v.as_array());
        let mut recent_videos = Vec::new();

        if let Some(vids) = videos {
            for video in vids {
                if let Some(title) = video.get("title").and_then(|v| v.as_str()) {
                    let cover = video
                        .get("cover")
                        .or_else(|| video.get("pic"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty());
                    let bvid = video
                        .get("bvid")
                        .and_then(|v| v.as_str())
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty());
                    let id = video
                        .get("id")
                        .map(|v| match v {
                            Value::String(s) => s.trim().to_string(),
                            Value::Number(n) => n.to_string(),
                            _ => String::new(),
                        })
                        .filter(|s| !s.is_empty());
                    recent_videos.push(VideoItem {
                        title: title.to_string(),
                        cover,
                        bvid,
                        id,
                    });
                }
            }
        }

        // 3. 使用动画数据库分析番剧/电视剧/电影（顺带保留封面/进度）
        let bangumi = data.get("bangumi").and_then(|v| v.as_array());
        let mut watch_list = Vec::new();
        // title → (cover, season_id, progress, season_type)
        type BangumiMetaEntry = (
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        );
        let mut bangumi_meta: std::collections::HashMap<String, BangumiMetaEntry> =
            std::collections::HashMap::new();

        if let Some(items) = bangumi {
            for item in items {
                if let Some(title) = item.get("title").and_then(|v| v.as_str()) {
                    let author = item
                        .get("author")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let cover = item
                        .get("cover")
                        .and_then(|v| v.as_str())
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty());
                    let season_id = item
                        .get("season_id")
                        .map(|v| match v {
                            Value::String(s) => s.trim().to_string(),
                            Value::Number(n) => n.to_string(),
                            _ => String::new(),
                        })
                        .filter(|s| !s.is_empty());
                    let progress = item
                        .get("progress")
                        .and_then(|v| v.as_str())
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty());
                    let season_type = item
                        .get("season_type")
                        .map(|v| match v {
                            Value::String(s) => s.trim().to_string(),
                            Value::Number(n) => n.to_string(),
                            _ => String::new(),
                        })
                        .filter(|s| !s.is_empty());
                    bangumi_meta
                        .insert(title.to_string(), (cover, season_id, progress, season_type));
                    watch_list.push((title.to_string(), author));
                }
            }
        }

        let anime_db = AnimeDatabase::new();
        let anime_analysis = anime_db.analyze(watch_list.clone());

        // 找出未知的番剧内容（写入 cover / season_id / progress 到 metadata）
        let mut raw_unknown_content = Vec::new();
        for (title, _author) in watch_list.iter() {
            if anime_db.find(title).is_none() {
                let mut metadata = std::collections::HashMap::new();
                if let Some((cover, season_id, progress, season_type)) = bangumi_meta.get(title) {
                    if let Some(c) = cover {
                        metadata.insert("cover".to_string(), c.clone());
                        metadata.insert("image".to_string(), c.clone());
                    }
                    if let Some(sid) = season_id {
                        metadata.insert("season_id".to_string(), sid.clone());
                        metadata.insert("id".to_string(), sid.clone());
                    }
                    if let Some(p) = progress {
                        metadata.insert("progress".to_string(), p.clone());
                    }
                    if let Some(st) = season_type {
                        metadata.insert("season_type".to_string(), st.clone());
                    }
                }
                raw_unknown_content.push(UnknownContent {
                    content_type: "anime".to_string(),
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
    pub(crate) fn filter_steam(data: &Value) -> Result<SmartFilteredData, String> {
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

        // name → (playtime, appid)
        let mut game_list = Vec::new();
        let mut name_to_appid: std::collections::HashMap<String, i64> =
            std::collections::HashMap::new();
        let mut recent_games = Vec::new();

        let push_game =
            |game: &Value,
             game_list: &mut Vec<(String, i64)>,
             name_to_appid: &mut std::collections::HashMap<String, i64>| {
                if let Some(name) = game.get("name").and_then(|v| v.as_str()) {
                    // Steam Web API: playtime_forever is **minutes**.
                    let playtime = game
                        .get("playtime_forever")
                        .and_then(json_nonneg_i64)
                        .unwrap_or(0)
                        .min(STEAM_PLAYTIME_MINUTES_CAP);
                    let appid = game.get("appid").and_then(json_nonneg_i64);
                    if let Some(id) = appid {
                        name_to_appid.insert(name.to_string(), id);
                    }
                    game_list.push((name.to_string(), playtime));
                }
            };

        // 收集所有拥有的游戏
        if let Some(games) = owned_games {
            for game in games {
                push_game(game, &mut game_list, &mut name_to_appid);
            }
        }

        // 收集最近玩的游戏（保留 appid + Steam CDN 封面）
        if let Some(games) = recently_played {
            for game in games {
                if let Some(name) = game.get("name").and_then(|v| v.as_str()) {
                    let playtime = game
                        .get("playtime_forever")
                        .and_then(json_nonneg_i64)
                        .unwrap_or(0)
                        .min(STEAM_PLAYTIME_MINUTES_CAP);
                    let appid = game
                        .get("appid")
                        .and_then(json_nonneg_i64)
                        .or_else(|| name_to_appid.get(name).copied());
                    if let Some(id) = appid {
                        name_to_appid.insert(name.to_string(), id);
                    }
                    let image = appid.map(|id| {
                        format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{id}/header.jpg")
                    });
                    recent_games.push(GameItem {
                        name: name.to_string(),
                        playtime,
                        appid,
                        image,
                    });
                }
            }
        }

        let games_count = game_list.len();
        let total_playtime_minutes: i64 = game_list
            .iter()
            .map(|(_, p)| (*p).max(0))
            .sum::<i64>()
            .min(STEAM_PLAYTIME_MINUTES_CAP);

        // 3. 使用游戏数据库分析
        let game_db = GameDatabase::new();
        let game_analysis = game_db.analyze(game_list);

        // 收集未知的游戏内容（写入 appid / playtime / image）
        let mut raw_unknown_content = Vec::new();
        for (name, playtime) in game_analysis.unknown_games {
            let mut metadata = std::collections::HashMap::new();
            metadata.insert("playtime".to_string(), playtime.to_string());
            if let Some(appid) = name_to_appid.get(&name).copied() {
                metadata.insert("appid".to_string(), appid.to_string());
                metadata.insert(
                    "image".to_string(),
                    format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{appid}/header.jpg"),
                );
                metadata.insert("id".to_string(), appid.to_string());
            }
            raw_unknown_content.push(UnknownContent {
                content_type: "game".to_string(),
                title: name,
                metadata,
            });
        }

        let content_analysis = ContentAnalysis::Steam(SteamAnalysis {
            game_summary: game_analysis.summary.clone(),
            genre_analysis: game_analysis.genre_analysis,
            recent_games,
            games_count,
            total_playtime_minutes,
        });

        Ok(SmartFilteredData {
            platform: "steam".to_string(),
            user_summary,
            content_analysis,
            raw_unknown_content,
        })
    }

    /// GitHub 智能过滤
    pub(crate) fn filter_github(data: &Value) -> Result<SmartFilteredData, String> {
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
                    .and_then(json_nonneg_i64),
                following_count: user
                    .and_then(|u| u.get("following"))
                    .and_then(json_nonneg_i64),
                total_content: 0,
            },
        };

        let public_repos = user
            .and_then(|u| u.get("public_repos"))
            .and_then(json_nonneg_i64);

        // 2. 收集所有仓库信息
        let repos = data.get("repos").and_then(|v| v.as_array());
        let mut recent_repos = Vec::new();
        let mut language_distribution = std::collections::HashMap::new();

        let owner = user
            .and_then(|u| u.get("login"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if let Some(repo_list) = repos {
            for repo in repo_list {
                if let Some(name) = repo.get("name").and_then(|v| v.as_str()) {
                    let language = repo
                        .get("language")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let stars = repo
                        .get("stargazers_count")
                        .or_else(|| repo.get("stars"))
                        .and_then(json_nonneg_i64);
                    let forks = repo
                        .get("forks_count")
                        .or_else(|| repo.get("forks"))
                        .and_then(json_nonneg_i64);
                    let description = repo
                        .get("description")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let url = repo
                        .get("html_url")
                        .or_else(|| repo.get("url"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                        .or_else(|| {
                            if owner.is_empty() {
                                None
                            } else {
                                Some(format!("https://github.com/{owner}/{name}"))
                            }
                        });
                    // Open Graph card art (no local image in GitHub API).
                    let image = if !owner.is_empty() {
                        Some(format!(
                            "https://opengraph.githubassets.com/1/{owner}/{name}"
                        ))
                    } else {
                        url.as_ref().and_then(|u| {
                            u.trim_start_matches("https://github.com/")
                                .split_once('/')
                                .map(|(o, r)| {
                                    format!("https://opengraph.githubassets.com/1/{o}/{r}")
                                })
                        })
                    };

                    recent_repos.push(RepoItem {
                        name: name.to_string(),
                        language: language.clone(),
                        stars,
                        forks,
                        description,
                        url,
                        image,
                    });

                    if let Some(lang) = language {
                        *language_distribution.entry(lang).or_insert(0) += 1;
                    }
                }
            }
        }

        let repo_count_display = public_repos
            .map(|n| n as usize)
            .unwrap_or(0)
            .max(recent_repos.len());
        let repo_summary = format!(
            "拥有 {} 个仓库，主要使用 {}",
            repo_count_display,
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
                        let count = day
                            .get("count")
                            .and_then(json_nonneg_i64)?
                            .min(GITHUB_CONTRIB_DAY_CAP);
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
            public_repos,
        });

        Ok(SmartFilteredData {
            platform: "github".to_string(),
            user_summary,
            content_analysis,
            raw_unknown_content: vec![],
        })
    }

    /// 网易云音乐智能过滤
    pub(crate) fn filter_netease(data: &Value) -> Result<SmartFilteredData, String> {
        tracing::debug!("🎵 Processing Netease data...");

        let profile = data.get("profile");

        // 1. 用户摘要（使用更宽松的默认值）
        let mut user_summary = UserSummary {
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
                // playlistCount on profile only — never use synthetic playlists.len()
                // (liked_songs are wrapped as a single fake playlist for track extraction).
                total_content: profile
                    .and_then(|p| p.get("playlistCount"))
                    .and_then(|v| v.as_i64())
                    .map(|n| n.max(0) as usize)
                    .unwrap_or(0),
            },
        };

        // 2. 收集所有歌曲信息（支持多种数据结构）
        let playlists = data.get("playlists").and_then(|v| v.as_array());
        // Only count real playlist arrays when profile.playlistCount missing AND
        // playlists look like a multi-list catalog (not a single synthetic shell).
        if user_summary.stats.total_content == 0 {
            if let Some(lists) = playlists {
                if lists.len() > 1 {
                    user_summary.stats.total_content = lists.len();
                }
            }
        }
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

                            let song_id = track
                                .get("id")
                                .map(|v| match v {
                                    Value::String(s) => s.trim().to_string(),
                                    Value::Number(n) => n.to_string(),
                                    _ => String::new(),
                                })
                                .filter(|s| !s.is_empty());
                            let album = track
                                .get("al")
                                .and_then(|al| al.get("name"))
                                .and_then(|v| v.as_str())
                                .or_else(|| track.get("album").and_then(|v| v.as_str()))
                                .map(|s| s.trim().to_string())
                                .filter(|s| !s.is_empty());
                            let cover = track
                                .get("al")
                                .and_then(|al| al.get("picUrl"))
                                .and_then(|v| v.as_str())
                                .or_else(|| {
                                    track
                                        .get("album")
                                        .and_then(|a| a.get("picUrl"))
                                        .and_then(|v| v.as_str())
                                })
                                .or_else(|| track.get("picUrl").and_then(|v| v.as_str()))
                                .map(|s| s.trim().to_string())
                                .filter(|s| !s.is_empty());
                            let fee = track
                                .get("fee")
                                .and_then(|v| v.as_i64())
                                .or_else(|| {
                                    track
                                        .get("privilege")
                                        .and_then(|p| p.get("fee"))
                                        .and_then(|v| v.as_i64())
                                });
                            let is_vip = track
                                .get("isVip")
                                .and_then(|v| v.as_bool())
                                .or_else(|| track.get("is_vip").and_then(|v| v.as_bool()))
                                .or_else(|| fee.map(|f| f == 1 || f == 4));

                            recent_songs.push(SongItem {
                                title: name.to_string(),
                                artist: artist.to_string(),
                                id: song_id,
                                cover,
                                album,
                                is_vip,
                                fee,
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

    pub(crate) fn filter_bangumi(data: &Value) -> Result<SmartFilteredData, String> {
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
}
