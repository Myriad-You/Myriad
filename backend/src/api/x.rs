// X (Twitter) API routes — 读数据 + Intent 分享（不走 OAuth / 不代发帖）
use crate::error::HttpError;
use axum::{extract::Query, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::services::fetcher::{
    build_x_intent_url, compose_x_share_text, PlatformFetcher, X_SHARE_DEFAULT_MAX_LEN,
};

/// Query for X debug/read endpoints.
///
/// `bearer_token` must **not** be supplied (logs/Referer leak). Handlers use
/// server-stored `x_bearer_token`. Optional `username` overrides `x_username`.
#[derive(Debug, Deserialize, Default)]
pub struct XQuery {
    pub username: Option<String>,
    /// Forbidden in query — configure `x_bearer_token` server-side.
    pub bearer_token: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct XUserResponse {
    pub user: serde_json::Value,
    pub tweets: Vec<serde_json::Value>,
    pub total_tweets: usize,
    pub following: Vec<serde_json::Value>,
    pub total_following: usize,
}

#[derive(Debug, Serialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub message: String,
}

/// Reject client-supplied X bearer tokens in the query string.
pub(crate) fn reject_query_bearer_token(bearer_token: &Option<String>) -> Result<(), HttpError> {
    if bearer_token.as_ref().is_some_and(|k| !k.trim().is_empty()) {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "success": false,
                "error": "bearer_token_not_allowed",
                "message": "Do not pass X bearer tokens in the query string; configure x_bearer_token server-side"
            })),
        )));
    }
    Ok(())
}

/// Server credentials: optional username query override + required server bearer.
async fn server_x_credentials(
    username_override: Option<String>,
) -> Result<(String, String), HttpError> {
    let cfg = crate::GLOBAL_DYNAMIC_CONFIG.read().await;
    let bearer = cfg
        .x_bearer_token
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            HttpError::from((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "success": false,
                    "error": "x_bearer_not_configured",
                    "message": "X bearer token not configured. Set x_bearer_token in platform settings."
                })),
            ))
        })?;

    let username = username_override
        .map(|s| s.trim().trim_start_matches('@').to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            cfg.x_username
                .as_ref()
                .map(|s| s.trim().trim_start_matches('@').to_string())
                .filter(|s| !s.is_empty())
        })
        .ok_or_else(|| {
            HttpError::from((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "success": false,
                    "error": "x_username_required",
                    "message": "username required (query or server x_username config)"
                })),
            ))
        })?;

    Ok((username, bearer))
}

/// 获取 X 用户完整信息（资料 + 时间线）
///
/// Bearer 仅来自服务端配置；query `bearer_token` 一律 400。
pub async fn get_x_user(
    Query(params): Query<XQuery>,
) -> Result<Json<ApiResponse<XUserResponse>>, HttpError> {
    reject_query_bearer_token(&params.bearer_token)?;
    let (username, bearer_token) = server_x_credentials(params.username).await?;

    let fetcher = PlatformFetcher::new().await;
    match fetcher
        .fetch_x_profile_bundle(&username, &bearer_token)
        .await
    {
        Ok(bundle) => {
            let tweets = bundle
                .get("tweets")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let following = bundle
                .get("following")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let user = bundle.get("user").cloned().unwrap_or_default();
            let display = user
                .get("name")
                .and_then(|v| v.as_str())
                .or_else(|| user.get("username").and_then(|v| v.as_str()))
                .unwrap_or(username.as_str())
                .to_string();

            Ok(Json(ApiResponse {
                success: true,
                data: Some(XUserResponse {
                    total_tweets: tweets.len(),
                    tweets,
                    total_following: following.len(),
                    following,
                    user,
                }),
                message: format!("✓ X user @{} verified ({})", username, display),
            }))
        }
        Err(e) => {
            tracing::error!("Failed to fetch X user @{}: {}", username, e);
            Ok(Json(ApiResponse {
                success: false,
                data: None,
                message: "Failed to fetch data".to_string(),
            }))
        }
    }
}

/// 仅验证服务端配置的用户名 + Bearer 是否有效
pub async fn get_x_user_info(
    Query(params): Query<XQuery>,
) -> Result<Json<ApiResponse<serde_json::Value>>, HttpError> {
    reject_query_bearer_token(&params.bearer_token)?;
    let (username, bearer_token) = server_x_credentials(params.username).await?;

    let fetcher = PlatformFetcher::new().await;
    match fetcher
        .fetch_x_user_by_username(&username, &bearer_token)
        .await
    {
        Ok(user) => Ok(Json(ApiResponse {
            success: true,
            data: Some(user),
            message: "ok".to_string(),
        })),
        Err(e) => {
            tracing::error!("Failed to verify X user @{}: {}", username, e);
            Ok(Json(ApiResponse {
                success: false,
                data: None,
                message: "Failed to fetch data".to_string(),
            }))
        }
    }
}

// 分享到 X（仅 Intent，无 OAuth / 无代发帖）

/// POST /api/x/share
///
/// 组装分享文案并返回 X Web Intent 链接。
/// 用户在浏览器打开链接后，用自己的 X 账号完成发布——站点不代发、不存用户 OAuth。
#[derive(Debug, Deserialize)]
pub struct ShareToXRequest {
    /// 直接指定正文（优先）
    pub text: Option<String>,
    /// 无 text 时：标题
    pub title: Option<String>,
    /// 无 text 时：摘要
    pub summary: Option<String>,
    /// 附带链接（会拼进正文，并用于 Intent url 参数）
    pub url: Option<String>,
    /// 话题标签（可不带 #）
    pub hashtags: Option<Vec<String>>,
    /// 正文最大长度，默认 280
    pub max_length: Option<usize>,
}

/// GET /api/x/share/status
pub async fn share_status() -> (StatusCode, Json<Value>) {
    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "mode": "intent",
            "can_intent": true,
            "can_post": false,
            "hint": "Share uses X Web Intent only. POST /api/x/share with text/title/summary/url; open intent_url in browser. No OAuth, no server-side posting.",
        })),
    )
}

/// POST /api/x/share — 生成 Intent 分享链接
pub async fn share_to_x(Json(req): Json<ShareToXRequest>) -> (StatusCode, Json<Value>) {
    let max_len = req.max_length.unwrap_or(X_SHARE_DEFAULT_MAX_LEN);
    let hashtags = req.hashtags.unwrap_or_default();
    let url = req
        .url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned);

    let composed = compose_x_share_text(
        req.text.as_deref(),
        req.title.as_deref(),
        req.summary.as_deref(),
        url.as_deref(),
        &hashtags,
        max_len,
    );

    if composed.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "success": false,
                "message": "分享内容为空：请提供 text，或 title/summary",
            })),
        );
    }

    let intent_url = build_x_intent_url(&composed, url.as_deref());

    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "mode": "intent",
            "text": composed,
            "char_count": composed.chars().count(),
            "max_length": max_len,
            "intent_url": intent_url,
            "message": "已生成 X 分享链接，请在浏览器打开 intent_url 完成发布",
        })),
    )
}

#[cfg(test)]
mod x_secret_gate_tests {
    use super::*;
    use axum::response::IntoResponse;

    #[tokio::test]
    async fn reject_query_bearer_token_blocks_nonempty() {
        let err = reject_query_bearer_token(&Some("AAAA-secret".into())).unwrap_err();
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(resp.into_body(), 64 * 1024)
            .await
            .expect("body");
        let v: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(v["error"], "bearer_token_not_allowed");
        assert!(
            v.get("message")
                .and_then(|m| m.as_str())
                .is_some_and(|m| m.to_ascii_lowercase().contains("query")),
            "message should mention query restriction: {v}"
        );
    }

    #[test]
    fn reject_query_bearer_token_allows_absent_or_blank() {
        assert!(reject_query_bearer_token(&None).is_ok());
        assert!(reject_query_bearer_token(&Some(String::new())).is_ok());
        assert!(reject_query_bearer_token(&Some("   ".into())).is_ok());
    }
}
