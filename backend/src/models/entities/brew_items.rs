//! Brew 阅读 - 文章/内容实体
//!
//! 存储订阅源的文章内容，支持离线阅读

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "brew_items")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// 关联订阅源
    pub source_id: i32,
    /// RSS guid / Atom id（唯一标识）
    pub guid: String,
    /// 文章标题
    #[sea_orm(column_type = "Text")]
    pub title: String,
    /// 原文链接
    #[sea_orm(column_type = "Text")]
    pub link: String,
    /// 文章摘要
    #[sea_orm(column_type = "Text", nullable)]
    pub summary: Option<String>,
    /// 全文内容（用于离线阅读）
    #[sea_orm(column_type = "Text", nullable)]
    pub content: Option<String>,
    /// 作者
    pub author: Option<String>,
    /// 封面图
    #[sea_orm(column_type = "Text", nullable)]
    pub image: Option<String>,
    /// 音频链接（播客支持）
    #[sea_orm(column_type = "Text", nullable)]
    pub audio_url: Option<String>,
    /// 视频链接
    #[sea_orm(column_type = "Text", nullable)]
    pub video_url: Option<String>,
    /// 附件信息 (JSON)
    #[sea_orm(column_type = "Json", nullable)]
    pub enclosures: Option<Json>,
    /// 文章分类/标签 (JSON Array)
    #[sea_orm(column_type = "Json", nullable)]
    pub categories: Option<Json>,
    /// 发布时间
    pub published_at: DateTimeWithTimeZone,
    /// 抓取时间
    pub fetched_at: DateTimeWithTimeZone,
    /// 字数统计
    pub word_count: Option<i32>,
    /// 预估阅读时间（分钟）
    pub reading_time: Option<i32>,
    /// 是否已抓取全文
    pub fulltext_fetched: bool,
    /// 预定义主题 key（如 "engineering"），不是展示文案。
    /// NULL = 未分类，不参与主题聚类；关键词入库同步写，AI 每小时补。
    #[sea_orm(column_type = "Text", nullable)]
    pub topic: Option<String>,
    /// 手记原文（Markdown）。只有手记源下的条目有值，抓来的文章恒为 NULL。
    ///
    /// 渲染后的 HTML 在 `content` 上 —— 阅读器、RSS、联邦、SEO 都只读那一列，
    /// 这一列的唯一用途是把原文取回编辑器。
    #[sea_orm(column_type = "Text", nullable)]
    pub content_md: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::brew_sources::Entity",
        from = "Column::SourceId",
        to = "super::brew_sources::Column::Id"
    )]
    BrewSource,
    #[sea_orm(has_many = "super::brew_user_states::Entity")]
    BrewUserStates,
}

impl Related<super::brew_sources::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::BrewSource.def()
    }
}

impl Related<super::brew_user_states::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::BrewUserStates.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

/// 文章响应（包含阅读状态）
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ItemResponse {
    pub id: i32,
    pub source_id: i32,
    pub source_name: Option<String>,
    pub source_icon: Option<String>,
    pub guid: String,
    pub title: String,
    pub link: String,
    pub summary: Option<String>,
    pub content: Option<String>,
    pub author: Option<String>,
    pub image: Option<String>,
    pub audio_url: Option<String>,
    pub video_url: Option<String>,
    pub categories: Option<Vec<String>>,
    pub published_at: i64,
    pub word_count: Option<i32>,
    pub reading_time: Option<i32>,
    pub fulltext_fetched: bool,
    /// 预定义主题 key；展示文案走前端 i18n
    pub topic: Option<String>,
    // 用户状态
    pub is_read: bool,
    pub is_starred: bool,
    pub read_progress: Option<f32>,
    // AI 功能状态
    /// 是否已生成 AI 注释
    #[serde(default)]
    pub has_ai_annotations: bool,
    /// 是否已生成 AI 播客
    #[serde(default)]
    pub has_ai_podcast: bool,
}

impl ItemResponse {
    /// 带 AI 状态的构造方法
    #[allow(clippy::too_many_arguments)]
    pub fn from_model_with_ai(
        m: Model,
        source_name: Option<String>,
        source_icon: Option<String>,
        is_read: bool,
        is_starred: bool,
        read_progress: Option<f32>,
        has_ai_annotations: bool,
        has_ai_podcast: bool,
    ) -> Self {
        let categories: Option<Vec<String>> =
            m.categories.and_then(|c| serde_json::from_value(c).ok());

        Self {
            id: m.id,
            source_id: m.source_id,
            source_name,
            source_icon,
            guid: m.guid,
            title: m.title,
            link: m.link,
            summary: m.summary,
            content: m.content,
            author: m.author,
            image: m.image,
            audio_url: m.audio_url,
            video_url: m.video_url,
            categories,
            published_at: m.published_at.timestamp_millis(),
            word_count: m.word_count,
            reading_time: m.reading_time,
            fulltext_fetched: m.fulltext_fetched,
            topic: m.topic,
            is_read,
            is_starred,
            read_progress,
            has_ai_annotations,
            has_ai_podcast,
        }
    }
}

/// 文章列表查询参数
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ItemsQuery {
    /// 订阅源 ID（可选，不传则返回所有）
    pub source_id: Option<i32>,
    /// 分类筛选
    pub category: Option<String>,
    /// 主题筛选（预定义 key）。与 category 同级；`topic IS NULL` 的文章不入结果。
    pub topic: Option<String>,
    /// 筛选类型: all, unread, starred
    pub filter: Option<String>,
    /// 搜索关键词
    pub search: Option<String>,
    /// 分页: 页码
    pub page: Option<i32>,
    /// 分页: 每页数量
    pub per_page: Option<i32>,
    /// 排序字段: published_at, fetched_at
    pub sort_by: Option<String>,
    /// 排序方向: asc, desc
    pub sort_order: Option<String>,
}

impl Default for ItemsQuery {
    fn default() -> Self {
        Self {
            source_id: None,
            category: None,
            topic: None,
            filter: Some("all".to_string()),
            search: None,
            page: Some(1),
            per_page: Some(20),
            sort_by: Some("published_at".to_string()),
            sort_order: Some("desc".to_string()),
        }
    }
}
