/// Tapp API - 第三方应用集成接口
///
/// 提供以下功能：
/// 1. 平台数据读取/写入 API
/// 2. AI 调用代理（带配额管理）
/// 3. Widget 注册同步
/// 4. 报告数据访问
/// 5. 数据处理（data/transform）
/// 6. 运行上下文（context/*）
/// 7. AI 对话（ai/chat）
/// 8. 报告 CRUD（reports/*）
/// 9. 媒体控制（media/*）
/// 10. 组件注册（components/*）
/// 11. 快捷键注册（shortcuts/*）
/// 12. 事件总线（events/*）
/// 13. Tapp API 声明系统（api/:name）
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Extension, Json,
};
use once_cell::sync::Lazy;
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseConnection, DbBackend, EntityTrait, QueryFilter,
    QueryOrder, QuerySelect, Statement,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::middleware::auth::Claims;
use crate::models::entities::tapps;
use crate::services::analyzer::{AiAnalyzer, AiProvider};
use crate::services::permission_service::{TappPermission, TappPermissionService, UserRole};
use crate::GLOBAL_DYNAMIC_CONFIG;

// ============ 性能优化：全局资源 ============

/// 全局 HTTP Client（复用连接池）
static HTTP_CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::builder()
        .pool_max_idle_per_host(10)
        .pool_idle_timeout(std::time::Duration::from_secs(90))
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("Failed to create HTTP client")
});

/// 动态获取可用平台列表（扫描 cache/platforms 目录）
async fn get_available_platforms() -> Vec<String> {
    let cache_dir = std::path::Path::new("cache/platforms");
    let mut platforms = Vec::new();

    if let Ok(mut entries) = tokio::fs::read_dir(cache_dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            if let Some(name) = entry.file_name().to_str() {
                // 匹配 {platform}_filtered.json 格式
                if name.ends_with("_filtered.json") {
                    let platform = name.trim_end_matches("_filtered.json");
                    platforms.push(platform.to_string());
                }
            }
        }
    }

    platforms.sort();
    platforms
}

/// 平台数据缓存
struct PlatformCache {
    data: HashMap<String, (Value, std::time::Instant)>,
    ttl: std::time::Duration,
}

impl PlatformCache {
    fn new(ttl_secs: u64) -> Self {
        Self {
            data: HashMap::new(),
            ttl: std::time::Duration::from_secs(ttl_secs),
        }
    }

    fn get(&self, platform: &str) -> Option<&Value> {
        self.data.get(platform).and_then(|(data, instant)| {
            if instant.elapsed() < self.ttl {
                Some(data)
            } else {
                None
            }
        })
    }

    fn set(&mut self, platform: String, data: Value) {
        self.data
            .insert(platform, (data, std::time::Instant::now()));
    }

    #[allow(dead_code)]
    fn invalidate(&mut self, platform: &str) {
        self.data.remove(platform);
    }
}

/// 全局平台数据缓存（30秒TTL）
static PLATFORM_CACHE: Lazy<Arc<RwLock<PlatformCache>>> =
    Lazy::new(|| Arc::new(RwLock::new(PlatformCache::new(30))));

/// 获取平台数据（带缓存）
async fn get_cached_platform_data(platform: &str) -> Result<Value, String> {
    // 先检查缓存
    {
        let cache = PLATFORM_CACHE.read().await;
        if let Some(data) = cache.get(platform) {
            return Ok(data.clone());
        }
    }

    // 缓存未命中，从文件读取
    let cache_file = format!("cache/platforms/{}_filtered.json", platform.to_lowercase());
    let content = tokio::fs::read_to_string(&cache_file)
        .await
        .map_err(|e| format!("Failed to read cache: {}", e))?;

    let data: Value = serde_json::from_str(&content).unwrap_or(json!({ "items": [] }));

    // 更新缓存
    {
        let mut cache = PLATFORM_CACHE.write().await;
        cache.set(platform.to_lowercase(), data.clone());
    }

    Ok(data)
}

// ============ AI 配置缓存 ============

/// AI 配置信息
#[derive(Clone)]
struct AiConfig {
    provider: AiProvider,
    api_key: String,
    model: String,
    base_url: Option<String>,
}

/// AI 图片生成配置
#[derive(Clone)]
struct AiImageConfig {
    provider: String, // "pollinations" 或 "imaginepro"
    model: String,    // e.g., "flux-anime"
    width: u32,
    height: u32,
    imaginepro_api_key: Option<String>,
}

/// AI 配置缓存（5分钟 TTL）
struct AiConfigCache {
    config: Option<AiConfig>,
    cached_at: Option<std::time::Instant>,
    ttl: std::time::Duration,
}

impl AiConfigCache {
    fn new() -> Self {
        Self {
            config: None,
            cached_at: None,
            ttl: std::time::Duration::from_secs(300), // 5分钟缓存
        }
    }

    fn get(&self) -> Option<AiConfig> {
        if let (Some(config), Some(cached_at)) = (&self.config, &self.cached_at) {
            if cached_at.elapsed() < self.ttl {
                return Some(config.clone());
            }
        }
        None
    }

    fn set(&mut self, config: AiConfig) {
        self.config = Some(config);
        self.cached_at = Some(std::time::Instant::now());
    }
}

static AI_CONFIG_CACHE: Lazy<Arc<RwLock<AiConfigCache>>> =
    Lazy::new(|| Arc::new(RwLock::new(AiConfigCache::new())));

/// AI 图片配置缓存（5分钟 TTL）
struct AiImageConfigCache {
    config: Option<AiImageConfig>,
    cached_at: Option<std::time::Instant>,
    ttl: std::time::Duration,
}

impl AiImageConfigCache {
    fn new() -> Self {
        Self {
            config: None,
            cached_at: None,
            ttl: std::time::Duration::from_secs(300), // 5分钟缓存
        }
    }

    fn get(&self) -> Option<AiImageConfig> {
        if let (Some(config), Some(cached_at)) = (&self.config, &self.cached_at) {
            if cached_at.elapsed() < self.ttl {
                return Some(config.clone());
            }
        }
        None
    }

    fn set(&mut self, config: AiImageConfig) {
        self.config = Some(config);
        self.cached_at = Some(std::time::Instant::now());
    }
}

static AI_IMAGE_CONFIG_CACHE: Lazy<Arc<RwLock<AiImageConfigCache>>> =
    Lazy::new(|| Arc::new(RwLock::new(AiImageConfigCache::new())));

// ============ 后端速率限制（持久化） ============

/// 速率限制记录
#[derive(Clone)]
struct RateLimitEntry {
    count: u32,
    window_start: std::time::Instant,
}

/// Tapp 速率限制器
struct TappRateLimiter {
    /// 用户+Tapp+操作 -> 速率记录
    limits: HashMap<String, RateLimitEntry>,
    /// 上次清理时间
    last_cleanup: std::time::Instant,
}

impl TappRateLimiter {
    fn new() -> Self {
        Self {
            limits: HashMap::new(),
            last_cleanup: std::time::Instant::now(),
        }
    }

    /// 检查并记录请求
    /// 返回 (是否允许, 剩余配额, 重置时间秒数)
    fn check_and_record(
        &mut self,
        user_id: i32,
        tapp_id: &str,
        operation: &str,
        limit: u32,
        window_secs: u64,
    ) -> (bool, u32, u64) {
        let key = format!("{}:{}:{}", user_id, tapp_id, operation);
        let window = std::time::Duration::from_secs(window_secs);
        let now = std::time::Instant::now();

        // 定期清理过期记录（每5分钟）
        if now.duration_since(self.last_cleanup) > std::time::Duration::from_secs(300) {
            self.cleanup(window);
            self.last_cleanup = now;
        }

        let entry = self.limits.entry(key).or_insert_with(|| RateLimitEntry {
            count: 0,
            window_start: now,
        });

        // 检查窗口是否过期
        if now.duration_since(entry.window_start) > window {
            entry.count = 0;
            entry.window_start = now;
        }

        let reset_in = window
            .checked_sub(now.duration_since(entry.window_start))
            .unwrap_or_default()
            .as_secs();

        if entry.count >= limit {
            return (false, 0, reset_in);
        }

        entry.count += 1;
        (true, limit - entry.count, reset_in)
    }

    fn cleanup(&mut self, max_age: std::time::Duration) {
        let now = std::time::Instant::now();
        self.limits
            .retain(|_, entry| now.duration_since(entry.window_start) <= max_age);
    }
}

/// 全局速率限制器
static TAPP_RATE_LIMITER: Lazy<Arc<RwLock<TappRateLimiter>>> =
    Lazy::new(|| Arc::new(RwLock::new(TappRateLimiter::new())));

/// 速率限制配置
struct RateLimitConfig {
    limit: u32,
    window_secs: u64,
}

/// 获取操作的速率限制配置
fn get_rate_limit_config(operation: &str) -> RateLimitConfig {
    match operation {
        "ai.generate" | "ai.analyze" | "ai.chat" => RateLimitConfig {
            limit: 20, // 每分钟 20 次 AI 调用
            window_secs: 60,
        },
        "platform.write" => RateLimitConfig {
            limit: 30, // 每分钟 30 次写入
            window_secs: 60,
        },
        "storage.set" | "storage.clear" => RateLimitConfig {
            limit: 100, // 每分钟 100 次存储操作
            window_secs: 60,
        },
        _ => RateLimitConfig {
            limit: 200, // 默认每分钟 200 次
            window_secs: 60,
        },
    }
}

/// 检查速率限制
async fn check_rate_limit(
    user_id: i32,
    tapp_id: &str,
    operation: &str,
) -> Result<(), (StatusCode, Json<Value>)> {
    let config = get_rate_limit_config(operation);
    let mut limiter = TAPP_RATE_LIMITER.write().await;
    let (allowed, remaining, reset_in) = limiter.check_and_record(
        user_id,
        tapp_id,
        operation,
        config.limit,
        config.window_secs,
    );

    if !allowed {
        tracing::warn!(
            user_id = user_id,
            tapp_id = tapp_id,
            operation = operation,
            "[TAPP] Rate limit exceeded"
        );
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({
                "error": "Rate limit exceeded",
                "code": "RATE_LIMIT_EXCEEDED",
                "retryAfter": reset_in,
                "limit": config.limit,
                "remaining": remaining
            })),
        ));
    }

    Ok(())
}

// ============ 性能指标收集 ============

/// API 调用指标
#[allow(dead_code)]
struct ApiMetrics {
    /// 操作 -> (调用次数, 总耗时毫秒, 错误次数)
    operations: HashMap<String, (u64, u64, u64)>,
    /// 上次重置时间
    last_reset: std::time::Instant,
}

#[allow(dead_code)]
impl ApiMetrics {
    fn new() -> Self {
        Self {
            operations: HashMap::new(),
            last_reset: std::time::Instant::now(),
        }
    }

    fn record(&mut self, operation: &str, duration_ms: u64, is_error: bool) {
        let entry = self
            .operations
            .entry(operation.to_string())
            .or_insert((0, 0, 0));
        entry.0 += 1;
        entry.1 += duration_ms;
        if is_error {
            entry.2 += 1;
        }
    }

    fn get_summary(&self) -> Value {
        let uptime = self.last_reset.elapsed().as_secs();
        let mut ops: Vec<Value> = self
            .operations
            .iter()
            .map(|(op, (count, total_ms, errors))| {
                json!({
                    "operation": op,
                    "count": count,
                    "avgMs": if *count > 0 { total_ms / count } else { 0 },
                    "errors": errors,
                    "errorRate": if *count > 0 {
                        format!("{:.2}%", (*errors as f64 / *count as f64) * 100.0)
                    } else {
                        "0%".to_string()
                    }
                })
            })
            .collect();

        ops.sort_by(|a, b| {
            b.get("count")
                .and_then(|v| v.as_u64())
                .unwrap_or(0)
                .cmp(&a.get("count").and_then(|v| v.as_u64()).unwrap_or(0))
        });

        json!({
            "uptimeSeconds": uptime,
            "operations": ops
        })
    }

    fn reset(&mut self) {
        self.operations.clear();
        self.last_reset = std::time::Instant::now();
    }
}

/// 全局指标收集器
static API_METRICS: Lazy<Arc<RwLock<ApiMetrics>>> =
    Lazy::new(|| Arc::new(RwLock::new(ApiMetrics::new())));

/// 记录 API 调用指标
async fn record_metric(operation: &str, duration_ms: u64, is_error: bool) {
    let mut metrics = API_METRICS.write().await;
    metrics.record(operation, duration_ms, is_error);
}

/// 获取 AI 配置（带缓存，统一配置获取逻辑）
async fn get_ai_config() -> Result<AiConfig, (StatusCode, Json<Value>)> {
    // 尝试从缓存获取
    {
        let cache = AI_CONFIG_CACHE.read().await;
        if let Some(config) = cache.get() {
            return Ok(config);
        }
    }

    // 缓存未命中，读取配置
    let config = GLOBAL_DYNAMIC_CONFIG.read().await;

    let ai_config = {
        // 按优先级尝试：OpenAI > Gemini
        if let Some(key) = &config.openai_api_key {
            if !key.is_empty() {
                let model = if config.openai_model.is_empty() {
                    "gpt-4o-mini".to_string()
                } else {
                    config.openai_model.clone()
                };
                let base_url = if config.openai_base_url.is_empty() {
                    None
                } else {
                    Some(config.openai_base_url.clone())
                };
                Some(AiConfig {
                    provider: AiProvider::OpenAI,
                    api_key: key.clone(),
                    model,
                    base_url,
                })
            } else {
                None
            }
        } else {
            None
        }
    }
    .or_else(|| {
        if let Some(key) = &config.gemini_api_key {
            if !key.is_empty() {
                let model = if config.gemini_model.is_empty() {
                    "gemini-1.5-flash".to_string()
                } else {
                    config.gemini_model.clone()
                };
                Some(AiConfig {
                    provider: AiProvider::Gemini,
                    api_key: key.clone(),
                    model,
                    base_url: None,
                })
            } else {
                None
            }
        } else {
            None
        }
    });

    match ai_config {
        Some(cfg) => {
            // 更新缓存
            {
                let mut cache = AI_CONFIG_CACHE.write().await;
                cache.set(cfg.clone());
            }
            Ok(cfg)
        }
        None => Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "No AI provider configured" })),
        )),
    }
}

/// 获取 AI 图片生成配置（带缓存）
async fn get_ai_image_config() -> Result<AiImageConfig, (StatusCode, Json<Value>)> {
    // 尝试从缓存获取
    {
        let cache = AI_IMAGE_CONFIG_CACHE.read().await;
        if let Some(config) = cache.get() {
            return Ok(config);
        }
    }

    // 缓存未命中，读取配置
    let config = GLOBAL_DYNAMIC_CONFIG.read().await;

    let image_config = AiImageConfig {
        provider: config.ai_image_provider.clone(),
        model: config.ai_image_model.clone(),
        width: config.ai_image_width as u32,
        height: config.ai_image_height as u32,
        imaginepro_api_key: config.imaginepro_api_key.clone(),
    };

    // 更新缓存
    {
        let mut cache = AI_IMAGE_CONFIG_CACHE.write().await;
        cache.set(image_config.clone());
    }

    Ok(image_config)
}

// ============ 安全：Tapp 所有权验证 ============

/// 获取管理员用户 ID
async fn get_admin_user_id(db: &DatabaseConnection) -> Result<i32, (StatusCode, Json<Value>)> {
    let result = db
        .query_one(Statement::from_string(
            DbBackend::Postgres,
            "SELECT id FROM users WHERE is_admin = true LIMIT 1".to_string(),
        ))
        .await
        .map_err(|e| {
            tracing::error!("[TAPP] Database error fetching admin ID: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Database error" })),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "No admin user found" })),
            )
        })?;

    result.try_get::<i32>("", "id").map_err(|e| {
        tracing::error!("[TAPP] Error parsing admin ID: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Database error" })),
        )
    })
}

/// 验证用户是否有权访问指定的 Tapp
///
/// 这是一个关键的安全函数，用于防止跨 Tapp 数据篡改攻击。
///
/// 访问规则：
/// - 管理员：可以访问所有 Tapp
/// - 普通用户：可以访问自己安装的 Tapp + 管理员的公开 Tapp
/// - 游客：只能访问管理员的公开 Tapp
async fn verify_tapp_ownership(
    db: &DatabaseConnection,
    user_id: i32,
    tapp_id: &str,
) -> Result<(), (StatusCode, Json<Value>)> {
    // 获取管理员 ID
    let admin_id = get_admin_user_id(db).await?;

    // 游客（负数 ID）只能访问管理员的公开 Tapp
    let is_guest = user_id < 0;

    if is_guest {
        // 查找管理员安装的该 Tapp
        let admin_tapp = tapps::Entity::find()
            .filter(tapps::Column::TappId.eq(tapp_id))
            .filter(tapps::Column::UserId.eq(admin_id))
            .one(db)
            .await
            .map_err(|e| {
                tracing::error!("[TAPP] Database error in ownership verification: {}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": "Database error" })),
                )
            })?;

        if admin_tapp.is_none() {
            tracing::warn!(
                "[TAPP] Guest access denied - tapp_id: {} is not a public admin Tapp",
                tapp_id
            );
            return Err((
                StatusCode::FORBIDDEN,
                Json(json!({
                    "error": "Access denied",
                    "message": "This Tapp is not available for guest access"
                })),
            ));
        }

        return Ok(());
    }

    // 普通用户：检查自己拥有的 Tapp 或管理员的公开 Tapp
    let tapp = tapps::Entity::find()
        .filter(tapps::Column::TappId.eq(tapp_id))
        .filter(
            tapps::Column::UserId
                .eq(user_id)
                .or(tapps::Column::UserId.eq(admin_id)),
        )
        .one(db)
        .await
        .map_err(|e| {
            tracing::error!("[TAPP] Database error in ownership verification: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Database error" })),
            )
        })?;

    if tapp.is_none() {
        tracing::warn!(
            "[TAPP] Ownership verification failed - user_id: {}, tapp_id: {}",
            user_id,
            tapp_id
        );
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "Access denied",
                "message": "You do not have permission to access this Tapp"
            })),
        ));
    }

    Ok(())
}

/// 从 Claims 解析 user_id
fn parse_user_id(claims: &Claims) -> Result<i32, (StatusCode, Json<Value>)> {
    claims.sub.parse().map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Invalid user ID" })),
        )
    })
}

/// 检查用户是否拥有特定 Tapp 权限
///
/// 根据用户角色和系统配置检查权限：
/// - 管理员：拥有所有权限
/// - 普通用户：basic 权限 + 配置下放的 elevated 权限
/// - 游客：仅配置下放的权限（需要明确下放）
async fn check_tapp_permission(
    claims: &Claims,
    permission: TappPermission,
) -> Result<(), (StatusCode, Json<Value>)> {
    // 确定用户角色
    // 游客的 sub 是负数（基于 IP 生成）
    let role = if claims.is_admin {
        UserRole::Admin
    } else if let Ok(user_id) = claims.sub.parse::<i32>() {
        if user_id < 0 {
            UserRole::Guest
        } else {
            UserRole::User
        }
    } else {
        UserRole::Guest // 解析失败也视为游客
    };

    // 获取动态配置并检查权限
    let config = GLOBAL_DYNAMIC_CONFIG.read().await;
    let has_permission = TappPermissionService::check(&config, role, permission);
    drop(config);

    if !has_permission {
        let perm_name = permission.as_str();
        tracing::warn!(
            user_id = %claims.sub,
            permission = %perm_name,
            role = ?role,
            "[TAPP] Permission denied"
        );
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "Permission denied",
                "message": format!("You do not have the '{}' permission", perm_name),
                "code": "PERMISSION_DENIED"
            })),
        ));
    }

    Ok(())
}

// ============ Platform API ============

#[derive(Debug, Deserialize)]
pub struct PlatformDataQuery {
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    /// 过滤条件（预留字段，可用于实现数据过滤）
    #[allow(dead_code)]
    pub filter: Option<String>,
}

/// 获取平台数据
/// GET /api/tapp/platform/{platform}/data
pub async fn get_platform_data(
    State(_db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(platform): Path<String>,
    Query(query): Query<PlatformDataQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    tracing::debug!(
        "[TAPP] get_platform_data - User: {}, Platform: {}",
        claims.username,
        platform
    );

    // 使用缓存读取平台数据
    let data = match get_cached_platform_data(&platform).await {
        Ok(d) => d,
        Err(_) => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(json!({
                    "error": "Platform data not found",
                    "platform": platform
                })),
            ));
        }
    };

    // 应用分页
    let items = data
        .get("items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let total = items.len();
    let offset = query.offset.unwrap_or(0) as usize;
    let limit = query.limit.unwrap_or(100) as usize;
    let paged_items: Vec<_> = items.into_iter().skip(offset).take(limit).collect();

    Ok(Json(json!({
        "platform": platform,
        "items": paged_items,
        "total": total,
        "offset": offset,
        "limit": limit
    })))
}

/// 获取平台统计数据
/// GET /api/tapp/platform/{platform}/stats
pub async fn get_platform_stats(
    State(_db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(platform): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    tracing::debug!(
        "[TAPP] get_platform_stats - User: {}, Platform: {}",
        claims.username,
        platform
    );

    // 使用缓存读取平台数据
    let data = get_cached_platform_data(&platform)
        .await
        .unwrap_or(json!({ "items": [] }));

    let items = data
        .get("items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let total = items.len();

    // 简单的类型分布统计
    let mut type_distribution: HashMap<String, usize> = HashMap::new();
    for item in &items {
        if let Some(item_type) = item.get("type").and_then(|v| v.as_str()) {
            *type_distribution.entry(item_type.to_string()).or_default() += 1;
        }
    }

    Ok(Json(json!({
        "platform": platform,
        "total": total,
        "distribution": type_distribution,
        "recentActivity": []
    })))
}

/// 获取平台数据分布
/// GET /api/tapp/platform/{platform}/distribution/{dimension}
pub async fn get_platform_distribution(
    State(_db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path((platform, dimension)): Path<(String, String)>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    tracing::debug!(
        "[TAPP] get_platform_distribution - User: {}, Platform: {}, Dimension: {}",
        claims.username,
        platform,
        dimension
    );

    // 使用缓存读取平台数据
    let data = get_cached_platform_data(&platform)
        .await
        .unwrap_or(json!({ "items": [] }));

    let items = data
        .get("items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut distribution: HashMap<String, usize> = HashMap::new();
    for item in &items {
        if let Some(value) = item.get(&dimension).and_then(|v| v.as_str()) {
            *distribution.entry(value.to_string()).or_default() += 1;
        }
    }

    let distribution_data: Vec<Value> = distribution
        .into_iter()
        .map(|(label, value)| json!({ "label": label, "value": value }))
        .collect();

    Ok(Json(json!({
        "dimension": dimension,
        "data": distribution_data
    })))
}

// ============ Platform Write API ============

#[derive(Debug, Deserialize)]
pub struct AddPlatformItemRequest {
    pub tapp_id: String,
    pub item: NewPlatformItem,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct NewPlatformItem {
    pub platform: String,
    #[serde(rename = "type")]
    pub item_type: String,
    pub title: String,
    pub cover: Option<String>,
    pub description: Option<String>,
    pub url: Option<String>,
    pub metadata: Option<Value>,
    #[serde(rename = "createdAt")]
    pub created_at: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PlatformItemResult {
    pub success: bool,
    #[serde(rename = "itemId")]
    pub item_id: String,
    pub source: String,
}

/// 添加平台数据条目
/// POST /api/tapp/platform/items
pub async fn add_platform_item(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<AddPlatformItemRequest>,
) -> Result<Json<PlatformItemResult>, (StatusCode, Json<Value>)> {
    // 权限检查：需要 platform:write 权限
    check_tapp_permission(&claims, TappPermission::PlatformWrite).await?;

    // 安全验证：确认用户拥有此 Tapp
    let user_id = parse_user_id(&claims)?;
    verify_tapp_ownership(&db, user_id, &req.tapp_id).await?;

    tracing::info!(
        "[TAPP] add_platform_item - User: {}, Tapp: {}, Platform: {}",
        claims.username,
        req.tapp_id,
        req.item.platform
    );

    // 生成唯一 ID
    let item_id = format!(
        "tapp_{}_{}_{}",
        req.tapp_id,
        req.item.platform,
        chrono::Utc::now().timestamp_millis()
    );

    // 读取现有数据
    let cache_dir = std::path::Path::new("cache/platforms");
    let cache_file = cache_dir.join(format!(
        "{}_filtered.json",
        req.item.platform.to_lowercase()
    ));

    let mut data = if cache_file.exists() {
        match tokio::fs::read_to_string(&cache_file).await {
            Ok(content) => {
                serde_json::from_str::<Value>(&content).unwrap_or(json!({ "items": [] }))
            }
            Err(_) => json!({ "items": [] }),
        }
    } else {
        // 创建缓存目录
        let _ = tokio::fs::create_dir_all(cache_dir).await;
        json!({ "items": [] })
    };

    // 添加新条目
    let new_item = json!({
        "id": item_id,
        "type": req.item.item_type,
        "title": req.item.title,
        "cover": req.item.cover,
        "description": req.item.description,
        "url": req.item.url,
        "metadata": req.item.metadata,
        "createdAt": req.item.created_at.unwrap_or_else(|| chrono::Utc::now().to_rfc3339()),
        "source": format!("tapp:{}", req.tapp_id)
    });

    if let Some(items) = data.get_mut("items").and_then(|v| v.as_array_mut()) {
        items.push(new_item);
    }

    // 保存数据
    if let Err(e) =
        tokio::fs::write(&cache_file, serde_json::to_string_pretty(&data).unwrap()).await
    {
        tracing::error!("[TAPP] Failed to write cache file: {}", e);
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Failed to save platform data" })),
        ));
    }

    Ok(Json(PlatformItemResult {
        success: true,
        item_id,
        source: format!("tapp:{}", req.tapp_id),
    }))
}

#[derive(Debug, Deserialize)]
pub struct AddPlatformItemsBatchRequest {
    pub tapp_id: String,
    pub items: Vec<NewPlatformItem>,
}

/// 批量添加平台数据条目（优化版本：一次读取，一次写入）
/// POST /api/tapp/platform/items/batch
pub async fn add_platform_items_batch(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<AddPlatformItemsBatchRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    // 权限检查：需要 platform:write 权限
    check_tapp_permission(&claims, TappPermission::PlatformWrite).await?;

    // 安全验证：确认用户拥有此 Tapp
    let user_id = parse_user_id(&claims)?;
    verify_tapp_ownership(&db, user_id, &req.tapp_id).await?;

    tracing::info!(
        "[TAPP] add_platform_items_batch - User: {}, Tapp: {}, Count: {}",
        claims.username,
        req.tapp_id,
        req.items.len()
    );

    // 按平台分组
    let mut grouped_items: HashMap<String, Vec<&NewPlatformItem>> = HashMap::new();
    for item in &req.items {
        grouped_items
            .entry(item.platform.to_lowercase())
            .or_default()
            .push(item);
    }

    let mut results = Vec::new();
    let cache_dir = std::path::Path::new("cache/platforms");

    // 确保缓存目录存在
    let _ = tokio::fs::create_dir_all(cache_dir).await;

    // 对每个平台进行批量处理
    for (platform, items) in grouped_items {
        let cache_file = cache_dir.join(format!("{}_filtered.json", platform));

        // 一次性读取现有数据
        let mut data = if cache_file.exists() {
            match tokio::fs::read_to_string(&cache_file).await {
                Ok(content) => {
                    serde_json::from_str::<Value>(&content).unwrap_or(json!({ "items": [] }))
                }
                Err(_) => json!({ "items": [] }),
            }
        } else {
            json!({ "items": [] })
        };

        // 获取或创建 items 数组
        let data_items = data
            .as_object_mut()
            .and_then(|obj| obj.get_mut("items"))
            .and_then(|v| v.as_array_mut());

        if let Some(data_items) = data_items {
            // 批量添加所有新条目
            for item in items {
                let item_id = format!(
                    "tapp_{}_{}_{}",
                    req.tapp_id,
                    item.platform,
                    chrono::Utc::now().timestamp_millis()
                );

                let new_item = json!({
                    "id": item_id.clone(),
                    "type": item.item_type,
                    "title": item.title,
                    "cover": item.cover,
                    "description": item.description,
                    "url": item.url,
                    "metadata": item.metadata,
                    "createdAt": item.created_at.clone().unwrap_or_else(|| chrono::Utc::now().to_rfc3339()),
                    "source": format!("tapp:{}", req.tapp_id)
                });

                data_items.push(new_item);

                results.push(json!({
                    "success": true,
                    "itemId": item_id,
                    "source": format!("tapp:{}", req.tapp_id)
                }));
            }
        } else {
            // 如果结构不正确，为每个条目标记失败
            for _ in items {
                results.push(json!({
                    "success": false,
                    "error": "Invalid cache file structure"
                }));
            }
            continue;
        }

        // 一次性写入文件
        if let Err(e) =
            tokio::fs::write(&cache_file, serde_json::to_string_pretty(&data).unwrap()).await
        {
            tracing::error!("[TAPP] Failed to write cache file for {}: {}", platform, e);
            // 标记该平台的所有条目为失败（回滚结果）
            let platform_count = results
                .iter()
                .filter(|r| r.get("success").and_then(|v| v.as_bool()).unwrap_or(false))
                .count();
            for _ in 0..platform_count {
                if let Some(last) = results.last_mut() {
                    *last = json!({
                        "success": false,
                        "error": "Failed to save platform data"
                    });
                }
            }
        }
    }

    let success_count = results
        .iter()
        .filter(|r| r.get("success").and_then(|v| v.as_bool()).unwrap_or(false))
        .count();

    Ok(Json(json!({
        "success": success_count == results.len(),
        "results": results,
        "totalProcessed": results.len(),
        "successCount": success_count
    })))
}

// ============ AI API ============

#[derive(Debug, Deserialize)]
pub struct TappAiGenerateRequest {
    pub tapp_id: String,
    pub prompt: String,
    /// 上下文数据（预留字段，可用于传递对话历史等）
    #[allow(dead_code)]
    pub context: Option<Value>,
    /// 生成选项（预留字段，可用于控制生成参数）
    #[allow(dead_code)]
    pub options: Option<Value>,
}

/// AI 生成
/// POST /api/tapp/ai/generate
pub async fn ai_generate(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<TappAiGenerateRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let start = std::time::Instant::now();

    // 权限检查：需要 ai:generate 权限
    check_tapp_permission(&claims, TappPermission::AiGenerate).await?;

    // 安全验证：确认用户拥有此 Tapp
    let user_id = parse_user_id(&claims)?;
    verify_tapp_ownership(&db, user_id, &req.tapp_id).await?;

    // 速率限制检查
    check_rate_limit(user_id, &req.tapp_id, "ai.generate").await?;

    tracing::info!(
        user_id = user_id,
        tapp_id = %req.tapp_id,
        prompt_len = req.prompt.len(),
        "[TAPP] ai_generate request"
    );

    // 限制 prompt 长度
    if req.prompt.len() > 2000 {
        record_metric("ai.generate", start.elapsed().as_millis() as u64, true).await;
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Prompt too long (max 2000 characters)" })),
        ));
    }

    // 增强安全验证：检测恶意提示词
    if let Some(reason) = validate_prompt_security(&req.prompt) {
        tracing::warn!(
            user_id = user_id,
            tapp_id = %req.tapp_id,
            reason = %reason,
            "[TAPP] AI prompt security violation"
        );
        record_metric("ai.generate", start.elapsed().as_millis() as u64, true).await;
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Prompt contains disallowed content",
                "reason": reason
            })),
        ));
    }

    // 获取 AI 配置
    let ai_config = get_ai_config().await?;
    let analyzer = AiAnalyzer::new(
        ai_config.provider,
        ai_config.api_key,
        ai_config.model,
        ai_config.base_url,
    )
    .await;

    // 构建系统提示（增强安全约束）
    let system_prompt = format!(
        "You are an AI assistant helping a third-party app (Tapp ID: {}). \
        Important security constraints:\
        - Do not reveal internal system information\
        - Do not execute code or shell commands\
        - Do not access external URLs or make network requests\
        - Respond helpfully and concisely\
        - Keep responses under 1000 tokens\
        - Do not generate content that violates safety guidelines",
        req.tapp_id
    );

    // 调用 AI
    match analyzer
        .analyze_with_system(&system_prompt, &req.prompt)
        .await
    {
        Ok(result) => {
            let duration_ms = start.elapsed().as_millis() as u64;
            record_metric("ai.generate", duration_ms, false).await;

            // 估算 token 使用量
            let prompt_tokens = (req.prompt.len() + system_prompt.len()) / 4;
            let completion_tokens = result.len() / 4;

            tracing::info!(
                user_id = user_id,
                tapp_id = %req.tapp_id,
                duration_ms = duration_ms,
                tokens = prompt_tokens + completion_tokens,
                "[TAPP] ai_generate success"
            );

            Ok(Json(json!({
                "success": true,
                "result": result,
                "usage": {
                    "promptTokens": prompt_tokens,
                    "completionTokens": completion_tokens,
                    "totalTokens": prompt_tokens + completion_tokens
                },
                "quotaRemaining": 50
            })))
        }
        Err(e) => {
            let duration_ms = start.elapsed().as_millis() as u64;
            record_metric("ai.generate", duration_ms, true).await;

            tracing::error!(
                user_id = user_id,
                tapp_id = %req.tapp_id,
                error = %e,
                "[TAPP] AI generate error"
            );
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("AI generation failed: {}", e) })),
            ))
        }
    }
}

/// 验证提示词安全性（后端层）
fn validate_prompt_security(prompt: &str) -> Option<String> {
    let prompt_lower = prompt.to_lowercase();

    // 角色覆盖攻击检测
    let role_override_patterns = [
        "ignore previous",
        "ignore all previous",
        "ignore above",
        "forget your instructions",
        "you are now",
        "new instructions:",
        "system prompt:",
        "[system]",
        "disregard",
    ];
    for pattern in role_override_patterns {
        if prompt_lower.contains(pattern) {
            return Some("Role override attempt detected".to_string());
        }
    }

    // 越狱尝试检测
    let jailbreak_patterns = [
        "jailbreak",
        "dan mode",
        "developer mode",
        "bypass safety",
        "bypass filter",
        "uncensored mode",
    ];
    for pattern in jailbreak_patterns {
        if prompt_lower.contains(pattern) {
            return Some("Jailbreak attempt detected".to_string());
        }
    }

    // 敏感信息探测
    if prompt_lower.contains("api_key")
        || prompt_lower.contains("api-key")
        || prompt_lower.contains("apikey")
        || prompt_lower.contains("private_key")
        || prompt_lower.contains("secret_key")
        || prompt_lower.contains("access_token")
    {
        return Some("Sensitive information probe detected".to_string());
    }

    // 重复字符检测（防止 token 溢出）
    let mut prev_char = '\0';
    let mut repeat_count = 0;
    for c in prompt.chars() {
        if c == prev_char {
            repeat_count += 1;
            if repeat_count > 50 {
                return Some("Abnormal character repetition detected".to_string());
            }
        } else {
            prev_char = c;
            repeat_count = 0;
        }
    }

    None
}

#[derive(Debug, Deserialize)]
pub struct TappAiAnalyzeRequest {
    pub tapp_id: String,
    pub data: Value,
    #[serde(rename = "type")]
    pub analyze_type: String,
    pub instruction: Option<String>,
}

/// AI 分析
/// POST /api/tapp/ai/analyze
pub async fn ai_analyze(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<TappAiAnalyzeRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    // 权限检查：需要 ai:analyze 权限
    check_tapp_permission(&claims, TappPermission::AiAnalyze).await?;

    // 安全验证：确认用户拥有此 Tapp
    let user_id = parse_user_id(&claims)?;
    verify_tapp_ownership(&db, user_id, &req.tapp_id).await?;

    tracing::info!(
        "[TAPP] ai_analyze - User: {}, Tapp: {}, Type: {}",
        claims.username,
        req.tapp_id,
        req.analyze_type
    );

    // 获取 AI 配置
    let ai_config = get_ai_config().await?;

    // 创建分析提示
    let analysis_prompt = match req.analyze_type.as_str() {
        "summarize" => format!(
            "Please summarize the following data concisely:\n{}",
            serde_json::to_string_pretty(&req.data).unwrap_or_default()
        ),
        "categorize" => format!(
            "Please categorize the following data into logical groups:\n{}",
            serde_json::to_string_pretty(&req.data).unwrap_or_default()
        ),
        "sentiment" => format!(
            "Please analyze the sentiment of the following data:\n{}",
            serde_json::to_string_pretty(&req.data).unwrap_or_default()
        ),
        "custom" => {
            if let Some(instruction) = &req.instruction {
                format!(
                    "{}\n\nData:\n{}",
                    instruction,
                    serde_json::to_string_pretty(&req.data).unwrap_or_default()
                )
            } else {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "Instruction required for custom analysis" })),
                ));
            }
        }
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "Invalid analysis type" })),
            ));
        }
    };

    // 创建 AI 分析器
    let analyzer = AiAnalyzer::new(
        ai_config.provider,
        ai_config.api_key,
        ai_config.model,
        ai_config.base_url,
    )
    .await;

    match analyzer.analyze(&analysis_prompt).await {
        Ok(result) => {
            // 尝试解析为 JSON
            let analysis =
                serde_json::from_str::<Value>(&result).unwrap_or(json!({ "result": result }));

            Ok(Json(json!({
                "success": true,
                "analysis": analysis,
                "confidence": 0.8,
                "quotaRemaining": 50
            })))
        }
        Err(e) => {
            tracing::error!("[TAPP] AI analyze error: {}", e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("AI analysis failed: {}", e) })),
            ))
        }
    }
}

// ============ Reports API ============

/// 获取报告列表
/// GET /api/reports/list
pub async fn list_reports(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    tracing::info!("[TAPP] list_reports - User: {}", claims.username);

    use crate::models::entities::platform_reports;

    let user_id = claims.sub.parse::<i32>().map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Invalid user" })),
        )
    })?;

    let reports = platform_reports::Entity::find()
        .filter(platform_reports::Column::UserId.eq(user_id))
        .order_by_desc(platform_reports::Column::CreatedAt)
        .all(&db)
        .await
        .map_err(|e| {
            tracing::error!("[TAPP] Failed to fetch reports: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Failed to fetch reports" })),
            )
        })?;

    let report_list: Vec<Value> = reports
        .iter()
        .map(|r| {
            json!({
                "id": r.id,
                "platform": r.platform,
                "type": "platform",
                "createdAt": r.created_at.to_string(),
                "summary": r.report.get("summary").and_then(|v| v.as_str()).unwrap_or("")
            })
        })
        .collect();

    Ok(Json(json!({ "reports": report_list })))
}

// ============ Data Processing API ============

#[derive(Debug, Deserialize)]
pub struct DataTransformRequest {
    pub tapp_id: String,
    pub input: DataInput,
    pub pipeline: Vec<ProcessStep>,
    pub output: Option<DataOutput>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "source")]
pub enum DataInput {
    #[serde(rename = "platform")]
    Platform { platform: String },
    #[serde(rename = "storage")]
    Storage { key: String },
    #[serde(rename = "inline")]
    Inline { data: Value },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "target")]
pub enum DataOutput {
    #[serde(rename = "platform")]
    Platform { platform: String },
    #[serde(rename = "storage")]
    Storage { key: String },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
#[allow(dead_code)]
pub enum ProcessStep {
    #[serde(rename = "filter")]
    Filter {
        field: String,
        operator: String,
        value: Value,
    },
    #[serde(rename = "sort")]
    Sort {
        field: String,
        order: Option<String>,
    },
    #[serde(rename = "limit")]
    Limit { count: usize },
    #[serde(rename = "offset")]
    Offset { count: usize },
    #[serde(rename = "select")]
    Select { fields: Vec<String> },
    #[serde(rename = "group")]
    Group { by: String },
    #[serde(rename = "aggregate")]
    Aggregate {
        operation: String,
        field: Option<String>,
    },
    #[serde(rename = "dedupe")]
    Dedupe { key: String },
    #[serde(rename = "map")]
    Map { expression: String },
}

/// 数据转换处理
/// POST /api/tapp/data/transform
pub async fn data_transform(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<DataTransformRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    // 安全验证：确认用户拥有此 Tapp
    let user_id = parse_user_id(&claims)?;
    verify_tapp_ownership(&db, user_id, &req.tapp_id).await?;

    tracing::debug!(
        "[TAPP] data_transform - User: {}, Tapp: {}, Steps: {}",
        claims.username,
        req.tapp_id,
        req.pipeline.len()
    );

    // 1. 获取输入数据（使用缓存）
    let mut items: Vec<Value> = match req.input {
        DataInput::Platform { platform } => {
            let data = get_cached_platform_data(&platform)
                .await
                .unwrap_or(json!({"items": []}));
            data.get("items")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default()
        }
        DataInput::Storage { key } => {
            use crate::models::entities::tapp_storage;
            let item = tapp_storage::Entity::find()
                .filter(tapp_storage::Column::UserId.eq(user_id))
                .filter(tapp_storage::Column::TappId.eq(&req.tapp_id))
                .filter(tapp_storage::Column::Key.eq(&key))
                .one(&db)
                .await
                .map_err(|_| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({ "error": "Failed to read storage" })),
                    )
                })?;

            match item {
                Some(i) => i.value.as_array().cloned().unwrap_or_default(),
                None => Vec::new(),
            }
        }
        DataInput::Inline { data } => data.as_array().cloned().unwrap_or_else(|| vec![data]),
    };

    // 2. 执行处理管道
    for step in req.pipeline {
        items = apply_process_step(items, step)?;
    }

    // 3. 输出结果
    if let Some(output) = req.output {
        match output {
            DataOutput::Platform { platform } => {
                let cache_dir = std::path::Path::new("cache/platforms");
                let cache_file =
                    cache_dir.join(format!("{}_filtered.json", platform.to_lowercase()));
                let data = json!({ "items": items });
                let _ = tokio::fs::create_dir_all(cache_dir).await;
                tokio::fs::write(&cache_file, serde_json::to_string_pretty(&data).unwrap())
                    .await
                    .map_err(|_| {
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(json!({ "error": "Failed to write platform data" })),
                        )
                    })?;
            }
            DataOutput::Storage { key } => {
                use crate::models::entities::tapp_storage;
                use sea_orm::{ActiveModelTrait, ActiveValue::NotSet, Set};

                let now = chrono::Utc::now().fixed_offset();
                let existing = tapp_storage::Entity::find()
                    .filter(tapp_storage::Column::UserId.eq(user_id))
                    .filter(tapp_storage::Column::TappId.eq(&req.tapp_id))
                    .filter(tapp_storage::Column::Key.eq(&key))
                    .one(&db)
                    .await
                    .map_err(|_| {
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(json!({ "error": "Failed to check storage" })),
                        )
                    })?;

                if let Some(item) = existing {
                    let mut active: tapp_storage::ActiveModel = item.into();
                    active.value = Set(json!(items));
                    active.updated_at = Set(now);
                    active.update(&db).await.map_err(|_| {
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(json!({ "error": "Failed to update storage" })),
                        )
                    })?;
                } else {
                    let new_item = tapp_storage::ActiveModel {
                        id: NotSet,
                        tapp_id: Set(req.tapp_id.clone()),
                        user_id: Set(user_id),
                        key: Set(key),
                        value: Set(json!(items)),
                        created_at: Set(now),
                        updated_at: Set(now),
                    };
                    new_item.insert(&db).await.map_err(|_| {
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(json!({ "error": "Failed to save storage" })),
                        )
                    })?;
                }
            }
        }
    }

    Ok(Json(json!({
        "success": true,
        "count": items.len(),
        "data": items
    })))
}

/// 应用单个处理步骤
fn apply_process_step(
    mut items: Vec<Value>,
    step: ProcessStep,
) -> Result<Vec<Value>, (StatusCode, Json<Value>)> {
    match step {
        ProcessStep::Filter {
            field,
            operator,
            value,
        } => {
            items.retain(|item| {
                let item_value = item.get(&field);
                match operator.as_str() {
                    "eq" => item_value == Some(&value),
                    "ne" => item_value != Some(&value),
                    "gt" => {
                        if let (Some(a), Some(b)) =
                            (item_value.and_then(|v| v.as_f64()), value.as_f64())
                        {
                            a > b
                        } else {
                            false
                        }
                    }
                    "gte" => {
                        if let (Some(a), Some(b)) =
                            (item_value.and_then(|v| v.as_f64()), value.as_f64())
                        {
                            a >= b
                        } else {
                            false
                        }
                    }
                    "lt" => {
                        if let (Some(a), Some(b)) =
                            (item_value.and_then(|v| v.as_f64()), value.as_f64())
                        {
                            a < b
                        } else {
                            false
                        }
                    }
                    "lte" => {
                        if let (Some(a), Some(b)) =
                            (item_value.and_then(|v| v.as_f64()), value.as_f64())
                        {
                            a <= b
                        } else {
                            false
                        }
                    }
                    "contains" => {
                        if let (Some(a), Some(b)) =
                            (item_value.and_then(|v| v.as_str()), value.as_str())
                        {
                            a.contains(b)
                        } else {
                            false
                        }
                    }
                    "in" => {
                        if let Some(arr) = value.as_array() {
                            item_value.map(|v| arr.contains(v)).unwrap_or(false)
                        } else {
                            false
                        }
                    }
                    "exists" => item_value.is_some() && !item_value.unwrap().is_null(),
                    _ => true,
                }
            });
        }
        ProcessStep::Sort { field, order } => {
            let desc = order.as_deref() == Some("desc");
            items.sort_by(|a, b| {
                let va = a.get(&field);
                let vb = b.get(&field);
                let cmp = match (va, vb) {
                    (Some(Value::Number(a)), Some(Value::Number(b))) => a
                        .as_f64()
                        .partial_cmp(&b.as_f64())
                        .unwrap_or(std::cmp::Ordering::Equal),
                    (Some(Value::String(a)), Some(Value::String(b))) => a.cmp(b),
                    _ => std::cmp::Ordering::Equal,
                };
                if desc {
                    cmp.reverse()
                } else {
                    cmp
                }
            });
        }
        ProcessStep::Limit { count } => {
            items.truncate(count);
        }
        ProcessStep::Offset { count } => {
            items = items.into_iter().skip(count).collect();
        }
        ProcessStep::Select { fields } => {
            items = items
                .into_iter()
                .map(|item| {
                    let mut new_item = json!({});
                    if let Some(obj) = item.as_object() {
                        for field in &fields {
                            if let Some(value) = obj.get(field) {
                                new_item[field] = value.clone();
                            }
                        }
                    }
                    new_item
                })
                .collect();
        }
        ProcessStep::Group { by } => {
            let mut groups: HashMap<String, Vec<Value>> = HashMap::new();
            for item in items {
                let key = item
                    .get(&by)
                    .and_then(|v| v.as_str())
                    .unwrap_or("_unknown")
                    .to_string();
                groups.entry(key).or_default().push(item);
            }
            items = groups
                .into_iter()
                .map(|(key, values)| json!({ "key": key, "items": values, "count": values.len() }))
                .collect();
        }
        ProcessStep::Aggregate { operation, field } => {
            let result = match operation.as_str() {
                "count" => json!({ "count": items.len() }),
                "sum" => {
                    let sum: f64 = items
                        .iter()
                        .filter_map(|i| {
                            field
                                .as_ref()
                                .and_then(|f| i.get(f))
                                .and_then(|v| v.as_f64())
                        })
                        .sum();
                    json!({ "sum": sum })
                }
                "avg" => {
                    let values: Vec<f64> = items
                        .iter()
                        .filter_map(|i| {
                            field
                                .as_ref()
                                .and_then(|f| i.get(f))
                                .and_then(|v| v.as_f64())
                        })
                        .collect();
                    let avg = if values.is_empty() {
                        0.0
                    } else {
                        values.iter().sum::<f64>() / values.len() as f64
                    };
                    json!({ "avg": avg })
                }
                "min" => {
                    let min = items
                        .iter()
                        .filter_map(|i| {
                            field
                                .as_ref()
                                .and_then(|f| i.get(f))
                                .and_then(|v| v.as_f64())
                        })
                        .fold(f64::INFINITY, f64::min);
                    json!({ "min": if min.is_infinite() { Value::Null } else { json!(min) } })
                }
                "max" => {
                    let max = items
                        .iter()
                        .filter_map(|i| {
                            field
                                .as_ref()
                                .and_then(|f| i.get(f))
                                .and_then(|v| v.as_f64())
                        })
                        .fold(f64::NEG_INFINITY, f64::max);
                    json!({ "max": if max.is_infinite() { Value::Null } else { json!(max) } })
                }
                _ => json!({ "error": "Unknown aggregation" }),
            };
            items = vec![result];
        }
        ProcessStep::Dedupe { key } => {
            let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
            items.retain(|item| {
                let k = item.get(&key).map(|v| v.to_string()).unwrap_or_default();
                seen.insert(k)
            });
        }
        ProcessStep::Map { expression: _ } => {
            // Map 表达式执行需要 JS 引擎，暂时跳过
            // 可以考虑使用简单的字段重命名或模板替换
        }
    }
    Ok(items)
}

// ============ Context API ============

/// 获取应用上下文
/// GET /api/tapp/context/app
pub async fn get_context_app(
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    tracing::debug!("[TAPP] get_context_app - User: {}", claims.username);

    let config = GLOBAL_DYNAMIC_CONFIG.read().await;
    let platforms = get_available_platforms().await;

    Ok(Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "locale": "zh-CN",
        "theme": "system",
        "features": {
            "aiEnabled": config.gemini_api_key.is_some() || config.openai_api_key.is_some(),
            "platforms": platforms
        }
    })))
}

/// 获取用户上下文
/// GET /api/tapp/context/user
pub async fn get_context_user(
    State(_db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    tracing::debug!("[TAPP] get_context_user - User: {}", claims.username);

    let user_id: i32 = claims.sub.parse().map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Invalid user" })),
        )
    })?;

    // 动态获取已连接的平台
    let connected_platforms = get_available_platforms().await;

    // 确定用户角色
    // - admin: 管理员（可使用所有权限）
    // - user: 普通用户（只能使用 basic 权限）
    let role = if claims.is_admin { "admin" } else { "user" };

    // 从 JWT claims 中获取用户信息（不依赖数据库 users 表）
    Ok(Json(json!({
        "id": format!("user_{}", user_id),
        "username": claims.username,
        "avatar": null,
        "isAdmin": claims.is_admin,
        "role": role,
        "connectedPlatforms": connected_platforms,
        "preferences": {
            "language": "zh-CN",
            "timezone": "Asia/Shanghai"
        }
    })))
}

/// 获取播放器上下文（占位，实际状态在前端）
/// GET /api/tapp/context/player
pub async fn get_context_player(
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    tracing::debug!("[TAPP] get_context_player - User: {}", claims.username);

    // 播放器状态主要在前端维护，后端返回基本结构
    // 前端通过 TappBridge 直接提供实时状态
    Ok(Json(json!({
        "isPlaying": false,
        "isPaused": false,
        "currentTrack": null,
        "progress": {
            "current": 0,
            "duration": 0,
            "percentage": 0
        },
        "playlist": null,
        "mode": "sequence",
        "volume": 80,
        "muted": false,
        "_note": "Real-time player state is provided via TappBridge events"
    })))
}

/// 获取导航上下文（占位，实际状态在前端）
/// GET /api/tapp/context/navigation
pub async fn get_context_navigation(
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    tracing::debug!("[TAPP] get_context_navigation - User: {}", claims.username);

    // 导航状态在前端维护
    Ok(Json(json!({
        "currentPath": "/",
        "previousPath": null,
        "history": [],
        "availableRoutes": [
            { "path": "/", "name": "home", "icon": "home" },
            { "path": "/library", "name": "library", "icon": "book" },
            { "path": "/reports", "name": "reports", "icon": "file-text" },
            { "path": "/settings", "name": "settings", "icon": "settings" }
        ],
        "tappPages": [],
        "_note": "Real-time navigation state is provided via TappBridge events"
    })))
}

/// 获取系统上下文
/// GET /api/tapp/context/system
pub async fn get_context_system(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    tracing::debug!("[TAPP] get_context_system - User: {}", claims.username);

    // 检查数据库连接
    let db_connected = db.ping().await.is_ok();

    // 获取后台任务状态（简化版）
    let background_tasks: Vec<Value> = Vec::new();

    // 获取最后同步时间（动态获取可用平台）
    let mut last_fetch: HashMap<String, Option<String>> = HashMap::new();
    let cache_dir = std::path::Path::new("cache/platforms");
    let platforms = get_available_platforms().await;
    for platform in platforms {
        let file = cache_dir.join(format!("{}_filtered.json", platform));
        if file.exists() {
            if let Ok(metadata) = tokio::fs::metadata(&file).await {
                if let Ok(modified) = metadata.modified() {
                    let datetime: chrono::DateTime<chrono::Utc> = modified.into();
                    last_fetch.insert(platform.to_string(), Some(datetime.to_rfc3339()));
                }
            }
        } else {
            last_fetch.insert(platform.to_string(), None);
        }
    }

    Ok(Json(json!({
        "online": true,
        "serverConnected": db_connected,
        "version": env!("CARGO_PKG_VERSION"),
        "backgroundTasks": background_tasks,
        "lastFetch": last_fetch
    })))
}

// ============ P1: AI Chat API ============

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct AIChatRequest {
    pub tapp_id: String,
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub context: Option<ChatContext>,
    #[serde(default)]
    pub options: Option<ChatOptions>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ChatMessage {
    pub role: String, // "user" | "assistant" | "system"
    pub content: String,
}

#[derive(Debug, Deserialize, Default)]
pub struct ChatContext {
    pub include_platform_stats: Option<bool>,
    pub include_user_profile: Option<bool>,
    pub custom_data: Option<Value>,
}

#[derive(Debug, Deserialize, Default)]
pub struct ChatOptions {
    #[allow(dead_code)]
    pub max_tokens: Option<u32>,
    #[allow(dead_code)]
    pub temperature: Option<f32>,
    #[allow(dead_code)]
    pub stream: Option<bool>,
}

/// AI 对话 API
/// POST /api/tapp/ai/chat
pub async fn ai_chat(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<AIChatRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    // 权限检查：需要 ai:chat 权限
    check_tapp_permission(&claims, TappPermission::AiChat).await?;

    // 安全验证：确认用户拥有此 Tapp
    let user_id = parse_user_id(&claims)?;
    verify_tapp_ownership(&db, user_id, &req.tapp_id).await?;

    tracing::debug!(
        "[TAPP] ai_chat - User: {}, Tapp: {}, Messages: {}",
        claims.username,
        req.tapp_id,
        req.messages.len()
    );

    // 获取统一的 AI 配置
    let ai_config = get_ai_config().await?;

    // 构建完整的消息历史
    let mut full_messages = req.messages.clone();

    // 添加系统上下文
    if let Some(context) = &req.context {
        let mut system_context = String::new();

        if context.include_platform_stats.unwrap_or(false) {
            // 并行获取所有平台统计信息
            let platforms = get_available_platforms().await;
            let futures: Vec<_> = platforms
                .iter()
                .map(|p| get_cached_platform_data(p))
                .collect();
            let results = futures::future::join_all(futures).await;

            for (platform, result) in platforms.iter().zip(results.iter()) {
                if let Ok(data) = result {
                    if let Some(items) = data.get("items").and_then(|v| v.as_array()) {
                        system_context.push_str(&format!(
                            "\n{} 平台有 {} 条数据。",
                            platform,
                            items.len()
                        ));
                    }
                }
            }
        }

        if context.include_user_profile.unwrap_or(false) {
            system_context.push_str(&format!("\n用户名: {}", claims.username));
        }

        if let Some(custom) = &context.custom_data {
            system_context.push_str(&format!("\n自定义数据: {}", custom));
        }

        if !system_context.is_empty() {
            full_messages.insert(
                0,
                ChatMessage {
                    role: "system".to_string(),
                    content: format!("以下是用户的上下文信息：{}", system_context),
                },
            );
        }
    }

    // 调用 AI
    let analyzer = AiAnalyzer::new(
        ai_config.provider,
        ai_config.api_key,
        ai_config.model,
        ai_config.base_url,
    )
    .await;

    // 将消息转换为提示对象
    let prompt_data = json!({
        "prompt": full_messages
            .iter()
            .map(|m| format!("{}: {}", m.role, m.content))
            .collect::<Vec<_>>()
            .join("\n\n")
    });

    match analyzer.analyze_profile(&prompt_data).await {
        Ok(response) => {
            // 估算 token 使用
            let prompt_len = prompt_data.to_string().len();
            let prompt_tokens = (prompt_len / 4) as u32;
            let completion_tokens = (response.len() / 4) as u32;

            Ok(Json(json!({
                "success": true,
                "message": {
                    "role": "assistant",
                    "content": response
                },
                "usage": {
                    "prompt_tokens": prompt_tokens,
                    "completion_tokens": completion_tokens,
                    "total_tokens": prompt_tokens + completion_tokens
                },
                "session_id": null
            })))
        }
        Err(e) => {
            tracing::error!("[TAPP] AI chat error: {}", e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("AI chat failed: {}", e) })),
            ))
        }
    }
}

// ============ AI 图片生成 API ============

#[derive(Debug, Deserialize)]
pub struct TappAiImageGenerateRequest {
    pub tapp_id: String,
    pub prompt: String,
    /// 图片宽度（可选，覆盖配置）
    pub width: Option<u32>,
    /// 图片高度（可选，覆盖配置）
    pub height: Option<u32>,
    /// 模型（可选，覆盖配置）
    pub model: Option<String>,
    /// 是否增强提示词
    pub enhance: Option<bool>,
    /// 随机种子（可选）
    pub seed: Option<i64>,
}

/// AI 图片生成
/// POST /api/tapp/ai/image
///
/// 使用配置的图片生成服务（Pollinations 或 ImaginePro）生成图片
pub async fn ai_image_generate(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<TappAiImageGenerateRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let start = std::time::Instant::now();

    // 权限检查：需要 ai:image 权限
    check_tapp_permission(&claims, TappPermission::AiImage).await?;

    // 安全验证：确认用户拥有此 Tapp
    let user_id = parse_user_id(&claims)?;
    verify_tapp_ownership(&db, user_id, &req.tapp_id).await?;

    // 速率限制检查
    check_rate_limit(user_id, &req.tapp_id, "ai.image").await?;

    tracing::info!(
        user_id = user_id,
        tapp_id = %req.tapp_id,
        prompt_len = req.prompt.len(),
        "[TAPP] ai_image_generate request"
    );

    // 限制 prompt 长度
    if req.prompt.len() > 1000 {
        record_metric("ai.image", start.elapsed().as_millis() as u64, true).await;
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Prompt too long (max 1000 characters)" })),
        ));
    }

    // 基本安全验证
    if let Some(reason) = validate_image_prompt_security(&req.prompt) {
        tracing::warn!(
            user_id = user_id,
            tapp_id = %req.tapp_id,
            reason = %reason,
            "[TAPP] AI image prompt security violation"
        );
        record_metric("ai.image", start.elapsed().as_millis() as u64, true).await;
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Prompt contains disallowed content",
                "reason": reason
            })),
        ));
    }

    // 获取 AI 图片配置
    let image_config = get_ai_image_config().await?;

    // 使用请求参数覆盖配置
    let width = req.width.unwrap_or(image_config.width);
    let height = req.height.unwrap_or(image_config.height);
    let model = req.model.clone().unwrap_or(image_config.model.clone());
    let enhance = req.enhance.unwrap_or(true);

    // 限制尺寸范围
    let width = width.clamp(256, 2048);
    let height = height.clamp(256, 2048);

    match image_config.provider.as_str() {
        "pollinations" => {
            // 使用 Pollinations API（免费）
            let encoded_prompt = urlencoding::encode(&req.prompt);
            let mut url = format!(
                "https://image.pollinations.ai/prompt/{}?width={}&height={}&model={}&nologo=true&private=true&enhance={}",
                encoded_prompt, width, height, model, enhance
            );

            if let Some(seed) = req.seed {
                url.push_str(&format!("&seed={}", seed));
            }

            let duration_ms = start.elapsed().as_millis() as u64;
            record_metric("ai.image", duration_ms, false).await;

            tracing::info!(
                user_id = user_id,
                tapp_id = %req.tapp_id,
                duration_ms = duration_ms,
                provider = "pollinations",
                "[TAPP] ai_image_generate success"
            );

            Ok(Json(json!({
                "success": true,
                "provider": "pollinations",
                "url": url,
                "width": width,
                "height": height,
                "model": model,
                "prompt": req.prompt,
                "quotaRemaining": 100  // Pollinations 免费，无限制
            })))
        }
        "imaginepro" => {
            // 使用 ImaginePro API（付费 Midjourney）
            let api_key = image_config.imaginepro_api_key.ok_or_else(|| {
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({ "error": "ImaginePro API key not configured" })),
                )
            })?;

            if api_key.is_empty() {
                return Err((
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({ "error": "ImaginePro API key not configured" })),
                ));
            }

            // 调用 ImaginePro API
            let client = &*HTTP_CLIENT;

            // 计算宽高比
            let aspect_ratio = if width == height {
                "1:1".to_string()
            } else {
                let g = gcd(width, height);
                format!("{}:{}", width / g, height / g)
            };

            let imagine_response = client
                .post("https://api.imaginepro.ai/api/v1/midjourney/imagine")
                .header("Authorization", format!("Bearer {}", api_key))
                .header("Content-Type", "application/json")
                .json(&json!({
                    "prompt": req.prompt,
                    "aspect_ratio": aspect_ratio,
                    "process_mode": "fast"
                }))
                .send()
                .await
                .map_err(|e| {
                    tracing::error!("[TAPP] ImaginePro API request failed: {}", e);
                    (
                        StatusCode::BAD_GATEWAY,
                        Json(json!({ "error": format!("ImaginePro API error: {}", e) })),
                    )
                })?;

            if !imagine_response.status().is_success() {
                let status = imagine_response.status();
                let body = imagine_response.text().await.unwrap_or_default();
                tracing::error!("[TAPP] ImaginePro API error: {} - {}", status, body);
                record_metric("ai.image", start.elapsed().as_millis() as u64, true).await;
                return Err((
                    StatusCode::BAD_GATEWAY,
                    Json(json!({ "error": format!("ImaginePro API error: {}", status) })),
                ));
            }

            let result: Value = imagine_response.json().await.map_err(|e| {
                tracing::error!("[TAPP] Failed to parse ImaginePro response: {}", e);
                (
                    StatusCode::BAD_GATEWAY,
                    Json(json!({ "error": "Failed to parse ImaginePro response" })),
                )
            })?;

            let duration_ms = start.elapsed().as_millis() as u64;
            record_metric("ai.image", duration_ms, false).await;

            tracing::info!(
                user_id = user_id,
                tapp_id = %req.tapp_id,
                duration_ms = duration_ms,
                provider = "imaginepro",
                "[TAPP] ai_image_generate success"
            );

            Ok(Json(json!({
                "success": true,
                "provider": "imaginepro",
                "task_id": result.get("messageId").or(result.get("taskId")),
                "status": result.get("status").unwrap_or(&json!("pending")),
                "result": result,
                "quotaRemaining": 50
            })))
        }
        _ => {
            record_metric("ai.image", start.elapsed().as_millis() as u64, true).await;
            Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(
                    json!({ "error": format!("Unknown image provider: {}", image_config.provider) }),
                ),
            ))
        }
    }
}

/// 计算最大公约数（用于宽高比计算）
fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

/// 验证图片提示词安全性
fn validate_image_prompt_security(prompt: &str) -> Option<String> {
    let prompt_lower = prompt.to_lowercase();

    // NSFW 内容检测
    let nsfw_patterns = [
        "nude", "naked", "nsfw", "porn", "xxx", "hentai", "explicit", "sexual", "erotic", "fetish",
    ];

    for pattern in &nsfw_patterns {
        if prompt_lower.contains(pattern) {
            return Some(format!("NSFW content detected: {}", pattern));
        }
    }

    // 暴力内容检测
    let violence_patterns = [
        "gore",
        "blood",
        "murder",
        "torture",
        "mutilation",
        "dismember",
        "decapitat",
    ];

    for pattern in &violence_patterns {
        if prompt_lower.contains(pattern) {
            return Some(format!("Violent content detected: {}", pattern));
        }
    }

    None
}

// ============ P1: Report CRUD API ============

#[derive(Debug, Deserialize)]
pub struct CreateReportRequest {
    pub tapp_id: String,
    pub title: String,
    pub report_type: String, // "platform" | "comprehensive" | "custom"
    pub content: Value,
    pub metadata: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateReportRequest {
    pub title: Option<String>,
    pub content: Option<Value>,
    pub metadata: Option<Value>,
}

/// 创建报告
/// POST /api/tapp/reports
///
/// Tapp 报告存储在 tapp_storage 中，使用特殊的 key 前缀 "_report:"
pub async fn create_report(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<CreateReportRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    // 权限检查：需要 report:write 权限
    check_tapp_permission(&claims, TappPermission::ReportWrite).await?;

    // 安全验证：确认用户拥有此 Tapp
    let user_id = parse_user_id(&claims)?;
    verify_tapp_ownership(&db, user_id, &req.tapp_id).await?;

    tracing::info!(
        "[TAPP] create_report - User: {}, Tapp: {}, Type: {}",
        claims.username,
        req.tapp_id,
        req.report_type
    );

    use crate::models::entities::tapp_storage;
    use sea_orm::{ActiveModelTrait, ActiveValue::NotSet, Set};

    let now = chrono::Utc::now().fixed_offset();
    let report_id = format!("report_{}_{}", req.report_type, uuid::Uuid::new_v4());
    let storage_key = format!("_report:{}", report_id);

    let report_data = json!({
        "id": report_id,
        "title": req.title,
        "type": req.report_type,
        "content": req.content,
        "metadata": req.metadata,
        "createdAt": now.to_rfc3339(),
        "updatedAt": now.to_rfc3339()
    });

    let storage = tapp_storage::ActiveModel {
        id: NotSet,
        tapp_id: Set(req.tapp_id.clone()),
        user_id: Set(user_id),
        key: Set(storage_key),
        value: Set(report_data.clone()),
        created_at: Set(now),
        updated_at: Set(now),
    };

    storage.insert(&db).await.map_err(|e| {
        tracing::error!("[TAPP] Failed to create report: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Failed to create report" })),
        )
    })?;

    Ok(Json(json!({
        "success": true,
        "report": {
            "id": report_id,
            "title": req.title,
            "type": req.report_type,
            "createdAt": now.to_rfc3339()
        }
    })))
}

/// 报告列表查询参数
#[derive(Debug, Deserialize)]
pub struct ListReportsQuery {
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

/// 获取 Tapp 报告列表
/// GET /api/tapp/reports/tapp/:tapp_id
pub async fn list_tapp_reports(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(tapp_id): Path<String>,
    Query(query): Query<ListReportsQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    tracing::debug!(
        "[TAPP] list_tapp_reports - User: {}, Tapp: {}",
        claims.username,
        tapp_id
    );

    use crate::models::entities::tapp_storage;

    let user_id: i32 = claims.sub.parse().map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Invalid user" })),
        )
    })?;

    // 分页参数，默认 limit=50, offset=0
    let limit = query.limit.unwrap_or(50).min(100) as u64;
    let offset = query.offset.unwrap_or(0) as u64;

    let items = tapp_storage::Entity::find()
        .filter(tapp_storage::Column::UserId.eq(user_id))
        .filter(tapp_storage::Column::TappId.eq(&tapp_id))
        .filter(tapp_storage::Column::Key.starts_with("_report:"))
        .order_by_desc(tapp_storage::Column::CreatedAt)
        .offset(offset)
        .limit(limit)
        .all(&db)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Database error" })),
            )
        })?;

    let reports: Vec<Value> = items
        .into_iter()
        .filter_map(|item| {
            let data = item.value;
            Some(json!({
                "id": data.get("id")?,
                "title": data.get("title")?,
                "type": data.get("type")?,
                "createdAt": data.get("createdAt")?,
                "updatedAt": data.get("updatedAt")?
            }))
        })
        .collect();

    Ok(Json(json!({
        "success": true,
        "reports": reports,
        "pagination": {
            "limit": limit,
            "offset": offset
        }
    })))
}

/// 获取报告详情
/// GET /api/tapp/reports/:tapp_id/:report_id
pub async fn get_tapp_report(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path((tapp_id, report_id)): Path<(String, String)>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    tracing::debug!(
        "[TAPP] get_tapp_report - User: {}, Report: {}",
        claims.username,
        report_id
    );

    use crate::models::entities::tapp_storage;

    let user_id: i32 = claims.sub.parse().map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Invalid user" })),
        )
    })?;

    let storage_key = format!("_report:{}", report_id);

    let item = tapp_storage::Entity::find()
        .filter(tapp_storage::Column::UserId.eq(user_id))
        .filter(tapp_storage::Column::TappId.eq(&tapp_id))
        .filter(tapp_storage::Column::Key.eq(&storage_key))
        .one(&db)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Database error" })),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "Report not found" })),
            )
        })?;

    Ok(Json(json!({
        "success": true,
        "report": item.value
    })))
}

/// 更新报告
/// PUT /api/tapp/reports/:tapp_id/:report_id
pub async fn update_tapp_report(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path((tapp_id, report_id)): Path<(String, String)>,
    Json(req): Json<UpdateReportRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    tracing::info!(
        "[TAPP] update_tapp_report - User: {}, Report: {}",
        claims.username,
        report_id
    );

    use crate::models::entities::tapp_storage;
    use sea_orm::{ActiveModelTrait, Set};

    let user_id: i32 = claims.sub.parse().map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Invalid user" })),
        )
    })?;

    let storage_key = format!("_report:{}", report_id);

    let item = tapp_storage::Entity::find()
        .filter(tapp_storage::Column::UserId.eq(user_id))
        .filter(tapp_storage::Column::TappId.eq(&tapp_id))
        .filter(tapp_storage::Column::Key.eq(&storage_key))
        .one(&db)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Database error" })),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "Report not found" })),
            )
        })?;

    let now = chrono::Utc::now().fixed_offset();
    let mut report_data = item.value.clone();

    if let Some(title) = req.title {
        report_data["title"] = json!(title);
    }
    if let Some(content) = req.content {
        report_data["content"] = content;
    }
    if let Some(metadata) = req.metadata {
        report_data["metadata"] = metadata;
    }
    report_data["updatedAt"] = json!(now.to_rfc3339());

    let mut active: tapp_storage::ActiveModel = item.into();
    active.value = Set(report_data.clone());
    active.updated_at = Set(now);

    active.update(&db).await.map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Failed to update report" })),
        )
    })?;

    Ok(Json(json!({
        "success": true,
        "report": report_data
    })))
}

/// 删除报告
/// DELETE /api/tapp/reports/:tapp_id/:report_id
pub async fn delete_tapp_report(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path((tapp_id, report_id)): Path<(String, String)>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    tracing::info!(
        "[TAPP] delete_tapp_report - User: {}, Report: {}",
        claims.username,
        report_id
    );

    use crate::models::entities::tapp_storage;

    let user_id: i32 = claims.sub.parse().map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Invalid user" })),
        )
    })?;

    let storage_key = format!("_report:{}", report_id);

    let result = tapp_storage::Entity::delete_many()
        .filter(tapp_storage::Column::UserId.eq(user_id))
        .filter(tapp_storage::Column::TappId.eq(&tapp_id))
        .filter(tapp_storage::Column::Key.eq(&storage_key))
        .exec(&db)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Failed to delete report" })),
            )
        })?;

    if result.rows_affected == 0 {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Report not found" })),
        ));
    }

    Ok(Json(json!({
        "success": true,
        "deleted": report_id
    })))
}

// ============ P1: Media Control API ============

#[derive(Debug, Deserialize)]
pub struct MediaControlRequest {
    pub tapp_id: String,
    pub action: String, // "play" | "pause" | "next" | "prev" | "seek" | "volume" | "mode"
    pub value: Option<Value>,
}

/// 媒体控制（播放器控制）
/// POST /api/tapp/media/control
///
/// 注意：实际的播放器状态在前端维护，这个 API 主要用于：
/// 1. 记录 Tapp 的媒体控制请求日志
/// 2. 验证权限
/// 3. 未来可以扩展为跨设备同步
pub async fn media_control(
    Extension(claims): Extension<Claims>,
    Json(req): Json<MediaControlRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    // 权限检查：需要 media:control 权限
    check_tapp_permission(&claims, TappPermission::MediaControl).await?;

    tracing::info!(
        "[TAPP] media_control - User: {}, Tapp: {}, Action: {}",
        claims.username,
        req.tapp_id,
        req.action
    );

    // 验证 action 类型
    let valid_actions = [
        "play", "pause", "next", "prev", "seek", "volume", "mode", "mute", "unmute",
    ];
    if !valid_actions.contains(&req.action.as_str()) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": format!("Invalid action: {}", req.action) })),
        ));
    }

    // 对于需要值的操作，验证值
    match req.action.as_str() {
        "seek" => {
            if req.value.is_none() {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "Seek action requires a position value" })),
                ));
            }
        }
        "volume" => {
            if let Some(val) = &req.value {
                if let Some(v) = val.as_f64() {
                    if !(0.0..=100.0).contains(&v) {
                        return Err((
                            StatusCode::BAD_REQUEST,
                            Json(json!({ "error": "Volume must be between 0 and 100" })),
                        ));
                    }
                }
            }
        }
        "mode" => {
            if let Some(val) = &req.value {
                let valid_modes = ["sequence", "loop", "shuffle", "single"];
                if let Some(mode) = val.as_str() {
                    if !valid_modes.contains(&mode) {
                        return Err((
                            StatusCode::BAD_REQUEST,
                            Json(json!({ "error": format!("Invalid mode: {}", mode) })),
                        ));
                    }
                }
            }
        }
        _ => {}
    }

    // 返回确认（实际控制由前端 TappBridge 处理）
    Ok(Json(json!({
        "success": true,
        "action": req.action,
        "value": req.value,
        "_note": "Media control is handled by TappBridge on the frontend"
    })))
}

/// 获取当前播放状态
/// GET /api/tapp/media/status
pub async fn media_status(
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    tracing::debug!("[TAPP] media_status - User: {}", claims.username);

    // 播放器状态主要在前端维护
    // 这里返回基本结构，前端通过 TappBridge 提供实时状态
    Ok(Json(json!({
        "success": true,
        "status": {
            "isPlaying": false,
            "isPaused": false,
            "currentTrack": null,
            "progress": {
                "current": 0,
                "duration": 0,
                "percentage": 0
            },
            "playlist": null,
            "mode": "sequence",
            "volume": 80,
            "muted": false
        },
        "_note": "Real-time status is provided via TappBridge"
    })))
}

// ============ P2: Component Registration API ============

/// 组件类型
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "lowercase")]
pub enum ComponentType {
    Page,
    Theme,
    Agent,
}

/// 注册组件请求
#[derive(Debug, Deserialize)]
pub struct RegisterComponentRequest {
    pub tapp_id: String,
    pub component_type: ComponentType,
    pub config: Value, // 组件特定配置
}

/// 注册组件
/// POST /api/tapp/components/register
///
/// 组件配置存储在 tapp_storage 中，key 格式：_component:{type}:{id}
pub async fn register_component(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<RegisterComponentRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    // 安全验证：确认用户拥有此 Tapp
    let user_id = parse_user_id(&claims)?;
    verify_tapp_ownership(&db, user_id, &req.tapp_id).await?;

    let type_str = match &req.component_type {
        ComponentType::Page => "page",
        ComponentType::Theme => "theme",
        ComponentType::Agent => "agent",
    };

    tracing::info!(
        "[TAPP] register_component - User: {}, Tapp: {}, Type: {}",
        claims.username,
        req.tapp_id,
        type_str
    );

    use crate::models::entities::tapp_storage;
    use sea_orm::{ActiveModelTrait, ActiveValue::NotSet, Set};

    // 从配置中获取组件 ID
    let component_id = req
        .config
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "Component config must include 'id'" })),
            )
        })?;

    let now = chrono::Utc::now().fixed_offset();
    let storage_key = format!("_component:{}:{}", type_str, component_id);

    let component_data = json!({
        "id": component_id,
        "type": type_str,
        "tappId": req.tapp_id,
        "config": req.config,
        "registeredAt": now.to_rfc3339(),
        "enabled": true
    });

    // 先检查是否已存在
    let existing = tapp_storage::Entity::find()
        .filter(tapp_storage::Column::UserId.eq(user_id))
        .filter(tapp_storage::Column::TappId.eq(&req.tapp_id))
        .filter(tapp_storage::Column::Key.eq(&storage_key))
        .one(&db)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Database error" })),
            )
        })?;

    if let Some(existing) = existing {
        // 更新现有组件
        let mut active: tapp_storage::ActiveModel = existing.into();
        active.value = Set(component_data.clone());
        active.updated_at = Set(now);
        active.update(&db).await.map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Failed to update component" })),
            )
        })?;
    } else {
        // 创建新组件
        let storage = tapp_storage::ActiveModel {
            id: NotSet,
            tapp_id: Set(req.tapp_id.clone()),
            user_id: Set(user_id),
            key: Set(storage_key),
            value: Set(component_data.clone()),
            created_at: Set(now),
            updated_at: Set(now),
        };
        storage.insert(&db).await.map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Failed to register component" })),
            )
        })?;
    }

    Ok(Json(json!({
        "success": true,
        "component": {
            "id": component_id,
            "type": type_str,
            "tappId": req.tapp_id,
            "registeredAt": now.to_rfc3339()
        }
    })))
}

/// 注销组件
/// DELETE /api/tapp/components/:tapp_id/:component_type/:component_id
pub async fn unregister_component(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path((tapp_id, component_type, component_id)): Path<(String, String, String)>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    tracing::info!(
        "[TAPP] unregister_component - User: {}, Tapp: {}, Type: {}, ID: {}",
        claims.username,
        tapp_id,
        component_type,
        component_id
    );

    use crate::models::entities::tapp_storage;

    let user_id: i32 = claims.sub.parse().map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Invalid user" })),
        )
    })?;

    let storage_key = format!("_component:{}:{}", component_type, component_id);

    let result = tapp_storage::Entity::delete_many()
        .filter(tapp_storage::Column::UserId.eq(user_id))
        .filter(tapp_storage::Column::TappId.eq(&tapp_id))
        .filter(tapp_storage::Column::Key.eq(&storage_key))
        .exec(&db)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Failed to unregister component" })),
            )
        })?;

    if result.rows_affected == 0 {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Component not found" })),
        ));
    }

    Ok(Json(json!({
        "success": true,
        "unregistered": {
            "id": component_id,
            "type": component_type,
            "tappId": tapp_id
        }
    })))
}

/// 列出已注册的组件
/// GET /api/tapp/components/:tapp_id
pub async fn list_components(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(tapp_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    tracing::debug!(
        "[TAPP] list_components - User: {}, Tapp: {}",
        claims.username,
        tapp_id
    );

    use crate::models::entities::tapp_storage;

    let user_id: i32 = claims.sub.parse().map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Invalid user" })),
        )
    })?;

    // 可选过滤组件类型
    let type_filter = params.get("type");
    let key_prefix = if let Some(t) = type_filter {
        format!("_component:{}:", t)
    } else {
        "_component:".to_string()
    };

    let items = tapp_storage::Entity::find()
        .filter(tapp_storage::Column::UserId.eq(user_id))
        .filter(tapp_storage::Column::TappId.eq(&tapp_id))
        .filter(tapp_storage::Column::Key.starts_with(&key_prefix))
        .order_by_asc(tapp_storage::Column::CreatedAt)
        .all(&db)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Database error" })),
            )
        })?;

    let components: Vec<Value> = items.into_iter().map(|item| item.value).collect();

    Ok(Json(json!({
        "success": true,
        "components": components
    })))
}

/// 列出所有 Tapp 的指定类型组件
/// GET /api/tapp/components/all/:component_type
pub async fn list_all_components_by_type(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(component_type): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    tracing::debug!(
        "[TAPP] list_all_components_by_type - User: {}, Type: {}",
        claims.username,
        component_type
    );

    use crate::models::entities::tapp_storage;

    let user_id: i32 = claims.sub.parse().map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Invalid user" })),
        )
    })?;

    let key_prefix = format!("_component:{}:", component_type);

    let items = tapp_storage::Entity::find()
        .filter(tapp_storage::Column::UserId.eq(user_id))
        .filter(tapp_storage::Column::Key.starts_with(&key_prefix))
        .order_by_asc(tapp_storage::Column::CreatedAt)
        .all(&db)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Database error" })),
            )
        })?;

    let components: Vec<Value> = items.into_iter().map(|item| item.value).collect();

    Ok(Json(json!({
        "success": true,
        "type": component_type,
        "components": components
    })))
}

// ============ P2: Shortcut Registration API ============

/// 快捷键注册请求
#[derive(Debug, Deserialize)]
pub struct RegisterShortcutRequest {
    pub tapp_id: String,
    pub shortcut_id: String,
    pub keys: String, // e.g., "Ctrl+Shift+P"
    pub description: String,
    pub action: String,        // 触发时调用的 action 名称
    pub scope: Option<String>, // "global" | "tapp" | "editor" 等
}

/// 注册快捷键
/// POST /api/tapp/shortcuts/register
pub async fn register_shortcut(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<RegisterShortcutRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    // 权限检查：需要 shortcut:register 权限
    check_tapp_permission(&claims, TappPermission::ShortcutRegister).await?;

    // 安全验证：确认用户拥有此 Tapp
    let user_id = parse_user_id(&claims)?;
    verify_tapp_ownership(&db, user_id, &req.tapp_id).await?;

    tracing::info!(
        "[TAPP] register_shortcut - User: {}, Tapp: {}, Keys: {}",
        claims.username,
        req.tapp_id,
        req.keys
    );

    use crate::models::entities::tapp_storage;
    use sea_orm::{ActiveModelTrait, ActiveValue::NotSet, Set};

    // 验证快捷键格式
    if !validate_shortcut_keys(&req.keys) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Invalid shortcut key format" })),
        ));
    }

    let now = chrono::Utc::now().fixed_offset();
    let storage_key = format!("_shortcut:{}", req.shortcut_id);

    let shortcut_data = json!({
        "id": req.shortcut_id,
        "tappId": req.tapp_id,
        "keys": req.keys,
        "description": req.description,
        "action": req.action,
        "scope": req.scope.unwrap_or_else(|| "tapp".to_string()),
        "registeredAt": now.to_rfc3339(),
        "enabled": true
    });

    // 检查是否有冲突的快捷键
    let existing = tapp_storage::Entity::find()
        .filter(tapp_storage::Column::UserId.eq(user_id))
        .filter(tapp_storage::Column::Key.starts_with("_shortcut:"))
        .all(&db)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Database error" })),
            )
        })?;

    for item in existing {
        if let Some(keys) = item.value.get("keys").and_then(|v| v.as_str()) {
            if keys == req.keys {
                // 检查是否是同一个快捷键（允许更新）
                if let Some(id) = item.value.get("id").and_then(|v| v.as_str()) {
                    if id != req.shortcut_id {
                        return Err((
                            StatusCode::CONFLICT,
                            Json(json!({
                                "error": "Shortcut key conflict",
                                "conflicting_shortcut": id
                            })),
                        ));
                    }
                }
            }
        }
    }

    // 使用 upsert 逻辑
    let existing_item = tapp_storage::Entity::find()
        .filter(tapp_storage::Column::UserId.eq(user_id))
        .filter(tapp_storage::Column::TappId.eq(&req.tapp_id))
        .filter(tapp_storage::Column::Key.eq(&storage_key))
        .one(&db)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Database error" })),
            )
        })?;

    if let Some(existing_item) = existing_item {
        let mut active: tapp_storage::ActiveModel = existing_item.into();
        active.value = Set(shortcut_data.clone());
        active.updated_at = Set(now);
        active.update(&db).await.map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Failed to update shortcut" })),
            )
        })?;
    } else {
        let storage = tapp_storage::ActiveModel {
            id: NotSet,
            tapp_id: Set(req.tapp_id.clone()),
            user_id: Set(user_id),
            key: Set(storage_key),
            value: Set(shortcut_data.clone()),
            created_at: Set(now),
            updated_at: Set(now),
        };
        storage.insert(&db).await.map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Failed to register shortcut" })),
            )
        })?;
    }

    Ok(Json(json!({
        "success": true,
        "shortcut": shortcut_data
    })))
}

/// 注销快捷键
/// DELETE /api/tapp/shortcuts/:tapp_id/:shortcut_id
pub async fn unregister_shortcut(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path((tapp_id, shortcut_id)): Path<(String, String)>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    tracing::info!(
        "[TAPP] unregister_shortcut - User: {}, Tapp: {}, ID: {}",
        claims.username,
        tapp_id,
        shortcut_id
    );

    use crate::models::entities::tapp_storage;

    let user_id: i32 = claims.sub.parse().map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Invalid user" })),
        )
    })?;

    let storage_key = format!("_shortcut:{}", shortcut_id);

    let result = tapp_storage::Entity::delete_many()
        .filter(tapp_storage::Column::UserId.eq(user_id))
        .filter(tapp_storage::Column::TappId.eq(&tapp_id))
        .filter(tapp_storage::Column::Key.eq(&storage_key))
        .exec(&db)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Failed to unregister shortcut" })),
            )
        })?;

    if result.rows_affected == 0 {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Shortcut not found" })),
        ));
    }

    Ok(Json(json!({
        "success": true,
        "unregistered": shortcut_id
    })))
}

/// 列出所有快捷键
/// GET /api/tapp/shortcuts
pub async fn list_shortcuts(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    tracing::debug!("[TAPP] list_shortcuts - User: {}", claims.username);

    use crate::models::entities::tapp_storage;

    let user_id: i32 = claims.sub.parse().map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Invalid user" })),
        )
    })?;

    let mut query = tapp_storage::Entity::find()
        .filter(tapp_storage::Column::UserId.eq(user_id))
        .filter(tapp_storage::Column::Key.starts_with("_shortcut:"));

    // 可选按 tapp_id 过滤
    if let Some(tapp_id) = params.get("tapp_id") {
        query = query.filter(tapp_storage::Column::TappId.eq(tapp_id));
    }

    let items = query
        .order_by_asc(tapp_storage::Column::CreatedAt)
        .all(&db)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Database error" })),
            )
        })?;

    let shortcuts: Vec<Value> = items.into_iter().map(|item| item.value).collect();

    Ok(Json(json!({
        "success": true,
        "shortcuts": shortcuts
    })))
}

/// 验证快捷键格式
fn validate_shortcut_keys(keys: &str) -> bool {
    // 基本验证：非空，长度合理
    if keys.is_empty() || keys.len() > 50 {
        return false;
    }

    // 验证格式（如 Ctrl+Shift+P）
    let parts: Vec<&str> = keys.split('+').collect();
    if parts.is_empty() || parts.len() > 4 {
        return false;
    }

    let valid_modifiers = ["ctrl", "alt", "shift", "meta", "cmd"];
    let mut has_key = false;

    for (i, part) in parts.iter().enumerate() {
        let lower = part.to_lowercase();

        // 最后一个应该是实际按键
        if i == parts.len() - 1 {
            // 允许单字符、功能键、特殊键
            if lower.len() == 1
                || lower.starts_with("f") && lower.len() <= 3
                || [
                    "enter",
                    "escape",
                    "space",
                    "tab",
                    "backspace",
                    "delete",
                    "up",
                    "down",
                    "left",
                    "right",
                    "home",
                    "end",
                    "pageup",
                    "pagedown",
                ]
                .contains(&lower.as_str())
            {
                has_key = true;
            }
        } else {
            // 前面的应该是修饰键
            if !valid_modifiers.contains(&lower.as_str()) {
                return false;
            }
        }
    }

    has_key
}

// ============ P2: Event Bus API ============

/// 事件发布请求
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct PublishEventRequest {
    pub tapp_id: String,
    pub event_type: String,
    pub payload: Value,
    pub target: Option<String>, // "all" | "self" | 特定 tapp_id
}

/// 发布事件（用于记录和审计）
/// POST /api/tapp/events/publish
///
/// 注意：实际的事件路由在前端 TappBridge 中完成，
/// 这个 API 主要用于记录事件日志和未来的事件持久化
pub async fn publish_event(
    Extension(claims): Extension<Claims>,
    Json(req): Json<PublishEventRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    // 权限检查：需要 event:publish 权限
    check_tapp_permission(&claims, TappPermission::EventPublish).await?;

    tracing::info!(
        "[TAPP] publish_event - User: {}, Tapp: {}, Event: {}",
        claims.username,
        req.tapp_id,
        req.event_type
    );

    let event_id = format!("evt_{}_{}", req.event_type, uuid::Uuid::new_v4());
    let now = chrono::Utc::now().fixed_offset();

    // 这里可以扩展为：
    // 1. 存储事件到数据库（用于事件历史）
    // 2. 推送到 WebSocket 连接
    // 3. 触发服务端事件处理器

    Ok(Json(json!({
        "success": true,
        "event": {
            "id": event_id,
            "type": req.event_type,
            "tappId": req.tapp_id,
            "target": req.target.unwrap_or_else(|| "all".to_string()),
            "timestamp": now.to_rfc3339()
        },
        "_note": "Event routing is handled by TappBridge on the frontend"
    })))
}

/// 获取事件订阅状态
/// GET /api/tapp/events/subscriptions/:tapp_id
pub async fn get_event_subscriptions(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(tapp_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    tracing::debug!(
        "[TAPP] get_event_subscriptions - User: {}, Tapp: {}",
        claims.username,
        tapp_id
    );

    use crate::models::entities::tapp_storage;

    let user_id: i32 = claims.sub.parse().map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Invalid user" })),
        )
    })?;

    // 从存储中读取事件订阅配置
    let item = tapp_storage::Entity::find()
        .filter(tapp_storage::Column::UserId.eq(user_id))
        .filter(tapp_storage::Column::TappId.eq(&tapp_id))
        .filter(tapp_storage::Column::Key.eq("_event_subscriptions"))
        .one(&db)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Database error" })),
            )
        })?;

    let subscriptions = item.map(|i| i.value).unwrap_or_else(|| json!([]));

    Ok(Json(json!({
        "success": true,
        "tappId": tapp_id,
        "subscriptions": subscriptions
    })))
}

/// 更新事件订阅
/// PUT /api/tapp/events/subscriptions/:tapp_id
#[derive(Debug, Deserialize)]
pub struct UpdateSubscriptionsRequest {
    pub subscriptions: Vec<String>, // 事件类型列表
}

pub async fn update_event_subscriptions(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(tapp_id): Path<String>,
    Json(req): Json<UpdateSubscriptionsRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    tracing::info!(
        "[TAPP] update_event_subscriptions - User: {}, Tapp: {}, Count: {}",
        claims.username,
        tapp_id,
        req.subscriptions.len()
    );

    use crate::models::entities::tapp_storage;
    use sea_orm::{ActiveModelTrait, ActiveValue::NotSet, Set};

    let user_id: i32 = claims.sub.parse().map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Invalid user" })),
        )
    })?;

    let now = chrono::Utc::now().fixed_offset();
    let storage_key = "_event_subscriptions".to_string();

    let existing = tapp_storage::Entity::find()
        .filter(tapp_storage::Column::UserId.eq(user_id))
        .filter(tapp_storage::Column::TappId.eq(&tapp_id))
        .filter(tapp_storage::Column::Key.eq(&storage_key))
        .one(&db)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Database error" })),
            )
        })?;

    let subscriptions_value = json!(req.subscriptions);

    if let Some(existing) = existing {
        let mut active: tapp_storage::ActiveModel = existing.into();
        active.value = Set(subscriptions_value);
        active.updated_at = Set(now);
        active.update(&db).await.map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Failed to update subscriptions" })),
            )
        })?;
    } else {
        let storage = tapp_storage::ActiveModel {
            id: NotSet,
            tapp_id: Set(tapp_id.clone()),
            user_id: Set(user_id),
            key: Set(storage_key),
            value: Set(subscriptions_value),
            created_at: Set(now),
            updated_at: Set(now),
        };
        storage.insert(&db).await.map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Failed to save subscriptions" })),
            )
        })?;
    }

    Ok(Json(json!({
        "success": true,
        "tappId": tapp_id,
        "subscriptions": req.subscriptions
    })))
}

// ============ Metrics & Health API ============

/// 获取 Tapp API 性能指标
/// GET /api/tapp/metrics
/// 需要管理员权限
pub async fn get_tapp_metrics(
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    // 使用 is_admin 标志进行权限检查
    if !claims.is_admin {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "Admin access required" })),
        ));
    }

    let metrics = API_METRICS.read().await;
    let summary = metrics.get_summary();

    // 获取速率限制器统计
    let limiter = TAPP_RATE_LIMITER.read().await;
    let active_limits = limiter.limits.len();

    // 获取缓存统计
    let platform_cache = PLATFORM_CACHE.read().await;
    let cached_platforms = platform_cache.data.len();

    Ok(Json(json!({
        "success": true,
        "metrics": summary,
        "rateLimiter": {
            "activeLimits": active_limits
        },
        "cache": {
            "platforms": cached_platforms
        }
    })))
}

/// 重置性能指标
/// POST /api/tapp/metrics/reset
/// 需要管理员权限
pub async fn reset_tapp_metrics(
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if !claims.is_admin {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "Admin access required" })),
        ));
    }

    let mut metrics = API_METRICS.write().await;
    metrics.reset();

    tracing::info!(
        admin = %claims.username,
        "[TAPP] Metrics reset by admin"
    );

    Ok(Json(json!({
        "success": true,
        "message": "Metrics reset successfully"
    })))
}

/// 获取用户的速率限制状态
/// GET /api/tapp/rate-limit/:tapp_id
pub async fn get_rate_limit_status(
    Extension(claims): Extension<Claims>,
    Path(tapp_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let user_id: i32 = claims.sub.parse().map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Invalid user" })),
        )
    })?;

    let limiter = TAPP_RATE_LIMITER.read().await;

    let operations = ["ai.generate", "platform.write", "storage.set"];
    let mut limits = Vec::new();

    for op in operations {
        let key = format!("{}:{}:{}", user_id, tapp_id, op);
        let config = get_rate_limit_config(op);

        if let Some(entry) = limiter.limits.get(&key) {
            let window = std::time::Duration::from_secs(config.window_secs);
            let elapsed = entry.window_start.elapsed();
            let reset_in = if elapsed < window {
                (window - elapsed).as_secs()
            } else {
                0
            };
            let remaining = if elapsed < window {
                config.limit.saturating_sub(entry.count)
            } else {
                config.limit
            };

            limits.push(json!({
                "operation": op,
                "limit": config.limit,
                "used": entry.count,
                "remaining": remaining,
                "resetIn": reset_in
            }));
        } else {
            limits.push(json!({
                "operation": op,
                "limit": config.limit,
                "used": 0,
                "remaining": config.limit,
                "resetIn": config.window_secs
            }));
        }
    }

    Ok(Json(json!({
        "success": true,
        "tappId": tapp_id,
        "limits": limits
    })))
}

// ============ Tapp API 声明系统 ============

use crate::api::tapps::{TappApiAccess, TappApiDef};
use crate::services::tapp_api_service::{ApiExecutionContext, TappApiService};

/// 调用 Tapp 声明的 API 请求体
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TappApiCallRequest {
    /// 前端传入的参数
    pub params: Option<Value>,
}

/// 执行 Tapp 声明的 API
/// POST /api/tapp/:tapp_id/api/:api_name
///
/// 支持两种访问级别：
/// - public: 所有用户（包括游客）可调用
/// - protected: 需要 network:fetch 权限
///
/// AI 相关的内置 API 强制需要对应权限（ai:chat, ai:generate）
pub async fn execute_tapp_api(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    headers: axum::http::HeaderMap,
    Path((tapp_id, api_name)): Path<(String, String)>,
    Json(body): Json<TappApiCallRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    tracing::debug!(
        "[TAPP API] Execute {} for tapp {} by user {}",
        api_name,
        tapp_id,
        claims.username
    );

    let user_id: i32 = claims.sub.parse().map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Invalid user" })),
        )
    })?;

    // 1. 查找 Tapp 并获取 manifest
    let tapp = tapps::Entity::find()
        .filter(tapps::Column::TappId.eq(&tapp_id))
        .one(&db)
        .await
        .map_err(|e| {
            tracing::error!("[TAPP API] Database error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Database error" })),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "Tapp not found" })),
            )
        })?;

    // 2. 解析 manifest 中的 APIs
    let apis: HashMap<String, TappApiDef> = tapp
        .manifest
        .get("apis")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    let api_def = apis.get(&api_name).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("API '{}' not defined in manifest", api_name) })),
        )
    })?;

    // 3. 获取用户已授权的权限
    let granted_permissions: Vec<String> = tapp
        .granted_permissions
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    // 4. 获取客户端 IP
    let client_ip = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(|s| s.trim().to_string())
        .or_else(|| {
            headers
                .get("x-real-ip")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
        });

    // 5. 确定用户角色
    let role = if claims.is_admin {
        crate::services::permission_service::UserRole::Admin
    } else if user_id < 0 {
        crate::services::permission_service::UserRole::Guest
    } else {
        crate::services::permission_service::UserRole::User
    };

    // 6. 构建执行上下文
    let context = ApiExecutionContext {
        user_id,
        username: claims.username.clone(),
        is_admin: claims.is_admin,
        role,
        client_ip,
        granted_permissions,
    };

    // 7. 执行 API
    let result = TappApiService::execute(&tapp_id, &api_name, api_def, body.params, &context).await;

    if result.success {
        Ok(Json(json!({
            "success": true,
            "data": result.data,
            "cached": result.cached
        })))
    } else {
        Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "success": false,
                "error": result.error
            })),
        ))
    }
}

/// 列出 Tapp 可用的 API
/// GET /api/tapp/:tapp_id/apis
pub async fn list_tapp_apis(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(tapp_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    tracing::debug!(
        "[TAPP API] List APIs for tapp {} by user {}",
        tapp_id,
        claims.username
    );

    // 查找 Tapp
    let tapp = tapps::Entity::find()
        .filter(tapps::Column::TappId.eq(&tapp_id))
        .one(&db)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("Database error: {}", e) })),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "Tapp not found" })),
            )
        })?;

    // 解析 APIs
    let apis: HashMap<String, TappApiDef> = tapp
        .manifest
        .get("apis")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    // 转换为响应格式
    let api_list: Vec<Value> = apis
        .iter()
        .map(|(name, def)| {
            json!({
                "name": name,
                "access": match def.access {
                    TappApiAccess::Public => "public",
                    TappApiAccess::Protected => "protected",
                },
                "type": def.api_type,
                "description": def.description,
                "cacheTtl": def.cache_ttl,
            })
        })
        .collect();

    Ok(Json(json!({
        "success": true,
        "apis": api_list
    })))
}

/// 获取地理位置信息（公开 API）
/// GET /api/tapp/context/geo
///
/// 这是一个公开 API，所有用户（包括游客）都可以调用
/// 用于获取客户端的地理位置信息
pub async fn get_context_geo(
    headers: axum::http::HeaderMap,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    use crate::services::tapp_api_service::TappApiService;

    // 获取客户端 IP
    let client_ip = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(|s| s.trim().to_string())
        .or_else(|| {
            headers
                .get("x-real-ip")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| addr.ip().to_string());

    tracing::debug!("[TAPP] get_context_geo for IP: {}", client_ip);

    // 复用 TappApiService 的 geo 逻辑
    let context = ApiExecutionContext {
        user_id: -1,
        username: "guest".to_string(),
        is_admin: false,
        role: crate::services::permission_service::UserRole::Guest,
        client_ip: Some(client_ip),
        granted_permissions: vec![],
    };

    // 创建一个内置 geo API 定义
    let geo_api = TappApiDef {
        access: TappApiAccess::Public,
        api_type: "builtin".to_string(),
        endpoint: None,
        url: None,
        params: None,
        method: "GET".to_string(),
        headers: None,
        body: None,
        builtin: Some("geo".to_string()),
        inject: None,
        cache_ttl: 300, // 缓存5分钟
        spoof: None,    // 内置 API 不需要伪装
        description: Some("Get client geolocation".to_string()),
    };

    let result = TappApiService::execute("system", "geo", &geo_api, None, &context).await;

    if result.success {
        Ok(Json(json!({
            "success": true,
            "data": result.data,
            "cached": result.cached
        })))
    } else {
        Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "success": false,
                "error": result.error
            })),
        ))
    }
}
