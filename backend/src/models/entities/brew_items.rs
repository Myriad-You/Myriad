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
    /// Server-owned body version; trigger increments when content or content_md changes.
    pub content_revision: i64,
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
    /// Present when this response includes an authoritative user-state snapshot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_revision: Option<i64>,
    pub content_revision: i64,
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
            content_revision: m.content_revision,
            is_read,
            is_starred,
            read_progress,
            state_revision: None,
            has_ai_annotations,
            has_ai_podcast,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ItemProjection {
    Full,
    Preview,
}

/// 文章列表查询参数
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ItemsQuery {
    pub projection: Option<ItemProjection>,
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
    /// Keyset cursor from a previous `next_cursor`. Wins over `page`.
    pub cursor: Option<String>,
}

impl Default for ItemsQuery {
    fn default() -> Self {
        Self {
            projection: None,
            source_id: None,
            category: None,
            topic: None,
            filter: Some("all".to_string()),
            search: None,
            page: Some(1),
            per_page: Some(20),
            sort_by: Some("published_at".to_string()),
            sort_order: Some("desc".to_string()),
            cursor: None,
        }
    }
}

pub struct ItemListCursor {
    pub published_at: DateTimeWithTimeZone,
    pub id: i32,
}

pub fn encode_item_cursor(published_at: DateTimeWithTimeZone, id: i32) -> String {
    format!("{}:{id}", published_at.timestamp_millis())
}

pub fn decode_item_cursor(raw: &str) -> Option<ItemListCursor> {
    let (millis, id) = raw.split_once(':')?;
    let published_at_ms: i64 = millis.parse().ok()?;
    let id: i32 = id.parse().ok()?;
    if id <= 0 {
        return None;
    }
    let published_at = chrono::DateTime::from_timestamp_millis(published_at_ms)?.fixed_offset();
    Some(ItemListCursor { published_at, id })
}

pub fn apply_item_cursor(
    query: sea_orm::Select<Entity>,
    ascending: bool,
    cursor: &ItemListCursor,
) -> sea_orm::Select<Entity> {
    use sea_orm::{ColumnTrait, Condition, QueryFilter};
    let same_time = Condition::all()
        .add(Column::PublishedAt.eq(cursor.published_at))
        .add(if ascending {
            Column::Id.gt(cursor.id)
        } else {
            Column::Id.lt(cursor.id)
        });
    let beyond = if ascending {
        Column::PublishedAt.gt(cursor.published_at)
    } else {
        Column::PublishedAt.lt(cursor.published_at)
    };
    query.filter(Condition::any().add(beyond).add(same_time))
}

pub fn split_list_page(mut items: Vec<Model>, per_page: i32) -> (Vec<Model>, Option<String>) {
    let limit = per_page.max(1) as usize;
    if items.len() <= limit {
        return (items, None);
    }
    items.truncate(limit);
    let next = items
        .last()
        .map(|item| encode_item_cursor(item.published_at, item.id));
    (items, next)
}

/// Preview reads never select either body column from storage. Null placeholders
/// retain the internal model mapping while the public preview omits content.
pub fn ordered_list_query(
    query: sea_orm::Select<Entity>,
    ascending: bool,
) -> sea_orm::Select<Entity> {
    use sea_orm::QueryOrder;
    if ascending {
        query
            .order_by_asc(Column::PublishedAt)
            .order_by_asc(Column::Id)
    } else {
        query
            .order_by_desc(Column::PublishedAt)
            .order_by_desc(Column::Id)
    }
}

pub fn preview_query(query: sea_orm::Select<Entity>) -> sea_orm::Select<Entity> {
    use sea_orm::{sea_query::Expr, QuerySelect};
    query
        .select_only()
        .columns([
            Column::Id,
            Column::SourceId,
            Column::Guid,
            Column::Title,
            Column::Link,
            Column::Summary,
            Column::Author,
            Column::Image,
            Column::AudioUrl,
            Column::VideoUrl,
            Column::Enclosures,
            Column::Categories,
            Column::PublishedAt,
            Column::FetchedAt,
            Column::WordCount,
            Column::ReadingTime,
            Column::FulltextFetched,
            Column::Topic,
            Column::ContentRevision,
        ])
        .column_as(Expr::val(Option::<String>::None), Column::Content)
        .column_as(Expr::val(Option::<String>::None), Column::ContentMd)
}

pub fn list_response_items(
    items: Vec<ItemResponse>,
    preview: bool,
) -> Result<serde_json::Value, serde_json::Error> {
    let mut value = serde_json::to_value(items)?;
    if preview {
        if let Some(items) = value.as_array_mut() {
            for item in items {
                if let Some(fields) = item.as_object_mut() {
                    fields.remove("content");
                }
            }
        }
    }
    Ok(value)
}

#[cfg(test)]
mod preview_tests {
    use super::*;
    use sea_orm::{DbBackend, QueryTrait};

    #[test]
    fn preview_sql_does_not_read_body_columns() {
        let sql = preview_query(Entity::find())
            .build(DbBackend::Postgres)
            .to_string();
        assert!(!sql.contains("\"brew_items\".\"content\""));
        assert!(!sql.contains("\"brew_items\".\"content_md\""));
        assert!(sql.contains("NULL AS \"content\""));
        assert!(sql.contains("\"brew_items\".\"summary\""));
        assert!(sql.contains("\"brew_items\".\"content_revision\""));
    }

    #[test]
    fn item_cursor_roundtrip_and_rejects_garbage() {
        let published_at = chrono::DateTime::from_timestamp_millis(1_700_000_000_000)
            .unwrap()
            .fixed_offset();
        let raw = encode_item_cursor(published_at, 9);
        let cursor = decode_item_cursor(&raw).unwrap();
        assert_eq!(cursor.id, 9);
        assert_eq!(cursor.published_at.timestamp_millis(), 1_700_000_000_000);
        assert!(decode_item_cursor("").is_none());
        assert!(decode_item_cursor("1700000000000").is_none());
        assert!(decode_item_cursor("x:9").is_none());
        assert!(decode_item_cursor("1700000000000:0").is_none());
        assert!(decode_item_cursor("1700000000000:-1").is_none());
    }

    #[test]
    fn item_cursor_sql_is_keyset_not_offset() {
        let cursor = decode_item_cursor("1700000000000:9").unwrap();
        let sql = apply_item_cursor(Entity::find(), false, &cursor)
            .build(DbBackend::Postgres)
            .to_string();
        assert!(!sql.to_ascii_lowercase().contains("offset"));
        assert!(sql.contains("published_at"));
        assert!(sql.contains("id"));
    }

    #[test]
    fn split_list_page_keeps_the_last_returned_row_as_cursor() {
        let stamp = chrono::DateTime::from_timestamp_millis(1_700_000_000_000)
            .unwrap()
            .fixed_offset();
        let row = |id: i32| Model {
            id,
            source_id: 1,
            guid: format!("g{id}"),
            title: format!("t{id}"),
            link: "https://example.com".into(),
            summary: None,
            content: None,
            content_md: None,
            author: None,
            image: None,
            audio_url: None,
            video_url: None,
            enclosures: None,
            categories: None,
            published_at: stamp,
            fetched_at: stamp,
            word_count: None,
            reading_time: None,
            fulltext_fetched: false,
            topic: None,
            content_revision: 1,
        };
        let (page, next) = split_list_page(vec![row(1), row(2), row(3)], 2);
        assert_eq!(
            page.iter().map(|item| item.id).collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(next.as_deref(), Some("1700000000000:2"));
        let (short, none) = split_list_page(vec![row(1)], 2);
        assert_eq!(short.len(), 1);
        assert_eq!(none, None);
    }
    #[test]
    fn preview_response_omits_body_and_full_response_preserves_it() {
        let model = Model {
            id: 1,
            source_id: 2,
            guid: "guid".into(),
            title: "title".into(),
            link: "https://example.com".into(),
            summary: Some("summary".into()),
            content: Some("<p>body</p>".into()),
            content_md: Some("body".into()),
            author: None,
            image: None,
            audio_url: None,
            video_url: None,
            enclosures: None,
            categories: None,
            published_at: chrono::Utc::now().into(),
            fetched_at: chrono::Utc::now().into(),
            word_count: Some(1),
            reading_time: Some(1),
            fulltext_fetched: true,
            topic: None,
            content_revision: 3,
        };
        let mut item = ItemResponse::from_model_with_ai(
            model,
            None,
            None,
            true,
            true,
            Some(42.0),
            false,
            false,
        );
        assert!(serde_json::to_value(&item)
            .unwrap()
            .get("state_revision")
            .is_none());
        item.state_revision = Some(0);
        assert_eq!(serde_json::to_value(&item).unwrap()["state_revision"], 0);
        item.state_revision = Some(9);
        let full = list_response_items(vec![item.clone()], false).unwrap();
        let preview = list_response_items(vec![item], true).unwrap();
        assert_eq!(full[0]["content"], "<p>body</p>");
        assert!(preview[0].get("content").is_none());
        assert!(preview[0].get("content_md").is_none());
        assert_eq!(preview[0]["summary"], "summary");
        assert_eq!(preview[0]["content_revision"], 3);
        assert_eq!(full[0]["content_revision"], 3);
        assert_eq!(preview[0]["is_starred"], true);
        assert_eq!(preview[0]["read_progress"], 42.0);
        assert_eq!(full[0]["state_revision"], 9);
        assert_eq!(preview[0]["state_revision"], 9);
    }

    #[test]
    fn projection_is_opt_in_and_unknown_projection_is_rejected() {
        let default: ItemsQuery = serde_json::from_str("{}").unwrap();
        assert_eq!(default.projection, None);
        let preview: ItemsQuery = serde_json::from_str(r#"{"projection":"preview"}"#).unwrap();
        assert_eq!(preview.projection, Some(ItemProjection::Preview));
        assert!(serde_json::from_str::<ItemsQuery>(r#"{"projection":"typo"}"#).is_err());
    }
    #[test]
    fn every_list_projection_has_a_stable_tie_breaker() {
        for ascending in [true, false] {
            for preview in [true, false] {
                let query = ordered_list_query(Entity::find(), ascending);
                let query = if preview { preview_query(query) } else { query };
                let sql = query.build(DbBackend::Postgres).to_string();
                let direction = if ascending { "ASC" } else { "DESC" };
                assert!(sql.contains(&format!("ORDER BY \"brew_items\".\"published_at\" {direction}, \"brew_items\".\"id\" {direction}")), "{sql}");
            }
        }
    }
}
