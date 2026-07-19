//! 平台数据 API

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Extension, Json,
};
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};

use crate::middleware::auth::Claims;
use crate::services::permission_service::TappPermission;

use super::common::{
    acquire_platform_lock, authorize_tapp_permission, get_cached_platform_data,
    update_cached_platform_data, validate_platform_name,
};
use super::runtime_grant::RuntimeGrantContext;

#[derive(Debug, Deserialize)]
pub struct PlatformDataQuery {
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

/// Smart-filter caches store content under `raw_unknown_content` / `content_analysis`,
/// not a top-level `items` array. Project those shapes into a uniform items[] for Tapps.
/// Prefer existing `items` when present (e.g. tapp-written entries).
fn extract_platform_items(data: &Value, platform: &str) -> Vec<Value> {
    if let Some(items) = data.get("items").and_then(|v| v.as_array()) {
        if !items.is_empty() {
            return items
                .iter()
                .enumerate()
                .map(|(i, item)| normalize_platform_item(item, platform, i))
                .collect();
        }
    }

    let mut items = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    if let Some(raw) = data.get("raw_unknown_content").and_then(|v| v.as_array()) {
        for (i, entry) in raw.iter().enumerate() {
            let item = project_unknown_content(entry, platform, i);
            let key = item_dedupe_key(&item);
            if seen.insert(key) {
                items.push(item);
            }
        }
    }

    if let Some(analysis) = data.get("content_analysis") {
        for item in project_content_analysis(analysis, platform, items.len()) {
            let key = item_dedupe_key(&item);
            if seen.insert(key) {
                items.push(item);
            }
        }
    }

    items
}

fn item_dedupe_key(item: &Value) -> String {
    // Prefer title so raw_unknown_content and content_analysis lists don't double-list the same entry.
    let title = item
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_lowercase();
    if !title.is_empty() {
        return title;
    }
    item.get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn first_string(obj: &Map<String, Value>, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(v) = obj.get(*key) {
            if let Some(s) = v.as_str() {
                let trimmed = s.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            } else if let Some(n) = v.as_i64() {
                return Some(n.to_string());
            } else if let Some(n) = v.as_u64() {
                return Some(n.to_string());
            } else if let Some(n) = v.as_f64() {
                return Some(n.to_string());
            }
        }
    }
    None
}

fn normalize_platform_item(item: &Value, platform: &str, index: usize) -> Value {
    let obj = item.as_object();
    let title = obj
        .and_then(|o| first_string(o, &["title", "name", "username", "label"]))
        .unwrap_or_else(|| format!("Item {}", index + 1));
    let item_type = obj
        .and_then(|o| first_string(o, &["type", "content_type", "subject_type"]))
        .unwrap_or_else(|| "item".to_string());
    let id = obj
        .and_then(|o| first_string(o, &["id", "title_id", "subject_id", "item_id"]))
        .unwrap_or_else(|| format!("{platform}_{index}"));
    let image = obj.and_then(|o| {
        first_string(
            o,
            &[
                "image",
                "cover",
                "display_image",
                "profile_image_url",
                "thumbnail",
                "poster",
            ],
        )
    });
    let metadata = item.get("metadata").cloned().unwrap_or_else(|| {
        // Promote remaining scalar fields into metadata for richer picker/detail.
        let mut meta = Map::new();
        if let Some(o) = obj {
            for (k, v) in o {
                if matches!(
                    k.as_str(),
                    "id" | "title" | "name" | "type" | "content_type"
                        | "subject_type" | "image" | "cover" | "display_image"
                        | "profile_image_url" | "thumbnail" | "poster" | "metadata"
                        | "platform" | "description" | "url" | "createdAt" | "source"
                ) {
                    continue;
                }
                if v.is_string() || v.is_number() || v.is_boolean() {
                    meta.insert(k.clone(), v.clone());
                }
            }
        }
        Value::Object(meta)
    });

    let mut out = json!({
        "id": id,
        "title": title,
        "type": item_type,
        "platform": platform,
        "metadata": metadata,
    });
    if let Some(img) = image {
        out["image"] = json!(img);
        out["cover"] = out["image"].clone();
    }
    if let Some(desc) = obj.and_then(|o| first_string(o, &["description", "summary", "desc"])) {
        out["description"] = json!(desc);
    }
    if let Some(url) = obj.and_then(|o| first_string(o, &["url", "link", "web_url"])) {
        out["url"] = json!(url);
    }
    // Preserve original fields that callers may already read.
    if let Some(o) = obj {
        for (k, v) in o {
            if out.get(k).is_none() {
                out[k] = v.clone();
            }
        }
    }
    out
}

fn project_unknown_content(entry: &Value, platform: &str, index: usize) -> Value {
    let obj = entry.as_object();
    let title = obj
        .and_then(|o| first_string(o, &["title", "name"]))
        .unwrap_or_else(|| format!("Item {}", index + 1));
    let item_type = obj
        .and_then(|o| first_string(o, &["content_type", "type"]))
        .unwrap_or_else(|| "item".to_string());
    let metadata = entry
        .get("metadata")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let image = obj.and_then(|o| {
        first_string(o, &["image", "cover", "display_image", "profile_image_url"])
    })
    .or_else(|| {
        metadata.as_object().and_then(|m| {
            first_string(m, &["image", "cover", "display_image", "profile_image_url"])
        })
    });
    let id = obj
        .and_then(|o| first_string(o, &["id", "title_id", "subject_id"]))
        .unwrap_or_else(|| format!("{platform}_{}_{}", slugify_fragment(&item_type), index));

    let mut out = json!({
        "id": id,
        "title": title,
        "type": item_type,
        "platform": platform,
        "metadata": metadata,
    });
    if let Some(img) = image {
        out["image"] = json!(img);
        out["cover"] = out["image"].clone();
    }
    out
}

fn slugify_fragment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if c == '-' || c == '_' || c.is_whitespace() {
            if !out.ends_with('_') {
                out.push('_');
            }
        }
    }
    let trimmed = out.trim_matches('_');
    if trimmed.is_empty() {
        "item".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Keys under content_analysis that are narrative summaries, not item lists.
const SKIP_ANALYSIS_KEYS: &[&str] = &[
    "game_summary",
    "gaming_summary",
    "video_summary",
    "repo_summary",
    "collection_summary",
    "anime_analysis",
    "genre_analysis",
    "language_distribution",
    "contribution_calendar",
    "subject_type_distribution",
    "collection_type_distribution",
    "tag_distribution",
    "music_summary",
    "tweet_summary",
    "server_summary",
];

fn project_content_analysis(analysis: &Value, platform: &str, start_index: usize) -> Vec<Value> {
    let Some(obj) = analysis.as_object() else {
        return Vec::new();
    };

    let mut items = Vec::new();
    let mut index = start_index;

    for (list_key, value) in obj {
        if SKIP_ANALYSIS_KEYS.contains(&list_key.as_str()) {
            continue;
        }
        let Some(arr) = value.as_array() else {
            continue;
        };
        // Skip arrays of pure scalars / date buckets
        let sample = arr.iter().find(|v| v.is_object());
        let Some(sample) = sample else {
            continue;
        };
        let sample_obj = sample.as_object().unwrap();
        // Must look like a content entry (has a display name field)
        if first_string(
            sample_obj,
            &["title", "name", "username", "label", "subject_title"],
        )
        .is_none()
        {
            continue;
        }

        let default_type = list_key
            .trim_start_matches("recent_")
            .trim_start_matches("top_")
            .trim_end_matches("_subjects")
            .trim_end_matches("_titles")
            .trim_end_matches("_games")
            .trim_end_matches("_videos")
            .trim_end_matches("_songs")
            .trim_end_matches("_repos")
            .trim_end_matches("_sample")
            .trim_end_matches('s');
        let default_type = if default_type.is_empty() {
            "item"
        } else {
            default_type
        };

        for entry in arr {
            let Some(entry_obj) = entry.as_object() else {
                continue;
            };
            let title = match first_string(
                entry_obj,
                &["title", "name", "username", "label", "subject_title"],
            ) {
                Some(t) => t,
                None => continue,
            };
            let item_type = first_string(entry_obj, &["content_type", "type", "subject_type"])
                .unwrap_or_else(|| default_type.to_string());
            let id = first_string(
                entry_obj,
                &["id", "title_id", "subject_id", "item_id", "appid"],
            )
            .unwrap_or_else(|| format!("{platform}_{}_{}", slugify_fragment(&item_type), index));
            let image = first_string(
                entry_obj,
                &[
                    "image",
                    "cover",
                    "display_image",
                    "profile_image_url",
                    "thumbnail",
                    "poster",
                ],
            );
            let description =
                first_string(entry_obj, &["description", "summary", "desc", "artist"]);

            let mut metadata = Map::new();
            for (k, v) in entry_obj {
                if matches!(
                    k.as_str(),
                    "id" | "title" | "name" | "username" | "type" | "content_type"
                        | "subject_type" | "image" | "cover" | "display_image"
                        | "profile_image_url" | "thumbnail" | "poster" | "description"
                        | "summary"
                ) {
                    continue;
                }
                if v.is_string() || v.is_number() || v.is_boolean() {
                    metadata.insert(k.clone(), v.clone());
                }
            }

            let mut item = json!({
                "id": id,
                "title": title,
                "type": item_type,
                "platform": platform,
                "metadata": metadata,
                "source_list": list_key,
            });
            if let Some(img) = image {
                item["image"] = json!(img);
                item["cover"] = item["image"].clone();
            }
            if let Some(desc) = description {
                item["description"] = json!(desc);
            }
            items.push(item);
            index += 1;
        }
    }

    items
}

/// GET /api/tapp/platform/{platform}/data
pub async fn get_platform_data(
    State(_db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path(platform): Path<String>,
    Query(query): Query<PlatformDataQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    runtime_grant.require(TappPermission::PlatformRead)?;
    tracing::debug!(
        "[TAPP] get_platform_data - User: {}, Platform: {}",
        claims.username,
        platform
    );

    let data = match get_cached_platform_data(&platform).await {
        Ok(d) => d,
        Err(_) => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "Platform data not found", "platform": platform })),
            ));
        }
    };

    let items = extract_platform_items(&data, &platform);
    let total = items.len();
    let offset = query.offset.unwrap_or(0) as usize;
    let limit = (query.limit.unwrap_or(100) as usize).min(1000);
    let paged_items: Vec<_> = items.into_iter().skip(offset).take(limit).collect();

    Ok(Json(json!({
        "platform": platform,
        "items": paged_items,
        "total": total,
        "offset": offset,
        "limit": limit
    })))
}

/// GET /api/tapp/platform/{platform}/stats
pub async fn get_platform_stats(
    State(_db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path(platform): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    runtime_grant.require(TappPermission::PlatformRead)?;
    tracing::debug!(
        "[TAPP] get_platform_stats - User: {}, Platform: {}",
        claims.username,
        platform
    );

    let data = get_cached_platform_data(&platform)
        .await
        .unwrap_or(json!({ "items": [] }));
    let items = extract_platform_items(&data, &platform);
    let total = items.len();

    let mut type_distribution: HashMap<String, usize> = HashMap::new();
    for item in &items {
        if let Some(item_type) = item.get("type").and_then(|v| v.as_str()) {
            *type_distribution.entry(item_type.to_string()).or_default() += 1;
        }
    }

    Ok(Json(json!({
        "platform": platform,
        "total": total,
        "distribution": type_distribution,
        "recentActivity": []
    })))
}

/// GET /api/tapp/platform/{platform}/distribution/{dimension}
pub async fn get_platform_distribution(
    State(_db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path((platform, dimension)): Path<(String, String)>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    runtime_grant.require(TappPermission::PlatformRead)?;
    tracing::debug!(
        "[TAPP] get_platform_distribution - User: {}, Platform: {}, Dimension: {}",
        claims.username,
        platform,
        dimension
    );

    let data = get_cached_platform_data(&platform)
        .await
        .unwrap_or(json!({ "items": [] }));
    let items = extract_platform_items(&data, &platform);

    let mut distribution: HashMap<String, usize> = HashMap::new();
    for item in &items {
        if let Some(value) = item.get(&dimension).and_then(|v| v.as_str()) {
            *distribution.entry(value.to_string()).or_default() += 1;
        }
    }

    let distribution_data: Vec<Value> = distribution
        .into_iter()
        .map(|(label, value)| json!({ "label": label, "value": value }))
        .collect();

    Ok(Json(json!({
        "dimension": dimension,
        "data": distribution_data
    })))
}

// ============ Platform Write API ============

#[derive(Debug, Deserialize)]
pub struct AddPlatformItemRequest {
    pub tapp_id: String,
    pub item: NewPlatformItem,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct NewPlatformItem {
    pub platform: String,
    #[serde(rename = "type")]
    pub item_type: String,
    pub title: String,
    pub cover: Option<String>,
    pub description: Option<String>,
    pub url: Option<String>,
    pub metadata: Option<Value>,
    #[serde(rename = "createdAt")]
    pub created_at: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PlatformItemResult {
    pub success: bool,
    #[serde(rename = "itemId")]
    pub item_id: String,
    pub source: String,
}

/// POST /api/tapp/platform/items
pub async fn add_platform_item(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Json(req): Json<AddPlatformItemRequest>,
) -> Result<Json<PlatformItemResult>, (StatusCode, Json<Value>)> {
    runtime_grant.require_tapp_id(&req.tapp_id)?;
    runtime_grant.require(TappPermission::PlatformWrite)?;
    authorize_tapp_permission(&db, &claims, &req.tapp_id, TappPermission::PlatformWrite).await?;

    validate_platform_name(&req.item.platform)
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))))?;

    tracing::info!(
        "[TAPP] add_platform_item - User: {}, Tapp: {}, Platform: {}",
        claims.username,
        req.tapp_id,
        req.item.platform
    );

    let item_id = format!("tapp_{}", uuid::Uuid::new_v4());

    let cache_dir = std::path::Path::new("cache/platforms");
    let cache_file = cache_dir.join(format!(
        "{}_filtered.json",
        req.item.platform.to_lowercase()
    ));

    let _platform_guard = acquire_platform_lock(&req.item.platform)
        .await
        .map_err(|error| (StatusCode::BAD_REQUEST, Json(json!({ "error": error }))))?;
    tokio::fs::create_dir_all(cache_dir)
        .await
        .map_err(|error| {
            tracing::error!(
                "[TAPP] Failed to create platform cache directory: {}",
                error
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Failed to save platform data" })),
            )
        })?;
    let mut data = match tokio::fs::read_to_string(&cache_file).await {
        Ok(content) => serde_json::from_str::<Value>(&content).map_err(|error| {
            tracing::error!("[TAPP] Invalid platform cache file: {}", error);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Invalid platform cache data" })),
            )
        })?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => json!({ "items": [] }),
        Err(error) => {
            tracing::error!("[TAPP] Failed to read platform cache: {}", error);
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Failed to read platform data" })),
            ));
        }
    };

    let new_item = json!({
        "id": item_id,
        "type": req.item.item_type,
        "title": req.item.title,
        "cover": req.item.cover,
        "description": req.item.description,
        "url": req.item.url,
        "metadata": req.item.metadata,
        "createdAt": req.item.created_at.unwrap_or_else(|| chrono::Utc::now().to_rfc3339()),
        "source": format!("tapp:{}", req.tapp_id)
    });

    if let Some(items) = data.get_mut("items").and_then(|v| v.as_array_mut()) {
        items.push(new_item);
    }

    // 原子写入：先写临时文件再重命名，避免并发写入导致数据损坏
    let tmp_file = cache_file.with_extension(format!("json.{}.tmp", uuid::Uuid::new_v4()));
    let content = serde_json::to_string_pretty(&data).unwrap();
    if let Err(e) = tokio::fs::write(&tmp_file, &content).await {
        tracing::error!("[TAPP] Failed to write temp cache file: {}", e);
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Failed to save platform data" })),
        ));
    }
    if let Err(e) = tokio::fs::rename(&tmp_file, &cache_file).await {
        tracing::error!("[TAPP] Failed to rename cache file: {}", e);
        let _ = tokio::fs::remove_file(&tmp_file).await;
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Failed to save platform data" })),
        ));
    }
    update_cached_platform_data(&req.item.platform, data)
        .await
        .map_err(|error| {
            tracing::error!("[TAPP] Failed to refresh platform cache: {}", error);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Failed to refresh platform data" })),
            )
        })?;

    Ok(Json(PlatformItemResult {
        success: true,
        item_id,
        source: format!("tapp:{}", req.tapp_id),
    }))
}

#[derive(Debug, Deserialize)]
pub struct AddPlatformItemsBatchRequest {
    pub tapp_id: String,
    pub items: Vec<NewPlatformItem>,
}

/// POST /api/tapp/platform/items/batch
pub async fn add_platform_items_batch(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Json(req): Json<AddPlatformItemsBatchRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    runtime_grant.require_tapp_id(&req.tapp_id)?;
    runtime_grant.require(TappPermission::PlatformWrite)?;
    authorize_tapp_permission(&db, &claims, &req.tapp_id, TappPermission::PlatformWrite).await?;

    tracing::info!(
        "[TAPP] add_platform_items_batch - User: {}, Tapp: {}, Count: {}",
        claims.username,
        req.tapp_id,
        req.items.len()
    );

    if req.items.len() > 500 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Too many items in batch (max 500)" })),
        ));
    }

    for item in &req.items {
        validate_platform_name(&item.platform)
            .map_err(|e| (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))))?;
    }

    let mut grouped_items: HashMap<String, Vec<&NewPlatformItem>> = HashMap::new();
    for item in &req.items {
        grouped_items
            .entry(item.platform.to_lowercase())
            .or_default()
            .push(item);
    }

    let mut results = Vec::new();
    let cache_dir = std::path::Path::new("cache/platforms");
    tokio::fs::create_dir_all(cache_dir)
        .await
        .map_err(|error| {
            tracing::error!(
                "[TAPP] Failed to create platform cache directory: {}",
                error
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Failed to save platform data" })),
            )
        })?;

    for (platform, items) in grouped_items {
        let cache_file = cache_dir.join(format!("{}_filtered.json", platform));
        let _platform_guard = acquire_platform_lock(&platform)
            .await
            .map_err(|error| (StatusCode::BAD_REQUEST, Json(json!({ "error": error }))))?;
        let result_start = results.len();

        let mut data = match tokio::fs::read_to_string(&cache_file).await {
            Ok(content) => serde_json::from_str::<Value>(&content).map_err(|error| {
                tracing::error!(
                    "[TAPP] Invalid platform cache file for {}: {}",
                    platform,
                    error
                );
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": "Invalid platform cache data" })),
                )
            })?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => json!({ "items": [] }),
            Err(error) => {
                tracing::error!(
                    "[TAPP] Failed to read platform cache for {}: {}",
                    platform,
                    error
                );
                return Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": "Failed to read platform data" })),
                ));
            }
        };

        let data_items = data
            .as_object_mut()
            .and_then(|obj| obj.get_mut("items"))
            .and_then(|v| v.as_array_mut());

        if let Some(data_items) = data_items {
            for item in items {
                let item_id = format!("tapp_{}", uuid::Uuid::new_v4());

                let new_item = json!({
                    "id": item_id.clone(),
                    "type": item.item_type,
                    "title": item.title,
                    "cover": item.cover,
                    "description": item.description,
                    "url": item.url,
                    "metadata": item.metadata,
                    "createdAt": item.created_at.clone().unwrap_or_else(|| chrono::Utc::now().to_rfc3339()),
                    "source": format!("tapp:{}", req.tapp_id)
                });

                data_items.push(new_item);
                results.push(json!({
                    "success": true,
                    "itemId": item_id,
                    "source": format!("tapp:{}", req.tapp_id)
                }));
            }
        } else {
            for _ in items {
                results.push(json!({ "success": false, "error": "Invalid cache file structure" }));
            }
            continue;
        }

        // 原子写入：先写临时文件再重命名
        let tmp_file = cache_file.with_extension(format!("json.{}.tmp", uuid::Uuid::new_v4()));
        let content = serde_json::to_string_pretty(&data).unwrap();
        let write_ok = match tokio::fs::write(&tmp_file, &content).await {
            Ok(_) => match tokio::fs::rename(&tmp_file, &cache_file).await {
                Ok(_) => true,
                Err(e) => {
                    tracing::error!("[TAPP] Failed to rename cache file for {}: {}", platform, e);
                    let _ = tokio::fs::remove_file(&tmp_file).await;
                    false
                }
            },
            Err(e) => {
                tracing::error!(
                    "[TAPP] Failed to write temp cache file for {}: {}",
                    platform,
                    e
                );
                false
            }
        };
        if !write_ok {
            for result in &mut results[result_start..] {
                *result = json!({ "success": false, "error": "Failed to save platform data" });
            }
        } else if let Err(error) = update_cached_platform_data(&platform, data).await {
            tracing::error!(
                "[TAPP] Failed to refresh platform cache for {}: {}",
                platform,
                error
            );
            for result in &mut results[result_start..] {
                *result = json!({ "success": false, "error": "Failed to refresh platform data" });
            }
        }
    }

    let success_count = results
        .iter()
        .filter(|r| r.get("success").and_then(|v| v.as_bool()).unwrap_or(false))
        .count();

    Ok(Json(json!({
        "success": success_count == results.len(),
        "results": results,
        "totalProcessed": results.len(),
        "successCount": success_count
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_prefers_existing_items() {
        let data = json!({
            "items": [{ "id": "a1", "title": "Written Item", "type": "game" }],
            "raw_unknown_content": [{ "content_type": "Game", "title": "Other" }]
        });
        let items = extract_platform_items(&data, "steam");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["title"], "Written Item");
        assert_eq!(items[0]["platform"], "steam");
    }

    #[test]
    fn extract_projects_raw_unknown_content() {
        let data = json!({
            "platform": "steam",
            "raw_unknown_content": [
                {
                    "content_type": "Game",
                    "title": "Left 4 Dead 2",
                    "metadata": { "playtime": "3324" }
                },
                {
                    "content_type": "Game",
                    "title": "Hades",
                    "metadata": { "playtime": "120" }
                }
            ]
        });
        let items = extract_platform_items(&data, "steam");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["title"], "Left 4 Dead 2");
        assert_eq!(items[0]["type"], "Game");
        assert_eq!(items[0]["platform"], "steam");
        assert!(items[0].get("id").and_then(|v| v.as_str()).is_some());
        assert_eq!(items[0]["metadata"]["playtime"], "3324");
    }

    #[test]
    fn extract_projects_content_analysis_lists() {
        let data = json!({
            "platform": "github",
            "raw_unknown_content": [],
            "content_analysis": {
                "repo_summary": "has many repos",
                "recent_repos": [
                    {
                        "name": "Sakurairo",
                        "language": "PHP",
                        "stars": 4024,
                        "description": "A WordPress theme"
                    }
                ],
                "contribution_calendar": [
                    { "date": "2025-07-20", "count": 2 }
                ]
            }
        });
        let items = extract_platform_items(&data, "github");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["title"], "Sakurairo");
        assert_eq!(items[0]["description"], "A WordPress theme");
        assert_eq!(items[0]["metadata"]["stars"], 4024);
    }

    #[test]
    fn extract_dedupes_raw_and_analysis() {
        let data = json!({
            "raw_unknown_content": [
                { "content_type": "Game", "title": "Hades", "metadata": {} }
            ],
            "content_analysis": {
                "recent_games": [
                    { "name": "Hades", "playtime": 120 },
                    { "name": "Celeste", "playtime": 40 }
                ]
            }
        });
        let items = extract_platform_items(&data, "steam");
        // Hades appears once (from raw), Celeste from analysis
        assert_eq!(items.len(), 2);
        let titles: Vec<&str> = items
            .iter()
            .filter_map(|i| i.get("title").and_then(|v| v.as_str()))
            .collect();
        assert!(titles.contains(&"Hades"));
        assert!(titles.contains(&"Celeste"));
    }

    #[test]
    fn extract_covers_from_bangumi_subjects() {
        let data = json!({
            "content_analysis": {
                "top_rated_subjects": [
                    {
                        "subject_id": 19643,
                        "title": "星之卡比",
                        "subject_type": "game",
                        "rate": 10,
                        "cover": "https://example.com/cover.jpg"
                    }
                ]
            }
        });
        let items = extract_platform_items(&data, "bangumi");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["id"], "19643");
        assert_eq!(items[0]["image"], "https://example.com/cover.jpg");
        assert_eq!(items[0]["cover"], "https://example.com/cover.jpg");
        assert_eq!(items[0]["type"], "game");
    }
}
