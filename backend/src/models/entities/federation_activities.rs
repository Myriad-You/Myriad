//! Activity 活动日志实体
//!
//! 记录本地发出和远程收到的 ActivityPub/MFP 活动

#![allow(dead_code)]

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "federation_activities")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    #[sea_orm(column_type = "Text")]
    pub activity_id: String,
    /// 本地用户 ID（发出时非 NULL）
    pub user_id: Option<i32>,
    /// 远程 Actor ID（收到时非 NULL）
    pub remote_actor_id: Option<i32>,
    /// Create, Announce, Follow, Accept, Like, myriad:ChannelOpen...
    pub activity_type: String,
    pub object_type: Option<String>,
    #[sea_orm(column_type = "Json")]
    pub object_json: Json,
    pub is_local: bool,
    pub published_at: DateTimeWithTimeZone,
    pub received_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
