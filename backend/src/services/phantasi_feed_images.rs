//! 订阅正文里的图片地址。
//!
//! 用 html5ever（经 scraper）解析片段，再把 `<img>`（以及 `<picture>` 里的
//! `<source>`）的 `src` / `srcset` / 常见懒加载属性换成调用方给的缓存地址。
//! 没换成功的地址保持原样。沙箱不会跑站点的懒加载脚本，所以占位 `src`
//! 会改成已经缓存的 `data-src`。

use std::collections::{HashMap, HashSet};

use ego_tree::NodeId;
use html5ever::tendril::StrTendril;
use html5ever::{LocalName, Namespace, QualName};
use scraper::{Html, Node, Selector};
use url::Url;

/// 单篇（正文和摘要合计）最多尝试缓存的图片数。多出来的地址保持原样。
pub(crate) const MAX_CONTENT_IMAGES_PER_ITEM: usize = 20;
/// 一次入库里最多发起的图片下载数（跨文章去重之后）。
pub(crate) const MAX_CONTENT_IMAGES_PER_BATCH: usize = 64;
pub(crate) const CONTENT_IMAGE_CONCURRENCY: usize = 4;

const URL_ATTRS: &[&str] = &[
    "src",
    "data-src",
    "data-original",
    "data-original-src",
    "data-lazy-src",
    "data-lazy",
    "data-echo",
    "data-actualsrc",
];

const SRCSET_ATTRS: &[&str] = &[
    "srcset",
    "data-srcset",
    "data-lazy-srcset",
    "data-original-srcset",
];

const LAZY_ATTRS: &[&str] = &[
    "data-src",
    "data-original",
    "data-original-src",
    "data-lazy-src",
    "data-lazy",
    "data-echo",
    "data-actualsrc",
];

/// 一篇条目的正文和摘要共用 [`MAX_CONTENT_IMAGES_PER_ITEM`]。先正文，后摘要。
pub(crate) fn plan_item_image_urls(
    content: Option<&str>,
    summary: Option<&str>,
    link: &str,
) -> Vec<String> {
    let trimmed = link.trim();
    let base = (!trimmed.is_empty()).then_some(trimmed);
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for html in [content, summary].into_iter().flatten() {
        if out.len() >= MAX_CONTENT_IMAGES_PER_ITEM {
            break;
        }
        for url in collect_content_image_urls(html, base, MAX_CONTENT_IMAGES_PER_ITEM) {
            if out.len() >= MAX_CONTENT_IMAGES_PER_ITEM {
                break;
            }
            if seen.insert(url.clone()) {
                out.push(url);
            }
        }
    }
    out
}

/// 按文档顺序收集要下载的绝对 http(s) 地址，去重，并停在 `limit`。
pub(crate) fn collect_content_image_urls(
    html: &str,
    base: Option<&str>,
    limit: usize,
) -> Vec<String> {
    if limit == 0 || html.is_empty() || !html.contains('<') {
        return Vec::new();
    }
    let document = Html::parse_fragment(html);
    let base_url = parse_base(base);
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    collect_node(
        &document,
        document.tree.root().id(),
        false,
        &base_url,
        limit,
        &mut out,
        &mut seen,
    );
    out
}

/// 用 `cached`（绝对 URL → `/api/phantasi/image-cache/...`）改写正文。
/// 表是空的，或没有任何属性命中时，返回原文，避免解析器重排无关 HTML。
pub(crate) fn rewrite_content_images(
    html: &str,
    base: Option<&str>,
    cached: &HashMap<String, String>,
) -> String {
    if cached.is_empty() || html.is_empty() {
        return html.to_string();
    }
    let mut document = Html::parse_fragment(html);
    let base_url = parse_base(base);
    let root_id = document.tree.root().id();
    let changed = rewrite_node(&mut document, root_id, false, &base_url, cached);
    if !changed {
        return html.to_string();
    }
    fragment_body_html(&document)
}

fn parse_base(base: Option<&str>) -> Option<Url> {
    let base = base?.trim();
    if base.is_empty() {
        None
    } else {
        Url::parse(base).ok()
    }
}

fn collect_node(
    document: &Html,
    id: NodeId,
    parent_picture: bool,
    base: &Option<Url>,
    limit: usize,
    out: &mut Vec<String>,
    seen: &mut HashSet<String>,
) {
    if out.len() >= limit {
        return;
    }
    let Some(node) = document.tree.get(id) else {
        return;
    };
    let children = sibling_ids(node.first_child());
    let (name, raw_urls) = match node.value() {
        Node::Element(element) => {
            let name = element.name().to_string();
            let urls = if is_image_element(&name, parent_picture) {
                element_image_urls(element)
            } else {
                Vec::new()
            };
            (name, urls)
        }
        _ => (String::new(), Vec::new()),
    };
    for raw in raw_urls {
        if out.len() >= limit {
            break;
        }
        let Some(absolute) = absolute_image_url(&raw, base) else {
            continue;
        };
        if absolute.len() > 2048 || !seen.insert(absolute.clone()) {
            continue;
        }
        out.push(absolute);
    }
    let picture = name == "picture";
    for child in children {
        collect_node(document, child, picture, base, limit, out, seen);
    }
}

fn rewrite_node(
    document: &mut Html,
    id: NodeId,
    parent_picture: bool,
    base: &Option<Url>,
    cached: &HashMap<String, String>,
) -> bool {
    let (children, name) = {
        let Some(node) = document.tree.get(id) else {
            return false;
        };
        let children = sibling_ids(node.first_child());
        let name = match node.value() {
            Node::Element(element) => element.name().to_string(),
            _ => String::new(),
        };
        (children, name)
    };
    let mut changed = false;
    if is_image_element(&name, parent_picture) {
        if let Some(mut node) = document.tree.get_mut(id) {
            if let Node::Element(element) = node.value() {
                changed = rewrite_image_element(element, base, cached);
            }
        }
    }
    let picture = name == "picture";
    for child in children {
        changed |= rewrite_node(document, child, picture, base, cached);
    }
    changed
}

fn is_image_element(name: &str, parent_picture: bool) -> bool {
    name == "img" || (parent_picture && name == "source")
}

fn element_image_urls(element: &scraper::node::Element) -> Vec<String> {
    let mut urls = Vec::new();
    for name in URL_ATTRS {
        if let Some(value) = element.attr(name) {
            urls.push(value.to_string());
        }
    }
    for name in SRCSET_ATTRS {
        if let Some(value) = element.attr(name) {
            urls.extend(srcset_urls(value));
        }
    }
    urls
}

fn rewrite_image_element(
    element: &mut scraper::node::Element,
    base: &Option<Url>,
    cached: &HashMap<String, String>,
) -> bool {
    let mut changed = false;
    for name in URL_ATTRS {
        let Some(value) = element.attr(name).map(str::to_string) else {
            continue;
        };
        if let Some(hit) = lookup_cached(&value, base, cached) {
            set_local_attr(element, name, &hit);
            changed = true;
        }
    }
    for name in SRCSET_ATTRS {
        let Some(value) = element.attr(name).map(str::to_string) else {
            continue;
        };
        let rewritten = rewrite_srcset(&value, |raw| lookup_cached(raw, base, cached));
        if rewritten != value {
            set_local_attr(element, name, &rewritten);
            changed = true;
        }
    }
    let src = element.attr("src").unwrap_or("").to_string();
    if src_is_placeholder(&src) {
        for name in LAZY_ATTRS {
            let Some(value) = element.attr(name).map(str::to_string) else {
                continue;
            };
            if value.starts_with("/api/phantasi/image-cache/") {
                set_local_attr(element, "src", &value);
                changed = true;
                break;
            }
        }
    }
    changed
}

fn lookup_cached(
    raw: &str,
    base: &Option<Url>,
    cached: &HashMap<String, String>,
) -> Option<String> {
    let absolute = absolute_image_url(raw, base)?;
    cached.get(&absolute).cloned()
}

/// 解析成绝对 http(s)。缓存路径、`data:`、带用户信息的地址返回 `None`（调用方保持原样）。
fn absolute_image_url(raw: &str, base: &Option<Url>) -> Option<String> {
    let raw = raw.trim().trim_matches(|ch: char| ch == '"' || ch == '\'');
    if raw.is_empty() || raw.starts_with('#') || raw.starts_with("/api/phantasi/image-cache/") {
        return None;
    }
    let mut url = if let Some(base) = base {
        base.join(raw).ok()?
    } else {
        Url::parse(raw).ok()?
    };
    if url.scheme() != "http" && url.scheme() != "https" {
        return None;
    }
    if !url.username().is_empty() || url.password().is_some() {
        return None;
    }
    if url.path().starts_with("/api/phantasi/image-cache/") {
        return None;
    }
    url.set_fragment(None);
    Some(url.to_string())
}

fn src_is_placeholder(src: &str) -> bool {
    let trimmed = src.trim();
    if trimmed.is_empty() || trimmed == "#" {
        return true;
    }
    let lower = trimmed.to_ascii_lowercase();
    lower.starts_with("data:") || lower.starts_with("javascript:") || lower.starts_with("about:")
}

fn srcset_urls(value: &str) -> Vec<String> {
    value
        .split(',')
        .filter_map(parse_srcset_candidate)
        .map(|(url, _)| url)
        .collect()
}

fn rewrite_srcset(value: &str, mut lookup: impl FnMut(&str) -> Option<String>) -> String {
    let mut parts = Vec::new();
    for candidate in value.split(',') {
        let Some((url, descriptor)) = parse_srcset_candidate(candidate) else {
            continue;
        };
        let next = lookup(&url).unwrap_or(url);
        if descriptor.is_empty() {
            parts.push(next);
        } else {
            parts.push(format!("{next} {descriptor}"));
        }
    }
    parts.join(", ")
}

fn parse_srcset_candidate(candidate: &str) -> Option<(String, String)> {
    let candidate = candidate.trim();
    if candidate.is_empty() {
        return None;
    }
    let mut chars = candidate.chars();
    let first = chars.next()?;
    if first == '"' || first == '\'' {
        let rest: String = chars.collect();
        let end = rest.find(first)?;
        let url = rest[..end].to_string();
        let descriptor = rest[end + 1..].trim().to_string();
        if url.is_empty() {
            return None;
        }
        return Some((url, descriptor));
    }
    let mut pieces = candidate.split_whitespace();
    let url = pieces.next()?.to_string();
    let descriptor = pieces.collect::<Vec<_>>().join(" ");
    Some((url, descriptor))
}

fn set_local_attr(element: &mut scraper::node::Element, name: &str, value: &str) {
    let tendril = StrTendril::from(value);
    if let Some((_, slot)) = element
        .attrs
        .iter_mut()
        .find(|(qual, _)| &*qual.local == name)
    {
        *slot = tendril;
        return;
    }
    let qual = QualName::new(None, Namespace::from(""), LocalName::from(name));
    match element
        .attrs
        .binary_search_by(|(existing, _)| existing.cmp(&qual))
    {
        Ok(index) => element.attrs[index].1 = tendril,
        Err(index) => element.attrs.insert(index, (qual, tendril)),
    }
}

fn sibling_ids(mut node: Option<ego_tree::NodeRef<'_, Node>>) -> Vec<NodeId> {
    let mut ids = Vec::new();
    while let Some(current) = node {
        ids.push(current.id());
        node = current.next_sibling();
    }
    ids
}

fn fragment_body_html(document: &Html) -> String {
    let Ok(selector) = Selector::parse("body") else {
        return document.html();
    };
    document
        .select(&selector)
        .next()
        .map(|element| element.inner_html())
        .unwrap_or_else(|| document.html())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cached(url: &str, path: &str) -> HashMap<String, String> {
        HashMap::from([(url.to_string(), path.to_string())])
    }

    fn img_attr(html: &str, name: &str) -> Option<String> {
        let document = Html::parse_fragment(html);
        let selector = Selector::parse("img").unwrap();
        document
            .select(&selector)
            .next()
            .and_then(|element| element.attr(name).map(str::to_string))
    }

    #[test]
    fn rewrites_src_and_leaves_uncached_src() {
        let html = r#"<p>hi</p><img src="https://cdn.example/a.jpg" alt="pic"><img src="https://cdn.example/b.jpg">"#;
        let map = cached(
            "https://cdn.example/a.jpg",
            "/api/phantasi/image-cache/aa/cached.jpg",
        );
        let rewritten = rewrite_content_images(html, None, &map);
        assert_eq!(
            img_attr(&rewritten, "src"),
            Some("/api/phantasi/image-cache/aa/cached.jpg".to_string())
        );
        assert_eq!(img_attr(&rewritten, "alt"), Some("pic".to_string()));
        let document = Html::parse_fragment(&rewritten);
        let selector = Selector::parse("img").unwrap();
        let srcs: Vec<_> = document
            .select(&selector)
            .filter_map(|element| element.attr("src"))
            .collect();
        assert_eq!(
            srcs,
            vec![
                "/api/phantasi/image-cache/aa/cached.jpg",
                "https://cdn.example/b.jpg"
            ]
        );
        assert!(!rewritten.contains("<body"));
    }

    #[test]
    fn empty_map_returns_the_original_string() {
        let html = r#"<p  class="x">Hi</p><img src="https://cdn.example/a.jpg">"#;
        assert_eq!(rewrite_content_images(html, None, &HashMap::new()), html);
    }

    #[test]
    fn script_text_is_not_treated_as_an_image() {
        let html =
            r#"<script>var s = "<img src=\"https://cdn.example/a.jpg\">";</script><p>ok</p>"#;
        let map = cached(
            "https://cdn.example/a.jpg",
            "/api/phantasi/image-cache/aa/cached.jpg",
        );
        assert_eq!(rewrite_content_images(html, None, &map), html);
        assert!(collect_content_image_urls(html, None, 8).is_empty());
    }

    #[test]
    fn promotes_lazy_data_src_over_a_placeholder() {
        let html = r#"<img src="data:image/gif;base64,R0lGODlhAQABAAAAACw=" data-src="https://cdn.example/real.jpg" alt="lazy">"#;
        let map = cached(
            "https://cdn.example/real.jpg",
            "/api/phantasi/image-cache/bb/real.jpg",
        );
        let rewritten = rewrite_content_images(html, None, &map);
        assert_eq!(
            img_attr(&rewritten, "src"),
            Some("/api/phantasi/image-cache/bb/real.jpg".to_string())
        );
        assert_eq!(
            img_attr(&rewritten, "data-src"),
            Some("/api/phantasi/image-cache/bb/real.jpg".to_string())
        );
    }

    #[test]
    fn rewrites_srcset_candidates_independently() {
        let html = r#"<img src="https://cdn.example/a.jpg" srcset="https://cdn.example/a.jpg 1x, https://cdn.example/b.jpg 2x">"#;
        let map = cached(
            "https://cdn.example/a.jpg",
            "/api/phantasi/image-cache/aa/a.jpg",
        );
        let rewritten = rewrite_content_images(html, None, &map);
        let srcset = img_attr(&rewritten, "srcset").unwrap();
        assert!(srcset.contains("/api/phantasi/image-cache/aa/a.jpg 1x"));
        assert!(srcset.contains("https://cdn.example/b.jpg 2x"));
    }

    #[test]
    fn resolves_relative_urls_against_the_article() {
        let html = r#"<img src="images/a.png"><img src="/media/b.png">"#;
        let urls = collect_content_image_urls(html, Some("https://blog.example/posts/1"), 8);
        assert_eq!(
            urls,
            vec![
                "https://blog.example/posts/images/a.png".to_string(),
                "https://blog.example/media/b.png".to_string(),
            ]
        );
    }

    #[test]
    fn skips_cache_paths_data_urls_and_stops_at_the_limit() {
        let html = r#"
            <img src="/api/phantasi/image-cache/aa/already.png">
            <img src="data:image/png;base64,aaaa">
            <img src="javascript:alert(1)">
            <img src="https://cdn.example/1.jpg">
            <img src="https://cdn.example/2.jpg">
            <img src="https://cdn.example/3.jpg">
            <a href="https://cdn.example/not-an-image.jpg">x</a>
        "#;
        let urls = collect_content_image_urls(html, Some("https://blog.example/post"), 2);
        assert_eq!(
            urls,
            vec![
                "https://cdn.example/1.jpg".to_string(),
                "https://cdn.example/2.jpg".to_string(),
            ]
        );
    }

    #[test]
    fn dedups_fragment_variants_and_reads_picture_sources() {
        let html = r#"<picture><source srcset="https://cdn.example/a.jpg 480w"><img src="https://cdn.example/a.jpg#frag"></picture>"#;
        let urls = collect_content_image_urls(html, None, 8);
        assert_eq!(urls, vec!["https://cdn.example/a.jpg".to_string()]);
    }

    #[test]
    fn content_and_summary_share_one_limit_and_keep_local_media() {
        let content: String = (0..19)
            .map(|index| format!(r#"<img src="https://cdn.example/{index}.jpg">"#))
            .collect();
        let summary = r#"<img src="https://cdn.example/0.jpg"><img src="https://cdn.example/summary.jpg"><img src="https://cdn.example/extra.jpg">"#;
        let urls = plan_item_image_urls(Some(&content), Some(summary), "https://blog.example/post");
        assert_eq!(urls.len(), MAX_CONTENT_IMAGES_PER_ITEM);
        assert!(urls.iter().any(|url| url.ends_with("/summary.jpg")));
        assert!(!urls.iter().any(|url| url.ends_with("/extra.jpg")));

        let html = r#"<p><img src="/media/assets/11111111-1111-1111-1111-111111111111/a.png"><img src="https://cdn.example/a.jpg"></p>"#;
        let map = cached(
            "https://cdn.example/a.jpg",
            "/api/phantasi/image-cache/aa/a.jpg",
        );
        let rewritten = rewrite_content_images(html, None, &map);
        assert!(rewritten.contains("/media/assets/11111111-1111-1111-1111-111111111111/a.png"));
        assert!(rewritten.contains("/api/phantasi/image-cache/aa/a.jpg"));
        assert!(!rewritten.contains("https://cdn.example/a.jpg"));
    }
}
