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
.myriad-island{position:fixed;left:50%;bottom:max(18px,env(safe-area-inset-bottom));z-index:2147483000;transform:translateX(-50%);margin:0;padding:0;color-scheme:light dark;font:500 13px/1 ui-sans-serif,system-ui,sans-serif}\n\
.myriad-island__pill{display:flex;align-items:center;gap:8px;height:40px;padding:0 18px;border-radius:999px;list-style:none;cursor:pointer;user-select:none;white-space:nowrap;color:#fafaf9;background:rgba(24,24,27,.84);-webkit-backdrop-filter:blur(16px) saturate(1.5);backdrop-filter:blur(16px) saturate(1.5);box-shadow:0 10px 30px rgba(0,0,0,.22),inset 0 0 0 1px rgba(255,255,255,.1);transition:transform .2s ease}\n\
.myriad-island__pill::-webkit-details-marker{display:none}\n\
.myriad-island__pill::marker{content:\"\"}\n\
.myriad-island__pill:hover{transform:translateY(-1px)}\n\
.myriad-island__pill:focus-visible{outline:2px solid #a78bfa;outline-offset:3px}\n\
.myriad-island__dot{width:8px;height:8px;border-radius:50%;background:linear-gradient(135deg,#a78bfa,#38bdf8)}\n\
.myriad-island[hidden]{display:none}\n\
.myriad-island[data-myriad-empty] .myriad-island__dot{background:#71717a}\n\
.myriad-island[data-state=playing] .myriad-island__dot{animation:myriad-island-pulse 1.4s ease-in-out infinite}\n\
.myriad-island__panel{position:absolute;left:50%;bottom:calc(100% + 12px);width:min(420px,calc(100vw - 24px));max-width:none;height:min(560px,calc(100vh - 110px));transform:translateX(-50%);margin:0;border:0;border-radius:24px;background:rgba(250,250,249,.88);-webkit-backdrop-filter:blur(20px) saturate(1.4);backdrop-filter:blur(20px) saturate(1.4);box-shadow:0 24px 60px rgba(0,0,0,.22),0 0 0 1px rgba(0,0,0,.06)}\n\
.myriad-island__panel[data-embed-scheme=dark]{background:rgba(24,24,27,.88);box-shadow:0 24px 60px rgba(0,0,0,.5),0 0 0 1px rgba(255,255,255,.08)}\n\
@media (prefers-color-scheme:dark){.myriad-island__panel:not([data-embed-scheme]){background:rgba(24,24,27,.88);box-shadow:0 24px 60px rgba(0,0,0,.5),0 0 0 1px rgba(255,255,255,.08)}}\n\
@keyframes myriad-island-pulse{50%{transform:scale(1.6);opacity:.55}}\n\
mark.myriad-annot{color:inherit;background:none;border-bottom:1.5px dashed var(--myriad-annot,#f97316);border-radius:2px;cursor:help;transition:background-color .2s}\n\
mark.myriad-annot:hover,mark.myriad-annot:focus-visible,mark.myriad-annot.is-flash{outline:none;background:color-mix(in srgb,var(--myriad-annot,#f97316) 22%,transparent)}\n\
mark.myriad-annot[data-type=reference]{--myriad-annot:#3b82f6}\n\
mark.myriad-annot[data-type=implicit]{--myriad-annot:#a855f7}\n\
mark.myriad-annot[data-type=context]{--myriad-annot:#22c55e}\n\
mark.myriad-annot[data-type=abbreviation]{--myriad-annot:#ec4899}\n\
.myriad-annot-tip{position:fixed;z-index:2147483001;max-width:min(320px,calc(100vw - 24px));padding:10px 12px;border-radius:14px;color:#f5f5f4;background:rgba(24,24,27,.94);box-shadow:0 12px 32px rgba(0,0,0,.28);font:13px/1.55 ui-sans-serif,system-ui,sans-serif;pointer-events:none}\n\
.myriad-annot-tip[hidden]{display:none}\n\
.myriad-annot-tip small{display:block;margin-bottom:4px;font-size:11px;opacity:.65}\n\
.myriad-annot-tip b{display:block;margin-bottom:2px;font-weight:600}\n\
@media (prefers-reduced-motion:reduce){.myriad-island__pill,mark.myriad-annot{transition:none}.myriad-island[data-state=playing] .myriad-island__dot{animation:none}}\n\
@media print{.myriad-island,.myriad-annot-tip{display:none}}\n\
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
