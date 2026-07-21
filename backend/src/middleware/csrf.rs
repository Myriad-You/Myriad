use axum::{
    extract::Request,
    http::{header, HeaderMap, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use rand::{distr::Alphanumeric, RngExt};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// CSRF Token 结构
#[derive(Debug, Clone)]
struct CsrfToken {
    token: String,
    created_at: Instant,
}

/// 全局 CSRF Token 存储
/// Key: Session ID (从 Cookie 或 JWT 中提取)
static CSRF_TOKENS: once_cell::sync::Lazy<Arc<RwLock<HashMap<String, CsrfToken>>>> =
    once_cell::sync::Lazy::new(|| {
        // 启动清理任务
        let store: Arc<RwLock<HashMap<String, CsrfToken>>> = Arc::new(RwLock::new(HashMap::new()));
        let store_clone = store.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(180)); // 每3分钟清理一次（优化内存）
            loop {
                interval.tick().await;
                let mut tokens = store_clone.write().await;
                let now = Instant::now();
                let before_count = tokens.len();
                tokens.retain(|_, csrf_token| {
                    now.duration_since(csrf_token.created_at) < Duration::from_secs(3600)
                    // Token 有效期 1 小时
                });
                let removed = before_count - tokens.len();
                if removed > 0 {
                    tracing::info!(
                        "🧹 CSRF cleanup: removed {} expired tokens, {} remaining",
                        removed,
                        tokens.len()
                    );
                }
            }
        });

        store
    });

/// 生成随机 CSRF Token
fn generate_csrf_token() -> String {
    rand::rng()
        .sample_iter(&Alphanumeric)
        .take(32)
        .map(char::from)
        .collect()
}

/// 从请求中提取会话标识符（用于关联 CSRF Token）
fn extract_session_id(headers: &HeaderMap) -> Option<String> {
    fn jwt_signature(token: &str) -> Option<String> {
        let mut segments = token.split('.');
        let header = segments.next()?;
        let payload = segments.next()?;
        let signature = segments.next()?;
        if header.is_empty()
            || payload.is_empty()
            || signature.is_empty()
            || segments.next().is_some()
        {
            return None;
        }
        Some(signature.to_string())
    }

    // 优先从 Authorization header 提取 JWT
    if let Some(auth_header) = headers.get("Authorization") {
        if let Ok(auth_str) = auth_header.to_str() {
            if let Some(token) = auth_str.strip_prefix("Bearer ") {
                return jwt_signature(token);
            }
        }
    }

    // 回退到 Cookie
    if let Some(cookie_header) = headers.get(header::COOKIE) {
        if let Ok(cookies) = cookie_header.to_str() {
            for cookie in cookies.split(';') {
                if let Some((name, value)) = cookie.trim().split_once('=') {
                    if name == "auth_token" {
                        return jwt_signature(value);
                    }
                }
            }
        }
    }

    None
}

/// CSRF 防护中间件
/// ✅ 安全修复 P0: 验证所有状态变更请求的 CSRF Token
///
/// 安全策略：
/// - 对于已认证用户：必须提供有效的 CSRF Token
/// - 对于未认证用户（游客）：跳过 CSRF 验证（CSRF 攻击对游客无意义，因为没有 session 可劫持）
/// - 某些公开 API 直接豁免（如登录、健康检查等）
pub async fn csrf_middleware(req: Request, next: Next) -> Response {
    let method = req.method();
    let path = req.uri().path();

    // 只对状态变更操作（POST/PUT/PATCH/DELETE）进行 CSRF 检查
    if !matches!(
        method,
        &Method::POST | &Method::PUT | &Method::PATCH | &Method::DELETE
    ) {
        return next.run(req).await;
    }

    // 排除不需要 CSRF 保护的端点（登录、公开接口等）
    if is_csrf_exempt(path) {
        return next.run(req).await;
    }

    let headers = req.headers();

    // 如果没有 session（游客），跳过 CSRF 验证
    // 理由：CSRF 攻击的目的是劫持已登录用户的 session 执行操作
    // 游客没有 session，无法被 CSRF 攻击利用
    // 后端 API 会通过权限下放配置决定游客能访问什么
    let Some(session_id) = extract_session_id(headers) else {
        tracing::debug!(
            "⏭️ CSRF check skipped: No session (guest user) for {}",
            path
        );
        return next.run(req).await;
    };

    // 从请求头获取 CSRF Token
    let client_token = headers
        .get("X-CSRF-Token")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if client_token.is_empty() {
        tracing::warn!("🚨 CSRF check failed: Missing X-CSRF-Token header");
        return (
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "CSRF token missing",
                "message": "X-CSRF-Token header is required for state-changing operations"
            })),
        )
            .into_response();
    }

    // 验证 CSRF Token
    let tokens = CSRF_TOKENS.read().await;
    match tokens.get(&session_id) {
        Some(stored_token) => {
            // 检查 Token 是否过期
            if Instant::now().duration_since(stored_token.created_at) > Duration::from_secs(3600) {
                drop(tokens); // 释放读锁
                tracing::warn!("🚨 CSRF check failed: Token expired");
                return (
                    StatusCode::FORBIDDEN,
                    Json(json!({
                        "error": "CSRF token expired",
                        "message": "Please refresh the page and try again"
                    })),
                )
                    .into_response();
            }

            // 验证 Token 是否匹配
            if stored_token.token != client_token {
                drop(tokens); // 释放读锁
                tracing::warn!("🚨 CSRF check failed: Token mismatch");
                return (
                    StatusCode::FORBIDDEN,
                    Json(json!({
                        "error": "CSRF token invalid",
                        "message": "Invalid CSRF token. Please refresh the page and try again."
                    })),
                )
                    .into_response();
            }

            drop(tokens); // 释放读锁
            tracing::debug!("✅ CSRF check passed for {}", path);
            next.run(req).await
        }
        None => {
            drop(tokens); // 释放读锁
            tracing::warn!("🚨 CSRF check failed: No token found for session");
            (
                StatusCode::FORBIDDEN,
                Json(json!({
                    "error": "CSRF token not found",
                    "message": "No CSRF token found for this session. Please refresh the page."
                })),
            )
                .into_response()
        }
    }
}

/// 检查路径是否不需要 CSRF 保护
fn is_csrf_exempt(path: &str) -> bool {
    // 公开接口、登录接口、健康检查等不需要 CSRF 保护
    path.starts_with("/api/auth/login")
        || path.starts_with("/api/auth/logout") // 退出登录不需要 CSRF（已经在退出了）
        || path.starts_with("/api/setup/")
        || path.starts_with("/health")
        || path.starts_with("/api/proxy/") // 图片代理等公开接口
        || path.starts_with("/api/ai/") // AI 推荐等公开接口
        || path.starts_with("/api/agent/") // Agent API - 已有 JWT 认证保护
                                           // 注意: /api/tapps/ 和 /api/tapp/ 不在豁免列表
                                           // 已登录用户需要 CSRF 保护，游客通过上面的 session 检查自动跳过
}

/// 生成并返回 CSRF Token 的接口
/// GET /api/csrf-token
///
/// **Contract (durable guest UX):** no session → **HTTP 200**
/// `{ "csrf_token": null }` (not 401). Guests do not need CSRF tokens;
/// middleware already skips CSRF checks when there is no session.
pub async fn get_csrf_token(headers: HeaderMap) -> impl IntoResponse {
    let session_id = match extract_session_id(&headers) {
        Some(id) => id,
        None => {
            tracing::debug!("[csrf] no session — returning null token (guest probe)");
            return (
                StatusCode::OK,
                Json(json!({
                    "csrf_token": null
                })),
            )
                .into_response();
        }
    };

    // 先检查是否已存在有效的 Token
    {
        let tokens = CSRF_TOKENS.read().await;
        if let Some(existing) = tokens.get(&session_id) {
            // 如果 Token 未过期（还有超过 5 分钟有效期），复用现有 Token
            if Instant::now().duration_since(existing.created_at) < Duration::from_secs(3300) {
                tracing::debug!("✅ Reusing existing CSRF token for session");
                return (
                    StatusCode::OK,
                    Json(json!({
                        "csrf_token": existing.token.clone()
                    })),
                )
                    .into_response();
            }
        }
    }

    // 生成新的 CSRF Token
    let token = generate_csrf_token();
    let csrf_token = CsrfToken {
        token: token.clone(),
        created_at: Instant::now(),
    };

    // 存储 Token（带大小限制防止内存泄漏）
    let mut tokens = CSRF_TOKENS.write().await;

    // 如果超过限制（10000个token），清理最旧的token
    const MAX_CSRF_TOKENS: usize = 10000;
    if tokens.len() >= MAX_CSRF_TOKENS {
        // 找出最旧的token并删除
        if let Some(oldest_key) = tokens
            .iter()
            .min_by_key(|(_, v)| v.created_at)
            .map(|(k, _)| k.clone())
        {
            tokens.remove(&oldest_key);
            tracing::warn!("🧹 CSRF token limit reached, removed oldest token");
        }
    }

    tokens.insert(session_id, csrf_token);

    tracing::debug!("✅ CSRF token generated (total: {})", tokens.len());

    (
        StatusCode::OK,
        Json(json!({
            "csrf_token": token
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    #[test]
    fn test_generate_csrf_token() {
        let token1 = generate_csrf_token();
        let token2 = generate_csrf_token();

        assert_eq!(token1.len(), 32);
        assert_eq!(token2.len(), 32);
        assert_ne!(token1, token2);
    }

    #[tokio::test]
    async fn csrf_token_guest_returns_200_with_null_token() {
        let headers = HeaderMap::new();
        let response = get_csrf_token(headers).await.into_response();
        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), 1024).await.expect("body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert!(value.get("csrf_token").is_some());
        assert!(value["csrf_token"].is_null());
        assert!(value.get("error").is_none());
    }

    #[test]
    fn session_id_is_bound_to_the_jwt_signature() {
        let mut first = HeaderMap::new();
        first.insert(
            "Authorization",
            "Bearer same.header.signature-one".parse().unwrap(),
        );
        let mut second = HeaderMap::new();
        second.insert(
            "Authorization",
            "Bearer same.header.signature-two".parse().unwrap(),
        );

        assert_eq!(extract_session_id(&first).as_deref(), Some("signature-one"));
        assert_eq!(
            extract_session_id(&second).as_deref(),
            Some("signature-two")
        );
        assert_ne!(extract_session_id(&first), extract_session_id(&second));
    }

    #[test]
    fn test_is_csrf_exempt() {
        assert!(is_csrf_exempt("/api/auth/login"));
        assert!(is_csrf_exempt("/api/setup/init-database"));
        assert!(is_csrf_exempt("/health"));
        assert!(is_csrf_exempt("/api/proxy/image"));

        // Tapp API 不在豁免列表（通过 session 检查决定是否需要 CSRF）
        assert!(!is_csrf_exempt("/api/auth/oauth/github/callback"));
        assert!(!is_csrf_exempt("/api/tapp/ai/v2/tasks"));
        assert!(!is_csrf_exempt("/api/tapps/install"));
        assert!(!is_csrf_exempt("/api/tapps/my-app/start"));

        assert!(!is_csrf_exempt("/api/config"));
        assert!(!is_csrf_exempt("/api/auth/change-password"));
    }
}
