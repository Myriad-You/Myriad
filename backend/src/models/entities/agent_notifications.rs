//! Agent 通知实体
//!
//! 持久化通知中心的历史记录，匹配 migration 004 的 agent_notifications 表

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "agent_notifications")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    /// 通知类型（task_completed / heartbeat_result / ...，与 NotificationType 的 snake_case 一致）
    pub notification_type: String,
    /// 优先级（low / normal / high / urgent）
    pub priority: String,
    #[sea_orm(column_type = "Text")]
    pub title: String,
    #[sea_orm(column_type = "Text")]
    pub body: String,
    /// 目标用户 ID（可空；`notify` 拒绝 `user_id=None`）
    #[sea_orm(nullable)]
    pub user_id: Option<i32>,
    #[sea_orm(column_type = "Json", nullable)]
    pub metadata: Option<Json>,
    pub read: bool,
    pub created_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
