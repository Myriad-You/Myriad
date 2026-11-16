//! Agent 消息实体
//!
//! 存储会话消息历史，匹配 migration 004 的 agent_messages 表

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "agent_messages")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub session_id: String,
    #[sea_orm(nullable)]
    pub task_id: Option<String>,
    pub role: String,
    #[sea_orm(column_type = "Text")]
    pub content: String,
    #[sea_orm(column_type = "Json", nullable)]
    pub metadata: Option<Json>,
    pub created_at: DateTimeWithTimeZone,
    /// The end a copied line was said on (D6-e); none for one said here.
    #[sea_orm(nullable)]
    pub origin: Option<String>,
    /// The row of her log a copied line is, or the row the partner's line
    /// said here was written as; none for a line the log does not have.
    #[sea_orm(nullable)]
    pub log_row: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
