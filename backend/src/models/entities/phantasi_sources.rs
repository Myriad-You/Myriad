//! Phantasi 阅读 - 订阅源实体
//!
//! 存储 RSS/Atom 订阅源信息

use sea_orm::ActiveValue;
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "phantasi_sources")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// 所属用户 ID
    pub user_id: i32,
    /// 订阅源名称
    pub name: String,
    /// 订阅源 URL
    #[sea_orm(column_type = "Text")]
    pub url: String,
    /// 订阅源类型: rss, atom, json_feed
    pub feed_type: FeedType,
    /// 来源类型: link(纯链接), rss(RSS订阅), phantasiai(AI增强订阅)
    #[sea_orm(default_value = "rss")]
    pub source_type: SourceType,
    /// 分类标签
    pub category: Option<String>,
    /// 订阅源图标
    #[sea_orm(column_type = "Text", nullable)]
    pub icon: Option<String>,
    /// 订阅源描述
    #[sea_orm(column_type = "Text", nullable)]
    pub description: Option<String>,
    /// 订阅源网站链接
    #[sea_orm(column_type = "Text", nullable)]
    pub site_url: Option<String>,
    /// 更新间隔（分钟）
    pub update_interval: i32,
    /// 最后抓取时间
    pub last_fetched_at: Option<DateTimeWithTimeZone>,
    /// 最后成功抓取时间
    pub last_success_at: Option<DateTimeWithTimeZone>,
    /// 最后错误信息
    #[sea_orm(column_type = "Text", nullable)]
    pub last_error: Option<String>,
    /// 连续错误次数
    pub error_count: i32,
    pub enabled: bool,
    /// 文章总数缓存
    pub item_count: i32,
    /// 卡片显示尺寸: full, mini
    pub card_size: Option<String>,
    /// 主题颜色（从图标提取）
    pub theme_color: Option<String>,
    /// 自定义排序顺序
    pub sort_order: Option<i32>,
    /// AI 风格标签（JSON 数组，如 ["\u6280\u672f", "\u6559\u7a0b"]\uff09
    #[sea_orm(column_type = "Json", nullable)]
    pub ai_style_tags: Option<serde_json::Value>,
    /// 额外配置（JSON 格式，用于存储 Notion token 等敏感配置）
    /// 对于 Notion 源：{ "token": "secret_xxx", "resource_type": "database" }
    #[sea_orm(column_type = "Json", nullable)]
    pub extra_config: Option<serde_json::Value>,
    /// RSSHub 路由路径（如 /bilibili/user/video/2267573）
    /// 仅当 feed_type = rsshub 时使用
    #[sea_orm(column_type = "Text", nullable)]
    pub rsshub_route: Option<String>,
    /// 仅管理员可见（非管理员用户无法看到此订阅源）
    #[sea_orm(default_value = false)]
    pub admin_only: bool,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
    /// `url_match_key(url)`；由 ActiveModel 保存钩子维护，用于按规范化 URL 去重。
    #[sea_orm(column_type = "Text", nullable)]
    #[serde(skip_serializing, default)]
    pub url_key: Option<String>,
    /// `url_match_key(site_url)`；site_url 为空时为 NULL。
    #[sea_orm(column_type = "Text", nullable)]
    #[serde(skip_serializing, default)]
    pub site_url_key: Option<String>,
}

/// 规范化 URL 比较键：小写 host、去 fragment、去尾部斜杠。无法解析时退化为
/// 去尾斜杠 + 小写。`url_key` / `site_url_key` 列存的就是它的输出。
pub fn url_match_key(url: &str) -> String {
    let Ok(mut parsed) = url::Url::parse(url) else {
        return url.trim_end_matches('/').to_ascii_lowercase();
    };
    if let Some(host) = parsed.host_str().map(|host| host.to_ascii_lowercase()) {
        let _ = parsed.set_host(Some(&host));
    }
    parsed.set_fragment(None);
    let mut key = parsed.to_string();
    while key.ends_with('/') {
        key.pop();
    }
    key
}

/// 订阅源类型
#[derive(Clone, Debug, PartialEq, Eq, EnumIter, DeriveActiveEnum, Serialize, Deserialize)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::N(20))")]
#[derive(Default)]
pub enum FeedType {
    #[sea_orm(string_value = "rss")]
    #[default]
    Rss,
    #[sea_orm(string_value = "atom")]
    Atom,
    #[sea_orm(string_value = "json_feed")]
    JsonFeed,
    #[sea_orm(string_value = "notion")]
    Notion,
    #[sea_orm(string_value = "rsshub")]
    RssHub,
}

/// 来源类型（订阅模式）
/// - Link: 纯链接，不订阅，仅作为快捷入口
/// - Rss: 标准 RSS/Atom 订阅
/// - Phantasiai: AI 增强订阅，在 RSS 基础上提供词汇注释等增强功能
/// - Note: 笔记，站长自己写的内容。没有上游 feed，条目由平台自己写入
#[derive(Clone, Debug, PartialEq, Eq, EnumIter, DeriveActiveEnum, Serialize, Deserialize)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::N(20))")]
#[derive(Default)]
pub enum SourceType {
    #[sea_orm(string_value = "link")]
    Link,
    #[sea_orm(string_value = "rss")]
    #[default]
    Rss,
    #[sea_orm(string_value = "phantasiai")]
    Phantasiai,
    #[sea_orm(string_value = "note")]
    Note,
}

/// 不联网抓取的来源类型。
///
/// 入口型只有一个链接，笔记的内容本来就在库里 —— 两者都没有上游可抓。
/// 调度器的筛选条件必须用这个常量，逐处写 `ne(Link)` 漏掉一处就是每隔
/// 半小时对着笔记源发一次无意义的请求。
pub const NON_FETCHABLE_SOURCE_TYPES: [SourceType; 2] = [SourceType::Link, SourceType::Note];

impl SourceType {
    /// 这个来源是否需要定时抓取。
    pub fn is_fetchable(&self) -> bool {
        !NON_FETCHABLE_SOURCE_TYPES.contains(self)
    }

    /// 对外的字符串形态（API 载荷、Agent 工具返回）。
    ///
    /// 取值必须与 `#[sea_orm(string_value)]` 和前端 `SourceType` 一致。加变体只改这里。
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Link => "link",
            Self::Rss => "rss",
            Self::Phantasiai => "phantasiai",
            Self::Note => "note",
        }
    }
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::phantasi_items::Entity")]
    PhantasiItems,
}

impl Related<super::phantasi_items::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::PhantasiItems.def()
    }
}

impl ActiveModel {
    /// 让 `url_key` / `site_url_key` 与当前 url / site_url 一致。`insert`/`update`/`save`
    /// 经 `before_save` 自动调用；绕过钩子的批量写（`insert_many`）必须手动调用。
    pub fn sync_url_keys(&mut self) {
        if let ActiveValue::Set(url) | ActiveValue::Unchanged(url) = &self.url {
            let key = Some(url_match_key(url));
            if !matches!(&self.url_key, ActiveValue::Set(k) | ActiveValue::Unchanged(k) if *k == key)
            {
                self.url_key = ActiveValue::Set(key);
            }
        }
        if let ActiveValue::Set(site) | ActiveValue::Unchanged(site) = &self.site_url {
            let key = site.as_deref().map(url_match_key);
            if !matches!(&self.site_url_key, ActiveValue::Set(k) | ActiveValue::Unchanged(k) if *k == key)
            {
                self.site_url_key = ActiveValue::Set(key);
            }
        }
    }
}

#[async_trait::async_trait]
impl ActiveModelBehavior for ActiveModel {
    async fn before_save<C>(mut self, _db: &C, _insert: bool) -> Result<Self, DbErr>
    where
        C: ConnectionTrait,
    {
        self.sync_url_keys();
        Ok(self)
    }
}

/// 更新订阅源的请求
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UpdateSourceRequest {
    pub name: Option<String>,
    pub category: Option<String>,
    pub update_interval: Option<i32>,
    pub enabled: Option<bool>,
    pub card_size: Option<String>,
    pub theme_color: Option<String>,
    /// 自定义图标 URL 或 Base64 数据
    pub icon: Option<String>,
    pub description: Option<String>,
    pub site_url: Option<String>,
    /// 自定义排序顺序
    pub sort_order: Option<i32>,
    /// 订阅地址。改类型时经常要一起换。
    pub url: Option<String>,
    /// 来源类型: link, rss, phantasiai
    pub source_type: Option<String>,
    /// Feed 类型: rss, atom, json_feed, notion, rsshub
    pub feed_type: Option<String>,
    /// 仅当 feed_type = rsshub 时使用
    pub rsshub_route: Option<String>,
    /// 额外配置（用于 Notion token 等）
    pub extra_config: Option<serde_json::Value>,
    /// AI 风格标签（用户自定义或 AI 生成）
    pub ai_style_tags: Option<Vec<String>>,
    /// 仅管理员可见
    pub admin_only: Option<bool>,
}

/// Icons that can be used as `src` without inflating JSON.
/// Drops empty values and `data:` URIs; those belong on disk, not in list payloads.
pub fn public_icon(icon: Option<&str>) -> Option<String> {
    let icon = icon.map(str::trim).filter(|value| !value.is_empty())?;
    if icon
        .get(..5)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("data:"))
    {
        return None;
    }
    Some(icon.to_string())
}

/// 订阅源响应（包含额外信息）
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceResponse {
    pub id: i32,
    pub name: String,
    pub url: String,
    pub feed_type: String,
    /// 来源类型: link, rss, phantasiai
    pub source_type: String,
    pub category: Option<String>,
    pub icon: Option<String>,
    pub description: Option<String>,
    pub site_url: Option<String>,
    pub update_interval: i32,
    pub last_fetched_at: Option<i64>,
    pub last_success_at: Option<i64>,
    pub last_error: Option<String>,
    pub error_count: i32,
    pub enabled: bool,
    pub item_count: i32,
    pub unread_count: i32,
    pub card_size: Option<String>,
    pub theme_color: Option<String>,
    pub sort_order: Option<i32>,
    /// AI 风格标签
    pub ai_style_tags: Option<Vec<String>>,
    /// 是否有额外配置（不返回敏感信息，只返回是否配置）
    pub has_extra_config: bool,
    /// RSSHub 路由路径
    pub rsshub_route: Option<String>,
    /// 仅管理员可见
    pub admin_only: bool,
    pub created_at: i64,
}

impl From<Model> for SourceResponse {
    fn from(m: Model) -> Self {
        Self {
            id: m.id,
            name: m.name,
            url: m.url,
            feed_type: match m.feed_type {
                FeedType::Rss => "rss".to_string(),
                FeedType::Atom => "atom".to_string(),
                FeedType::JsonFeed => "json_feed".to_string(),
                FeedType::Notion => "notion".to_string(),
                FeedType::RssHub => "rsshub".to_string(),
            },
            source_type: m.source_type.as_str().to_string(),
            category: m.category,
            icon: public_icon(m.icon.as_deref()),
            description: m.description,
            site_url: m.site_url,
            update_interval: m.update_interval,
            last_fetched_at: m.last_fetched_at.map(|t| t.timestamp_millis()),
            last_success_at: m.last_success_at.map(|t| t.timestamp_millis()),
            last_error: m.last_error,
            error_count: m.error_count,
            enabled: m.enabled,
            item_count: m.item_count,
            unread_count: 0,
            card_size: m.card_size,
            theme_color: m.theme_color,
            sort_order: m.sort_order,
            ai_style_tags: m.ai_style_tags.and_then(|v| {
                v.as_array().map(|arr| {
                    arr.iter()
                        .filter_map(|item| item.as_str().map(|s| s.to_string()))
                        .collect()
                })
            }),
            has_extra_config: m.extra_config.is_some(),
            rsshub_route: m.rsshub_route,
            admin_only: m.admin_only,
            created_at: m.created_at.timestamp_millis(),
        }
    }
}

#[cfg(test)]
mod public_icon_tests {
    use super::public_icon;

    #[test]
    fn public_icon_keeps_paths_and_http_and_drops_data_uris() {
        assert_eq!(public_icon(None), None);
        assert_eq!(public_icon(Some("")), None);
        assert_eq!(public_icon(Some("   ")), None);
        assert_eq!(public_icon(Some("data:image/jpeg;base64,/9j/4AAQ")), None);
        assert_eq!(public_icon(Some("DATA:image/png;base64,AAAA")), None);
        assert_eq!(
            public_icon(Some("/api/phantasi/icons/source_31.jpg")),
            Some("/api/phantasi/icons/source_31.jpg".into())
        );
        assert_eq!(
            public_icon(Some(" https://example.com/favicon.ico ")),
            Some("https://example.com/favicon.ico".into())
        );
    }
}
