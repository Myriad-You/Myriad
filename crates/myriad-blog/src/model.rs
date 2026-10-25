//! 博客导出用的数据。时间是已经格式化好的 RFC3339 字符串，这个 crate 不解析时区。

use serde_json::{Map, Value};

use crate::theme::sanitize_theme_id;

/// ConfigService 里博客设置的键。
pub const BLOG_SETTINGS_KEY: &str = "blog_settings";

/// 没装第三方主题、或主题 id 不合法时用的内置主题。
pub const DEFAULT_THEME_ID: &str = "myriad-default";

/// 站点身份。`origin` 是站点根（不含 `/blog`），例如 `https://example.com`；
/// 没配站点地址时为空，博客只写站内路径。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BlogSite {
    pub origin: String,
    pub title: String,
    pub description: String,
    pub locale: String,
    /// 站长写在站点上的 Hugo params。对象以外的值在生成配置时忽略。
    pub params: Value,
    /// 站点图标。站内路径或绝对地址，空则不写。
    pub icon: String,
    /// 站点分享图。文章没有图时分享卡用它，空则不写。
    pub image: String,
    /// 笔记 RSS 地址。站长没打开订阅时为空，页面不挂订阅链接。
    pub feed: String,
    /// 站点不让搜索引擎收录。每一页都写 `noindex, nofollow`。
    pub noindex: bool,
}

/// 一篇要导出的笔记。`summary_source` 可以是纯文本，也可以是带公式 span 的 HTML。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlogPost {
    pub item_id: i32,
    pub slug: String,
    pub title: String,
    /// RFC3339。空字符串表示不写 `date`。
    pub published_at: String,
    /// RFC3339。空字符串表示不写 `lastmod`。
    pub updated_at: String,
    /// 已消毒的 HTML 正文，不是 Markdown。
    pub content_html: String,
    /// 摘要材料。空则改用 [`BlogPost::content_html`]。
    pub summary_source: String,
    pub category: Option<String>,
    pub authors: Vec<String>,
    /// 封面地址。和正文第一张图相同时不写进 front matter。
    pub cover: Option<String>,
    /// 这篇的语言（BCP-47，如 `ja-JP`）。和站点一样或认不出时为空。
    pub locale: String,
}

/// 友情链接。字段按纯文本处理，导出时再转义。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FriendLink {
    pub name: String,
    pub url: String,
    pub description: String,
    pub icon: String,
}

/// `blog_settings` 的解析结果。缺字段和类型不对都落在默认值上。
#[derive(Debug, Clone, PartialEq)]
pub struct BlogSettings {
    pub theme_id: String,
    pub site_title: String,
    pub description: String,
    /// 站长自定义的 Hugo `[params]`。只接受 JSON 对象。
    pub params: Value,
}

impl Default for BlogSettings {
    fn default() -> Self {
        Self {
            theme_id: DEFAULT_THEME_ID.to_string(),
            site_title: String::new(),
            description: String::new(),
            params: Value::Object(Map::new()),
        }
    }
}

/// 从数据库读回的 JSON 解析成设置。这是配置字段的读回分支：脏数据不报错，改走默认。
pub fn blog_settings_from_value(value: &Value) -> BlogSettings {
    let Some(obj) = value.as_object() else {
        return BlogSettings::default();
    };
    let theme_id = obj
        .get("theme_id")
        .and_then(Value::as_str)
        .and_then(sanitize_theme_id)
        .unwrap_or_else(|| DEFAULT_THEME_ID.to_string());
    BlogSettings {
        theme_id,
        site_title: string_field(obj, "site_title"),
        description: string_field(obj, "description"),
        params: obj
            .get("params")
            .filter(|value| value.is_object())
            .cloned()
            .unwrap_or_else(|| Value::Object(Map::new())),
    }
}

impl BlogSettings {
    /// 写回数据库的 JSON。与 [`blog_settings_to_value`] 相同。
    pub fn to_value(&self) -> Value {
        blog_settings_to_value(self)
    }
}

/// 设置写回 JSON。调用方负责落库；这里不校验 `theme_id`，读回时 [`blog_settings_from_value`] 会再收一遍。
pub fn blog_settings_to_value(settings: &BlogSettings) -> Value {
    let params = if settings.params.is_object() {
        settings.params.clone()
    } else {
        Value::Object(Map::new())
    };
    serde_json::json!({
        "theme_id": settings.theme_id,
        "site_title": settings.site_title,
        "description": settings.description,
        "params": params,
    })
}

fn string_field(obj: &Map<String, Value>, key: &str) -> String {
    obj.get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string()
}
