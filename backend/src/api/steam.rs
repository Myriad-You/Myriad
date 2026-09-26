// Steam API routes
use crate::error::HttpError;
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;

use crate::services::fetcher::PlatformFetcher;
use crate::services::platform_id::PlatformId;
use crate::services::steam_presence::{
    PresenceHit, SteamPresenceResponse, fetch_and_store_presence, read_presence, stored,
    trigger_presence_refresh,
};

#[derive(Debug, Deserialize)]
pub struct SteamQuery {
    /// Optional override of configured steam_id (admin debug only).
    pub steam_id: Option<String>,
    /// Forbidden: must not appear in query (use server `steam_api_key`).
    #[serde(default)]
    pub api_key: Option<String>,
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

/// Reject client-supplied Steam API keys (must not travel in query/logs).
pub(crate) fn reject_query_api_key(api_key: &Option<String>) -> Result<(), HttpError> {
    if api_key.as_ref().is_some_and(|k| !k.trim().is_empty()) {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "success": false,
                "error": "api_key_not_allowed",
                "message": "Do not pass steam API keys in the query string; configure steam_api_key server-side"
            })),
        )));
    }
    Ok(())
}

async fn server_steam_credentials(
    steam_id_override: Option<String>,
) -> Result<(String, String), HttpError> {
    let cfg = crate::GLOBAL_DYNAMIC_CONFIG.read().await;
    let api_key = stored(cfg.steam_api_key.as_ref()).ok_or_else(|| {
        HttpError::from((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "success": false,
                "message": "Steam API key not configured"
            })),
        ))
    })?;
    let steam_id = stored(steam_id_override.as_ref())
        .or_else(|| stored(cfg.steam_id.as_ref()))
        .ok_or_else(|| {
            HttpError::from((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "success": false,
                    "message": "steam_id required (query or server config)"
                })),
            ))
        })?;
    Ok((api_key, steam_id))
}

/// 获取当前配置对应的 Steam 在线状态
pub async fn get_steam_presence(
    State(db): State<DatabaseConnection>,
) -> Result<Json<ApiResponse<SteamPresenceResponse>>, HttpError> {
    let config_service = crate::services::config_service::ConfigService::new(db);
    let config = config_service.load_config().await.ok().unwrap_or_default();

    // Same switch as reports / public cards / Agent.
    if PlatformId::Steam.explicit_enabled(&config) == Some(false) {
        return Ok(Json(ApiResponse {
            success: false,
            data: None,
            message: "Steam is not enabled".to_string(),
        }));
    }

    let (Some(api_key), Some(steam_id)) = (
        stored(config.steam_api_key.as_ref()),
        stored(config.steam_id.as_ref()),
    ) else {
        return Ok(Json(ApiResponse {
            success: false,
            data: None,
            message: "Steam is not configured".to_string(),
        }));
    };

    // stale-while-revalidate：Fresh/Stale 立刻返回；冷缓存同步拉一次
    match read_presence(&steam_id) {
        // 新鲜：直接返回
        Some(PresenceHit::Fresh(presence)) => {
            return Ok(Json(ApiResponse {
                success: true,
                data: Some(presence),
                message: "ok".to_string(),
            }));
        }
        // 陈旧：先返回旧值，后台异步刷新
        Some(PresenceHit::Stale(presence)) => {
            trigger_presence_refresh(api_key, steam_id);
            return Ok(Json(ApiResponse {
                success: true,
                data: Some(presence),
                message: "ok".to_string(),
            }));
        }
        // 冷缓存：只能同步拉一次
        None => {}
    }

    match fetch_and_store_presence(&api_key, &steam_id).await {
        Ok(presence) => Ok(Json(ApiResponse {
            success: true,
            data: Some(presence),
            message: "ok".to_string(),
        })),
        Err(e) => {
            tracing::warn!("Failed to fetch Steam presence for {}: {}", steam_id, e);
            Ok(Json(ApiResponse {
                success: false,
                data: None,
                message: "Failed to fetch data".to_string(),
            }))
        }
    }
}

/// 获取 Steam 用户完整信息
pub async fn get_steam_user(
    Query(params): Query<SteamQuery>,
) -> Result<Json<ApiResponse<SteamUserResponse>>, HttpError> {
    reject_query_api_key(&params.api_key)?;
    let (api_key, steam_id) = server_steam_credentials(params.steam_id).await?;
    let fetcher = PlatformFetcher::new().await;

    // 获取用户信息
    let user_info = match fetcher.fetch_steam_user(&api_key, &steam_id).await {
        Ok(info) => serde_json::to_value(info).unwrap_or_default(),
        Err(e) => {
            tracing::error!("Failed to fetch Steam user {}: {}", steam_id, e);
            return Ok(Json(ApiResponse {
                success: false,
                data: None,
                message: "Failed to fetch data".to_string(),
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
        message: "ok".to_string(),
    }))
}

/// 获取 Steam 用户基本信息
pub async fn get_steam_user_info(
    Query(params): Query<SteamQuery>,
) -> Result<Json<ApiResponse<serde_json::Value>>, HttpError> {
    reject_query_api_key(&params.api_key)?;
    let (api_key, steam_id) = server_steam_credentials(params.steam_id).await?;
    let fetcher = PlatformFetcher::new().await;

    match fetcher.fetch_steam_user(&api_key, &steam_id).await {
        Ok(info) => {
            let data = serde_json::to_value(info).unwrap_or_default();
            Ok(Json(ApiResponse {
                success: true,
                data: Some(data),
                message: "ok".to_string(),
            }))
        }
        Err(e) => {
            tracing::error!("Failed to fetch Steam user {}: {}", steam_id, e);
            Ok(Json(ApiResponse {
                success: false,
                data: None,
                message: "Failed to fetch data".to_string(),
            }))
        }
    }
}

/// 获取 Steam 游戏库
pub async fn get_steam_games(
    Query(params): Query<SteamQuery>,
) -> Result<Json<ApiResponse<SteamGamesResponse>>, HttpError> {
    reject_query_api_key(&params.api_key)?;
    let (api_key, steam_id) = server_steam_credentials(params.steam_id).await?;
    let fetcher = PlatformFetcher::new().await;

    match fetcher.fetch_steam_games(&api_key, &steam_id).await {
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
                message: "ok".to_string(),
            }))
        }
        Err(e) => {
            tracing::error!("Failed to fetch Steam games for {}: {}", steam_id, e);
            Ok(Json(ApiResponse {
                success: false,
                data: None,
                message: "Failed to fetch data".to_string(),
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
) -> Result<Json<ApiResponse<Vec<serde_json::Value>>>, HttpError> {
    let fetcher = PlatformFetcher::new().await;

    match fetcher.fetch_steam_wishlist(&steam_id).await {
        Ok(wishlist) => {
            let data: Vec<serde_json::Value> = wishlist
                .into_iter()
                .filter_map(|w| serde_json::to_value(w).ok())
                .collect();

            Ok(Json(ApiResponse {
                success: true,
                data: Some(data),
                message: "ok".to_string(),
            }))
        }
        Err(e) => {
            tracing::error!("Failed to fetch Steam wishlist for {}: {}", steam_id, e);
            Ok(Json(ApiResponse {
                success: false,
                data: None,
                message: "Failed to fetch data".to_string(),
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
) -> Result<Json<ApiResponse<GameStats>>, HttpError> {
    reject_query_api_key(&params.api_key)?;
    let (api_key, steam_id) = server_steam_credentials(params.steam_id).await?;
    let fetcher = PlatformFetcher::new().await;

    match fetcher.fetch_steam_games(&api_key, &steam_id).await {
        Ok(mut games) => {
            let total_minutes: i32 = games.iter().map(|g| g.playtime_forever).sum();
            let total_hours = total_minutes as f32 / 60.0;

            // 最多游玩的游戏（前10）
            games.sort_by_key(|b| Reverse(b.playtime_forever));
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
                message: "ok".to_string(),
            }))
        }
        Err(e) => {
            tracing::error!("Failed to fetch Steam stats for {}: {}", steam_id, e);
            Ok(Json(ApiResponse {
                success: false,
                data: None,
                message: "Failed to fetch data".to_string(),
            }))
        }
    }
}

/// Query override for Steam Store language (`l=` param).
#[derive(Debug, Deserialize, Default)]
pub struct SteamGameDetailsQuery {
    /// Explicit Steam language code (e.g. `english`, `schinese`, `tchinese`, `japanese`).
    pub lang: Option<String>,
    /// Alternate alias used by some clients.
    pub l: Option<String>,
}

/// Map Accept-Language / site locale tags to Steam Store `l=` codes.
fn steam_store_language(preferred: Option<&str>, accept_language: Option<&str>) -> &'static str {
    let raw = preferred
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| accept_language.map(str::trim).filter(|s| !s.is_empty()))
        .unwrap_or("en");
    let primary = raw
        .split(',')
        .next()
        .unwrap_or(raw)
        .split(';')
        .next()
        .unwrap_or(raw)
        .trim()
        .to_ascii_lowercase()
        .replace('_', "-");

    // Explicit Steam language codes
    const STEAM_CODES: &[&str] = &[
        "schinese",
        "tchinese",
        "english",
        "japanese",
        "koreana",
        "thai",
        "brazilian",
        "portuguese",
        "french",
        "german",
        "spanish",
        "latam",
        "italian",
        "russian",
        "polish",
        "danish",
        "dutch",
        "finnish",
        "norwegian",
        "swedish",
        "hungarian",
        "czech",
        "romanian",
        "turkish",
        "arabic",
        "ukrainian",
        "vietnamese",
    ];
    if let Some(&code) = STEAM_CODES.iter().find(|&&c| c == primary.as_str()) {
        return code;
    }

    // BCP-47 / browser tags
    if primary.starts_with("zh-tw")
        || primary.starts_with("zh-hk")
        || primary.starts_with("zh-hant")
        || primary.starts_with("zh-mo")
    {
        return "tchinese";
    }
    if primary.starts_with("zh") {
        return "schinese";
    }
    if primary.starts_with("ja") {
        return "japanese";
    }
    if primary.starts_with("ko") {
        return "koreana";
    }
    if primary.starts_with("pt-br") {
        return "brazilian";
    }
    if primary.starts_with("pt") {
        return "portuguese";
    }
    if primary.starts_with("es-419") || primary.starts_with("es-mx") || primary.starts_with("es-ar")
    {
        return "latam";
    }
    if primary.starts_with("es") {
        return "spanish";
    }
    if primary.starts_with("fr") {
        return "french";
    }
    if primary.starts_with("de") {
        return "german";
    }
    if primary.starts_with("it") {
        return "italian";
    }
    if primary.starts_with("ru") {
        return "russian";
    }
    if primary.starts_with("pl") {
        return "polish";
    }
    if primary.starts_with("th") {
        return "thai";
    }
    if primary.starts_with("tr") {
        return "turkish";
    }
    if primary.starts_with("vi") {
        return "vietnamese";
    }
    if primary.starts_with("uk") {
        return "ukrainian";
    }
    if primary.starts_with("ar") {
        return "arabic";
    }
    "english"
}

/// 获取单个 Steam 游戏详情
/// 通过 Steam Store API 获取游戏信息（名称、描述、价格等）
///
/// Language: `?lang=` / `?l=` override, else `Accept-Language`, default English.
pub async fn get_steam_game_details(
    Path(app_id): Path<String>,
    Query(query): Query<SteamGameDetailsQuery>,
    headers: axum::http::HeaderMap,
) -> Result<Json<ApiResponse<serde_json::Value>>, HttpError> {
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap();

    let accept_lang = headers
        .get(axum::http::header::ACCEPT_LANGUAGE)
        .and_then(|v| v.to_str().ok());
    let preferred = query.lang.as_deref().or(query.l.as_deref());
    let store_lang = steam_store_language(preferred, accept_lang);

    let url = format!(
        "https://store.steampowered.com/api/appdetails?appids={}&l={}",
        app_id, store_lang
    );

    match client.get(&url).send().await {
        Ok(resp) => {
            if !resp.status().is_success() {
                tracing::error!("Steam API returned status: {}", resp.status());
                return Ok(Json(ApiResponse {
                    success: false,
                    data: None,
                    message: "Failed to fetch data".to_string(),
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
                                        message: "ok".to_string(),
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
                        message: "Game not found".to_string(),
                    }))
                }
                Err(e) => {
                    tracing::error!("Failed to parse Steam API response for {}: {}", app_id, e);
                    Ok(Json(ApiResponse {
                        success: false,
                        data: None,
                        message: "Failed to fetch data".to_string(),
                    }))
                }
            }
        }
        Err(e) => {
            tracing::error!("Failed to fetch Steam game details for {}: {}", app_id, e);
            Ok(Json(ApiResponse {
                success: false,
                data: None,
                message: "Failed to fetch data".to_string(),
            }))
        }
    }
}

#[cfg(test)]
mod steam_secret_gate_tests {
    use super::*;
    use axum::response::IntoResponse;

    #[tokio::test]
    async fn reject_query_api_key_blocks_nonempty_client_key() {
        let err = reject_query_api_key(&Some("SK-secret".into())).unwrap_err();
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(resp.into_body(), 64 * 1024)
            .await
            .expect("body");
        let v: serde_json::Value = serde_json::from_slice(&body).expect("json");
        // HttpError/AppError body uses the `error` label field (not a success flag).
        assert_eq!(v["error"], "api_key_not_allowed");
        assert!(
            v.get("message")
                .and_then(|m| m.as_str())
                .is_some_and(|m| m.to_ascii_lowercase().contains("query")),
            "message should mention query restriction: {v}"
        );
    }

    #[test]
    fn reject_query_api_key_allows_absent_or_blank() {
        assert!(reject_query_api_key(&None).is_ok());
        assert!(reject_query_api_key(&Some(String::new())).is_ok());
        assert!(reject_query_api_key(&Some("   ".into())).is_ok());
    }
}

#[cfg(test)]
mod credential_source_tests {
    use super::stored;
    use crate::config::DynamicConfig;
    use crate::services::platform_id::PlatformId;

    /// Presence / Steam proxy credentials are the fetch arm's: stored values
    /// only, blank = missing. No env fallback.
    #[test]
    fn steam_credentials_follow_platform_id() {
        let mut config = DynamicConfig::default();
        let pair = |c: &DynamicConfig| {
            stored(c.steam_api_key.as_ref()).is_some() && stored(c.steam_id.as_ref()).is_some()
        };
        crate::services::platform_id::tests::with_env(
            &[("STEAM_API_KEY", "key"), ("STEAM_ID", "7656")],
            || {
                for (api_key, steam_id) in [
                    (None, None),
                    (Some(" "), Some("7656")),
                    (Some("key"), None),
                    (Some("key"), Some("7656")),
                ] {
                    config.steam_api_key = api_key.map(str::to_string);
                    config.steam_id = steam_id.map(str::to_string);
                    assert_eq!(
                        pair(&config),
                        PlatformId::Steam.credentials_present(&config),
                        "{api_key:?} {steam_id:?}"
                    );
                }
            },
        );
        let src = include_str!("steam.rs");
        let body = src
            .split("mod credential_source_tests")
            .next()
            .expect("body");
        assert!(!body.contains("env::var(\"STEAM_"));
    }
}
