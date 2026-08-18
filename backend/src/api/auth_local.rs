use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use myriad_error::AppError;
use once_cell::sync::Lazy;
use regex::Regex;
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration as StdDuration;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::error::HttpError;
use crate::middleware::auth::{
    auth_cookie_value, encode_session_token, ensure_current_admin_on, mint_session_claims,
    notify_auth_cache_invalidation,
};

/// Modest global cap on concurrent Argon2 hash/verify work (MYR-006).
///
/// Argon2 is intentionally CPU- and memory-heavy. Unbounded `spawn_blocking`
/// under concurrent login/register can exhaust the blocking pool. We allow a
/// small fixed concurrency (not a harsh multi-axis rate limit) and fail with
/// 503 after a short wait rather than queue forever. Login, register,
/// change-password, set-password, setup create-admin, and admin create-user
/// all share this single permit path via [`hash_password`] / [`verify_password`].
/// Historical default concurrency (default memory profile). Saver uses 1 via memory_profile.
const PASSWORD_HASH_PERMITS: usize = 4;
/// How long a request may wait for a hash/verify permit before 503.
/// Acts as a short queue bound — waiters beyond this get 503, not harsher IP limits.
const PASSWORD_HASH_ACQUIRE_TIMEOUT: StdDuration = StdDuration::from_secs(15);

/// Acquire a global Argon2 permit, or return 503 if the wait times out.
async fn acquire_password_hash_permit() -> Result<OwnedSemaphorePermit, HttpError> {
    let permits = crate::services::memory_profile::argon2_permits();
    acquire_password_hash_permit_from(
        crate::services::memory_profile::argon2_semaphore(),
        PASSWORD_HASH_ACQUIRE_TIMEOUT,
        permits,
    )
    .await
}

/// Internal/testable permit acquire with an explicit wait budget and semaphore.
async fn acquire_password_hash_permit_from(
    semaphore: Arc<Semaphore>,
    timeout: StdDuration,
    permits_for_log: usize,
) -> Result<OwnedSemaphorePermit, HttpError> {
    match tokio::time::timeout(timeout, semaphore.acquire_owned()).await {
        Ok(Ok(permit)) => Ok(permit),
        Ok(Err(e)) => {
            // Semaphore closed — should not happen for a static; treat as internal.
            tracing::error!("Password hash semaphore closed: {:?}", e);
            Err(HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Failed to process password"})),
            )))
        }
        Err(_) => {
            tracing::warn!(
                permits = permits_for_log,
                timeout_secs = timeout.as_secs_f64(),
                "Password hash concurrency limit reached; returning 503"
            );
            Err(HttpError::from((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "error": "Server busy",
                    "message": "Too many password operations in progress. Please try again shortly."
                })),
            )))
        }
    }
}

/// Postgres advisory-lock key for setup `create-admin`.
///
/// Multi-admin is allowed *after* setup via admin user-management, so we cannot
/// put a partial unique index on `is_admin`. Concurrent first-time setup must
/// still serialize on a single gate — `pg_advisory_xact_lock` does that without
/// blocking other user inserts.
pub(crate) const CREATE_ADMIN_ADVISORY_LOCK_KEY: i64 = 0x4D59_5249_4144_0001; // MYRIAD\0\1

/// Request to create admin account
#[derive(Deserialize)]
pub struct CreateAdminRequest {
    pub username: String,
    pub password: String,
    /// 与 `.env` 里 `MYRIAD_SETUP_SECRET` 对暗号。也可改走 `X-Setup-Secret`。
    #[serde(default)]
    pub setup_secret: Option<String>,
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

/// User information (login / session mint — aligned with `/api/auth/me` staff fields)
#[derive(Debug, Serialize)]
pub struct UserInfo {
    pub id: i32,
    pub username: String,
    pub is_admin: bool,
    /// Durable site owner; LoginForm uses this with `is_admin` for analytics staff.
    pub is_owner: bool,
    pub auth_provider: String,
}

/// Stable 409 body when setup has already minted an admin.
pub(crate) fn admin_already_exists_error() -> AppError {
    AppError::conflict("Admin account already exists").with_message(
        "Setup has already been completed. Sign in as an existing admin to create more accounts.",
    )
}

/// Gate used inside the create-admin transaction after the advisory lock is held.
pub(crate) fn create_admin_gate(admin_exists: bool) -> Result<(), AppError> {
    if admin_exists {
        Err(admin_already_exists_error())
    } else {
        Ok(())
    }
}

/// Classify Postgres / SeaORM insert failures for setup create-admin.
///
/// Unique violations (username, single-owner) become 409 so a race that slips
/// past the EXISTS check still returns a coherent client error — never 500.
pub(crate) fn map_create_admin_insert_error(err: &dyn std::fmt::Display) -> AppError {
    let s = err.to_string();
    let lower = s.to_ascii_lowercase();
    if lower.contains("23505")
        || lower.contains("unique")
        || lower.contains("duplicate key")
        || lower.contains("idx_users_username")
        || lower.contains("idx_users_single_owner")
    {
        // Prefer the setup-specific message: unique on owner/username during
        // first-admin race almost always means setup already completed.
        return admin_already_exists_error();
    }
    AppError::internal("Failed to create admin account").with_message(s)
}

/// POST /api/setup/create-admin
/// Create the local administrator account (only during setup).
///
/// Concurrency: transaction + `pg_advisory_xact_lock` serializes first-admin
/// creation so two concurrent setup requests cannot both pass EXISTS then INSERT.
///
/// Ownership: if orchestration/deploy pre-set `MYRIAD_SETUP_SECRET`, it must
/// match. Wizard-only first install (no secret configured) is not gated.
pub async fn create_admin(
    crate::extract::Db(db): crate::extract::Db,
    headers: HeaderMap,
    Json(request): Json<CreateAdminRequest>,
) -> Result<Json<Value>, HttpError> {
    crate::api::setup_bootstrap::require_setup_secret(&headers, request.setup_secret.as_deref())
        .map_err(HttpError)?;

    tracing::info!("Creating local admin account: {}", request.username);

    validate_username(&request.username)?;
    validate_password(&request.password)?;

    use sea_orm::Value as SeaValue;

    let txn = db.begin().await.map_err(|e| {
        tracing::error!("create-admin begin transaction failed: {:?}", e);
        HttpError(AppError::internal("Database error").with_message(e.to_string()))
    })?;

    // Serialize concurrent setup; released automatically on commit/rollback.
    txn.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT pg_advisory_xact_lock($1)",
        [CREATE_ADMIN_ADVISORY_LOCK_KEY.into()],
    ))
    .await
    .map_err(|e| {
        tracing::error!("create-admin advisory lock failed: {:?}", e);
        HttpError(AppError::internal("Database error").with_message(e.to_string()))
    })?;

    // Setup-only: reject if any admin already exists (any auth_provider).
    let admin_exists_result = txn
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT EXISTS (
                SELECT 1 FROM users WHERE is_admin = true OR COALESCE(is_owner, false) = true
            ) as exists",
            vec![],
        ))
        .await
        .map_err(|e| {
            tracing::error!("Failed to check existing admin: {:?}", e);
            HttpError(AppError::internal("Database error").with_message(e.to_string()))
        })?;

    let admin_exists: bool = admin_exists_result
        .and_then(|row| row.try_get("", "exists").ok())
        .unwrap_or(false);

    if let Err(err) = create_admin_gate(admin_exists) {
        tracing::error!(
            "🚨 setup/create-admin REJECTED: an admin already exists (use admin user-management instead)"
        );
        let _ = txn.rollback().await;
        return Err(HttpError(err));
    }

    // The cheap installation capability, transaction, advisory lock, and
    // durable owner/admin gate all succeeded. Only the single lock winner may
    // now spend CPU/memory on Argon2; concurrent losers wait, recheck, and
    // return 409 without hashing.
    let password_hash = match hash_password(&request.password).await {
        Ok(hash) => hash,
        Err(error) => {
            let _ = txn.rollback().await;
            return Err(error);
        }
    };

    // First setup admin is also the durable site owner (`is_owner`). Schema readiness
    // is a startup/setup invariant, so a missing column is fatal instead of a fallback.
    let insert_with_owner = "INSERT INTO users (
        username,
        auth_provider,
        password_hash,
        is_admin,
        is_owner,
        github_id,
        avatar_url,
        created_at,
        updated_at
    ) VALUES ($1, $2, $3, true, true, NULL, $4, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
    RETURNING id";
    // avatar_url 留空：占位头像是「显示时的兜底」，不是账号数据。
    // 曾在这里播种 ui-avatars.com 外链（渲染出的 "Ad" 就是 name=Admin 的缩写），
    // 结果是每个新装站点从第一天起就依赖一个外部图床，且用户显式选「账号头像」
    // 时会把这张占位图当成真头像用。现在交给前端 <Avatar> 本地生成。
    let insert_params = vec![
        SeaValue::String(Some(request.username.clone())),
        SeaValue::String(Some("local".to_string())),
        SeaValue::String(Some(password_hash)),
        SeaValue::String(None),
    ];

    let user_result = match txn
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            insert_with_owner,
            insert_params.clone(),
        ))
        .await
    {
        Ok(row) => row,
        Err(e) => {
            tracing::error!("Failed to create admin user: {:?}", e);
            let _ = txn.rollback().await;
            return Err(HttpError(map_create_admin_insert_error(&e)));
        }
    };

    let user_id: i32 = match user_result.and_then(|row| row.try_get("", "id").ok()) {
        Some(id) => id,
        None => {
            tracing::error!("Failed to get user ID after create-admin insert");
            let _ = txn.rollback().await;
            return Err(HttpError(AppError::internal(
                "Failed to create admin account",
            )));
        }
    };

    // Persist the claimed marker and close the window before committing the
    // durable owner. A crash after commit must not leave setup open while the
    // database is temporarily unavailable on restart.
    if let Err(error) = crate::api::setup_bootstrap::consume_setup() {
        tracing::error!(%error, "create-admin setup cleanup failed before commit");
        if let Err(reopen_error) = crate::api::setup_bootstrap::reopen_setup_after_failed_claim() {
            tracing::error!(
                %reopen_error,
                "create-admin could not reopen setup after consume failure"
            );
        }
        let _ = txn.rollback().await;
        return Err(HttpError(
            AppError::internal("Failed to close setup window").with_message(
                "无法写入安装认领标记；管理员账户尚未提交，请检查数据目录权限后重试。",
            ),
        ));
    }

    txn.commit().await.map_err(|e| {
        tracing::error!("create-admin commit failed: {:?}", e);
        if let Err(error) = crate::api::setup_bootstrap::reopen_setup_after_failed_claim() {
            tracing::error!(
                %error,
                "create-admin could not clear the claimed marker after commit failure"
            );
        }
        HttpError(map_create_admin_insert_error(&e))
    })?;

    tracing::info!(
        "✅ Local admin account created: {} (ID: {}, is_owner=true)",
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
    crate::extract::Db(db): crate::extract::Db,
    Json(request): Json<LocalLoginRequest>,
) -> Result<impl IntoResponse, HttpError> {
    tracing::info!("Local login attempt: {}", request.username);

    // Query user by username
    // PR #4: 不再要求 auth_provider='local' — 只要 password_hash 存在就能本地登录。
    // 这样 GitHub-注册用户走 /api/auth/me/set-password 后也能用 username 登录。
    use sea_orm::Value as SeaValue;

    let query = "SELECT id, username, password_hash, is_admin,
                        COALESCE(is_owner, false) AS is_owner,
                        COALESCE(token_version, 0) AS token_version,
                        auth_provider, local_login_disabled
                 FROM users
                 WHERE LOWER(username) = LOWER($1) AND password_hash IS NOT NULL";

    let user_result = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            query,
            vec![SeaValue::String(Some(request.username.clone()))],
        ))
        .await
        .map_err(|_e| {
            HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Database error"})),
            ))
        })?;

    let user_row = match user_result {
        Some(row) => row,
        None => {
            // 执行虚拟 Argon2 验证以防止时序攻击枚举用户名
            let dummy_hash = "$argon2id$v=19$m=19456,t=2,p=1$dW5rbm93bnNhbHQ$dW5rbm93bmhhc2g";
            let _ = verify_password(&request.password, dummy_hash).await;
            tracing::warn!("User not found: {}", request.username);
            return Err(HttpError::from((
                StatusCode::UNAUTHORIZED,
                Json(json!({
                    "error": "Invalid credentials",
                    "message": "Username or password is incorrect"
                })),
            )));
        }
    };

    // Extract user data
    let user_id: i32 = user_row.try_get("", "id").map_err(|_| {
        HttpError::from((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Failed to read user data"})),
        ))
    })?;

    let username: String = user_row.try_get("", "username").map_err(|_| {
        HttpError::from((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Failed to read user data"})),
        ))
    })?;

    let password_hash: String = user_row.try_get("", "password_hash").map_err(|_| {
        HttpError::from((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Failed to read user data"})),
        ))
    })?;

    let is_admin: bool = user_row.try_get("", "is_admin").unwrap_or(false);
    let is_owner: bool = user_row.try_get("", "is_owner").unwrap_or(false);
    let token_version: i64 = user_row
        .try_get::<i32>("", "token_version")
        .ok()
        .map(i64::from)
        .or_else(|| user_row.try_get::<i64>("", "token_version").ok())
        .unwrap_or(0);

    let local_login_disabled: bool = user_row
        .try_get("", "local_login_disabled")
        .unwrap_or(false);

    // Check if local login is disabled
    if local_login_disabled {
        return Err(HttpError::from((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "Local login disabled",
                "message": "This account has been linked to GitHub. Please use GitHub OAuth to login."
            })),
        )));
    }

    // Verify password
    verify_password(&request.password, &password_hash).await?;

    // Update last login timestamp
    let _ = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE users SET last_login_at = CURRENT_TIMESTAMP WHERE id = $1",
            vec![SeaValue::Int(Some(user_id))],
        ))
        .await;

    let claims = mint_session_claims(user_id, &username, is_admin, is_owner, token_version);
    let token = encode_session_token(&claims).map_err(|_e| {
        HttpError::from((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Failed to create session token"})),
        ))
    })?;

    tracing::info!("✅ Local login successful: {}", username);

    // 设置 HttpOnly Cookie
    // 使用 SameSite=Lax 保持与 OAuth 登录一致
    // 使用 SiteConfig 判断是否为生产环境（基于 base_url 是否为 HTTPS）
    use crate::oauth_url_builder::SiteConfig;
    let is_production = SiteConfig::is_production().await;
    let cookie_value = auth_cookie_value(&token, is_production);

    let response = Json(AuthResponse {
        user: UserInfo {
            id: user_id,
            username,
            is_admin,
            is_owner,
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
/// Change password for local account.
///
/// Bumps `token_version` so other devices' JWTs fail closed, then re-issues a
/// fresh cookie for this browser so the current session is not harsh-kicked.
pub async fn change_password(
    crate::extract::Db(db): crate::extract::Db,
    headers: axum::http::HeaderMap,
    Json(request): Json<ChangePasswordRequest>,
) -> Result<impl IntoResponse, HttpError> {
    // Full auth (crypto + session epoch). Supports Authorization and HttpOnly cookie.
    let claims = crate::middleware::auth::authenticate_request(&headers, &db)
        .await
        .map_err(|_| {
            HttpError::from((
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Unauthorized", "message": "Invalid or missing token"})),
            ))
        })?;

    let user_id = claims.sub.parse::<i32>().map_err(|_| {
        HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Invalid user ID"})),
        ))
    })?;

    tracing::info!("Password change request for user ID: {}", user_id);

    // Validate new password
    validate_password(&request.new_password)?;

    // Query user data
    use sea_orm::Value as SeaValue;

    let query = "SELECT username, password_hash, auth_provider, local_login_disabled,
                        COALESCE(is_admin, false) AS is_admin,
                        COALESCE(is_owner, false) AS is_owner
                 FROM users
                 WHERE id = $1";

    let user_result = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            query,
            vec![SeaValue::Int(Some(user_id))],
        ))
        .await
        .map_err(|_e| {
            HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Database error"})),
            ))
        })?;

    let user_row = user_result.ok_or_else(|| {
        tracing::warn!("User not found: {}", user_id);
        HttpError::from((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "User not found"})),
        ))
    })?;

    let username: String = user_row.try_get("", "username").map_err(|_| {
        HttpError::from((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Failed to read user data"})),
        ))
    })?;

    let auth_provider: String = user_row.try_get("", "auth_provider").map_err(|_| {
        HttpError::from((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Failed to read user data"})),
        ))
    })?;

    // PR #4: 任何拥有 password_hash 的账户都能改密码（不再要求 auth_provider='local'）
    // 没有密码的账户（纯 OAuth）应走 /api/auth/me/set-password 后补密码。
    let _ = auth_provider; // 信息性字段，保留读取以兼容旧 SELECT

    let is_admin: bool = user_row.try_get("", "is_admin").unwrap_or(false);
    let is_owner: bool = user_row.try_get("", "is_owner").unwrap_or(false);

    let current_password_hash: Option<String> = user_row.try_get("", "password_hash").ok();
    let current_password_hash = match current_password_hash {
        Some(h) => h,
        None => {
            return Err(HttpError::from((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "No password set",
                    "message": "This account has no password yet. Use /api/auth/me/set-password instead."
                })),
            )));
        }
    };

    // Verify old password
    verify_password(&request.old_password, &current_password_hash).await?;

    // Hash new password
    let new_password_hash = hash_password(&request.new_password).await?;

    // Update password + bump session epoch so other sessions die immediately.
    let update_query = "UPDATE users SET password_hash = $1, \
                              token_version = COALESCE(token_version, 0) + 1, \
                              updated_at = CURRENT_TIMESTAMP \
                       WHERE id = $2 \
                       RETURNING token_version";

    let updated = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            update_query,
            vec![
                SeaValue::String(Some(new_password_hash)),
                SeaValue::Int(Some(user_id)),
            ],
        ))
        .await
        .map_err(|_e| {
            HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Failed to update password"})),
            ))
        })?
        .ok_or_else(|| {
            HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Failed to update password"})),
            ))
        })?;

    let new_tv: i64 = updated
        .try_get::<i32>("", "token_version")
        .ok()
        .map(i64::from)
        .or_else(|| updated.try_get::<i64>("", "token_version").ok())
        .unwrap_or(claims.tv + 1);

    if let Err(error) = notify_auth_cache_invalidation(&db, user_id).await {
        tracing::warn!(
            user_id,
            error = %error,
            "auth cache invalidation NOTIFY failed after password change"
        );
    }

    // Re-issue cookie for this browser (other devices keep the old tv → 401).
    let new_claims = mint_session_claims(user_id, &username, is_admin, is_owner, new_tv);
    let token = encode_session_token(&new_claims).map_err(|_e| {
        HttpError::from((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Failed to refresh session token"})),
        ))
    })?;

    use crate::oauth_url_builder::SiteConfig;
    let is_production = SiteConfig::is_production().await;
    let cookie_value = auth_cookie_value(&token, is_production);

    tracing::info!(
        "✅ Password changed successfully for user: {} (token_version → {})",
        username,
        new_tv
    );

    let mut response = Json(json!({
        "success": true,
        "message": "Password changed successfully"
    }))
    .into_response();
    if let Ok(value) = cookie_value.parse() {
        response.headers_mut().insert(header::SET_COOKIE, value);
    }
    Ok(response)
}

// Helper functions

/// Username charset: letters, digits, underscore (compiled once — MYR-036).
static USERNAME_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[a-zA-Z0-9_]+$").expect("username regex"));

/// Validate username format
fn validate_username(username: &str) -> Result<(), HttpError> {
    if username.len() < 3 || username.len() > 20 {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Invalid username",
                "message": "Username must be between 3 and 20 characters"
            })),
        )));
    }

    if !USERNAME_RE.is_match(username) {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Invalid username",
                "message": "Username can only contain letters, numbers, and underscores"
            })),
        )));
    }

    Ok(())
}

/// Validate password strength.
/// Length is **Unicode scalar count** (`chars().count()`), matching FE
/// `password.length` for BMP/emoji better than UTF-8 byte `len()`.
fn validate_password(password: &str) -> Result<(), HttpError> {
    let char_len = password.chars().count();
    if char_len < 8 {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Invalid password",
                "message": "Password must be at least 8 characters long"
            })),
        )));
    }
    if char_len > 128 {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Invalid password",
                "message": "Password must be at most 128 characters long"
            })),
        )));
    }

    // Check password complexity: must contain both letters and numbers
    let has_letter = password.chars().any(|c| c.is_alphabetic());
    let has_digit = password.chars().any(|c| c.is_numeric());

    if !has_letter || !has_digit {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Invalid password",
                "message": "Password must contain both letters and numbers for security"
            })),
        )));
    }

    Ok(())
}

/// Argon2id 是**故意**设计成慢且吃内存的（默认参数约 19 MiB / 数十毫秒）。
///
/// 直接在 async handler 里同步跑，等于在 Tokio worker 线程上阻塞几十毫秒 ——
/// 登录和注册都是公开端点，并发请求足以让整个 runtime 的调度停摆，连不相关
/// 的请求也被拖住。所有 Argon2 计算都必须挪到 blocking 线程池。
fn blocking_pool_error<T>(e: tokio::task::JoinError) -> Result<T, HttpError> {
    tracing::error!("Password hashing task failed: {:?}", e);
    Err(HttpError::from((
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"error": "Failed to process password"})),
    )))
}

/// Hash password using Argon2id (on the blocking pool, concurrency-capped).
async fn hash_password(password: &str) -> Result<String, HttpError> {
    let _permit = acquire_password_hash_permit().await?;
    let password = password.to_owned();
    let joined = tokio::task::spawn_blocking(move || {
        let salt = SaltString::generate(&mut OsRng);
        let argon2 = Argon2::default();
        argon2
            .hash_password(password.as_bytes(), &salt)
            .map(|h| h.to_string())
    })
    .await;

    match joined {
        Ok(Ok(hash)) => Ok(hash),
        Ok(Err(e)) => {
            tracing::error!("Failed to hash password: {:?}", e);
            Err(HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Failed to process password"})),
            )))
        }
        Err(e) => blocking_pool_error(e),
    }
}

/// Verify password against hash (on the blocking pool, concurrency-capped).
async fn verify_password(password: &str, hash: &str) -> Result<(), HttpError> {
    let _permit = acquire_password_hash_permit().await?;
    let password = password.to_owned();
    let hash = hash.to_owned();

    // Discriminate auth failure vs hash-parse failure without pattern-matching HttpError.
    #[derive(Clone, Copy)]
    enum VerifyFail {
        Unauthorized,
        Internal,
    }

    let joined = tokio::task::spawn_blocking(move || {
        let parsed_hash = PasswordHash::new(&hash).map_err(|e| {
            tracing::error!("Failed to parse password hash: {:?}", e);
            VerifyFail::Internal
        })?;
        Argon2::default()
            .verify_password(password.as_bytes(), &parsed_hash)
            .map_err(|_| VerifyFail::Unauthorized)
    })
    .await;

    match joined {
        Ok(Ok(())) => Ok(()),
        Ok(Err(VerifyFail::Unauthorized)) => {
            tracing::warn!("Password verification failed");
            Err(HttpError::from((
                StatusCode::UNAUTHORIZED,
                Json(json!({
                    "error": "Invalid credentials",
                    "message": "Username or password is incorrect"
                })),
            )))
        }
        Ok(Err(VerifyFail::Internal)) => Err(HttpError::from((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Failed to verify password"})),
        ))),
        Err(e) => blocking_pool_error(e),
    }
}

// PR #4 新增端点：公开注册 + 后补密码 + 本地登录开关
// 详见 docs/development/OAUTH.md

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
    crate::extract::Db(db): crate::extract::Db,
    axum::extract::State(dynamic_config): axum::extract::State<
        std::sync::Arc<tokio::sync::RwLock<crate::config::DynamicConfig>>,
    >,
    Json(req): Json<RegisterRequest>,
) -> Result<impl IntoResponse, HttpError> {
    // 开关检查（AppState.dynamic_config，与 GLOBAL_* 同 Arc）
    {
        let cfg = dynamic_config.read().await;
        if !cfg.allow_local_registration {
            return Err(HttpError::from((
                StatusCode::FORBIDDEN,
                Json(json!({
                    "error": "Registration disabled",
                    "message": "Public registration is disabled. Ask an administrator to create an account."
                })),
            )));
        }
    }
    match crate::services::site_owner::installation_has_owner(&db).await {
        Ok(true) => {}
        Ok(false) => {
            return Err(HttpError::from((
                StatusCode::FORBIDDEN,
                Json(json!({
                    "error": "setup_required",
                    "message": "Finish the setup wizard before creating an account."
                })),
            )));
        }
        Err(error) => {
            tracing::error!(error = %error, "register: failed to read installation claim");
            return Err(HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Database error"})),
            )));
        }
    }

    validate_username(&req.username)?;
    validate_password(&req.password)?;

    use sea_orm::Value as SeaValue;

    // username 冲突检查（大小写不敏感）
    let dup = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT 1 FROM users WHERE LOWER(username) = LOWER($1) LIMIT 1",
            vec![SeaValue::String(Some(req.username.clone()))],
        ))
        .await
        .map_err(|_e| {
            HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Database error"})),
            ))
        })?;
    if dup.is_some() {
        return Err(HttpError::from((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Username taken",
                "message": "This username is already in use"
            })),
        )));
    }

    let password_hash = hash_password(&req.password).await?;

    let insert = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO users (username, email, password_hash, auth_provider, is_admin, \
                                 avatar_url, created_at, updated_at, last_login_at) \
             VALUES ($1, $2, $3, 'local', false, $4, NOW(), NOW(), NOW()) \
             RETURNING id",
            vec![
                SeaValue::String(Some(req.username.clone())),
                req.email
                    .clone()
                    .map(|s| SeaValue::String(Some(s)))
                    .unwrap_or(SeaValue::String(None)),
                SeaValue::String(Some(password_hash)),
                // 占位头像退成显示兜底，不落库（见 create_owner 处说明）
                SeaValue::String(None),
            ],
        ))
        .await
        .map_err(|_e| {
            HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Failed to create account"})),
            ))
        })?
        .ok_or_else(|| {
            HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Insert returned no row"})),
            ))
        })?;

    let user_id: i32 = insert.try_get("", "id").map_err(|_| {
        HttpError::from((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Failed to read new user id"})),
        ))
    })?;

    tracing::info!("✅ Public registration: {} (id={})", req.username, user_id);

    // 注册即登录：颁发 JWT + cookie
    issue_session_cookie(&db, user_id, &req.username, false, false).await
}

/// POST /api/auth/me/set-password —— GitHub-only 用户后补密码
///
/// 要求当前账户**没有**密码（已有密码走 `change_password`）。
#[derive(Debug, Deserialize)]
pub struct SetPasswordRequest {
    pub new_password: String,
}

pub async fn set_password(
    crate::extract::Db(db): crate::extract::Db,
    headers: axum::http::HeaderMap,
    Json(req): Json<SetPasswordRequest>,
) -> Result<impl IntoResponse, HttpError> {
    use sea_orm::Value as SeaValue;

    let claims = crate::middleware::auth::authenticate_request(&headers, &db)
        .await
        .map_err(|_| {
            HttpError::from((
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Unauthorized"})),
            ))
        })?;
    let user_id: i32 = claims.sub.parse().map_err(|_| {
        HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Invalid user id"})),
        ))
    })?;

    validate_password(&req.new_password)?;

    // 必须当前 password_hash 为 NULL
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT password_hash IS NOT NULL AS has_password FROM users WHERE id = $1",
            vec![SeaValue::Int(Some(user_id))],
        ))
        .await
        .map_err(|_e| {
            HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Database error"})),
            ))
        })?
        .ok_or_else(|| {
            HttpError::from((
                StatusCode::NOT_FOUND,
                Json(json!({"error": "User not found"})),
            ))
        })?;
    let has_password: bool = row.try_get("", "has_password").unwrap_or(false);
    if has_password {
        return Err(HttpError::from((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Password already set",
                "message": "Use POST /api/auth/change-password to change an existing password."
            })),
        )));
    }

    let hash = hash_password(&req.new_password).await?;

    let updated = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE users SET password_hash = $1, \
                               token_version = COALESCE(token_version, 0) + 1, \
                               updated_at = NOW() \
                        WHERE id = $2 \
                        RETURNING token_version",
            vec![SeaValue::String(Some(hash)), SeaValue::Int(Some(user_id))],
        ))
        .await
        .map_err(|_e| {
            HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Failed to set password"})),
            ))
        })?;
    let new_tv = updated
        .as_ref()
        .and_then(|row| {
            row.try_get::<i32>("", "token_version")
                .ok()
                .map(i64::from)
                .or_else(|| row.try_get::<i64>("", "token_version").ok())
        })
        .ok_or_else(|| {
            HttpError::from((
                StatusCode::NOT_FOUND,
                Json(json!({"error": "User not found"})),
            ))
        })?;
    if let Err(error) = notify_auth_cache_invalidation(&db, user_id).await {
        tracing::warn!(
            user_id,
            error = %error,
            "auth cache invalidation NOTIFY failed after password set"
        );
    }

    // Re-issue this browser's cookie at the new epoch so setting the first
    // password does not immediately eject the active OAuth session. Other
    // devices retain the old epoch and fail closed on their next request.
    let new_claims = mint_session_claims(
        user_id,
        claims.username.clone(),
        claims.is_admin,
        claims.is_owner,
        new_tv,
    );
    let token = encode_session_token(&new_claims).map_err(|_| {
        HttpError::from((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Failed to refresh session token"})),
        ))
    })?;
    let is_production = crate::oauth_url_builder::SiteConfig::is_production().await;
    let mut response = Json(json!({"success": true})).into_response();
    if let Ok(value) = auth_cookie_value(&token, is_production).parse() {
        response.headers_mut().insert(header::SET_COOKIE, value);
    }
    Ok(response)
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
    crate::extract::Db(db): crate::extract::Db,
    headers: axum::http::HeaderMap,
    Json(req): Json<LocalLoginToggleRequest>,
) -> Result<Json<Value>, HttpError> {
    use sea_orm::Value as SeaValue;

    let claims = crate::middleware::auth::authenticate_request(&headers, &db)
        .await
        .map_err(|_| {
            HttpError::from((
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Unauthorized"})),
            ))
        })?;
    let user_id: i32 = claims.sub.parse().map_err(|_| {
        HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Invalid user id"})),
        ))
    })?;

    // 取当前状态
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT password_hash IS NOT NULL AS has_password, \
                    (SELECT COUNT(*) FROM user_identities WHERE user_id = users.id) AS identity_count \
             FROM users WHERE id = $1",
            vec![SeaValue::Int(Some(user_id))],
        ))
        .await
        .map_err(|_e| {
            HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Database error"})),
            ))
        })?
        .ok_or_else(|| {
            HttpError::from((
                StatusCode::NOT_FOUND,
                Json(json!({"error": "User not found"})),
            ))
        })?;

    let has_password: bool = row.try_get("", "has_password").unwrap_or(false);
    let identity_count: i64 = row.try_get("", "identity_count").unwrap_or(0);

    // 前置检查
    if req.enabled {
        if !has_password {
            return Err(HttpError::from((
                StatusCode::CONFLICT,
                Json(json!({
                    "error": "No password set",
                    "message": "Set a password via /api/auth/me/set-password before enabling local login."
                })),
            )));
        }
    } else if identity_count == 0 {
        return Err(HttpError::from((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Cannot disable last login method",
                "message": "Link at least one OAuth provider before disabling local login."
            })),
        )));
    }

    let disabled = !req.enabled;
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE users SET local_login_disabled = $1, updated_at = NOW() WHERE id = $2",
        vec![SeaValue::Bool(Some(disabled)), SeaValue::Int(Some(user_id))],
    ))
    .await
    .map_err(|_e| {
        HttpError::from((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Failed to update"})),
        ))
    })?;

    Ok(Json(json!({"success": true, "enabled": req.enabled})))
}

/// 内部：给指定 user 颁发 JWT + 设置 cookie，返回 AuthResponse + Set-Cookie
///
/// `token_version` is read from the user row (defaults 0) so the mint matches
/// the session epoch checked by auth middleware.
async fn issue_session_cookie(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    username: &str,
    is_admin: bool,
    is_owner: bool,
) -> Result<axum::response::Response, HttpError> {
    use sea_orm::Value as SeaValue;

    let token_version = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT COALESCE(token_version, 0) AS token_version FROM users WHERE id = $1",
            vec![SeaValue::Int(Some(user_id))],
        ))
        .await
        .ok()
        .flatten()
        .and_then(|r| {
            r.try_get::<i32>("", "token_version")
                .ok()
                .map(i64::from)
                .or_else(|| r.try_get::<i64>("", "token_version").ok())
        })
        .unwrap_or(0);

    let claims = mint_session_claims(user_id, username, is_admin, is_owner, token_version);
    let token = encode_session_token(&claims).map_err(|_e| {
        HttpError::from((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Failed to create session token"})),
        ))
    })?;

    use crate::oauth_url_builder::SiteConfig;
    let is_production = SiteConfig::is_production().await;
    let cookie_value = auth_cookie_value(&token, is_production);

    let body = Json(AuthResponse {
        user: UserInfo {
            id: user_id,
            username: username.to_string(),
            is_admin,
            is_owner,
            auth_provider: "local".to_string(),
        },
    });
    let mut resp = body.into_response();
    resp.headers_mut()
        .insert(header::SET_COOKIE, cookie_value.parse().unwrap());
    Ok(resp)
}

// PR #6: Admin 后台建本地账号
// 详见 docs/development/OAUTH.md
//
// 不受 allow_local_registration 开关限制；is_admin=true 仅站点 owner 可设。

#[derive(Debug, Deserialize)]
pub struct AdminCreateUserRequest {
    pub username: String,
    pub password: String,
    pub email: Option<String>,
    #[serde(default)]
    pub is_admin: bool,
}

pub async fn admin_create_user(
    crate::extract::Db(db): crate::extract::Db,
    headers: axum::http::HeaderMap,
    Json(req): Json<AdminCreateUserRequest>,
) -> Result<Json<Value>, HttpError> {
    let claims = crate::middleware::auth::authenticate_request(&headers, &db)
        .await
        .map_err(|_| {
            HttpError::from((
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Unauthorized"})),
            ))
        })?;
    ensure_current_admin_on(&claims, &db).await?;

    let actor_id: i32 = claims.sub.parse().unwrap_or(0);
    // 仅站点 owner 可创建带 is_admin=true 的账号（was: actor id=1）
    let actor_is_owner = crate::api::admin_users::actor_is_owner(&db, actor_id).await?;
    if let Some(msg) =
        crate::api::admin_users::non_owner_grant_admin_on_create_error(actor_is_owner, req.is_admin)
    {
        return Err(HttpError::from((
            StatusCode::FORBIDDEN,
            Json(json!({"error": msg})),
        )));
    }

    validate_username(&req.username)?;
    validate_password(&req.password)?;

    use sea_orm::Value as SeaValue;

    let dup = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT 1 FROM users WHERE LOWER(username) = LOWER($1) LIMIT 1",
            vec![SeaValue::String(Some(req.username.clone()))],
        ))
        .await
        .map_err(|_e| {
            HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Database error"})),
            ))
        })?;
    if dup.is_some() {
        return Err(HttpError::from((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Username taken",
                "message": "This username is already in use"
            })),
        )));
    }

    let password_hash = hash_password(&req.password).await?;
    // 非 owner 路径上 is_admin 必为 false（上方已校验）
    let create_as_admin = req.is_admin;

    let insert = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO users (username, email, password_hash, auth_provider, is_admin, \
                                 avatar_url, created_at, updated_at) \
             VALUES ($1, $2, $3, 'local', $4, $5, NOW(), NOW()) \
             RETURNING id",
            vec![
                SeaValue::String(Some(req.username.clone())),
                req.email
                    .clone()
                    .map(|s| SeaValue::String(Some(s)))
                    .unwrap_or(SeaValue::String(None)),
                SeaValue::String(Some(password_hash)),
                SeaValue::Bool(Some(create_as_admin)),
                // 占位头像退成显示兜底，不落库（见 create_owner 处说明）
                SeaValue::String(None),
            ],
        ))
        .await
        .map_err(|_e| {
            HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Failed to create account"})),
            ))
        })?
        .ok_or_else(|| {
            HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Insert returned no row"})),
            ))
        })?;

    let user_id: i32 = insert.try_get("", "id").map_err(|_| {
        HttpError::from((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Failed to read new id"})),
        ))
    })?;

    tracing::info!(
        "✅ Admin {} created account: {} (id={}, is_admin={})",
        claims.username,
        req.username,
        user_id,
        create_as_admin
    );

    let mut body = json!({
        "success": true,
        "user_id": user_id,
        "username": req.username,
        "is_admin": create_as_admin,
    });
    if create_as_admin {
        body["notice"] = json!(crate::api::admin_users::PROMOTE_RELOGIN_NOTICE);
        body["message"] = json!(crate::api::admin_users::PROMOTE_RELOGIN_NOTICE);
    }
    Ok(Json(body))
}

// admin 用户列表已迁移到 api::admin_users::list_users（设置页用户管理模块）

#[cfg(test)]
mod tests {
    use super::{
        acquire_password_hash_permit_from, admin_already_exists_error, create_admin_gate,
        hash_password, map_create_admin_insert_error, verify_password, AuthResponse,
        CreateAdminRequest, UserInfo, CREATE_ADMIN_ADVISORY_LOCK_KEY,
        PASSWORD_HASH_ACQUIRE_TIMEOUT, PASSWORD_HASH_PERMITS,
    };
    use crate::error::{app_error_response, HttpError};
    use axum::body::to_bytes;
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use std::sync::Arc;
    use std::time::Duration as StdDuration;
    use tokio::sync::Semaphore;

    #[test]
    fn auth_response_never_serializes_the_jwt() {
        let response = AuthResponse {
            user: UserInfo {
                id: 7,
                username: "alice".to_string(),
                is_admin: false,
                is_owner: true,
                auth_provider: "local".to_string(),
            },
        };
        let serialized = serde_json::to_value(response).unwrap();

        assert!(serialized.get("token").is_none());
        assert_eq!(serialized["user"]["id"], 7);
        assert_eq!(serialized["user"]["is_owner"], true);
        assert_eq!(serialized["user"]["is_admin"], false);
    }

    #[test]
    fn create_admin_gate_rejects_when_admin_exists() {
        let err = create_admin_gate(true).expect_err("must reject");
        assert_eq!(err.status_u16(), 409);
        assert_eq!(err.error_label(), "Admin account already exists");
        let json = err.to_json();
        assert!(
            json["message"]
                .as_str()
                .unwrap_or("")
                .contains("Setup has already been completed"),
            "{json}"
        );
    }

    #[test]
    fn create_admin_gate_allows_first_admin() {
        assert!(create_admin_gate(false).is_ok());
    }

    #[test]
    fn create_admin_request_accepts_setup_secret() {
        let v: CreateAdminRequest = serde_json::from_value(serde_json::json!({
            "username": "owner",
            "password": "hunter2ab",
            "setup_secret": "phrase-from-env"
        }))
        .unwrap();
        assert_eq!(v.setup_secret.as_deref(), Some("phrase-from-env"));

        let legacy: CreateAdminRequest = serde_json::from_value(serde_json::json!({
            "username": "owner",
            "password": "hunter2ab"
        }))
        .unwrap();
        assert_eq!(legacy.setup_secret, None);
    }

    #[test]
    fn admin_already_exists_error_is_stable_conflict() {
        let e = admin_already_exists_error();
        assert_eq!(e.status_u16(), 409);
        assert_eq!(e.error_label(), "Admin account already exists");
        // Two constructions must yield identical public JSON (client contract).
        assert_eq!(e.to_json(), admin_already_exists_error().to_json());
    }

    #[test]
    fn map_insert_error_unique_violation_is_409() {
        let cases = [
            "error returned from database: 23505 duplicate key value violates unique constraint \"idx_users_username_unique\"",
            "duplicate key value violates unique constraint \"idx_users_single_owner\"",
            "UNIQUE constraint failed: users.username",
        ];
        for msg in cases {
            let e = map_create_admin_insert_error(&msg);
            assert_eq!(e.status_u16(), 409, "msg={msg}");
            assert_eq!(e.error_label(), "Admin account already exists");
        }
    }

    #[test]
    fn map_insert_error_other_is_500() {
        let e = map_create_admin_insert_error(&"connection reset by peer");
        assert_eq!(e.status_u16(), 500);
        assert_eq!(e.error_label(), "Failed to create admin account");
    }

    #[test]
    fn advisory_lock_key_is_stable_nonzero() {
        // Changing this key would allow concurrent create-admin across deploys
        // that disagree on the constant — pin it.
        assert_eq!(CREATE_ADMIN_ADVISORY_LOCK_KEY, 0x4D59_5249_4144_0001);
        assert_ne!(CREATE_ADMIN_ADVISORY_LOCK_KEY, 0);
    }

    #[tokio::test]
    async fn create_admin_conflict_http_response_is_409_json() {
        let resp = HttpError(admin_already_exists_error()).into_response();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let bytes = to_bytes(resp.into_body(), 64 * 1024).await.expect("body");
        let v: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(v["error"], "Admin account already exists");
        assert!(v["message"].as_str().unwrap().contains("Setup has already"));
    }

    #[tokio::test]
    async fn app_error_response_matches_http_error_for_conflict() {
        let err = admin_already_exists_error();
        let a = app_error_response(err.clone());
        let b = HttpError(err).into_response();
        assert_eq!(a.status(), b.status());
        assert_eq!(a.status(), StatusCode::CONFLICT);
    }

    #[test]
    fn password_hash_permit_budget_is_modest() {
        // MYR-006: cap concurrency ~4; leave headroom — not a harsh multi-axis governor.
        assert_eq!(PASSWORD_HASH_PERMITS, 4);
        assert!(PASSWORD_HASH_ACQUIRE_TIMEOUT >= StdDuration::from_secs(5));
        assert!(PASSWORD_HASH_ACQUIRE_TIMEOUT <= StdDuration::from_secs(30));
    }

    #[tokio::test]
    async fn password_hash_permit_returns_503_when_saturated() {
        // Local semaphore mirrors production so we do not starve parallel Argon2 tests
        // that share the process-wide permit pool.
        let sem = Arc::new(Semaphore::new(PASSWORD_HASH_PERMITS));
        let mut held = Vec::with_capacity(PASSWORD_HASH_PERMITS);
        for _ in 0..PASSWORD_HASH_PERMITS {
            held.push(sem.clone().acquire_owned().await.expect("permit"));
        }
        assert_eq!(sem.available_permits(), 0);

        let err = acquire_password_hash_permit_from(
            Arc::clone(&sem),
            StdDuration::from_millis(40),
            PASSWORD_HASH_PERMITS,
        )
        .await
        .expect_err("must 503 when all Argon2 permits are held");
        assert_eq!(err.0.status_u16(), 503);
        assert_eq!(err.0.error_label(), "Server busy");
        let json = err.0.to_json();
        assert!(
            json["message"]
                .as_str()
                .unwrap_or("")
                .contains("password operations"),
            "{json}"
        );

        drop(held);
        let permit = acquire_password_hash_permit_from(
            Arc::clone(&sem),
            StdDuration::from_millis(200),
            PASSWORD_HASH_PERMITS,
        )
        .await
        .expect("permit available after release");
        drop(permit);
    }

    #[tokio::test]
    async fn hash_and_verify_share_permit_path() {
        // Round-trip proves both helpers take a permit and run Argon2 on the blocking pool.
        let hash = hash_password("CorrectHorseBattery1").await.expect("hash");
        assert!(
            hash.starts_with("$argon2"),
            "expected argon2id PHC string, got {hash}"
        );
        verify_password("CorrectHorseBattery1", &hash)
            .await
            .expect("verify ok");
        let bad = verify_password("wrong-password-99", &hash)
            .await
            .expect_err("wrong password");
        assert_eq!(bad.0.status_u16(), 401);
    }
}
