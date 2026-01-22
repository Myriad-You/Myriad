//! Tapp 实体定义

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// Tapp 状态
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "String(Some(20))")]
pub enum TappStatus {
    #[sea_orm(string_value = "installed")]
    Installed,
    #[sea_orm(string_value = "running")]
    Running,
    #[sea_orm(string_value = "suspended")]
    Suspended,
    #[sea_orm(string_value = "error")]
    Error,
}

impl Default for TappStatus {
    fn default() -> Self {
        Self::Installed
    }
}

/// Tapp 应用实体
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "tapps")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,

    /// Tapp 唯一标识符 (如 com.example.my-app)
    #[sea_orm(column_type = "String(Some(255))")]
    pub tapp_id: String,

    /// 所属用户 ID
    pub user_id: i32,

    /// 显示名称
    #[sea_orm(column_type = "String(Some(255))")]
    pub name: String,

    /// 版本号
    #[sea_orm(column_type = "String(Some(50))")]
    pub version: String,

    /// 描述
    #[sea_orm(column_type = "Text", nullable)]
    pub description: Option<String>,

    /// 作者信息 (JSON)
    #[sea_orm(column_type = "Json", nullable)]
    pub author: Option<serde_json::Value>,

    /// 图标 URL
    #[sea_orm(column_type = "Text", nullable)]
    pub icon: Option<String>,

    /// 主题色
    #[sea_orm(column_type = "String(Some(20))", nullable)]
    pub theme_color: Option<String>,

    /// 完整清单 (JSON)
    #[sea_orm(column_type = "Json")]
    pub manifest: serde_json::Value,

    /// 当前状态
    pub status: TappStatus,

    /// 已授权的权限 (JSON 数组)
    #[sea_orm(column_type = "Json")]
    pub granted_permissions: serde_json::Value,

    /// .tapp 文件路径
    #[sea_orm(column_type = "Text")]
    pub file_path: String,

    /// 主代码文件路径
    #[sea_orm(column_type = "Text")]
    pub code_path: String,

    /// 安装时间
    pub installed_at: DateTimeWithTimeZone,

    /// 最后运行时间
    #[sea_orm(nullable)]
    pub last_run_at: Option<DateTimeWithTimeZone>,

    /// 更新时间
    pub updated_at: DateTimeWithTimeZone,

    /// 错误信息
    #[sea_orm(column_type = "Text", nullable)]
    pub error_message: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::tapp_widgets::Entity")]
    TappWidgets,
    #[sea_orm(has_many = "super::tapp_storage::Entity")]
    TappStorage,
}

impl Related<super::tapp_widgets::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::TappWidgets.def()
    }
}

impl Related<super::tapp_storage::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::TappStorage.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
