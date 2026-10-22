//! 主题怎么切深浅色。
//!
//! Hugo 没有通用约定：有的主题在 `<html>` 上加 `dark` 类，有的在 `<body>` 上加，
//! 有的写一个 `data-theme` 之类的属性，有的只认系统的 `prefers-color-scheme`。
//! 切换靠主题自己的脚本，而博客页不跑脚本，所以安装时认出它用哪一种（站长再确认），
//! 构建时记在 `<html>` 上，返回页面时由后端按主站现在的深浅色替它加上。

use lol_html::{RewriteStrSettings, element, rewrite_str};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

/// 构建时写在 `<html>` 上的标记，返回页面时读掉。
pub const SCHEME_MARK_ATTR: &str = "data-myriad-scheme";

/// 类名、属性值最长这么多；超过的不像是切换深浅色用的。
const TOKEN_MAX: usize = 40;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SchemeTarget {
    Html,
    Body,
}

impl SchemeTarget {
    fn as_str(self) -> &'static str {
        match self {
            Self::Html => "html",
            Self::Body => "body",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "html" => Some(Self::Html),
            "body" => Some(Self::Body),
            _ => None,
        }
    }
}

/// 主题切深浅色的办法。`light` 是浅色时要写的值；没有就是浅色时去掉。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SchemeSwitch {
    /// 在 `<html>` 或 `<body>` 上加一个类。
    Class {
        target: SchemeTarget,
        dark: String,
        #[serde(default)]
        light: Option<String>,
    },
    /// 在 `<html>` 或 `<body>` 上写一个 `data-*` 属性。
    Attribute {
        target: SchemeTarget,
        name: String,
        dark: String,
        #[serde(default)]
        light: Option<String>,
    },
    /// 只认系统的 `prefers-color-scheme`：后端替它改不了，跟着访客的系统走。
    System,
    /// 主题没有深色。
    None,
}

impl SchemeSwitch {
    /// 站长改的值也走这里：这些字会原样写进页面。
    pub fn is_valid(&self) -> bool {
        match self {
            Self::Class { dark, light, .. } => {
                is_token(dark)
                    && light
                        .as_deref()
                        .is_none_or(|light| is_token(light) && light != dark)
            }
            Self::Attribute {
                name, dark, light, ..
            } => {
                is_attr_name(name)
                    && is_token(dark)
                    && light
                        .as_deref()
                        .is_none_or(|light| is_token(light) && light != dark)
            }
            Self::System | Self::None => true,
        }
    }

    /// 写在 `<html>` 上的标记；只有后端能替主题切的两种才有。
    fn mark(&self) -> Option<String> {
        if !self.is_valid() {
            return None;
        }
        let light = |light: &Option<String>| light.clone().unwrap_or_else(|| "-".to_string());
        match self {
            Self::Class {
                target,
                dark,
                light: lit,
            } => Some(format!("class {} {dark} {}", target.as_str(), light(lit))),
            Self::Attribute {
                target,
                name,
                dark,
                light: lit,
            } => Some(format!(
                "attr {} {name} {dark} {}",
                target.as_str(),
                light(lit)
            )),
            Self::System | Self::None => None,
        }
    }

    fn from_mark(mark: &str) -> Option<Self> {
        let parts: Vec<&str> = mark.split(' ').collect();
        let light = |value: &str| (value != "-").then(|| value.to_string());
        let switch = match parts.as_slice() {
            ["class", target, dark, lit] => Self::Class {
                target: SchemeTarget::parse(target)?,
                dark: dark.to_string(),
                light: light(lit),
            },
            ["attr", target, name, dark, lit] => Self::Attribute {
                target: SchemeTarget::parse(target)?,
                name: name.to_string(),
                dark: dark.to_string(),
                light: light(lit),
            },
            _ => return None,
        };
        switch.is_valid().then_some(switch)
    }

    fn target(&self) -> Option<SchemeTarget> {
        match self {
            Self::Class { target, .. } | Self::Attribute { target, .. } => Some(*target),
            Self::System | Self::None => None,
        }
    }
}

fn is_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= TOKEN_MAX
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

/// 不带 `data-` 的那几个：LoveIt 一家写的是 `<body theme="dark">`。
const BARE_SCHEME_ATTRS: &[&str] = &["theme"];

/// 后端会替主题写的属性名：`data-*`，或 [`BARE_SCHEME_ATTRS`] 里的。
fn is_attr_name(name: &str) -> bool {
    is_data_attr(name) || BARE_SCHEME_ATTRS.contains(&name)
}

fn is_data_attr(name: &str) -> bool {
    name.strip_prefix("data-").is_some_and(|rest| {
        !rest.is_empty()
            && rest.len() <= TOKEN_MAX
            && rest
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    })
}

/// 认出来的办法，连同依据（是哪些文件让人这么认为的）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemeDetection {
    pub switch: SchemeSwitch,
    #[serde(default)]
    pub evidence: Vec<String>,
}

/// 要看哪些文件：样式、脚本、模板和 Tailwind 配置。
pub fn is_scheme_source(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    if name.starts_with("tailwind.config.") {
        return true;
    }
    let ext = name
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase());
    matches!(
        ext.as_deref(),
        Some("css" | "scss" | "sass" | "less" | "js" | "mjs" | "ts" | "html")
    )
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Key {
    Class(SchemeTarget, String),
    Attribute(SchemeTarget, String),
}

#[derive(Default)]
struct Candidate {
    score: u32,
    css_hits: u32,
    dark: BTreeMap<String, u32>,
    evidence: Vec<String>,
}

#[derive(Default)]
struct Tally {
    candidates: BTreeMap<Key, Candidate>,
    /// 浅色那一边见过的值：类名按目标分，属性按名字分。
    light_classes: BTreeMap<SchemeTarget, BTreeMap<String, u32>>,
    light_values: BTreeMap<String, BTreeMap<String, u32>>,
    media: Vec<String>,
}

impl Tally {
    fn add(&mut self, key: Key, dark: Option<&str>, weight: u32, file: &str, css: bool) {
        let entry = self.candidates.entry(key).or_default();
        if css {
            // 编译过的 CSS 里同一个选择器会出现几百次，算到几次就够了。
            if entry.css_hits >= 6 {
                return;
            }
            entry.css_hits += 1;
        }
        entry.score += weight;
        if let Some(dark) = dark {
            *entry.dark.entry(dark.to_string()).or_default() += 1;
        }
        if !entry.evidence.iter().any(|seen| seen == file) {
            entry.evidence.push(file.to_string());
        }
    }
}

/// 安装时看主题的文件，猜它怎么切深浅色。`(路径, 内容)`，路径相对主题根目录。
pub fn detect_scheme_switch<'a>(
    files: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> SchemeDetection {
    let mut tally = Tally::default();
    for (path, body) in files {
        let name = path.rsplit('/').next().unwrap_or(path);
        let ext = name
            .rsplit_once('.')
            .map(|(_, ext)| ext.to_ascii_lowercase())
            .unwrap_or_default();
        if name.starts_with("tailwind.config.") {
            scan_tailwind(body, path, &mut tally);
            continue;
        }
        match ext.as_str() {
            "css" | "scss" | "sass" | "less" => scan_css(body, path, &mut tally),
            "js" | "mjs" | "ts" => scan_script(body, path, &mut tally),
            "html" => {
                scan_script(body, path, &mut tally);
                scan_template(body, path, &mut tally);
                scan_css(body, path, &mut tally);
            }
            _ => {}
        }
    }
    decide(tally)
}

fn decide(tally: Tally) -> SchemeDetection {
    let best = tally
        .candidates
        .iter()
        .filter(|(_, candidate)| candidate.score >= 2)
        .max_by(|(left_key, left), (right_key, right)| {
            left.score
                .cmp(&right.score)
                .then_with(|| right_key.cmp(left_key))
        });
    let Some((key, candidate)) = best else {
        let switch = if tally.media.is_empty() {
            SchemeSwitch::None
        } else {
            SchemeSwitch::System
        };
        return SchemeDetection {
            switch,
            evidence: tally.media.into_iter().take(5).collect(),
        };
    };
    let most = |values: Option<&BTreeMap<String, u32>>| {
        values.and_then(|values| {
            values
                .iter()
                .max_by(|(_, left), (_, right)| left.cmp(right))
                .map(|(value, _)| value.clone())
        })
    };
    let switch = match key {
        Key::Class(target, name) => SchemeSwitch::Class {
            target: *target,
            dark: name.clone(),
            light: most(tally.light_classes.get(target)).filter(|light| light != name),
        },
        Key::Attribute(target, name) => {
            let dark = most(Some(&candidate.dark)).unwrap_or_else(|| "dark".to_string());
            SchemeSwitch::Attribute {
                target: *target,
                name: name.clone(),
                light: most(tally.light_values.get(name)).filter(|light| *light != dark),
                dark,
            }
        }
    };
    let switch = if switch.is_valid() {
        switch
    } else {
        SchemeSwitch::None
    };
    SchemeDetection {
        switch,
        evidence: candidate.evidence.iter().take(5).cloned().collect(),
    }
}

fn is_dark_word(value: &str) -> bool {
    is_scheme_word(value, "dark")
}

fn is_light_word(value: &str) -> bool {
    is_scheme_word(value, "light")
}

/// 属性值还认 `dim` / `lit`（Clarity 的两档）。类名不认：`.dim` 多半是调透明度的工具类。
fn is_dark_value(value: &str) -> bool {
    is_dark_word(value) || value.eq_ignore_ascii_case("dim")
}

fn is_light_value(value: &str) -> bool {
    is_light_word(value) || value.eq_ignore_ascii_case("lit")
}

/// `dark`、`dark-mode`、`theme-dark`、`is-dark` 这几种写法；`highlight`、`bg-dark-blue` 不算。
fn is_scheme_word(value: &str, word: &str) -> bool {
    let value = value.to_ascii_lowercase();
    let Some(at) = value.find(word) else {
        return false;
    };
    let before = &value[..at];
    let after = &value[at + word.len()..];
    matches!(
        before,
        "" | "is-" | "theme-" | "mode-" | "color-" | "scheme-" | "colorscheme-" | "color-scheme-"
    ) && matches!(
        after,
        "" | "-mode" | "mode" | "_mode" | "-theme" | "-scheme"
    )
}

/// 主题用来区分深浅的属性名长这样；`data-src` 之类的不算。
fn is_scheme_attr(name: &str) -> bool {
    BARE_SCHEME_ATTRS.contains(&name)
        || (is_data_attr(name)
            && ["theme", "scheme", "mode", "appearance", "color"]
                .iter()
                .any(|word| name.contains(word)))
}

fn ident_at(text: &str, start: usize) -> &str {
    let rest = &text[start..];
    let end = rest
        .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'))
        .unwrap_or(rest.len());
    &rest[..end]
}

/// `("dark")` 里引号里的那个词。
fn quoted_after(text: &str, start: usize) -> Option<&str> {
    let rest = text[start..].trim_start();
    let rest = rest.strip_prefix('(')?.trim_start();
    let quote = rest
        .chars()
        .next()
        .filter(|ch| matches!(ch, '"' | '\'' | '`'))?;
    let rest = &rest[1..];
    let end = rest.find(quote)?;
    Some(&rest[..end])
}

fn camel_to_kebab(name: &str) -> String {
    let mut out = String::from("data-");
    for ch in name.chars() {
        if ch.is_ascii_uppercase() {
            out.push('-');
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

/// 主题的切换脚本：`documentElement.classList.add("dark")`、`body.setAttribute("data-theme", …)` 之类。
fn scan_script(body: &str, file: &str, tally: &mut Tally) {
    for (needle, target) in [
        ("documentElement.classList.", SchemeTarget::Html),
        ("querySelector(\"html\").classList.", SchemeTarget::Html),
        ("querySelector('html').classList.", SchemeTarget::Html),
        ("document.body.classList.", SchemeTarget::Body),
    ] {
        for (at, _) in body.match_indices(needle) {
            let after = at + needle.len();
            let method = ident_at(body, after);
            if !matches!(method, "add" | "toggle" | "remove" | "contains" | "replace") {
                continue;
            }
            let Some(class) = quoted_after(body, after + method.len()) else {
                continue;
            };
            if is_token(class) && is_dark_word(class) {
                tally.add(Key::Class(target, class.to_string()), None, 10, file, false);
            } else if is_token(class) && is_light_word(class) {
                *tally
                    .light_classes
                    .entry(target)
                    .or_default()
                    .entry(class.to_string())
                    .or_default() += 1;
            }
        }
    }
    for (needle, target) in [
        ("documentElement.setAttribute", SchemeTarget::Html),
        ("document.body.setAttribute", SchemeTarget::Body),
    ] {
        for (at, _) in body.match_indices(needle) {
            if let Some(name) = quoted_after(body, at + needle.len())
                && is_scheme_attr(name)
            {
                tally.add(
                    Key::Attribute(target, name.to_string()),
                    None,
                    10,
                    file,
                    false,
                );
            }
        }
    }
    for (needle, target) in [
        ("documentElement.dataset.", SchemeTarget::Html),
        ("querySelector(\"html\").dataset.", SchemeTarget::Html),
        ("querySelector('html').dataset.", SchemeTarget::Html),
        ("document.body.dataset.", SchemeTarget::Body),
    ] {
        for (at, _) in body.match_indices(needle) {
            let ident = ident_at(body, at + needle.len());
            let rest = body[at + needle.len() + ident.len()..].trim_start();
            if rest.starts_with('=') && !rest.starts_with("==") {
                let name = camel_to_kebab(ident);
                if is_scheme_attr(&name) {
                    tally.add(Key::Attribute(target, name), None, 10, file, false);
                }
            }
        }
    }
}

/// `darkMode: "class"`（Tailwind 默认挂在 `<html>` 上的 `dark` 类）。
fn scan_tailwind(body: &str, file: &str, tally: &mut Tally) {
    let Some(at) = body.find("darkMode") else {
        return;
    };
    let rest: String = body[at..].chars().take(80).collect();
    if rest.contains("class") || rest.contains("selector") {
        tally.add(
            Key::Class(SchemeTarget::Html, "dark".to_string()),
            None,
            8,
            file,
            false,
        );
    }
}

/// 模板里 `<html>` / `<body>` 开标签上已经写着的 `data-theme="…"`。
fn scan_template(body: &str, file: &str, tally: &mut Tally) {
    let lower = body.to_ascii_lowercase();
    for (tag, target) in [("<html", SchemeTarget::Html), ("<body", SchemeTarget::Body)] {
        let Some(at) = lower.find(tag) else {
            continue;
        };
        let end = lower[at..].find('>').map_or(lower.len(), |end| at + end);
        let open = &lower[at..end];
        for (pos, _) in open.match_indices("data-") {
            let name = ident_at(open, pos);
            if is_scheme_attr(name) && open[pos + name.len()..].trim_start().starts_with('=') {
                tally.add(
                    Key::Attribute(target, name.to_string()),
                    None,
                    3,
                    file,
                    false,
                );
            }
        }
    }
}

/// 样式里的 `html.dark`、`body.dark-mode`、`[data-theme="dark"]` 和 `prefers-color-scheme`。
fn scan_css(body: &str, file: &str, tally: &mut Tally) {
    if (body.contains("prefers-color-scheme: dark") || body.contains("prefers-color-scheme:dark"))
        && !tally.media.iter().any(|seen| seen == file)
    {
        tally.media.push(file.to_string());
    }
    let bytes = body.as_bytes();
    for (at, _) in body.match_indices('.') {
        let class = ident_at(body, at + 1);
        if class.is_empty() || !is_token(class) {
            continue;
        }
        let next = bytes.get(at + 1 + class.len()).copied();
        // Tailwind 编出来的 `.dark\:bg-…` 是工具类，不是切换用的类。
        if next == Some(b'\\') {
            continue;
        }
        // 前面紧挨着字母数字的是 `1.5`、`a.b` 之类，不是选择器。
        if at > 0 && bytes[at - 1].is_ascii_alphanumeric() && !preceded_by_tag(body, at) {
            continue;
        }
        let target = selector_target(body, at);
        if is_dark_word(class) {
            tally.add(Key::Class(target, class.to_string()), None, 1, file, true);
        } else if is_light_word(class) {
            *tally
                .light_classes
                .entry(target)
                .or_default()
                .entry(class.to_string())
                .or_default() += 1;
        }
    }
    let selectors = body
        .match_indices("[data-")
        .chain(BARE_SCHEME_ATTRS.iter().flat_map(|name| {
            body.match_indices(name).filter(move |(at, _)| {
                *at > 0
                    && body.as_bytes()[at - 1] == b'['
                    && body[at + name.len()..].starts_with('=')
            })
        }))
        .map(|(at, text)| if text.starts_with('[') { at } else { at - 1 })
        .collect::<Vec<_>>();
    for at in selectors {
        let name = ident_at(body, at + 1);
        if !is_scheme_attr(name) {
            continue;
        }
        let rest = &body[at + 1 + name.len()..];
        let Some(rest) = rest.strip_prefix('=').or_else(|| rest.strip_prefix("~=")) else {
            continue;
        };
        let rest = rest.trim_start_matches(['"', '\'']);
        let value = ident_at(rest, 0);
        if !is_token(value) {
            continue;
        }
        let target = selector_target(body, at);
        if is_dark_value(value) {
            tally.add(
                Key::Attribute(target, name.to_string()),
                Some(value),
                1,
                file,
                true,
            );
        } else if is_light_value(value) {
            *tally
                .light_values
                .entry(name.to_string())
                .or_default()
                .entry(value.to_string())
                .or_default() += 1;
        }
    }
}

fn preceded_by_tag(body: &str, at: usize) -> bool {
    let before = &body[..at];
    before.ends_with("html") || before.ends_with("body") || before.ends_with(":root")
}

/// 选择器挂在哪：紧跟在 `body` 后面的算 `<body>`，其余（`html`、`:root`、光秃秃的 `.dark`）算 `<html>`。
fn selector_target(body: &str, at: usize) -> SchemeTarget {
    if body[..at].ends_with("body") {
        SchemeTarget::Body
    } else {
        SchemeTarget::Html
    }
}

/// 构建出来的样式里有没有这个办法的深色选择器（`.dark`、`[data-theme="dark"]`）。
/// 主题的深色样式可能要站点参数打开才会被模板引进来（Coder 的 `colorScheme`），
/// 也可能认错了属性：样式里没有，替它切了页面也不会变，按钮成了摆设。
/// 页面只认站内的样式表（CSP 不放外链样式），所以看构建产物就够了。
pub fn scheme_is_styled(switch: &SchemeSwitch, css: &str) -> bool {
    let ident = |byte: Option<&u8>| {
        byte.is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'-' || *b == b'_')
    };
    match switch {
        SchemeSwitch::Class { dark, .. } => {
            let needle = format!(".{dark}");
            css.match_indices(&needle).any(|(at, _)| {
                let next = css.as_bytes().get(at + needle.len());
                !ident(next) && next != Some(&b'\\')
            })
        }
        SchemeSwitch::Attribute { name, dark, .. } => {
            let needle = format!("[{name}");
            css.match_indices(&needle).any(|(at, _)| {
                let rest = &css[at + needle.len()..];
                let Some(rest) = rest.strip_prefix('=').or_else(|| rest.strip_prefix("~=")) else {
                    return false;
                };
                let rest = rest.trim_start_matches(['"', '\'']);
                rest.strip_prefix(dark.as_str())
                    .is_some_and(|after| !ident(after.as_bytes().first()))
            })
        }
        SchemeSwitch::System | SchemeSwitch::None => false,
    }
}

/// 构建时：把确认过的办法记在 `<html>` 上，返回页面时 [`apply_page_scheme`] 读它。
pub fn mark_page_scheme(html: &str, switch: &SchemeSwitch) -> String {
    let Some(mark) = switch.mark() else {
        return html.to_string();
    };
    rewrite_str(
        html,
        RewriteStrSettings {
            element_content_handlers: vec![element!("html", move |el| {
                let _ = el.set_attribute(SCHEME_MARK_ATTR, &mark);
                Ok(())
            })],
            ..RewriteStrSettings::new()
        },
    )
    .unwrap_or_else(|_| html.to_string())
}

/// 返回页面时：照 `<html>` 上的标记，按主站现在是深色还是浅色替主题切好。没有标记就原样不动。
pub fn apply_page_scheme(html: &str, dark: bool) -> Option<String> {
    if !html.contains(SCHEME_MARK_ATTR) {
        return None;
    }
    let found: Rc<RefCell<Option<SchemeSwitch>>> = Rc::new(RefCell::new(None));
    let on_html = Rc::clone(&found);
    let on_body = Rc::clone(&found);
    rewrite_str(
        html,
        RewriteStrSettings {
            element_content_handlers: vec![
                element!("html", move |el| {
                    let switch = el
                        .get_attribute(SCHEME_MARK_ATTR)
                        .and_then(|mark| SchemeSwitch::from_mark(&mark));
                    // 标记留着：伴读上的深浅色按钮靠它知道这个主题怎么切。
                    if let Some(switch) = &switch
                        && switch.target() == Some(SchemeTarget::Html)
                    {
                        set_scheme(el, switch, dark);
                    }
                    *on_html.borrow_mut() = switch;
                    Ok(())
                }),
                element!("body", move |el| {
                    if let Some(switch) = on_body.borrow().as_ref()
                        && switch.target() == Some(SchemeTarget::Body)
                    {
                        set_scheme(el, switch, dark);
                    }
                    Ok(())
                }),
            ],
            ..RewriteStrSettings::new()
        },
    )
    .ok()
}

fn set_scheme(el: &mut lol_html::html_content::Element<'_, '_>, switch: &SchemeSwitch, dark: bool) {
    match switch {
        SchemeSwitch::Class {
            dark: dark_class,
            light,
            ..
        } => {
            let current = el.get_attribute("class").unwrap_or_default();
            // `colorscheme-auto` 这种「跟系统」的类也拿掉：留着它，系统是深色时主题照样变深。
            let auto = dark_class.replace("dark", "auto");
            let mut classes: Vec<&str> = current
                .split_whitespace()
                .filter(|class| {
                    *class != dark_class && Some(*class) != light.as_deref() && *class != auto
                })
                .collect();
            if dark {
                classes.push(dark_class);
            } else if let Some(light) = light {
                classes.push(light);
            }
            if classes.is_empty() {
                el.remove_attribute("class");
            } else {
                let _ = el.set_attribute("class", &classes.join(" "));
            }
        }
        SchemeSwitch::Attribute {
            name,
            dark: dark_value,
            light,
            ..
        } => {
            if dark {
                let _ = el.set_attribute(name, dark_value);
            } else if let Some(light) = light {
                let _ = el.set_attribute(name, light);
            } else {
                el.remove_attribute(name);
            }
        }
        SchemeSwitch::System | SchemeSwitch::None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detect(files: &[(&str, &str)]) -> SchemeSwitch {
        detect_scheme_switch(files.iter().copied()).switch
    }

    #[test]
    fn tailwind_class_on_html() {
        let switch = detect(&[
            (
                "tailwind.config.js",
                "module.exports = { darkMode: \"class\", theme: {} }",
            ),
            (
                "assets/js/appearance.js",
                "document.documentElement.classList.add(\"dark\");",
            ),
            (
                "assets/css/compiled/main.css",
                ":is(.dark *) a{color:red} .dark\\:bg-x:is(.dark *){}",
            ),
        ]);
        assert_eq!(
            switch,
            SchemeSwitch::Class {
                target: SchemeTarget::Html,
                dark: "dark".into(),
                light: None
            }
        );
    }

    #[test]
    fn class_on_body_with_light_partner() {
        let switch = detect(&[
            (
                "assets/js/theme.js",
                "document.body.classList.toggle('dark'); document.body.classList.remove('light')",
            ),
            (
                "assets/css/core.css",
                "body.dark{--bg:#000} body.light{--bg:#fff}",
            ),
        ]);
        assert_eq!(
            switch,
            SchemeSwitch::Class {
                target: SchemeTarget::Body,
                dark: "dark".into(),
                light: Some("light".into())
            }
        );
    }

    #[test]
    fn data_attribute_with_values_from_css() {
        let switch = detect(&[
            (
                "assets/ts/colorScheme.ts",
                "document.documentElement.dataset.scheme = scheme",
            ),
            (
                "assets/scss/vars.scss",
                ":root[data-scheme=\"dark\"]{--a:1} :root[data-scheme=\"light\"]{--a:2}",
            ),
        ]);
        assert_eq!(
            switch,
            SchemeSwitch::Attribute {
                target: SchemeTarget::Html,
                name: "data-scheme".into(),
                dark: "dark".into(),
                light: Some("light".into())
            }
        );
    }

    #[test]
    fn media_only_and_nothing() {
        assert_eq!(
            detect(&[(
                "static/css/style.css",
                "@media (prefers-color-scheme: dark){body{background:#000}}"
            )]),
            SchemeSwitch::System
        );
        assert_eq!(
            detect(&[("static/css/style.css", "body{color:#222} .a{width:1.5rem}")]),
            SchemeSwitch::None
        );
    }

    #[test]
    fn colorscheme_class_on_body() {
        // Coder：模板在 <body> 上写 colorscheme-light / -dark / -auto，脚本照类名判断。
        let switch = detect(&[
            (
                "assets/js/coder.js",
                "body.classList.contains('colorscheme-dark'); document.body.classList.remove('colorscheme-light')",
            ),
            (
                "assets/js/toggle.js",
                "document.body.classList.contains('colorscheme-dark')",
            ),
            (
                "assets/scss/_base_dark.scss",
                "body.colorscheme-dark{color:#fff} @media (prefers-color-scheme: dark){body.colorscheme-auto{color:#fff}}",
            ),
        ]);
        assert_eq!(
            switch,
            SchemeSwitch::Class {
                target: SchemeTarget::Body,
                dark: "colorscheme-dark".into(),
                light: Some("colorscheme-light".into())
            }
        );
    }

    #[test]
    fn bare_theme_attribute_on_body() {
        // LoveIt：`document.body.setAttribute('theme', …)`，样式写 `[theme=dark]`。
        let switch = detect(&[
            (
                "layouts/baseof.html",
                "<script>document.body.setAttribute('theme', isDark ? 'dark' : 'light');</script>",
            ),
            (
                "assets/css/_core/_base.scss",
                "[theme=dark] & {color:#fff} [theme=dark] a {color:#ccc}",
            ),
        ]);
        assert_eq!(
            switch,
            SchemeSwitch::Attribute {
                target: SchemeTarget::Body,
                name: "theme".into(),
                dark: "dark".into(),
                light: None
            }
        );
        assert!(switch.is_valid());
    }

    #[test]
    fn dim_and_lit_attribute_values() {
        // Clarity：`<html data-mode="dim">`，样式写 `html[data-mode="dim"]` / `[data-mode="lit"]`。
        let switch = detect(&[
            ("layouts/_default/baseof.html", "<html data-mode=\"lit\">"),
            (
                "assets/sass/_base.sass",
                "html[data-mode=\"dim\"]\n  --bg: #000\nhtml[data-mode=\"lit\"]\n  --bg: #fff",
            ),
        ]);
        assert_eq!(
            switch,
            SchemeSwitch::Attribute {
                target: SchemeTarget::Html,
                name: "data-mode".into(),
                dark: "dim".into(),
                light: Some("lit".into())
            }
        );
    }

    #[test]
    fn switching_drops_the_follow_system_class() {
        let page =
            "<!doctype html><html><head></head><body class=\"colorscheme-auto x\"></body></html>";
        let class = SchemeSwitch::Class {
            target: SchemeTarget::Body,
            dark: "colorscheme-dark".into(),
            light: Some("colorscheme-light".into()),
        };
        let marked = mark_page_scheme(page, &class);
        assert!(
            apply_page_scheme(&marked, false)
                .unwrap()
                .contains("<body class=\"x colorscheme-light\">")
        );
        assert!(
            apply_page_scheme(&marked, true)
                .unwrap()
                .contains("<body class=\"x colorscheme-dark\">")
        );
    }

    #[test]
    fn styled_only_when_the_built_css_has_the_dark_selector() {
        let class = SchemeSwitch::Class {
            target: SchemeTarget::Html,
            dark: "dark".into(),
            light: None,
        };
        assert!(scheme_is_styled(&class, "html.dark{--bg:#000}"));
        assert!(scheme_is_styled(&class, ":is(.dark *){color:#fff}"));
        assert!(!scheme_is_styled(&class, ".dark\\:bg-black{} .darker{}"));
        let attr = SchemeSwitch::Attribute {
            target: SchemeTarget::Html,
            name: "data-theme".into(),
            dark: "dark".into(),
            light: None,
        };
        assert!(scheme_is_styled(&attr, ":root[data-theme=\"dark\"]{}"));
        assert!(scheme_is_styled(&attr, "[data-theme=dark] a{}"));
        assert!(!scheme_is_styled(
            &attr,
            "[data-theme=darkblue]{} [data-themes=dark]{}"
        ));
        assert!(!scheme_is_styled(&SchemeSwitch::System, "html.dark{}"));
    }

    #[test]
    fn mark_then_apply_round_trip() {
        let page = "<!doctype html><html lang=\"zh\" class=\"scroll-smooth\"><head></head><body class=\"x\"></body></html>";
        let class = SchemeSwitch::Class {
            target: SchemeTarget::Html,
            dark: "dark".into(),
            light: None,
        };
        let marked = mark_page_scheme(page, &class);
        assert!(marked.contains(SCHEME_MARK_ATTR));
        let dark = apply_page_scheme(&marked, true).unwrap();
        assert!(dark.contains("class=\"scroll-smooth dark\""));
        assert!(dark.contains(SCHEME_MARK_ATTR));
        let light = apply_page_scheme(&marked, false).unwrap();
        assert!(light.contains("class=\"scroll-smooth\""));

        let attr = SchemeSwitch::Attribute {
            target: SchemeTarget::Body,
            name: "data-theme".into(),
            dark: "dark".into(),
            light: Some("light".into()),
        };
        let marked = mark_page_scheme(page, &attr);
        assert!(
            apply_page_scheme(&marked, true)
                .unwrap()
                .contains("<body class=\"x\" data-theme=\"dark\">")
        );
        assert!(
            apply_page_scheme(&marked, false)
                .unwrap()
                .contains("data-theme=\"light\"")
        );
        assert_eq!(apply_page_scheme(page, true), None);
    }

    #[test]
    fn rejects_unsafe_values() {
        let bad = SchemeSwitch::Attribute {
            target: SchemeTarget::Html,
            name: "onload".into(),
            dark: "x".into(),
            light: None,
        };
        assert!(!bad.is_valid());
        assert_eq!(mark_page_scheme("<html></html>", &bad), "<html></html>");
        let quote = SchemeSwitch::Class {
            target: SchemeTarget::Html,
            dark: "a\"b".into(),
            light: None,
        };
        assert!(!quote.is_valid());
        assert_eq!(SchemeSwitch::from_mark("class html dark\" -"), None);
    }
}
