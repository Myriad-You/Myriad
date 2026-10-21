//! 交给后端落盘的内容文件。front matter 用 JSON，公式里的反斜杠不会把 YAML 拆开。

use serde_json::{Map, Value};

use crate::config::{render_hugo_config, resolved_description, resolved_site_title};
use crate::cover::{absolutize_cover, cover_front_matter, covers_same_asset};
use crate::labels::labels;
use crate::model::{BlogPost, BlogSettings, BlogSite, FriendLink};
use crate::rewrite::{companion_island_html, first_content_image, prepare_content_html};
use crate::slug::{fallback_slug, validate_slug};
use crate::summary::summarize;
use crate::urlutil::{escape_html, is_javascript_url};

/// `content/posts/{slug}/index.html`。slug 不合法时改用 `n{item_id}`，避免路径跑出 `posts/`。
pub fn render_post_file(site: &BlogSite, post: &BlogPost, theme_id: &str) -> (String, String) {
    let slug = if validate_slug(&post.slug) {
        post.slug.clone()
    } else {
        fallback_slug(post.item_id)
    };
    let page_locale = if post.locale.trim().is_empty() {
        site.locale.as_str()
    } else {
        post.locale.as_str()
    };
    let mut body =
        prepare_content_html(&post.content_html, &site.origin, post.item_id, page_locale);
    body.push_str(&companion_island_html(
        post.item_id,
        post.companion,
        page_locale,
    ));
    let summary_source = if post.summary_source.trim().is_empty() {
        post.content_html.as_str()
    } else {
        post.summary_source.as_str()
    };
    let summary = summarize(summary_source);

    let mut map = Map::new();
    map.insert("title".to_string(), Value::String(post.title.clone()));
    if !post.published_at.is_empty() {
        map.insert("date".to_string(), Value::String(post.published_at.clone()));
    }
    if !post.updated_at.is_empty() {
        map.insert(
            "lastmod".to_string(),
            Value::String(post.updated_at.clone()),
        );
    }
    map.insert("slug".to_string(), Value::String(slug.clone()));
    if !summary.is_empty() {
        map.insert("summary".to_string(), Value::String(summary.clone()));
        map.insert("description".to_string(), Value::String(summary));
    }
    if let Some(category) = post
        .category
        .as_deref()
        .map(str::trim)
        .filter(|category| !category.is_empty())
    {
        map.insert(
            "categories".to_string(),
            Value::Array(vec![Value::String(category.to_string())]),
        );
    }
    let authors: Vec<String> = post
        .authors
        .iter()
        .map(|name| name.trim())
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect();
    if !authors.is_empty() {
        map.insert("author".to_string(), Value::String(authors[0].clone()));
        map.insert(
            "authors".to_string(),
            Value::Array(authors.into_iter().map(Value::String).collect()),
        );
    }
    let cover = post
        .cover
        .as_deref()
        .map(str::trim)
        .filter(|cover| !cover.is_empty());
    if let Some(cover) = cover
        && !covers_same_asset(&site.origin, cover, &post.content_html)
    {
        let absolute = absolutize_cover(&site.origin, cover);
        for (key, value) in cover_front_matter(theme_id, &absolute) {
            map.insert(key, value);
        }
    }
    // 分享卡和结构化数据用的图。封面和正文第一张相同时，上面不写封面键，但分享图照样要有。
    // `images` 是 Hugo 约定的分享图键，主题拿它写 og:image，不会把它当封面再画一遍。
    if let Some(image) = cover
        .map(str::to_string)
        .or_else(|| first_content_image(&post.content_html))
    {
        map.insert(
            "images".to_string(),
            Value::Array(vec![Value::String(absolutize_cover(&site.origin, &image))]),
        );
    }
    let locale = post.locale.trim();
    if !locale.is_empty() {
        map.insert("inLanguage".to_string(), Value::String(locale.to_string()));
    }

    let front =
        serde_json::to_string_pretty(&Value::Object(map)).unwrap_or_else(|_| "{}".to_string());
    (
        format!("content/posts/{slug}/index.html"),
        format!("{front}\n{body}"),
    )
}

/// `content/_index.html`。标题和 Hugo 配置用同一套回退。
pub fn render_home_file(site: &BlogSite, settings: &BlogSettings) -> (String, String) {
    let title = resolved_site_title(site, settings);
    let front = serde_json::json!({ "title": title });
    let front = serde_json::to_string_pretty(&front).unwrap_or_else(|_| "{}".to_string());
    let description = resolved_description(site, settings);
    let body = if description.is_empty() {
        String::new()
    } else {
        format!("<p>{}</p>\n", escape_html(&description))
    };
    (
        "content/_index.html".to_string(),
        format!("{front}\n{body}"),
    )
}

/// 文章列表和分类/标签/作者总览的 `_index.html`，只给标题。不写的话 Hugo 拿目录名当标题，
/// 中文博客上就是「Posts」「Tags」。
pub fn render_list_titles(site: &BlogSite) -> Vec<(String, String)> {
    let words = labels(&site.locale);
    [
        ("posts", words.posts),
        ("categories", words.categories),
        ("tags", words.tags),
        ("authors", words.authors),
    ]
    .into_iter()
    .map(|(dir, title)| {
        let front = serde_json::json!({ "title": title });
        (
            format!("content/{dir}/_index.html"),
            serde_json::to_string_pretty(&front).unwrap_or_else(|_| "{}".to_string()),
        )
    })
    .collect()
}

/// `content/friends/index.html`。文字做 HTML 转义，外链带 `rel="noopener"`，页尾是友联申请 iframe。
pub fn render_friends_file(site: &BlogSite, links: &[FriendLink]) -> (String, String) {
    let front = serde_json::json!({
        "title": labels(&site.locale).friends,
        "slug": "friends",
    });
    let front = serde_json::to_string_pretty(&front).unwrap_or_else(|_| "{}".to_string());
    let body = friends_html(site, links);
    (
        "content/friends/index.html".to_string(),
        format!("{front}\n{body}"),
    )
}

fn friends_html(site: &BlogSite, links: &[FriendLink]) -> String {
    let mut out = String::from("<ul class=\"friend-links\">\n");
    for link in links {
        let href = friend_href(&link.url, &site.origin);
        out.push_str("<li><a href=\"");
        out.push_str(&escape_html(&href));
        out.push_str("\" rel=\"noopener\">");
        out.push_str(&escape_html(link.name.trim()));
        out.push_str("</a>");
        let description = link.description.trim();
        if !description.is_empty() {
            out.push_str("<p>");
            out.push_str(&escape_html(description));
            out.push_str("</p>");
        }
        let icon = link.icon.trim();
        if !icon.is_empty() {
            let icon = absolutize_cover(&site.origin, icon);
            out.push_str("<img src=\"");
            out.push_str(&escape_html(&icon));
            out.push_str("\" alt=\"\">");
        }
        out.push_str("</li>\n");
    }
    out.push_str("</ul>\n");
    out.push_str(
        &format!(
            "<iframe class=\"myriad-friend-apply\" src=\"/embed/friend-apply\" loading=\"lazy\" title=\"{}\" data-embed-fit></iframe>\n",
            escape_html(labels(&site.locale).friend_apply)
        ),
    );
    out
}

/// 一次导出要落盘的文件：配置、首页、友链页、列表标题、每篇笔记。内容是 UTF-8 字节。
pub fn export_site_files(
    site: &BlogSite,
    settings: &BlogSettings,
    posts: &[BlogPost],
    friends: &[FriendLink],
) -> Vec<(String, Vec<u8>)> {
    let mut files = Vec::with_capacity(posts.len() + 7);
    files.push((
        "hugo.toml".to_string(),
        render_hugo_config(site, settings, &settings.theme_id).into_bytes(),
    ));
    let (path, body) = render_home_file(site, settings);
    files.push((path, body.into_bytes()));
    let (path, body) = render_friends_file(site, friends);
    files.push((path, body.into_bytes()));
    files.extend(
        render_list_titles(site)
            .into_iter()
            .map(|(path, body)| (path, body.into_bytes())),
    );
    for post in posts {
        let (path, body) = render_post_file(site, post, &settings.theme_id);
        files.push((path, body.into_bytes()));
    }
    files
}

/// 友链应当是网页地址。脚本 URL 写成 `#`，避免进到主题页里。
fn friend_href(url: &str, origin: &str) -> String {
    let url = url.trim();
    if url.is_empty() || is_javascript_url(url) || is_blocked_scheme(url) {
        return "#".to_string();
    }
    absolutize_cover(origin, url)
}

fn is_blocked_scheme(url: &str) -> bool {
    let compact: String = url.chars().filter(|ch| !ch.is_ascii_whitespace()).collect();
    let lower = compact.to_ascii_lowercase();
    lower.starts_with("data:") || lower.starts_with("vbscript:")
}
