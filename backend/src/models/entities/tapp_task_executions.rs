//! Tapp 任务执行历史实体定义

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// 执行状态
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::N(20))")]
#[derive(Default)]
pub enum ExecutionStatus {
    #[sea_orm(string_value = "pending")]
    #[default]
    Pending,
    #[sea_orm(string_value = "running")]
    Running,
    #[sea_orm(string_value = "success")]
    Success,
    /// 本次未完整执行（例如 frontend 阶段没有在线受众）。
    #[sea_orm(string_value = "skipped")]
    Skipped,
    #[sea_orm(string_value = "failed")]
    Failed,
    #[sea_orm(string_value = "timeout")]
    Timeout,
}

/// 任务执行历史实体
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "tapp_task_executions")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,

    /// 关联的定时任务表 ID
    pub scheduled_task_id: i32,

    /// 用户 ID
    pub user_id: i32,

    #[sea_orm(column_type = "String(StringLen::N(255))")]
    pub tapp_id: String,

    /// 任务 ID
    #[sea_orm(column_type = "String(StringLen::N(255))")]
    pub task_id: String,

    /// 计划执行时间
    pub scheduled_at: DateTimeWithTimeZone,

    /// 实际执行时间
    pub executed_at: DateTimeWithTimeZone,

    /// 完成时间
    #[sea_orm(nullable)]
    pub completed_at: Option<DateTimeWithTimeZone>,

    /// 执行目标
    #[sea_orm(column_type = "String(StringLen::N(20))")]
    pub execution_target: String,

    /// 执行状态
    pub status: ExecutionStatus,

    /// 是否为补偿执行
    pub is_compensation: bool,

    /// 执行结果 (JSON)
    #[sea_orm(column_type = "Json", nullable)]
    pub result: Option<serde_json::Value>,

    /// 错误信息
    #[sea_orm(column_type = "Text", nullable)]
    pub error: Option<String>,

    /// 执行时长（毫秒）
    #[sea_orm(nullable)]
    pub duration_ms: Option<i32>,

    /// 重试次数
    pub retry_count: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::tapp_scheduled_tasks::Entity",
        from = "Column::ScheduledTaskId",
        to = "super::tapp_scheduled_tasks::Column::Id"
    )]
    ScheduledTask,
}

impl Related<super::tapp_scheduled_tasks::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ScheduledTask.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
