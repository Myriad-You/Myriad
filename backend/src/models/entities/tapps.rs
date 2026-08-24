//! Tapp 实体定义

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// Tapp 状态
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::N(20))")]
#[derive(Default)]
pub enum TappStatus {
    #[sea_orm(string_value = "installed")]
    #[default]
    Installed,
    #[sea_orm(string_value = "running")]
    Running,
    #[sea_orm(string_value = "suspended")]
    Suspended,
    #[sea_orm(string_value = "error")]
    Error,
}

/// Tapp 应用实体
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "tapps")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,

    /// Tapp 唯一标识符 (如 com.example.my-app)
    #[sea_orm(column_type = "String(StringLen::N(255))")]
    pub tapp_id: String,

    /// 所属用户 ID
    pub user_id: i32,

    /// 显示名称
    #[sea_orm(column_type = "String(StringLen::N(255))")]
    pub name: String,

    /// 版本号
    #[sea_orm(column_type = "String(StringLen::N(50))")]
    pub version: String,

    #[sea_orm(column_type = "Text", nullable)]
    pub description: Option<String>,

    /// 作者信息 (JSON)
    #[sea_orm(column_type = "Json", nullable)]
    pub author: Option<serde_json::Value>,

    /// 图标 URL
    #[sea_orm(column_type = "Text", nullable)]
    pub icon: Option<String>,

    /// 主题色
    #[sea_orm(column_type = "String(StringLen::N(20))", nullable)]
    pub theme_color: Option<String>,

    /// 完整清单 (JSON)
    #[sea_orm(column_type = "Json")]
    pub manifest: serde_json::Value,

    /// 当前状态
    pub status: TappStatus,

    /// 安装时的有效权限快照（兼容旧数据；运行时不以此字段作为最终授权事实）
    #[sea_orm(column_type = "Json")]
    pub granted_permissions: serde_json::Value,

    /// 安装时由用户批准的权限；运行时再与当前角色和管理员策略求交集
    #[sea_orm(column_type = "JsonBinary")]
    pub approved_permissions: serde_json::Value,

    /// 升级迁移清除旧权限后置 true：重新授权成功前，运行时不签发、不校验
    /// runtime grant，不启动；inbound `/tapi` 与调度器同样拒绝。仅显式的
    /// 安装/更新/重新授权路径可清回 false；普通读取、启动、schema heal 不得清。
    pub needs_reauthorization: bool,

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

    pub updated_at: DateTimeWithTimeZone,

    /// 错误信息
    #[sea_orm(column_type = "Text", nullable)]
    pub error_message: Option<String>,

    /// 公开安装的可见性：`all`（全体）| `admin`（仅管理员）
    /// 仅对站点主/管理员命名空间的安装生效；私有临时安装始终仅本人可见。
    #[sea_orm(column_type = "String(StringLen::N(20))")]
    pub visibility: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
