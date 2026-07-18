use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{
    extract::State,
    http::{header, StatusCode},
    response::IntoResponse,
    Json,
};
use chrono::{Duration, Utc};
use jsonwebtoken::{encode, EncodingKey, Header};
use regex::Regex;
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::env;

use crate::middleware::auth::ensure_current_admin;

use super::auth::Claims;

/// Request to create admin account
#[derive(Debug, Deserialize)]
pub struct CreateAdminRequest {
    pub username: String,
    pub password: String,
}

/// Request for local login
#[derive(Debug, Deserialize)]
pub struct LocalLoginRequest {
    pub username: String,
    pub password: String,
}

/// Request to change password
#[derive(Debug, Deserialize)]
pub struct ChangePasswordRequest {
    pub old_password: String,
    pub new_password: String,
}

/// Browser session response. The JWT is delivered only in the HttpOnly cookie.
#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub user: UserInfo,
}

/// User information
#[derive(Debug, Serialize)]
pub struct UserInfo {
    pub id: i32,
    pub username: String,
    pub is_admin: bool,
    pub auth_provider: String,
}

/// POST /api/setup/create-admin
/// Create the local administrator account (only during setup)
/// ✅ PROTECTION: Checks if admin already exists and prevents duplicate creation
pub async fn create_admin(
    State(db): State<DatabaseConnection>,
    Json(request): Json<CreateAdminRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    tracing::info!("Creating local admin account: {}", request.username);

    // Validate username
    validate_username(&request.username)?;

    // Validate password
    validate_password(&request.password)?;

    // ✅ SECURITY CHECK: setup-only — 拒绝若已经存在任意 admin（不再限于 local）
    // PR #4: 改成"检查任意 admin"，因为现在 admin 不再强制 local provider
    let admin_exists_result = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT EXISTS (SELECT 1 FROM users WHERE is_admin = true) as exists",
            vec![],
        ))
        .await
        .map_err(|e| {
            tracing::error!("Failed to check existing admin: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Database error"})),
            )
        })?;

    let admin_exists: bool = admin_exists_result
        .and_then(|row| row.try_get("", "exists").ok())
        .unwrap_or(false);

    if admin_exists {
        tracing::error!(
            "🚨 setup/create-admin REJECTED: an admin already exists (use admin user-management instead)"
        );
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Admin account already exists",
                "message": "Setup has already been completed. Sign in as an existing admin to create more accounts."
            })),
        ));
    }

    // Hash password using Argon2id
    let password_hash = hash_password(&request.password)?;

    // Insert admin user
    use sea_orm::Value as SeaValue;

    let insert_query = "INSERT INTO users (
        username, 
        auth_provider, 
        password_hash, 
        is_admin, 
        github_id,
        avatar_url,
        created_at, 
        updated_at
    ) VALUES ($1, $2, $3, $4, NULL, $5, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
    RETURNING id";

    let user_result = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            insert_query,
            vec![
                SeaValue::String(Some(Box::new(request.username.clone()))),
                SeaValue::String(Some(Box::new("local".to_string()))),
                SeaValue::String(Some(Box::new(password_hash))),
                SeaValue::Bool(Some(true)),
                SeaValue::String(Some(Box::new(
                    "https://ui-avatars.com/api/?name=Admin&background=4f46e5&color=fff"
                        .to_string(),
                ))),
            ],
        ))
        .await
        .map_err(|e| {
            tracing::error!("Failed to create admin user: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Failed to create admin account"})),
            )
        })?;

    let user_id: i32 = user_result
        .and_then(|row| row.try_get("", "id").ok())
        .ok_or_else(|| {
            tracing::error!("Failed to get user ID");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Failed to create admin account"})),
            )
        })?;

    tracing::info!(
        "✅ Local admin account created: {} (ID: {})",
        request.username,
        user_id
    );

    Ok(Json(json!({
        "success": true,
        "message": "Admin account created successfully",
        "user_id": user_id
    })))
}

/// POST /api/auth/login
/// Local login endpoint
pub async fn local_login(
    State(db): State<DatabaseConnection>,
    Json(request): Json<LocalLoginRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<Value>)> {
    tracing::info!("Local login attempt: {}", request.username);

    // Query user by username
    // PR #4: 不再要求 auth_provider='local' — 只要 password_hash 存在就能本地登录。
    // 这样 GitHub-注册用户走 /api/auth/me/set-password 后也能用 username 登录。
    use sea_orm::Value as SeaValue;

    let query = "SELECT id, username, password_hash, is_admin, auth_provider, local_login_disabled
                 FROM users
                 WHERE LOWER(username) = LOWER($1) AND password_hash IS NOT NULL";

    let user_result = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            query,
            vec![SeaValue::String(Some(Box::new(request.username.clone())))],
        ))
        .await
        .map_err(|e| {
            tracing::error!("Database error: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Database error"})),
            )
        })?;

    let user_row = match user_result {
        Some(row) => row,
        None => {
            // 执行虚拟 Argon2 验证以防止时序攻击枚举用户名
            let dummy_hash = "$argon2id$v=19$m=19456,t=2,p=1$dW5rbm93bnNhbHQ$dW5rbm93bmhhc2g";
            let _ = verify_password(&request.password, dummy_hash);
            tracing::warn!("User not found: {}", request.username);
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({
                    "error": "Invalid credentials",
                    "message": "Username or password is incorrect"
                })),
            ));
        }
    };

    // Extract user data
    let user_id: i32 = user_row.try_get("", "id").map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Failed to read user data"})),
        )
    })?;

    let username: String = user_row.try_get("", "username").map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Failed to read user data"})),
        )
    })?;

    let password_hash: String = user_row.try_get("", "password_hash").map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Failed to read user data"})),
        )
    })?;

    let is_admin: bool = user_row.try_get("", "is_admin").unwrap_or(false);

    let local_login_disabled: bool = user_row
        .try_get("", "local_login_disabled")
        .unwrap_or(false);

    // Check if local login is disabled
    if local_login_disabled {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "Local login disabled",
                "message": "This account has been linked to GitHub. Please use GitHub OAuth to login."
            })),
        ));
    }

    // Verify password
    verify_password(&request.password, &password_hash)?;

    // Update last login timestamp
    let _ = db
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE users SET last_login_at = CURRENT_TIMESTAMP WHERE id = $1",
            vec![SeaValue::Int(Some(user_id))],
        ))
        .await;

    // Generate JWT token
    let jwt_secret = env::var("JWT_SECRET").map_err(|_| {
        tracing::error!("JWT_SECRET not set");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Server configuration error"})),
        )
    })?;

    let claims = Claims {
        sub: user_id.to_string(),
        username: username.clone(),
        is_admin, // ✅ 安全修复 P0: 从数据库读取 is_admin
        exp: (Utc::now() + Duration::days(30)).timestamp(),
        iat: Utc::now().timestamp(),
    };

    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(jwt_secret.as_bytes()),
    )
    .map_err(|e| {
        tracing::error!("Failed to create JWT: {:?}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Failed to create session token"})),
        )
    })?;

    tracing::info!("✅ Local login successful: {}", username);

    // 设置 HttpOnly Cookie
    // 使用 SameSite=Lax 保持与 OAuth 登录一致
    // 使用 SiteConfig 判断是否为生产环境（基于 base_url 是否为 HTTPS）
    use crate::oauth_url_builder::SiteConfig;
    let is_production = SiteConfig::is_production().await;
    let cookie_value = format!(
        "auth_token={}; Path=/; HttpOnly; SameSite=Lax; Max-Age=2592000{}",
        token,
        if is_production { "; Secure" } else { "" }
    );

    let response = Json(AuthResponse {
        user: UserInfo {
            id: user_id,
            username,
            is_admin,
            auth_provider: "local".to_string(),
        },
    });

    // 构建包含 Set-Cookie 的响应
    let mut response = response.into_response();
    response
        .headers_mut()
        .insert(header::SET_COOKIE, cookie_value.parse().unwrap());

    Ok(response)
}

/// POST /api/auth/change-password
/// Change password for local account
pub async fn change_password(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Json(request): Json<ChangePasswordRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    // Extract and validate JWT token from headers
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Missing or invalid authorization header"})),
            )
        })?;

    let jwt_secret = env::var("JWT_SECRET").map_err(|_| {
        tracing::error!("JWT_SECRET not set");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Server configuration error"})),
        )
    })?;

    // Decode and validate token
    let claims = jsonwebtoken::decode::<Claims>(
        token,
        &jsonwebtoken::DecodingKey::from_secret(jwt_secret.as_bytes()),
        &jsonwebtoken::Validation::default(),
    )
    .map_err(|e| {
        tracing::warn!("Invalid JWT token: {:?}", e);
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "Invalid or expired token"})),
        )
    })?
    .claims;

    let user_id = claims.sub.parse::<i32>().map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Invalid user ID"})),
        )
    })?;

    tracing::info!("Password change request for user ID: {}", user_id);

    // Validate new password
    validate_password(&request.new_password)?;

    // Query user data
    use sea_orm::Value as SeaValue;

    let query = "SELECT username, password_hash, auth_provider, local_login_disabled 
                 FROM users 
                 WHERE id = $1";

    let user_result = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            query,
            vec![SeaValue::Int(Some(user_id))],
        ))
        .await
        .map_err(|e| {
            tracing::error!("Database error: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Database error"})),
            )
        })?;

    let user_row = user_result.ok_or_else(|| {
        tracing::warn!("User not found: {}", user_id);
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "User not found"})),
        )
    })?;

    let username: String = user_row.try_get("", "username").map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Failed to read user data"})),
        )
    })?;

    let auth_provider: String = user_row.try_get("", "auth_provider").map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Failed to read user data"})),
        )
    })?;

    // PR #4: 任何拥有 password_hash 的账户都能改密码（不再要求 auth_provider='local'）
    // 没有密码的账户（纯 OAuth）应走 /api/auth/me/set-password 后补密码。
    let _ = auth_provider; // 信息性字段，保留读取以兼容旧 SELECT

    let current_password_hash: Option<String> = user_row.try_get("", "password_hash").ok();
    let current_password_hash = match current_password_hash {
        Some(h) => h,
        None => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "No password set",
                    "message": "This account has no password yet. Use /api/auth/me/set-password instead."
                })),
            ));
        }
    };

    // Verify old password
    verify_password(&request.old_password, &current_password_hash)?;

    // Hash new password
    let new_password_hash = hash_password(&request.new_password)?;

    // Update password
    let update_query =
        "UPDATE users SET password_hash = $1, updated_at = CURRENT_TIMESTAMP WHERE id = $2";

    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        update_query,
        vec![
            SeaValue::String(Some(Box::new(new_password_hash))),
            SeaValue::Int(Some(user_id)),
        ],
    ))
    .await
    .map_err(|e| {
        tracing::error!("Failed to update password: {:?}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Failed to update password"})),
        )
    })?;

    tracing::info!("✅ Password changed successfully for user: {}", username);

    Ok(Json(json!({
        "success": true,
        "message": "Password changed successfully"
    })))
}

// Helper functions

/// Validate username format
fn validate_username(username: &str) -> Result<(), (StatusCode, Json<Value>)> {
    if username.len() < 3 || username.len() > 20 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Invalid username",
                "message": "Username must be between 3 and 20 characters"
            })),
        ));
    }

    let regex = Regex::new(r"^[a-zA-Z0-9_]+$").unwrap();
    if !regex.is_match(username) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Invalid username",
                "message": "Username can only contain letters, numbers, and underscores"
            })),
        ));
    }

    Ok(())
}

/// Validate password strength
fn validate_password(password: &str) -> Result<(), (StatusCode, Json<Value>)> {
    if password.len() < 8 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Invalid password",
                "message": "Password must be at least 8 characters long"
            })),
        ));
    }

    // Check password complexity: must contain both letters and numbers
    let has_letter = password.chars().any(|c| c.is_alphabetic());
    let has_digit = password.chars().any(|c| c.is_numeric());

    if !has_letter || !has_digit {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Invalid password",
                "message": "Password must contain both letters and numbers for security"
            })),
        ));
    }

    Ok(())
}

/// Hash password using Argon2id
fn hash_password(password: &str) -> Result<String, (StatusCode, Json<Value>)> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();

    let password_hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| {
            tracing::error!("Failed to hash password: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Failed to process password"})),
            )
        })?
        .to_string();

    Ok(password_hash)
}

/// Verify password against hash
fn verify_password(password: &str, hash: &str) -> Result<(), (StatusCode, Json<Value>)> {
    let parsed_hash = PasswordHash::new(hash).map_err(|e| {
        tracing::error!("Failed to parse password hash: {:?}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Failed to verify password"})),
        )
    })?;

    let argon2 = Argon2::default();

    argon2
        .verify_password(password.as_bytes(), &parsed_hash)
        .map_err(|_| {
            tracing::warn!("Password verification failed");
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({
                    "error": "Invalid credentials",
                    "message": "Username or password is incorrect"
                })),
            )
        })
}

// ============================================================================
// PR #4 新增端点：公开注册 + 后补密码 + 本地登录开关
// ============================================================================
// 详见 docs/oauth-refactor-plan.md §7.2、§9

/// POST /api/auth/register —— 公开本地账号注册
///
/// 受 `DynamicConfig.allow_local_registration` 开关控制；关闭时返回 403。
#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub password: String,
    pub email: Option<String>,
}

pub async fn register(
    State(db): State<DatabaseConnection>,
    Json(req): Json<RegisterRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<Value>)> {
    // 开关检查
    {
        let cfg = crate::GLOBAL_DYNAMIC_CONFIG.read().await;
        if !cfg.allow_local_registration {
            return Err((
                StatusCode::FORBIDDEN,
                Json(json!({
                    "error": "Registration disabled",
                    "message": "Public registration is disabled. Ask an administrator to create an account."
                })),
            ));
        }
    }

    validate_username(&req.username)?;
    validate_password(&req.password)?;

    use sea_orm::Value as SeaValue;

    // username 冲突检查（大小写不敏感）
    let dup = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT 1 FROM users WHERE LOWER(username) = LOWER($1) LIMIT 1",
            vec![SeaValue::String(Some(Box::new(req.username.clone())))],
        ))
        .await
        .map_err(|e| {
            tracing::error!("Database error: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Database error"})),
            )
        })?;
    if dup.is_some() {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Username taken",
                "message": "This username is already in use"
            })),
        ));
    }

    let password_hash = hash_password(&req.password)?;

    let insert = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO users (username, email, password_hash, auth_provider, is_admin, \
                                 avatar_url, created_at, updated_at, last_login_at) \
             VALUES ($1, $2, $3, 'local', false, $4, NOW(), NOW(), NOW()) \
             RETURNING id",
            vec![
                SeaValue::String(Some(Box::new(req.username.clone()))),
                req.email
                    .clone()
                    .map(|s| SeaValue::String(Some(Box::new(s))))
                    .unwrap_or(SeaValue::String(None)),
                SeaValue::String(Some(Box::new(password_hash))),
                SeaValue::String(Some(Box::new(
                    "https://ui-avatars.com/api/?name=User&background=4f46e5&color=fff".to_string(),
                ))),
            ],
        ))
        .await
        .map_err(|e| {
            tracing::error!("Failed to insert user: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Failed to create account"})),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Insert returned no row"})),
            )
        })?;

    let user_id: i32 = insert.try_get("", "id").map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Failed to read new user id"})),
        )
    })?;

    tracing::info!("✅ Public registration: {} (id={})", req.username, user_id);

    // 注册即登录：颁发 JWT + cookie
    issue_session_cookie(user_id, &req.username, false).await
}

/// POST /api/auth/me/set-password —— GitHub-only 用户后补密码
///
/// 要求当前账户**没有**密码（已有密码走 `change_password`）。
#[derive(Debug, Deserialize)]
pub struct SetPasswordRequest {
    pub new_password: String,
}

pub async fn set_password(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Json(req): Json<SetPasswordRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    use crate::middleware::auth::verify_jwt_token;
    use sea_orm::Value as SeaValue;

    let claims = verify_jwt_token(&headers).map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "Unauthorized"})),
        )
    })?;
    let user_id: i32 = claims.sub.parse().map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Invalid user id"})),
        )
    })?;

    validate_password(&req.new_password)?;

    // 必须当前 password_hash 为 NULL
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT password_hash IS NOT NULL AS has_password FROM users WHERE id = $1",
            vec![SeaValue::Int(Some(user_id))],
        ))
        .await
        .map_err(|e| {
            tracing::error!("DB error: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Database error"})),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "User not found"})),
            )
        })?;
    let has_password: bool = row.try_get("", "has_password").unwrap_or(false);
    if has_password {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Password already set",
                "message": "Use POST /api/auth/change-password to change an existing password."
            })),
        ));
    }

    let hash = hash_password(&req.new_password)?;

    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE users SET password_hash = $1, updated_at = NOW() WHERE id = $2",
        vec![
            SeaValue::String(Some(Box::new(hash))),
            SeaValue::Int(Some(user_id)),
        ],
    ))
    .await
    .map_err(|e| {
        tracing::error!("Failed to set password: {:?}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Failed to set password"})),
        )
    })?;

    Ok(Json(json!({"success": true})))
}

/// PATCH /api/auth/me/local-login —— 开关本地登录
///
/// - `enabled=false`：禁用本地登录；前置条件 — 至少有一个 OAuth identity（防失联）
/// - `enabled=true`：启用本地登录；前置条件 — 已有密码（password_hash 非 NULL）
#[derive(Debug, Deserialize)]
pub struct LocalLoginToggleRequest {
    pub enabled: bool,
}

pub async fn toggle_local_login(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Json(req): Json<LocalLoginToggleRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    use crate::middleware::auth::verify_jwt_token;
    use sea_orm::Value as SeaValue;

    let claims = verify_jwt_token(&headers).map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "Unauthorized"})),
        )
    })?;
    let user_id: i32 = claims.sub.parse().map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Invalid user id"})),
        )
    })?;

    // 取当前状态
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT password_hash IS NOT NULL AS has_password, \
                    (SELECT COUNT(*) FROM user_identities WHERE user_id = users.id) AS identity_count \
             FROM users WHERE id = $1",
            vec![SeaValue::Int(Some(user_id))],
        ))
        .await
        .map_err(|e| {
            tracing::error!("DB error: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Database error"})),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "User not found"})),
            )
        })?;

    let has_password: bool = row.try_get("", "has_password").unwrap_or(false);
    let identity_count: i64 = row.try_get("", "identity_count").unwrap_or(0);

    // 前置检查
    if req.enabled {
        if !has_password {
            return Err((
                StatusCode::CONFLICT,
                Json(json!({
                    "error": "No password set",
                    "message": "Set a password via /api/auth/me/set-password before enabling local login."
                })),
            ));
        }
    } else if identity_count == 0 {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Cannot disable last login method",
                "message": "Link at least one OAuth provider before disabling local login."
            })),
        ));
    }

    let disabled = !req.enabled;
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE users SET local_login_disabled = $1, updated_at = NOW() WHERE id = $2",
        vec![SeaValue::Bool(Some(disabled)), SeaValue::Int(Some(user_id))],
    ))
    .await
    .map_err(|e| {
        tracing::error!("Failed to update local_login_disabled: {:?}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Failed to update"})),
        )
    })?;

    Ok(Json(json!({"success": true, "enabled": req.enabled})))
}

/// 内部：给指定 user 颁发 JWT + 设置 cookie，返回 AuthResponse + Set-Cookie
async fn issue_session_cookie(
    user_id: i32,
    username: &str,
    is_admin: bool,
) -> Result<axum::response::Response, (StatusCode, Json<Value>)> {
    let jwt_secret = env::var("JWT_SECRET").map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "JWT_SECRET not configured"})),
        )
    })?;
    let claims = Claims {
        sub: user_id.to_string(),
        username: username.to_string(),
        is_admin,
        exp: (Utc::now() + Duration::days(30)).timestamp(),
        iat: Utc::now().timestamp(),
    };
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(jwt_secret.as_bytes()),
    )
    .map_err(|e| {
        tracing::error!("JWT encode failed: {:?}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Failed to create session token"})),
        )
    })?;

    use crate::oauth_url_builder::SiteConfig;
    let is_production = SiteConfig::is_production().await;
    let cookie_value = format!(
        "auth_token={}; Path=/; HttpOnly; SameSite=Lax; Max-Age=2592000{}",
        token,
        if is_production { "; Secure" } else { "" }
    );

    let body = Json(AuthResponse {
        user: UserInfo {
            id: user_id,
            username: username.to_string(),
            is_admin,
            auth_provider: "local".to_string(),
        },
    });
    let mut resp = body.into_response();
    resp.headers_mut()
        .insert(header::SET_COOKIE, cookie_value.parse().unwrap());
    Ok(resp)
}

// ============================================================================
// PR #6: Admin 后台建本地账号
// ============================================================================
// 详见 docs/oauth-refactor-plan.md §7.2
//
// 不受 allow_local_registration 开关限制；可选 is_admin 字段提升新账号为管理员。

#[derive(Debug, Deserialize)]
pub struct AdminCreateUserRequest {
    pub username: String,
    pub password: String,
    pub email: Option<String>,
    #[serde(default)]
    pub is_admin: bool,
}

pub async fn admin_create_user(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Json(req): Json<AdminCreateUserRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    use crate::middleware::auth::verify_jwt_token;

    let claims = verify_jwt_token(&headers).map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "Unauthorized"})),
        )
    })?;
    ensure_current_admin(&claims).await?;

    validate_username(&req.username)?;
    validate_password(&req.password)?;

    use sea_orm::Value as SeaValue;

    let dup = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT 1 FROM users WHERE LOWER(username) = LOWER($1) LIMIT 1",
            vec![SeaValue::String(Some(Box::new(req.username.clone())))],
        ))
        .await
        .map_err(|e| {
            tracing::error!("DB error: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Database error"})),
            )
        })?;
    if dup.is_some() {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Username taken",
                "message": "This username is already in use"
            })),
        ));
    }

    let password_hash = hash_password(&req.password)?;

    let insert = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO users (username, email, password_hash, auth_provider, is_admin, \
                                 avatar_url, created_at, updated_at) \
             VALUES ($1, $2, $3, 'local', $4, $5, NOW(), NOW()) \
             RETURNING id",
            vec![
                SeaValue::String(Some(Box::new(req.username.clone()))),
                req.email
                    .clone()
                    .map(|s| SeaValue::String(Some(Box::new(s))))
                    .unwrap_or(SeaValue::String(None)),
                SeaValue::String(Some(Box::new(password_hash))),
                SeaValue::Bool(Some(req.is_admin)),
                SeaValue::String(Some(Box::new(format!(
                    "https://ui-avatars.com/api/?name={}&background=4f46e5&color=fff",
                    urlencoding::encode(&req.username)
                )))),
            ],
        ))
        .await
        .map_err(|e| {
            tracing::error!("Failed to create user: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Failed to create account"})),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Insert returned no row"})),
            )
        })?;

    let user_id: i32 = insert.try_get("", "id").map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Failed to read new id"})),
        )
    })?;

    tracing::info!(
        "✅ Admin {} created account: {} (id={}, is_admin={})",
        claims.username,
        req.username,
        user_id,
        req.is_admin
    );

    Ok(Json(json!({
        "success": true,
        "user_id": user_id,
        "username": req.username,
        "is_admin": req.is_admin,
    })))
}

// admin 用户列表已迁移到 api::admin_users::list_users（设置页用户管理模块）

#[cfg(test)]
mod tests {
    use super::{AuthResponse, UserInfo};

    #[test]
    fn auth_response_never_serializes_the_jwt() {
        let response = AuthResponse {
            user: UserInfo {
                id: 7,
                username: "alice".to_string(),
                is_admin: false,
                auth_provider: "local".to_string(),
            },
        };
        let serialized = serde_json::to_value(response).unwrap();

        assert!(serialized.get("token").is_none());
        assert_eq!(serialized["user"]["id"], 7);
    }
}
