use axum::{
    extract::Request,
    http::{header, HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use jsonwebtoken::{decode, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::env;

/// JWT Claims structure (must match auth.rs)
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String,      // User ID
    pub username: String, // Username
    pub is_admin: bool,   // ✅ 安全修复 P0: Admin status
    pub exp: i64,         // Expiration time
    pub iat: i64,         // Issued at
}

/// Authentication middleware - verifies JWT token
/// Returns 401 if token is missing or invalid
pub async fn auth_middleware(req: Request, next: Next) -> Response {
    let headers = req.headers();

    match verify_jwt_token(headers) {
        Ok(claims) => {
            // Token is valid, inject claims into request extensions
            let mut req = req;
            req.extensions_mut().insert(claims);
            next.run(req).await
        }
        Err(error_response) => *error_response,
    }
}

/// Admin-only middleware - verifies JWT token and checks admin status
/// Returns 403 if user is not an admin
///
/// ✅ SECURITY: Checks if username is "admin" (simple but effective)
/// Used for dangerous operations like deleting all reports
///
/// TODO: For production, consider checking is_admin field from database or JWT claims
pub async fn admin_middleware(req: Request, next: Next) -> Response {
    let headers = req.headers();

    match verify_jwt_token(headers) {
        Ok(claims) => {
            // ✅ 安全修复 P0: 检查 is_admin 字段而不是用户名
            // 这防止 GitHub 用户名为 "admin" 的用户获得管理员权限
            if !claims.is_admin {
                tracing::warn!(
                    "⚠️  User {} (is_admin={}) attempted to access admin-only endpoint (Forbidden)",
                    claims.username,
                    claims.is_admin
                );
                return (
                    StatusCode::FORBIDDEN,
                    Json(json!({
                        "error": "Forbidden",
                        "message": "Administrator access required. Only admin users can perform this action."
                    })),
                )
                    .into_response();
            }

            tracing::info!(
                "✅ Admin access granted to user: {} (is_admin=true)",
                claims.username
            );

            // ✅ 关键修复: 将 claims 注入到 request extensions 中
            // 这样后续的 Extension(claims) 提取器才能正常工作
            let mut req = req;
            req.extensions_mut().insert(claims);
            next.run(req).await
        }
        Err(error_response) => *error_response,
    }
}

/// 从请求中提取客户端 IP 地址
///
/// 优先级：
/// 1. X-Forwarded-For（反向代理）
/// 2. X-Real-IP（Nginx）
/// 3. 直连 IP（暂不支持，需要 ConnectInfo）
fn extract_client_ip(headers: &HeaderMap) -> String {
    // X-Forwarded-For: client, proxy1, proxy2
    if let Some(forwarded) = headers.get("x-forwarded-for") {
        if let Ok(value) = forwarded.to_str() {
            // 取第一个 IP（真实客户端 IP）
            if let Some(ip) = value.split(',').next() {
                return ip.trim().to_string();
            }
        }
    }

    // X-Real-IP
    if let Some(real_ip) = headers.get("x-real-ip") {
        if let Ok(value) = real_ip.to_str() {
            return value.trim().to_string();
        }
    }

    // 默认未知 IP
    "unknown".to_string()
}

/// 根据 IP 生成稳定的游客 ID
///
/// 使用 IP 的哈希值生成负数 ID（与正数用户 ID 区分）
/// 范围: -2147483648 到 -1
fn generate_guest_id(ip: &str) -> i32 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    ip.hash(&mut hasher);
    // 生成负数 ID（范围 -2147483647 到 -1）
    let hash = hasher.finish();
    -((hash % 2147483647) as i32 + 1)
}

/// Optional authentication middleware - allows guest access
///
/// 用于支持权限下放的 API：
/// - 如果有有效 token，验证并注入 Claims
/// - 如果没有 token 或 token 无效，注入游客 Claims
///
/// 游客 ID 策略：
/// - 基于客户端 IP 生成稳定的负数 ID
/// - 同一 IP 的游客始终获得相同的 ID
/// - 负数 ID 与正数用户 ID 区分，便于管理
///
/// 安全说明：
/// - 游客 Claims 的 is_admin 为 false
/// - API 端点需要自行检查权限（通过 TappPermissionService）
pub async fn optional_auth_middleware(req: Request, next: Next) -> Response {
    let headers = req.headers();

    let claims = match verify_jwt_token(headers) {
        Ok(claims) => claims,
        Err(_) => {
            // 无 token 或 token 无效，创建游客 Claims
            let client_ip = extract_client_ip(headers);
            let guest_id = generate_guest_id(&client_ip);

            tracing::debug!(
                "🎭 Guest access from IP: {} -> Guest ID: {}",
                client_ip,
                guest_id
            );

            Claims {
                sub: guest_id.to_string(), // 负数 ID 字符串
                username: format!("guest:{}", &client_ip),
                is_admin: false,
                exp: 0,
                iat: 0,
            }
        }
    };

    let mut req = req;
    req.extensions_mut().insert(claims);
    next.run(req).await
}

/// Verify JWT token from Authorization header or Cookie
/// ✅ Made public for use in other modules (e.g., auth.rs link_github_account)
pub fn verify_jwt_token(headers: &HeaderMap) -> Result<Claims, Box<Response>> {
    // Extract token from Authorization header or Cookie (优先 Header)
    let token = headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .or_else(|| {
            // 回退到 HttpOnly Cookie
            headers
                .get(header::COOKIE)
                .and_then(|v| v.to_str().ok())
                .and_then(|cookies| {
                    cookies.split(';').find_map(|cookie| {
                        let (name, value) = cookie.trim().split_once('=')?;
                        if name == "auth_token" {
                            Some(value)
                        } else {
                            None
                        }
                    })
                })
        })
        .ok_or_else(|| {
            tracing::debug!("Missing or invalid Authorization header/cookie");
            Box::new(
                (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({
                        "error": "Unauthorized",
                        "message": "Missing or invalid authorization token. Please login first."
                    })),
                )
                    .into_response(),
            )
        })?;

    // Get JWT secret
    let jwt_secret = env::var("JWT_SECRET").map_err(|_| {
        tracing::error!("JWT_SECRET not configured");
        Box::new(
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "Server configuration error",
                    "message": "Authentication system not properly configured"
                })),
            )
                .into_response(),
        )
    })?;

    // Decode and verify token
    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(jwt_secret.as_bytes()),
        &Validation::default(),
    )
    .map_err(|e| {
        tracing::debug!("Invalid JWT token: {:?}", e);
        Box::new(
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({
                    "error": "Invalid token",
                    "message": "Token is invalid or expired. Please login again."
                })),
            )
                .into_response(),
        )
    })?;

    Ok(token_data.claims)
}

/// Optional authentication - extracts claims if token is present, but doesn't fail if missing
/// Useful for endpoints that behave differently for authenticated users but are also public
pub fn extract_optional_claims(headers: &HeaderMap) -> Option<Claims> {
    // 尝试从 Authorization header 或 Cookie 获取 token
    let token = headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .or_else(|| {
            headers
                .get(header::COOKIE)
                .and_then(|v| v.to_str().ok())
                .and_then(|cookies| {
                    cookies.split(';').find_map(|cookie| {
                        let (name, value) = cookie.trim().split_once('=')?;
                        if name == "auth_token" {
                            Some(value)
                        } else {
                            None
                        }
                    })
                })
        })?;

    let jwt_secret = env::var("JWT_SECRET").ok()?;
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(jwt_secret.as_bytes()),
        &Validation::default(),
    )
    .ok()
    .map(|data| data.claims)
}
