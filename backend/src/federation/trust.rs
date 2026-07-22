//! 联邦信任策略模块（Phase 5 补全 — Layer 2 安全增强）
//!
//! 实际入站 enforcement（`enforce_inbound`）执行：
//! 1. 域名黑名单（`federation_instances.is_blocked`）
//! 2. 速率限制（进程内窗口计数 + DB `received_at` 统计）
//! 3. allowlist / min_trust（`federation_policy_settings`，空 allowlist = 不限制）
//! 4. 内容过滤（`federation_content_filters`）

#![allow(dead_code)]

use axum::http::StatusCode;
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::federation::types::TrustLevel;

// ==================== 类型定义 ====================

/// 实例策略（allowlist / min_trust / auto_discover + 入站限流）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstancePolicy {
    /// 全局最低信任层级（低于此层级的实例请求将被拒绝）
    pub min_trust_level: TrustLevel,
    /// 允许的域名列表（空 = 允许所有未被封禁的）
    pub allowed_domains: Vec<String>,
    /// 封禁的域名列表（优先级最高）
    pub blocked_domains: Vec<String>,
    /// 是否自动登记新发现的实例（设为 Discovered）
    pub auto_discover: bool,
    /// 入站限流（可在管理后台「高级」中配置）
    pub rate_limit: RateLimitPolicy,
}

impl Default for InstancePolicy {
    fn default() -> Self {
        Self {
            min_trust_level: TrustLevel::Unknown,
            allowed_domains: Vec::new(),
            blocked_domains: Vec::new(),
            auto_discover: true,
            rate_limit: RateLimitPolicy::default(),
        }
    }
}

/// 速率限制条目
#[derive(Debug, Clone)]
pub struct RateLimitEntry {
    pub domain: String,
    /// 时间窗口内的请求计数
    pub request_count: i64,
    /// 窗口开始时间
    pub window_start: chrono::DateTime<chrono::Utc>,
}

/// 速率限制策略
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitPolicy {
    /// 每个窗口允许的最大请求数
    pub max_requests_per_window: i64,
    /// 时间窗口大小（秒）
    pub window_seconds: i64,
    /// 信任实例的倍率加成
    pub trusted_multiplier: i64,
}

impl Default for RateLimitPolicy {
    fn default() -> Self {
        Self {
            max_requests_per_window: 100,
            window_seconds: 60,
            trusted_multiplier: 5,
        }
    }
}

/// 内容过滤规则
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentFilterRule {
    /// 规则名称
    pub name: String,
    /// 过滤类型: block_activity_type, block_keyword, require_trust_level
    pub filter_type: String,
    /// 过滤值（Activity 类型名、关键词、或信任层级数字）
    pub value: String,
    /// 是否启用
    pub enabled: bool,
}

/// 内容过滤裁决
#[derive(Debug, Clone)]
pub enum FilterVerdict {
    /// 允许通过
    Allow,
    /// 拒绝并给出原因
    Reject(String),
}

/// 策略检查结果
#[derive(Debug, Clone, Serialize)]
pub struct PolicyCheckResult {
    pub allowed: bool,
    pub reason: Option<String>,
    pub trust_level: Option<i16>,
}

// ==================== 实例策略执行 ====================

/// 检查实例是否被允许与本实例联邦
///
/// 优先级：blocked_domains > allowed_domains > min_trust_level
///
/// `enforce_inbound` 会在 DB 黑名单与速率限制之后调用本函数
///（blocked_domains 字段通常为空，DB `is_blocked` 已先检查）。
pub async fn check_instance_policy(
    db: &DatabaseConnection,
    domain: &str,
    policy: &InstancePolicy,
) -> PolicyCheckResult {
    let domain_lc = domain.to_lowercase();
    // 1. 黑名单检查（最高优先级）
    if policy
        .blocked_domains
        .iter()
        .any(|d| d.eq_ignore_ascii_case(&domain_lc))
    {
        return PolicyCheckResult {
            allowed: false,
            reason: Some(format!("Domain {} is blocked", domain)),
            trust_level: None,
        };
    }

    // 2. 白名单检查（如果白名单非空，则只允许白名单中的域名）
    if !policy.allowed_domains.is_empty()
        && !policy
            .allowed_domains
            .iter()
            .any(|d| d.eq_ignore_ascii_case(&domain_lc))
    {
        return PolicyCheckResult {
            allowed: false,
            reason: Some(format!("Domain {} is not in allowed list", domain)),
            trust_level: None,
        };
    }

    // 3. 查询实例信任层级
    let trust = get_instance_trust_level(db, domain).await;

    // 如果实例未知且开启自动发现，自动设为 Discovered
    let effective_trust = if trust == TrustLevel::Unknown && policy.auto_discover {
        let _ = ensure_instance_discovered(db, domain).await;
        TrustLevel::Discovered
    } else {
        trust
    };

    // 4. 信任层级门槛检查
    if effective_trust < policy.min_trust_level {
        return PolicyCheckResult {
            allowed: false,
            reason: Some(format!(
                "Domain {} trust level {:?} below minimum {:?}",
                domain, effective_trust, policy.min_trust_level
            )),
            trust_level: Some(effective_trust as i16),
        };
    }

    PolicyCheckResult {
        allowed: true,
        reason: None,
        trust_level: Some(effective_trust as i16),
    }
}

/// 获取实例的信任层级
pub async fn get_instance_trust_level(db: &DatabaseConnection, domain: &str) -> TrustLevel {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT trust_level FROM federation_instances WHERE domain = $1",
            [domain.into()],
        ))
        .await
        .ok()
        .flatten();

    match row {
        Some(r) => {
            let level: i16 = r.try_get("", "trust_level").unwrap_or(0);
            TrustLevel::from_i16(level)
        }
        None => TrustLevel::Unknown,
    }
}

/// 设置实例信任层级
pub async fn set_instance_trust_level(
    db: &DatabaseConnection,
    domain: &str,
    level: TrustLevel,
) -> Result<(), sea_orm::DbErr> {
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"UPDATE federation_instances SET trust_level = $2 WHERE domain = $1"#,
        [domain.into(), (level as i16).into()],
    ))
    .await?;
    Ok(())
}

/// 确保实例至少被记录为 Discovered
async fn ensure_instance_discovered(
    db: &DatabaseConnection,
    domain: &str,
) -> Result<(), sea_orm::DbErr> {
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"INSERT INTO federation_instances (domain, trust_level, created_at)
           VALUES ($1, $2, NOW())
           ON CONFLICT (domain) DO NOTHING"#,
        [domain.into(), (TrustLevel::Discovered as i16).into()],
    ))
    .await?;
    Ok(())
}

// ==================== 联邦速率限制 ====================

/// Process-local per-domain window counter (no Redis).
/// Complements DB counts: Follow / MFP paths may never insert `federation_activities`.
#[derive(Debug, Clone)]
struct DomainWindow {
    window_start: Instant,
    count: i64,
}

fn inbound_windows() -> &'static Mutex<HashMap<String, DomainWindow>> {
    static WINDOWS: OnceLock<Mutex<HashMap<String, DomainWindow>>> = OnceLock::new();
    WINDOWS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Pure helper: given current map state, return new entry after recording one hit
/// in a fixed window of `window_seconds`. Returns `(next_entry, exceeded)`.
fn record_window_hit(
    entry: Option<&DomainWindow>,
    now: Instant,
    window_seconds: i64,
    max_requests: i64,
) -> (DomainWindow, bool) {
    let window = Duration::from_secs(window_seconds.max(1) as u64);
    let next = match entry {
        Some(e) if now.duration_since(e.window_start) < window => DomainWindow {
            window_start: e.window_start,
            count: e.count + 1,
        },
        _ => DomainWindow {
            window_start: now,
            count: 1,
        },
    };
    let exceeded = next.count > max_requests;
    (next, exceeded)
}

/// Record one inbound hit for `domain` in the process-local window.
/// Returns `true` if the domain is still under the limit after this hit.
fn check_and_record_memory_rate(domain: &str, max_requests: i64, window_seconds: i64) -> bool {
    let now = Instant::now();
    let mut map = match inbound_windows().lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    let prev = map.get(domain);
    let (next, exceeded) = record_window_hit(prev, now, window_seconds, max_requests);
    map.insert(domain.to_string(), next);
    !exceeded
}

/// Effective max requests for a trust level under a rate policy.
fn effective_max_requests(policy: &RateLimitPolicy, trust: TrustLevel) -> i64 {
    let multiplier = if trust >= TrustLevel::Trusted {
        policy.trusted_multiplier
    } else {
        1
    };
    policy.max_requests_per_window * multiplier
}

/// 检查某域名的联邦请求是否超过速率限制
///
/// 1. Process-local window counter (catches all inbound, not just stored activities)
/// 2. Durable DB count using **received_at** (server-side), never client-spoofable published_at
pub async fn check_rate_limit(
    db: &DatabaseConnection,
    domain: &str,
    policy: &RateLimitPolicy,
) -> PolicyCheckResult {
    let trust = get_instance_trust_level(db, domain).await;
    let max_requests = effective_max_requests(policy, trust);

    // Fast path: process-local counter (no Redis)
    if !check_and_record_memory_rate(domain, max_requests, policy.window_seconds) {
        return PolicyCheckResult {
            allowed: false,
            reason: Some(format!(
                "Rate limit exceeded for domain {}: >{} requests in {}s in-memory window",
                domain, max_requests, policy.window_seconds
            )),
            trust_level: Some(trust as i16),
        };
    }

    // Durable path: count inbound activities by received_at (set server-side on accept).
    // Fall back to published_at only when received_at is NULL (legacy rows).
    // Do not use published_at alone — remote senders control that timestamp.
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT COUNT(*) as cnt
               FROM federation_activities a
               JOIN federation_remote_actors ra ON a.remote_actor_id = ra.id
               WHERE a.is_local = false
                 AND ra.domain = $1
                 AND COALESCE(a.received_at, a.published_at)
                     > NOW() - make_interval(secs => $2::double precision)"#,
            [domain.into(), policy.window_seconds.into()],
        ))
        .await
        .ok()
        .flatten();

    let count: i64 = row.and_then(|r| r.try_get("", "cnt").ok()).unwrap_or(0);

    if count >= max_requests {
        PolicyCheckResult {
            allowed: false,
            reason: Some(format!(
                "Rate limit exceeded for domain {}: {}/{} requests in {}s window",
                domain, count, max_requests, policy.window_seconds
            )),
            trust_level: Some(trust as i16),
        }
    } else {
        PolicyCheckResult {
            allowed: true,
            reason: None,
            trust_level: Some(trust as i16),
        }
    }
}

// ==================== 内容过滤 ====================

/// 对入站 Activity 执行内容过滤规则
///
/// 支持三种过滤类型：
/// - block_activity_type: 阻止特定 Activity 类型
/// - block_keyword: 阻止包含特定关键词的内容
/// - require_trust_level: 特定操作要求最低信任层级
///
/// 对入站 Activity 套用 `federation_content_filters` 规则（见 `load_content_filter_rules`）。
pub fn apply_content_filters(
    activity: &serde_json::Value,
    domain_trust: TrustLevel,
    rules: &[ContentFilterRule],
) -> FilterVerdict {
    let activity_type = activity.get("type").and_then(|v| v.as_str()).unwrap_or("");

    for rule in rules {
        if !rule.enabled {
            continue;
        }

        match rule.filter_type.as_str() {
            "block_activity_type" => {
                if activity_type == rule.value {
                    return FilterVerdict::Reject(format!(
                        "Activity type '{}' blocked by rule '{}'",
                        activity_type, rule.name
                    ));
                }
            }
            "block_keyword" => {
                let content = activity.to_string().to_lowercase();
                let keyword = rule.value.to_lowercase();
                if content.contains(&keyword) {
                    return FilterVerdict::Reject(format!(
                        "Content contains blocked keyword, rule '{}'",
                        rule.name
                    ));
                }
            }
            "require_trust_level" => {
                let required: i16 = rule.value.parse().unwrap_or(0);
                let required_level = TrustLevel::from_i16(required);
                if domain_trust < required_level {
                    return FilterVerdict::Reject(format!(
                        "Trust level {:?} below required {:?} for rule '{}'",
                        domain_trust, required_level, rule.name
                    ));
                }
            }
            _ => {}
        }
    }

    FilterVerdict::Allow
}

// ==================== API 端点 ====================

/// 获取当前 *有效* 实例策略（管理员）
pub async fn get_policy(
    db: &DatabaseConnection,
) -> Result<serde_json::Value, (StatusCode, serde_json::Value)> {
    // 从 federation_instances 聚合统计 + 真实黑名单
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT
                 COUNT(*) as total_instances,
                 COUNT(*) FILTER (WHERE trust_level >= 3) as trusted_count,
                 COUNT(*) FILTER (WHERE trust_level = 0) as unknown_count,
                 COALESCE(
                   (SELECT json_agg(domain) FROM federation_instances WHERE is_blocked = true),
                   '[]'::json
                 ) as blocked_list
               FROM federation_instances"#,
            [],
        ))
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({"error": format!("DB error: {}", e)}),
            )
        })?;

    let (total, trusted, unknown, blocked_list) = match &row {
        Some(r) => (
            r.try_get::<i64>("", "total_instances").unwrap_or(0),
            r.try_get::<i64>("", "trusted_count").unwrap_or(0),
            r.try_get::<i64>("", "unknown_count").unwrap_or(0),
            r.try_get::<serde_json::Value>("", "blocked_list")
                .unwrap_or_else(|_| json!([])),
        ),
        None => (0, 0, 0, json!([])),
    };

    let policy = load_instance_policy(db).await;
    let allowlist_active = !policy.allowed_domains.is_empty();
    let min_trust_active = (policy.min_trust_level as i16) > 0;
    let filter_rules = load_content_filter_rules(db).await;
    let filters_enabled = filter_rules.iter().any(|r| r.enabled);
    let filters_json: Vec<serde_json::Value> = filter_rules
        .iter()
        .map(|r| {
            json!({
                "name": r.name,
                "filter_type": r.filter_type,
                "value": r.value,
                "enabled": r.enabled,
            })
        })
        .collect();

    Ok(json!({
        "enforcement": {
            "domain_blocklist": true,
            "rate_limit": true,
            "content_filters": filters_enabled,
            "allowlist": allowlist_active,
            "min_trust_level": min_trust_active,
        },
        "notes": {
            "domain_blocklist": "federation_instances.is_blocked is checked on inbound and outbound",
            "rate_limit": "per-domain window from federation_policy_settings; Trusted+ uses trusted_multiplier",
            "content_filters": "federation_content_filters rows are loaded and applied in enforce_inbound",
            "allowlist": "empty allowed_domains = allow all non-blocked; non-empty = only listed domains",
            "min_trust_level": "domains below min_trust_level are rejected on inbound (0 = Unknown, no floor)",
        },
        "blocked_domains": blocked_list,
        "allowed_domains": policy.allowed_domains,
        "min_trust_level": policy.min_trust_level as i16,
        "auto_discover": policy.auto_discover,
        "rate_limit": {
            "max_requests_per_window": policy.rate_limit.max_requests_per_window,
            "window_seconds": policy.rate_limit.window_seconds,
            "trusted_multiplier": policy.rate_limit.trusted_multiplier,
        },
        "content_filters": filters_json,
        "stats": {
            "total_instances": total,
            "trusted_count": trusted,
            "unknown_count": unknown
        }
    }))
}

/// 更新实例级策略（allowlist / min_trust / auto_discover / rate limit）— 管理员
pub async fn update_policy(
    db: &DatabaseConnection,
    min_trust_level: Option<i16>,
    allowed_domains: Option<Vec<String>>,
    auto_discover: Option<bool>,
    rate_max_requests: Option<i64>,
    rate_window_seconds: Option<i64>,
    rate_trusted_multiplier: Option<i64>,
) -> Result<serde_json::Value, (StatusCode, serde_json::Value)> {
    let mut current = load_instance_policy(db).await;
    if let Some(level) = min_trust_level {
        if !(0..=4).contains(&level) {
            return Err((
                StatusCode::BAD_REQUEST,
                json!({"error": "min_trust_level must be 0..=4"}),
            ));
        }
        current.min_trust_level = TrustLevel::from_i16(level);
    }
    if let Some(domains) = allowed_domains {
        current.allowed_domains = domains
            .into_iter()
            .map(|d| d.trim().to_lowercase())
            .filter(|d| !d.is_empty())
            .collect();
    }
    if let Some(ad) = auto_discover {
        current.auto_discover = ad;
    }
    if let Some(max) = rate_max_requests {
        if !(1..=1_000_000).contains(&max) {
            return Err((
                StatusCode::BAD_REQUEST,
                json!({"error": "rate_max_requests must be 1..=1000000"}),
            ));
        }
        current.rate_limit.max_requests_per_window = max;
    }
    if let Some(win) = rate_window_seconds {
        if !(1..=86_400).contains(&win) {
            return Err((
                StatusCode::BAD_REQUEST,
                json!({"error": "rate_window_seconds must be 1..=86400"}),
            ));
        }
        current.rate_limit.window_seconds = win;
    }
    if let Some(mul) = rate_trusted_multiplier {
        if !(1..=100).contains(&mul) {
            return Err((
                StatusCode::BAD_REQUEST,
                json!({"error": "rate_trusted_multiplier must be 1..=100"}),
            ));
        }
        current.rate_limit.trusted_multiplier = mul;
    }

    let domains_json = serde_json::to_value(&current.allowed_domains).unwrap_or_else(|_| json!([]));
    let level = current.min_trust_level as i16;

    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"INSERT INTO federation_policy_settings
               (id, min_trust_level, allowed_domains, auto_discover,
                rate_max_requests, rate_window_seconds, rate_trusted_multiplier, updated_at)
           VALUES (1, $1, $2, $3, $4, $5, $6, NOW())
           ON CONFLICT (id) DO UPDATE SET
               min_trust_level = EXCLUDED.min_trust_level,
               allowed_domains = EXCLUDED.allowed_domains,
               auto_discover = EXCLUDED.auto_discover,
               rate_max_requests = EXCLUDED.rate_max_requests,
               rate_window_seconds = EXCLUDED.rate_window_seconds,
               rate_trusted_multiplier = EXCLUDED.rate_trusted_multiplier,
               updated_at = NOW()"#,
        [
            level.into(),
            domains_json.into(),
            current.auto_discover.into(),
            current.rate_limit.max_requests_per_window.into(),
            current.rate_limit.window_seconds.into(),
            current.rate_limit.trusted_multiplier.into(),
        ],
    ))
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({"error": format!("DB error: {}", e)}),
        )
    })?;

    Ok(json!({
        "success": true,
        "min_trust_level": level,
        "allowed_domains": current.allowed_domains,
        "auto_discover": current.auto_discover,
        "rate_limit": {
            "max_requests_per_window": current.rate_limit.max_requests_per_window,
            "window_seconds": current.rate_limit.window_seconds,
            "trusted_multiplier": current.rate_limit.trusted_multiplier,
        }
    }))
}

/// Load persisted InstancePolicy (defaults if row missing).
async fn load_instance_policy(db: &DatabaseConnection) -> InstancePolicy {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT min_trust_level, allowed_domains, auto_discover,
                      rate_max_requests, rate_window_seconds, rate_trusted_multiplier
               FROM federation_policy_settings WHERE id = 1"#,
            [],
        ))
        .await
        .ok()
        .flatten();

    let Some(r) = row else {
        return InstancePolicy::default();
    };

    let min_level: i16 = r.try_get("", "min_trust_level").unwrap_or(0);
    let domains_val: serde_json::Value = r
        .try_get("", "allowed_domains")
        .unwrap_or_else(|_| json!([]));
    let allowed_domains: Vec<String> = domains_val
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_lowercase()))
                .collect()
        })
        .unwrap_or_default();
    let auto_discover: bool = r.try_get("", "auto_discover").unwrap_or(true);
    let defaults = RateLimitPolicy::default();
    let rate_limit = RateLimitPolicy {
        max_requests_per_window: r
            .try_get::<i64>("", "rate_max_requests")
            .unwrap_or(defaults.max_requests_per_window)
            .max(1),
        window_seconds: r
            .try_get::<i64>("", "rate_window_seconds")
            .unwrap_or(defaults.window_seconds)
            .max(1),
        trusted_multiplier: r
            .try_get::<i64>("", "rate_trusted_multiplier")
            .unwrap_or(defaults.trusted_multiplier)
            .max(1),
    };

    InstancePolicy {
        min_trust_level: TrustLevel::from_i16(min_level),
        allowed_domains,
        blocked_domains: Vec::new(), // DB is_blocked is authoritative
        auto_discover,
        rate_limit,
    }
}

/// 更新实例信任层级（管理员）
pub async fn update_instance_trust(
    db: &DatabaseConnection,
    domain: &str,
    level: i16,
) -> Result<serde_json::Value, (StatusCode, serde_json::Value)> {
    let trust_level = TrustLevel::from_i16(level);

    // 验证实例存在
    let exists = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT domain FROM federation_instances WHERE domain = $1",
            [domain.into()],
        ))
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({"error": format!("DB error: {}", e)}),
            )
        })?;

    if exists.is_none() {
        return Err((
            StatusCode::NOT_FOUND,
            json!({"error": format!("Instance {} not found", domain)}),
        ));
    }

    set_instance_trust_level(db, domain, trust_level)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({"error": format!("Failed to update: {}", e)}),
            )
        })?;

    Ok(json!({
        "success": true,
        "domain": domain,
        "trust_level": level,
        "trust_label": format!("{:?}", trust_level)
    }))
}

/// 封禁/解封实例（管理员）
pub async fn toggle_instance_block(
    db: &DatabaseConnection,
    domain: &str,
    block: bool,
) -> Result<serde_json::Value, (StatusCode, serde_json::Value)> {
    // 先确保实例存在
    let _ = ensure_instance_discovered(db, domain).await;

    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE federation_instances SET is_blocked = $2 WHERE domain = $1",
        [domain.into(), block.into()],
    ))
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({"error": format!("DB error: {}", e)}),
        )
    })?;

    let action = if block { "blocked" } else { "unblocked" };
    tracing::info!("[Trust] Instance {} {}", domain, action);

    Ok(json!({
        "success": true,
        "domain": domain,
        "blocked": block
    }))
}

/// 列出所有已知实例及信任状态
pub async fn list_instances(
    db: &DatabaseConnection,
) -> Result<serde_json::Value, (StatusCode, serde_json::Value)> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT domain, trust_level, software,
                      software_version AS version, is_blocked AS blocked,
                      created_at AS first_seen_at, last_seen_at AS last_fetched_at
               FROM federation_instances
               ORDER BY trust_level DESC, created_at ASC"#,
            [],
        ))
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({"error": format!("DB error: {}", e)}),
            )
        })?;

    let instances: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            json!({
                "domain": r.try_get::<String>("", "domain").unwrap_or_default(),
                "trust_level": r.try_get::<i16>("", "trust_level").unwrap_or(0),
                "software": r.try_get::<Option<String>>("", "software").unwrap_or(None),
                "version": r.try_get::<Option<String>>("", "version").unwrap_or(None),
                "blocked": r.try_get::<bool>("", "blocked").unwrap_or(false),
                "first_seen_at": r.try_get::<chrono::DateTime<chrono::FixedOffset>>("", "first_seen_at")
                    .map(|t| t.to_rfc3339()).unwrap_or_default(),
                "last_fetched_at": r.try_get::<Option<chrono::DateTime<chrono::FixedOffset>>>("", "last_fetched_at")
                    .ok().flatten().map(|t| t.to_rfc3339()),
            })
        })
        .collect();

    Ok(json!({
        "instances": instances,
        "total": instances.len()
    }))
}

// ==================== Enforcement (inbox / delivery 调用入口) ====================

/// 检查域名是否被封禁
async fn is_domain_blocked(db: &DatabaseConnection, domain: &str) -> bool {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT is_blocked FROM federation_instances WHERE domain = $1",
            [domain.into()],
        ))
        .await
        .ok()
        .flatten();
    row.map(|r| r.try_get::<bool>("", "is_blocked").unwrap_or(false))
        .unwrap_or(false)
}

/// 入站请求策略检查（inbox 调用）
///
/// 顺序：实例黑名单 → allowlist/min_trust → 速率限制 → 内容过滤
/// 返回 `Err(reason)` 表示拒绝。
pub async fn enforce_inbound(
    db: &DatabaseConnection,
    domain: &str,
    activity: &serde_json::Value,
) -> Result<(), String> {
    if domain.is_empty() {
        return Err("Empty domain".to_string());
    }

    if is_domain_blocked(db, domain).await {
        return Err(format!("Instance {} is blocked", domain));
    }

    // allowlist + min_trust from federation_policy_settings
    let policy = load_instance_policy(db).await;
    let verdict = check_instance_policy(db, domain, &policy).await;
    if !verdict.allowed {
        return Err(verdict
            .reason
            .unwrap_or_else(|| format!("Domain {} rejected by instance policy", domain)));
    }

    let rate = check_rate_limit(db, domain, &policy.rate_limit).await;
    if !rate.allowed {
        return Err(rate.reason.unwrap_or_else(|| "Rate limited".to_string()));
    }

    // Content filters from federation_content_filters (enabled rows only applied inside).
    let trust = get_instance_trust_level(db, domain).await;
    if let FilterVerdict::Reject(reason) =
        apply_content_filters(activity, trust, &load_content_filter_rules(db).await)
    {
        return Err(reason);
    }

    Ok(())
}

/// 出站投递策略检查（delivery 调用）
///
/// 仅检查目标实例是否被封禁 —— 投递不消耗入站速率配额。
pub async fn enforce_outbound(db: &DatabaseConnection, target_domain: &str) -> Result<(), String> {
    if target_domain.is_empty() {
        return Err("Empty target domain".to_string());
    }
    if is_domain_blocked(db, target_domain).await {
        return Err(format!("Target {} is blocked", target_domain));
    }
    Ok(())
}

/// 加载当前生效的内容过滤规则（`federation_content_filters`）
async fn load_content_filter_rules(db: &DatabaseConnection) -> Vec<ContentFilterRule> {
    let rows = match db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT name, filter_type, value, enabled
               FROM federation_content_filters
               ORDER BY id ASC"#,
            [],
        ))
        .await
    {
        Ok(r) => r,
        Err(e) => {
            // Table may not exist yet on brand-new DBs before schema heal.
            tracing::debug!("load_content_filter_rules: {}", e);
            return Vec::new();
        }
    };
    rows.iter()
        .map(|r| ContentFilterRule {
            name: r.try_get("", "name").unwrap_or_default(),
            filter_type: r.try_get("", "filter_type").unwrap_or_default(),
            value: r.try_get("", "value").unwrap_or_default(),
            enabled: r.try_get::<bool>("", "enabled").unwrap_or(true),
        })
        .collect()
}

/// List content filter rules (admin API)
pub async fn list_content_filters(
    db: &DatabaseConnection,
) -> Result<serde_json::Value, (StatusCode, serde_json::Value)> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT id, name, filter_type, value, enabled, created_at
               FROM federation_content_filters
               ORDER BY id ASC"#,
            [],
        ))
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({"error": format!("DB error: {}", e)}),
            )
        })?;
    let filters: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            json!({
                "id": r.try_get::<i32>("", "id").unwrap_or(0),
                "name": r.try_get::<String>("", "name").unwrap_or_default(),
                "filter_type": r.try_get::<String>("", "filter_type").unwrap_or_default(),
                "value": r.try_get::<String>("", "value").unwrap_or_default(),
                "enabled": r.try_get::<bool>("", "enabled").unwrap_or(true),
                "created_at": r
                    .try_get::<chrono::DateTime<chrono::FixedOffset>>("", "created_at")
                    .ok()
                    .map(|t| t.to_rfc3339())
                    .unwrap_or_default(),
            })
        })
        .collect();
    Ok(json!({ "filters": filters, "total": filters.len() }))
}

/// Create a content filter rule (admin)
pub async fn create_content_filter(
    db: &DatabaseConnection,
    name: &str,
    filter_type: &str,
    value: &str,
    enabled: bool,
) -> Result<serde_json::Value, (StatusCode, serde_json::Value)> {
    let name = name.trim();
    let filter_type = filter_type.trim();
    let value = value.trim();
    if name.is_empty() || filter_type.is_empty() || value.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            json!({"error": "name, filter_type, and value are required"}),
        ));
    }
    if !["block_activity_type", "block_keyword", "require_trust_level"].contains(&filter_type) {
        return Err((
            StatusCode::BAD_REQUEST,
            json!({"error": "filter_type must be block_activity_type | block_keyword | require_trust_level"}),
        ));
    }
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"INSERT INTO federation_content_filters (name, filter_type, value, enabled, created_at)
               VALUES ($1, $2, $3, $4, NOW())
               RETURNING id"#,
            [
                name.into(),
                filter_type.into(),
                value.into(),
                enabled.into(),
            ],
        ))
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({"error": format!("DB error: {}", e)}),
            )
        })?;
    let id = row
        .and_then(|r| r.try_get::<i32>("", "id").ok())
        .unwrap_or(0);
    Ok(json!({
        "success": true,
        "id": id,
        "name": name,
        "filter_type": filter_type,
        "value": value,
        "enabled": enabled
    }))
}

/// Update enabled flag or fields of a content filter (admin)
pub async fn update_content_filter(
    db: &DatabaseConnection,
    id: i32,
    name: Option<&str>,
    filter_type: Option<&str>,
    value: Option<&str>,
    enabled: Option<bool>,
) -> Result<serde_json::Value, (StatusCode, serde_json::Value)> {
    if id <= 0 {
        return Err((StatusCode::BAD_REQUEST, json!({"error": "invalid id"})));
    }
    if let Some(ft) = filter_type {
        if !["block_activity_type", "block_keyword", "require_trust_level"].contains(&ft) {
            return Err((
                StatusCode::BAD_REQUEST,
                json!({"error": "invalid filter_type"}),
            ));
        }
    }
    let existing = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT name, filter_type, value, enabled FROM federation_content_filters WHERE id = $1",
            [id.into()],
        ))
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({"error": format!("DB error: {}", e)}),
            )
        })?
        .ok_or_else(|| (StatusCode::NOT_FOUND, json!({"error": "filter not found"})))?;

    let new_name = name
        .map(|s| s.to_string())
        .unwrap_or_else(|| existing.try_get("", "name").unwrap_or_default());
    let new_type = filter_type
        .map(|s| s.to_string())
        .unwrap_or_else(|| existing.try_get("", "filter_type").unwrap_or_default());
    let new_value = value
        .map(|s| s.to_string())
        .unwrap_or_else(|| existing.try_get("", "value").unwrap_or_default());
    let new_enabled = enabled.unwrap_or_else(|| {
        existing
            .try_get::<bool>("", "enabled")
            .unwrap_or(true)
    });

    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"UPDATE federation_content_filters
           SET name = $2, filter_type = $3, value = $4, enabled = $5
           WHERE id = $1"#,
        [
            id.into(),
            new_name.into(),
            new_type.into(),
            new_value.into(),
            new_enabled.into(),
        ],
    ))
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({"error": format!("DB error: {}", e)}),
        )
    })?;
    Ok(json!({ "success": true, "id": id }))
}

/// Delete a content filter rule (admin)
pub async fn delete_content_filter(
    db: &DatabaseConnection,
    id: i32,
) -> Result<serde_json::Value, (StatusCode, serde_json::Value)> {
    let result = db
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "DELETE FROM federation_content_filters WHERE id = $1",
            [id.into()],
        ))
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({"error": format!("DB error: {}", e)}),
            )
        })?;
    if result.rows_affected() == 0 {
        return Err((StatusCode::NOT_FOUND, json!({"error": "filter not found"})));
    }
    Ok(json!({ "success": true, "id": id }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn record_window_hit_resets_after_window() {
        let t0 = Instant::now();
        let (e1, exceeded1) = record_window_hit(None, t0, 60, 3);
        assert_eq!(e1.count, 1);
        assert!(!exceeded1);

        let (e2, exceeded2) = record_window_hit(Some(&e1), t0 + Duration::from_secs(1), 60, 3);
        assert_eq!(e2.count, 2);
        assert!(!exceeded2);

        let (e3, exceeded3) = record_window_hit(Some(&e2), t0 + Duration::from_secs(2), 60, 3);
        assert_eq!(e3.count, 3);
        assert!(!exceeded3);

        let (e4, exceeded4) = record_window_hit(Some(&e3), t0 + Duration::from_secs(3), 60, 3);
        assert_eq!(e4.count, 4);
        assert!(exceeded4);

        // After window expires, counter resets
        let (e5, exceeded5) =
            record_window_hit(Some(&e4), t0 + Duration::from_secs(61), 60, 3);
        assert_eq!(e5.count, 1);
        assert!(!exceeded5);
    }

    #[test]
    fn effective_max_requests_trusted_multiplier() {
        let policy = RateLimitPolicy {
            max_requests_per_window: 100,
            window_seconds: 60,
            trusted_multiplier: 5,
        };
        assert_eq!(
            effective_max_requests(&policy, TrustLevel::Unknown),
            100
        );
        assert_eq!(
            effective_max_requests(&policy, TrustLevel::Trusted),
            500
        );
    }

    #[test]
    fn apply_content_filters_empty_rules_allow() {
        let activity = json!({"type": "Create", "content": "hello"});
        assert!(matches!(
            apply_content_filters(&activity, TrustLevel::Unknown, &[]),
            FilterVerdict::Allow
        ));
    }

    #[test]
    fn apply_content_filters_blocks_activity_type() {
        let activity = json!({"type": "Announce"});
        let rules = vec![ContentFilterRule {
            name: "no-boost".into(),
            filter_type: "block_activity_type".into(),
            value: "Announce".into(),
            enabled: true,
        }];
        assert!(matches!(
            apply_content_filters(&activity, TrustLevel::Unknown, &rules),
            FilterVerdict::Reject(_)
        ));
    }

    #[test]
    fn effective_max_requests_untrusted_is_base() {
        let policy = RateLimitPolicy {
            max_requests_per_window: 100,
            window_seconds: 60,
            trusted_multiplier: 5,
        };
        assert_eq!(effective_max_requests(&policy, TrustLevel::Unknown), 100);
        assert_eq!(effective_max_requests(&policy, TrustLevel::Discovered), 100);
        assert_eq!(effective_max_requests(&policy, TrustLevel::Followed), 100);

    }

    #[test]
    fn effective_max_requests_trusted_and_federated() {
        let policy = RateLimitPolicy {
            max_requests_per_window: 10,
            window_seconds: 60,
            trusted_multiplier: 3,
        };
        assert_eq!(effective_max_requests(&policy, TrustLevel::Trusted), 30);
        assert_eq!(effective_max_requests(&policy, TrustLevel::Federated), 30);

    }

    #[test]
    fn apply_content_filters_block_keyword() {
        let rules = vec![ContentFilterRule {
            name: "kw".into(),
            filter_type: "block_keyword".into(),
            value: "spam".into(),
            enabled: true,
        }];
        let act = serde_json::json!({"type": "Create", "content": "buy spam now"});
        assert!(
            matches!(
                apply_content_filters(&act, TrustLevel::Discovered, &rules),
                FilterVerdict::Reject(_)
            ),
            "keyword spam should block"
        );
    }

    #[test]
    fn apply_content_filters_disabled_rule_ignored() {
        let rules = vec![ContentFilterRule {
            name: "off".into(),
            filter_type: "block_activity_type".into(),
            value: "Announce".into(),
            enabled: false,
        }];
        let act = serde_json::json!({"type": "Announce"});
        assert!(matches!(
            apply_content_filters(&act, TrustLevel::Unknown, &rules),
            FilterVerdict::Allow
        ));
    }

    #[test]
    fn w175_effective_max_untrusted_base() {
        let policy = RateLimitPolicy {
            max_requests_per_window: 50,
            window_seconds: 60,
            trusted_multiplier: 4,
        };
        assert_eq!(effective_max_requests(&policy, TrustLevel::Unknown), 50);
        assert_eq!(effective_max_requests(&policy, TrustLevel::Discovered), 50);
        assert_eq!(effective_max_requests(&policy, TrustLevel::Followed), 50);

    }


    #[test]
    fn w175_effective_max_trusted_mul() {
        let policy = RateLimitPolicy {
            max_requests_per_window: 10,
            window_seconds: 60,
            trusted_multiplier: 3,
        };
        assert_eq!(effective_max_requests(&policy, TrustLevel::Trusted), 30);
        assert_eq!(effective_max_requests(&policy, TrustLevel::Federated), 30);

    }


    #[test]
    fn w175_filter_block_keyword() {
        let rules = vec![ContentFilterRule {
            name: "kw".into(),
            filter_type: "block_keyword".into(),
            value: "spam".into(),
            enabled: true,
        }];
        let act = serde_json::json!({"type": "Create", "content": "buy spam now"});
        assert!(matches!(
            apply_content_filters(&act, TrustLevel::Discovered, &rules),
            FilterVerdict::Reject(_)
        ));

    }


    #[test]
    fn w175_filter_disabled_ignored() {
        let rules = vec![ContentFilterRule {
            name: "off".into(),
            filter_type: "block_activity_type".into(),
            value: "Announce".into(),
            enabled: false,
        }];
        let act = serde_json::json!({"type": "Announce"});
        assert!(matches!(
            apply_content_filters(&act, TrustLevel::Unknown, &rules),
            FilterVerdict::Allow
        ));

    }

}

