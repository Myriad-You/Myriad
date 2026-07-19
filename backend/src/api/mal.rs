// MyAnimeList API routes — public load.json, username only (no Client ID)
use axum::{
    extract::{Path, Query},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};

use crate::services::fetcher::PlatformFetcher;

#[derive(Debug, Deserialize)]
pub struct MalQuery {
    pub username: String,
}

#[derive(Debug, Serialize)]
pub struct MalUserResponse {
    pub user: serde_json::Value,
    pub anime_list: Vec<serde_json::Value>,
    pub manga_list: Vec<serde_json::Value>,
    pub total_anime: usize,
    pub total_manga: usize,
}

#[derive(Debug, Serialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub message: String,
}

fn clean(input: &str) -> &str {
    input.trim()
}

/// 获取 MyAnimeList 用户完整信息（资料 + 动画/漫画公开列表）
pub async fn get_mal_user(
    Query(params): Query<MalQuery>,
) -> Result<Json<ApiResponse<MalUserResponse>>, StatusCode> {
    let username = clean(&params.username);

    if username.is_empty() {
        return Ok(Json(ApiResponse {
            success: false,
            data: None,
            message: "username 为必填".to_string(),
        }));
    }

    let fetcher = PlatformFetcher::new().await;
    match fetcher.fetch_mal_profile_bundle(username).await {
        Ok(bundle) => {
            let anime_list = bundle
                .get("anime_list")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let manga_list = bundle
                .get("manga_list")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let user = bundle.get("user").cloned().unwrap_or_default();
            let display = user
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or(username)
                .to_string();

            Ok(Json(ApiResponse {
                success: true,
                data: Some(MalUserResponse {
                    total_anime: anime_list.len(),
                    total_manga: manga_list.len(),
                    anime_list,
                    manga_list,
                    user,
                }),
                message: format!("✓ MAL user '{}' verified ({})", username, display),
            }))
        }
        Err(e) => {
            tracing::error!("Failed to fetch MAL user {}: {}", username, e);
            Ok(Json(ApiResponse {
                success: false,
                data: None,
                message: format!("获取 MyAnimeList 用户失败: {}", e),
            }))
        }
    }
}

/// 仅验证用户名是否可访问公开列表
pub async fn get_mal_user_info(
    Path(username): Path<String>,
) -> Result<Json<ApiResponse<serde_json::Value>>, StatusCode> {
    let username = clean(&username);

    if username.is_empty() {
        return Ok(Json(ApiResponse {
            success: false,
            data: None,
            message: "username 为必填".to_string(),
        }));
    }

    let fetcher = PlatformFetcher::new().await;
    match fetcher.fetch_mal_user(username).await {
        Ok(user) => Ok(Json(ApiResponse {
            success: true,
            data: Some(user),
            message: "ok".to_string(),
        })),
        Err(e) => Ok(Json(ApiResponse {
            success: false,
            data: None,
            message: format!("验证失败: {}", e),
        })),
    }
}

/// 获取动画列表
pub async fn get_mal_anime_list(
    Path(username): Path<String>,
) -> Result<Json<ApiResponse<Vec<serde_json::Value>>>, StatusCode> {
    let username = clean(&username);

    if username.is_empty() {
        return Ok(Json(ApiResponse {
            success: false,
            data: None,
            message: "username 为必填".to_string(),
        }));
    }

    let fetcher = PlatformFetcher::new().await;
    match fetcher.fetch_mal_anime_list(username).await {
        Ok(list) => {
            let count = list.len();
            Ok(Json(ApiResponse {
                success: true,
                data: Some(list),
                message: format!("获取成功，共 {} 部动画", count),
            }))
        }
        Err(e) => {
            tracing::error!("Failed to fetch MAL anime list for {}: {}", username, e);
            Ok(Json(ApiResponse {
                success: false,
                data: None,
                message: format!("获取失败: {}", e),
            }))
        }
    }
}

/// 获取漫画列表
pub async fn get_mal_manga_list(
    Path(username): Path<String>,
) -> Result<Json<ApiResponse<Vec<serde_json::Value>>>, StatusCode> {
    let username = clean(&username);

    if username.is_empty() {
        return Ok(Json(ApiResponse {
            success: false,
            data: None,
            message: "username 为必填".to_string(),
        }));
    }

    let fetcher = PlatformFetcher::new().await;
    match fetcher.fetch_mal_manga_list(username).await {
        Ok(list) => {
            let count = list.len();
            Ok(Json(ApiResponse {
                success: true,
                data: Some(list),
                message: format!("获取成功，共 {} 部漫画", count),
            }))
        }
        Err(e) => {
            tracing::error!("Failed to fetch MAL manga list for {}: {}", username, e);
            Ok(Json(ApiResponse {
                success: false,
                data: None,
                message: format!("获取失败: {}", e),
            }))
        }
    }
}
