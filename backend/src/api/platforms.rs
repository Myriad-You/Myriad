use axum::{extract::State, http::StatusCode, Json};
use sea_orm::{DatabaseConnection, EntityTrait, QueryOrder};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::Path;

use crate::db::schema_check::{default_platform_seeds, DefaultPlatformSeed};
use crate::models::entities::platforms;

/// 平台描述映射（数据库不存储描述，这里提供默认描述）
fn get_platform_description(name: &str) -> &'static str {
    match name {
        "github" => "Aggregate your repositories, stars, and contributions",
        "bilibili" => "Track your favorites, bangumi, and viewing history",
        "steam" => "Sync your game library and wishlist",
        "netease" | "netease_music" => "Analyze your music taste and playlists",
        "bangumi" => "Sync your Bangumi collection, ratings, and watching status",
        "x" => "Sync your X profile and posts; share via Web Intent",
        "discord" => "Sync your Discord profile, server footprint, and linked accounts",
        "mal" => "Username required; optional Client ID uses official API (else public load.json)",
        "xbox" => "Sync your Xbox achievements, Gamerscore, and recently played titles",
        "psn" => "Sync your PSN trophies, trophy level, and recently played titles",
        _ => "Connect and sync your data",
    }
}

/// Catalog / seed name → smart-filter cache key used by `getData`
/// (`cache/platforms/{slug}_filtered.json`).
///
/// Seeds store `netease_music` but smart_filter writes `netease_filtered.json`.
fn cache_slug_for_platform(name: &str) -> &str {
    match name {
        "netease_music" => "netease",
        other => other,
    }
}

/// Platforms that currently have library data on disk (filtered cache present).
/// This is the source of truth for Tapp `listEnabled` — the catalog `enabled`
/// column is only a seed default and is not kept in sync with config toggles.
fn platforms_with_library_cache() -> HashSet<String> {
    let cache_dir = Path::new("cache/platforms");
    let mut out = HashSet::new();
    let Ok(entries) = std::fs::read_dir(cache_dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if let Some(slug) = name.strip_suffix("_filtered.json") {
            if !slug.is_empty() {
                out.insert(slug.to_ascii_lowercase());
            }
        }
    }
    out
}

fn platform_json(id: i32, catalog_name: &str, display_name: &str, icon: &str, catalog_enabled: bool, with_data: &HashSet<String>) -> Value {
    // Prefer cache-compatible slug so `platform.getData(id)` resolves files.
    let slug = cache_slug_for_platform(catalog_name);
    let has_library = with_data.contains(&slug.to_ascii_lowercase())
        || with_data.contains(&catalog_name.to_ascii_lowercase());
    // Tapp SDK listEnabled filters on `enabled`. Catalog seed only marks GitHub
    // enabled; real library presence is the filtered cache. Expose either.
    let enabled = catalog_enabled || has_library;
    json!({
        "id": id,
        "name": display_name,
        "slug": slug,
        "key": slug,
        "enabled": enabled,
        "icon": icon,
        "description": get_platform_description(catalog_name),
        "hasLibraryData": has_library,
    })
}

fn platform_json_from_seed(seed: &DefaultPlatformSeed, id: i32, with_data: &HashSet<String>) -> Value {
    // Keep `id` = numeric PK and `name` = display label for host UI compatibility.
    // Additive `slug`/`key` = stable platform key for cache paths / Tapp SDK.
    platform_json(
        id,
        seed.name,
        seed.display_name,
        seed.icon,
        seed.enabled,
        with_data,
    )
}

fn platform_json_from_row(p: &platforms::Model, with_data: &HashSet<String>) -> Value {
    let icon = p.icon.as_ref().unwrap_or(&p.name);
    platform_json(
        p.id,
        &p.name,
        &p.display_name,
        icon,
        p.enabled.unwrap_or(false),
        with_data,
    )
}

/// 用种子目录补齐 DB 中缺失的平台（仅内存响应，不写库；写库由 schema_check 负责）
fn merge_missing_seed_platforms(
    mut platforms: Vec<Value>,
    present_names: &[String],
    with_data: &HashSet<String>,
) -> Vec<Value> {
    let mut next_id = platforms
        .iter()
        .filter_map(|p| p.get("id").and_then(|v| v.as_i64()))
        .max()
        .unwrap_or(0) as i32
        + 1;

    for seed in default_platform_seeds() {
        if present_names.iter().any(|n| n == seed.name) {
            continue;
        }
        platforms.push(platform_json_from_seed(seed, next_id, with_data));
        next_id += 1;
    }

    platforms
}

/// Append cache-only platforms not present in catalog/seeds (defensive).
fn merge_cache_only_platforms(mut platforms: Vec<Value>, with_data: &HashSet<String>) -> Vec<Value> {
    let mut known: HashSet<String> = HashSet::new();
    for p in &platforms {
        if let Some(slug) = p.get("slug").and_then(|v| v.as_str()) {
            known.insert(slug.to_ascii_lowercase());
        }
        if let Some(key) = p.get("key").and_then(|v| v.as_str()) {
            known.insert(key.to_ascii_lowercase());
        }
    }
    // Catalog netease_music maps to netease — treat as known.
    if known.contains("netease_music") {
        known.insert("netease".to_string());
    }

    let next_id = platforms
        .iter()
        .filter_map(|p| p.get("id").and_then(|v| v.as_i64()))
        .max()
        .unwrap_or(0) as i32
        + 1;

    let mut missing: Vec<String> = with_data
        .iter()
        .filter(|slug| !known.contains(slug.as_str()))
        .cloned()
        .collect();
    missing.sort();

    for (offset, slug) in missing.into_iter().enumerate() {
        let display = match slug.as_str() {
            "netease" => "Netease Music",
            "github" => "GitHub",
            "bilibili" => "Bilibili",
            "steam" => "Steam",
            "bangumi" => "Bangumi",
            "mal" => "MyAnimeList",
            "xbox" => "Xbox",
            "psn" => "PlayStation",
            "x" => "X",
            "discord" => "Discord",
            other => other,
        };
        platforms.push(platform_json(
            next_id + offset as i32,
            &slug,
            display,
            &slug,
            false,
            with_data,
        ));
    }

    platforms
}

pub async fn list_platforms(State(db): State<DatabaseConnection>) -> (StatusCode, Json<Value>) {
    let with_data = platforms_with_library_cache();

    // 从数据库读取平台列表
    match platforms::Entity::find()
        .order_by_asc(platforms::Column::Id)
        .all(&db)
        .await
    {
        Ok(platform_list) => {
            let present_names: Vec<String> = platform_list.iter().map(|p| p.name.clone()).collect();

            let mut platforms: Vec<Value> = platform_list
                .iter()
                .map(|p| platform_json_from_row(p, &with_data))
                .collect();

            // 兼容旧库尚未跑 seed 同步的情况：响应里补齐缺失平台
            platforms = merge_missing_seed_platforms(platforms, &present_names, &with_data);
            platforms = merge_cache_only_platforms(platforms, &with_data);

            (StatusCode::OK, Json(json!({ "platforms": platforms })))
        }
        Err(e) => {
            tracing::error!("Failed to fetch platforms: {}", e);
            // 降级到种子目录硬编码数据
            let mut platforms: Vec<Value> = default_platform_seeds()
                .iter()
                .enumerate()
                .map(|(i, seed)| platform_json_from_seed(seed, (i + 1) as i32, &with_data))
                .collect();
            platforms = merge_cache_only_platforms(platforms, &with_data);
            (StatusCode::OK, Json(json!({ "platforms": platforms })))
        }
    }
}

pub async fn get_profiles(State(_db): State<DatabaseConnection>) -> (StatusCode, Json<Value>) {
    // TODO: Fetch from database
    (
        StatusCode::OK,
        Json(json!({
            "profiles": [],
            "message": "No profiles fetched yet"
        })),
    )
}

pub async fn trigger_fetch(State(_db): State<DatabaseConnection>) -> (StatusCode, Json<Value>) {
    // TODO: Implement fetch logic
    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "message": "Fetch triggered successfully"
        })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_slug_aliases_netease() {
        assert_eq!(cache_slug_for_platform("netease_music"), "netease");
        assert_eq!(cache_slug_for_platform("steam"), "steam");
    }

    #[test]
    fn platform_json_enables_when_library_cache_present() {
        let mut with_data = HashSet::new();
        with_data.insert("steam".to_string());
        with_data.insert("bangumi".to_string());

        let steam = platform_json(1, "steam", "Steam", "steam", false, &with_data);
        assert_eq!(steam["enabled"], true);
        assert_eq!(steam["hasLibraryData"], true);
        assert_eq!(steam["slug"], "steam");

        let github = platform_json(2, "github", "GitHub", "github", true, &with_data);
        assert_eq!(github["enabled"], true);
        assert_eq!(github["hasLibraryData"], false);

        let psn = platform_json(3, "psn", "PlayStation", "psn", false, &with_data);
        assert_eq!(psn["enabled"], false);
        assert_eq!(psn["hasLibraryData"], false);

        let netease = platform_json(
            4,
            "netease_music",
            "Netease Music",
            "netease",
            false,
            &with_data,
        );
        // no netease in with_data yet
        assert_eq!(netease["enabled"], false);
        assert_eq!(netease["slug"], "netease");

        with_data.insert("netease".to_string());
        let netease2 = platform_json(
            4,
            "netease_music",
            "Netease Music",
            "netease",
            false,
            &with_data,
        );
        assert_eq!(netease2["enabled"], true);
        assert_eq!(netease2["slug"], "netease");
        assert_eq!(netease2["key"], "netease");
    }
}
