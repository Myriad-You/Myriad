/// 缓存管理 API
///
/// 提供缓存状态查询、清除等功能
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;

use crate::services::image_proxy_urls::proxy_image_url;

#[derive(Debug, Serialize)]
pub struct CacheInfo {
    pub platform: String,
    pub exists: bool,
    pub size_bytes: Option<u64>,
    pub modified_at: Option<String>,
    pub path: String,
}

#[derive(Debug, Deserialize)]
pub struct ClearCacheRequest {
    pub platforms: Option<Vec<String>>,
}

/// 获取所有平台缓存状态
///
/// GET /api/cache/status
pub async fn get_cache_status(State(_db): State<DatabaseConnection>) -> (StatusCode, Json<Value>) {
    let platforms = vec![
        "netease", "bilibili", "github", "steam", "youtube", "bangumi", "x", "discord", "mal",
        "xbox", "psn",
    ];
    let mut cache_info = Vec::new();

    for platform in platforms {
        let info = get_platform_cache_info(platform);
        cache_info.push(info);
    }

    // 计算总大小
    let total_size: u64 = cache_info.iter().filter_map(|info| info.size_bytes).sum();

    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "caches": cache_info,
            "total_size_bytes": total_size,
            "total_size_mb": format!("{:.2}", total_size as f64 / 1024.0 / 1024.0),
        })),
    )
}

/// 获取单个平台缓存状态
///
/// GET /api/cache/status/{platform}
pub async fn get_platform_cache_status(
    State(_db): State<DatabaseConnection>,
    Path(platform): Path<String>,
) -> (StatusCode, Json<Value>) {
    let info = get_platform_cache_info(&platform);

    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "cache": info
        })),
    )
}

/// 平台智能过滤缓存的「即时快照」——设置二级页一次加载，不轮询。
///
/// GET /api/cache/preview/{platform}
///
/// 从 `{platform}_filtered.json` 抽取：
/// - user_summary（账号）
/// - 一条文字 summary（各平台 *\_summary）
/// - 少量数值 metrics
/// - 最多 10 条样本（封面/标题）
pub async fn get_platform_cache_preview(
    State(_db): State<DatabaseConnection>,
    Path(platform): Path<String>,
) -> (StatusCode, Json<Value>) {
    let slug = normalize_cache_platform_slug(&platform);
    let body = build_platform_preview(&slug, PreviewOptions::detail());
    (StatusCode::OK, Json(body))
}

/// 列表入口用的批量轻量快照——一次返回各平台 user + 少量 metrics，无 samples，不轮询。
///
/// GET /api/cache/previews
pub async fn get_all_platform_cache_previews(
    State(_db): State<DatabaseConnection>,
) -> (StatusCode, Json<Value>) {
    let platforms = [
        "netease", "bilibili", "github", "steam", "youtube", "bangumi", "x", "discord", "mal",
        "xbox", "psn",
    ];
    let mut previews = serde_json::Map::new();
    for platform in platforms {
        previews.insert(
            platform.to_string(),
            build_platform_preview(platform, PreviewOptions::list_card()),
        );
    }

    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "previews": previews,
        })),
    )
}

struct PreviewOptions {
    /// 是否包含 samples
    include_samples: bool,
    /// metrics 条数上限
    max_metrics: usize,
}

impl PreviewOptions {
    fn detail() -> Self {
        Self {
            include_samples: true,
            max_metrics: 8,
        }
    }

    /// 入口卡片：无 samples；指标可略多，前端数据行 marquee 循环展示
    fn list_card() -> Self {
        Self {
            include_samples: false,
            max_metrics: 6,
        }
    }
}

/// 读取单个平台 filtered 缓存并组装预览 JSON（含 success / platform / exists 等）。
fn build_platform_preview(slug: &str, opts: PreviewOptions) -> Value {
    let info = get_platform_cache_info(slug);

    if !info.exists {
        return json!({
            "success": true,
            "platform": slug,
            "exists": false,
            "modified_at": Value::Null,
            "user": Value::Null,
            "summary": Value::Null,
            "metrics": [],
            "samples": [],
        });
    }

    let cache_path = PathBuf::from(format!("./cache/platforms/{}_filtered.json", slug));
    let content = match fs::read_to_string(&cache_path) {
        Ok(c) => c,
        Err(error) => {
            tracing::warn!(platform = %slug, %error, "Failed to read filtered cache for preview");
            return json!({
                "success": true,
                "platform": slug,
                "exists": true,
                "modified_at": info.modified_at,
                "user": Value::Null,
                "summary": Value::Null,
                "metrics": [],
                "samples": [],
                "message": "Cache file unreadable",
            });
        }
    };

    let data: Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(error) => {
            tracing::warn!(platform = %slug, %error, "Failed to parse filtered cache for preview");
            return json!({
                "success": true,
                "platform": slug,
                "exists": true,
                "modified_at": info.modified_at,
                "user": Value::Null,
                "summary": Value::Null,
                "metrics": [],
                "samples": [],
                "message": "Cache file invalid",
            });
        }
    };

    let user = extract_preview_user(&data);
    let analysis = data.get("content_analysis").cloned().unwrap_or(Value::Null);
    let summary = extract_preview_summary(&analysis);
    let mut metrics = extract_preview_metrics(&data, &analysis);
    // 入口卡：丢掉无意义的 0 值，给真实指标腾位
    if !opts.include_samples {
        metrics.retain(|m| {
            let v = m.get("value");
            match v {
                Some(Value::Number(n)) => n
                    .as_f64()
                    .map(|f| f.is_finite() && f.abs() > f64::EPSILON)
                    .unwrap_or(false),
                _ => false,
            }
        });
    }
    if metrics.len() > opts.max_metrics {
        metrics.truncate(opts.max_metrics);
    }
    let samples = if opts.include_samples {
        extract_preview_samples(&analysis, data.get("raw_unknown_content"))
    } else {
        Vec::new()
    };

    json!({
        "success": true,
        "platform": slug,
        "exists": true,
        "modified_at": info.modified_at,
        "user": user,
        "summary": summary,
        "metrics": metrics,
        "samples": samples,
    })
}

/// 与 smart_filter / platforms 列表一致的缓存文件 slug。
fn normalize_cache_platform_slug(platform: &str) -> String {
    let key = platform.trim().to_ascii_lowercase().replace(' ', "_");
    match key.as_str() {
        "netease_music" | "netease-music" | "neteasecloud" => "netease".into(),
        "myanimelist" | "my_anime_list" => "mal".into(),
        "playstation" | "play_station" => "psn".into(),
        "twitter" | "x_twitter" => "x".into(),
        "bgm" => "bangumi".into(),
        other => other.to_string(),
    }
}

fn extract_preview_user(data: &Value) -> Value {
    let Some(summary) = data.get("user_summary") else {
        return Value::Null;
    };
    let stats = summary.get("stats");
    json!({
        "username": summary.get("username").and_then(|v| v.as_str()).unwrap_or(""),
        "user_id": summary.get("user_id").and_then(|v| v.as_str()).unwrap_or(""),
        "level": summary.get("level").cloned().unwrap_or(Value::Null),
        "follower_count": stats.and_then(|s| s.get("follower_count")).cloned().unwrap_or(Value::Null),
        "following_count": stats.and_then(|s| s.get("following_count")).cloned().unwrap_or(Value::Null),
        "total_content": stats.and_then(|s| s.get("total_content")).and_then(|v| v.as_u64()).unwrap_or(0),
    })
}

fn extract_preview_summary(analysis: &Value) -> Value {
    if !analysis.is_object() {
        return Value::Null;
    }
    const KEYS: &[&str] = &[
        "game_summary",
        "video_summary",
        "music_summary",
        "repo_summary",
        "collection_summary",
        "post_summary",
        "gaming_summary",
        "trophy_summary",
        "discord_summary",
        "summary",
    ];
    for key in KEYS {
        if let Some(s) = analysis.get(*key).and_then(|v| v.as_str()) {
            let t = s.trim();
            if !t.is_empty() {
                return Value::String(t.to_string());
            }
        }
    }
    Value::Null
}

fn push_metric(out: &mut Vec<Value>, key: &str, value: &Value) {
    if value.is_null() {
        return;
    }
    if let Some(n) = value.as_i64() {
        out.push(json!({ "key": key, "value": n }));
        return;
    }
    if let Some(n) = value.as_u64() {
        out.push(json!({ "key": key, "value": n }));
        return;
    }
    if let Some(n) = value.as_f64() {
        if n.is_finite() {
            out.push(json!({ "key": key, "value": (n * 10.0).round() / 10.0 }));
        }
    }
}

fn extract_preview_metrics(data: &Value, analysis: &Value) -> Vec<Value> {
    let mut metrics = Vec::new();

    if let Some(stats) = data.get("user_summary").and_then(|u| u.get("stats")) {
        push_metric(
            &mut metrics,
            "total_content",
            stats.get("total_content").unwrap_or(&Value::Null),
        );
        push_metric(
            &mut metrics,
            "follower_count",
            stats.get("follower_count").unwrap_or(&Value::Null),
        );
        push_metric(
            &mut metrics,
            "following_count",
            stats.get("following_count").unwrap_or(&Value::Null),
        );
    }

    if !analysis.is_object() {
        return metrics;
    }

    const KEYS: &[&str] = &[
        "games_count",
        "total_playtime_minutes",
        "public_repos",
        "subscriber_count",
        "view_count",
        "video_count",
        "gamerscore",
        "achievement_games",
        "completed_games",
        "total_achievements_earned",
        "total_achievements_available",
        "average_completion",
        "hardcore_score",
        "mean_score",
        "days_watched",
        "trophy_level",
        "trophy_count",
        "bronze",
        "silver",
        "gold",
        "platinum",
    ];
    for key in KEYS {
        if let Some(v) = analysis.get(*key) {
            push_metric(&mut metrics, key, v);
        }
    }

    // artist_analysis.total_songs (netease)
    if let Some(aa) = analysis.get("artist_analysis") {
        push_metric(
            &mut metrics,
            "total_songs",
            aa.get("total_songs").unwrap_or(&Value::Null),
        );
    }

    // engagement_stats (x)
    if let Some(es) = analysis.get("engagement_stats") {
        push_metric(
            &mut metrics,
            "total_posts",
            es.get("total_posts").unwrap_or(&Value::Null),
        );
        push_metric(
            &mut metrics,
            "liked_posts_count",
            es.get("liked_posts_count").unwrap_or(&Value::Null),
        );
    }

    // Deduplicate by key while preserving order
    let mut seen = std::collections::HashSet::new();
    metrics.retain(|m| {
        let key = m
            .get("key")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        seen.insert(key)
    });

    // Cap metrics so the UI stays scannable
    metrics.truncate(8);
    metrics
}

fn sample_from_object(item: &Value) -> Option<Value> {
    if !item.is_object() {
        return None;
    }
    let title = item
        .get("title")
        .or_else(|| item.get("name"))
        .or_else(|| item.get("username"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())?;

    let image = item
        .get("image")
        .or_else(|| item.get("cover"))
        .or_else(|| item.get("profile_image_url"))
        .or_else(|| item.get("avatar"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(proxy_image_url)
        .filter(|s| !s.is_empty());

    let subtitle = item
        .get("artist")
        .or_else(|| item.get("language"))
        .or_else(|| item.get("subject_type"))
        .or_else(|| item.get("collection_type"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .or_else(|| {
            item.get("playtime")
                .and_then(|v| v.as_i64().or_else(|| v.as_u64().map(|n| n as i64)))
                .filter(|&n| n > 0)
                .map(|n| format!("{n}m"))
        })
        .or_else(|| {
            item.get("stars")
                .and_then(|v| v.as_i64())
                .filter(|&n| n > 0)
                .map(|n| format!("★ {n}"))
        })
        .or_else(|| {
            item.get("rate")
                .and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f.round() as i64)))
                .filter(|&n| n > 0)
                .map(|n| format!("{n}/10"))
        })
        .or_else(|| {
            item.get("follower_count")
                .and_then(|v| v.as_i64())
                .filter(|&n| n > 0)
                .map(|n| format!("{n}"))
        });

    Some(json!({
        "title": title,
        "subtitle": subtitle,
        "image": image,
    }))
}

fn push_preview_samples_from_list(
    samples: &mut Vec<Value>,
    seen_titles: &mut std::collections::HashSet<String>,
    arr: Option<&Value>,
    max: usize,
) {
    let Some(list) = arr.and_then(|v| v.as_array()) else {
        return;
    };
    for item in list {
        if samples.len() >= max {
            return;
        }
        if let Some(sample) = sample_from_object(item) {
            let title = sample
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if title.is_empty() || !seen_titles.insert(title) {
                continue;
            }
            samples.push(sample);
        }
    }
}

fn extract_preview_samples(analysis: &Value, raw_unknown: Option<&Value>) -> Vec<Value> {
    const MAX: usize = 10;
    let mut samples = Vec::new();
    let mut seen_titles = std::collections::HashSet::new();

    if analysis.is_object() {
        const LISTS: &[&str] = &[
            "recent_games",
            "recent_videos",
            "recent_songs",
            "recent_repos",
            "recent_posts",
            "top_posts",
            "recent_titles",
            "top_completed_titles",
            "watching_subjects",
            "top_rated_subjects",
            "recent_updates",
            "following_sample",
        ];
        for key in LISTS {
            if samples.len() >= MAX {
                break;
            }
            push_preview_samples_from_list(&mut samples, &mut seen_titles, analysis.get(*key), MAX);
        }
    }

    if samples.len() < MAX {
        push_preview_samples_from_list(&mut samples, &mut seen_titles, raw_unknown, MAX);
    }

    samples
}

/// 清除指定平台的缓存
///
/// DELETE /api/cache/{platform}
pub async fn clear_platform_cache(
    State(_db): State<DatabaseConnection>,
    Path(platform): Path<String>,
) -> (StatusCode, Json<Value>) {
    let cache_path = PathBuf::from(format!("./cache/platforms/{}_filtered.json", platform));

    if !cache_path.exists() {
        return (
            StatusCode::OK,
            Json(json!({
                "success": true,
                "message": format!("Cache for {} does not exist", platform)
            })),
        );
    }

    match fs::remove_file(&cache_path) {
        Ok(_) => {
            tracing::info!("✓ Cleared cache for {}", platform);
            (
                StatusCode::OK,
                Json(json!({
                    "success": true,
                    "message": format!("Successfully cleared cache for {}", platform)
                })),
            )
        }
        Err(e) => {
            tracing::error!("❌ Failed to clear cache for {}: {}", platform, e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "error": "Failed to clear cache",
                    "code": "cache_clear_failed"
                })),
            )
        }
    }
}

/// 批量清除缓存
///
/// POST /api/cache/clear
/// Body: { "platforms": ["netease", "bilibili"] } 或 {} 清除所有
pub async fn clear_caches(
    State(_db): State<DatabaseConnection>,
    Json(payload): Json<ClearCacheRequest>,
) -> (StatusCode, Json<Value>) {
    let platforms = match payload.platforms {
        Some(p) => p,
        None => vec![
            "netease".to_string(),
            "bilibili".to_string(),
            "github".to_string(),
            "steam".to_string(),
            "youtube".to_string(),
            "bangumi".to_string(),
            "x".to_string(),
            "discord".to_string(),
            "mal".to_string(),
            "xbox".to_string(),
            "psn".to_string(),
        ],
    };

    let mut cleared = Vec::new();
    let mut errors = Vec::new();

    for platform in platforms {
        let cache_path = PathBuf::from(format!("./cache/platforms/{}_filtered.json", platform));

        if !cache_path.exists() {
            continue;
        }

        match fs::remove_file(&cache_path) {
            Ok(_) => {
                tracing::info!("✓ Cleared cache for {}", platform);
                cleared.push(platform);
            }
            Err(e) => {
                tracing::error!("❌ Failed to clear cache for {}: {}", platform, e);
                errors.push(json!({
                    "platform": platform,
                    "error": "Failed to clear cache",
                    "code": "cache_clear_failed"
                }));
            }
        }
    }

    (
        StatusCode::OK,
        Json(json!({
            "success": errors.is_empty(),
            "cleared": cleared,
            "errors": errors,
            "message": format!("Cleared {} cache(s)", cleared.len())
        })),
    )
}

/// 清除所有缓存（包括原始数据）
///
/// DELETE /api/cache/all
pub async fn clear_all_caches(State(_db): State<DatabaseConnection>) -> (StatusCode, Json<Value>) {
    let cache_dir = PathBuf::from("./cache/platforms");
    let mut removed_files = Vec::new();
    let mut errors = Vec::new();

    if !cache_dir.exists() {
        return (
            StatusCode::OK,
            Json(json!({
                "success": true,
                "message": "Cache directory does not exist",
                "removed": []
            })),
        );
    }

    // 读取缓存目录
    match fs::read_dir(&cache_dir) {
        Ok(entries) => {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    let file_name = path.file_name().unwrap().to_string_lossy().to_string();

                    // 只删除 JSON 缓存文件
                    if file_name.ends_with(".json") {
                        match fs::remove_file(&path) {
                            Ok(_) => {
                                tracing::info!("✓ Removed cache file: {}", file_name);
                                removed_files.push(file_name);
                            }
                            Err(e) => {
                                tracing::error!("❌ Failed to remove {}: {}", file_name, e);
                                errors.push(json!({
                                    "file": file_name,
                                    "error": "Failed to clear cache",
                                    "code": "cache_clear_failed"
                                }));
                            }
                        }
                    }
                }
            }
        }
        Err(e) => {
            tracing::error!("Failed to read cache directory: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "error": "Failed to read cache directory",
                    "code": "cache_clear_failed"
                })),
            );
        }
    }

    (
        StatusCode::OK,
        Json(json!({
            "success": errors.is_empty(),
            "removed": removed_files,
            "errors": errors,
            "message": format!("Removed {} cache file(s)", removed_files.len())
        })),
    )
}

/// 辅助函数：获取平台缓存信息
fn get_platform_cache_info(platform: &str) -> CacheInfo {
    let cache_path = PathBuf::from(format!("./cache/platforms/{}_filtered.json", platform));

    let (exists, size_bytes, modified_at) = if cache_path.exists() {
        match fs::metadata(&cache_path) {
            Ok(metadata) => {
                let size = metadata.len();
                let modified = metadata
                    .modified()
                    .ok()
                    .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|duration| {
                        chrono::DateTime::from_timestamp(duration.as_secs() as i64, 0)
                            .map(|dt| dt.to_rfc3339())
                            .unwrap_or_default()
                    });
                (true, Some(size), modified)
            }
            Err(_) => (true, None, None),
        }
    } else {
        (false, None, None)
    };

    CacheInfo {
        platform: platform.to_string(),
        exists,
        size_bytes,
        modified_at,
        path: cache_path.to_string_lossy().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_samples_proxy_bangumi_hotlink_covers() {
        let sample = sample_from_object(&json!({
            "title": "Example",
            "cover": "https://lain.bgm.tv/pic/cover/l/1.jpg",
            "rate": 8,
        }))
        .expect("sample");
        let image = sample.get("image").and_then(|v| v.as_str()).unwrap();
        assert!(
            image.starts_with("/api/proxy/image?url="),
            "bangumi covers must go through the image proxy: {image}"
        );
        assert!(image.contains("lain.bgm.tv"), "{image}");
    }

    #[test]
    fn preview_samples_leave_non_hotlink_https() {
        let sample = sample_from_object(&json!({
            "title": "Repo",
            "image": "https://avatars.githubusercontent.com/u/1",
        }))
        .expect("sample");
        assert_eq!(
            sample.get("image").and_then(|v| v.as_str()),
            Some("https://avatars.githubusercontent.com/u/1")
        );
    }
}
