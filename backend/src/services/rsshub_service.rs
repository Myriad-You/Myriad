//! RSSHub 服务
//!
//! 提供 RSSHub 实例管理、健康检查、自动故障转移功能

use chrono::Utc;
use reqwest::Url;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter,
    QueryOrder,
};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

use crate::models::entities::rsshub_instances::{self, HealthStatus, Model as InstanceModel};
use crate::services::brew_parser::{FeedParser, ParsedFeed};

/// RSSHub 服务配置
#[derive(Clone, Debug)]
pub struct RsshubConfig {
    /// 健康检查间隔（秒）— 预留；当前由调用方/调度器控制周期
    #[allow(dead_code)]
    pub health_check_interval_secs: u64,
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
            health_check_interval_secs: 300, // 5 分钟
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
    /// 缓存的可用实例列表（预留热缓存）
    #[allow(dead_code)]
    cached_instances: Arc<RwLock<Vec<InstanceModel>>>,
}

impl RsshubService {
    /// 创建新的 RSSHub 服务
    pub fn new(db: DatabaseConnection) -> Self {
        Self {
            db,
            parser: FeedParser::new(),
            config: RsshubConfig::default(),
            cached_instances: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// 获取用户的所有实例（包括全局默认实例）
    pub async fn get_instances(&self, user_id: Option<i32>) -> Result<Vec<InstanceModel>, String> {
        let mut instances = rsshub_instances::Entity::find()
            .filter(rsshub_instances::Column::UserId.is_null()) // 全局实例
            .order_by_asc(rsshub_instances::Column::Priority)
            .all(&self.db)
            .await
            .map_err(|e| format!("Failed to fetch global instances: {}", e))?;

        // 如果有用户 ID，也获取用户自己的实例
        if let Some(uid) = user_id {
            let user_instances = rsshub_instances::Entity::find()
                .filter(rsshub_instances::Column::UserId.eq(uid))
                .order_by_asc(rsshub_instances::Column::Priority)
                .all(&self.db)
                .await
                .map_err(|e| format!("Failed to fetch user instances: {}", e))?;

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
            .map_err(|e| format!("Failed to check existing instances: {}", e))?;

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

    /// 从完整 URL 解析出路由
    pub fn extract_route(&self, url: &str) -> Option<String> {
        // 尝试匹配常见的 RSSHub 实例域名
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
                    let path = parsed.path();
                    if !path.is_empty() && path != "/" {
                        return Some(path.to_string());
                    }
                }
            }
        }

        // 如果 URL 中包含 rsshub，尝试解析
        if url.to_lowercase().contains("rsshub") {
            if let Ok(parsed) = Url::parse(url) {
                let path = parsed.path();
                if !path.is_empty() && path != "/" {
                    return Some(path.to_string());
                }
            }
        }

        None
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
        let now = Utc::now();
        let mut active: rsshub_instances::ActiveModel = instance.clone().into();

        active.last_health_check = Set(Some(now.into()));
        active.last_response_time_ms = Set(Some(response_time_ms));
        active.consecutive_failures = Set(0);
        active.total_requests = Set(instance.total_requests + 1);
        active.success_requests = Set(instance.success_requests + 1);
        active.updated_at = Set(now.into());

        // 更新健康状态
        if response_time_ms > self.config.degraded_threshold_ms {
            active.health_status = Set(HealthStatus::Degraded);
        } else {
            active.health_status = Set(HealthStatus::Healthy);
        }

        if let Err(e) = active.update(&self.db).await {
            tracing::error!("[RSSHub] Failed to update instance stats: {}", e);
        }
    }

    /// 记录失败请求
    async fn record_failure(&self, instance: &InstanceModel) {
        let now = Utc::now();
        let new_failures = instance.consecutive_failures + 1;
        let mut active: rsshub_instances::ActiveModel = instance.clone().into();

        active.last_health_check = Set(Some(now.into()));
        active.consecutive_failures = Set(new_failures);
        active.total_requests = Set(instance.total_requests + 1);
        active.updated_at = Set(now.into());

        // 更新健康状态
        if new_failures >= self.config.unhealthy_threshold {
            active.health_status = Set(HealthStatus::Unhealthy);
        } else {
            active.health_status = Set(HealthStatus::Degraded);
        }

        if let Err(e) = active.update(&self.db).await {
            tracing::error!("[RSSHub] Failed to update instance stats: {}", e);
        }
    }

    /// Live health check and persist success/failure stats (same path as brew admin checks).
    pub async fn health_check_and_record(
        &self,
        instance: &InstanceModel,
    ) -> Result<i32, String> {
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
            Some("Myriad Brew Reader/1.0 (RSSHub Health Check)"),
        )
        .await
        .map_err(|e| format!("Unsafe RSSHub URL blocked: {e}"))?;

        let start = Instant::now();
        let response = client
            .get(target_url)
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;

        let elapsed = start.elapsed().as_millis() as i32;

        if response.status().is_success() {
            Ok(elapsed)
        } else {
            Err(format!("HTTP {}", response.status()))
        }
    }

    /// 对所有实例执行健康检查
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
            .map_err(|e| format!("Failed to insert instance: {}", e))
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
            .map_err(|e| format!("Failed to find instance: {}", e))?
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
            .map_err(|e| format!("Failed to update instance: {}", e))
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
            .map_err(|e| format!("Failed to find instance: {}", e))?
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
            .map_err(|e| format!("Failed to delete instance: {}", e))?;

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
            .map_err(|e| format!("Failed to find instance: {}", e))?
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
            .map_err(|e| format!("Failed to reset instance: {}", e))?;

        Ok(())
    }
}
