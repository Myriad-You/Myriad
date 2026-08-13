//! 联邦投递队列实体
//!
//! 持久化的 Activity 投递队列，支持指数退避重试

#![allow(dead_code)]

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "federation_delivery_queue")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub activity_id: i32,
    #[sea_orm(column_type = "Text")]
    pub target_inbox: String,
    #[sea_orm(column_type = "Text")]
    pub target_domain: String,
    /// pending, delivering, delivered, failed, dead
    pub status: String,
    pub attempts: i32,
    pub max_attempts: i32,
    pub last_attempt_at: Option<DateTimeWithTimeZone>,
    pub lease_token: Option<Uuid>,
    pub lease_expires_at: Option<DateTimeWithTimeZone>,
    pub next_retry_at: Option<DateTimeWithTimeZone>,
    #[sea_orm(column_type = "Text", nullable)]
    pub error_message: Option<String>,
    pub created_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::federation_activities::Entity",
        from = "Column::ActivityId",
        to = "super::federation_activities::Column::Id"
    )]
    Activity,
}

impl Related<super::federation_activities::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Activity.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
