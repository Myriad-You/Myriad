use crate::services::agent::executor::utils::validate_platform_name;
use crate::services::data_paths::platform_filtered_file;
use serde_json::{json, Value};
use std::cmp::Reverse;
use std::collections::HashMap;
use std::io::ErrorKind;

fn platform_cache_read_failed(platform: &str, error: std::io::Error) -> String {
    tracing::error!(%error, platform, "failed to read platform cache");
    match error.kind() {
        ErrorKind::NotFound => format!("No cached {platform} data"),
        ErrorKind::PermissionDenied | ErrorKind::ReadOnlyFilesystem => {
            format!("Failed to read {platform} data: storage is not writable")
        }
        ErrorKind::StorageFull => {
            format!("Failed to read {platform} data: not enough disk space")
        }
        _ => format!("Failed to read {platform} data"),
    }
}

fn platform_cache_parse_failed(platform: &str, error: impl std::fmt::Display) -> String {
    tracing::error!(%error, platform, "failed to parse platform cache");
    format!("Failed to parse {platform} data")
}

// Platform 相关

pub(super) async fn execute_platform_read(
    params: &HashMap<String, Value>,
) -> Result<Value, String> {
    let platform_raw = params
        .get("platform")
        .and_then(|v| v.as_str())
        .ok_or("Missing platform parameter")?;
    let platform_lower = platform_raw.to_lowercase();
    let platform = validate_platform_name(&platform_lower)?;

    let cache_file = platform_filtered_file(platform);
    let content = tokio::fs::read_to_string(&cache_file)
        .await
        .map_err(|error| platform_cache_read_failed(platform, error))?;

    let data: Value = serde_json::from_str(&content)
        .map_err(|error| platform_cache_parse_failed(platform, error))?;

    let items = extract_platform_items(&platform.to_lowercase(), &data);
    let mut filtered_items = items;

    // 时间过滤
    if let Some(since) = params.get("since").and_then(|v| v.as_str()) {
        if let Ok(since_time) = chrono::DateTime::parse_from_rfc3339(since) {
            filtered_items.retain(|item| {
                item.get("createdAt")
                    .or_else(|| item.get("created_at"))
                    .or_else(|| item.get("timestamp"))
                    .and_then(|v| v.as_str())
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                    .map(|t| t >= since_time)
                    .unwrap_or(true)
            });
        }
    }

    // 数量限制
    if let Some(limit) = params.get("limit").and_then(|v| v.as_u64()) {
        filtered_items.truncate(limit as usize);
    }

    Ok(json!({
        "platform": platform,
        "items": filtered_items,
        "total": filtered_items.len(),
        "raw_data": data
    }))
}

pub(super) async fn execute_platform_stats(
    params: &HashMap<String, Value>,
) -> Result<Value, String> {
    let platform_raw = params
        .get("platform")
        .and_then(|v| v.as_str())
        .ok_or("Missing platform parameter")?;
    let platform_lower = platform_raw.to_lowercase();
    let platform = validate_platform_name(&platform_lower)?;

    let cache_file = platform_filtered_file(platform);
    let content = tokio::fs::read_to_string(&cache_file)
        .await
        .map_err(|error| platform_cache_read_failed(platform, error))?;

    let data: Value = serde_json::from_str(&content)
        .map_err(|error| platform_cache_parse_failed(platform, error))?;

    // 根据平台类型使用专门的分析函数
    match platform.to_lowercase().as_str() {
        "steam" => analyze_steam_stats(&data),
        "bilibili" => analyze_bilibili_stats(&data),
        "github" => analyze_github_stats(&data),
        "netease" => analyze_netease_stats(&data),
        _ => {
            // 通用平台统计
            let items = extract_platform_items(&platform.to_lowercase(), &data);
            let total = items.len();

            let mut distribution: HashMap<String, usize> = HashMap::new();
            for item in &items {
                if let Some(item_type) = item.get("type").and_then(|v| v.as_str()) {
                    *distribution.entry(item_type.to_string()).or_default() += 1;
                }
            }

            Ok(json!({
                "platform": platform,
                "total": total,
                "distribution": distribution
            }))
        }
    }
}

/// 分析 Bilibili 统计数据
fn analyze_bilibili_stats(data: &Value) -> Result<Value, String> {
    let content_analysis = data.get("content_analysis");

    // 获取番剧分析
    let anime_analysis = content_analysis
        .and_then(|v| v.get("anime_analysis"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let anime_count = anime_analysis.len();

    // 统计番剧类型分布
    let mut genre_distribution: HashMap<String, usize> = HashMap::new();
    for anime in &anime_analysis {
        if let Some(genres) = anime.get("genres").and_then(|v| v.as_object()) {
            for genre in genres.keys() {
                *genre_distribution.entry(genre.clone()).or_default() += 1;
            }
        }
    }

    // 获取观看进度统计
    let mut completed = 0;
    let mut watching = 0;
    for anime in &anime_analysis {
        if let Some(progress) = anime.get("progress").and_then(|v| v.as_str()) {
            if progress.contains("已看完") || progress.contains("全部") {
                completed += 1;
            } else {
                watching += 1;
            }
        }
    }

    // 用户摘要
    let username = data
        .get("user_summary")
        .and_then(|v| v.get("username"))
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown");

    let summary = content_analysis
        .and_then(|v| v.get("summary"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    Ok(json!({
        "platform": "bilibili",
        "username": username,
        "summary": summary,
        "statistics": {
            "total_anime": anime_count,
            "completed": completed,
            "watching": watching
        },
        "distribution": {
            "by_genre": genre_distribution
        },
        "top_anime": anime_analysis.iter().take(10).collect::<Vec<_>>()
    }))
}

/// 分析 GitHub 统计数据
fn analyze_github_stats(data: &Value) -> Result<Value, String> {
    let content_analysis = data.get("content_analysis");

    // 语言分布
    let language_distribution = content_analysis
        .and_then(|v| v.get("language_distribution"))
        .cloned()
        .unwrap_or(json!({}));

    // 仓库数据
    let repositories = content_analysis
        .and_then(|v| v.get("repositories"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let repo_count = repositories.len();

    // 计算总 stars
    let total_stars: u64 = repositories
        .iter()
        .filter_map(|r| r.get("stars").and_then(|v| v.as_u64()))
        .sum();

    // 用户摘要
    let username = data
        .get("user_summary")
        .and_then(|v| v.get("username"))
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown");

    let summary = content_analysis
        .and_then(|v| v.get("summary"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    Ok(json!({
        "platform": "github",
        "username": username,
        "summary": summary,
        "statistics": {
            "total_repos": repo_count,
            "total_stars": total_stars
        },
        "distribution": {
            "by_language": language_distribution
        },
        "top_repos": repositories.iter().take(10).collect::<Vec<_>>()
    }))
}

/// 分析网易云音乐统计数据
fn analyze_netease_stats(data: &Value) -> Result<Value, String> {
    let content_analysis = data.get("content_analysis");
    let artist_analysis = content_analysis.and_then(|v| v.get("artist_analysis"));

    // 获取 Top 艺术家
    let top_artists = artist_analysis
        .and_then(|v| v.get("top_artists"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    // 风格分布
    let genre_analysis = artist_analysis
        .and_then(|v| v.get("genre_analysis"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    // 统计播放次数
    let total_plays: u64 = top_artists
        .iter()
        .filter_map(|a| a.get("play_count").and_then(|v| v.as_u64()))
        .sum();

    // 用户摘要
    let username = data
        .get("user_summary")
        .and_then(|v| v.get("username"))
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown");

    let music_summary = content_analysis
        .and_then(|v| v.get("music_summary"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    Ok(json!({
        "platform": "netease",
        "username": username,
        "summary": music_summary,
        "statistics": {
            "total_artists": top_artists.len(),
            "total_plays": total_plays,
            "genres_count": genre_analysis.len()
        },
        "distribution": {
            "by_genre": genre_analysis
        },
        "top_artists": top_artists.iter().take(10).collect::<Vec<_>>()
    }))
}

/// 分析 Steam 游戏统计数据
fn analyze_steam_stats(data: &Value) -> Result<Value, String> {
    // 获取游戏列表
    let recent_games = data
        .get("content_analysis")
        .and_then(|v| v.get("recent_games"))
        .and_then(|v| v.as_array());

    // 计算游戏时间分布
    let mut total_playtime: u64 = 0;
    let mut game_count = 0;
    let mut playtime_distribution: HashMap<String, u64> = HashMap::new();
    let mut games_by_time: Vec<(String, u64)> = Vec::new();

    // 从 recent_games 获取数据
    if let Some(games) = recent_games {
        for game in games {
            let name = game
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown");
            let playtime = game.get("playtime").and_then(|v| v.as_u64()).unwrap_or(0);

            total_playtime += playtime;
            game_count += 1;
            games_by_time.push((name.to_string(), playtime));
        }
    }

    // 排序（按游戏时间降序）
    games_by_time.sort_by_key(|b| Reverse(b.1));

    // 计算时间段分布
    for (_, playtime) in &games_by_time {
        let hours = *playtime / 60;
        let category = match hours {
            0..=10 => "少于10小时",
            11..=50 => "10-50小时",
            51..=100 => "50-100小时",
            101..=200 => "100-200小时",
            _ => "200小时以上",
        };
        *playtime_distribution
            .entry(category.to_string())
            .or_default() += 1;
    }

    // 获取 Top 10 游戏
    let top_games: Vec<Value> = games_by_time
        .iter()
        .take(10)
        .map(|(name, playtime)| {
            json!({
                "name": name,
                "playtime_minutes": playtime,
                "playtime_hours": *playtime as f64 / 60.0
            })
        })
        .collect();

    // 获取类型分析
    let genre_analysis = data
        .get("content_analysis")
        .and_then(|v| v.get("genre_analysis"))
        .cloned()
        .unwrap_or(json!([]));

    // 获取用户摘要
    let username = data
        .get("user_summary")
        .and_then(|v| v.get("username"))
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown");

    let summary = data
        .get("content_analysis")
        .and_then(|v| v.get("summary"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    Ok(json!({
        "platform": "steam",
        "username": username,
        "summary": summary,
        "statistics": {
            "total_games": game_count,
            "total_playtime_hours": total_playtime as f64 / 60.0
        },
        "distribution": {
            "by_playtime": playtime_distribution,
            "by_genre": genre_analysis
        },
        "top_games": top_games
    }))
}

/// 从平台数据中提取标准化的 items
pub(super) fn extract_platform_items(platform: &str, data: &Value) -> Vec<Value> {
    match platform {
        "steam" => {
            let mut items = Vec::new();
            if let Some(games) = data
                .get("content_analysis")
                .and_then(|v| v.get("recent_games"))
                .and_then(|v| v.as_array())
            {
                for game in games {
                    items.push(json!({
                        "type": "game",
                        "name": game.get("name").and_then(|v| v.as_str()).unwrap_or("Unknown"),
                        "playtime_minutes": game.get("playtime").and_then(|v| v.as_u64()).unwrap_or(0),
                        "appid": game.get("appid"),
                        "icon_url": game.get("icon_url")
                    }));
                }
            }
            items
        }
        "bilibili" => {
            let mut items = Vec::new();
            if let Some(anime_list) = data
                .get("content_analysis")
                .and_then(|v| v.get("anime_analysis"))
                .and_then(|v| v.as_array())
            {
                for anime in anime_list {
                    items.push(json!({
                        "type": "anime",
                        "title": anime.get("title").and_then(|v| v.as_str()).unwrap_or("Unknown"),
                        "progress": anime.get("progress"),
                        "genres": anime.get("genres")
                    }));
                }
            }
            items
        }
        "github" => {
            let mut items = Vec::new();
            if let Some(repos) = data
                .get("content_analysis")
                .and_then(|v| v.get("repositories"))
                .and_then(|v| v.as_array())
            {
                items.extend(repos.clone());
            }
            items
        }
        // Filtered YouTube cache: content_analysis.recent_videos (public uploads)
        "youtube" => {
            let mut items = Vec::new();
            if let Some(videos) = data
                .get("content_analysis")
                .and_then(|v| v.get("recent_videos"))
                .and_then(|v| v.as_array())
            {
                for video in videos {
                    let video_id = video.get("video_id").and_then(|v| v.as_str()).unwrap_or("");
                    items.push(json!({
                        "type": "video",
                        "title": video.get("title").and_then(|v| v.as_str()).unwrap_or("Untitled"),
                        "video_id": video_id,
                        "cover": video.get("cover"),
                        "view_count": video.get("view_count"),
                        "like_count": video.get("like_count"),
                        "published_at": video.get("published_at"),
                        "url": video.get("url").cloned().unwrap_or_else(|| {
                            if video_id.is_empty() {
                                Value::Null
                            } else {
                                json!(format!("https://www.youtube.com/watch?v={video_id}"))
                            }
                        }),
                    }));
                }
            }
            items
        }
        "netease" => {
            let mut items = Vec::new();
            if let Some(artists) = data
                .get("content_analysis")
                .and_then(|v| v.get("artist_analysis"))
                .and_then(|v| v.get("top_artists"))
                .and_then(|v| v.as_array())
            {
                for artist in artists {
                    items.push(json!({
                        "type": "artist",
                        "name": artist.get("name"),
                        "play_count": artist.get("play_count")
                    }));
                }
            }
            items
        }
        // Bangumi / MAL 过滤结果同构：top_rated / watching / recent
        "bangumi" | "mal" => {
            let mut items = Vec::new();
            let content = data.get("content_analysis");
            for key in ["top_rated_subjects", "watching_subjects", "recent_updates"] {
                if let Some(subjects) = content.and_then(|v| v.get(key)).and_then(|v| v.as_array())
                {
                    for subject in subjects {
                        items.push(json!({
                            "type": subject.get("subject_type").and_then(|v| v.as_str()).unwrap_or("subject"),
                            "title": subject.get("title"),
                            "rate": subject.get("rate"),
                            "collection_type": subject.get("collection_type"),
                            "subject_id": subject.get("subject_id"),
                            "platform": platform
                        }));
                    }
                }
            }
            items
        }
        "x" => data
            .get("content_analysis")
            .and_then(|v| v.get("top_posts").or_else(|| v.get("recent_posts")))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default(),
        "discord" => data
            .get("content_analysis")
            .and_then(|v| v.get("guilds_preview"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default(),
        _ => data
            .get("items")
            .or_else(|| data.get("data"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default(),
    }
}

#[cfg(test)]
mod extract_platform_items_tests {
    use super::extract_platform_items;
    use serde_json::json;

    #[test]
    fn youtube_extracts_recent_videos_from_filtered_cache_shape() {
        // Shape matches SmartFilter youtube_filtered.json (untagged YouTubeAnalysis)
        let data = json!({
            "platform": "youtube",
            "user_summary": {
                "username": "Google for Developers",
                "user_id": "UC_x5XG1OV2P6uZZ5FSM9Ttw",
                "level": "@GoogleDevelopers",
                "stats": { "follower_count": 2300000, "total_content": 5800 }
            },
            "content_analysis": {
                "video_summary": "sample",
                "subscriber_count": 2300000,
                "view_count": 250000000,
                "video_count": 5800,
                "recent_videos": [
                    {
                        "title": "Sample Upload One",
                        "video_id": "dQw4w9WgXcQ",
                        "cover": "https://i.ytimg.com/vi/dQw4w9WgXcQ/mqdefault.jpg",
                        "view_count": 1000,
                        "like_count": 50,
                        "url": "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
                    },
                    {
                        "title": "Sample Upload Two",
                        "video_id": "abc123xyz",
                        "cover": "https://i.ytimg.com/vi/abc123xyz/mqdefault.jpg",
                        "view_count": 200
                    }
                ]
            }
        });

        let items = extract_platform_items("youtube", &data);
        assert_eq!(items.len(), 2, "youtube must surface recent_videos items");
        assert_eq!(items[0]["type"], "video");
        assert_eq!(items[0]["video_id"], "dQw4w9WgXcQ");
        assert_eq!(items[0]["title"], "Sample Upload One");
        assert_eq!(items[0]["view_count"], 1000);
        assert!(
            items[0]["url"]
                .as_str()
                .unwrap_or("")
                .contains("dQw4w9WgXcQ"),
            "url should point at watch page"
        );
        // Fallback path without youtube branch would return []
        let empty = extract_platform_items("unknown-platform", &data);
        assert!(empty.is_empty());
    }
}
