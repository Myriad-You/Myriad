// Steam API routes
use axum::{
    extract::{Path, Query},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};

use crate::services::fetcher::PlatformFetcher;

#[derive(Debug, Deserialize)]
pub struct SteamQuery {
    pub steam_id: String,
    pub api_key: String,
}

#[derive(Debug, Serialize)]
pub struct SteamUserResponse {
    pub user_info: serde_json::Value,
    pub games: Vec<serde_json::Value>,
    pub wishlist: Vec<serde_json::Value>,
    pub total_games: usize,
    pub total_playtime: i32, // 总游戏时长（分钟）
}

#[derive(Debug, Serialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub message: String,
}

/// 获取 Steam 用户完整信息
pub async fn get_steam_user(
    Query(params): Query<SteamQuery>,
) -> Result<Json<ApiResponse<SteamUserResponse>>, StatusCode> {
    let fetcher = PlatformFetcher::new().await;
    let steam_id = params.steam_id;
    let api_key = params.api_key;

    // 获取用户信息
    let user_info = match fetcher.fetch_steam_user(&api_key, &steam_id).await {
        Ok(info) => serde_json::to_value(info).unwrap_or_default(),
        Err(e) => {
            tracing::error!("Failed to fetch Steam user {}: {}", steam_id, e);
            return Ok(Json(ApiResponse {
                success: false,
                data: None,
                message: format!("获取用户信息失败: {}", e),
            }));
        }
    };

    // 获取游戏库
    let (games_data, total_playtime) = match fetcher.fetch_steam_games(&api_key, &steam_id).await {
        Ok(games) => {
            let total_time: i32 = games.iter().map(|g| g.playtime_forever).sum();
            let games_json: Vec<serde_json::Value> = games
                .into_iter()
                .filter_map(|g| serde_json::to_value(g).ok())
                .collect();
            (games_json, total_time)
        }
        Err(e) => {
            tracing::warn!("Failed to fetch Steam games for {}: {}", steam_id, e);
            (Vec::new(), 0)
        }
    };

    // 获取愿望单
    let wishlist = match fetcher.fetch_steam_wishlist(&steam_id).await {
        Ok(items) => items
            .into_iter()
            .filter_map(|w| serde_json::to_value(w).ok())
            .collect(),
        Err(e) => {
            tracing::warn!("Failed to fetch Steam wishlist for {}: {}", steam_id, e);
            Vec::new()
        }
    };

    let total_games = games_data.len();

    Ok(Json(ApiResponse {
        success: true,
        data: Some(SteamUserResponse {
            user_info,
            games: games_data,
            wishlist,
            total_games,
            total_playtime,
        }),
        message: "获取成功".to_string(),
    }))
}

/// 获取 Steam 用户基本信息
pub async fn get_steam_user_info(
    Query(params): Query<SteamQuery>,
) -> Result<Json<ApiResponse<serde_json::Value>>, StatusCode> {
    let fetcher = PlatformFetcher::new().await;

    match fetcher
        .fetch_steam_user(&params.api_key, &params.steam_id)
        .await
    {
        Ok(info) => {
            let data = serde_json::to_value(info).unwrap_or_default();
            Ok(Json(ApiResponse {
                success: true,
                data: Some(data),
                message: "获取成功".to_string(),
            }))
        }
        Err(e) => {
            tracing::error!("Failed to fetch Steam user {}: {}", params.steam_id, e);
            Ok(Json(ApiResponse {
                success: false,
                data: None,
                message: format!("获取失败: {}", e),
            }))
        }
    }
}

/// 获取 Steam 游戏库
pub async fn get_steam_games(
    Query(params): Query<SteamQuery>,
) -> Result<Json<ApiResponse<SteamGamesResponse>>, StatusCode> {
    let fetcher = PlatformFetcher::new().await;

    match fetcher
        .fetch_steam_games(&params.api_key, &params.steam_id)
        .await
    {
        Ok(games) => {
            let total_playtime: i32 = games.iter().map(|g| g.playtime_forever).sum();
            let total_games = games.len();
            let games_data: Vec<serde_json::Value> = games
                .into_iter()
                .filter_map(|g| serde_json::to_value(g).ok())
                .collect();

            Ok(Json(ApiResponse {
                success: true,
                data: Some(SteamGamesResponse {
                    games: games_data,
                    total_games,
                    total_playtime,
                }),
                message: format!("获取成功，共 {} 个游戏", total_games),
            }))
        }
        Err(e) => {
            tracing::error!("Failed to fetch Steam games for {}: {}", params.steam_id, e);
            Ok(Json(ApiResponse {
                success: false,
                data: None,
                message: format!("获取失败: {}", e),
            }))
        }
    }
}

#[derive(Debug, Serialize)]
pub struct SteamGamesResponse {
    pub games: Vec<serde_json::Value>,
    pub total_games: usize,
    pub total_playtime: i32, // 分钟
}

/// 获取 Steam 愿望单
pub async fn get_steam_wishlist(
    Path(steam_id): Path<String>,
) -> Result<Json<ApiResponse<Vec<serde_json::Value>>>, StatusCode> {
    let fetcher = PlatformFetcher::new().await;

    match fetcher.fetch_steam_wishlist(&steam_id).await {
        Ok(wishlist) => {
            let data: Vec<serde_json::Value> = wishlist
                .into_iter()
                .filter_map(|w| serde_json::to_value(w).ok())
                .collect();

            let count = data.len();
            Ok(Json(ApiResponse {
                success: true,
                data: Some(data),
                message: format!("获取成功，共 {} 个游戏", count),
            }))
        }
        Err(e) => {
            tracing::error!("Failed to fetch Steam wishlist for {}: {}", steam_id, e);
            Ok(Json(ApiResponse {
                success: false,
                data: None,
                message: format!("获取失败: {}", e),
            }))
        }
    }
}

/// 获取游戏详细统计信息
#[derive(Debug, Serialize)]
pub struct GameStats {
    pub most_played: Vec<serde_json::Value>,
    pub recently_played: Vec<serde_json::Value>,
    pub total_hours: f32,
}

pub async fn get_steam_stats(
    Query(params): Query<SteamQuery>,
) -> Result<Json<ApiResponse<GameStats>>, StatusCode> {
    let fetcher = PlatformFetcher::new().await;

    match fetcher
        .fetch_steam_games(&params.api_key, &params.steam_id)
        .await
    {
        Ok(mut games) => {
            let total_minutes: i32 = games.iter().map(|g| g.playtime_forever).sum();
            let total_hours = total_minutes as f32 / 60.0;

            // 最多游玩的游戏（前10）
            games.sort_by(|a, b| b.playtime_forever.cmp(&a.playtime_forever));
            let most_played: Vec<serde_json::Value> = games
                .iter()
                .take(10)
                .filter_map(|g| serde_json::to_value(g).ok())
                .collect();

            // 最近游玩的游戏
            let mut recent_games = games.clone();
            recent_games.retain(|g| g.playtime_2weeks.is_some());
            recent_games.sort_by(|a, b| {
                b.playtime_2weeks
                    .unwrap_or(0)
                    .cmp(&a.playtime_2weeks.unwrap_or(0))
            });
            let recently_played: Vec<serde_json::Value> = recent_games
                .iter()
                .take(10)
                .filter_map(|g| serde_json::to_value(g).ok())
                .collect();

            Ok(Json(ApiResponse {
                success: true,
                data: Some(GameStats {
                    most_played,
                    recently_played,
                    total_hours,
                }),
                message: "获取成功".to_string(),
            }))
        }
        Err(e) => {
            tracing::error!("Failed to fetch Steam stats for {}: {}", params.steam_id, e);
            Ok(Json(ApiResponse {
                success: false,
                data: None,
                message: format!("获取失败: {}", e),
            }))
        }
    }
}

/// 获取单个 Steam 游戏详情
/// 通过 Steam Store API 获取游戏信息（名称、描述、价格等）
pub async fn get_steam_game_details(
    Path(app_id): Path<String>,
) -> Result<Json<ApiResponse<serde_json::Value>>, StatusCode> {
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap();

    let url = format!(
        "https://store.steampowered.com/api/appdetails?appids={}&l=schinese",
        app_id
    );

    match client.get(&url).send().await {
        Ok(resp) => {
            if !resp.status().is_success() {
                tracing::error!("Steam API returned status: {}", resp.status());
                return Ok(Json(ApiResponse {
                    success: false,
                    data: None,
                    message: format!("Steam API 返回错误状态: {}", resp.status()),
                }));
            }

            match resp.json::<serde_json::Value>().await {
                Ok(mut data) => {
                    // Steam API 返回格式: { "appid": { "success": true, "data": {...} } }
                    if let Some(app_data) = data.get_mut(&app_id) {
                        if let Some(success) = app_data.get("success").and_then(|v| v.as_bool()) {
                            if success {
                                if let Some(game_data) = app_data.get("data") {
                                    tracing::info!(
                                        "Successfully fetched Steam game details for app {}",
                                        app_id
                                    );
                                    return Ok(Json(ApiResponse {
                                        success: true,
                                        data: Some(game_data.clone()),
                                        message: "获取成功".to_string(),
                                    }));
                                }
                            }
                        }
                    }

                    // 如果没有找到游戏数据
                    tracing::warn!("No game data found for app {}", app_id);
                    Ok(Json(ApiResponse {
                        success: false,
                        data: None,
                        message: "未找到游戏信息".to_string(),
                    }))
                }
                Err(e) => {
                    tracing::error!("Failed to parse Steam API response for {}: {}", app_id, e);
                    Ok(Json(ApiResponse {
                        success: false,
                        data: None,
                        message: format!("解析响应失败: {}", e),
                    }))
                }
            }
        }
        Err(e) => {
            tracing::error!("Failed to fetch Steam game details for {}: {}", app_id, e);
            Ok(Json(ApiResponse {
                success: false,
                data: None,
                message: format!("请求失败: {}", e),
            }))
        }
    }
}
