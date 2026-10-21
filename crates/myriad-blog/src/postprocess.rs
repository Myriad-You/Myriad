//! 构建产物的 HTML 清理。CSP 才是边界，这里只是让页面上不留主题脚本。

use std::cell::Cell;

use lol_html::html_content::ContentType;
use lol_html::{RewriteStrSettings, element, rewrite_str};

use crate::config::blog_base_url;
use crate::model::BlogSite;
use crate::urlutil::{absolutize_root_url, escape_html, is_javascript_url};

/// 注入到每个页面 `<head>` 末尾的样式表地址。
pub const NOTES_CSS_HREF: &str = "/blog/_myriad/notes.css";

/// 分栏、小组件占位、块级公式和嵌入页 iframe 的样子。主题样式表不管这些类。
/// 正文（末尾挂着评论框的那个元素）里再垫几条防撑宽的：长网址折行、代码块和宽表格横着滚、
/// 图片不超过栏宽。手机上正文一宽，固定在屏幕上的导航岛和伴读就跟着跑出屏幕。这几条包在
/// `:where` 里，优先级是零，主题自己写了的照主题的。
/// 页面不跑脚本，嵌入页和它同源，由嵌入页来碰页面（frontend/src/components/embed/embedHost.ts）：
/// 带 `data-embed-fit` 的框由里面按内容定高，这里写的高度只是加载完之前的占位；
/// 里面第一帧之前把自己的明暗写到框的 `data-embed-scheme` 上，框的颜色跟着它，
/// 框和里面明暗一致，浏览器才不会在框里垫一层不透明的底。
pub const NOTES_CSS: &str = "\
.note-columns{display:grid;grid-template-columns:repeat(auto-fit,minmax(12rem,1fr));gap:1.25rem;margin:1em 0}\n\
.note-column>:first-child{margin-top:0}\n\
.note-widget{border:1px dashed currentColor;opacity:.7;border-radius:8px;padding:1rem;margin:1em 0;font-size:.9em}\n\
.note-widget::before{content:\"Myriad 小组件：\" attr(data-widget) \"（\" attr(data-size) \"）\"}\n\
math[display=block]{margin:1em 0;font-size:1.15em;max-width:100%;overflow-x:auto}\n\
.myriad-table{max-width:100%;overflow-x:auto}\n\
:where(:has(>iframe.myriad-comments)){overflow-wrap:anywhere}\n\
:where(:has(>iframe.myriad-comments) table){overflow-wrap:break-word}\n\
:where(:has(>iframe.myriad-comments) pre){overflow-x:auto}\n\
:where(:has(>iframe.myriad-comments) :is(img,video)){max-width:100%;height:auto}\n\
.myriad-comments,.myriad-friend-apply{display:block;width:100%;border:0;margin-top:2rem}\n\
.myriad-comments{height:420px}\n\
.myriad-friend-apply{height:760px}\n\
iframe[data-embed-scheme=light]{color-scheme:light}\n\
iframe[data-embed-scheme=dark]{color-scheme:dark}\n\
.myriad-companion__frame{display:none}\n\
mark.myriad-annot{padding:0 .1em;color:inherit;background:color-mix(in srgb,var(--myriad-annot,#f97316) 10%,transparent);border-bottom:2px dotted color-mix(in srgb,var(--myriad-annot,#f97316) 70%,transparent);border-radius:3px;cursor:help;transition:background-color .2s,box-shadow .2s}\n\
mark.myriad-annot:hover,mark.myriad-annot:focus-visible{outline:none;background:color-mix(in srgb,var(--myriad-annot,#f97316) 18%,transparent);box-shadow:0 0 0 2px color-mix(in srgb,var(--myriad-annot,#f97316) 30%,transparent)}\n\
mark.myriad-annot.is-flash{animation:myriad-annot-flash 1.5s ease-in-out}\n\
@keyframes myriad-annot-flash{0%,100%{box-shadow:0 0 0 2px transparent}25%,75%{background:color-mix(in srgb,var(--myriad-annot,#f97316) 26%,transparent);box-shadow:0 0 0 3px color-mix(in srgb,#a855f7 60%,transparent)}}\n\
mark.myriad-annot[data-type=reference]{--myriad-annot:#3b82f6}\n\
mark.myriad-annot[data-type=implicit]{--myriad-annot:#a855f7}\n\
mark.myriad-annot[data-type=context]{--myriad-annot:#22c55e}\n\
mark.myriad-annot[data-type=abbreviation]{--myriad-annot:#ec4899}\n\
.myriad-annot-tip{position:fixed;z-index:2147483001;max-width:min(20rem,calc(100vw - 2rem));padding:10px 12px;border:1px solid var(--myriad-tip-line,light-dark(#e8e2d4,#333));border-radius:12px;color:var(--myriad-tip-fg,light-dark(#2c2c2c,#c9c9c9));background:var(--myriad-tip-bg,light-dark(#fff9f0,#242424));color-scheme:var(--myriad-tip-scheme,normal);box-shadow:0 20px 25px -5px rgb(0 0 0/12%),0 8px 10px -6px rgb(0 0 0/10%);font:13px/1.55 ui-sans-serif,system-ui,sans-serif;pointer-events:none}\n\
.myriad-annot-tip[hidden]{display:none}\n\
.myriad-annot-tip small{display:block;margin-bottom:4px;font-size:11px;opacity:.65}\n\
.myriad-annot-tip b{display:block;margin-bottom:2px;font-weight:600}\n\
@media (prefers-reduced-motion:reduce){mark.myriad-annot{transition:none}mark.myriad-annot.is-flash{animation:none;background:color-mix(in srgb,var(--myriad-annot,#f97316) 26%,transparent)}}\n\
@media print{#myriad-companion,.myriad-annot-tip{display:none}}\n\
";

/// `/blog/*` 响应头。脚本全部不允许；样式允许内联，是因为主题把 CSS 写在标签上。
pub const BLOG_CSP: &str = "default-src 'self'; script-src 'none'; style-src 'self' 'unsafe-inline'; font-src 'self' data:; img-src 'self' data:; frame-src 'self'; connect-src 'none'; object-src 'none'; base-uri 'self'; form-action 'self'; frame-ancestors 'self'";

/// 删掉非 JSON 的 `<script>`、`on*` 属性和 `javascript:` 链接，并在 `</head>` 前挂上笔记样式。
pub fn postprocess_html(html: &str) -> String {
    rewrite_page(html, &HeadPlan::default())
}

/// 构建出的一页。`rel` 是它在输出目录里的路径（如 `posts/hello/index.html`）。
///
/// 除了 [`postprocess_html`] 的清理，还按平台的规矩补 head，第三方主题也一样：
/// - 站点不让收录时每页都写 `noindex, nofollow`；分类/标签/作者总览、标签和作者页、
///   文章列表和翻页写 `noindex, follow`，链接照样跟，页面本身不进索引。主题自己的 robots 换掉。
/// - 主题没写 canonical、图标时补上。
/// - Hugo 不出订阅，主题指向它的订阅链接删掉；站长打开了笔记 RSS 就挂那一个。
pub fn postprocess_page(html: &str, rel: &str, site: &BlogSite) -> String {
    let found = scan_head(html);
    let feed = absolutize_root_url(site.feed.trim(), &site.origin);
    let icon = absolutize_root_url(site.icon.trim(), &site.origin);
    let plan = HeadPlan {
        robots: robots_for(rel, site.noindex),
        canonical: canonical_for(rel, &site.origin).filter(|_| !found.canonical),
        icon: Some(icon).filter(|icon| !icon.is_empty() && !found.icon),
        feed: Some(feed).filter(|feed| !feed.is_empty()),
        feed_title: site.title.trim().to_string(),
    };
    rewrite_page(html, &plan)
}

/// 这一页该给搜索引擎的 robots。`None` 表示照常收录。
pub fn robots_for(rel: &str, site_noindex: bool) -> Option<&'static str> {
    if site_noindex {
        return Some("noindex, nofollow");
    }
    let rel = rel.trim_start_matches('/');
    let thin = matches!(
        rel,
        "categories/index.html" | "tags/index.html" | "authors/index.html" | "posts/index.html"
    ) || rel.starts_with("tags/")
        || rel.starts_with("authors/")
        || rel.starts_with("page/")
        || rel.contains("/page/");
    thin.then_some("noindex, follow")
}

/// `posts/hello/index.html` → `{origin}/blog/posts/hello/`。404 页和不是 `index.html` 的文件没有。
fn canonical_for(rel: &str, origin: &str) -> Option<String> {
    let rel = rel.trim_start_matches('/');
    let dir = rel.strip_suffix("index.html")?;
    if !(dir.is_empty() || dir.ends_with('/')) {
        return None;
    }
    Some(format!("{}{dir}", blog_base_url(origin)))
}

#[derive(Default)]
struct HeadPlan {
    robots: Option<&'static str>,
    canonical: Option<String>,
    icon: Option<String>,
    feed: Option<String>,
    feed_title: String,
}

#[derive(Default)]
struct HeadFound {
    canonical: bool,
    icon: bool,
}

/// 先看一遍主题已经写了什么，补 head 时不重复。
fn scan_head(html: &str) -> HeadFound {
    let canonical = Cell::new(false);
    let icon = Cell::new(false);
    let _ = rewrite_str(
        html,
        RewriteStrSettings {
            element_content_handlers: vec![element!("link[rel]", |el| {
                let rel = el.get_attribute("rel").unwrap_or_default();
                if has_token(&rel, "canonical") {
                    canonical.set(true);
                }
                if has_token(&rel, "icon") {
                    icon.set(true);
                }
                Ok(())
            })],
            ..RewriteStrSettings::new()
        },
    );
    HeadFound {
        canonical: canonical.get(),
        icon: icon.get(),
    }
}

fn rewrite_page(html: &str, plan: &HeadPlan) -> String {
    let mut head = String::new();
    if let Some(robots) = plan.robots {
        head.push_str(&format!("<meta name=\"robots\" content=\"{robots}\">"));
    }
    if let Some(canonical) = &plan.canonical {
        head.push_str(&format!(
            "<link rel=\"canonical\" href=\"{}\">",
            escape_html(canonical)
        ));
    }
    if let Some(icon) = &plan.icon {
        head.push_str(&format!(
            "<link rel=\"icon\" href=\"{}\">",
            escape_html(icon)
        ));
    }
    if let Some(feed) = &plan.feed {
        head.push_str(&format!(
            "<link rel=\"alternate\" type=\"application/rss+xml\" title=\"{}\" href=\"{}\">",
            escape_html(&plan.feed_title),
            escape_html(feed)
        ));
    }
    head.push_str(&format!(
        "<link rel=\"stylesheet\" href=\"{NOTES_CSS_HREF}\">"
    ));
    rewrite_str(
        html,
        RewriteStrSettings {
            element_content_handlers: vec![
                element!("script", |el| {
                    let ty = el.get_attribute("type").unwrap_or_default();
                    if !is_json_script_type(&ty) {
                        el.remove();
                    }
                    Ok(())
                }),
                // 主题的懒加载靠脚本把 `data-src` 换成 `src`。脚本不跑，图就一直是占位，
                // 访客看不到、搜索引擎也收不到。这里换回来，交给浏览器自己的 `loading="lazy"`。
                element!("img, source", |el| {
                    restore_lazy_media(el);
                    Ok(())
                }),
                element!("meta[name]", |el| {
                    let name = el.get_attribute("name").unwrap_or_default();
                    if plan.robots.is_some() && name.trim().eq_ignore_ascii_case("robots") {
                        el.remove();
                    }
                    Ok(())
                }),
                element!("link[rel][type]", |el| {
                    let rel = el.get_attribute("rel").unwrap_or_default();
                    let ty = el.get_attribute("type").unwrap_or_default();
                    if has_token(&rel, "alternate") && is_feed_type(&ty) {
                        el.remove();
                    }
                    Ok(())
                }),
                element!("a[href]", |el| {
                    let href = el.get_attribute("href").unwrap_or_default();
                    if is_built_feed_link(&href) {
                        match &plan.feed {
                            Some(feed) => {
                                let _ = el.set_attribute("href", feed);
                            }
                            None => el.remove_attribute("href"),
                        }
                    }
                    Ok(())
                }),
                element!("*", |el| {
                    let names: Vec<String> = el
                        .attributes()
                        .iter()
                        .map(|attr| attr.name())
                        .filter(|name| is_event_handler(name))
                        .collect();
                    for name in names {
                        el.remove_attribute(&name);
                    }
                    for name in ["href", "src", "action"] {
                        if let Some(value) = el.get_attribute(name)
                            && is_javascript_url(&value)
                        {
                            let _ = el.set_attribute(name, "#");
                        }
                    }
                    Ok(())
                }),
                element!("head", |el| {
                    el.append(&head, ContentType::Html);
                    Ok(())
                }),
            ],
            ..RewriteStrSettings::new()
        },
    )
    .unwrap_or_else(|_| html.to_string())
}

/// 懒加载库常用的地址属性。`src` 缺着、或只是 `data:` 占位时才换。
const LAZY_SRC_ATTRS: &[&str] = &["data-src", "data-lazy-src", "data-original"];
const LAZY_SRCSET_ATTRS: &[&str] = &["data-srcset", "data-lazy-srcset"];

fn restore_lazy_media(el: &mut lol_html::html_content::Element) {
    let placeholder = |value: Option<String>| {
        value.is_none_or(|value| {
            let value = value.trim().to_ascii_lowercase();
            value.is_empty() || value.starts_with("data:")
        })
    };
    let mut restored = false;
    if placeholder(el.get_attribute("src"))
        && let Some(url) = LAZY_SRC_ATTRS
            .iter()
            .find_map(|name| el.get_attribute(name))
            .filter(|url| !url.trim().is_empty())
    {
        let _ = el.set_attribute("src", &url);
        restored = true;
    }
    if placeholder(el.get_attribute("srcset"))
        && let Some(set) = LAZY_SRCSET_ATTRS
            .iter()
            .find_map(|name| el.get_attribute(name))
            .filter(|set| !set.trim().is_empty())
    {
        let _ = el.set_attribute("srcset", &set);
        restored = true;
    }
    if !restored {
        return;
    }
    if el.tag_name() == "img" && !el.has_attribute("loading") {
        let _ = el.set_attribute("loading", "lazy");
    }
    // 常见的懒加载样式在脚本加上 `lazyloaded` 之前把图藏着（透明度为零）。
    if let Some(class) = el.get_attribute("class") {
        let next: Vec<&str> = class
            .split_ascii_whitespace()
            .map(|token| match token {
                "lazyload" | "lazyloading" | "lazy" => "lazyloaded",
                other => other,
            })
            .collect();
        let _ = el.set_attribute("class", &next.join(" "));
    }
}

fn has_token(value: &str, token: &str) -> bool {
    value
        .split_ascii_whitespace()
        .any(|part| part.eq_ignore_ascii_case(token))
}

fn is_feed_type(value: &str) -> bool {
    let mime = value.split(';').next().unwrap_or("").trim();
    mime.eq_ignore_ascii_case("application/rss+xml")
        || mime.eq_ignore_ascii_case("application/atom+xml")
}

/// 主题页面上指向 Hugo 订阅（`index.xml`）的链接。Hugo 不再出订阅，它们会落到 404。
/// 外站地址不算：主题也可能链到站长别处的订阅。
fn is_built_feed_link(href: &str) -> bool {
    let path = href.split(['?', '#']).next().unwrap_or(href).trim();
    if !path.ends_with("index.xml") {
        return false;
    }
    let lower = path.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("//") {
        return lower.contains("/blog/");
    }
    true
}

/// `.js` / `.mjs` 资源。查询串和 hash 不算进后缀。
pub fn is_script_asset(path: &str) -> bool {
    let path = path.split(['?', '#']).next().unwrap_or(path);
    let file = path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
        .to_ascii_lowercase();
    file.ends_with(".js") || file.ends_with(".mjs")
}

/// [`is_script_asset`] 的别名。构建输出里的 `.js` / `.mjs` 要删掉。
pub fn is_stripped_asset(path: &str) -> bool {
    is_script_asset(path)
}

fn is_json_script_type(value: &str) -> bool {
    let mime = value.split(';').next().unwrap_or("").trim();
    mime.eq_ignore_ascii_case("application/json")
        || mime.eq_ignore_ascii_case("application/ld+json")
}

fn is_event_handler(name: &str) -> bool {
    let name = name.as_bytes();
    name.len() >= 3
        && name[0].eq_ignore_ascii_case(&b'o')
        && name[1].eq_ignore_ascii_case(&b'n')
        && name[2].is_ascii_alphanumeric()
}
