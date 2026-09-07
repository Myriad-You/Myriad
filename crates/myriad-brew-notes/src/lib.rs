//! 手记的纯规则：Markdown 渲染、消毒，以及从正文派生的文章字段。
//!
//! 全站只有这一处把 Markdown 变成 HTML。阅读器、RSS 输出、联邦投递、SEO 摘要
//! 看到的都是这里产出的同一份 HTML —— 编辑器的预览也走同一个函数，预览和发布
//! 后不一致这件事在结构上就不可能发生。
//!
//! 没有 I/O：不读库、不发请求、不碰文件。消毒是白名单制，`ammonia` 默认放行的
//! 那些标签这里再收窄一遍。

use std::collections::{HashMap, HashSet};

use pulldown_cmark::{html, Options, Parser};

/// 正文长度上限（字符）。超出的部分不截断，直接拒绝 —— 悄悄截掉用户写的东西
/// 比报错更糟。
pub const MAX_NOTE_CHARS: usize = 200_000;
/// 标题长度上限（字符）。库里是 text，这个上限是产品上限。
pub const MAX_TITLE_CHARS: usize = 200;
/// 摘要取多少字符。
pub const SUMMARY_CHARS: usize = 200;
/// 估算阅读速度（字/分钟）。中英混排取一个折中值，不区分语言。
pub const WORDS_PER_MINUTE: i32 = 400;

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
            Self::EmptyTitle => "标题不能为空".to_string(),
            Self::TitleTooLong { chars } => {
                format!("标题最多 {MAX_TITLE_CHARS} 字，现在有 {chars} 字")
            }
            Self::BodyTooLong { chars } => {
                format!("正文最多 {MAX_NOTE_CHARS} 字，现在有 {chars} 字")
            }
        }
    }
}

/// 一篇手记渲染后的全部派生字段。写库时逐个落到 `brew_items` 的同名列。
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

/// 开启的 Markdown 扩展。删除线、表格、任务列表、脚注 —— 与编辑器工具栏
/// 能产出的语法保持一致，不多开。
fn markdown_options() -> Options {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_FOOTNOTES);
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
    // 任务列表渲染成 checkbox；只放行这三个属性，checked 由 Markdown 决定
    map.insert(
        "input",
        ["type", "checked", "disabled"].into_iter().collect(),
    );
    map.insert("td", ["colspan", "rowspan"].into_iter().collect());
    map.insert("th", ["colspan", "rowspan", "scope"].into_iter().collect());
    // 代码块的语言类名（`language-rust`），高亮靠它
    map.insert("code", ["class"].into_iter().collect());
    map
}

/// 把 Markdown 渲染成可以直接插进阅读器的 HTML。
///
/// 消毒是硬性的一步，不是可选项：手记正文虽然只有站长能写，但它会经联邦发到
/// 别人的实例上，也会被搜索引擎抓走。
pub fn render_markdown(markdown: &str) -> String {
    let parser = Parser::new_ext(markdown, markdown_options());
    let mut raw = String::new();
    html::push_html(&mut raw, parser);

    ammonia::Builder::default()
        .tags(allowed_tags())
        .tag_attributes(allowed_attributes())
        // 站外链接一律新窗口打开并断开 referrer / opener
        .link_rel(Some("noopener noreferrer nofollow"))
        // 只认这几种协议：javascript: / data: 一律拦掉
        .url_schemes(["http", "https", "mailto"].into_iter().collect())
        .clean(&raw)
        .to_string()
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

/// 取正文里第一张图的地址当封面。
///
/// 在已消毒的 HTML 上扫 `<img src="...">`：消毒已经保证了协议合法、引号规整，
/// 这里不需要再做一次 URL 校验。
fn first_image(html: &str) -> Option<String> {
    let bytes = html.as_bytes();
    let mut i = 0;
    while let Some(found) = html[i..].find("<img ") {
        let tag_start = i + found;
        let tag_end = html[tag_start..].find('>').map(|e| tag_start + e)?;
        let tag = &html[tag_start..tag_end];
        if let Some(src_at) = tag.find("src=\"") {
            let value_start = src_at + 5;
            if let Some(value_len) = tag[value_start..].find('"') {
                let src = &tag[value_start..value_start + value_len];
                if !src.is_empty() {
                    return Some(src.to_string());
                }
            }
        }
        i = tag_end.min(bytes.len());
        if i >= html.len() {
            break;
        }
    }
    None
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

/// 渲染一篇手记并算出全部派生字段。调用前先过 [`validate_note`]。
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
        // 一篇字数为 0 的手记（只有一张图）也要显示 1 分钟，不显示 0
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

/// 手记在 `brew_items.guid` 里的取值。手记没有上游 feed，guid 由平台自己生成。
pub fn note_guid(uuid: &str) -> String {
    format!("note:{uuid}")
}

/// 手记的站内链接。与前端 `brewOwnItemPath` 同一口径。
pub fn note_link(item_id: i32) -> String {
    format!("/brew/item/{item_id}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_basic_markdown() {
        let html = render_markdown("# 标题\n\n正文**加粗**。");
        assert!(html.contains("<h1>标题</h1>"));
        assert!(html.contains("<strong>加粗</strong>"));
    }

    #[test]
    fn strips_script_tags() {
        let html = render_markdown("<script>alert(1)</script>\n\n正文");
        assert!(!html.contains("script"));
        assert!(html.contains("正文"));
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
    fn title_is_trimmed_in_the_render_result() {
        let note = render_note("  标题  ", "正文");
        assert_eq!(note.title, "标题");
    }

    #[test]
    fn guid_and_link_shapes() {
        assert_eq!(note_guid("abc"), "note:abc");
        assert_eq!(note_link(12), "/brew/item/12");
    }

    #[test]
    fn entities_survive_a_round_trip() {
        let note = render_note("标题", "a & b < c");
        assert_eq!(note.summary.as_deref(), Some("a & b < c"));
    }
}
