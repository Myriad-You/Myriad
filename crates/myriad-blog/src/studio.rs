//! 创作台覆盖层：站长看博客时叠在页面上的一层，像游戏里的覆盖层，页面一直在原地。
//!
//! 只在站长请求时由后端插进响应，静态站本身没有它，访客拿到的字节不变。
//! 这里只放两样东西：带着页面上下文的宿主节点，和一个看不见的同源控制器 iframe。
//! 页面不跑脚本；顶部细条、左侧工具栏、各个工具窗口都由控制器画进宿主节点的 Shadow DOM。

/// 控制器嵌入页。
pub const STUDIO_OVERLAY_SRC: &str = "/embed/studio/overlay";

/// 控制器找宿主节点用的 id。
pub const STUDIO_HOST_ID: &str = "myriad-studio-host";

/// 当前页面在创作台眼里是什么。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StudioPageContext {
    /// 请求路径，例如 `/blog/posts/hello/`。
    pub path: String,
    /// 文章页对应的笔记条目和文档。
    pub item_id: Option<i32>,
    pub doc_id: Option<i32>,
    /// `?studio=` 要打开的工具，已经过 [`studio_open_spec`]。
    pub open: Option<String>,
}

/// `?studio=` 的值：工具名，可带一个参数，例如 `editor-12`、`comments`、`media-kept`。
/// 只收小写字母、数字和 `-`，最长 40 个字符；不合格的当没给。
pub fn studio_open_spec(raw: Option<&str>) -> Option<String> {
    let value = raw?.trim();
    let ok = !value.is_empty()
        && value.len() <= 40
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    ok.then(|| value.to_string())
}

/// 宿主节点和控制器。控制器 iframe 不懒加载：快捷键和细条都靠它。
pub fn studio_overlay_html(ctx: &StudioPageContext) -> String {
    let mut attrs = format!(" data-path=\"{}\"", escape_attr(&ctx.path));
    if let Some(item) = ctx.item_id {
        attrs.push_str(&format!(" data-item=\"{item}\""));
    }
    if let Some(doc) = ctx.doc_id {
        attrs.push_str(&format!(" data-doc=\"{doc}\""));
    }
    if let Some(open) = &ctx.open {
        attrs.push_str(&format!(" data-open=\"{}\"", escape_attr(open)));
    }
    format!(
        "<div id=\"{STUDIO_HOST_ID}\"{attrs} style=\"position:fixed;inset:0;z-index:2147483600;pointer-events:none\"></div>\
<iframe src=\"{STUDIO_OVERLAY_SRC}\" hidden aria-hidden=\"true\" tabindex=\"-1\" title=\"studio\" style=\"display:none\"></iframe>\n"
    )
}

/// 插在最后一个 `</body>` 前；没有就接在末尾。
pub fn inject_studio_island(html: &str, island: &str) -> String {
    let lower = html.to_ascii_lowercase();
    match lower.rfind("</body>") {
        Some(at) => {
            let mut out = String::with_capacity(html.len() + island.len());
            out.push_str(&html[..at]);
            out.push_str(island);
            out.push_str(&html[at..]);
            out
        }
        None => format!("{html}{island}"),
    }
}

/// 博客没有可用页面时给站长的空壳页，只放创作台覆盖层，好让站长进去修。
pub fn studio_only_page(island: &str, title: &str) -> String {
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{}</title></head><body style=\"margin:0;min-height:100vh;background:#f4f4f5\">{island}</body></html>",
        escape_attr(title)
    )
}

fn escape_attr(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_spec_only_takes_plain_tool_names() {
        assert_eq!(studio_open_spec(None), None);
        assert_eq!(studio_open_spec(Some("editor-12")).as_deref(), Some("editor-12"));
        assert_eq!(studio_open_spec(Some(" comments ")).as_deref(), Some("comments"));
        assert_eq!(studio_open_spec(Some("")), None);
        assert_eq!(studio_open_spec(Some("/notes/12")), None);
        assert_eq!(studio_open_spec(Some("Editor")), None);
        assert_eq!(studio_open_spec(Some("a\"b")), None);
        assert_eq!(studio_open_spec(Some(&"x".repeat(41))), None);
    }

    #[test]
    fn island_goes_before_the_last_body_close() {
        let page = "<html><body><p>hi</p></BODY></html>";
        let out = inject_studio_island(page, "<i>x</i>");
        assert_eq!(out, "<html><body><p>hi</p><i>x</i></BODY></html>");
        assert_eq!(inject_studio_island("plain", "<i>x</i>"), "plain<i>x</i>");
    }

    #[test]
    fn overlay_carries_page_context() {
        let html = studio_overlay_html(&StudioPageContext {
            path: "/blog/posts/a\"b/".into(),
            item_id: Some(7),
            doc_id: Some(3),
            open: Some("editor-3".into()),
        });
        assert!(html.contains("id=\"myriad-studio-host\""));
        assert!(html.contains("data-path=\"/blog/posts/a&quot;b/\""));
        assert!(html.contains("data-item=\"7\""));
        assert!(html.contains("data-doc=\"3\""));
        assert!(html.contains("data-open=\"editor-3\""));
        assert!(html.contains("src=\"/embed/studio/overlay\""));
        assert!(!html.contains("loading=\"lazy\""));

        let bare = studio_overlay_html(&StudioPageContext {
            path: "/blog/".into(),
            ..Default::default()
        });
        assert!(!bare.contains("data-item"));
        assert!(!bare.contains("data-open"));
    }
}
