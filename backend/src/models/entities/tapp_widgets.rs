//! Tapp 小组件实体定义

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// Tapp 注册的小组件
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "tapp_widgets")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,

    /// 小组件完整 ID (tapp.{tapp_id}.{widget_id})
    #[sea_orm(column_type = "String(Some(255))")]
    pub widget_id: String,

    /// 所属 Tapp ID
    #[sea_orm(column_type = "String(Some(255))")]
    pub tapp_id: String,

    /// 所属用户 ID
    pub user_id: i32,

    /// 显示名称
    #[sea_orm(column_type = "String(Some(255))")]
    pub name: String,

    /// 描述
    #[sea_orm(column_type = "Text", nullable)]
    pub description: Option<String>,

    /// 图标
    #[sea_orm(column_type = "Text", nullable)]
    pub icon: Option<String>,

    /// 默认尺寸
    #[sea_orm(column_type = "String(Some(10))")]
    pub default_size: String,

    /// 支持的尺寸 (JSON 数组)
    #[sea_orm(column_type = "Json")]
    pub sizes: serde_json::Value,

    /// 分类
    #[sea_orm(column_type = "String(Some(50))", nullable)]
    pub category: Option<String>,

    /// 完整配置 (JSON)
    #[sea_orm(column_type = "Json")]
    pub config: serde_json::Value,

    /// 注册时间
    pub registered_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::tapps::Entity",
        from = "Column::TappId",
        to = "super::tapps::Column::TappId"
    )]
    Tapp,
}

impl Related<super::tapps::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Tapp.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
