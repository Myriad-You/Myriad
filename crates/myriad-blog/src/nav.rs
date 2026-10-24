//! 导航岛：博客页上回主站的那一条，和主站的导航岛长一个样。
//!
//! 博客页不跑脚本，所以这是一份纯 HTML 加样式，后端每次返回页面时插进去：换了主题
//! 也照样有，静态站本身的文件不变。条目（顺序、地址、模块、图标）和主站共用
//! `shared/nav_island.json`；文案由调用方按访客的语言给。
//! 主站和博客之间切换的动效，博客这一半也在这份样式里（`nav_island.css`）。

use std::sync::LazyLock;

use serde::Deserialize;

use crate::rewrite::{DORMANT_FRAME_SRC, REVEALED_FRAME_SRC};
use crate::studio::escape_attr;

/// 导航岛的 id；样式里的 `<link rel="expect">` 等它解析出来才画第一帧。
pub const NAV_ISLAND_ID: &str = "myriad-nav";

/// 博客自己那一项：在博客页上它是选中的。
const BLOG_ITEM: &str = "phantasi";

const NAV_ISLAND_CSS: &str = include_str!("nav_island.css");

/// 页尾的空白：手机上导航岛横在底部，给它让出位置，页脚不被盖住。只在那时有高度。
const NAV_ISLAND_ROOM: &str = "<div class=\"myriad-nav-room\" aria-hidden=\"true\"></div>";

#[derive(Deserialize)]
struct Contract {
    items: Vec<NavItem>,
}

/// `shared/nav_island.json` 里的一个条目。
#[derive(Debug, Deserialize)]
pub struct NavItem {
    pub id: String,
    pub path: String,
    /// 模块可见性的键；没有就是谁都能看。
    pub module: Option<String>,
    /// 读屏念的文案键（前端 `nav.*`、后端 `backend/i18n/nav.*`）。
    pub label: String,
    /// 悬停提示的文案键。
    pub tip: String,
    icon: NavIcon,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NavIcon {
    stroke_width: f64,
    transform: Option<String>,
    paths: Vec<String>,
}

static ITEMS: LazyLock<Vec<NavItem>> = LazyLock::new(|| {
    serde_json::from_str::<Contract>(include_str!("../../../shared/nav_island.json"))
        .expect("shared/nav_island.json")
        .items
});

pub fn nav_island_items() -> &'static [NavItem] {
    &ITEMS
}

/// 插在 `</head>` 前：样式，以及「导航岛解析出来之前不画第一帧」，
/// 换页动效拍新页面的那一帧时导航岛必须已经在。
pub fn nav_island_head() -> &'static str {
    static HEAD: LazyLock<String> = LazyLock::new(|| {
        format!(
            "<style id=\"{NAV_ISLAND_ID}-style\">{NAV_ISLAND_CSS}</style>\
<link rel=\"expect\" href=\"#{NAV_ISLAND_ID}\" blocking=\"render\">"
        )
    });
    &HEAD
}

/// 博客页回主站的预渲染规则，放在 `Speculation-Rules` 响应头里指向这个地址：
/// 博客页不许跑脚本，规则写不进页面。主站去博客那一半在 frontend/src/utils/siteTransitionScript.ts。
pub const NAV_SPECULATION_RULES_PATH: &str = "/blog/_myriad/speculation-rules.json";

/// 鼠标停在导航岛的主站条目上（触屏是按下时）就在后台把主站打开：点下去时主站的导航岛
/// 已经画好，换页动效能接上，不再先看到加载页。
pub const NAV_SPECULATION_RULES: &str = r##"{"prerender":[{"where":{"selector_matches":"#myriad-nav a:not([aria-current])"},"eagerness":"moderate"}]}"##;

/// 主站写给博客的 cookie：主站的深浅色和颜色，博客页不跑脚本，只能这样知道。
pub const SITE_CHROME_COOKIE: &str = "myriad_chrome";

/// 主站现在的样子，由 frontend/src/utils/siteChromeCookie.ts 写成
/// `{设置}.{现在的深浅色}.{主色}.{强调色}`，颜色是六位十六进制、不带 `#`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteChrome {
    /// 访客在主站选了固定的深色或浅色；`None` 是跟随系统。
    pinned: Option<Scheme>,
    /// 写 cookie 时主站是哪一边，强调色是按这一边算的。
    current: Scheme,
    primary: String,
    accent: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scheme {
    Light,
    Dark,
}

impl Scheme {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "light" => Some(Self::Light),
            "dark" => Some(Self::Dark),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
}

impl SiteChrome {
    /// 只认上面那个格式，别的一律不要：这些字会原样写进页面的样式里。
    pub fn parse(value: &str) -> Option<Self> {
        let mut parts = value.split('.');
        let (setting, current, primary, accent) =
            (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
        if parts.next().is_some() {
            return None;
        }
        let pinned = match setting {
            "auto" => None,
            other => Some(Scheme::parse(other)?),
        };
        let hex = |value: &str| {
            (value.len() == 6 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
                .then(|| value.to_ascii_lowercase())
        };
        Some(Self {
            pinned,
            current: Scheme::parse(current)?,
            primary: hex(primary)?,
            accent: hex(accent)?,
        })
    }

    /// 主站现在是不是深色：固定了就是固定的那一边，跟随系统就是写 cookie 时系统那一边。
    pub fn is_dark(&self) -> bool {
        self.pinned.unwrap_or(self.current) == Scheme::Dark
    }

    /// 写在导航岛上的属性。强调色只对写 cookie 时那一边有效，跟随系统时另一边留默认色。
    fn nav_attrs(&self) -> String {
        let accent = format!("#{}", self.accent);
        let accent = match (self.pinned, self.current) {
            (Some(_), _) => accent,
            (None, Scheme::Light) => format!("light-dark({accent}, {DEFAULT_ACCENT_DARK})"),
            (None, Scheme::Dark) => format!("light-dark({DEFAULT_ACCENT_LIGHT}, {accent})"),
        };
        let scheme = self
            .pinned
            .map(|scheme| format!(" data-scheme=\"{}\"", scheme.as_str()))
            .unwrap_or_default();
        format!(
            "{scheme} style=\"--myriad-nav-primary: #{}; --myriad-nav-accent: {accent}\"",
            self.primary
        )
    }
}

/// 没有主站颜色时的强调色，和 `nav_island.css` 里的默认值一致。
const DEFAULT_ACCENT_LIGHT: &str = "#64748b";
const DEFAULT_ACCENT_DARK: &str = "#cbd5e1";

/// 导航岛本身。`shown` 按模块键判断访客能不能看到这一项；`text` 按文案键取字，
/// 除了条目的键还会问 `mainNavigation`。`chrome` 是主站的颜色，没有就用默认的。
pub fn nav_island_html(
    shown: impl Fn(&str) -> bool,
    text: impl Fn(&str) -> String,
    chrome: Option<&SiteChrome>,
) -> String {
    let mut links = String::new();
    for item in nav_island_items() {
        if item.module.as_deref().is_some_and(|module| !shown(module)) {
            continue;
        }
        // 选中的底色和小圆点画在单独一块上：换页时只有它从主站原来的条目滑过来。
        // 图标也各有自己的名字，叠在底色上面，底色滑过时不会把它们盖住。
        let (current, mark) = if item.id == BLOG_ITEM {
            (
                " aria-current=\"page\"",
                "<span class=\"myriad-nav__mark\" aria-hidden=\"true\"></span>",
            )
        } else {
            ("", "")
        };
        links.push_str(&format!(
            "<a class=\"myriad-nav__item\" href=\"{}\" aria-label=\"{}\" data-tip=\"{}\"{current}>{mark}{}</a>",
            escape_attr(&item.path),
            escape_attr(&text(&item.label)),
            escape_attr(&text(&item.tip)),
            icon_svg(&item.id, &item.icon),
        ));
    }
    format!(
        "<nav id=\"{NAV_ISLAND_ID}\" class=\"myriad-nav\" aria-label=\"{}\"{}><div class=\"myriad-nav__island\">{links}</div></nav>",
        escape_attr(&text("mainNavigation")),
        chrome.map(SiteChrome::nav_attrs).unwrap_or_default(),
    )
}

/// 图标的换页名字和主站 `NavigationIsland` 给的一样，两边同一个图标原地交接。
fn icon_svg(id: &str, icon: &NavIcon) -> String {
    let transform = icon
        .transform
        .as_deref()
        .map(|value| format!(" transform=\"{}\"", escape_attr(value)))
        .unwrap_or_default();
    let paths: String = icon
        .paths
        .iter()
        .map(|d| format!("<path d=\"{}\"/>", escape_attr(d)))
        .collect();
    format!(
        "<svg class=\"myriad-nav__icon\" style=\"view-transition-name: myriad-nav-icon-{}\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" aria-hidden=\"true\" focusable=\"false\">\
<g stroke-linecap=\"round\" stroke-linejoin=\"round\" stroke-width=\"{}\"{transform}>{paths}</g></svg>",
        escape_attr(id),
        icon.stroke_width
    )
}

/// `head` 插在第一个 `</head>` 前，`island` 插在 `<body>` 开始标签之后：放在最前面，
/// 第一帧之前就解析到。找不到标签就都放在最前。页尾再留一块给手机上的底栏让位。
pub fn inject_nav_island(html: &str, head: &str, island: &str) -> String {
    decorate_page(html, head, island, None)
}

/// 和 [`inject_nav_island`] 一样插导航岛；`studio` 有值时（站长）再给伴读控制器地址
/// （同 [`crate::reveal_companion_island`]），并把创作台放在页尾留白之后。
/// 插入点在原页上一次找齐、一次拼好：不整页转小写，也不每插一样就复制一遍。
pub fn decorate_page(html: &str, head: &str, island: &str, studio: Option<&str>) -> String {
    let head_at = find_tag(html, "</head>");
    let body_at = find_tag(html, "<body")
        .and_then(|start| html[start..].find('>').map(|end| start + end + 1));
    let mut edits: Vec<(usize, usize, &str)> = Vec::with_capacity(5);
    match (head_at, body_at) {
        (Some(head_at), Some(body_at)) if head_at < body_at => {
            edits.push((head_at, 0, head));
            edits.push((body_at, 0, island));
        }
        (_, Some(body_at)) => {
            edits.push((body_at, 0, head));
            edits.push((body_at, 0, island));
        }
        _ => {
            edits.push((0, 0, head));
            edits.push((0, 0, island));
        }
    }
    let body_end = rfind_tag(html, "</body>").unwrap_or(html.len());
    edits.push((body_end, 0, NAV_ISLAND_ROOM));
    if let Some(studio) = studio {
        if let Some(at) = html.find(DORMANT_FRAME_SRC) {
            edits.push((at, DORMANT_FRAME_SRC.len(), REVEALED_FRAME_SRC));
        }
        edits.push((body_end, 0, studio));
    }
    splice(html, edits)
}

/// 按位置依次替换；同一位置按加入的先后。
fn splice(html: &str, mut edits: Vec<(usize, usize, &str)>) -> String {
    edits.sort_by_key(|(at, _, _)| *at);
    let added: usize = edits.iter().map(|(_, _, text)| text.len()).sum();
    let mut out = String::with_capacity(html.len() + added);
    let mut cursor = 0;
    for (at, removed, text) in edits {
        let at = at.max(cursor);
        out.push_str(&html[cursor..at]);
        out.push_str(text);
        cursor = at + removed;
    }
    out.push_str(&html[cursor..]);
    out
}

/// 不分大小写找第一个 `tag`（`tag` 以 `<` 开头、全是小写 ASCII）。
fn find_tag(html: &str, tag: &str) -> Option<usize> {
    html.match_indices('<')
        .map(|(at, _)| at)
        .find(|&at| tag_at(html, at, tag))
}

fn rfind_tag(html: &str, tag: &str) -> Option<usize> {
    html.rmatch_indices('<')
        .map(|(at, _)| at)
        .find(|&at| tag_at(html, at, tag))
}

fn tag_at(html: &str, at: usize, tag: &str) -> bool {
    html.as_bytes()
        .get(at..at + tag.len())
        .is_some_and(|bytes| bytes.eq_ignore_ascii_case(tag.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn island(shown: impl Fn(&str) -> bool) -> String {
        nav_island_html(shown, |key| format!("[{key}]"), None)
    }

    #[test]
    fn items_come_from_the_shared_contract() {
        let ids: Vec<&str> = nav_island_items()
            .iter()
            .map(|item| item.id.as_str())
            .collect();
        assert_eq!(ids, ["main", "library", "phantasi", "tapp"]);
    }

    #[test]
    fn blog_item_is_current_and_hidden_modules_are_left_out() {
        let html = island(|module| module != "library");
        assert!(html.contains("href=\"/blog/\" aria-label=\"[phantasiReading]\" data-tip=\"[phantasiReading]\" aria-current=\"page\""));
        assert!(html.contains("href=\"/\" aria-label=\"[backToHome]\" data-tip=\"[home]\">"));
        assert!(!html.contains("href=\"/library\""));
        assert!(html.contains("aria-label=\"[mainNavigation]\""));
        assert_eq!(html.matches("aria-current").count(), 1);
        assert_eq!(html.matches("myriad-nav__mark").count(), 1);
        // 图标的换页名字和主站 NavigationIsland 给的一样，按条目 id 取。
        assert!(html.contains("view-transition-name: myriad-nav-icon-main\""));
        assert!(html.contains("view-transition-name: myriad-nav-icon-phantasi\""));
    }

    #[test]
    fn text_is_escaped() {
        let html = nav_island_html(|_| true, |_| "a\"<b>".into(), None);
        assert!(html.contains("aria-label=\"a&quot;&lt;b&gt;\""));
        assert!(!html.contains("a\"<b>"));
    }

    #[test]
    fn head_goes_before_head_end_and_island_right_after_body_open() {
        let html =
            "<html><head><title>t</title></head><body class=\"x\"><main>m</main></body></html>";
        let out = inject_nav_island(html, "<H>", "<I>");
        assert_eq!(
            out,
            "<html><head><title>t</title><H></head><body class=\"x\"><I><main>m</main><div class=\"myriad-nav-room\" aria-hidden=\"true\"></div></body></html>"
        );
    }

    #[test]
    fn missing_tags_still_get_both_parts() {
        assert_eq!(
            inject_nav_island("<p>x</p>", "<H>", "<I>"),
            format!("<H><I><p>x</p>{NAV_ISLAND_ROOM}")
        );
        assert_eq!(
            inject_nav_island("<BODY><p>x</p>", "<H>", "<I>"),
            format!("<BODY><H><I><p>x</p>{NAV_ISLAND_ROOM}")
        );
    }

    #[test]
    fn one_pass_matches_inserting_each_part_in_turn() {
        let studio = "<S>";
        for html in [
            "<html><head><title>t</title></head><body class=\"x\"><main>m</main></body></html>",
            "<HTML><HEAD></HEAD><BODY>x<iframe class=\"myriad-companion__frame\" data-myriad-src=\"/a\"></iframe></BODY></HTML>",
            "<body></body>",
            "<p>x</p></body>",
            "</body><head></head><body>y",
            "<p>x</p>",
            "",
        ] {
            let stepwise = crate::studio::inject_studio_island(
                &crate::reveal_companion_island(&inject_nav_island_stepwise(html, "<H>", "<I>")),
                studio,
            );
            assert_eq!(
                decorate_page(html, "<H>", "<I>", Some(studio)),
                stepwise,
                "{html}"
            );
            assert_eq!(
                decorate_page(html, "<H>", "<I>", None),
                inject_nav_island_stepwise(html, "<H>", "<I>"),
                "{html}"
            );
        }
    }

    /// 改成一次拼接之前的做法，留着对照。
    fn inject_nav_island_stepwise(html: &str, head: &str, island: &str) -> String {
        let lower = html.to_ascii_lowercase();
        let head_at = lower.find("</head>");
        let body_at = lower
            .find("<body")
            .and_then(|start| lower[start..].find('>').map(|end| start + end + 1));
        let out = match (head_at, body_at) {
            (Some(head_at), Some(body_at)) if head_at < body_at => format!(
                "{}{head}{}{island}{}",
                &html[..head_at],
                &html[head_at..body_at],
                &html[body_at..]
            ),
            (_, Some(body_at)) => format!("{}{head}{island}{}", &html[..body_at], &html[body_at..]),
            _ => format!("{head}{island}{html}"),
        };
        crate::studio::inject_studio_island(&out, NAV_ISLAND_ROOM)
    }

    #[test]
    fn speculation_rules_are_json() {
        let rules: serde_json::Value = serde_json::from_str(NAV_SPECULATION_RULES).unwrap();
        assert_eq!(rules["prerender"][0]["eagerness"], "moderate");
    }

    #[test]
    fn site_chrome_takes_only_the_exact_format() {
        let chrome = SiteChrome::parse("dark.dark.6B7280.c7d2fe").unwrap();
        let html = nav_island_html(|_| true, |key| key.into(), Some(&chrome));
        assert!(html.contains(
            "aria-label=\"mainNavigation\" data-scheme=\"dark\" style=\"--myriad-nav-primary: #6b7280; --myriad-nav-accent: #c7d2fe\">"
        ));
        let auto = SiteChrome::parse("auto.light.6b7280.4b5563").unwrap();
        assert_eq!(
            auto.nav_attrs(),
            " style=\"--myriad-nav-primary: #6b7280; --myriad-nav-accent: light-dark(#4b5563, #cbd5e1)\""
        );
        for bad in [
            "",
            "auto.light.6b7280",
            "auto.light.6b7280.4b5563.x",
            "dim.light.6b7280.4b5563",
            "auto.auto.6b7280.4b5563",
            "auto.light.6b728.4b5563",
            "auto.light.6b7280;}.4b5563",
            "auto.light.#6b728.4b5563",
            "auto.light.gggggg.4b5563",
        ] {
            assert_eq!(SiteChrome::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn head_waits_for_the_island_and_carries_both_transition_halves() {
        let head = nav_island_head();
        assert!(head.contains("<link rel=\"expect\" href=\"#myriad-nav\" blocking=\"render\">"));
        assert!(head.contains("@view-transition"));
        assert!(head.contains("view-transition-name: myriad-blog"));
    }
}
