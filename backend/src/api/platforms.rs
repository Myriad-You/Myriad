use axum::{extract::State, http::StatusCode, Json};
use sea_orm::{DatabaseConnection, EntityTrait, QueryOrder};
use serde_json::{json, Value};

use crate::db::schema_check::{default_platform_seeds, DefaultPlatformSeed};
use crate::models::entities::platforms;

/// 平台描述映射（数据库不存储描述，这里提供默认描述）
fn get_platform_description(name: &str) -> &'static str {
    match name {
        "github" => "Aggregate your repositories, stars, and contributions",
        "bilibili" => "Track your favorites, bangumi, and viewing history",
        "steam" => "Sync your game library and wishlist",
        "netease_music" => "Analyze your music taste and playlists",
        "bangumi" => "Sync your Bangumi collection, ratings, and watching status",
        "x" => "Sync your X profile and posts; share via Web Intent",
        "discord" => "Sync your Discord profile, server footprint, and linked accounts",
        "mal" => "Sync your public MyAnimeList anime/manga lists by username (no API key)",
        "xbox" => "Sync your Xbox achievements, Gamerscore, and recently played titles",
        "psn" => "Sync your PSN trophies, trophy level, and recently played titles",
        _ => "Connect and sync your data",
    }
}

fn platform_json_from_seed(seed: &DefaultPlatformSeed, id: i32) -> Value {
    json!({
        "id": id,
        "name": seed.display_name,
        "enabled": seed.enabled,
        "icon": seed.icon,
        "description": get_platform_description(seed.name),
    })
}

fn platform_json_from_row(p: &platforms::Model) -> Value {
    json!({
        "id": p.id,
        "name": p.display_name,
        "enabled": p.enabled.unwrap_or(false),
        "icon": p.icon.as_ref().unwrap_or(&p.name),
        "description": get_platform_description(&p.name),
    })
}

/// 用种子目录补齐 DB 中缺失的平台（仅内存响应，不写库；写库由 schema_check 负责）
fn merge_missing_seed_platforms(mut platforms: Vec<Value>, present_names: &[String]) -> Vec<Value> {
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
        platforms.push(platform_json_from_seed(seed, next_id));
        next_id += 1;
    }

    platforms
}

pub async fn list_platforms(State(db): State<DatabaseConnection>) -> (StatusCode, Json<Value>) {
    // 从数据库读取平台列表
    match platforms::Entity::find()
        .order_by_asc(platforms::Column::Id)
        .all(&db)
        .await
    {
        Ok(platform_list) => {
            let present_names: Vec<String> = platform_list.iter().map(|p| p.name.clone()).collect();

            let mut platforms: Vec<Value> =
                platform_list.iter().map(platform_json_from_row).collect();

            // 兼容旧库尚未跑 seed 同步的情况：响应里补齐缺失平台
            platforms = merge_missing_seed_platforms(platforms, &present_names);

            (StatusCode::OK, Json(json!({ "platforms": platforms })))
        }
        Err(e) => {
            tracing::error!("Failed to fetch platforms: {}", e);
            // 降级到种子目录硬编码数据
            let platforms: Vec<Value> = default_platform_seeds()
                .iter()
                .enumerate()
                .map(|(i, seed)| platform_json_from_seed(seed, (i + 1) as i32))
                .collect();
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
