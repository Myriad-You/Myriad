//! Tapp 共享基础模块
//!
//! 提供：
//! - 通用 TTL 缓存
//! - 全局 HTTP Client
//! - 平台数据缓存
//! - AI 配置缓存
//! - 速率限制器
//! - 安全验证
//! - 权限检查
//! - 性能指标

use axum::{http::StatusCode, Json};
use hmac::{Hmac, Mac};
use once_cell::sync::Lazy;
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseConnection, DbBackend, EntityTrait, FromQueryResult,
    QueryFilter, Statement, TransactionTrait,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, OwnedMutexGuard, RwLock};

use crate::middleware::auth::{ensure_current_admin, Claims};
use crate::models::entities::tapps;
use crate::services::analyzer::AiProvider;
use crate::services::permission_service::{TappPermission, TappPermissionService, UserRole};
use crate::GLOBAL_DYNAMIC_CONFIG;

// ============ 通用 TTL 缓存 ============

/// 通用 TTL 缓存条目
struct CacheEntry<V> {
    value: V,
    created_at: Instant,
}

/// 通用的 TTL 缓存（线程安全）
pub struct TtlCache<V: Clone> {
    data: HashMap<String, CacheEntry<V>>,
    ttl: Duration,
}

impl<V: Clone> TtlCache<V> {
    pub fn new(ttl: Duration) -> Self {
        Self {
            data: HashMap::new(),
            ttl,
        }
    }

    pub fn get(&self, key: &str) -> Option<&V> {
        self.data.get(key).and_then(|entry| {
            if entry.created_at.elapsed() < self.ttl {
                Some(&entry.value)
            } else {
                None
            }
        })
    }

    pub fn set(&mut self, key: String, value: V) {
        self.data.insert(
            key,
            CacheEntry {
                value,
                created_at: Instant::now(),
            },
        );
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }
}

/// 单值 TTL 缓存（用于 AI 配置等全局配置）
pub struct SingleCache<V: Clone> {
    value: Option<V>,
    cached_at: Option<Instant>,
    ttl: Duration,
}

impl<V: Clone> SingleCache<V> {
    pub fn new(ttl: Duration) -> Self {
        Self {
            value: None,
            cached_at: None,
            ttl,
        }
    }

    pub fn get(&self) -> Option<V> {
        if let (Some(value), Some(cached_at)) = (&self.value, &self.cached_at) {
            if cached_at.elapsed() < self.ttl {
                return Some(value.clone());
            }
        }
        None
    }

    pub fn set(&mut self, value: V) {
        self.value = Some(value);
        self.cached_at = Some(Instant::now());
    }
}

// ============ 全局 HTTP Client ============

/// 全局 HTTP Client（复用连接池）
pub static HTTP_CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::builder()
        .pool_max_idle_per_host(10)
        .pool_idle_timeout(Duration::from_secs(90))
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .user_agent("Myriad-Tapp/1.0")
        .build()
        .expect("Failed to create HTTP client")
});

// ============ 平台数据缓存 ============

/// 全局平台数据缓存（30秒 TTL）
pub static PLATFORM_CACHE: Lazy<Arc<RwLock<TtlCache<Value>>>> =
    Lazy::new(|| Arc::new(RwLock::new(TtlCache::new(Duration::from_secs(30)))));

/// 每个平台共享一把锁，使缓存未命中的文件读取和 read-modify-write 串行化。
static PLATFORM_LOCKS: Lazy<RwLock<HashMap<String, Weak<Mutex<()>>>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

/// 平台列表缓存（60秒 TTL）
static PLATFORM_LIST_CACHE: Lazy<Arc<RwLock<SingleCache<Vec<String>>>>> =
    Lazy::new(|| Arc::new(RwLock::new(SingleCache::new(Duration::from_secs(60)))));

/// 动态获取可用平台列表（带缓存）
pub async fn get_available_platforms() -> Vec<String> {
    // 检查缓存
    {
        let cache = PLATFORM_LIST_CACHE.read().await;
        if let Some(platforms) = cache.get() {
            return platforms;
        }
    }

    // 缓存未命中，扫描文件系统
    let cache_dir = std::path::Path::new("cache/platforms");
    let mut platforms = Vec::new();

    if let Ok(mut entries) = tokio::fs::read_dir(cache_dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            if let Some(name) = entry.file_name().to_str() {
                if name.ends_with("_filtered.json") {
                    let platform = name.trim_end_matches("_filtered.json");
                    platforms.push(platform.to_string());
                }
            }
        }
    }

    platforms.sort();

    // 更新缓存
    {
        let mut cache = PLATFORM_LIST_CACHE.write().await;
        cache.set(platforms.clone());
    }

    platforms
}

/// 验证平台名称安全性（防止路径遍历）
pub fn validate_platform_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 64 {
        return Err("Invalid platform name length".to_string());
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("Platform name contains invalid characters".to_string());
    }
    Ok(())
}

/// 获取规范化平台名对应的共享 I/O 锁。
pub async fn acquire_platform_lock(platform: &str) -> Result<OwnedMutexGuard<()>, String> {
    validate_platform_name(platform)?;
    let key = platform.to_lowercase();
    let lock = {
        let mut locks = PLATFORM_LOCKS.write().await;
        locks.retain(|_, lock| lock.strong_count() > 0);
        if let Some(lock) = locks.get(&key).and_then(Weak::upgrade) {
            lock
        } else {
            let lock = Arc::new(Mutex::new(()));
            locks.insert(key, Arc::downgrade(&lock));
            lock
        }
    };
    Ok(lock.lock_owned().await)
}

/// 文件写入成功后立即刷新内存缓存，避免最多 30 秒的陈旧读取。
pub async fn update_cached_platform_data(platform: &str, data: Value) -> Result<(), String> {
    validate_platform_name(platform)?;
    let key = platform.to_lowercase();
    PLATFORM_CACHE.write().await.set(key.clone(), data);

    let mut list_cache = PLATFORM_LIST_CACHE.write().await;
    if let Some(mut platforms) = list_cache.get() {
        if !platforms.iter().any(|value| value == &key) {
            platforms.push(key);
            platforms.sort();
            list_cache.set(platforms);
        }
    }
    Ok(())
}

/// 获取平台数据（带缓存；同平台缓存未命中只执行一次文件读取）
pub async fn get_cached_platform_data(platform: &str) -> Result<Value, String> {
    validate_platform_name(platform)?;
    let key = platform.to_lowercase();

    // 先检查缓存（read lock）
    {
        let cache = PLATFORM_CACHE.read().await;
        if let Some(data) = cache.get(&key) {
            return Ok(data.clone());
        }
    }

    let _platform_guard = acquire_platform_lock(&key).await?;

    // 获得同平台锁后再次检查，前一个请求可能已经填充缓存。
    {
        let cache = PLATFORM_CACHE.read().await;
        if let Some(data) = cache.get(&key) {
            return Ok(data.clone());
        }
    }

    let cache_file = format!("cache/platforms/{}_filtered.json", key);
    let content = tokio::fs::read_to_string(&cache_file)
        .await
        .map_err(|e| format!("Failed to read cache: {}", e))?;

    let data: Value = serde_json::from_str(&content)
        .map_err(|error| format!("Failed to parse cache: {}", error))?;

    // 重新获取写锁写入缓存
    {
        let mut cache = PLATFORM_CACHE.write().await;
        cache.set(key, data.clone());
    }

    Ok(data)
}

// ============ AI 配置缓存 ============

/// AI 配置信息
#[derive(Clone)]
pub struct AiConfig {
    pub provider: AiProvider,
    pub api_key: String,
    pub model: String,
    pub base_url: Option<String>,
}

/// AI 图片生成配置
#[derive(Clone)]
pub struct AiImageConfig {
    pub provider: String,
    pub model: String,
    pub width: u32,
    pub height: u32,
    pub pixai_api_key: Option<String>,
}

/// AI 配置缓存（5分钟 TTL）- 标准层级
static AI_CONFIG_CACHE: Lazy<Arc<RwLock<SingleCache<AiConfig>>>> =
    Lazy::new(|| Arc::new(RwLock::new(SingleCache::new(Duration::from_secs(300)))));

/// AI 配置缓存（5分钟 TTL）- Pro 层级
static AI_PRO_CONFIG_CACHE: Lazy<Arc<RwLock<SingleCache<AiConfig>>>> =
    Lazy::new(|| Arc::new(RwLock::new(SingleCache::new(Duration::from_secs(300)))));

/// AI 图片配置缓存（5分钟 TTL）
static AI_IMAGE_CONFIG_CACHE: Lazy<Arc<RwLock<SingleCache<AiImageConfig>>>> =
    Lazy::new(|| Arc::new(RwLock::new(SingleCache::new(Duration::from_secs(300)))));

/// 获取指定层级的 AI 配置（带缓存）
pub async fn get_ai_config_for_tier(
    tier: crate::config::ModelTier,
) -> Result<AiConfig, (StatusCode, Json<Value>)> {
    let cache_ref = match tier {
        crate::config::ModelTier::Standard => &*AI_CONFIG_CACHE,
        crate::config::ModelTier::Pro => &*AI_PRO_CONFIG_CACHE,
    };

    {
        let cache = cache_ref.read().await;
        if let Some(config) = cache.get() {
            return Ok(config);
        }
    }

    let config = GLOBAL_DYNAMIC_CONFIG.read().await;
    let resolved = config.resolve_ai_config(tier);

    let api_key = resolved.api_key.filter(|k| !k.is_empty());
    let ai_config = api_key.map(|key| {
        let provider = AiProvider::from_str(&resolved.provider);
        let base_url = if resolved.base_url.is_empty() {
            None
        } else {
            Some(resolved.base_url.clone())
        };
        AiConfig {
            provider,
            api_key: key,
            model: resolved.model.clone(),
            base_url,
        }
    });

    match ai_config {
        Some(cfg) => {
            let mut cache = cache_ref.write().await;
            cache.set(cfg.clone());
            Ok(cfg)
        }
        None => Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "No AI provider configured" })),
        )),
    }
}

/// 获取 AI 图片生成配置（带缓存）
pub async fn get_ai_image_config() -> Result<AiImageConfig, (StatusCode, Json<Value>)> {
    {
        let cache = AI_IMAGE_CONFIG_CACHE.read().await;
        if let Some(config) = cache.get() {
            return Ok(config);
        }
    }

    let config = GLOBAL_DYNAMIC_CONFIG.read().await;
    let image_config = AiImageConfig {
        provider: config.ai_image_provider.clone(),
        model: config.ai_image_model.clone(),
        width: config.ai_image_width as u32,
        height: config.ai_image_height as u32,
        pixai_api_key: config.pixai_api_key.clone(),
    };

    let mut cache = AI_IMAGE_CONFIG_CACHE.write().await;
    cache.set(image_config.clone());
    Ok(image_config)
}

// ============ 速率限制器 ============

const RATE_LIMIT_NAMESPACE: &str = "rate_limit";

#[derive(FromQueryResult)]
struct RateLimitRow {
    count: i64,
    expires_at: i64,
}

#[derive(FromQueryResult)]
struct CountRow {
    count: i64,
}

/// 获取操作的速率限制配置。
///
/// Returns `(limit, window_secs)`. Defaults are coarse per-(subject, tapp,
/// operation) caps for sandboxed traffic; host UI (no runtime grant) does not
/// use these keys. Tune here rather than adding a parallel limiter.
///
/// Host-proxied write classes (enforced in `host_attribution` after grant
/// validation) intentionally sit in the tens–low hundreds / minute range, with
/// stricter caps for manage/trust and speech synthesis.
pub fn get_rate_limit_config(operation: &str) -> (u32, u64) {
    match operation {
        "ai.task" => (20, 60),
        "ai.anonymous" => (10, 60),
        operation if operation.starts_with("network.fetch:") => (60, 60),
        "platform.write" => (30, 60),
        "storage.set" | "storage.clear" => (100, 60),
        // Host-proxied brew mutations (grant-bearing only).
        "brew.write" => (60, 60),
        "brew.comment" => (60, 60),
        "brew.manage" => (20, 60),
        // Host-proxied federation mutations.
        "federation.write" => (60, 60),
        "federation.message" => (120, 60),
        "federation.files" => (40, 60),
        "federation.trust" => (15, 60),
        // Host-proxied speech write paths (TTS/ASR POST).
        "speech.tts" => (30, 60),
        "speech.asr" => (30, 60),
        _ => (200, 60),
    }
}

/// Map a host-proxied [`TappPermission`] to a coarse rate-limit operation class.
///
/// Returns `None` for pure-read permissions (`brew:read`, `federation:read`)
/// and anything outside brew/federation/speech host proxies — those paths are
/// not subject to this host-attribution limiter. Callers must also skip safe
/// HTTP methods (GET/HEAD/OPTIONS) so e.g. `GET /api/speech/voices` is not
/// counted against `speech.tts`.
pub fn host_write_rate_limit_operation(permission: TappPermission) -> Option<&'static str> {
    match permission {
        TappPermission::BrewWrite => Some("brew.write"),
        TappPermission::BrewComment => Some("brew.comment"),
        TappPermission::BrewManage => Some("brew.manage"),
        TappPermission::FederationWrite => Some("federation.write"),
        TappPermission::FederationMessage => Some("federation.message"),
        TappPermission::FederationFiles => Some("federation.files"),
        TappPermission::FederationTrust => Some("federation.trust"),
        TappPermission::SpeechTts => Some("speech.tts"),
        TappPermission::SpeechAsr => Some("speech.asr"),
        // Reads and unrelated permissions: no additional host-proxy limit.
        _ => None,
    }
}

fn rate_limit_key(user_id: i32, tapp_id: &str, operation: &str) -> String {
    format!("{user_id}:{tapp_id}:{operation}")
}

fn rate_limit_record_id(key: &str) -> String {
    format!("{:x}", Sha256::digest(key.as_bytes()))
}

pub(super) fn anonymous_subject_fingerprint(value: &str) -> String {
    let secret = std::env::var("JWT_SECRET")
        .unwrap_or_else(|_| "myriad-development-anonymous-quota-v1".to_string());
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .expect("HMAC accepts arbitrary key lengths");
    mac.update(b"myriad-tapp-anonymous-quota-v1\0");
    mac.update(value.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

fn rate_limit_unavailable(error: impl std::fmt::Display) -> (StatusCode, Json<Value>) {
    tracing::error!(%error, "[TAPP] Shared rate limiter unavailable");
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({
            "error": "Rate limiter unavailable",
            "code": "RATE_LIMITER_UNAVAILABLE"
        })),
    )
}

async fn load_rate_limit_row(
    db: &impl ConnectionTrait,
    record_id: &str,
) -> Result<Option<RateLimitRow>, sea_orm::DbErr> {
    RateLimitRow::find_by_statement(Statement::from_sql_and_values(
        DbBackend::Postgres,
        r#"
SELECT
    COALESCE((payload ->> 'count')::BIGINT, 0) AS count,
    expires_at
FROM tapp_runtime_registry
WHERE namespace = $1 AND record_id = $2
"#,
        vec![RATE_LIMIT_NAMESPACE.into(), record_id.into()],
    ))
    .one(db)
    .await
}

/// 检查速率限制
pub async fn check_rate_limit(
    user_id: i32,
    tapp_id: &str,
    operation: &str,
) -> Result<(), (StatusCode, Json<Value>)> {
    check_rate_limit_key(
        user_id,
        rate_limit_key(user_id, tapp_id, operation),
        tapp_id,
        operation,
    )
    .await
}

/// Coarse anonymous limiter keyed by a one-way client-address fingerprint.
/// The source address itself is never persisted in the runtime registry.
pub async fn check_anonymous_rate_limit(
    client_ip: Option<&str>,
    tapp_id: &str,
) -> Result<(), (StatusCode, Json<Value>)> {
    let fingerprint = anonymous_subject_fingerprint(client_ip.unwrap_or("unresolved"));
    check_rate_limit_key(
        0,
        format!("anonymous:{fingerprint}:{tapp_id}:ai.anonymous"),
        tapp_id,
        "ai.anonymous",
    )
    .await
}

async fn check_rate_limit_key(
    registry_subject_id: i32,
    key: String,
    tapp_id: &str,
    operation: &str,
) -> Result<(), (StatusCode, Json<Value>)> {
    let (limit, window_secs) = get_rate_limit_config(operation);
    let record_id = rate_limit_record_id(&key);
    let db = super::shared_registry::database()
        .await
        .map_err(rate_limit_unavailable)?;
    let transaction = db.begin().await.map_err(rate_limit_unavailable)?;
    transaction
        .execute(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
            vec![format!("tapp_rate_limit:{key}").into()],
        ))
        .await
        .map_err(rate_limit_unavailable)?;

    let now = chrono::Utc::now().timestamp();
    let current = load_rate_limit_row(&transaction, &record_id)
        .await
        .map_err(rate_limit_unavailable)?;
    let (count, expires_at) = match current {
        Some(row) if row.expires_at > now => (row.count.max(0) as u32, row.expires_at),
        _ => (0, now.saturating_add(window_secs as i64)),
    };
    let allowed = count < limit;
    let remaining = limit.saturating_sub(count.saturating_add(u32::from(allowed)));
    let reset_in = expires_at.saturating_sub(now) as u64;

    if allowed {
        transaction
            .execute(Statement::from_sql_and_values(
                DbBackend::Postgres,
                r#"
INSERT INTO tapp_runtime_registry
    (namespace, record_id, subject_id, tapp_id, payload, expires_at, updated_at)
VALUES ($1, $2, $3, $4, jsonb_build_object('count', $5::BIGINT), $6, NOW())
ON CONFLICT (namespace, record_id) DO UPDATE SET
    subject_id = EXCLUDED.subject_id,
    tapp_id = EXCLUDED.tapp_id,
    payload = EXCLUDED.payload,
    expires_at = EXCLUDED.expires_at,
    updated_at = NOW()
"#,
                vec![
                    RATE_LIMIT_NAMESPACE.into(),
                    record_id.into(),
                    registry_subject_id.into(),
                    tapp_id.to_string().into(),
                    i64::from(count.saturating_add(1)).into(),
                    expires_at.into(),
                ],
            ))
            .await
            .map_err(rate_limit_unavailable)?;
        transaction.commit().await.map_err(rate_limit_unavailable)?;
        super::shared_registry::maybe_cleanup(&db).await;
    } else {
        transaction
            .rollback()
            .await
            .map_err(rate_limit_unavailable)?;
    }

    if !allowed {
        tracing::warn!(
            user_id = registry_subject_id,
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
                "limit": limit,
                "remaining": remaining
            })),
        ));
    }

    Ok(())
}

/// 获取速率限制状态（只读，不记录）
pub async fn get_rate_limit_status_for(
    user_id: i32,
    tapp_id: &str,
    operation: &str,
) -> Result<(u32, u32, u64), (StatusCode, Json<Value>)> {
    let (limit, window_secs) = get_rate_limit_config(operation);
    let key = rate_limit_key(user_id, tapp_id, operation);
    let record_id = rate_limit_record_id(&key);
    let db = super::shared_registry::database()
        .await
        .map_err(rate_limit_unavailable)?;
    let now = chrono::Utc::now().timestamp();
    let Some(row) = load_rate_limit_row(&db, &record_id)
        .await
        .map_err(rate_limit_unavailable)?
    else {
        return Ok((0, limit, window_secs));
    };
    if row.expires_at <= now {
        return Ok((0, limit, 0));
    }
    let used = row.count.clamp(0, i64::from(u32::MAX)) as u32;
    Ok((
        used,
        limit.saturating_sub(used),
        row.expires_at.saturating_sub(now) as u64,
    ))
}

pub async fn get_rate_limiter_active_count() -> Result<usize, (StatusCode, Json<Value>)> {
    let db = super::shared_registry::database()
        .await
        .map_err(rate_limit_unavailable)?;
    let row = CountRow::find_by_statement(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "SELECT COUNT(*)::BIGINT AS count FROM tapp_runtime_registry WHERE namespace = $1 AND expires_at > EXTRACT(EPOCH FROM NOW())::BIGINT",
        vec![RATE_LIMIT_NAMESPACE.into()],
    ))
    .one(&db)
    .await
    .map_err(rate_limit_unavailable)?;
    Ok(row.map_or(0, |row| row.count.max(0) as usize))
}

// ============ 安全验证 ============

/// 管理员 ID 缓存（60秒 TTL，避免每次请求都查询数据库）
static ADMIN_ID_CACHE: Lazy<Arc<RwLock<SingleCache<i32>>>> =
    Lazy::new(|| Arc::new(RwLock::new(SingleCache::new(Duration::from_secs(60)))));

/// 获取可选管理员用户 ID（带缓存，使用参数化查询）。
///
/// 全新数据库在 setup 创建站点 owner 前合法地没有管理员；公开读取路径应把它视为空集合，
/// 需要 owner 的控制面路径再通过 `get_admin_user_id` 提升为错误。
pub async fn find_admin_user_id(
    db: &DatabaseConnection,
) -> Result<Option<i32>, (StatusCode, Json<Value>)> {
    // 检查缓存
    {
        let cache = ADMIN_ID_CACHE.read().await;
        if let Some(id) = cache.get() {
            return Ok(Some(id));
        }
    }

    let result = db
        .query_one(Statement::from_string(
            DbBackend::Postgres,
            // The public Tapp namespace belongs to the original site owner.
            // Multiple administrator accounts may exist, so selection must be
            // deterministic and match the profile/setup ownership rule.
            "SELECT id FROM users WHERE is_admin = true ORDER BY id ASC LIMIT 1".to_string(),
        ))
        .await
        .map_err(|e| {
            tracing::error!("[TAPP] Database error fetching admin ID: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Database error" })),
            )
        })?;

    let Some(result) = result else {
        return Ok(None);
    };

    let id = result.try_get::<i32>("", "id").map_err(|e| {
        tracing::error!("[TAPP] Error parsing admin ID: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Database error" })),
        )
    })?;

    // 写入缓存
    {
        let mut cache = ADMIN_ID_CACHE.write().await;
        cache.set(id);
    }

    Ok(Some(id))
}

/// 获取管理员用户 ID；仅用于确实要求站点 owner 已完成 setup 的路径。
pub async fn get_admin_user_id(db: &DatabaseConnection) -> Result<i32, (StatusCode, Json<Value>)> {
    find_admin_user_id(db).await?.ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "No admin user found" })),
        )
    })
}

/// 验证用户是否有权访问指定的 Tapp
///
/// 安全校验规则：
/// - 站点所有者、管理员和普通用户：只能运行站点所有者的公开 Tapp 或自己的安装
/// - 游客：只能运行站点所有者的公开 Tapp
///
/// 管理员的控制面权限不能隐式变成其他用户 Tapp 的代码、授权或私有数据访问权。
pub async fn verify_tapp_ownership(
    db: &DatabaseConnection,
    user_id: i32,
    tapp_id: &str,
) -> Result<(), (StatusCode, Json<Value>)> {
    let admin_id = get_admin_user_id(db).await?;
    let is_guest = user_id < 0;
    let mut query = tapps::Entity::find()
        .filter(tapps::Column::TappId.eq(tapp_id))
        .filter(tapps::Column::UserId.eq(admin_id));
    if !is_guest && user_id != admin_id {
        query = tapps::Entity::find()
            .filter(tapps::Column::TappId.eq(tapp_id))
            .filter(
                tapps::Column::UserId
                    .eq(admin_id)
                    .or(tapps::Column::UserId.eq(user_id)),
            );
    }
    let tapp = query.one(db).await.map_err(|e| {
        tracing::error!("[TAPP] Database error in ownership verification: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Database error" })),
        )
    })?;

    if tapp.is_none() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "Access denied",
                "message": if is_guest {
                    "This Tapp is not available for guest access"
                } else {
                    "You do not have permission to access this Tapp"
                }
            })),
        ));
    }

    Ok(())
}

/// Resolve the exact installation record used to execute a Tapp for this subject.
///
/// When the subject has a private install of the same `tapp_id`, that record wins over the
/// site-owner public install so code, resources, APIs, grants and storage all come from the
/// private copy. Guests and users without a private copy use the public admin install.
pub async fn resolve_accessible_tapp(
    db: &DatabaseConnection,
    user_id: i32,
    tapp_id: &str,
) -> Result<tapps::Model, (StatusCode, Json<Value>)> {
    let admin_id = get_admin_user_id(db).await?;
    let mut owner_ids = vec![admin_id];
    if user_id >= 0 && user_id != admin_id {
        owner_ids.push(user_id);
    }
    let mut candidates = tapps::Entity::find()
        .filter(tapps::Column::TappId.eq(tapp_id))
        .filter(tapps::Column::UserId.is_in(owner_ids))
        .all(db)
        .await
        .map_err(|error| {
            tracing::error!(%error, "[TAPP] Failed to resolve accessible Tapp");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Database error" })),
            )
        })?;
    candidates.sort_by_key(|tapp| tapp_owner_priority(tapp.user_id, user_id, admin_id));
    candidates.into_iter().next().ok_or_else(|| {
        (
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "Access denied",
                "message": if user_id < 0 {
                    "This Tapp is not available for guest access"
                } else {
                    "You do not have permission to access this Tapp"
                }
            })),
        )
    })
}

/// 验证当前可访问的 Tapp 安装记录确实获得了指定权限。
///
/// 角色级权限下放只能说明调用者角色可以使用该能力；这里再检查安装时授权，
/// 防止客户端伪造 tapp_id 绕过 manifest/approved_permissions。
pub async fn verify_tapp_approved_permissions(
    db: &DatabaseConnection,
    user_id: i32,
    tapp_id: &str,
    permissions: &[TappPermission],
) -> Result<(), (StatusCode, Json<Value>)> {
    let tapp = resolve_accessible_tapp(db, user_id, tapp_id).await?;
    let approved_permissions = tapp
        .approved_permissions
        .as_array()
        .cloned()
        .unwrap_or_default();
    let missing_permission = permissions.iter().find(|permission| {
        !approved_permissions
            .iter()
            .any(|value| value.as_str() == Some(permission.as_str()))
    });
    if let Some(permission) = missing_permission {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "Permission denied",
                "message": format!("Tapp was not granted '{}'", permission.as_str()),
                "code": "TAPP_PERMISSION_NOT_GRANTED"
            })),
        ));
    }
    Ok(())
}

/// Priority for install selection: 0 = subject's private, 1 = site admin public, 2 = other.
/// Used by `resolve_accessible_tapp` (and declared-API paths that call it).
pub fn tapp_owner_priority(owner_id: i32, user_id: i32, admin_id: i32) -> u8 {
    // Prefer the subject's private install when both private and public copies exist so
    // runtime grants and code resolve deterministically. Sandbox storage is separately
    // subject-scoped. Guests only resolve the public admin install.
    if user_id >= 0 && owner_id == user_id {
        0
    } else if owner_id == admin_id {
        1
    } else {
        2
    }
}

/// 完整授权一个带 `tapp_id` 的运行时能力调用。
///
/// 同时验证角色级权限下放、当前用户可访问该 Tapp，以及安装记录确实获授此权限。
/// 返回解析后的用户 ID，避免各端点重复且容易漏掉其中一层检查。
pub async fn authorize_tapp_permission(
    db: &DatabaseConnection,
    claims: &Claims,
    tapp_id: &str,
    permission: TappPermission,
) -> Result<i32, (StatusCode, Json<Value>)> {
    authorize_tapp_permissions(db, claims, tapp_id, &[permission]).await
}

pub async fn authorize_tapp_permissions(
    db: &DatabaseConnection,
    claims: &Claims,
    tapp_id: &str,
    permissions: &[TappPermission],
) -> Result<i32, (StatusCode, Json<Value>)> {
    for permission in permissions {
        check_tapp_permission(claims, *permission).await?;
    }
    let user_id = parse_user_id(claims)?;
    verify_tapp_approved_permissions(db, user_id, tapp_id, permissions).await?;
    Ok(user_id)
}

/// 从 Claims 解析 user_id
pub fn parse_user_id(claims: &Claims) -> Result<i32, (StatusCode, Json<Value>)> {
    claims.sub.parse().map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Invalid user ID" })),
        )
    })
}

/// 检查用户是否拥有特定 Tapp 权限
pub async fn check_tapp_permission(
    claims: &Claims,
    permission: TappPermission,
) -> Result<(), (StatusCode, Json<Value>)> {
    let role = current_tapp_user_role(claims).await;

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

/// Resolve the current role used by Tapp capability filtering.
pub async fn current_tapp_user_role(claims: &Claims) -> UserRole {
    if claims.is_admin && ensure_current_admin(claims).await.is_ok() {
        UserRole::Admin
    } else if let Ok(user_id) = claims.sub.parse::<i32>() {
        if user_id < 0 {
            UserRole::Guest
        } else {
            UserRole::User
        }
    } else {
        UserRole::Guest
    }
}

// ============ Prompt 安全验证 ============

/// 验证提示词安全性（后端层）
pub fn validate_prompt_security(prompt: &str) -> Option<String> {
    let prompt_lower = prompt.to_lowercase();

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

    if prompt_lower.contains("api_key")
        || prompt_lower.contains("api-key")
        || prompt_lower.contains("apikey")
        || prompt_lower.contains("private_key")
        || prompt_lower.contains("secret_key")
        || prompt_lower.contains("access_token")
    {
        return Some("Sensitive information probe detected".to_string());
    }

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

/// 验证图片提示词安全性
pub fn validate_image_prompt_security(prompt: &str) -> Option<String> {
    let prompt_lower = prompt.to_lowercase();

    let nsfw_patterns = [
        "nude", "naked", "nsfw", "porn", "xxx", "hentai", "explicit", "sexual", "erotic", "fetish",
    ];
    for pattern in &nsfw_patterns {
        if prompt_lower.contains(pattern) {
            return Some(format!("NSFW content detected: {}", pattern));
        }
    }

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

#[cfg(test)]
mod tests {
    use super::{
        get_rate_limit_config, host_write_rate_limit_operation, rate_limit_key,
        rate_limit_record_id, tapp_owner_priority,
    };
    use crate::services::permission_service::TappPermission;

    #[test]
    fn private_install_precedes_same_id_admin_tapp() {
        // Subject's private install wins over site-owner public install.
        assert_eq!(tapp_owner_priority(42, 42, 1), 0);
        assert_eq!(tapp_owner_priority(1, 42, 1), 1);
        assert_eq!(tapp_owner_priority(99, 42, 1), 2);
        // Guests never match a private owner_id; public admin still ranks above unrelated.
        assert_eq!(tapp_owner_priority(1, -1, 1), 1);
        assert_eq!(tapp_owner_priority(99, -1, 1), 2);
    }

    #[test]
    fn rate_limit_keys_are_identity_and_operation_scoped() {
        let base = rate_limit_key(42, "com.example.notes", "storage.set");
        assert_ne!(base, rate_limit_key(43, "com.example.notes", "storage.set"));
        assert_ne!(base, rate_limit_key(42, "com.example.tasks", "storage.set"));
        assert_ne!(base, rate_limit_key(42, "com.example.notes", "ai.task"));
        assert_eq!(rate_limit_record_id(&base).len(), 64);
        assert_eq!(rate_limit_record_id(&base), rate_limit_record_id(&base));
    }

    #[test]
    fn host_write_permissions_map_to_operation_classes() {
        assert_eq!(
            host_write_rate_limit_operation(TappPermission::BrewWrite),
            Some("brew.write")
        );
        assert_eq!(
            host_write_rate_limit_operation(TappPermission::BrewComment),
            Some("brew.comment")
        );
        assert_eq!(
            host_write_rate_limit_operation(TappPermission::BrewManage),
            Some("brew.manage")
        );
        assert_eq!(
            host_write_rate_limit_operation(TappPermission::FederationWrite),
            Some("federation.write")
        );
        assert_eq!(
            host_write_rate_limit_operation(TappPermission::FederationMessage),
            Some("federation.message")
        );
        assert_eq!(
            host_write_rate_limit_operation(TappPermission::FederationFiles),
            Some("federation.files")
        );
        assert_eq!(
            host_write_rate_limit_operation(TappPermission::FederationTrust),
            Some("federation.trust")
        );
        assert_eq!(
            host_write_rate_limit_operation(TappPermission::SpeechTts),
            Some("speech.tts")
        );
        assert_eq!(
            host_write_rate_limit_operation(TappPermission::SpeechAsr),
            Some("speech.asr")
        );
    }

    #[test]
    fn host_read_permissions_are_not_rate_limited() {
        assert_eq!(
            host_write_rate_limit_operation(TappPermission::BrewRead),
            None
        );
        assert_eq!(
            host_write_rate_limit_operation(TappPermission::FederationRead),
            None
        );
        // Unrelated capabilities stay outside the host-proxy limiter.
        assert_eq!(
            host_write_rate_limit_operation(TappPermission::PlatformWrite),
            None
        );
        assert_eq!(
            host_write_rate_limit_operation(TappPermission::AiGenerate),
            None
        );
    }

    #[test]
    fn host_write_rate_limit_defaults_are_sensible() {
        // (limit, window_secs) — tens–low hundreds / minute; manage/trust/speech stricter.
        assert_eq!(get_rate_limit_config("brew.write"), (60, 60));
        assert_eq!(get_rate_limit_config("brew.comment"), (60, 60));
        assert_eq!(get_rate_limit_config("brew.manage"), (20, 60));
        assert_eq!(get_rate_limit_config("federation.write"), (60, 60));
        assert_eq!(get_rate_limit_config("federation.message"), (120, 60));
        assert_eq!(get_rate_limit_config("federation.files"), (40, 60));
        assert_eq!(get_rate_limit_config("federation.trust"), (15, 60));
        assert_eq!(get_rate_limit_config("speech.tts"), (30, 60));
        assert_eq!(get_rate_limit_config("speech.asr"), (30, 60));

        // Stricter classes stay below chatty ones.
        assert!(get_rate_limit_config("brew.manage").0 < get_rate_limit_config("brew.write").0);
        assert!(
            get_rate_limit_config("federation.trust").0
                < get_rate_limit_config("federation.message").0
        );

        // Existing keys unchanged.
        assert_eq!(get_rate_limit_config("ai.task"), (20, 60));
        assert_eq!(get_rate_limit_config("platform.write"), (30, 60));
        assert_eq!(get_rate_limit_config("storage.set"), (100, 60));
    }
}
