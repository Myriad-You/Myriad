//! 构建产物的 HTML 清理。CSP 才是边界，这里只是让页面上不留主题脚本。

use lol_html::html_content::ContentType;
use lol_html::{RewriteStrSettings, element, rewrite_str};

use crate::urlutil::is_javascript_url;

/// 注入到每个页面 `<head>` 末尾的样式表地址。
pub const NOTES_CSS_HREF: &str = "/blog/_myriad/notes.css";

/// 分栏、小组件占位、块级公式和嵌入页 iframe 的样子。主题样式表不管这些类。
/// 页面不跑脚本，嵌入页和它同源，由嵌入页来碰页面（frontend/src/components/embed/embedHost.ts）：
/// 带 `data-embed-fit` 的框由里面按内容定高，这里写的高度只是加载完之前的占位；
/// 里面第一帧之前把自己的明暗写到框的 `data-embed-scheme` 上，框的颜色跟着它，
/// 框和里面明暗一致，浏览器才不会在框里垫一层不透明的底。
pub const NOTES_CSS: &str = "\
.note-columns{display:grid;grid-template-columns:repeat(auto-fit,minmax(12rem,1fr));gap:1.25rem;margin:1em 0}\n\
.note-column>:first-child{margin-top:0}\n\
.note-widget{border:1px dashed currentColor;opacity:.7;border-radius:8px;padding:1rem;margin:1em 0;font-size:.9em}\n\
.note-widget::before{content:\"Myriad 小组件：\" attr(data-widget) \"（\" attr(data-size) \"）\"}\n\
math[display=block]{margin:1em 0;font-size:1.15em}\n\
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
.myriad-annot-tip{position:fixed;z-index:2147483001;max-width:min(20rem,calc(100vw - 2rem));padding:10px 12px;border:1px solid light-dark(#e8e2d4,#333);border-radius:12px;color:light-dark(#2c2c2c,#c9c9c9);background:light-dark(#fff9f0,#242424);box-shadow:0 20px 25px -5px rgb(0 0 0/12%),0 8px 10px -6px rgb(0 0 0/10%);font:13px/1.55 ui-sans-serif,system-ui,sans-serif;pointer-events:none}\n\
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
                    el.append(
                        &format!("<link rel=\"stylesheet\" href=\"{NOTES_CSS_HREF}\">"),
                        ContentType::Html,
                    );
                    Ok(())
                }),
            ],
            ..RewriteStrSettings::new()
        },
    )
    .unwrap_or_else(|_| html.to_string())
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
