//! 创作台岛：站长看博客时，页顶居中的细条，展开是盖住整页的创作台。
//!
//! 只在站长请求时由后端插进响应，静态站本身没有它，访客拿到的字节不变。
//! 页面不跑脚本，开合靠 `<details>`；iframe 是同源的创作台，由它关岛、刷新外层页面。
//! iframe 懒加载：收着的时候不去拉整个创作台。

/// 嵌入页里创作台的根地址。
pub const STUDIO_EMBED_ROOT: &str = "/embed/studio";

const STUDIO_ISLAND_CSS: &str = "\
.myriad-studio{position:fixed;inset:0 0 auto;z-index:2147483600;margin:0;padding:0;pointer-events:none;color-scheme:light dark;font:600 12px/1 ui-sans-serif,system-ui,sans-serif}\
.myriad-studio__bar{position:fixed;top:max(8px,env(safe-area-inset-top));left:50%;z-index:2;display:flex;align-items:center;gap:10px;height:30px;padding:0 6px 0 14px;transform:translateX(-50%);border-radius:999px;pointer-events:auto;list-style:none;white-space:nowrap;color:#fafaf9;background:rgba(24,24,27,.82);-webkit-backdrop-filter:blur(16px) saturate(1.5);backdrop-filter:blur(16px) saturate(1.5);box-shadow:0 8px 24px rgba(0,0,0,.2),inset 0 0 0 1px rgba(255,255,255,.1)}\
.myriad-studio__bar::-webkit-details-marker{display:none}\
.myriad-studio__bar::marker{content:\"\"}\
.myriad-studio__toggle{display:flex;align-items:center;gap:8px;height:100%;cursor:pointer;user-select:none}\
.myriad-studio__dot{width:7px;height:7px;border-radius:50%;background:linear-gradient(135deg,#f97316,#a78bfa)}\
.myriad-studio__ctx{display:flex;align-items:center;height:22px;padding:0 10px;border-radius:999px;color:inherit;text-decoration:none;background:rgba(255,255,255,.12)}\
.myriad-studio__ctx:hover{background:rgba(255,255,255,.2)}\
.myriad-studio__chev{width:18px;text-align:center;opacity:.7;transition:transform .2s ease}\
.myriad-studio[open] .myriad-studio__chev{transform:rotate(180deg)}\
.myriad-studio__bar:focus-visible,.myriad-studio__ctx:focus-visible{outline:2px solid #a78bfa;outline-offset:3px}\
.myriad-studio__sheet{position:fixed;inset:0;z-index:1;box-sizing:border-box;display:flex;padding:calc(max(8px,env(safe-area-inset-top)) + 40px) max(12px,2vw) max(12px,2vh);pointer-events:auto;background:rgba(9,9,11,.42);-webkit-backdrop-filter:blur(6px);backdrop-filter:blur(6px);animation:myriad-studio-in .22s ease-out}\
.myriad-studio__frame{flex:1;display:block;width:100%;height:100%;margin:0;border:0;border-radius:18px;background:#fafaf9;box-shadow:0 30px 80px rgba(0,0,0,.35),0 0 0 1px rgba(0,0,0,.06)}\
@media (prefers-color-scheme:dark){.myriad-studio__frame{background:#18181b;box-shadow:0 30px 80px rgba(0,0,0,.6),0 0 0 1px rgba(255,255,255,.08)}}\
@media (max-width:640px){.myriad-studio__sheet{padding:calc(max(8px,env(safe-area-inset-top)) + 40px) 0 0}.myriad-studio__frame{border-radius:18px 18px 0 0}}\
@media (prefers-reduced-motion:reduce){.myriad-studio__sheet{animation:none}.myriad-studio__chev{transition:none}}\
@keyframes myriad-studio-in{from{opacity:0}to{opacity:1}}\
";

/// 细条上的文字。按站点语言选，后端给。
#[derive(Debug, Clone, Copy)]
pub struct StudioIslandLabels {
    pub studio: &'static str,
    pub edit_post: &'static str,
}

pub fn studio_island_labels(locale: &str) -> StudioIslandLabels {
    let locale = locale.to_ascii_lowercase();
    let (studio, edit_post) = if locale.starts_with("zh-tw") || locale.starts_with("zh-hant") {
        ("創作臺", "編輯這篇")
    } else if locale.starts_with("zh") {
        ("创作台", "编辑这篇")
    } else if locale.starts_with("ja") {
        ("スタジオ", "この記事を編集")
    } else if locale.starts_with("ko") {
        ("스튜디오", "이 글 편집")
    } else if locale.starts_with("de") {
        ("Studio", "Beitrag bearbeiten")
    } else if locale.starts_with("fr") {
        ("Studio", "Modifier l’article")
    } else {
        ("Studio", "Edit this post")
    };
    StudioIslandLabels { studio, edit_post }
}

/// `?studio=` 里给的创作台内路径，例如 `/notes/12`。只收小写字母、数字、`-` 和 `/`，
/// 其余一律当成创作台首页，免得拼出别的地址。
pub fn studio_frame_src(sub_path: Option<&str>) -> String {
    let sub = sub_path.unwrap_or("").trim();
    let safe = sub.starts_with('/')
        && !sub.contains("//")
        && sub
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'/');
    if safe && sub != "/" {
        format!("{STUDIO_EMBED_ROOT}{}", sub.trim_end_matches('/'))
    } else {
        STUDIO_EMBED_ROOT.to_string()
    }
}

/// 岛本身。`edit_href` 是文章页上「编辑这篇」的地址（整页带 `?studio=` 重开）。
pub fn studio_island_html(
    frame_src: &str,
    open: bool,
    edit_href: Option<&str>,
    labels: StudioIslandLabels,
) -> String {
    let open_attr = if open { " open" } else { "" };
    let loading = if open { "" } else { " loading=\"lazy\"" };
    let ctx = edit_href
        .map(|href| {
            format!(
                "<a class=\"myriad-studio__ctx\" href=\"{}\">{}</a>",
                escape_attr(href),
                labels.edit_post
            )
        })
        .unwrap_or_default();
    format!(
        "<style>{STUDIO_ISLAND_CSS}</style>\
<details class=\"myriad-studio\" data-myriad-studio{open_attr}>\
<summary class=\"myriad-studio__bar\"><span class=\"myriad-studio__toggle\"><span class=\"myriad-studio__dot\" aria-hidden=\"true\"></span><span>{studio}</span></span>{ctx}<span class=\"myriad-studio__chev\" aria-hidden=\"true\">⌄</span></summary>\
<div class=\"myriad-studio__sheet\"><iframe class=\"myriad-studio__frame\" name=\"myriad-studio\" src=\"{src}\"{loading} title=\"{studio}\"></iframe></div>\
</details>\n",
        studio = labels.studio,
        src = escape_attr(frame_src),
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

/// 博客没有可用页面时给站长的空壳页，只放创作台岛，好让站长进去修。
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
    fn frame_src_only_takes_plain_studio_paths() {
        assert_eq!(studio_frame_src(None), "/embed/studio");
        assert_eq!(studio_frame_src(Some("/notes/12")), "/embed/studio/notes/12");
        assert_eq!(studio_frame_src(Some("/media/kept/")), "/embed/studio/media/kept");
        assert_eq!(studio_frame_src(Some("/")), "/embed/studio");
        assert_eq!(studio_frame_src(Some("//evil.example")), "/embed/studio");
        assert_eq!(studio_frame_src(Some("/notes/1?x=\"")), "/embed/studio");
        assert_eq!(studio_frame_src(Some("javascript:alert(1)")), "/embed/studio");
    }

    #[test]
    fn island_goes_before_the_last_body_close() {
        let page = "<html><body><p>hi</p></BODY></html>";
        let out = inject_studio_island(page, "<i>x</i>");
        assert_eq!(out, "<html><body><p>hi</p><i>x</i></BODY></html>");
        assert_eq!(inject_studio_island("plain", "<i>x</i>"), "plain<i>x</i>");
    }

    #[test]
    fn closed_island_lazy_loads_and_open_one_does_not() {
        let labels = studio_island_labels("zh-CN");
        let closed = studio_island_html("/embed/studio", false, None, labels);
        assert!(closed.contains("loading=\"lazy\""));
        assert!(!closed.contains(" open>"));
        let open = studio_island_html("/embed/studio/notes/3", true, Some("/blog/?studio=/notes/3"), labels);
        assert!(open.contains("data-myriad-studio open"));
        assert!(!open.contains("loading=\"lazy\""));
        assert!(open.contains("href=\"/blog/?studio=/notes/3\""));
        assert!(open.contains("创作台"));
    }

    #[test]
    fn labels_follow_site_locale() {
        assert_eq!(studio_island_labels("zh-TW").studio, "創作臺");
        assert_eq!(studio_island_labels("ja-JP").studio, "スタジオ");
        assert_eq!(studio_island_labels("en-US").edit_post, "Edit this post");
    }
}
