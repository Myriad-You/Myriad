//! 笔记的纯规则：Markdown 渲染、消毒，以及从正文派生的文章字段。
//!
//! 全站只有这一处把 Markdown 变成 HTML。阅读器、RSS 输出、联邦投递、SEO 摘要
//! 看到的都是这里产出的同一份 HTML —— 编辑器的预览也走同一个函数，预览和发布
//! 后不一致这件事在结构上就不可能发生。
//!
//! 没有 I/O：不读库、不发请求、不碰文件。消毒是白名单制，`ammonia` 默认放行的
//! 那些标签这里再收窄一遍。

use std::collections::{HashMap, HashSet};

use pulldown_cmark::{Event, Options, Parser, Tag, html};

mod layout;
#[cfg(test)]
mod merge;
mod old_paths;
mod status;
pub use old_paths::{
    MAX_OLD_PATH_BYTES, MAX_OLD_PATHS, OldPathError, OldPathRejection, is_reserved_old_path,
    normalize_old_path, normalize_old_paths, old_path_lookup_key,
};
pub use status::{NoteDocStatus, ScheduleError, is_due, schedule_at};

use layout::{
    LayoutSeg, MdFence, eat_fence, fence_close, fence_open, parse_note_layout, stamp_wrapper,
    widget_html,
};

/// 正文长度上限（字符）。超出的部分不截断，直接拒绝 —— 悄悄截掉用户写的东西
/// 比报错更糟。
const MAX_NOTE_CHARS: usize = 200_000;
/// 标题长度上限（字符）。库里是 text，这个上限是产品上限。
const MAX_TITLE_CHARS: usize = 200;
/// 摘要取多少字符。
const SUMMARY_CHARS: usize = 200;
/// 估算阅读速度（字/分钟）。中英混排取一个折中值，不区分语言。
const WORDS_PER_MINUTE: i32 = 400;

/// 正文不合法的原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoteError {
    /// 标题去空白后为空。
    EmptyTitle,
    /// 标题超出 [`MAX_TITLE_CHARS`]。
    TitleTooLong { chars: usize },
    /// 正文超出 [`MAX_NOTE_CHARS`]。
    BodyTooLong { chars: usize },
}

impl NoteError {
    /// 给用户看的一句话。不含内部术语。
    pub fn message(&self) -> String {
        match self {
            Self::EmptyTitle => "A title is required".to_string(),
            Self::TitleTooLong { chars } => {
                format!("Titles can be at most {MAX_TITLE_CHARS} characters (this one is {chars})")
            }
            Self::BodyTooLong { chars } => {
                format!("Notes can be at most {MAX_NOTE_CHARS} characters (this one is {chars})")
            }
        }
    }
}

/// 一篇笔记渲染后的全部派生字段。写库时逐个落到 `phantasi_items` 的同名列。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedNote {
    /// 去掉首尾空白的标题。
    pub title: String,
    /// 消毒后的 HTML 正文。
    pub html: String,
    /// 纯文本摘要，最多 [`SUMMARY_CHARS`] 字；正文为空时是 `None`。
    pub summary: Option<String>,
    /// 正文纯文本字数。
    pub word_count: i32,
    /// 预估阅读分钟数，至少 1。
    pub reading_time: i32,
    /// 正文里第一张图的地址，用作封面；没有就是 `None`。
    pub image: Option<String>,
}

/// 校验标题与正文。渲染前先过这一关，避免把超长正文送进解析器。
pub fn validate_note(title: &str, markdown: &str) -> Result<(), NoteError> {
    let title = title.trim();
    if title.is_empty() {
        return Err(NoteError::EmptyTitle);
    }
    let title_chars = title.chars().count();
    if title_chars > MAX_TITLE_CHARS {
        return Err(NoteError::TitleTooLong { chars: title_chars });
    }
    let body_chars = markdown.chars().count();
    if body_chars > MAX_NOTE_CHARS {
        return Err(NoteError::BodyTooLong { chars: body_chars });
    }
    Ok(())
}

/// 开启的 Markdown 扩展。删除线、表格、任务列表、脚注、TeX 公式 ——
/// 与编辑器工具栏能产出的语法保持一致，不多开。
fn markdown_options() -> Options {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_MATH);
    options
}

/// 允许出现在正文里的标签白名单。
///
/// 显式列出而不是在 `ammonia` 默认集上做加减 —— 默认集会随依赖升级变动，
/// 一份写死的名单读起来就是「正文能长成什么样」的完整答案。
fn allowed_tags() -> HashSet<&'static str> {
    [
        "a",
        "blockquote",
        "br",
        "code",
        "del",
        // 只为脚注定义放行；别的 div 会被剥掉外壳只留内容
        "div",
        "em",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "hr",
        "img",
        "input",
        "li",
        "ol",
        "p",
        "pre",
        "span",
        "strong",
        "sup",
        "table",
        "tbody",
        "td",
        "th",
        "thead",
        "tr",
        "ul",
    ]
    .into_iter()
    .collect()
}

fn allowed_attributes() -> HashMap<&'static str, HashSet<&'static str>> {
    let mut map: HashMap<&'static str, HashSet<&'static str>> = HashMap::new();
    map.insert("a", ["href", "title"].into_iter().collect());
    map.insert("img", ["src", "alt", "title"].into_iter().collect());
    map.insert("ol", ["start"].into_iter().collect());
    // 任务列表渲染成 checkbox；只放行这三个属性，checked 由 Markdown 决定
    map.insert(
        "input",
        ["type", "checked", "disabled"].into_iter().collect(),
    );
    // 列对齐：渲染器写的是 style，消毒前改成 `align` 属性，白名单里只放它
    map.insert("td", ["colspan", "rowspan", "align"].into_iter().collect());
    map.insert(
        "th",
        ["colspan", "rowspan", "scope", "align"]
            .into_iter()
            .collect(),
    );
    // 代码块的语言类名（`language-rust`），高亮靠它
    map.insert("code", ["class"].into_iter().collect());
    // 脚注定义的锚点；`id_prefix` 会给它和指向它的 `#` 链接一起加前缀
    // 正文分栏 / 正文小组件：类型、尺寸、配置走这三个 data-*
    map.insert(
        "div",
        ["id", "data-widget", "data-size", "data-config"]
            .into_iter()
            .collect(),
    );
    // 公式：TeX 原文挂在 data-tex，阅读器拿它去排版；标签里不再另藏一份。
    map.insert("span", ["data-tex"].into_iter().collect());
    map
}

/// 脚注用到的类名。`class` 不整体放行，只认这几个。
fn allowed_classes() -> HashMap<&'static str, HashSet<&'static str>> {
    let mut map: HashMap<&'static str, HashSet<&'static str>> = HashMap::new();
    map.insert(
        "div",
        [
            "footnote-definition",
            "link-definition",
            "note-columns",
            "note-column",
            "note-widget",
            "not-prose",
        ]
        .into_iter()
        .collect(),
    );
    map.insert(
        "span",
        ["math", "math-inline", "math-display"]
            .into_iter()
            .collect(),
    );
    map.insert(
        "sup",
        ["footnote-reference", "footnote-definition-label"]
            .into_iter()
            .collect(),
    );
    map
}

/// 脚注锚点的前缀，避免和页面上别的 id 撞车。
const FOOTNOTE_ID_PREFIX: &str = "note-fn-";

/// 把 Markdown 渲染成可以直接插进阅读器的 HTML。
///
/// 消毒是硬性的一步，不是可选项：笔记正文虽然只有站长能写，但它会经联邦发到
/// 别人的实例上，也会被搜索引擎抓走。
pub fn render_markdown(markdown: &str) -> String {
    render_markdown_inner(markdown, false)
}

/// 预览专用的顶层块标签。`hr` 自闭合；`li` 不算块，`<ul>` 里的 `<p>` 深度是 1。
const BLOCK_TAGS: &[&str] = &[
    "p",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "ul",
    "ol",
    "blockquote",
    "pre",
    "table",
    "hr",
    "div",
];

/// 编辑器预览：和 [`render_markdown`] 同一份 HTML，只多两个属性——
/// 每个顶层块带上它在原文里的字节区间 `data-md-start` / `data-md-end`。
/// 前端靠它把「点了预览的哪一段」映射回 Markdown 的哪个位置。发布用的 HTML 不带。
pub fn render_markdown_preview(markdown: &str) -> String {
    render_markdown_inner(markdown, true)
}

fn math_html(class: &str, tex: &str) -> String {
    let tex = tex.trim();
    format!(
        "<span class=\"{class}\" data-tex=\"{}\">{}</span>",
        escape_html(tex),
        escape_html(tex)
    )
}

fn pulldown_html(markdown: &str) -> String {
    let parser = Parser::new_ext(markdown, markdown_options()).map(|event| match event {
        Event::InlineMath(text) => Event::InlineHtml(math_html("math math-inline", &text).into()),
        Event::DisplayMath(text) => Event::InlineHtml(math_html("math math-display", &text).into()),
        other => other,
    });
    let mut raw = String::new();
    html::push_html(&mut raw, parser);
    raw
}

fn render_markdown_inner(markdown: &str, preview: bool) -> String {
    let markdown = expand_jammed_definitions(markdown);
    let defs = collect_link_defs(&markdown);
    let raw = render_layout_segments(&markdown, preview);
    let raw = with_link_definitions(&raw, &defs);
    sanitize_rendered(&raw, preview)
}

fn render_layout_segments(markdown: &str, preview: bool) -> String {
    let segs = parse_note_layout(markdown);
    if segs.is_empty() {
        let raw = pulldown_html(markdown);
        return if preview {
            stamp_preview_html(&raw, &top_level_block_ranges(markdown))
        } else {
            raw
        };
    }

    // 小组件是 HTML 岛，正文仍是一份 Markdown。拆开分别 pulldown 会让
    // 文末的参考定义 / 脚注够不到前面的 `[text][label]` / `[^id]`。
    let mut stitched = String::new();
    let mut text_ranges: Vec<(usize, usize)> = Vec::new();
    for seg in segs {
        match seg {
            LayoutSeg::Text { start, end } => {
                let slice = &markdown[start..end];
                if preview {
                    text_ranges.extend(
                        top_level_block_ranges(slice)
                            .into_iter()
                            .map(|(a, b)| (a + start, b + start)),
                    );
                }
                stitched.push_str(slice);
            }
            LayoutSeg::Columns {
                start,
                end,
                columns,
            } => {
                let mut inner = String::from("<div class=\"note-columns\">");
                for col in columns {
                    inner.push_str("<div class=\"note-column\">");
                    inner.push_str(&render_layout_segments(&col, false));
                    inner.push_str("</div>");
                }
                inner.push_str("</div>");
                let html = if preview {
                    stamp_wrapper(&inner, start, end)
                } else {
                    inner
                };
                push_html_island(&mut stitched, &html);
            }
            LayoutSeg::Widget {
                start,
                end,
                typ,
                size,
                config,
            } => {
                let html = widget_html(&typ, &size, config.as_deref());
                let html = if preview {
                    stamp_wrapper(&html, start, end)
                } else {
                    html
                };
                push_html_island(&mut stitched, &html);
            }
        }
    }

    let raw = pulldown_html(&stitched);
    if preview {
        stamp_preview_html(&raw, &text_ranges)
    } else {
        raw
    }
}

fn push_html_island(out: &mut String, html: &str) {
    if !out.is_empty() {
        if !out.ends_with('\n') {
            out.push('\n');
        }
        if !out.ends_with("\n\n") {
            out.push('\n');
        }
    }
    out.push_str(html);
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push('\n');
}

/// 可视层曾把相邻的 `[label]:` / `[^id]:` 揉成一行。CommonMark 不认这种行，
/// 阅读器和预览会把整段定义当正文。拆开后再交给解析器。代码围栏里不动。
fn expand_jammed_definitions(markdown: &str) -> String {
    let normalized = markdown.replace("\r\n", "\n");
    let mut fence = None;
    let mut out = Vec::new();
    for line in normalized.split('\n') {
        if eat_fence(line, &mut fence) {
            out.push(line.to_string());
            continue;
        }
        out.push(expand_definition_line(line));
    }
    close_fence_before_trailing_defs(&out.join("\n"))
}

/// 未闭合的 ` ``` ` / `~~~` 会把文末 `[label]:` / `[^id]:` 吞进代码块。
/// 定义行本身不是围栏内容；在它们前面补上闭合行。
fn close_fence_before_trailing_defs(markdown: &str) -> String {
    let lines: Vec<&str> = markdown.split('\n').collect();
    let mut fence = None;
    let mut opener: Option<(usize, MdFence)> = None;
    for (index, line) in lines.iter().enumerate() {
        if fence.is_none() {
            if let Some(open) = fence_open(line) {
                fence = Some(open);
                opener = Some((index, open));
                continue;
            }
        } else if let Some(open) = fence
            && fence_close(line, open)
        {
            fence = None;
        }
    }
    let Some((open_at, open)) = opener.filter(|_| fence.is_some()) else {
        return markdown.to_string();
    };
    let mut end = lines.len();
    while end > 0 && lines[end - 1].trim().is_empty() {
        end -= 1;
    }
    let mut start = end;
    while start > 0 && is_definition_line(lines[start - 1]) {
        start -= 1;
    }
    if start == end || start <= open_at {
        return markdown.to_string();
    }
    let closer = open.closer();
    let mut out: Vec<String> = lines[..start]
        .iter()
        .map(|line| (*line).to_string())
        .collect();
    out.push(closer);
    out.push(String::new());
    out.extend(lines[start..].iter().map(|line| (*line).to_string()));
    out.join("\n")
}

fn is_definition_line(line: &str) -> bool {
    let trimmed = line.trim();
    take_footnote_def(trimmed).is_some() || take_link_def(trimmed).is_some()
}

fn expand_definition_line(line: &str) -> String {
    let trimmed = line.trim();
    if !trimmed.starts_with('[') {
        return line.to_string();
    }
    let pieces = split_definition_blocks(trimmed);
    if pieces.len() <= 1 {
        line.to_string()
    } else {
        pieces.join("\n")
    }
}

fn split_definition_blocks(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text.to_string();
    while !rest.is_empty() {
        if let Some((head, tail)) = take_footnote_def(&rest) {
            out.push(head);
            rest = tail;
            continue;
        }
        if let Some((head, tail)) = take_link_def(&rest) {
            out.push(head);
            rest = tail;
            continue;
        }
        if out.is_empty() {
            return vec![text.to_string()];
        }
        out.push(rest);
        break;
    }
    if out.is_empty() {
        vec![text.to_string()]
    } else {
        out
    }
}

fn take_footnote_def(text: &str) -> Option<(String, String)> {
    let rest = text.strip_prefix("[^")?;
    let close = rest.find("]:")?;
    let id = &rest[..close];
    if id.is_empty() || id.chars().any(char::is_whitespace) {
        return None;
    }
    let after = rest[close + 2..].trim_start();
    let next = next_def_index(after);
    let (body, tail) = match next {
        Some(index) => (after[..index].trim(), after[index..].trim_start()),
        None => (after.trim(), ""),
    };
    let head = if body.is_empty() {
        format!("[^{id}]:")
    } else {
        format!("[^{id}]: {body}")
    };
    Some((head, tail.to_string()))
}

fn take_link_def(text: &str) -> Option<(String, String)> {
    if !text.starts_with('[') || text.starts_with("[^") {
        return None;
    }
    let close = text.find("]:")?;
    let label = &text[1..close];
    if label.is_empty() || label.contains(']') {
        return None;
    }
    let after_colon = &text[close + 2..];
    if !after_colon.starts_with(char::is_whitespace) {
        return None;
    }
    let dest_src = after_colon.trim_start();
    if dest_src.is_empty() {
        return None;
    }
    let dest_len = if dest_src.starts_with('<') {
        dest_src.find('>')? + 1
    } else {
        dest_src.find(char::is_whitespace).unwrap_or(dest_src.len())
    };
    if dest_len == 0 {
        return None;
    }
    let dest = &dest_src[..dest_len];
    let after_dest = dest_src[dest_len..].trim_start();
    let title_len = link_title_len(after_dest).unwrap_or(0);
    let title = &after_dest[..title_len];
    let consumed = text.len() - after_dest.len() + title_len;
    let tail = text[consumed..].trim_start().to_string();
    let head = if title.is_empty() {
        format!("[{label}]: {dest}")
    } else {
        format!("[{label}]: {dest} {title}")
    };
    Some((head, tail))
}

fn link_title_len(text: &str) -> Option<usize> {
    let close = match text.as_bytes().first()? {
        b'"' => text[1..].find('"')?,
        b'\'' => text[1..].find('\'')?,
        b'(' => text[1..].find(')')?,
        _ => return None,
    };
    Some(close + 2)
}

fn next_def_index(text: &str) -> Option<usize> {
    let footnote = find_ascii_marker(text, "[^", "]:", |id| {
        !id.is_empty() && !id.chars().any(char::is_whitespace)
    });
    let link = find_link_def_start(text);
    match (footnote, link) {
        (None, None) => None,
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (Some(a), Some(b)) => Some(a.min(b)),
    }
}

fn find_ascii_marker(
    text: &str,
    open: &str,
    close: &str,
    ok: impl Fn(&str) -> bool,
) -> Option<usize> {
    let mut from = 0;
    while let Some(rel) = text[from..].find(open) {
        let at = from + rel;
        let inner_at = at + open.len();
        if let Some(end) = text[inner_at..].find(close)
            && ok(&text[inner_at..inner_at + end])
        {
            return Some(at);
        }
        from = at + open.len();
    }
    None
}

struct LinkDefRef {
    label: String,
    href: String,
    title: String,
}

fn collect_link_defs(markdown: &str) -> Vec<LinkDefRef> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut fence = None;
    for line in markdown.split('\n') {
        if eat_fence(line, &mut fence) {
            continue;
        }
        let Some((def, tail)) = parse_link_def(line.trim()) else {
            continue;
        };
        if !tail.is_empty() {
            continue;
        }
        let key = def.label.to_ascii_lowercase();
        if seen.insert(key) {
            out.push(def);
        }
    }
    out
}

fn parse_link_def(text: &str) -> Option<(LinkDefRef, String)> {
    let (head, tail) = take_link_def(text)?;
    let close = head.find("]:")?;
    let label = head[1..close].to_string();
    let rest = head[close + 2..].trim_start();
    let (href, title) = if rest.starts_with('<') {
        let end = rest.find('>')?;
        let href = rest[1..end].to_string();
        (href, title_from_suffix(rest[end + 1..].trim_start()))
    } else {
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let href = rest[..end].to_string();
        (href, title_from_suffix(rest[end..].trim_start()))
    };
    Some((LinkDefRef { label, href, title }, tail))
}

fn title_from_suffix(text: &str) -> String {
    let len = link_title_len(text).unwrap_or(0);
    if len < 2 {
        return String::new();
    }
    text[1..len - 1].to_string()
}

fn with_link_definitions(html: &str, defs: &[LinkDefRef]) -> String {
    if defs.is_empty() || html.contains("class=\"link-definition\"") {
        return html.to_string();
    }
    let extra: String = defs
        .iter()
        .map(|def| {
            let shown = if def.title.is_empty() {
                def.href.as_str()
            } else {
                def.title.as_str()
            };
            let title = if def.title.is_empty() {
                String::new()
            } else {
                format!(" title=\"{}\"", escape_html(&def.title))
            };
            format!(
                "<div class=\"link-definition\"><a href=\"{}\"{}>[{}] {}</a></div>",
                escape_html(&def.href),
                title,
                escape_html(&def.label),
                escape_html(shown),
            )
        })
        .collect();
    match footnote_definition_start(html) {
        Some(at) => format!("{}{}{}", &html[..at], extra, &html[at..]),
        None => format!("{html}{extra}"),
    }
}

/// 预览会在开标签里插 `data-md-*`，不能只认 `"<div class=\"footnote-definition\""`。
fn footnote_definition_start(html: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(rel) = html[from..].find("<div") {
        let at = from + rel;
        let rest = &html[at..];
        let gt = rest.find('>')?;
        let tag = &rest[..=gt];
        if tag.contains("class=\"footnote-definition\"") {
            return Some(at);
        }
        from = at + 4;
    }
    None
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn find_link_def_start(text: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(rel) = text[from..].find('[') {
        let at = from + rel;
        if text[at..].starts_with("[^") {
            from = at + 1;
            continue;
        }
        if take_link_def(&text[at..]).is_some() {
            return Some(at);
        }
        from = at + 1;
    }
    None
}

fn sanitize_rendered(raw: &str, preview: bool) -> String {
    // 消毒器只给 id 加前缀，不改指向它的 `#` 链接；这里先把站内锚点补上同一个前缀。
    // 正文里唯一能有 id 的就是脚注定义，所以所有 `#` 链接都按脚注处理。
    let raw = raw.replace("href=\"#", &format!("href=\"#{FOOTNOTE_ID_PREFIX}"));
    // 表格列对齐：`style="text-align: center"` 过不了消毒，换成 `align="center"`。
    let raw = raw
        .replace("style=\"text-align: left\"", "align=\"left\"")
        .replace("style=\"text-align: center\"", "align=\"center\"")
        .replace("style=\"text-align: right\"", "align=\"right\"");

    let mut attributes = allowed_attributes();
    if preview {
        for tag in BLOCK_TAGS {
            attributes
                .entry(tag)
                .or_default()
                .extend(["data-md-start", "data-md-end"]);
        }
    }

    ammonia::Builder::default()
        .tags(allowed_tags())
        .tag_attributes(attributes)
        .allowed_classes(allowed_classes())
        .id_prefix(Some(FOOTNOTE_ID_PREFIX))
        // 站外链接一律新窗口打开并断开 referrer / opener
        .link_rel(Some("noopener noreferrer nofollow"))
        // 只认这几种协议：javascript: / data: 一律拦掉
        .url_schemes(["http", "https", "mailto"].into_iter().collect())
        .clean(&raw)
        .to_string()
}

/// 每个顶层块在原文里的字节区间，按出现顺序。
fn top_level_block_ranges(markdown: &str) -> Vec<(usize, usize)> {
    use pulldown_cmark::Event;
    let mut ranges = Vec::new();
    let mut depth = 0usize;
    for (event, range) in Parser::new_ext(markdown, markdown_options()).into_offset_iter() {
        match event {
            Event::Start(tag) if is_block_tag(&tag) => {
                // 消毒会剥掉的 HtmlBlock（iframe 等）不能占一格，否则后文抢到它的区间。
                // 留下来的块级 HTML（<hr>、<p>）仍要占一格，否则它会偷走后文的区间。
                if depth == 0 {
                    let keep = match tag {
                        pulldown_cmark::Tag::HtmlBlock => {
                            html_block_survives_as_block(&markdown[range.start..range.end])
                        }
                        _ => true,
                    };
                    if keep {
                        ranges.push((range.start, range.end));
                    }
                }
                depth += 1;
            }
            Event::End(end) if is_block_tag_end(&end) => {
                depth = depth.saturating_sub(1);
            }
            Event::Rule if depth == 0 => ranges.push((range.start, range.end)),
            _ => {}
        }
    }
    ranges
}

fn is_block_tag(tag: &pulldown_cmark::Tag<'_>) -> bool {
    use pulldown_cmark::Tag;
    matches!(
        tag,
        Tag::Paragraph
            | Tag::Heading { .. }
            | Tag::BlockQuote(_)
            | Tag::CodeBlock(_)
            | Tag::List(_)
            | Tag::Table(_)
            | Tag::FootnoteDefinition(_)
            | Tag::HtmlBlock
    )
}

fn is_block_tag_end(end: &pulldown_cmark::TagEnd) -> bool {
    use pulldown_cmark::TagEnd;
    matches!(
        end,
        TagEnd::Paragraph
            | TagEnd::Heading(_)
            | TagEnd::BlockQuote(_)
            | TagEnd::CodeBlock
            | TagEnd::List(_)
            | TagEnd::Table
            | TagEnd::FootnoteDefinition
            | TagEnd::HtmlBlock
    )
}

/// 预扫消毒：不加脚注 id 前缀，也不改写 `href="#"`，完整消毒仍只在
/// `sanitize_rendered` 做一次。必须放行 `data-md-*`，否则小组件上
/// `stamp_wrapper` 打的区间会被剥掉。
fn preview_pre_clean(html: &str) -> String {
    let mut attributes = allowed_attributes();
    for tag in BLOCK_TAGS {
        attributes
            .entry(tag)
            .or_default()
            .extend(["data-md-start", "data-md-end"]);
    }
    ammonia::Builder::default()
        .tags(allowed_tags())
        .tag_attributes(attributes)
        .allowed_classes(allowed_classes())
        .link_rel(Some("noopener noreferrer nofollow"))
        .url_schemes(["http", "https", "mailto"].into_iter().collect())
        .clean(html)
        .to_string()
}

fn html_block_survives_as_block(src: &str) -> bool {
    html_has_top_level_block(&preview_pre_clean(src))
}

fn html_has_top_level_block(html: &str) -> bool {
    let mut depth = 0usize;
    let mut rest = html;
    while let Some(lt) = rest.find('<') {
        let tag_slice = &rest[lt..];
        let Some(gt) = tag_slice.find('>') else {
            return false;
        };
        let tag = &tag_slice[..=gt];
        let inner = tag[1..tag.len() - 1].trim_end_matches('/').trim();
        let closing = inner.starts_with('/');
        let name = inner
            .trim_start_matches('/')
            .split(|c: char| c.is_whitespace())
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        let is_block = BLOCK_TAGS.contains(&name.as_str());
        let self_closing = name == "hr";
        if is_block && !closing && depth == 0 {
            return true;
        }
        if is_block && !self_closing {
            if closing {
                depth = depth.saturating_sub(1);
            } else {
                depth += 1;
            }
        }
        rest = &tag_slice[gt + 1..];
    }
    false
}

/// 先丢掉消毒器会剥掉的块，再打区间。被剥掉的 iframe 不能占一格，
/// 否则后面的段落会拿到它的原文范围，点预览即编辑就跳错。
fn stamp_preview_html(html: &str, ranges: &[(usize, usize)]) -> String {
    stamp_block_ranges(&preview_pre_clean(html), ranges)
}

/// 扫一遍渲染出的 HTML，给第 i 个顶层块的开标签插上第 i 个区间。
/// 深度只数块级标签；`li` / `tr` 这些不算，所以列表项里的 `<p>` 不会被当成顶层。
fn stamp_block_ranges(html: &str, ranges: &[(usize, usize)]) -> String {
    let mut out = String::with_capacity(html.len() + ranges.len() * 48);
    let mut depth = 0usize;
    let mut next = 0usize;
    let mut rest = html;
    while let Some(lt) = rest.find('<') {
        out.push_str(&rest[..lt]);
        let tag_slice = &rest[lt..];
        let Some(gt) = tag_slice.find('>') else {
            out.push_str(tag_slice);
            return out;
        };
        let tag = &tag_slice[..=gt];
        let inner = tag[1..tag.len() - 1].trim_end_matches('/').trim();
        let closing = inner.starts_with('/');
        let name = inner
            .trim_start_matches('/')
            .split(|c: char| c.is_whitespace())
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        let is_block = BLOCK_TAGS.contains(&name.as_str());
        let self_closing = name == "hr";
        if is_block && !closing && depth == 0 {
            if tag.contains("data-md-start=") {
                out.push_str(tag);
            } else if let Some((start, end)) = ranges.get(next) {
                let insert_at = 1 + name.len();
                out.push_str(&tag[..insert_at]);
                out.push_str(&format!(" data-md-start=\"{start}\" data-md-end=\"{end}\""));
                out.push_str(&tag[insert_at..]);
                next += 1;
            } else {
                out.push_str(tag);
            }
        } else {
            out.push_str(tag);
        }
        if is_block && !self_closing {
            if closing {
                depth = depth.saturating_sub(1);
            } else {
                depth += 1;
            }
        }
        rest = &tag_slice[gt + 1..];
    }
    out.push_str(rest);
    out
}

/// 从 HTML 里剥出纯文本。只处理本模块产出的、已消毒的 HTML。
fn strip_html(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut last_was_space = true;
    for ch in html.chars() {
        match ch {
            '<' => {
                in_tag = true;
                // 标签边界当作一个空格，否则 `<p>甲</p><p>乙</p>` 会粘成「甲乙」
                if !last_was_space {
                    out.push(' ');
                    last_was_space = true;
                }
            }
            '>' => in_tag = false,
            _ if in_tag => {}
            c if c.is_whitespace() => {
                if !last_was_space {
                    out.push(' ');
                    last_was_space = true;
                }
            }
            c => {
                out.push(c);
                last_was_space = false;
            }
        }
    }
    decode_entities(out.trim())
}

/// 反转义消毒器写出的那几个实体。只有这五个 —— 白名单渲染不会产出别的。
fn decode_entities(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        // & 放最后，否则 `&amp;lt;` 会被解成 `<`
        .replace("&amp;", "&")
}

/// Collect image destinations from Markdown, including reference links and HTML `<img>`.
pub fn markdown_media_urls(markdown: &str) -> Vec<String> {
    let mut urls = Vec::new();
    for event in Parser::new_ext(markdown, markdown_options()) {
        match event {
            Event::Start(Tag::Image { dest_url, .. } | Tag::Link { dest_url, .. }) => {
                push_unique_url(&mut urls, &dest_url)
            }
            Event::Html(html) | Event::InlineHtml(html) => {
                // HTML events contain source syntax; normalize attributes with the same
                // HTML parser and sanitizer used by the renderer before extracting.
                collect_img_srcs(&sanitize_rendered(&html, false), &mut urls);
            }
            _ => {}
        }
    }
    urls
}

fn push_unique_url(urls: &mut Vec<String>, url: &str) {
    if url.is_empty() {
        return;
    }
    if !urls.iter().any(|existing| existing == url) {
        urls.push(url.to_string());
    }
}

fn collect_img_srcs(html: &str, urls: &mut Vec<String>) {
    let mut i = 0;
    while let Some(found) = html[i..].find("<img ") {
        let tag_start = i + found;
        let Some(rel_end) = html[tag_start..].find('>') else {
            break;
        };
        let tag_end = tag_start + rel_end;
        let tag = &html[tag_start..tag_end];
        if let Some(src_at) = tag.find("src=\"") {
            let value_start = src_at + 5;
            if let Some(value_len) = tag[value_start..].find('"') {
                push_unique_url(
                    urls,
                    &decode_entities(&tag[value_start..value_start + value_len]),
                );
            }
        }
        i = tag_end + 1;
        if i >= html.len() {
            break;
        }
    }
}

/// 取正文里第一张图的地址当封面。
///
/// 在已消毒的 HTML 上扫 `<img src="...">`：消毒已经保证了协议合法、引号规整，
/// 这里不需要再做一次 URL 校验。
fn first_image(html: &str) -> Option<String> {
    let mut urls = Vec::new();
    collect_img_srcs(html, &mut urls);
    urls.into_iter().next()
}

/// 字数。CJK 按字算，拉丁按空白分词算 —— 中英混排不区分语言就没法给出
/// 一个两边都不离谱的数。
fn count_words(text: &str) -> i32 {
    let mut cjk = 0usize;
    let mut latin_runs = 0usize;
    let mut in_latin_run = false;
    for ch in text.chars() {
        if is_cjk(ch) {
            cjk += 1;
            in_latin_run = false;
        } else if ch.is_alphanumeric() {
            if !in_latin_run {
                latin_runs += 1;
                in_latin_run = true;
            }
        } else {
            in_latin_run = false;
        }
    }
    i32::try_from(cjk + latin_runs).unwrap_or(i32::MAX)
}

fn is_cjk(ch: char) -> bool {
    matches!(ch as u32,
        0x3040..=0x30FF   // 平假名 / 片假名
        | 0x3400..=0x4DBF // 扩展 A
        | 0x4E00..=0x9FFF // 基本区
        | 0xF900..=0xFAFF // 兼容表意
        | 0xAC00..=0xD7AF // 谚文
    )
}

/// 渲染一篇笔记并算出全部派生字段。调用前先过 [`validate_note`]。
pub fn render_note(title: &str, markdown: &str) -> RenderedNote {
    let html = render_markdown(markdown);
    let plain = strip_html(&html);
    let word_count = count_words(&plain);
    let summary = if plain.is_empty() {
        None
    } else {
        Some(truncate_chars(&plain, SUMMARY_CHARS))
    };
    RenderedNote {
        title: title.trim().to_string(),
        image: first_image(&html),
        summary,
        // 一篇字数为 0 的笔记（只有一张图）也要显示 1 分钟，不显示 0
        reading_time: (word_count / WORDS_PER_MINUTE).max(1),
        word_count,
        html,
    }
}

/// 按**字符**截断，不是按字节 —— 按字节切会把一个汉字劈成两半。
fn truncate_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    text.chars().take(max).collect()
}

/// 笔记在 `phantasi_items.guid` 里的取值。笔记没有上游 feed，guid 由平台自己生成。
pub fn note_guid(uuid: &str) -> String {
    format!("note:{uuid}")
}

/// 公开 slug 的长度上限。和博客地址、库里的部分唯一索引同一口径。
pub const SLUG_MAX_CHARS: usize = myriad_phantasi::SLUG_MAX_LEN;

/// 标题里能进 slug 的 ASCII。非 ASCII（例如纯中文标题）被丢掉，结果可能为空。
pub fn slugify_title(title: &str) -> String {
    myriad_phantasi::slugify_title(title)
}

/// 标题 ASCII 化之后为空时的 slug。`item_id` 在笔记源内唯一，所以这个兜底本身不撞。
pub fn fallback_slug(item_id: i32) -> String {
    myriad_phantasi::fallback_slug(item_id)
}

/// `base` 已被占用时追加 `-2`、`-3`。结果仍满足 [`validate_slug`]。
///
/// 编号规则在 [`myriad_phantasi::dedupe_slug`]。非法词干不当地址，从 `n-2` 起。
pub fn dedupe_slug(base: &str, taken: &HashSet<String>) -> String {
    if validate_slug(base).is_ok() {
        myriad_phantasi::dedupe_slug(base, |slug| taken.contains(slug))
    } else {
        myriad_phantasi::dedupe_slug("n", |slug| slug == "n" || taken.contains(slug))
    }
}

/// 发布时生成 slug：标题 ASCII 化，空则 `n{item_id}`，和 `taken` 冲突再编号。
pub fn allocate_note_slug(title: &str, item_id: i32, taken: &HashSet<String>) -> String {
    let ascii = slugify_title(title);
    let base = if ascii.is_empty() {
        fallback_slug(item_id)
    } else {
        ascii
    };
    dedupe_slug(&base, taken)
}

/// 站长手改的 slug。小写、`[a-z0-9-]`、不能以 `-` 开头或结尾、不能连续 `-`、长度 1..=80。
#[allow(clippy::result_unit_err)] // 调用方只看 is_ok/is_err，没有可报的细节。
pub fn validate_slug(slug: &str) -> Result<(), ()> {
    if myriad_phantasi::is_note_slug(slug) {
        Ok(())
    } else {
        Err(())
    }
}

/// 旧手帐公开地址：`/journal` 或 `/journal/...`。启动修复把这些 link 改成博客地址。
pub fn is_legacy_journal_article_link(link: &str) -> bool {
    let link = link.trim();
    link == "/journal" || link.starts_with("/journal/")
}

/// 一篇已发布笔记在修复前的样子。
#[derive(Clone, Debug)]
pub struct NoteSlugInput {
    pub id: i32,
    pub source_id: i32,
    pub title: String,
    pub slug: Option<String>,
    pub link: String,
}

/// 需要写回的 slug 和公开地址。已经符合当前规则的行不会出现在这里。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoteSlugRepair {
    pub id: i32,
    pub slug: String,
    pub link: String,
}

/// 给已发布笔记补 slug，并把旧 `/journal/articles/{id}` 改成博客地址。
///
/// 同一 `source_id` 内唯一。已经合法且不冲突的 slug 保持不变。重复调用不再产生修复。
pub fn plan_note_slug_repairs(mut rows: Vec<NoteSlugInput>) -> Vec<NoteSlugRepair> {
    rows.sort_by_key(|row| (row.source_id, row.id));
    let mut taken: HashMap<i32, HashSet<String>> = HashMap::new();
    let mut repairs = Vec::new();
    for row in rows {
        let set = taken.entry(row.source_id).or_default();
        let previous = row.slug.clone();
        let kept = previous
            .as_deref()
            .filter(|slug| validate_slug(slug).is_ok() && set.insert((*slug).to_string()));
        let slug = if let Some(slug) = kept {
            slug.to_string()
        } else {
            let allocated = allocate_note_slug(&row.title, row.id, set);
            set.insert(allocated.clone());
            allocated
        };
        let slug_changed = previous.as_deref() != Some(slug.as_str());
        let link = if is_legacy_journal_article_link(&row.link)
            || row.link.trim().is_empty()
            || slug_changed
        {
            note_link(&slug)
        } else {
            row.link.clone()
        };
        if slug_changed || link != row.link {
            repairs.push(NoteSlugRepair {
                id: row.id,
                slug,
                link,
            });
        }
    }
    repairs
}

/// 笔记的公开地址：`/blog/posts/{slug}/`。
pub fn note_link(slug: &str) -> String {
    myriad_phantasi::item_path(slug)
}

/// 已发布笔记的公开 RSS。代理和前端分享入口必须用同一条路径。
pub const NOTES_RSS_PATH: &str = myriad_phantasi::NOTES_RSS_PATH;

/// `configurations` 里笔记 RSS 开关的 key。缺省或无法解析 = 关。
pub const NOTES_RSS_PREFERENCES_KEY: &str = myriad_phantasi::NOTES_RSS_PREFERENCES_KEY;

/// 解析站长开关。认 JSON bool、`"true"`、`{"enabled": ...}`。
pub fn notes_rss_enabled_from_value(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Bool(enabled) => *enabled,
        serde_json::Value::String(text) => text.eq_ignore_ascii_case("true"),
        serde_json::Value::Object(map) => map
            .get("enabled")
            .map(notes_rss_enabled_from_value)
            .unwrap_or(false),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn renders_basic_markdown() {
        let html = render_markdown("# 标题\n\n正文**加粗**。");
        assert!(html.contains("<h1>标题</h1>"));
        assert!(html.contains("<strong>加粗</strong>"));
    }

    #[test]
    fn renders_inline_and_display_math_with_tex_attr() {
        let html = render_markdown("行内 $E=mc^2$ 和独立的\n\n$$\n\\frac{1}{2}\n$$");
        assert!(
            html.contains("<span class=\"math math-inline\" data-tex=\"E=mc^2\">E=mc^2</span>"),
            "{html}"
        );
        assert!(html.contains("class=\"math math-display\""), "{html}");
        assert!(
            html.contains(
                "<span class=\"math math-display\" data-tex=\"\\frac{1}{2}\">\\frac{1}{2}</span>"
            ),
            "{html}"
        );
    }

    #[test]
    fn math_escapes_html_in_tex() {
        let html = render_markdown("比 $a<b$ 和 $x\"y$");
        assert!(html.contains("data-tex=\"a&lt;b\""), "{html}");
        assert!(html.contains("data-tex=\"x&quot;y\""), "{html}");
        assert!(!html.contains("<b>"), "{html}");
    }

    #[test]
    fn math_inside_code_stays_code() {
        let html = render_markdown("`` $E=mc^2$ ``\n\n```\n$E=mc^2$\n```");
        assert!(!html.contains("class=\"math"), "{html}");
        assert!(html.contains("<code>$E=mc^2$</code>"), "{html}");
        assert!(
            html.contains("<pre><code>$E=mc^2$\n</code></pre>") || html.contains("$E=mc^2$"),
            "{html}"
        );
    }

    #[test]
    fn preview_keeps_math_and_source_ranges() {
        let md = "见 $x^2$\n";
        let preview = render_markdown_preview(md);
        let published = render_markdown(md);
        assert!(preview.contains("data-md-start="), "{preview}");
        assert!(preview.contains("class=\"math math-inline\""), "{preview}");
        assert!(preview.contains("data-tex=\"x^2\""), "{preview}");
        assert_eq!(strip_preview_ranges(&preview), published);
    }

    #[test]
    fn strips_script_tags() {
        let html = render_markdown("<script>alert(1)</script>\n\n正文");
        assert!(!html.contains("script"));
        assert!(html.contains("正文"));
    }

    #[test]
    fn publish_html_drops_tapp_iframe_and_secret_query() {
        let html = render_markdown(
            "<iframe src=\"https://tapp.example/sandbox?token=HOST_SECRET\"></iframe>\n\n:::widget weather 2x2",
        );
        assert!(!html.contains("iframe"), "{html}");
        assert!(!html.contains("HOST_SECRET"), "{html}");
        assert!(html.contains("data-widget=\"weather\""), "{html}");
    }

    #[test]
    fn strips_event_handlers() {
        let html = render_markdown("<img src=\"https://a/b.png\" onerror=\"alert(1)\">");
        assert!(!html.contains("onerror"));
    }

    #[test]
    fn rejects_javascript_urls() {
        let html = render_markdown("[点我](javascript:alert(1))");
        assert!(!html.contains("javascript:"));
    }

    #[test]
    fn rejects_data_urls_in_images() {
        let html = render_markdown("![x](data:text/html;base64,PHNjcmlwdD4=)");
        assert!(!html.contains("data:"));
    }

    #[test]
    fn keeps_https_links_and_adds_rel() {
        let html = render_markdown("[站](https://example.com)");
        assert!(html.contains("https://example.com"));
        assert!(html.contains("noopener"));
    }

    #[test]
    fn renders_tables_and_strikethrough() {
        let html = render_markdown("| a | b |\n| - | - |\n| 1 | 2 |\n\n~~删~~");
        assert!(html.contains("<table>"));
        assert!(html.contains("<del>删</del>"));
    }

    #[test]
    fn preview_stamps_top_level_blocks_with_source_ranges() {
        let md = "# 标题\n\n第一段 **粗**。\n\n- 甲\n- 乙\n\n---\n\n```rust\nfn a() {}\n```";
        let html = render_markdown_preview(md);
        // 标题 0..9（"# 标题\n" 是 9 字节）
        assert!(
            html.contains("<h1 data-md-start=\"0\" data-md-end=\"9\">"),
            "{html}"
        );
        assert!(html.contains("<p data-md-start=\"10\""), "{html}");
        assert!(html.contains("<ul data-md-start="), "{html}");
        assert!(html.contains("<hr data-md-start="), "{html}");
        assert!(html.contains("<pre data-md-start="), "{html}");
        // 列表项里的 <li> 不带；顶层块各带一次
        assert_eq!(html.matches("data-md-start=").count(), 5, "{html}");
        assert!(!html.contains("<li data-md"), "{html}");
        // 发布用的那份一个都不带
        assert!(!render_markdown(md).contains("data-md-"));
    }

    #[test]
    fn preview_ranges_slice_back_to_the_source() {
        let md = "前言\n\n> 引用\n> 两行\n\n[^1]: 底\n\n注[^1]";
        let html = render_markdown_preview(md);
        let mut seen = 0;
        for cap in html.split("data-md-start=\"").skip(1) {
            let start: usize = cap.split('"').next().unwrap().parse().unwrap();
            let end: usize = cap
                .split("data-md-end=\"")
                .nth(1)
                .unwrap()
                .split('"')
                .next()
                .unwrap()
                .parse()
                .unwrap();
            assert!(
                md.is_char_boundary(start) && md.is_char_boundary(end),
                "{start}..{end}"
            );
            assert!(start < end && end <= md.len());
            seen += 1;
        }
        assert!(seen >= 3, "{html}");
        // "前言\n\n" 是 8 字节，引用块从这里开始
        assert!(html.contains("<blockquote data-md-start=\"8\""), "{html}");
    }

    #[test]
    fn jammed_reference_and_footnote_defs_parse() {
        let md = concat!(
            "看 [规范][cm] 和 [gfm]。\n\n",
            "[cm]: https://spec.commonmark.org/0.31.2/ \"CommonMark Spec 0.31.2\" ",
            "[gfm]: https://github.github.com/gfm/ \"GitHub Flavored Markdown Spec\"\n\n",
            "[^didion]: 一 [^swartz]: 二",
        );
        let html = render_markdown(md);
        assert!(
            html.contains("href=\"https://spec.commonmark.org/0.31.2/\""),
            "{html}"
        );
        assert!(
            html.contains("href=\"https://github.github.com/gfm/\""),
            "{html}"
        );
        assert!(html.contains("class=\"link-definition\""), "{html}");
        assert!(html.contains("[cm] CommonMark Spec 0.31.2"), "{html}");
        assert!(html.contains("footnote-definition"), "{html}");
        assert!(html.contains(">一<"), "{html}");
        assert!(html.contains(">二<"), "{html}");
        assert!(!html.contains("[cm]:"), "{html}");
        assert!(!html.contains("[^didion]:"), "{html}");
    }

    #[test]
    fn widget_does_not_break_reference_and_footnotes() {
        let md = concat!(
            "看 [规范][cm] 和脚注[^didion]。\n\n",
            ":::widget weather 2x2\n\n",
            "![杯][cup]\n\n",
            "[cm]: https://spec.commonmark.org/0.31.2/ \"CommonMark Spec 0.31.2\"\n",
            "[cup]: https://example.com/cup.jpg \"Cup\"\n",
            "[^didion]: 为了看见自己的想法。\n",
        );
        let html = render_markdown(md);
        assert!(
            html.contains("href=\"https://spec.commonmark.org/0.31.2/\""),
            "{html}"
        );
        assert!(html.contains("class=\"footnote-reference\""), "{html}");
        assert!(html.contains("class=\"footnote-definition\""), "{html}");
        assert!(
            html.contains("src=\"https://example.com/cup.jpg\""),
            "{html}"
        );
        assert!(html.contains("data-widget=\"weather\""), "{html}");
        assert!(!html.contains("[^didion]"), "{html}");
        assert!(!html.contains("[规范][cm]"), "{html}");
        assert!(!html.contains("![杯][cup]"), "{html}");
    }

    #[test]
    fn tilde_fence_keeps_trailing_defs() {
        let md = concat!("前[^fn]\n\n", "~~~\n", "围栏里\n", "~~~\n\n", "[^fn]: 底\n",);
        let html = render_markdown(md);
        assert!(html.contains("class=\"footnote-definition\""), "{html}");
        assert!(html.contains("围栏里"), "{html}");
        assert!(html.contains("footnote-reference"), "{html}");
        assert!(!html.contains("[^fn]"), "{html}");
    }

    #[test]
    fn unclosed_tilde_does_not_swallow_trailing_defs() {
        let md = concat!(
            "看 [规范][cm] 和脚注[^didion]。\n\n",
            "~~~第一杯：16g\n",
            "缩进四个空格是更老的代码块。\n",
            "## 收尾时只做清单上的事\n\n",
            "[cm]: https://spec.commonmark.org/0.31.2/ \"CommonMark Spec 0.31.2\"\n",
            "[^didion]: 为了看见自己的想法。\n",
        );
        let html = render_markdown(md);
        assert!(html.contains("class=\"footnote-definition\""), "{html}");
        assert!(
            html.contains("href=\"https://spec.commonmark.org/0.31.2/\""),
            "{html}"
        );
        assert!(html.contains("缩进四个空格是更老的代码块。"), "{html}");
        assert!(!html.contains("[cm]:"), "{html}");
        assert!(!html.contains("[^didion]:"), "{html}");
    }

    #[test]
    fn fence_keeps_jammed_defs_as_code() {
        let html = render_markdown("```\n[cm]: https://example.com/ [gfm]: https://x/\n```");
        assert!(
            html.contains("[cm]: https://example.com/ [gfm]: https://x/"),
            "{html}"
        );
        assert!(!html.contains("href=\"https://example.com/\""), "{html}");
    }

    #[test]
    fn footnotes_keep_anchor_and_classes() {
        let html = render_markdown("注[^1]\n\n[^1]: 底");
        assert!(html.contains("class=\"footnote-reference\""), "{html}");
        assert!(html.contains("class=\"footnote-definition\""), "{html}");
        assert!(html.contains("id=\"note-fn-1\""), "{html}");
        assert!(html.contains("href=\"#note-fn-1\""), "{html}");
        assert!(html.contains("footnote-definition-label"), "{html}");
    }

    #[test]
    fn task_list_checkbox_survives() {
        let html = render_markdown("- [x] 做完\n- [ ] 还没");
        assert!(html.contains("type=\"checkbox\""), "{html}");
        assert!(html.contains("checked"), "{html}");
        assert!(html.contains("disabled"), "{html}");
    }

    #[test]
    fn browser_roundtrip_corpus_matches_preview_and_published_rendering() {
        let source = include_str!("../testdata/note-roundtrip.md");
        let edited = include_str!("../testdata/note-roundtrip-edited.md");
        assert_eq!(
            render_markdown(source),
            include_str!("../testdata/note-roundtrip.html")
        );
        assert_eq!(
            render_markdown(edited),
            include_str!("../testdata/note-roundtrip-edited.html")
        );
        assert_eq!(
            render_markdown_preview(edited),
            include_str!("../testdata/note-roundtrip-edited-preview.html")
        );
    }

    #[test]
    fn table_cell_break_stays_in_value_column() {
        let md = "| PARAM | VALUE |\n| --- | --- |\n| model | `deepseek-flash` (1)<br>`deepseek-v4-pro` (2) |";
        for html in [render_markdown(md), render_markdown_preview(md)] {
            assert!(
                html.contains(
                    "<td><code>deepseek-flash</code> (1)<br><code>deepseek-v4-pro</code> (2)</td>"
                ),
                "{html}"
            );
            assert_eq!(html.matches("<td>").count(), 2, "{html}");
        }
    }

    #[test]
    fn ordered_list_start_survives_preview_and_published_sanitization() {
        for html in [
            render_markdown("3. three\n4. four"),
            render_markdown_preview("3. three\n4. four"),
        ] {
            assert!(html.contains("start=\"3\""), "{html}");
        }
    }

    #[test]
    fn table_alignment_becomes_align_attribute() {
        let html = render_markdown("| a | b | c |\n| :-- | :-: | --: |\n| 1 | 2 | 3 |");
        assert!(html.contains("<th align=\"left\">"), "{html}");
        assert!(html.contains("<th align=\"center\">"), "{html}");
        assert!(html.contains("<td align=\"right\">"), "{html}");
        assert!(!html.contains("style="), "{html}");
    }

    #[test]
    fn nested_lists_render() {
        let html = render_markdown("- 甲\n  - 甲一\n- 乙");
        assert!(html.contains("<li>甲\n<ul>"), "{html}");
    }

    #[test]
    fn keeps_code_language_class() {
        let html = render_markdown("```rust\nfn main() {}\n```");
        assert!(html.contains("language-rust"));
    }

    #[test]
    fn first_image_becomes_the_cover() {
        let note = render_note(
            "标题",
            "![封面](https://example.com/a.png)\n\n![二](https://example.com/b.png)",
        );
        assert_eq!(note.image.as_deref(), Some("https://example.com/a.png"));
    }

    #[test]
    fn no_image_means_no_cover() {
        let note = render_note("标题", "只有文字");
        assert_eq!(note.image, None);
    }

    #[test]
    fn summary_is_plain_text_across_blocks() {
        let note = render_note("标题", "第一段\n\n第二段");
        // 段落之间要有分隔，不能粘成「第一段第二段」
        assert_eq!(note.summary.as_deref(), Some("第一段 第二段"));
    }

    #[test]
    fn summary_truncates_by_char_not_byte() {
        let body = "字".repeat(SUMMARY_CHARS + 50);
        let note = render_note("标题", &body);
        assert_eq!(note.summary.unwrap().chars().count(), SUMMARY_CHARS);
    }

    #[test]
    fn empty_body_has_no_summary() {
        let note = render_note("标题", "");
        assert_eq!(note.summary, None);
        assert_eq!(note.word_count, 0);
        assert_eq!(note.reading_time, 1);
    }

    #[test]
    fn counts_cjk_by_char_and_latin_by_word() {
        let note = render_note("标题", "中文三字 hello world");
        assert_eq!(note.word_count, 6);
    }

    #[test]
    fn reading_time_is_at_least_one_minute() {
        let note = render_note("标题", "短");
        assert_eq!(note.reading_time, 1);
    }

    #[test]
    fn title_must_not_be_blank() {
        assert_eq!(validate_note("   ", "正文"), Err(NoteError::EmptyTitle));
    }

    #[test]
    fn title_length_is_capped_by_chars() {
        let title = "字".repeat(MAX_TITLE_CHARS + 1);
        assert_eq!(
            validate_note(&title, ""),
            Err(NoteError::TitleTooLong {
                chars: MAX_TITLE_CHARS + 1
            })
        );
    }

    #[test]
    fn body_length_is_capped() {
        let body = "字".repeat(MAX_NOTE_CHARS + 1);
        assert_eq!(
            validate_note("标题", &body),
            Err(NoteError::BodyTooLong {
                chars: MAX_NOTE_CHARS + 1
            })
        );
    }

    #[test]
    fn a_valid_note_passes() {
        assert_eq!(validate_note(" 标题 ", "正文"), Ok(()));
    }

    #[test]
    fn validation_messages_are_english() {
        assert_eq!(NoteError::EmptyTitle.message(), "A title is required");
        assert_eq!(
            NoteError::TitleTooLong { chars: 201 }.message(),
            "Titles can be at most 200 characters (this one is 201)"
        );
        assert_eq!(
            NoteError::BodyTooLong { chars: 200_001 }.message(),
            "Notes can be at most 200000 characters (this one is 200001)"
        );
    }

    #[test]
    fn title_is_trimmed_in_the_render_result() {
        let note = render_note("  标题  ", "正文");
        assert_eq!(note.title, "标题");
    }

    #[test]
    fn guid_and_link_shapes() {
        assert_eq!(note_guid("abc"), "note:abc");
        assert_eq!(note_link("hello-world"), "/blog/posts/hello-world/");
        assert_eq!(NOTES_RSS_PATH, "/api/phantasi/notes.xml");
        assert_eq!(NOTES_RSS_PREFERENCES_KEY, "phantasi_notes_rss");
    }

    #[test]
    fn notes_rss_defaults_off_and_parses_common_shapes() {
        assert!(!notes_rss_enabled_from_value(&serde_json::json!(null)));
        assert!(!notes_rss_enabled_from_value(&serde_json::json!(false)));
        assert!(!notes_rss_enabled_from_value(&serde_json::json!("false")));
        assert!(!notes_rss_enabled_from_value(
            &serde_json::json!({"enabled": false})
        ));
        assert!(notes_rss_enabled_from_value(&serde_json::json!(true)));
        assert!(notes_rss_enabled_from_value(&serde_json::json!("true")));
        assert!(notes_rss_enabled_from_value(
            &serde_json::json!({"enabled": true})
        ));
    }

    #[test]
    fn entities_survive_a_round_trip() {
        let note = render_note("标题", "a & b < c");
        assert_eq!(note.summary.as_deref(), Some("a & b < c"));
    }

    #[test]
    fn renders_columns_and_keeps_inner_markdown() {
        let html = render_markdown(":::columns\n左 **粗**\n:::col\n右\n:::");
        assert!(html.contains("class=\"note-columns\""), "{html}");
        assert!(html.contains("class=\"note-column\""), "{html}");
        assert!(html.contains("<strong>粗</strong>"), "{html}");
        assert!(html.contains("右"), "{html}");
    }

    #[test]
    fn renders_widget_placeholder_and_strips_unknown_attrs() {
        assert_eq!(
            render_markdown(":::WIDGET weather 2x2"),
            render_markdown(":::widget weather 2x2")
        );
        let html = render_markdown(":::widget weather 2x2");
        assert!(html.contains("class=\"note-widget not-prose\""), "{html}");
        assert!(html.contains("data-widget=\"weather\""), "{html}");
        assert!(html.contains("data-size=\"2x2\""), "{html}");
        assert!(html.contains("></div>"), "{html}");
        assert!(!html.contains(">weather<"), "{html}");

        let configured = render_markdown(r#":::widget weather 2x2 {"city":"Tokyo"}"#);
        assert!(
            configured.contains("data-config=\"%7B%22city%22%3A%22Tokyo%22%7D\""),
            "{configured}"
        );

        let dirty = render_markdown(
            "<div class=\"note-widget\" data-widget=\"weather\" data-size=\"2x2\" data-config=\"%7B%22city%22%3A%22Tokyo%22%7D\" onclick=\"alert(1)\"><iframe src=\"https://evil\"></iframe></div>",
        );
        assert!(!dirty.contains("onclick"), "{dirty}");
        assert!(!dirty.contains("iframe"), "{dirty}");
        assert!(dirty.contains("class=\"note-widget\""), "{dirty}");
        assert!(dirty.contains("data-config="), "{dirty}");
    }

    #[test]
    fn preview_stamps_layout_wrappers() {
        let md = "上\n\n:::widget quote 2x2\n\n下";
        let html = render_markdown_preview(md);
        assert!(html.contains("class=\"note-widget not-prose\""), "{html}");
        assert!(html.contains("data-md-start="), "{html}");
        assert!(!render_markdown(md).contains("data-md-"));
    }

    fn strip_preview_ranges(html: &str) -> String {
        let mut out = String::with_capacity(html.len());
        let mut rest = html;
        while let Some(at) = rest.find(" data-md-") {
            out.push_str(&rest[..at]);
            let after = &rest[at + 1..];
            let end = after.find('"').and_then(|first| {
                after[first + 1..]
                    .find('"')
                    .map(|second| first + 1 + second + 1)
            });
            match end {
                Some(end) => rest = &after[end..],
                None => {
                    out.push_str(&rest[at..]);
                    return out;
                }
            }
        }
        out.push_str(rest);
        out
    }

    #[test]
    fn preview_html_matches_publish_except_source_ranges() {
        let md = "# 标题\n\n第一段 **粗**。\n\n:::widget weather 2x2 {\"city\":\"Tokyo\"}\n\n- [x] 完\n- [ ] 待\n\n```rust\nfn a() {}\n```";
        let published = render_markdown(md);
        let preview = render_markdown_preview(md);
        assert_ne!(preview, published);
        assert!(preview.contains("data-md-start="), "{preview}");
        assert!(!published.contains("data-md-"), "{published}");
        assert!(!published.contains(">weather<"), "{published}");
        assert!(!preview.contains(">weather<"), "{preview}");
        assert_eq!(strip_preview_ranges(&preview), published);
    }

    #[test]
    fn preview_keeps_link_defs_before_footnotes_when_stamped() {
        let md = concat!(
            "看 [规范][cm] 和脚注[^1]。\n\n",
            ":::widget weather 2x2 {\"city\":\"Tokyo\"}\n\n",
            ":::widget friend-links 4x2\n\n",
            "<iframe src=\"https://tapp.example/sandbox?token=HOST_SECRET\"></iframe>\n\n",
            "[cm]: https://spec.commonmark.org/0.31.2/ \"CommonMark Spec 0.31.2\"\n",
            "[^1]: 底。\n",
        );
        let published = render_markdown(md);
        let preview = render_markdown_preview(md);
        assert!(
            published.contains("class=\"link-definition\""),
            "{published}"
        );
        assert!(preview.contains("class=\"link-definition\""), "{preview}");
        assert!(!published.contains("iframe"), "{published}");
        assert!(!published.contains("HOST_SECRET"), "{published}");
        assert!(!preview.contains("iframe"), "{preview}");
        assert!(!preview.contains("HOST_SECRET"), "{preview}");
        let link = published.find("class=\"link-definition\"").unwrap();
        let foot = published.find("class=\"footnote-definition\"").unwrap();
        assert!(link < foot, "{published}");
        assert_eq!(strip_preview_ranges(&preview), published);
        assert_eq!(
            published,
            include_str!("../testdata/link-defs.published.html")
        );
        assert_eq!(preview, include_str!("../testdata/link-defs.preview.html"));
    }

    #[test]
    fn preview_stamps_surviving_text_not_stripped_iframe() {
        let md = "前\n\n<iframe src=\"https://tapp.example/sandbox?token=HOST_SECRET\"></iframe>\n\n后\n";
        let preview = render_markdown_preview(md);
        assert!(!preview.contains("iframe"), "{preview}");
        assert!(!preview.contains("HOST_SECRET"), "{preview}");
        let (start, end) = preview_range_for(&preview, ">后</p>");
        assert_eq!(md[start..end].trim(), "后", "{preview}");
        let (start, end) = preview_range_for(&preview, ">前</p>");
        assert_eq!(md[start..end].trim(), "前", "{preview}");
    }

    #[test]
    fn preview_stamps_surviving_raw_html_block_not_next_paragraph() {
        let md = "vor\n\n<hr>\n\nnach\n";
        let preview = render_markdown_preview(md);
        let published = render_markdown(md);
        assert!(published.contains("<hr>"), "{published}");
        assert_eq!(strip_preview_ranges(&preview), published);
        let (start, end) = preview_range_for(&preview, ">nach</p>");
        assert_eq!(md[start..end].trim(), "nach", "{preview}");
        let (start, end) = preview_range_on_open_tag(&preview, "<hr");
        assert_eq!(md[start..end].trim(), "<hr>", "{preview}");

        let kept = "vor\n\n<p>kept</p>\n\nnach\n";
        let preview = render_markdown_preview(kept);
        let (start, end) = preview_range_for(&preview, ">nach</p>");
        assert_eq!(kept[start..end].trim(), "nach", "{preview}");
        let (start, end) = preview_range_for(&preview, ">kept</p>");
        assert_eq!(kept[start..end].trim(), "<p>kept</p>", "{preview}");
    }

    #[test]
    fn preview_friend_report_tapp_match_publish_and_drop_secret_iframe() {
        let md = concat!(
            "vor\n\n",
            ":::widget friend-links 4x2\n\n",
            ":::widget report-bilibili 4x2\n\n",
            ":::widget tapp-shortcut 1x1\n\n",
            "<iframe src=\"https://tapp.example/sandbox?token=HOST_SECRET\"></iframe>\n\n",
            "nach\n",
        );
        let published = render_markdown(md);
        let preview = render_markdown_preview(md);
        assert!(
            published.contains("data-widget=\"friend-links\""),
            "{published}"
        );
        assert!(
            published.contains("data-widget=\"report-bilibili\""),
            "{published}"
        );
        assert!(
            published.contains("data-widget=\"tapp-shortcut\""),
            "{published}"
        );
        assert!(!published.contains("iframe"), "{published}");
        assert!(!published.contains("HOST_SECRET"), "{published}");
        assert!(!preview.contains("iframe"), "{preview}");
        assert!(!preview.contains("HOST_SECRET"), "{preview}");
        assert!(!published.contains(">friend-links<"), "{published}");
        assert!(!published.contains(">report-bilibili<"), "{published}");
        assert!(!published.contains(">tapp-shortcut<"), "{published}");
        assert_eq!(strip_preview_ranges(&preview), published);
        assert_eq!(
            published,
            include_str!("../testdata/friend-report-tapp.published.html")
        );
        assert_eq!(
            preview,
            include_str!("../testdata/friend-report-tapp.preview.html")
        );
        let (start, end) = preview_range_for(&preview, ">nach</p>");
        assert_eq!(md[start..end].trim(), "nach", "{preview}");
    }

    fn preview_range_for(html: &str, needle: &str) -> (usize, usize) {
        let at = html.find(needle).expect(needle);
        let before = &html[..at];
        let start_at = before.rfind("data-md-start=\"").expect("start");
        let start: usize = before[start_at + 15..]
            .split('"')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        let end_at = before.rfind("data-md-end=\"").expect("end");
        let end: usize = before[end_at + 13..]
            .split('"')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        (start, end)
    }

    fn preview_range_on_open_tag(html: &str, tag_prefix: &str) -> (usize, usize) {
        let at = html.find(tag_prefix).expect(tag_prefix);
        let gt = html[at..].find('>').expect("gt") + at;
        let tag = &html[at..=gt];
        let start_at = tag.find("data-md-start=\"").expect("start");
        let start: usize = tag[start_at + 15..]
            .split('"')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        let end_at = tag.find("data-md-end=\"").expect("end");
        let end: usize = tag[end_at + 13..]
            .split('"')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        (start, end)
    }

    #[test]
    fn preview_columns_with_inner_widget_match_publish() {
        let md = concat!(
            ":::columns\n左 [^1]\n:::col\n:::widget quote 2x2\n:::\n\n",
            "[^1]: 底。\n",
        );
        let published = render_markdown(md);
        let preview = render_markdown_preview(md);
        assert!(published.contains("class=\"note-columns\""), "{published}");
        assert!(published.contains("data-widget=\"quote\""), "{published}");
        assert!(
            published.contains("class=\"footnote-definition\""),
            "{published}"
        );
        assert!(!published.contains("iframe"), "{published}");
        assert_eq!(strip_preview_ranges(&preview), published);
        assert_eq!(
            published,
            include_str!("../testdata/columns-widget.published.html")
        );
        assert_eq!(
            preview,
            include_str!("../testdata/columns-widget.preview.html")
        );
    }

    #[test]
    fn markdown_media_urls_normalize_html_attributes() {
        for html in [
            "<img src='/api/media/7/content'>",
            "<IMG SRC = /api/media/7/content>",
            "<img\nsrc = '/api/media/7/content'>",
            "<img title='a > b' src='/api/media/7/content'>",
        ] {
            assert_eq!(
                markdown_media_urls(html),
                vec!["/api/media/7/content"],
                "{html}"
            );
        }
        assert!(markdown_media_urls("<img src='javascript:alert(1)'>").is_empty());
    }

    #[test]
    fn markdown_media_urls_include_reference_images_and_html() {
        let md = concat!(
            "![cover](/media/assets/11111111-1111-1111-1111-111111111111/a.png)\n\n",
            "![ref][pic]\n\n",
            "[pic]: /api/media/7/content \"title\"\n\n",
            "<img src=\"/media/federation/1/b.jpg\">\n",
        );
        let urls = markdown_media_urls(md);
        assert!(urls.contains(&"/media/assets/11111111-1111-1111-1111-111111111111/a.png".into()));
        assert!(urls.contains(&"/api/media/7/content".into()));
        assert!(urls.contains(&"/media/federation/1/b.jpg".into()));
    }

    #[test]
    fn slugify_keeps_ascii_and_drops_everything_else() {
        assert_eq!(slugify_title("  Hello, World! "), "hello-world");
        assert_eq!(slugify_title("MiXeD_Case"), "mixed-case");
        assert_eq!(slugify_title("你好"), "");
        assert_eq!(slugify_title("Café Notes"), "cafe-notes");
        assert_eq!(slugify_title("Straße"), "strasse");
        assert_eq!(slugify_title("Hello 你好 World"), "hello-world");
        assert_eq!(slugify_title("---"), "");
        assert_eq!(slugify_title(&"A".repeat(100)).chars().count(), 80);
        assert!(!slugify_title(&format!("{}!", "a".repeat(80))).ends_with('-'));
    }

    #[test]
    fn slug_validation_is_the_charset_and_length() {
        assert!(validate_slug("a").is_ok());
        assert!(validate_slug("n12").is_ok());
        assert!(validate_slug("hello-world").is_ok());
        assert!(validate_slug("-a").is_err());
        assert!(validate_slug("a-").is_err());
        assert!(validate_slug("a--b").is_err());
        assert!(validate_slug("").is_err());
        assert!(validate_slug("Hello").is_err());
        assert!(validate_slug("a_b").is_err());
        assert!(validate_slug(&"a".repeat(81)).is_err());
    }

    #[test]
    fn dedupe_appends_numeric_suffixes() {
        let mut taken = HashSet::new();
        assert_eq!(dedupe_slug("hello", &taken), "hello");
        taken.insert("hello".into());
        assert_eq!(dedupe_slug("hello", &taken), "hello-2");
        taken.insert("hello-2".into());
        assert_eq!(dedupe_slug("hello", &taken), "hello-3");
        assert_eq!(fallback_slug(12), "n12");
        assert_eq!(allocate_note_slug("你好", 12, &HashSet::new()), "n12");
        let mut taken_fallback = HashSet::new();
        taken_fallback.insert("n12".into());
        assert_eq!(allocate_note_slug("你好", 12, &taken_fallback), "n12-2");
    }

    #[test]
    fn dedupe_keeps_the_suffix_inside_eighty_characters() {
        let base = "a".repeat(80);
        let mut taken = HashSet::new();
        taken.insert(base.clone());
        let slug = dedupe_slug(&base, &taken);
        assert!(validate_slug(&slug).is_ok());
        assert!(slug.ends_with("-2"));
        assert_ne!(slug, base);
    }

    #[test]
    fn legacy_journal_links_cover_the_old_public_tree() {
        assert!(is_legacy_journal_article_link("/journal"));
        assert!(is_legacy_journal_article_link("/journal/articles/12"));
        assert!(is_legacy_journal_article_link("/journal/articles/12/"));
        assert!(is_legacy_journal_article_link("/journal/notes"));
        assert!(is_legacy_journal_article_link("/journal/topics/ai"));
        assert!(!is_legacy_journal_article_link("/blog/posts/hello/"));
        assert!(!is_legacy_journal_article_link(
            "https://ex.com/journal/articles/1"
        ));
    }

    #[test]
    fn slug_repair_is_idempotent_and_unique_inside_one_source() {
        let rows = vec![
            NoteSlugInput {
                id: 1,
                source_id: 7,
                title: "Hello".into(),
                slug: None,
                link: "/journal/articles/1".into(),
            },
            NoteSlugInput {
                id: 2,
                source_id: 7,
                title: "Hello!".into(),
                slug: None,
                link: "/journal/articles/2".into(),
            },
            NoteSlugInput {
                id: 3,
                source_id: 8,
                title: "Hello".into(),
                slug: None,
                link: "/journal/articles/3".into(),
            },
            NoteSlugInput {
                id: 4,
                source_id: 7,
                title: "Kept".into(),
                slug: Some("kept".into()),
                link: "/blog/posts/kept/".into(),
            },
        ];
        let repairs = plan_note_slug_repairs(rows);
        assert_eq!(
            repairs,
            vec![
                NoteSlugRepair {
                    id: 1,
                    slug: "hello".into(),
                    link: "/blog/posts/hello/".into(),
                },
                NoteSlugRepair {
                    id: 2,
                    slug: "hello-2".into(),
                    link: "/blog/posts/hello-2/".into(),
                },
                NoteSlugRepair {
                    id: 3,
                    slug: "hello".into(),
                    link: "/blog/posts/hello/".into(),
                },
            ]
        );
        let settled = vec![
            NoteSlugInput {
                id: 1,
                source_id: 7,
                title: "Hello".into(),
                slug: Some("hello".into()),
                link: "/blog/posts/hello/".into(),
            },
            NoteSlugInput {
                id: 2,
                source_id: 7,
                title: "Hello!".into(),
                slug: Some("hello-2".into()),
                link: "/blog/posts/hello-2/".into(),
            },
        ];
        assert!(plan_note_slug_repairs(settled).is_empty());
    }
}
