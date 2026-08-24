// Bilibili API routes
use crate::error::HttpError;
use axum::{
    extract::{Path, Query},
    Json,
};
use serde::{Deserialize, Serialize};

use crate::services::fetcher::PlatformFetcher;

#[derive(Debug, Deserialize)]
pub struct BilibiliQuery {
    pub uid: i64,
}

#[derive(Debug, Serialize)]
pub struct BilibiliUserResponse {
    pub user_info: serde_json::Value,
    pub favorites: Vec<serde_json::Value>,
    pub bangumi: Vec<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub message: String,
}

/// 获取 Bilibili 用户完整信息
pub async fn get_bilibili_user(
    Query(params): Query<BilibiliQuery>,
) -> Result<Json<ApiResponse<BilibiliUserResponse>>, HttpError> {
    let fetcher = PlatformFetcher::new().await;
    let uid = params.uid;

    // 获取用户信息
    let user_info = match fetcher.fetch_bilibili_user(uid).await {
        Ok(info) => serde_json::to_value(info).unwrap_or_default(),
        Err(e) => {
            tracing::error!("Failed to fetch Bilibili user {}: {}", uid, e);
            return Ok(Json(ApiResponse {
                success: false,
                data: None,
                message: "Failed to fetch data".to_string(),
            }));
        }
    };

    // 获取收藏夹
    let favorites = match fetcher.fetch_bilibili_favorites(uid).await {
        Ok(favs) => favs
            .into_iter()
            .filter_map(|f| serde_json::to_value(f).ok())
            .collect(),
        Err(e) => {
            tracing::warn!("Failed to fetch Bilibili favorites for {}: {}", uid, e);
            Vec::new()
        }
    };

    // 获取追番/追剧
    let bangumi = match fetcher.fetch_all_bilibili_bangumi(uid).await {
        Ok(items) => items
            .into_iter()
            .filter_map(|b| serde_json::to_value(b).ok())
            .collect(),
        Err(e) => {
            tracing::warn!("Failed to fetch Bilibili bangumi for {}: {}", uid, e);
            Vec::new()
        }
    };

    Ok(Json(ApiResponse {
        success: true,
        data: Some(BilibiliUserResponse {
            user_info,
            favorites,
            bangumi,
        }),
        message: "获取成功".to_string(),
    }))
}

/// 获取 Bilibili 用户基本信息
pub async fn get_bilibili_user_info(
    Path(uid): Path<i64>,
) -> Result<Json<ApiResponse<serde_json::Value>>, HttpError> {
    let fetcher = PlatformFetcher::new().await;

    match fetcher.fetch_bilibili_user(uid).await {
        Ok(info) => {
            let data = serde_json::to_value(info).unwrap_or_default();
            Ok(Json(ApiResponse {
                success: true,
                data: Some(data),
                message: "获取成功".to_string(),
            }))
        }
        Err(e) => {
            tracing::error!("Failed to fetch Bilibili user {}: {}", uid, e);
            Ok(Json(ApiResponse {
                success: false,
                data: None,
                message: "Failed to fetch data".to_string(),
            }))
        }
    }
}

/// 获取 Bilibili 收藏夹
pub async fn get_bilibili_favorites(
    Path(uid): Path<i64>,
) -> Result<Json<ApiResponse<Vec<serde_json::Value>>>, HttpError> {
    let fetcher = PlatformFetcher::new().await;

    match fetcher.fetch_bilibili_favorites(uid).await {
        Ok(favorites) => {
            let data: Vec<serde_json::Value> = favorites
                .into_iter()
                .filter_map(|f| serde_json::to_value(f).ok())
                .collect();

            let count = data.len();
            Ok(Json(ApiResponse {
                success: true,
                data: Some(data),
                message: format!("获取成功，共 {} 个收藏夹", count),
            }))
        }
        Err(e) => {
            tracing::error!("Failed to fetch Bilibili favorites for {}: {}", uid, e);
            Ok(Json(ApiResponse {
                success: false,
                data: None,
                message: "Failed to fetch data".to_string(),
            }))
        }
    }
}

/// 获取 Bilibili 追番列表
pub async fn get_bilibili_bangumi(
    Path(uid): Path<i64>,
    Query(params): Query<BanguminQuery>,
) -> Result<Json<ApiResponse<Vec<serde_json::Value>>>, HttpError> {
    let fetcher = PlatformFetcher::new().await;
    let bangumi_type = params.bangumi_type.unwrap_or(1); // 默认获取动画

    match fetcher.fetch_bilibili_bangumi(uid, bangumi_type).await {
        Ok(bangumi) => {
            let data: Vec<serde_json::Value> = bangumi
                .into_iter()
                .filter_map(|b| serde_json::to_value(b).ok())
                .collect();

            let type_name = match bangumi_type {
                1 => "动画",
                2 => "电影",
                3 => "纪录片",
                4 => "国创",
                5 => "电视剧",
                _ => "其他",
            };

            let count = data.len();
            Ok(Json(ApiResponse {
                success: true,
                data: Some(data),
                message: format!("获取成功，共 {} 个{}", count, type_name),
            }))
        }
        Err(e) => {
            tracing::error!("Failed to fetch Bilibili bangumi for {}: {}", uid, e);
            Ok(Json(ApiResponse {
                success: false,
                data: None,
                message: "Failed to fetch data".to_string(),
            }))
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct BanguminQuery {
    pub bangumi_type: Option<i32>, // 1: 动画, 2: 电影, 3: 纪录片, 4: 国创, 5: 电视剧
}

/// 获取所有 Bilibili 追番/追剧
pub async fn get_all_bilibili_bangumi(
    Path(uid): Path<i64>,
) -> Result<Json<ApiResponse<Vec<serde_json::Value>>>, HttpError> {
    let fetcher = PlatformFetcher::new().await;

    match fetcher.fetch_all_bilibili_bangumi(uid).await {
        Ok(bangumi) => {
            let data: Vec<serde_json::Value> = bangumi
                .into_iter()
                .filter_map(|b| serde_json::to_value(b).ok())
                .collect();

            let count = data.len();
            Ok(Json(ApiResponse {
                success: true,
                data: Some(data),
                message: format!("获取成功，共 {} 项", count),
            }))
        }
        Err(e) => {
            tracing::error!("Failed to fetch all Bilibili bangumi for {}: {}", uid, e);
            Ok(Json(ApiResponse {
                success: false,
                data: None,
                message: "Failed to fetch data".to_string(),
            }))
        }
    }
}
