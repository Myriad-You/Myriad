//! RSSHub 服务
//!
//! 提供 RSSHub 实例管理、健康检查、自动故障转移功能

use chrono::Utc;
use reqwest::Url;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseBackend,
    DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, Statement, Value as SeaValue,
};
use std::time::{Duration, Instant};

use crate::models::entities::rsshub_instances::{self, HealthStatus, Model as InstanceModel};

fn rsshub_store_failed(context: &'static str, error: impl std::fmt::Display) -> String {
    tracing::error!(%error, context, "rsshub store failed");
    format!("Failed to {context}")
}

fn unique_violation(err: &impl std::fmt::Display) -> bool {
    let lower = err.to_string().to_ascii_lowercase();
    lower.contains("23505") || lower.contains("duplicate key") || lower.contains("unique")
}

/// Pure: path + query from an RSSHub-style URL (no host/fragment).
///
/// Used by [`RsshubService::extract_route`] and unit-tested without a DB.
pub fn extract_rsshub_route_path_and_query(url: &str) -> Option<String> {
    fn path_and_query(parsed: &Url) -> Option<String> {
        let path = parsed.path();
        if path.is_empty() || path == "/" {
            return None;
        }
        match parsed.query() {
            Some(q) if !q.is_empty() => Some(format!("{path}?{q}")),
            _ => Some(path.to_string()),
        }
    }

    let rsshub_patterns = [
        "rsshub.app",
        "rsshub.rssforever.com",
        "hub.slarker.me",
        "rsshub.feeded.xyz",
        "rsshub.ktachibana.party",
    ];

    for pattern in &rsshub_patterns {
        if url.contains(pattern) {
            if let Ok(parsed) = Url::parse(url) {
                if let Some(route) = path_and_query(&parsed) {
                    return Some(route);
                }
            }
        }
    }

    if url.to_lowercase().contains("rsshub") {
        if let Ok(parsed) = Url::parse(url) {
            if let Some(route) = path_and_query(&parsed) {
                return Some(route);
            }
        }
    }

    None
}
use crate::services::phantasi_parser::{FeedParser, ParsedFeed};

/// RSSHub 服务配置
#[derive(Clone, Debug)]
pub struct RsshubConfig {
    /// 请求超时（秒）
    pub request_timeout_secs: u64,
    /// 连续失败多少次后标记为不健康
    pub unhealthy_threshold: i32,
    /// 响应时间超过多少毫秒标记为降级
    pub degraded_threshold_ms: i32,
}

impl Default for RsshubConfig {
    fn default() -> Self {
        Self {
            request_timeout_secs: 30,
            unhealthy_threshold: 3,
            degraded_threshold_ms: 2000,
        }
    }
}

/// RSSHub 服务
pub struct RsshubService {
    db: DatabaseConnection,
    parser: FeedParser,
    config: RsshubConfig,
}

impl RsshubService {
    /// 创建新的 RSSHub 服务
    pub fn new(db: DatabaseConnection) -> Self {
        Self {
            db,
            parser: FeedParser::new(),
            config: RsshubConfig::default(),
        }
    }

    /// 获取用户的所有实例（包括全局默认实例）
    pub async fn get_instances(&self, user_id: Option<i32>) -> Result<Vec<InstanceModel>, String> {
        let mut instances = rsshub_instances::Entity::find()
            .filter(rsshub_instances::Column::UserId.is_null()) // 全局实例
            .order_by_asc(rsshub_instances::Column::Priority)
            .all(&self.db)
            .await
            .map_err(|error| rsshub_store_failed("fetch RSSHub instances", error))?;

        // 如果有用户 ID，也获取用户自己的实例
        if let Some(uid) = user_id {
            let user_instances = rsshub_instances::Entity::find()
                .filter(rsshub_instances::Column::UserId.eq(uid))
                .order_by_asc(rsshub_instances::Column::Priority)
                .all(&self.db)
                .await
                .map_err(|error| rsshub_store_failed("fetch RSSHub instances", error))?;

            instances.extend(user_instances);
        }

        // 按评分排序
        instances.sort_by(|a, b| {
            b.calculate_score()
                .partial_cmp(&a.calculate_score())
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok(instances)
    }

    /// 确保默认全局实例存在
    /// 如果数据库中没有全局实例，则创建预置的默认实例
    pub async fn ensure_default_instances(&self) -> Result<(), String> {
        // 检查是否已有全局实例
        let existing = rsshub_instances::Entity::find()
            .filter(rsshub_instances::Column::UserId.is_null())
            .all(&self.db)
            .await
            .map_err(|error| rsshub_store_failed("check RSSHub instances", error))?;

        if !existing.is_empty() {
            tracing::debug!("[RSSHub] Default instances already exist, skipping initialization");
            return Ok(());
        }

        tracing::info!("[RSSHub] Initializing default global instances...");

        // 预置的全局实例列表
        let default_instances = [
            ("RSSHub 官方", "https://rsshub.app", 0),
            ("RSSForever", "https://rsshub.rssforever.com", 10),
        ];

        let now = Utc::now();

        for (name, url, priority) in default_instances {
            let new_instance = rsshub_instances::ActiveModel {
                user_id: Set(None),
                name: Set(name.to_string()),
                url: Set(url.to_string()),
                access_key: Set(None),
                priority: Set(priority),
                enabled: Set(true),
                health_status: Set(HealthStatus::Unknown),
                consecutive_failures: Set(0),
                total_requests: Set(0),
                success_requests: Set(0),
                created_at: Set(now.into()),
                updated_at: Set(now.into()),
                ..Default::default()
            };

            match new_instance.insert(&self.db).await {
                Ok(_) => tracing::info!("[RSSHub] Created default instance: {}", name),
                Err(e) if unique_violation(&e) => {
                    tracing::debug!("[RSSHub] Default instance already present: {}", name);
                }
                Err(e) => tracing::warn!("[RSSHub] Failed to create instance {}: {}", name, e),
            }
        }

        Ok(())
    }

    /// 获取启用的健康实例
    pub async fn get_healthy_instances(
        &self,
        user_id: Option<i32>,
    ) -> Result<Vec<InstanceModel>, String> {
        let instances = self.get_instances(user_id).await?;
        Ok(instances
            .into_iter()
            .filter(|i| i.enabled && i.health_status != HealthStatus::Unhealthy)
            .collect())
    }

    /// 构建完整的 RSSHub URL
    pub fn build_url(&self, instance: &InstanceModel, route: &str) -> String {
        let base_url = instance.url.trim_end_matches('/');
        let route = if route.starts_with('/') {
            route.to_string()
        } else {
            format!("/{}", route)
        };

        let mut url = format!("{}{}", base_url, route);

        // 如果有 access_key，添加到查询参数
        if let Some(ref key) = instance.access_key {
            if url.contains('?') {
                url.push_str(&format!("&key={}", key));
            } else {
                url.push_str(&format!("?key={}", key));
            }
        }

        url
    }

    /// 从完整 URL 解析出路由（**path + query**，不含 fragment / host）。
    ///
    /// RSSHub 路由常带 `?limit=` / `?mode=` 等查询参数；只保留 path 会在实例
    /// 切换刷新时丢掉这些参数。
    pub fn extract_route(&self, url: &str) -> Option<String> {
        extract_rsshub_route_path_and_query(url)
    }

    /// 抓取 RSSHub 订阅（带故障转移）
    pub async fn fetch_with_failover(
        &self,
        route: &str,
        user_id: Option<i32>,
    ) -> Result<ParsedFeed, String> {
        let instances = self.get_healthy_instances(user_id).await?;

        if instances.is_empty() {
            return Err("No healthy RSSHub instances available".to_string());
        }

        let mut last_error = String::new();

        for instance in instances {
            let url = self.build_url(&instance, route);
            tracing::debug!(
                "[RSSHub] Trying instance {} for route {}",
                instance.name,
                route
            );

            let start = Instant::now();
            let result = self.parser.fetch_and_parse(&url).await;
            let elapsed = start.elapsed();

            match result {
                Ok(feed) => {
                    // 成功，更新统计
                    self.record_success(&instance, elapsed.as_millis() as i32)
                        .await;
                    tracing::info!(
                        "[RSSHub] Successfully fetched from {} ({}ms)",
                        instance.name,
                        elapsed.as_millis()
                    );
                    return Ok(feed);
                }
                Err(e) => {
                    // 失败，记录并尝试下一个
                    self.record_failure(&instance).await;
                    last_error = format!("{}: {}", instance.name, e);
                    tracing::warn!("[RSSHub] Failed to fetch from {}: {}", instance.name, e);
                }
            }
        }

        Err(format!(
            "All RSSHub instances failed. Last error: {}",
            last_error
        ))
    }

    /// 记录成功请求
    async fn record_success(&self, instance: &InstanceModel, response_time_ms: i32) {
        let health = if response_time_ms > self.config.degraded_threshold_ms {
            "degraded"
        } else {
            "healthy"
        };
        if let Err(e) = self
            .db
            .execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "UPDATE rsshub_instances SET \
                    last_health_check = NOW(), \
                    last_response_time_ms = $1, \
                    consecutive_failures = 0, \
                    total_requests = total_requests + 1, \
                    success_requests = success_requests + 1, \
                    health_status = $2, \
                    updated_at = NOW() \
                 WHERE id = $3",
                [
                    SeaValue::Int(Some(response_time_ms)),
                    SeaValue::String(Some(health.to_string())),
                    SeaValue::Int(Some(instance.id)),
                ],
            ))
            .await
        {
            tracing::error!("[RSSHub] Failed to update instance stats: {}", e);
        }
    }

    /// 记录失败请求
    async fn record_failure(&self, instance: &InstanceModel) {
        if let Err(e) = self
            .db
            .execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "UPDATE rsshub_instances SET \
                    last_health_check = NOW(), \
                    consecutive_failures = consecutive_failures + 1, \
                    total_requests = total_requests + 1, \
                    health_status = CASE \
                        WHEN consecutive_failures + 1 >= $1 THEN 'unhealthy' \
                        ELSE 'degraded' \
                    END, \
                    updated_at = NOW() \
                 WHERE id = $2",
                [
                    SeaValue::Int(Some(self.config.unhealthy_threshold)),
                    SeaValue::Int(Some(instance.id)),
                ],
            ))
            .await
        {
            tracing::error!("[RSSHub] Failed to update instance stats: {}", e);
        }
    }

    /// Live health check and persist success/failure stats.
    pub async fn health_check_and_record(&self, instance: &InstanceModel) -> Result<i32, String> {
        match self.health_check(instance).await {
            Ok(ms) => {
                self.record_success(instance, ms).await;
                Ok(ms)
            }
            Err(e) => {
                self.record_failure(instance).await;
                Err(e)
            }
        }
    }

    /// 执行健康检查（出站经 outbound_security，防 SSRF）
    pub async fn health_check(&self, instance: &InstanceModel) -> Result<i32, String> {
        // 使用一个简单的路由进行健康检查
        let test_route = "/";
        let url = format!("{}{}", instance.url.trim_end_matches('/'), test_route);

        let (target_url, client) = crate::services::outbound_security::build_public_http_client(
            &url,
            Duration::from_secs(self.config.request_timeout_secs),
            Some("Myriad Phantasi Reader/1.0 (RSSHub Health Check)"),
        )
        .await
        .map_err(|error| {
            tracing::warn!(%error, "unsafe RSSHub URL blocked");
            "Unsafe RSSHub URL blocked".to_string()
        })?;

        let start = Instant::now();
        let response = client.get(target_url).send().await.map_err(|error| {
            tracing::warn!(%error, "RSSHub health check request failed");
            "Request failed".to_string()
        })?;

        let elapsed = start.elapsed().as_millis() as i32;

        if response.status().is_success() {
            Ok(elapsed)
        } else {
            Err(format!("HTTP {}", response.status()))
        }
    }

    /// 对已启用实例执行健康检查
    pub async fn check_all_instances(&self, user_id: Option<i32>) -> Result<(), String> {
        let instances = self.get_instances(user_id).await?;

        for instance in instances {
            if !instance.enabled {
                continue;
            }

            match self.health_check(&instance).await {
                Ok(response_time) => {
                    self.record_success(&instance, response_time).await;
                    tracing::info!(
                        "[RSSHub] Health check passed for {} ({}ms)",
                        instance.name,
                        response_time
                    );
                }
                Err(e) => {
                    self.record_failure(&instance).await;
                    tracing::warn!("[RSSHub] Health check failed for {}: {}", instance.name, e);
                }
            }

            // 避免过快检查
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        Ok(())
    }

    /// 校验实例 URL 必须为公网 HTTP(S)（防 SSRF）
    async fn validate_instance_url(url: &str) -> Result<String, String> {
        let trimmed = url.trim().trim_end_matches('/').to_string();
        FeedParser::validate_public_url(&trimmed)
            .await
            .map_err(|e| e.to_string())?;
        Ok(trimmed)
    }

    /// 添加新实例
    pub async fn add_instance(
        &self,
        user_id: Option<i32>,
        name: String,
        url: String,
        access_key: Option<String>,
        priority: Option<i32>,
        // 创建全局实例需要管理员
        is_admin: bool,
    ) -> Result<InstanceModel, String> {
        if user_id.is_none() && !is_admin {
            return Err("Only admins can create global RSSHub instances".to_string());
        }

        let safe_url = Self::validate_instance_url(&url).await?;
        let now = Utc::now();

        let new_instance = rsshub_instances::ActiveModel {
            user_id: Set(user_id),
            name: Set(name),
            url: Set(safe_url),
            access_key: Set(access_key),
            priority: Set(priority.unwrap_or(100)),
            enabled: Set(true),
            health_status: Set(HealthStatus::Unknown),
            consecutive_failures: Set(0),
            total_requests: Set(0),
            success_requests: Set(0),
            created_at: Set(now.into()),
            updated_at: Set(now.into()),
            ..Default::default()
        };

        new_instance
            .insert(&self.db)
            .await
            .map_err(|error| rsshub_store_failed("create RSSHub instance", error))
    }

    /// 更新实例
    #[allow(clippy::too_many_arguments)]
    pub async fn update_instance(
        &self,
        id: i32,
        user_id: Option<i32>,
        name: Option<String>,
        url: Option<String>,
        access_key: Option<String>,
        priority: Option<i32>,
        enabled: Option<bool>,
        is_admin: bool,
    ) -> Result<InstanceModel, String> {
        let instance = rsshub_instances::Entity::find_by_id(id)
            .one(&self.db)
            .await
            .map_err(|error| rsshub_store_failed("find RSSHub instance", error))?
            .ok_or_else(|| "Instance not found".to_string())?;

        // 全局实例仅管理员可改；用户实例仅本人可改
        if instance.user_id.is_none() {
            if !is_admin {
                return Err("Only admins can modify global RSSHub instances".to_string());
            }
        } else if instance.user_id != user_id {
            return Err("Permission denied".to_string());
        }

        let now = Utc::now();
        let mut active: rsshub_instances::ActiveModel = instance.into();

        if let Some(n) = name {
            active.name = Set(n);
        }
        if let Some(u) = url {
            let safe_url = Self::validate_instance_url(&u).await?;
            active.url = Set(safe_url);
        }
        if let Some(k) = access_key {
            active.access_key = Set(Some(k));
        }
        if let Some(p) = priority {
            active.priority = Set(p);
        }
        if let Some(e) = enabled {
            active.enabled = Set(e);
        }
        active.updated_at = Set(now.into());

        active
            .update(&self.db)
            .await
            .map_err(|error| rsshub_store_failed("update RSSHub instance", error))
    }

    /// 删除实例
    pub async fn delete_instance(
        &self,
        id: i32,
        user_id: Option<i32>,
        is_admin: bool,
    ) -> Result<(), String> {
        let instance = rsshub_instances::Entity::find_by_id(id)
            .one(&self.db)
            .await
            .map_err(|error| rsshub_store_failed("find RSSHub instance", error))?
            .ok_or_else(|| "Instance not found".to_string())?;

        if instance.user_id.is_none() {
            if !is_admin {
                return Err("Only admins can delete global RSSHub instances".to_string());
            }
            // 不允许删除全局默认实例
            if instance.priority == 0 {
                return Err("Cannot delete the default global instance".to_string());
            }
        } else if instance.user_id != user_id {
            return Err("Permission denied".to_string());
        }

        rsshub_instances::Entity::delete_by_id(id)
            .exec(&self.db)
            .await
            .map_err(|error| rsshub_store_failed("delete RSSHub instance", error))?;

        Ok(())
    }

    /// 重置实例统计（全局需管理员；用户实例需本人）
    pub async fn reset_instance_stats(
        &self,
        id: i32,
        user_id: Option<i32>,
        is_admin: bool,
    ) -> Result<(), String> {
        let instance = rsshub_instances::Entity::find_by_id(id)
            .one(&self.db)
            .await
            .map_err(|error| rsshub_store_failed("find RSSHub instance", error))?
            .ok_or_else(|| "Instance not found".to_string())?;

        if instance.user_id.is_none() {
            if !is_admin {
                return Err("Only admins can reset global RSSHub instances".to_string());
            }
        } else if instance.user_id != user_id {
            return Err("Permission denied".to_string());
        }

        let now = Utc::now();
        let mut active: rsshub_instances::ActiveModel = instance.into();

        active.health_status = Set(HealthStatus::Unknown);
        active.last_health_check = Set(None);
        active.last_response_time_ms = Set(None);
        active.consecutive_failures = Set(0);
        active.total_requests = Set(0);
        active.success_requests = Set(0);
        active.updated_at = Set(now.into());

        active
            .update(&self.db)
            .await
            .map_err(|error| rsshub_store_failed("reset RSSHub instance", error))?;

        Ok(())
    }
}

#[cfg(test)]
mod extract_route_tests {
    use super::extract_rsshub_route_path_and_query;

    #[test]
    fn preserves_query_params_on_refresh_route() {
        let route = extract_rsshub_route_path_and_query(
            "https://rsshub.app/bilibili/user/video/1?limit=20&mode=full",
        )
        .expect("route");
        assert_eq!(route, "/bilibili/user/video/1?limit=20&mode=full");
    }

    #[test]
    fn path_only_when_no_query() {
        let route =
            extract_rsshub_route_path_and_query("https://rsshub.rssforever.com/github/issue/x/y")
                .expect("route");
        assert_eq!(route, "/github/issue/x/y");
    }

    #[test]
    fn drops_fragment_keeps_query() {
        let route = extract_rsshub_route_path_and_query(
            "https://hub.slarker.me/twitter/user/a?limit=5#frag",
        )
        .expect("route");
        assert_eq!(route, "/twitter/user/a?limit=5");
    }
}

#[cfg(test)]
mod stats_and_unique_tests {
    #[test]
    fn counters_increment_in_place() {
        let src = include_str!("rsshub_service.rs");
        let success = src
            .split("async fn record_success")
            .nth(1)
            .and_then(|rest| rest.split("async fn record_failure").next())
            .expect("record_success");
        assert!(success.contains("total_requests = total_requests + 1"));
        assert!(success.contains("success_requests = success_requests + 1"));
        assert!(!success.contains("instance.total_requests + 1"));
        let failure = src
            .split("async fn record_failure")
            .nth(1)
            .and_then(|rest| rest.split("pub async fn health_check_and_record").next())
            .expect("record_failure");
        assert!(failure.contains("total_requests = total_requests + 1"));
        assert!(failure.contains("consecutive_failures = consecutive_failures + 1"));
        assert!(!failure.contains("instance.total_requests + 1"));
    }

    #[test]
    fn default_instance_insert_treats_unique_as_already_present() {
        let src = include_str!("rsshub_service.rs");
        let ensure = src
            .split("pub async fn ensure_default_instances")
            .nth(1)
            .and_then(|rest| rest.split("pub async fn get_healthy_instances").next())
            .expect("ensure_default_instances");
        assert!(ensure.contains("unique_violation"));
    }

    #[tokio::test]
    async fn concurrent_success_records_do_not_drop_counts() {
        use super::RsshubService;
        use crate::models::entities::rsshub_instances::{self, HealthStatus};
        use chrono::Utc;
        use sea_orm::{
            ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectOptions, ConnectionTrait,
            Database, DatabaseBackend, EntityTrait, QueryFilter, Schema,
        };

        let Ok(url) = std::env::var("PHANTASI_TEST_DATABASE_URL") else {
            return;
        };
        let admin = Database::connect(&url).await.unwrap();
        let scope = format!("rsshub_stats_{}", uuid::Uuid::new_v4().simple());
        admin
            .execute_unprepared(&format!("CREATE SCHEMA {scope}"))
            .await
            .unwrap();
        let connect = || {
            let mut options = ConnectOptions::new(url.clone());
            options
                .max_connections(1)
                .min_connections(1)
                .sqlx_logging(false)
                .set_schema_search_path(scope.clone());
            Database::connect(options)
        };
        let db = connect().await.unwrap();
        let schema = Schema::new(DatabaseBackend::Postgres);
        let sql = schema
            .create_table_from_entity(rsshub_instances::Entity)
            .to_string(sea_orm::sea_query::PostgresQueryBuilder);
        db.execute_unprepared(&sql).await.unwrap();
        db.execute_unprepared(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_rsshub_instances_global_url \
             ON rsshub_instances (url) WHERE user_id IS NULL",
        )
        .await
        .unwrap();
        let now = Utc::now();
        let instance = rsshub_instances::ActiveModel {
            user_id: Set(Some(1)),
            name: Set("test".into()),
            url: Set("https://rsshub.example".into()),
            priority: Set(0),
            enabled: Set(true),
            health_status: Set(HealthStatus::Unknown),
            consecutive_failures: Set(0),
            total_requests: Set(0),
            success_requests: Set(0),
            created_at: Set(now.into()),
            updated_at: Set(now.into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let a = RsshubService::new(connect().await.unwrap());
        let b = RsshubService::new(connect().await.unwrap());
        tokio::join!(
            a.record_success(&instance, 10),
            b.record_success(&instance, 20)
        );
        let saved = rsshub_instances::Entity::find_by_id(instance.id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(saved.total_requests, 2);
        assert_eq!(saved.success_requests, 2);

        let seed_a = RsshubService::new(connect().await.unwrap());
        let seed_b = RsshubService::new(connect().await.unwrap());
        let _ = tokio::join!(
            seed_a.ensure_default_instances(),
            seed_b.ensure_default_instances()
        );
        let globals = rsshub_instances::Entity::find()
            .filter(rsshub_instances::Column::UserId.is_null())
            .filter(rsshub_instances::Column::Url.eq("https://rsshub.app"))
            .all(&db)
            .await
            .unwrap();
        admin
            .execute_unprepared(&format!("DROP SCHEMA {scope} CASCADE"))
            .await
            .ok();
        assert_eq!(globals.len(), 1);
    }
}
