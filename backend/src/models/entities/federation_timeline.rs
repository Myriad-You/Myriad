//! 联邦时间线实体
//!
//! 聚合来自关注对象的远程内容

#![allow(dead_code)]

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "federation_timeline")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub user_id: i32,
    #[sea_orm(column_type = "Text")]
    pub activity_id: String,
    pub remote_actor_id: Option<i32>,
    pub activity_type: Option<String>,
    pub object_type: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    pub content_preview: Option<String>,
    #[sea_orm(column_type = "Json", nullable)]
    pub content_json: Option<Json>,
    pub is_read: bool,
    pub received_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
