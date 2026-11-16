//! 站内搜索的结果页。纯 HTML，不需要脚本，也不需要主题配合。
//!
//! 页面本身只依赖主题构建出来的 CSS 路径，导航岛由 `serve_blog` 统一注入。

use std::path::Path;

use crate::services::blog::content::SearchHit;

/// 一页最多展示多少条。
pub const SEARCH_PAGE_LIMIT: u64 = 50;

/// 结果页的文案。后端按访客语言填好，这里只负责占位替换和转义。
pub struct SearchCopy {
    pub title: String,
    pub placeholder: String,
    pub hint: String,
    /// `{q}` 换成转义后的关键词。
    pub no_results: String,
    /// `{n}` 条数、`{q}` 关键词。
    pub results: String,
    /// 命中数达到上限时用这条，`{n}` 是展示条数。
    pub results_more: String,
    /// 站点没配标题时的兜底。
    pub site_fallback: String,
}

/// 渲染搜索结果页。`query` 已 trim 过；空串表示还没输入。
pub fn render_search_page(
    site_title: &str,
    locale: &str,
    query: &str,
    hits: &[SearchHit],
    more: bool,
    copy: &SearchCopy,
) -> String {
    let suggested = site_title.trim();
    let title = if suggested.is_empty() {
        copy.site_fallback.trim()
    } else {
        suggested
    };
    let styles = theme_stylesheets();
    let mut html = String::with_capacity(4096 + hits.len() * 320);

    html.push_str("<!doctype html>\n<html lang=\"");
    html.push_str(&escape(locale));
    html.push_str("\">\n<head>\n<meta charset=\"utf-8\">\n");
    html.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    html.push_str("<meta name=\"robots\" content=\"noindex\">\n<title>");
    if query.is_empty() {
        html.push_str(&escape(copy.title.trim()));
    } else {
        html.push_str(&escape(query));
    }
    html.push_str(" · ");
    html.push_str(&escape(title));
    html.push_str("</title>\n");
    for href in &styles {
        html.push_str("<link rel=\"stylesheet\" href=\"");
        html.push_str(&escape(href));
        html.push_str("\">\n");
    }
    html.push_str(SEARCH_CSS);
    html.push_str("</head>\n<body>\n<main class=\"myriad-search\">\n");

    html.push_str("<form class=\"myriad-search-form\" action=\"/blog/search\" method=\"get\" role=\"search\">");
    html.push_str("<input type=\"search\" name=\"q\" value=\"");
    html.push_str(&escape(query));
    html.push_str("\" placeholder=\"");
    html.push_str(&escape(copy.placeholder.trim()));
    html.push_str("\" aria-label=\"");
    html.push_str(&escape(copy.placeholder.trim()));
    html.push_str("\" autofocus>");
    html.push_str("<button type=\"submit\">");
    html.push_str(&escape(copy.title.trim()));
    html.push_str("</button></form>\n");

    if query.is_empty() {
        html.push_str("<p class=\"myriad-search-hint\">");
        html.push_str(&escape(copy.hint.trim()));
        html.push_str("</p>\n");
    } else if hits.is_empty() {
        html.push_str("<p class=\"myriad-search-hint\">");
        html.push_str(&fill(&copy.no_results, &[("q", &escape(query))]));
        html.push_str("</p>\n");
    } else {
        let template = if more {
            &copy.results_more
        } else {
            &copy.results
        };
        let count = hits.len().to_string();
        html.push_str("<p class=\"myriad-search-hint\">");
        html.push_str(&fill(template, &[("n", &count), ("q", &escape(query))]));
        html.push_str("</p>\n<ul class=\"myriad-search-results\">\n");
        for hit in hits {
            let post = &hit.post;
            // 和 render_post_file 用同一份 slug：手改坏的 slug 会退回 `n{item_id}`。
            let href = myriad_blog::post_path(&myriad_blog::post_slug(post));
            let summary = if hit.excerpt.trim().is_empty() {
                myriad_blog::summarize(&post.content_html)
            } else {
                hit.excerpt.trim().to_string()
            };
            html.push_str("<li><h2><a href=\"");
            html.push_str(&escape(&href));
            html.push_str("\">");
            html.push_str(&escape(&post.title));
            html.push_str("</a></h2><p class=\"myriad-search-meta\">");
            if let Some(date) = post.published_at.get(..10) {
                html.push_str(&escape(date));
            }
            if let Some(category) = post.category.as_deref().filter(|c| !c.trim().is_empty()) {
                html.push_str(" · ");
                html.push_str(&escape(category));
            }
            html.push_str("</p>");
            if !summary.is_empty() {
                html.push_str("<p class=\"myriad-search-summary\">");
                html.push_str(&escape(&summary));
                html.push_str("</p>");
            }
            html.push_str("</li>\n");
        }
        html.push_str("</ul>\n");
    }

    html.push_str("</main>\n</body>\n</html>\n");
    html
}

/// 把 `{name}` 换成已经转义好的值。模板来自我们自己的 i18n 文件，不再转义。
fn fill(template: &str, params: &[(&str, &str)]) -> String {
    let mut out = template.to_string();
    for (name, value) in params {
        out = out.replace(&format!("{{{name}}}"), value);
    }
    out
}

/// 主题的样式表都在构建产物的 `css/` 下，按文件名排序取用（多数主题只有一个 bundle）。
fn theme_stylesheets() -> Vec<String> {
    let dir = crate::services::blog::output_dir();
    let mut hrefs = Vec::new();
    let mut names = read_dir_css(&dir.join("css"));
    names.sort();
    for name in names {
        hrefs.push(format!("/blog/css/{name}"));
    }
    if dir.join("_myriad").join("notes.css").is_file() {
        hrefs.push("/blog/_myriad/notes.css".to_string());
    }
    hrefs
}

fn read_dir_css(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
        .filter(|name| name.ends_with(".css"))
        .collect()
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

/// 只够让结果页可读，不覆盖主题自己的排版。
const SEARCH_CSS: &str = "<style>\
.myriad-search{max-width:44rem;margin:0 auto;padding:2.5rem 1.25rem 4rem;line-height:1.7}\
.myriad-search-form{display:flex;gap:.5rem;margin-bottom:1.5rem}\
.myriad-search-form input{flex:1;min-width:0;padding:.6rem .75rem;border:1px solid rgba(127,127,127,.45);border-radius:.5rem;background:transparent;color:inherit;font:inherit}\
.myriad-search-form button{padding:.6rem 1rem;border:1px solid rgba(127,127,127,.45);border-radius:.5rem;background:transparent;color:inherit;font:inherit;cursor:pointer}\
.myriad-search-hint{opacity:.75;margin:0 0 1.5rem}\
.myriad-search-results{list-style:none;padding:0;margin:0}\
.myriad-search-results li{padding:1rem 0;border-top:1px solid rgba(127,127,127,.25)}\
.myriad-search-results h2{margin:0 0 .25rem;font-size:1.125rem;line-height:1.4}\
.myriad-search-results h2 a{text-decoration:none}\
.myriad-search-results h2 a:hover{text-decoration:underline}\
.myriad-search-meta{margin:0 0 .35rem;font-size:.85rem;opacity:.7}\
.myriad-search-summary{margin:0;font-size:.95rem;opacity:.85}\
</style>\n";
