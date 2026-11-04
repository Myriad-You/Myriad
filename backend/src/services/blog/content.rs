//! 从库里取出要导出的笔记和友链。公开笔记不含 `admin_only` 源。

use chrono::Utc;
use myriad_blog::{BlogPost, BlogSettings, BlogSite, FriendLink, blog_settings_from_value};
use sea_orm::{
    ColumnTrait, Condition, ConnectionTrait, DatabaseBackend, DatabaseConnection, EntityTrait,
    QueryFilter, QueryOrder, QuerySelect, Statement,
    sea_query::{Expr, extension::postgres::PgExpr},
};

use crate::models::entities::{phantasi_items, phantasi_sources};
use crate::services::public_site::{self, listed_board_keeps};

pub async fn load_settings(db: &impl ConnectionTrait) -> BlogSettings {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT value FROM configurations WHERE key = $1",
            vec![myriad_blog::BLOG_SETTINGS_KEY.into()],
        ))
        .await;
    let Ok(Some(row)) = row else {
        return BlogSettings::default();
    };
    let Ok(value) = row.try_get::<serde_json::Value>("", "value") else {
        return BlogSettings::default();
    };
    let value = crate::services::data_key::open_config_value(myriad_blog::BLOG_SETTINGS_KEY, value)
        .unwrap_or(serde_json::Value::Null);
    blog_settings_from_value(&value)
}

pub async fn load_site(db: &DatabaseConnection, settings: &BlogSettings) -> BlogSite {
    let branding = public_site::load_site_branding(db).await;
    let locale = public_site::infer_site_locale(&[&branding.title, &branding.description]);
    // 没配站点地址就只写站内路径，不编一个域名进 canonical 和分享卡。
    let origin = public_site::resolve_public_base_url().unwrap_or_default();
    let feed = if public_site::notes_rss_enabled(db).await {
        myriad_phantasi_notes::NOTES_RSS_PATH.to_string()
    } else {
        String::new()
    };
    BlogSite {
        origin,
        title: if settings.site_title.trim().is_empty() {
            branding.title
        } else {
            settings.site_title.clone()
        },
        description: if settings.description.trim().is_empty() {
            branding.description
        } else {
            settings.description.clone()
        },
        locale: locale.html_lang.to_ascii_lowercase(),
        params: serde_json::json!({}),
        icon: branding.favicon,
        image: branding.og_image,
        feed,
        noindex: branding.noindex,
    }
}

/// 博客的语言跟着文章走：站点标题是英文、文章是中文时，页面该标中文。
/// 认得出语言的文章里最多的那种定为博客语言；没有文章或都认不出时用站点标题推断的。
/// 和博客语言不同的文章单独标上自己的语言。
pub fn settle_locales(site: &mut BlogSite, posts: &mut [BlogPost]) {
    let found: Vec<Option<public_site::SiteLocale>> = posts
        .iter()
        .map(|post| {
            public_site::text_locale(&post.title, &myriad_blog::summarize(&post.content_html))
        })
        .collect();
    if let Some(winner) = public_site::majority_locale(&found, &site.locale) {
        site.locale = winner.html_lang.to_ascii_lowercase();
    }
    for (post, locale) in posts.iter_mut().zip(found) {
        post.locale = locale
            .map(|locale| locale.html_lang)
            .filter(|locale| !locale.eq_ignore_ascii_case(&site.locale))
            .map(str::to_string)
            .unwrap_or_default();
    }
}

pub async fn load_posts(db: &DatabaseConnection) -> Result<Vec<BlogPost>, &'static str> {
    let sources = phantasi_sources::Entity::find()
        .filter(phantasi_sources::Column::SourceType.eq(phantasi_sources::SourceType::Note))
        .filter(phantasi_sources::Column::AdminOnly.eq(false))
        .all(db)
        .await
        .map_err(|_| "Could not read published notes")?;
    if sources.is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<i32> = sources.iter().map(|source| source.id).collect();
    let mut items = phantasi_items::Entity::find()
        .filter(phantasi_items::Column::SourceId.is_in(ids))
        .all(db)
        .await
        .map_err(|_| "Could not read published notes")?;
    items.sort_by_key(|item| item.published_at);
    Ok(items.into_iter().map(item_to_post).collect())
}

/// 一条搜索结果：文章本身，加上「为什么命中」的那段上下文。
pub struct SearchHit {
    pub post: BlogPost,
    /// 命中处前后的纯文本片段，带 `…` 边界。标题命中时给摘要开头。
    pub excerpt: String,
}

/// 站内搜索：在已发布的文章里，标题 / 摘要 / 正文任一命中即算。
///
/// 沿用 [`load_posts`] 同一套「什么是已发布」的口径，避免搜索结果和静态页对不上。
/// 只做子串匹配，中文不需要分词；`%` `_` `\` 会转义，不会被当成通配符。
pub async fn search_posts(
    db: &DatabaseConnection,
    query: &str,
    limit: u64,
) -> Result<Vec<SearchHit>, &'static str> {
    let needle = query.trim();
    if needle.is_empty() {
        return Ok(Vec::new());
    }
    let sources = phantasi_sources::Entity::find()
        .filter(phantasi_sources::Column::SourceType.eq(phantasi_sources::SourceType::Note))
        .filter(phantasi_sources::Column::AdminOnly.eq(false))
        .all(db)
        .await
        .map_err(|_| "Could not read published notes")?;
    if sources.is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<i32> = sources.iter().map(|source| source.id).collect();
    let pattern = like_pattern(needle);
    let items = phantasi_items::Entity::find()
        .filter(phantasi_items::Column::SourceId.is_in(ids))
        .filter(
            Condition::any()
                .add(Expr::col(phantasi_items::Column::Title).ilike(&pattern))
                .add(Expr::col(phantasi_items::Column::Summary).ilike(&pattern))
                .add(Expr::col(phantasi_items::Column::Content).ilike(&pattern)),
        )
        .order_by_desc(phantasi_items::Column::PublishedAt)
        .limit(limit)
        .all(db)
        .await
        .map_err(|_| "Could not search published notes")?;
    Ok(items
        .into_iter()
        .map(|item| {
            let excerpt = hit_excerpt(&item, needle);
            SearchHit {
                post: item_to_post(item),
                excerpt,
            }
        })
        .collect())
}

/// 命中在哪一段就把哪一段的上下文给出来；都没命中（理论上不会）就退回正文开头。
fn hit_excerpt(item: &phantasi_items::Model, needle: &str) -> String {
    let summary = item.summary.as_deref().unwrap_or_default();
    let content = item.content.as_deref().unwrap_or_default();
    let content_text = strip_tags(content);
    if let Some(excerpt) = excerpt_around(&content_text, needle) {
        return excerpt;
    }
    if let Some(excerpt) = excerpt_around(summary, needle) {
        return excerpt;
    }
    if excerpt_around(&item.title, needle).is_some() {
        // 标题命中时正文里没有这个词，给摘要开头当说明。
        let fallback = if summary.trim().is_empty() {
            content_text
        } else {
            summary.to_string()
        };
        return clip(&fallback, 120);
    }
    clip(&content_text, 120)
}

/// `text` 里第一次出现 `needle` 的地方，取前后各 40 个字符。
fn excerpt_around(text: &str, needle: &str) -> Option<String> {
    // 先按原文找；找不到再退到大小写不敏感。小写化可能改变字节长度，
    // 所以那条路只在 ASCII 上走，避免切片错位。
    let (mut start, mut end) = match text.find(needle) {
        Some(at) => (at, at + needle.len()),
        None => {
            if !text.is_ascii() || !needle.is_ascii() {
                return None;
            }
            let at = text
                .to_ascii_lowercase()
                .find(&needle.to_ascii_lowercase())?;
            (at, at + needle.len())
        }
    };
    while start > 0 && !text.is_char_boundary(start) {
        start -= 1;
    }
    while end < text.len() && !text.is_char_boundary(end) {
        end += 1;
    }
    let before: String = text[..start]
        .chars()
        .rev()
        .take(40)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let after: String = text[end..].chars().take(40).collect();
    let mut out = String::new();
    if !before.trim().is_empty() {
        out.push('…');
        out.push_str(before.trim_start());
    }
    out.push_str(&text[start..end]);
    let after_trimmed = after.trim_end();
    if !after_trimmed.is_empty() {
        out.push_str(after_trimmed);
        out.push('…');
    }
    Some(collapse(&out))
}

/// 去掉 HTML 标签，保留文字。只处理笔记消毒器会写出的那些标签，够用。
fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut inside = false;
    for ch in html.chars() {
        match ch {
            '<' => inside = true,
            '>' => inside = false,
            _ if !inside => out.push(ch),
            _ => {}
        }
    }
    out.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn clip(text: &str, max_chars: usize) -> String {
    let collapsed = collapse(text);
    if collapsed.chars().count() <= max_chars {
        return collapsed;
    }
    let mut out: String = collapsed.chars().take(max_chars).collect();
    out.push('…');
    out
}

/// 把用户输入包成 `ILIKE` 模式，转义通配符（Postgres 的 ILIKE 默认用 `\` 转义）。
fn like_pattern(query: &str) -> String {
    let mut pattern = String::with_capacity(query.len() + 2);
    pattern.push('%');
    for ch in query.chars() {
        if matches!(ch, '%' | '_' | '\\') {
            pattern.push('\\');
        }
        pattern.push(ch);
    }
    pattern.push('%');
    pattern
}

pub async fn load_friends(db: &DatabaseConnection) -> Result<Vec<FriendLink>, &'static str> {
    let sources = phantasi_sources::Entity::find()
        .all(db)
        .await
        .map_err(|_| "Could not read friend links")?;
    let mut friends: Vec<_> = sources
        .into_iter()
        .filter(|source| {
            source.enabled
                && !source.admin_only
                && listed_board_keeps(
                    &source.source_type,
                    source.category.as_deref(),
                    source.admin_only,
                    "sites",
                )
        })
        .map(|source| FriendLink {
            name: source.name,
            url: source
                .site_url
                .filter(|url| !url.trim().is_empty())
                .unwrap_or(source.url),
            description: source.description.unwrap_or_default(),
            icon: source.icon.unwrap_or_default(),
        })
        .collect();
    friends.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(friends)
}

fn item_to_post(item: phantasi_items::Model) -> BlogPost {
    let authors = item
        .author
        .unwrap_or_default()
        .split(" · ")
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect();
    BlogPost {
        item_id: item.id,
        slug: item.slug.unwrap_or_default(),
        title: item.title,
        published_at: item.published_at.to_rfc3339(),
        updated_at: item.fetched_at.with_timezone(&Utc).to_rfc3339(),
        content_html: item.content.unwrap_or_default(),
        // `item.summary` 里的公式已经拍平成 TeX 源码，认不出来；只有正文 HTML 还带公式 span。
        summary_source: String::new(),
        category: item.topic.filter(|topic| !topic.trim().is_empty()),
        authors,
        cover: item.image.filter(|image| !image.trim().is_empty()),
        locale: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn post(title: &str, html: &str) -> BlogPost {
        BlogPost {
            item_id: 1,
            slug: "a".to_string(),
            title: title.to_string(),
            published_at: String::new(),
            updated_at: String::new(),
            content_html: html.to_string(),
            summary_source: String::new(),
            category: None,
            authors: Vec::new(),
            cover: None,
            locale: String::new(),
        }
    }

    #[test]
    fn blog_language_follows_the_posts() {
        let mut site = BlogSite {
            locale: "en".to_string(),
            ..BlogSite::default()
        };
        let mut posts = vec![
            post(
                "自己的桌子",
                "<p>我把写作从时间线里搬出来，不是因为讨厌人。</p>",
            ),
            post("早晨的咖啡", "<p>豆子不必好看，只要是真的烘过。</p>"),
            post("朝のコーヒー", "<p>ゆっくり書くことにした。</p>"),
            post("42", "<p>1 2 3</p>"),
        ];
        settle_locales(&mut site, &mut posts);
        assert_eq!(site.locale, "zh-cn");
        assert_eq!(posts[0].locale, "");
        assert_eq!(posts[2].locale, "ja");
        assert_eq!(posts[3].locale, "");

        // 没有认得出的文章：留着站点标题推断的。
        let mut site = BlogSite {
            locale: "ja".to_string(),
            ..BlogSite::default()
        };
        let mut posts = vec![post("v2", "<p>ok</p>")];
        settle_locales(&mut site, &mut posts);
        assert_eq!(site.locale, "ja");

        // 平票时站点原来的语言优先。
        let mut site = BlogSite {
            locale: "en".to_string(),
            ..BlogSite::default()
        };
        let mut posts = vec![
            post("早晨的咖啡", "<p>豆子不必好看，只要是真的烘过。</p>"),
            post(
                "Morning coffee",
                "<p>The beans do not need to look good, only to be roasted for real.</p>",
            ),
        ];
        settle_locales(&mut site, &mut posts);
        assert_eq!(site.locale, "en");
        assert_eq!(posts[0].locale, "zh-CN");
    }

    #[test]
    fn like_pattern_escapes_wildcards() {
        assert_eq!(like_pattern("搜索"), "%搜索%");
        assert_eq!(like_pattern("%"), "%\\%%");
        assert_eq!(like_pattern("a_b"), "%a\\_b%");
        assert_eq!(like_pattern("a\\b"), "%a\\\\b%");
    }

    #[test]
    fn excerpt_around_finds_chinese_mid_string() {
        let excerpt = excerpt_around("讲的是别的事情。", "是").unwrap();
        assert_eq!(excerpt, "…讲的是别的事情。…");
    }

    #[test]
    fn excerpt_around_from_the_start_has_no_leading_ellipsis() {
        let excerpt = excerpt_around("完全无关。", "完全").unwrap();
        assert_eq!(excerpt, "完全无关。…");
    }

    #[test]
    fn excerpt_around_at_the_end_has_no_trailing_ellipsis() {
        let excerpt = excerpt_around("abc开头", "开头").unwrap();
        assert_eq!(excerpt, "…abc开头");
    }

    #[test]
    fn excerpt_around_is_case_insensitive_for_ascii() {
        let excerpt = excerpt_around("Hello World", "world").unwrap();
        assert_eq!(excerpt, "…Hello World");
    }

    #[test]
    fn excerpt_around_returns_none_without_a_match() {
        assert!(excerpt_around("完全无关。", "搜索").is_none());
    }

    #[test]
    fn strip_tags_keeps_text_and_decodes_basic_entities() {
        assert_eq!(strip_tags("<p>a &amp; b</p><br/>c"), "a & bc");
    }
}
