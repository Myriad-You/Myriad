//! RSSHub 实例实体
//!
//! 存储用户的 RSSHub 实例配置，支持健康检查和自动故障转移

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "rsshub_instances")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// 所属用户 ID（NULL 表示全局默认实例）
    pub user_id: Option<i32>,
    /// 实例名称
    pub name: String,
    /// 实例 URL（如 https://rsshub.app）
    #[sea_orm(column_type = "Text")]
    pub url: String,
    /// 访问密钥（可选）
    pub access_key: Option<String>,
    /// 优先级（数字越小优先级越高）
    pub priority: i32,
    pub enabled: bool,
    /// 健康状态: healthy, degraded, unhealthy, unknown
    pub health_status: HealthStatus,
    /// 最后健康检查时间
    pub last_health_check: Option<DateTimeWithTimeZone>,
    /// 最后响应时间（毫秒）
    pub last_response_time_ms: Option<i32>,
    /// 连续失败次数
    pub consecutive_failures: i32,
    /// 总请求次数
    pub total_requests: i32,
    /// 成功请求次数
    pub success_requests: i32,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}

/// 实例健康状态
#[derive(Clone, Debug, PartialEq, Eq, EnumIter, DeriveActiveEnum, Serialize, Deserialize)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::N(20))")]
#[derive(Default)]
pub enum HealthStatus {
    /// 健康 - 响应正常
    #[sea_orm(string_value = "healthy")]
    Healthy,
    /// 降级 - 响应慢或部分失败
    #[sea_orm(string_value = "degraded")]
    Degraded,
    /// 不健康 - 连续失败
    #[sea_orm(string_value = "unhealthy")]
    Unhealthy,
    /// 未知 - 尚未检查
    #[sea_orm(string_value = "unknown")]
    #[default]
    Unknown,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

/// RSSHub 实例响应
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InstanceResponse {
    pub id: i32,
    pub user_id: Option<i32>,
    pub name: String,
    pub url: String,
    pub has_access_key: bool,
    pub priority: i32,
    pub enabled: bool,
    pub health_status: String,
    pub last_health_check: Option<i64>,
    pub last_response_time_ms: Option<i32>,
    pub consecutive_failures: i32,
    pub success_rate: f64,
    pub created_at: i64,
}

impl From<Model> for InstanceResponse {
    fn from(m: Model) -> Self {
        let success_rate = if m.total_requests > 0 {
            (m.success_requests as f64 / m.total_requests as f64) * 100.0
        } else {
            0.0
        };

        Self {
            id: m.id,
            user_id: m.user_id,
            name: m.name,
            url: m.url,
            has_access_key: m.access_key.is_some(),
            priority: m.priority,
            enabled: m.enabled,
            health_status: match m.health_status {
                HealthStatus::Healthy => "healthy".to_string(),
                HealthStatus::Degraded => "degraded".to_string(),
                HealthStatus::Unhealthy => "unhealthy".to_string(),
                HealthStatus::Unknown => "unknown".to_string(),
            },
            last_health_check: m.last_health_check.map(|t| t.timestamp_millis()),
            last_response_time_ms: m.last_response_time_ms,
            consecutive_failures: m.consecutive_failures,
            success_rate,
            created_at: m.created_at.timestamp_millis(),
        }
    }
}

impl Model {
    /// 计算实例的综合评分
    /// 考虑因素：优先级、健康状态、成功率、响应时间
    pub fn calculate_score(&self) -> f64 {
        let mut score = 100.0;

        // 优先级影响（priority 越小越好）
        score -= self.priority as f64 * 0.5;

        // 健康状态影响
        match self.health_status {
            HealthStatus::Healthy => score += 50.0,
            HealthStatus::Degraded => score += 20.0,
            HealthStatus::Unhealthy => score -= 100.0,
            HealthStatus::Unknown => score += 10.0, // 未知状态给予一定信任
        }

        // 成功率影响
        if self.total_requests > 0 {
            let success_rate = self.success_requests as f64 / self.total_requests as f64;
            score += success_rate * 30.0;
        }

        // 响应时间影响（越快越好）
        if let Some(response_time) = self.last_response_time_ms {
            if response_time < 500 {
                score += 20.0;
            } else if response_time < 1000 {
                score += 10.0;
            } else if response_time < 2000 {
                score += 5.0;
            } else {
                score -= 10.0;
            }
        }

        // 连续失败惩罚
        score -= self.consecutive_failures as f64 * 15.0;

        score
    }
}
