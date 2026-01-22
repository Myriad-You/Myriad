use axum::{
    extract::{Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Redirect},
    Json,
};
use chrono::{Duration, Utc};
use jsonwebtoken::{encode, EncodingKey, Header};
use oauth2::{
    basic::BasicClient, reqwest::async_http_client, AuthUrl, AuthorizationCode, ClientId,
    ClientSecret, CsrfToken, RedirectUrl, Scope, TokenResponse, TokenUrl,
};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::env;
use std::sync::Arc;
use std::time::{Duration as StdDuration, Instant};
use tokio::sync::RwLock;

use crate::oauth_url_builder::OAuthUrlBuilder;

// ✅ 安全修复 P0: OAuth CSRF State 存储
// 存储 OAuth state 参数，防止 CSRF 攻击
#[derive(Debug, Clone)]
struct OAuthState {
    #[allow(dead_code)] // 用于调试时查看
    state: String,
    created_at: Instant,
    purpose: OAuthPurpose, // 区分 login 和 link_account
    user_id: Option<i32>,  // LinkAccount 时存储当前登录用户的 ID
}

#[derive(Debug, Clone, PartialEq)]
enum OAuthPurpose {
    Login,
    LinkAccount,
}

// 全局 OAuth State 存储（内存存储，生产环境建议使用 Redis）
static OAUTH_STATES: once_cell::sync::Lazy<Arc<RwLock<HashMap<String, OAuthState>>>> =
    once_cell::sync::Lazy::new(|| {
        let store: Arc<RwLock<HashMap<String, OAuthState>>> = Arc::new(RwLock::new(HashMap::new()));
        let store_clone = store.clone();

        // 启动清理任务：每 60 秒清理过期的 state（有效期 10 分钟）
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(StdDuration::from_secs(60));
            loop {
                interval.tick().await;
                let mut states = store_clone.write().await;
                let now = Instant::now();
                let before_count = states.len();
                states.retain(|_, oauth_state| {
                    now.duration_since(oauth_state.created_at) < StdDuration::from_secs(600)
                });
                let removed = before_count - states.len();
                if removed > 0 {
                    tracing::debug!(
                        "🧹 OAuth state cleanup: removed {} expired states, {} remaining",
                        removed,
                        states.len()
                    );
                }
            }
        });

        store
    });

// JWT Claims structure
#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,      // User ID
    pub username: String, // GitHub username
    pub is_admin: bool,   // ✅ 安全修复 P0: Admin status from database
    pub exp: i64,         // Expiration time
    pub iat: i64,         // Issued at
}

// GitHub OAuth callback query parameters
#[derive(Debug, Deserialize)]
pub struct AuthCallbackQuery {
    code: String,
    #[allow(dead_code)]
    state: String,
}

// GitHub user info from API
#[derive(Debug, Deserialize)]
struct GitHubUser {
    #[allow(dead_code)]
    id: i64,
    login: String,
    #[allow(dead_code)]
    name: Option<String>,
    #[allow(dead_code)]
    email: Option<String>,
    #[allow(dead_code)]
    avatar_url: String,
    #[allow(dead_code)]
    html_url: String,
    #[allow(dead_code)]
    bio: Option<String>,
    #[allow(dead_code)]
    location: Option<String>,
    #[allow(dead_code)]
    company: Option<String>,
}

// User entity (simplified, you should use SeaORM entities)
#[derive(Debug, Serialize)]
#[allow(dead_code)]
pub struct User {
    pub id: i32,
    pub github_id: i64,
    pub username: String,
    pub display_name: Option<String>,
    pub email: Option<String>,
    pub avatar_url: String,
}

/// GET /api/auth/github/login
/// Redirect user to GitHub OAuth page
/// 支持动态环境检测，自动适配开发/生产环境
pub async fn github_login(
    _headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, Json<Value>)> {
    // 使用新的统一 OAuth 配置获取（支持数据库 + 环境变量 + 自动推断）
    let oauth_config = OAuthUrlBuilder::get_github_oauth_config()
        .await
        .map_err(|e| {
            tracing::error!("GitHub OAuth not configured: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "GitHub OAuth not configured",
                    "message": e
                })),
            )
        })?;

    tracing::info!(
        "🔐 GitHub OAuth login initiated with redirect URL: {}",
        oauth_config.redirect_url
    );

    let client = BasicClient::new(
        ClientId::new(oauth_config.client_id),
        Some(ClientSecret::new(oauth_config.client_secret)),
        AuthUrl::new("https://github.com/login/oauth/authorize".to_string()).unwrap(),
        Some(TokenUrl::new("https://github.com/login/oauth/access_token".to_string()).unwrap()),
    )
    .set_redirect_uri(RedirectUrl::new(oauth_config.redirect_url).unwrap());

    let (auth_url, csrf_token) = client
        .authorize_url(CsrfToken::new_random)
        .add_scope(Scope::new("read:user".to_string()))
        .add_scope(Scope::new("user:email".to_string()))
        .url();

    // ✅ 安全修复 P0: 存储 OAuth state，防止 CSRF 攻击
    let state_value = csrf_token.secret().to_string();
    {
        let mut states = OAUTH_STATES.write().await;
        // 限制存储大小，防止内存耗尽攻击
        const MAX_OAUTH_STATES: usize = 10000;
        if states.len() >= MAX_OAUTH_STATES {
            // 删除最旧的 state
            if let Some(oldest_key) = states
                .iter()
                .min_by_key(|(_, v)| v.created_at)
                .map(|(k, _)| k.clone())
            {
                states.remove(&oldest_key);
            }
        }
        states.insert(
            state_value.clone(),
            OAuthState {
                state: state_value,
                created_at: Instant::now(),
                purpose: OAuthPurpose::Login,
                user_id: None, // Login 流程不需要 user_id
            },
        );
        tracing::debug!("🔐 OAuth state stored (total: {})", states.len());
    }

    Ok(Redirect::to(auth_url.as_str()))
}

/// GET /api/auth/github/callback
/// Handle GitHub OAuth callback
/// 支持动态环境检测，自动适配开发/生产环境
/// 支持两种流程：Login（登录）和 LinkAccount（绑定账户）
pub async fn github_callback(
    Query(params): Query<AuthCallbackQuery>,
    State(_db): State<DatabaseConnection>,
    _headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, Json<Value>)> {
    tracing::info!("🔐 GitHub OAuth callback received");

    // ✅ 安全修复 P0: 验证 OAuth CSRF state，防止 CSRF 攻击
    let state_value = &params.state;
    let oauth_state = {
        let mut states = OAUTH_STATES.write().await;
        states.remove(state_value) // 一次性使用，验证后立即删除
    };

    let stored_state = match oauth_state {
        Some(stored_state) => {
            // 检查是否过期（10分钟有效期）
            if Instant::now().duration_since(stored_state.created_at) > StdDuration::from_secs(600)
            {
                tracing::warn!("🚨 OAuth state expired");
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": "OAuth state expired",
                        "message": "Session expired. Please try again."
                    })),
                ));
            }
            tracing::debug!(
                "✅ OAuth state verified, purpose: {:?}",
                stored_state.purpose
            );
            stored_state
        }
        None => {
            tracing::warn!("🚨 OAuth CSRF check failed: state not found or already used");
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "Invalid OAuth state",
                    "message": "OAuth state invalid or expired. This may be a CSRF attack. Please try again."
                })),
            ));
        }
    };

    // 使用新的统一 OAuth 配置获取（支持数据库 + 环境变量 + 自动推断）
    let oauth_config = OAuthUrlBuilder::get_github_oauth_config()
        .await
        .map_err(|e| {
            tracing::error!("GitHub OAuth not configured: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "GitHub OAuth not configured", "message": e})),
            )
        })?;

    let frontend_url = OAuthUrlBuilder::get_frontend_url().await;

    tracing::info!(
        "🔐 OAuth callback - redirect_url: {}, frontend_url: {}",
        oauth_config.redirect_url,
        frontend_url
    );

    let client = BasicClient::new(
        ClientId::new(oauth_config.client_id),
        Some(ClientSecret::new(oauth_config.client_secret)),
        AuthUrl::new("https://github.com/login/oauth/authorize".to_string()).unwrap(),
        Some(TokenUrl::new("https://github.com/login/oauth/access_token".to_string()).unwrap()),
    )
    .set_redirect_uri(RedirectUrl::new(oauth_config.redirect_url).unwrap());

    // Exchange code for token
    let token_result = client
        .exchange_code(AuthorizationCode::new(params.code))
        .request_async(async_http_client)
        .await
        .map_err(|e| {
            tracing::error!("Failed to exchange code: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Failed to exchange authorization code"})),
            )
        })?;

    let access_token = token_result.access_token().secret();
    tracing::info!("✅ Access token obtained");

    // Get user info from GitHub
    let http_client = crate::services::http_client::get_global_client().await;
    let user_url = format!(
        "{}/user",
        crate::services::http_client::GitHubApiUrl::get_api_base().await
    );
    let user_info: GitHubUser = http_client
        .get(user_url)
        .header("Authorization", format!("Bearer {}", access_token))
        .header("User-Agent", "Myriad-App")
        .send()
        .await
        .map_err(|e| {
            tracing::error!("Failed to get user info: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Failed to get user info from GitHub"})),
            )
        })?
        .json()
        .await
        .map_err(|e| {
            tracing::error!("Failed to parse user info: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Failed to parse GitHub user info"})),
            )
        })?;

    tracing::info!("✅ GitHub user info: {}", user_info.login);

    // TODO: Use SeaORM entities here
    // For now, we'll use raw SQL queries via SeaORM
    use sea_orm::Value as SeaValue;

    // ========== LinkAccount 流程处理 ==========
    // 如果是账户绑定流程，执行绑定逻辑并返回
    if stored_state.purpose == OAuthPurpose::LinkAccount {
        let admin_user_id = stored_state.user_id.ok_or_else(|| {
            tracing::error!("LinkAccount state missing user_id");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Invalid link state"})),
            )
        })?;

        tracing::info!(
            "🔗 Processing LinkAccount: admin_id={}, github_login={}",
            admin_user_id,
            user_info.login
        );

        // 验证用户仍然是本地管理员
        let admin_check = _db
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT id, username, auth_provider, is_admin FROM users
                 WHERE id = $1 AND auth_provider = 'local' AND is_admin = true",
                vec![SeaValue::Int(Some(admin_user_id))],
            ))
            .await
            .map_err(|e| {
                tracing::error!("Failed to verify admin: {:?}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": "Database error"})),
                )
            })?;

        if admin_check.is_none() {
            tracing::warn!("Admin user {} not found or not admin", admin_user_id);
            let redirect_url = format!("{}/?link=error&reason=not_admin", frontend_url);
            return Ok(Redirect::to(&redirect_url).into_response());
        }

        // 检查此 GitHub 账户是否已被其他用户绑定
        let existing_link = _db
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT id FROM users WHERE linked_github_id = $1 AND id != $2",
                vec![
                    SeaValue::BigInt(Some(user_info.id)),
                    SeaValue::Int(Some(admin_user_id)),
                ],
            ))
            .await
            .map_err(|e| {
                tracing::error!("Failed to check existing link: {:?}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": "Database error"})),
                )
            })?;

        if existing_link.is_some() {
            tracing::warn!(
                "GitHub account {} already linked to another user",
                user_info.id
            );
            let redirect_url = format!("{}/?link=error&reason=already_linked", frontend_url);
            return Ok(Redirect::to(&redirect_url).into_response());
        }

        // 先删除可能存在的独立 GitHub 用户记录（避免冲突）
        let delete_result = _db
            .execute(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "DELETE FROM users WHERE github_id = $1 AND auth_provider = 'github'",
                vec![SeaValue::BigInt(Some(user_info.id))],
            ))
            .await;

        if let Ok(result) = delete_result {
            if result.rows_affected() > 0 {
                tracing::info!(
                    "🗑️  Deleted existing GitHub user record (github_id: {}) before linking to admin",
                    user_info.id
                );
            }
        }

        // 执行绑定：更新管理员账户
        _db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE users SET
                linked_github_id = $1,
                local_login_disabled = true,
                avatar_url = $2,
                updated_at = CURRENT_TIMESTAMP
             WHERE id = $3",
            vec![
                SeaValue::BigInt(Some(user_info.id)),
                SeaValue::String(Some(Box::new(user_info.avatar_url.clone()))),
                SeaValue::Int(Some(admin_user_id)),
            ],
        ))
        .await
        .map_err(|e| {
            tracing::error!("Failed to link GitHub account: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Failed to link GitHub account"})),
            )
        })?;

        tracing::info!(
            "✅ GitHub account linked successfully: admin_id={} -> github_login={} (github_id: {})",
            admin_user_id,
            user_info.login,
            user_info.id
        );

        // 重定向到前端，带成功参数
        let redirect_url = format!(
            "{}/?link=success&github_username={}",
            frontend_url,
            urlencoding::encode(&user_info.login)
        );
        return Ok(Redirect::to(&redirect_url).into_response());
    }

    // ========== Login 流程处理 ==========
    // 账户处理策略：
    // 1. 检查是否有管理员已绑定此 GitHub ID (linked_github_id) -> 使用管理员账户登录
    // 2. 检查是否有 GitHub 用户已存在 (github_id) -> 更新并使用该用户
    // 3. 否则 -> 创建新的普通 GitHub 用户

    // Step 1: 检查是否有本地管理员已绑定此 GitHub ID
    let linked_admin_check = _db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id, username, is_admin FROM users
             WHERE auth_provider = 'local'
             AND linked_github_id = $1
             LIMIT 1",
            vec![SeaValue::BigInt(Some(user_info.id))],
        ))
        .await
        .map_err(|e| {
            tracing::error!("Failed to check for linked admin: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Database error"})),
            )
        })?;

    // Step 2: 检查该 GitHub ID 是否已作为独立用户存在
    let github_user_check = _db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id, username FROM users WHERE github_id = $1",
            vec![SeaValue::BigInt(Some(user_info.id))],
        ))
        .await
        .map_err(|e| {
            tracing::error!("Failed to check GitHub user: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Database error"})),
            )
        })?;

    // 决定处理策略
    let user_id = if let Some(admin_row) = linked_admin_check {
        // 场景 1: 管理员已绑定此 GitHub 账户 -> 使用管理员账户登录
        let admin_id: i32 = admin_row.try_get("", "id").map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Failed to read admin data"})),
            )
        })?;
        let admin_username: String = admin_row
            .try_get("", "username")
            .unwrap_or_else(|_| "admin".to_string());

        tracing::info!(
            "✅ GitHub account {} is linked to admin account (id: {}, username: {})",
            user_info.login,
            admin_id,
            admin_username
        );

        // 更新管理员的最后登录时间和头像
        _db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE users SET
                avatar_url = $1,
                last_login_at = CURRENT_TIMESTAMP,
                updated_at = CURRENT_TIMESTAMP
             WHERE id = $2",
            vec![
                SeaValue::String(Some(Box::new(user_info.avatar_url.clone()))),
                SeaValue::Int(Some(admin_id)),
            ],
        ))
        .await
        .map_err(|e| {
            tracing::error!("Failed to update admin: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Failed to update admin"})),
            )
        })?;

        admin_id
    } else if let Some(github_user_row) = github_user_check {
        // 场景 2: GitHub 用户已存在 -> 更新现有用户
        let existing_user_id: i32 = github_user_row.try_get("", "id").map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Failed to read user data"})),
            )
        })?;

        tracing::info!("Updating existing GitHub user: {}", user_info.login);

        _db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE users SET
                username = $1,
                display_name = $2,
                email = $3,
                avatar_url = $4,
                github_profile_url = $5,
                bio = $6,
                location = $7,
                company = $8,
                last_login_at = CURRENT_TIMESTAMP,
                updated_at = CURRENT_TIMESTAMP
             WHERE id = $9",
            vec![
                SeaValue::String(Some(Box::new(user_info.login.clone()))),
                user_info.name.as_ref().map_or(SeaValue::String(None), |s| {
                    SeaValue::String(Some(Box::new(s.clone())))
                }),
                user_info
                    .email
                    .as_ref()
                    .map_or(SeaValue::String(None), |s| {
                        SeaValue::String(Some(Box::new(s.clone())))
                    }),
                SeaValue::String(Some(Box::new(user_info.avatar_url.clone()))),
                SeaValue::String(Some(Box::new(user_info.html_url.clone()))),
                user_info.bio.as_ref().map_or(SeaValue::String(None), |s| {
                    SeaValue::String(Some(Box::new(s.clone())))
                }),
                user_info
                    .location
                    .as_ref()
                    .map_or(SeaValue::String(None), |s| {
                        SeaValue::String(Some(Box::new(s.clone())))
                    }),
                user_info
                    .company
                    .as_ref()
                    .map_or(SeaValue::String(None), |s| {
                        SeaValue::String(Some(Box::new(s.clone())))
                    }),
                SeaValue::Int(Some(existing_user_id)),
            ],
        ))
        .await
        .map_err(|e| {
            tracing::error!("Failed to update GitHub user: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Failed to update user"})),
            )
        })?;

        existing_user_id
    } else {
        // 场景 3: 创建新的普通GitHub用户
        tracing::info!("Creating new GitHub user: {}", user_info.login);

        let insert_result = _db
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "INSERT INTO users (github_id, username, display_name, email, avatar_url, github_profile_url, bio, location, company, is_admin, auth_provider, last_login_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, false, 'github', CURRENT_TIMESTAMP)
                 RETURNING id",
                vec![
                    SeaValue::BigInt(Some(user_info.id)),
                    SeaValue::String(Some(Box::new(user_info.login.clone()))),
                    user_info.name.as_ref().map_or(SeaValue::String(None), |s| {
                        SeaValue::String(Some(Box::new(s.clone())))
                    }),
                    user_info.email.as_ref().map_or(SeaValue::String(None), |s| {
                        SeaValue::String(Some(Box::new(s.clone())))
                    }),
                    SeaValue::String(Some(Box::new(user_info.avatar_url.clone()))),
                    SeaValue::String(Some(Box::new(user_info.html_url.clone()))),
                    user_info.bio.as_ref().map_or(SeaValue::String(None), |s| {
                        SeaValue::String(Some(Box::new(s.clone())))
                    }),
                    user_info.location.as_ref().map_or(SeaValue::String(None), |s| {
                        SeaValue::String(Some(Box::new(s.clone())))
                    }),
                    user_info.company.as_ref().map_or(SeaValue::String(None), |s| {
                        SeaValue::String(Some(Box::new(s.clone())))
                    }),
                ],
            ))
            .await
            .map_err(|e| {
                tracing::error!("Failed to create GitHub user: {:?}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": "Failed to create user"})),
                )
            })?
            .ok_or_else(|| {
                tracing::error!("Insert returned no result");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": "Failed to create user"})),
                )
            })?;

        insert_result.try_get("", "id").map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Failed to get user ID"})),
            )
        })?
    };

    // ✅ 安全修复 P0: 查询用户的 is_admin 状态
    let is_admin_query = "SELECT is_admin FROM users WHERE id = $1";
    let is_admin_result = _db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            is_admin_query,
            vec![SeaValue::Int(Some(user_id))],
        ))
        .await
        .map_err(|e| {
            tracing::error!("Failed to query is_admin: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Database error"})),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "User not found after creation"})),
            )
        })?;

    let is_admin: bool = is_admin_result.try_get("", "is_admin").unwrap_or(false);

    // Generate JWT token
    let jwt_secret = env::var("JWT_SECRET").map_err(|_| {
        tracing::error!("JWT_SECRET not set");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "JWT secret not configured"})),
        )
    })?;
    let claims = Claims {
        sub: user_id.to_string(),
        username: user_info.login.clone(),
        is_admin, // ✅ 安全修复 P0: 从数据库读取
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

    tracing::info!("✅ JWT token created for user: {}", user_info.login);

    // ✅ 安全修复 P0: 设置 HttpOnly Cookie（安全）
    // 注意：使用 SameSite=Lax 而不是 Strict，因为 OAuth 重定向是跨站请求
    // Strict 会导致从 GitHub 重定向回来时 Cookie 不被设置

    // 使用 SiteConfig 判断是否为生产环境（基于 base_url 是否为 HTTPS）
    use crate::oauth_url_builder::SiteConfig;
    let is_production = SiteConfig::is_production().await;

    tracing::info!(
        "🍪 Cookie config: is_production={}, frontend_url={}",
        is_production,
        frontend_url
    );

    let cookie_value = format!(
        "auth_token={}; Path=/; HttpOnly; SameSite=Lax; Max-Age=2592000{}",
        token,
        if is_production { "; Secure" } else { "" } // 生产环境启用 Secure 标志
    );

    tracing::info!("🍪 Set-Cookie header (token truncated): auth_token={}...; Path=/; HttpOnly; SameSite=Lax; Max-Age=2592000{}",
        &token[..20.min(token.len())],
        if is_production { "; Secure" } else { "" }
    );

    // ✅ 安全修复 P0: 移除 URL 中的 token 参数，防止通过历史记录/Referer泄露
    // 前端将完全依赖 HttpOnly Cookie 进行认证
    let redirect_url = format!("{}/?auth=success", frontend_url);

    // 使用 HTML 页面进行重定向，确保 Cookie 被正确设置
    // 302 重定向在某些浏览器/配置下可能不会保存 Set-Cookie
    let html_body = format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <meta http-equiv="refresh" content="0;url={}">
    <title>登录成功</title>
</head>
<body>
    <p>登录成功，正在跳转...</p>
    <script>window.location.href = "{}";</script>
</body>
</html>"#,
        redirect_url, redirect_url
    );

    // 构建包含 Set-Cookie 的 HTML 响应
    let mut response = axum::response::Html(html_body).into_response();
    response
        .headers_mut()
        .insert(header::SET_COOKIE, cookie_value.parse().unwrap());

    tracing::info!("✅ GitHub OAuth successful, setting cookie and redirecting via HTML");
    Ok(response)
}

/// GET /api/auth/me
/// Get current user info from JWT
pub async fn get_current_user(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    // Extract JWT token from Authorization header or Cookie
    let token = headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .or_else(|| {
            // 回退到 Cookie
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
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({
                    "error": "Unauthorized",
                    "message": "Missing or invalid authorization token"
                })),
            )
        })?;

    // Decode and verify JWT
    let jwt_secret = env::var("JWT_SECRET").map_err(|_| {
        tracing::error!("JWT_SECRET not configured");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": "Server configuration error",
                "message": "JWT secret not configured"
            })),
        )
    })?;

    let token_data = jsonwebtoken::decode::<Claims>(
        token,
        &jsonwebtoken::DecodingKey::from_secret(jwt_secret.as_bytes()),
        &jsonwebtoken::Validation::default(),
    )
    .map_err(|e| {
        tracing::debug!("Invalid JWT token: {:?}", e);
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": "Invalid token",
                "message": "Token is invalid or expired"
            })),
        )
    })?;

    let user_id: i32 = token_data.claims.sub.parse().map_err(|_| {
        tracing::error!("Invalid user ID in token");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": "Invalid token data"
            })),
        )
    })?;

    // Query user from database
    use sea_orm::Value as SeaValue;

    let query =
        "SELECT id, username, auth_provider, is_admin, avatar_url, github_id, linked_github_id, bio
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
        tracing::debug!("User not found: {}", user_id);
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": "User not found",
                "message": "User account no longer exists"
            })),
        )
    })?;

    // Extract user data
    let id: i32 = user_row.try_get("", "id").map_err(|_| {
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

    let auth_provider: String = user_row
        .try_get("", "auth_provider")
        .unwrap_or_else(|_| "local".to_string());
    let is_admin: bool = user_row.try_get("", "is_admin").unwrap_or(false);
    let avatar_url: String = user_row
        .try_get("", "avatar_url")
        .unwrap_or_else(|_| "https://github.com/ghost.png".to_string());
    let github_id: Option<i64> = user_row.try_get("", "github_id").ok();
    // linked_github_id 在数据库中是 BIGINT 类型，需要读取为 i64
    let linked_github_id: Option<i64> = user_row.try_get("", "linked_github_id").ok();
    let bio: Option<String> = user_row.try_get("", "bio").ok();

    tracing::info!(
        "User info retrieved: {} (ID: {}, auth_provider: {}, linked_github_id: {:?})",
        username,
        id,
        auth_provider,
        linked_github_id
    );

    Ok(Json(json!({
        "id": id,
        "username": username,
        "display_name": username,
        "auth_provider": auth_provider,
        "is_admin": is_admin,
        "avatar_url": avatar_url,
        "github_id": github_id,
        "linked_github_id": linked_github_id.map(|id| id.to_string()),
        "bio": bio,
    })))
}

/// GET /api/auth/github/link
/// Redirect user to GitHub OAuth page for linking account
/// 支持动态环境检测，自动适配开发/生产环境
/// 🔒 需要先登录（通过 auth_middleware 保护）
pub async fn github_link(
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, Json<Value>)> {
    // ✅ 从 JWT 获取当前登录用户 ID（已通过 auth_middleware 验证）
    use crate::middleware::auth::verify_jwt_token;

    let claims = verify_jwt_token(&headers).map_err(|_| {
        tracing::error!("Failed to verify JWT for github_link");
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": "Unauthorized",
                "message": "Please login first before linking GitHub account."
            })),
        )
    })?;

    let user_id: i32 = claims.sub.parse().map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Invalid token",
                "message": "User ID in token is invalid"
            })),
        )
    })?;

    // 验证用户是管理员
    if !claims.is_admin {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "Forbidden",
                "message": "Only administrators can link GitHub accounts."
            })),
        ));
    }

    tracing::info!("🔗 Admin user {} initiating GitHub link", user_id);

    // 使用新的统一 OAuth 配置获取（支持数据库 + 环境变量 + 自动推断）
    let oauth_config = OAuthUrlBuilder::get_github_oauth_config()
        .await
        .map_err(|e| {
            tracing::error!("GitHub OAuth not configured: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "GitHub OAuth not configured",
                    "message": e
                })),
            )
        })?;

    let client = BasicClient::new(
        ClientId::new(oauth_config.client_id),
        Some(ClientSecret::new(oauth_config.client_secret)),
        AuthUrl::new("https://github.com/login/oauth/authorize".to_string()).unwrap(),
        Some(TokenUrl::new("https://github.com/login/oauth/access_token".to_string()).unwrap()),
    )
    .set_redirect_uri(RedirectUrl::new(oauth_config.redirect_url).unwrap());

    // ✅ 安全修复: 使用随机 state 并存储，防止 CSRF 攻击
    let (auth_url, csrf_token) = client
        .authorize_url(CsrfToken::new_random)
        .add_scope(Scope::new("read:user".to_string()))
        .add_scope(Scope::new("user:email".to_string()))
        .url();

    // 存储 state，标记为 LinkAccount 用途，并记录用户 ID
    let state_value = csrf_token.secret().to_string();
    {
        let mut states = OAUTH_STATES.write().await;
        const MAX_OAUTH_STATES: usize = 10000;
        if states.len() >= MAX_OAUTH_STATES {
            if let Some(oldest_key) = states
                .iter()
                .min_by_key(|(_, v)| v.created_at)
                .map(|(k, _)| k.clone())
            {
                states.remove(&oldest_key);
            }
        }
        states.insert(
            state_value.clone(),
            OAuthState {
                state: state_value,
                created_at: Instant::now(),
                purpose: OAuthPurpose::LinkAccount,
                user_id: Some(user_id), // ✅ 存储当前登录用户 ID
            },
        );
        tracing::debug!(
            "🔐 OAuth state stored for link_account (user_id: {}, total: {})",
            user_id,
            states.len()
        );
    }

    Ok(Redirect::to(auth_url.as_str()))
}

/// POST /api/auth/link-github
/// ⚠️ DEPRECATED: 此 API 已废弃
/// 绑定流程已改为通过 /api/auth/github/link -> GitHub -> /api/auth/github/callback 完成
/// 保留此端点仅为向后兼容，返回提示使用新流程
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct LinkGitHubRequest {
    pub code: String,
    pub state: String,
}

pub async fn link_github_account(
    State(_db): State<DatabaseConnection>,
    _headers: HeaderMap,
    Json(_request): Json<LinkGitHubRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    // 返回废弃提示
    tracing::warn!(
        "⚠️ Deprecated API /api/auth/link-github called. Use /api/auth/github/link instead."
    );
    Err((
        StatusCode::GONE,
        Json(json!({
            "error": "API deprecated",
            "message": "This API is deprecated. Please use GET /api/auth/github/link to initiate GitHub account linking. The callback will be handled automatically."
        })),
    ))
}

/// POST /api/auth/logout
/// Logout user and invalidate session
/// 不需要认证，彻底清理Cookie
pub async fn logout() -> impl IntoResponse {
    tracing::info!("🚪 User logout - clearing auth cookie");

    // 清除 HttpOnly Cookie（设置为空值+立即过期+删除标记）
    // 使用 Expires 和 Max-Age 双重保险确保Cookie被删除
    let cookie_value = "auth_token=deleted; Path=/; HttpOnly; SameSite=Strict; Max-Age=0; Expires=Thu, 01 Jan 1970 00:00:00 GMT";

    let mut response = Json(json!({
        "success": true,
        "message": "Logged out successfully"
    }))
    .into_response();

    response
        .headers_mut()
        .insert(header::SET_COOKIE, cookie_value.parse().unwrap());

    tracing::info!("✅ Auth cookie cleared");
    response
}

// Helper function to hash token
#[allow(dead_code)]
fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}
