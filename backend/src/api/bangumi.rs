// Bangumi API routes
use crate::error::HttpError;
use axum::{
    extract::{Path, Query},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};

use crate::services::fetcher::PlatformFetcher;

/// Reject client-supplied Bangumi tokens in the query string.
pub(crate) fn reject_query_access_token(access_token: &Option<String>) -> Result<(), HttpError> {
    if access_token.as_ref().is_some_and(|k| !k.trim().is_empty()) {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "success": false,
                "error": "access_token_not_allowed",
                "message": "Do not pass Bangumi access tokens in the query string; configure bangumi_access_token server-side"
            })),
        )));
    }
    Ok(())
}

async fn server_bangumi_token_and_ua(
    user_agent_override: Option<String>,
) -> Result<(Option<String>, Option<String>), HttpError> {
    let cfg = crate::GLOBAL_DYNAMIC_CONFIG.read().await;
    let token = cfg
        .bangumi_access_token
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let ua = user_agent_override
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            cfg.bangumi_user_agent
                .as_ref()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        });
    Ok((token, ua))
}

#[derive(Debug, Deserialize)]
pub struct BangumiQuery {
    pub username: Option<String>,
    pub access_token: Option<String>,
    pub user_agent: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct BangumiCollectionsQuery {
    pub access_token: Option<String>,
    pub user_agent: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BangumiUserResponse {
    pub user_info: serde_json::Value,
    pub collections: Vec<serde_json::Value>,
    pub total_collections: usize,
}

#[derive(Debug, Serialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub message: String,
}

fn clean_optional(input: Option<String>) -> Option<String> {
    input
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn value_username(value: &serde_json::Value) -> Option<String> {
    value
        .get("username")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

/// 获取 Bangumi 用户完整信息。
///
/// `username` 可选；缺省时需要 `access_token`，并通过 `/v0/me` 解析用户名。
pub async fn get_bangumi_user(
    Query(params): Query<BangumiQuery>,
) -> Result<Json<ApiResponse<BangumiUserResponse>>, HttpError> {
    reject_query_access_token(&params.access_token)?;
    let (server_token, server_ua) = server_bangumi_token_and_ua(params.user_agent.clone()).await?;
    let access_token = server_token;
    let user_agent = server_ua;

    let fetcher = PlatformFetcher::new().await;
    let username = clean_optional(params.username);
    let access_token = access_token; // server-side only
    let user_agent = user_agent;

    let user_result = if let Some(username) = username.as_deref() {
        fetcher
            .fetch_bangumi_user(username, access_token.as_deref(), user_agent.as_deref())
            .await
    } else if let Some(access_token) = access_token.as_deref() {
        fetcher
            .fetch_bangumi_me(access_token, user_agent.as_deref())
            .await
    } else {
        return Ok(Json(ApiResponse {
            success: false,
            data: None,
            message: "username 或 access_token 至少需要提供一个".to_string(),
        }));
    };

    let user_info = match user_result {
        Ok(info) => info,
        Err(e) => {
            tracing::error!("Failed to fetch Bangumi user: {}", e);
            return Ok(Json(ApiResponse {
                success: false,
                data: None,
                message: "Failed to fetch data".to_string(),
            }));
        }
    };

    let resolved_username = value_username(&user_info).or(username);
    let collections = if let Some(username) = resolved_username.as_deref() {
        match fetcher
            .fetch_bangumi_collections(username, access_token.as_deref(), user_agent.as_deref())
            .await
        {
            Ok(items) => items,
            Err(e) => {
                tracing::warn!(
                    "Failed to fetch Bangumi collections for {}: {}",
                    username,
                    e
                );
                Vec::new()
            }
        }
    } else {
        tracing::warn!("Bangumi user response did not include username; collections skipped");
        Vec::new()
    };

    let total_collections = collections.len();
    Ok(Json(ApiResponse {
        success: true,
        data: Some(BangumiUserResponse {
            user_info,
            collections,
            total_collections,
        }),
        message: format!("获取成功，共 {} 个收藏", total_collections),
    }))
}

/// 获取 Bangumi 用户基本信息
pub async fn get_bangumi_user_info(
    Path(username): Path<String>,
    Query(params): Query<BangumiCollectionsQuery>,
) -> Result<Json<ApiResponse<serde_json::Value>>, HttpError> {
    reject_query_access_token(&params.access_token)?;
    let (server_token, server_ua) = server_bangumi_token_and_ua(params.user_agent.clone()).await?;
    let access_token = server_token;
    let user_agent = server_ua;

    let fetcher = PlatformFetcher::new().await;
    let access_token = access_token; // server-side only
    let user_agent = user_agent;

    match fetcher
        .fetch_bangumi_user(&username, access_token.as_deref(), user_agent.as_deref())
        .await
    {
        Ok(info) => Ok(Json(ApiResponse {
            success: true,
            data: Some(info),
            message: "获取成功".to_string(),
        })),
        Err(e) => {
            tracing::error!("Failed to fetch Bangumi user {}: {}", username, e);
            Ok(Json(ApiResponse {
                success: false,
                data: None,
                message: "Failed to fetch data".to_string(),
            }))
        }
    }
}

/// 使用 access token 获取当前 Bangumi 用户信息
pub async fn get_bangumi_me(
    Query(params): Query<BangumiCollectionsQuery>,
) -> Result<Json<ApiResponse<serde_json::Value>>, HttpError> {
    reject_query_access_token(&params.access_token)?;
    let (server_token, server_ua) = server_bangumi_token_and_ua(params.user_agent.clone()).await?;
    let access_token = server_token;
    let user_agent = server_ua;

    let fetcher = PlatformFetcher::new().await;
    let access_token = match access_token {
        Some(token) => token,
        None => {
            return Ok(Json(ApiResponse {
                success: false,
                data: None,
                message: "Bangumi access token not configured".to_string(),
            }));
        }
    };
    let user_agent = user_agent;

    match fetcher
        .fetch_bangumi_me(&access_token, user_agent.as_deref())
        .await
    {
        Ok(info) => Ok(Json(ApiResponse {
            success: true,
            data: Some(info),
            message: "获取成功".to_string(),
        })),
        Err(e) => {
            tracing::error!("Failed to fetch Bangumi /me: {}", e);
            Ok(Json(ApiResponse {
                success: false,
                data: None,
                message: "Failed to fetch data".to_string(),
            }))
        }
    }
}

/// 获取 Bangumi 收藏列表
pub async fn get_bangumi_collections(
    Path(username): Path<String>,
    Query(params): Query<BangumiCollectionsQuery>,
) -> Result<Json<ApiResponse<Vec<serde_json::Value>>>, HttpError> {
    reject_query_access_token(&params.access_token)?;
    let (server_token, server_ua) = server_bangumi_token_and_ua(params.user_agent.clone()).await?;
    let access_token = server_token;
    let user_agent = server_ua;

    let fetcher = PlatformFetcher::new().await;
    let access_token = access_token; // server-side only
    let user_agent = user_agent;

    match fetcher
        .fetch_bangumi_collections(&username, access_token.as_deref(), user_agent.as_deref())
        .await
    {
        Ok(collections) => {
            let count = collections.len();
            Ok(Json(ApiResponse {
                success: true,
                data: Some(collections),
                message: format!("获取成功，共 {} 个收藏", count),
            }))
        }
        Err(e) => {
            tracing::error!(
                "Failed to fetch Bangumi collections for {}: {}",
                username,
                e
            );
            Ok(Json(ApiResponse {
                success: false,
                data: None,
                message: "Failed to fetch data".to_string(),
            }))
        }
    }
}

#[cfg(test)]
mod bangumi_secret_gate_tests {
    use super::*;
    use axum::response::IntoResponse;

    #[tokio::test]
    async fn reject_query_access_token_blocks_nonempty_client_token() {
        let err = reject_query_access_token(&Some("bgm_token_xyz".into())).unwrap_err();
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(resp.into_body(), 64 * 1024)
            .await
            .expect("body");
        let v: serde_json::Value = serde_json::from_slice(&body).expect("json");
        // HttpError/AppError body uses the `error` label field (not a success flag).
        assert_eq!(v["error"], "access_token_not_allowed");
        assert!(
            v.get("message")
                .and_then(|m| m.as_str())
                .is_some_and(|m| m.to_ascii_lowercase().contains("query")),
            "message should mention query restriction: {v}"
        );
    }

    #[test]
    fn reject_query_access_token_allows_absent_or_blank() {
        assert!(reject_query_access_token(&None).is_ok());
        assert!(reject_query_access_token(&Some(String::new())).is_ok());
        assert!(reject_query_access_token(&Some(" \t ".into())).is_ok());
    }
}
