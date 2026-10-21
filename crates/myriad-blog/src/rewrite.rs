//! 正文 HTML 变换：公式 span 转 MathML，表格包一层横滚，根相对地址改绝对地址，末尾挂评论 iframe。
//!
//! lol_html 按完整文档解析。笔记正文是片段，所以先包一层再把内层切回来。
//! 包层标记若已经出现在正文里，就跳过重写，原样返回，避免切错。

use latex2mathml::{DisplayStyle, latex_to_mathml};
use lol_html::html_content::ContentType;
use lol_html::{RewriteStrSettings, element, rewrite_str, text};

use crate::urlutil::{absolutize_root_url, rewrite_srcset};

const FRAG_OPEN: &str = "<myriad-frag>";
const FRAG_CLOSE: &str = "</myriad-frag>";

/// 导出前的正文。评论 iframe 在地址改写之后追加，`src` 保持 `/embed/comments/{id}`。
pub fn prepare_content_html(html: &str, origin: &str, item_id: i32) -> String {
    let rewritten = map_fragment(html, |wrapped| {
        rewrite_str(
            wrapped,
            RewriteStrSettings {
                element_content_handlers: vec![
                    element!("span[data-tex]", |el| {
                        if has_class(el.get_attribute("class").as_deref(), "math") {
                            let tex = el.get_attribute("data-tex").unwrap_or_default();
                            let tex = tex.trim();
                            if !tex.is_empty() {
                                let style = if has_class(
                                    el.get_attribute("class").as_deref(),
                                    "math-display",
                                ) {
                                    DisplayStyle::Block
                                } else {
                                    DisplayStyle::Inline
                                };
                                if let Ok(mathml) = latex_to_mathml(tex, style) {
                                    el.replace(&mathml, ContentType::Html);
                                }
                            }
                        }
                        Ok(())
                    }),
                    // 宽表格包一层能横着滚的：主题多半不管，手机上会把整页撑宽（见 NOTES_CSS）。
                    // 表格自己的样式不动，主题怎么画表格还怎么画。
                    element!("table", |el| {
                        el.before("<div class=\"myriad-table\">", ContentType::Html);
                        el.after("</div>", ContentType::Html);
                        Ok(())
                    }),
                    element!("*", |el| {
                        if el.tag_name() != "myriad-frag" {
                            for name in ["src", "href", "srcset", "poster"] {
                                if let Some(value) = el.get_attribute(name) {
                                    let next = if name == "srcset" {
                                        rewrite_srcset(&value, origin)
                                    } else {
                                        absolutize_root_url(&value, origin)
                                    };
                                    if next != value {
                                        let _ = el.set_attribute(name, &next);
                                    }
                                }
                            }
                        }
                        Ok(())
                    }),
                ],
                ..RewriteStrSettings::new()
            },
        )
    });
    format!(
        "{rewritten}\n<iframe class=\"myriad-comments\" src=\"/embed/comments/{item_id}\" loading=\"lazy\" title=\"评论\" data-embed-fit></iframe>\n"
    )
}

/// 还没有注释和播客的文章上，控制器还没有地址。
pub(crate) const DORMANT_FRAME_SRC: &str =
    "<iframe class=\"myriad-companion__frame\" data-myriad-src=";

/// 伴读：正文末尾一个看不见的同源控制器 iframe。页面不跑脚本，两侧的胶囊、面板和正文里的
/// 注释都由控制器画（frontend/src/components/embed/EmbedCompanion.tsx）；框所在的元素就是正文，
/// 胶囊贴着它的两边。
/// 有注释或播客（`ready`）时控制器照常加载。还没有的文章也放一份，但框没有地址，访客那边
/// 什么也不加载；站长打开页面时后端用 [`reveal_companion_island`] 给它地址，在面板里就能补生成。
pub fn companion_island_html(item_id: i32, ready: bool) -> String {
    let src = if ready { "src" } else { "data-myriad-src" };
    format!(
        "<iframe class=\"myriad-companion__frame\" {src}=\"/embed/companion/{item_id}\" title=\"AI 伴读\" hidden aria-hidden=\"true\" tabindex=\"-1\"></iframe>\n"
    )
}

/// 站长看到的页面：给还没内容的伴读控制器地址。只认构建时写下的那个标签，正文里的字不会
/// 被碰到（正文里的 `<` 早已转义）。
pub fn reveal_companion_island(html: &str) -> String {
    html.replacen(DORMANT_FRAME_SRC, REVEALED_FRAME_SRC, 1)
}

pub(crate) const REVEALED_FRAME_SRC: &str = "<iframe class=\"myriad-companion__frame\" src=";

/// 正文里第一张 `<img>` 的 `src`。封面去重用它，不把 `<source>` 算进去。
pub fn first_content_image(html: &str) -> Option<String> {
    let mut first = None;
    let _ = map_fragment(html, |wrapped| {
        let result = rewrite_str(
            wrapped,
            RewriteStrSettings {
                element_content_handlers: vec![element!("img", |el| {
                    if first.is_none()
                        && let Some(src) = el.get_attribute("src")
                    {
                        let src = src.trim();
                        if !src.is_empty() {
                            first = Some(src.to_string());
                        }
                    }
                    Ok(())
                })],
                ..RewriteStrSettings::new()
            },
        );
        if result.is_err() {
            first = None;
        }
        result
    });
    first
}

/// 去掉公式 span（连同里面的 TeX 文本）和脚注、链接定义，并在块级元素后补一个空格，避免段落粘在一起。
pub(crate) fn strip_for_summary(html: &str) -> String {
    map_fragment(html, |wrapped| {
        rewrite_str(
            wrapped,
            RewriteStrSettings {
                element_content_handlers: vec![
                    element!("span[data-tex]", |el| {
                        if has_class(el.get_attribute("class").as_deref(), "math") {
                            el.remove();
                        }
                        Ok(())
                    }),
                    element!(
                        "sup.footnote-reference, div.footnote-definition, div.link-definition",
                        |el| {
                            el.remove();
                            Ok(())
                        }
                    ),
                    element!(
                        "p, div, li, h1, h2, h3, h4, h5, h6, tr, blockquote, pre, br, figcaption, section, article, ul, ol, figure",
                        |el| {
                            el.after(" ", ContentType::Text);
                            Ok(())
                        }
                    ),
                ],
                ..RewriteStrSettings::new()
            },
        )
    })
}

/// 取出片段里的文本。调用方应先跑过 [`strip_for_summary`]。
pub(crate) fn html_text(html: &str) -> String {
    let mut acc = String::new();
    if html.contains("<myriad-frag") {
        return String::new();
    }
    let wrapped = wrap_fragment(html);
    let result = rewrite_str(
        &wrapped,
        RewriteStrSettings {
            element_content_handlers: vec![text!("myriad-frag, myriad-frag *", |chunk| {
                acc.push_str(chunk.as_str());
                Ok(())
            })],
            ..RewriteStrSettings::new()
        },
    );
    if result.is_err() {
        return String::new();
    }
    acc
}

fn has_class(class_attr: Option<&str>, class: &str) -> bool {
    class_attr
        .unwrap_or("")
        .split_whitespace()
        .any(|name| name == class)
}

fn map_fragment(
    html: &str,
    rewrite: impl FnOnce(&str) -> Result<String, lol_html::errors::RewritingError>,
) -> String {
    if html.contains("<myriad-frag") {
        return html.to_string();
    }
    let wrapped = wrap_fragment(html);
    match rewrite(&wrapped) {
        Ok(out) => unwrap_fragment(&out).unwrap_or_else(|| html.to_string()),
        Err(_) => html.to_string(),
    }
}

fn wrap_fragment(html: &str) -> String {
    format!("<html><head></head><body>{FRAG_OPEN}{html}{FRAG_CLOSE}</body></html>")
}

fn unwrap_fragment(html: &str) -> Option<String> {
    let start = html.find(FRAG_OPEN)? + FRAG_OPEN.len();
    let end = html.rfind(FRAG_CLOSE)?;
    if end < start {
        return None;
    }
    Some(html[start..end].to_string())
}
