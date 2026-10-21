//! Hugo 站点配置。站长 params 只进 `[params]`；安全项和站点骨架在根上写死。

use serde::Serialize;
use serde_json::Value;

use crate::model::{BlogSettings, BlogSite, DEFAULT_THEME_ID};
use crate::theme::sanitize_theme_id;

/// 主题目录相对 Hugo 的 `--source`。后端把主题放在 `{工作区}/themes/{id}/`。
pub const HUGO_THEMES_DIR: &str = "themes";

/// 笔记永久链接。配上 `baseURL = {origin}/blog/` 后，公开地址是 `/blog/posts/{slug}/`。
pub const POSTS_PERMALINK: &str = "/posts/:slug/";

const DEFAULT_LOCALE: &str = "zh-Hans";

/// Hugo 不出的页面：订阅只走笔记 RSS（站长打开才有），站点地图由主站统一给，
/// robots.txt 只在站点根上有意义。
const DISABLED_KINDS: &[&str] = &["rss", "sitemap", "robotstxt"];

/// 平台写给主题的参数都在 `[params.myriad]` 下，站长 params 盖不掉。
pub const MYRIAD_PARAMS_KEY: &str = "myriad";

/// `{origin}/blog/`。`origin` 末尾的斜杠会去掉，避免叠成双斜杠。
/// 没有站点地址时是 `/blog/`：宁可写站内路径，也不编一个域名进页面。
pub fn blog_base_url(origin: &str) -> String {
    format!("{}/blog/", origin.trim().trim_end_matches('/'))
}

/// 友链页的标题和菜单名，跟着博客的语言走。它会出现在页面标题里。
pub(crate) fn friends_label(locale: &str) -> &'static str {
    let locale = locale.trim().to_ascii_lowercase();
    let lang = locale.split(['-', '_']).next().unwrap_or("");
    match lang {
        "zh" if locale.contains("tw") || locale.contains("hk") || locale.contains("hant") => {
            "友情連結"
        }
        "zh" | "" => "友链",
        "ja" => "リンク",
        "ko" => "링크",
        "fr" => "Liens",
        _ => "Links",
    }
}

/// 生成 `hugo.toml`。`theme_id` 不合法时用设置里的主题，还不行就用内置主题。
///
/// 站长即使在 params 里放了 `security`、`baseURL`、`themesDir`、`theme`、`permalinks`，
/// 也不会进到根配置。这些键在写 `[params]` 之前就被丢掉。
pub fn render_hugo_config(site: &BlogSite, settings: &BlogSettings, theme_id: &str) -> String {
    let theme = sanitize_theme_id(theme_id)
        .or_else(|| sanitize_theme_id(&settings.theme_id))
        .unwrap_or_else(|| DEFAULT_THEME_ID.to_string());
    let locale = resolved_locale(site);
    let config = HugoConfig {
        base_url: blog_base_url(&site.origin),
        title: resolved_site_title(site, settings),
        locale: locale.clone(),
        // 语言名 Hugo 只收小写。
        default_content_language: locale.to_ascii_lowercase(),
        has_cjk_language: true,
        disable_kinds: DISABLED_KINDS.to_vec(),
        theme,
        themes_dir: HUGO_THEMES_DIR,
        taxonomies: Taxonomies {
            category: "categories",
            tag: "tags",
            author: "authors",
        },
        permalinks: Permalinks {
            posts: POSTS_PERMALINK,
        },
        params: params_table(site, settings),
        menus: Menus {
            main: vec![MenuEntry {
                name: friends_label(&locale),
                url: "/friends/",
                weight: 10,
            }],
        },
        security: forced_security(),
    };
    toml::to_string_pretty(&config).unwrap_or_default()
}

pub(crate) fn resolved_site_title(site: &BlogSite, settings: &BlogSettings) -> String {
    let from_settings = settings.site_title.trim();
    if !from_settings.is_empty() {
        return from_settings.to_string();
    }
    site.title.trim().to_string()
}

pub(crate) fn resolved_description(site: &BlogSite, settings: &BlogSettings) -> String {
    let from_settings = settings.description.trim();
    if !from_settings.is_empty() {
        return from_settings.to_string();
    }
    site.description.trim().to_string()
}

/// Hugo 按这个值本地化日期。中文要写文字（`zh-Hans` / `zh-Hant`）：`zh-cn` 这种地区写法它认不出，
/// 日期会退成英文。别的语言带地区也认得。
fn resolved_locale(site: &BlogSite) -> String {
    let locale = site.locale.trim();
    if locale.is_empty() {
        return DEFAULT_LOCALE.to_string();
    }
    let lower = locale.to_ascii_lowercase().replace('_', "-");
    match lower.as_str() {
        "zh" | "zh-cn" | "zh-sg" | "zh-hans" | "zh-hans-cn" => "zh-Hans".to_string(),
        "zh-tw" | "zh-hk" | "zh-mo" | "zh-hant" | "zh-hant-tw" => "zh-Hant".to_string(),
        _ => locale.to_string(),
    }
}

fn params_table(site: &BlogSite, settings: &BlogSettings) -> toml::Value {
    let mut merged = serde_json::Map::new();
    if let Some(params) = site.params.as_object() {
        for (key, value) in params {
            merged.insert(key.clone(), value.clone());
        }
    }
    if let Some(params) = settings.params.as_object() {
        for (key, value) in params {
            merged.insert(key.clone(), value.clone());
        }
    }
    merged.retain(|key, _| !is_reserved_param(key));
    if !merged.contains_key("mainSections") {
        merged.insert(
            "mainSections".to_string(),
            Value::Array(vec![Value::String("posts".to_string())]),
        );
    }
    if !merged.contains_key("description") {
        let description = resolved_description(site, settings);
        if !description.is_empty() {
            merged.insert("description".to_string(), Value::String(description));
        }
    }
    merged.insert(MYRIAD_PARAMS_KEY.to_string(), myriad_params(site));
    json_object_to_toml(&merged)
}

/// 内置主题读这些写 head。`noindex` 和 `feed` 也在这里，开关一变构建输入就变，不会沿用旧页面。
fn myriad_params(site: &BlogSite) -> Value {
    let absolute = |url: &str| crate::urlutil::absolutize_root_url(url.trim(), &site.origin);
    let mut map = serde_json::Map::new();
    for (key, value) in [
        ("icon", &site.icon),
        ("image", &site.image),
        ("feed", &site.feed),
    ] {
        if !value.trim().is_empty() {
            map.insert(key.to_string(), Value::String(absolute(value)));
        }
    }
    map.insert("noindex".to_string(), Value::Bool(site.noindex));
    Value::Object(map)
}

/// 只挡 `[params]` 顶层。嵌套表里的同名键留着，它们盖不到根上的 `[security]`。
fn is_reserved_param(key: &str) -> bool {
    matches!(
        key.to_ascii_lowercase().as_str(),
        "security"
            | "baseurl"
            | "themesdir"
            | "theme"
            | "permalinks"
            | "taxonomies"
            | "menus"
            | MYRIAD_PARAMS_KEY
    )
}

fn json_object_to_toml(map: &serde_json::Map<String, Value>) -> toml::Value {
    let mut table = toml::map::Map::new();
    for (key, value) in map {
        if let Some(value) = json_to_toml(value) {
            table.insert(key.clone(), value);
        }
    }
    toml::Value::Table(table)
}

fn json_to_toml(value: &Value) -> Option<toml::Value> {
    match value {
        Value::Null => None,
        Value::Bool(v) => Some(toml::Value::Boolean(*v)),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(toml::Value::Integer(i))
            } else if let Some(u) = n.as_u64() {
                i64::try_from(u).ok().map(toml::Value::Integer)
            } else {
                n.as_f64().map(toml::Value::Float)
            }
        }
        Value::String(s) => Some(toml::Value::String(s.clone())),
        Value::Array(items) => Some(toml::Value::Array(
            items.iter().filter_map(json_to_toml).collect(),
        )),
        Value::Object(map) => Some(json_object_to_toml(map)),
    }
}

fn forced_security() -> SecurityConfig {
    SecurityConfig {
        allow_content: vec!["^text/html$", "^text/markdown$"],
        enable_inline_shortcodes: false,
        exec: SecurityExec {
            allow: vec!["^sass$"],
            os_env: vec!["(?i)^(PATH|HOME|TMPDIR)$"],
        },
        funcs: SecurityFuncs {
            getenv: vec!["^HUGO_"],
        },
        http: SecurityHttp { urls: "none" },
    }
}

#[derive(Serialize)]
struct HugoConfig {
    #[serde(rename = "baseURL")]
    base_url: String,
    title: String,
    locale: String,
    #[serde(rename = "defaultContentLanguage")]
    default_content_language: String,
    #[serde(rename = "hasCJKLanguage")]
    has_cjk_language: bool,
    #[serde(rename = "disableKinds")]
    disable_kinds: Vec<&'static str>,
    theme: String,
    #[serde(rename = "themesDir")]
    themes_dir: &'static str,
    taxonomies: Taxonomies,
    permalinks: Permalinks,
    params: toml::Value,
    menus: Menus,
    security: SecurityConfig,
}

#[derive(Serialize)]
struct Taxonomies {
    category: &'static str,
    tag: &'static str,
    author: &'static str,
}

#[derive(Serialize)]
struct Permalinks {
    posts: &'static str,
}

#[derive(Serialize)]
struct Menus {
    main: Vec<MenuEntry>,
}

#[derive(Serialize)]
struct MenuEntry {
    name: &'static str,
    url: &'static str,
    weight: i64,
}

#[derive(Serialize)]
struct SecurityConfig {
    #[serde(rename = "allowContent")]
    allow_content: Vec<&'static str>,
    #[serde(rename = "enableInlineShortcodes")]
    enable_inline_shortcodes: bool,
    exec: SecurityExec,
    funcs: SecurityFuncs,
    http: SecurityHttp,
}

#[derive(Serialize)]
struct SecurityExec {
    allow: Vec<&'static str>,
    #[serde(rename = "osEnv")]
    os_env: Vec<&'static str>,
}

#[derive(Serialize)]
struct SecurityFuncs {
    getenv: Vec<&'static str>,
}

#[derive(Serialize)]
struct SecurityHttp {
    urls: &'static str,
}
