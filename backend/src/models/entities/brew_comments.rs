//! Brew 阅读 - 用户评论实体
//!
//! 存储用户对文章选中文本的评论（类似批注功能）
//! 支持嵌套回复（通过 parent_id 字段）

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "brew_comments")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// 关联文章 ID
    pub item_id: i32,
    /// 评论用户 ID
    pub user_id: i32,
    /// 选中的原文文本
    #[sea_orm(column_type = "Text")]
    pub selected_text: String,
    /// 评论内容
    #[sea_orm(column_type = "Text")]
    pub comment: String,
    /// 选中文本在原文中的起始位置（字符偏移）
    pub start_offset: Option<i32>,
    /// 选中文本在原文中的结束位置（字符偏移）
    pub end_offset: Option<i32>,
    /// 前文上下文（用于定位）
    #[sea_orm(column_type = "Text", nullable)]
    pub context_before: Option<String>,
    /// 后文上下文（用于定位）
    #[sea_orm(column_type = "Text", nullable)]
    pub context_after: Option<String>,
    /// 评论颜色标记
    #[sea_orm(column_type = "String(StringLen::N(20))", nullable)]
    pub color: Option<String>,
    /// 是否公开（预留）
    pub is_public: bool,
    /// 父评论 ID（用于嵌套回复，NULL 表示顶级评论）
    pub parent_id: Option<i32>,
    /// Article body version when this comment was created. Null on legacy rows.
    pub content_revision: Option<i64>,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::brew_items::Entity",
        from = "Column::ItemId",
        to = "super::brew_items::Column::Id",
        on_delete = "Cascade"
    )]
    BrewItem,
    // 自引用关系 - 父评论
    #[sea_orm(
        belongs_to = "Entity",
        from = "Column::ParentId",
        to = "Column::Id",
        on_delete = "Cascade"
    )]
    Parent,
}

impl Related<super::brew_items::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::BrewItem.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

/// 评论响应（API 返回格式）
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommentResponse {
    pub id: i32,
    pub item_id: i32,
    pub user_id: i32,
    /// 用户名
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_name: Option<String>,
    /// 用户显示名称
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_display_name: Option<String>,
    /// 用户头像
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_avatar: Option<String>,
    pub selected_text: String,
    pub comment: String,
    pub start_offset: Option<i32>,
    pub end_offset: Option<i32>,
    pub context_before: Option<String>,
    pub context_after: Option<String>,
    pub color: Option<String>,
    pub is_public: bool,
    pub parent_id: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_revision: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
    /// 回复列表（仅用于获取详情时填充）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replies: Option<Vec<CommentResponse>>,
    /// 回复数量
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_count: Option<i32>,
}

impl From<Model> for CommentResponse {
    fn from(m: Model) -> Self {
        Self {
            id: m.id,
            item_id: m.item_id,
            user_id: m.user_id,
            user_name: None,
            user_display_name: None,
            user_avatar: None,
            selected_text: m.selected_text,
            comment: m.comment,
            start_offset: m.start_offset,
            end_offset: m.end_offset,
            context_before: m.context_before,
            context_after: m.context_after,
            color: m.color,
            is_public: m.is_public,
            parent_id: m.parent_id,
            content_revision: m.content_revision,
            created_at: m.created_at.timestamp_millis(),
            updated_at: m.updated_at.timestamp_millis(),
            replies: None,
            reply_count: None,
        }
    }
}

/// 创建评论请求
#[derive(Debug, Deserialize)]
pub struct CreateCommentRequest {
    pub selected_text: String,
    pub comment: String,
    pub start_offset: Option<i32>,
    pub end_offset: Option<i32>,
    pub context_before: Option<String>,
    pub context_after: Option<String>,
    pub color: Option<String>,
    /// 父评论 ID（回复时指定）
    pub parent_id: Option<i32>,
    /// 是否公开批注（默认 false；与 FE CreateCommentRequest.is_public 对齐）
    #[serde(default)]
    pub is_public: Option<bool>,
}

/// 更新评论请求
#[derive(Debug, Deserialize)]
pub struct UpdateCommentRequest {
    pub comment: Option<String>,
    pub color: Option<String>,
    /// Optional visibility toggle (aligned with create; ignored when absent).
    #[serde(default)]
    pub is_public: Option<bool>,
}
