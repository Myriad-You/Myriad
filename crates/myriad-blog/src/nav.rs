//! 导航岛：博客页上回主站的那一条，和主站的导航岛长一个样。
//!
//! 博客页不跑脚本，所以这是一份纯 HTML 加样式，后端每次返回页面时插进去：换了主题
//! 也照样有，静态站本身的文件不变。条目（顺序、地址、模块、图标）和主站共用
//! `shared/nav_island.json`；文案由调用方按访客的语言给。
//! 主站和博客之间切换的动效，博客这一半也在这份样式里（`nav_island.css`）。

use std::sync::LazyLock;

use serde::Deserialize;

use crate::studio::{escape_attr, insert_before_body_end};

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
pub fn nav_island_head() -> String {
    format!(
        "<style id=\"{NAV_ISLAND_ID}-style\">{NAV_ISLAND_CSS}</style>\
<link rel=\"expect\" href=\"#{NAV_ISLAND_ID}\" blocking=\"render\">"
    )
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
        // 选中的底色和小圆点画在单独一块上：换页时只有它从主站原来的条目滑过来，图标不动。
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
            icon_svg(&item.icon),
        ));
    }
    format!(
        "<nav id=\"{NAV_ISLAND_ID}\" class=\"myriad-nav\" aria-label=\"{}\"{}><div class=\"myriad-nav__island\">{links}</div></nav>",
        escape_attr(&text("mainNavigation")),
        chrome.map(SiteChrome::nav_attrs).unwrap_or_default(),
    )
}

fn icon_svg(icon: &NavIcon) -> String {
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
        "<svg viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" aria-hidden=\"true\" focusable=\"false\">\
<g stroke-linecap=\"round\" stroke-linejoin=\"round\" stroke-width=\"{}\"{transform}>{paths}</g></svg>",
        icon.stroke_width
    )
}

/// `head` 插在第一个 `</head>` 前，`island` 插在 `<body>` 开始标签之后：放在最前面，
/// 第一帧之前就解析到。找不到标签就都放在最前。页尾再留一块给手机上的底栏让位。
pub fn inject_nav_island(html: &str, head: &str, island: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let head_at = lower.find("</head>");
    let body_at = lower
        .find("<body")
        .and_then(|start| lower[start..].find('>').map(|end| start + end + 1));
    let mut out = String::with_capacity(html.len() + head.len() + island.len());
    match (head_at, body_at) {
        (Some(head_at), Some(body_at)) if head_at < body_at => {
            out.push_str(&html[..head_at]);
            out.push_str(head);
            out.push_str(&html[head_at..body_at]);
            out.push_str(island);
            out.push_str(&html[body_at..]);
        }
        (_, Some(body_at)) => {
            out.push_str(&html[..body_at]);
            out.push_str(head);
            out.push_str(island);
            out.push_str(&html[body_at..]);
        }
        _ => {
            out.push_str(head);
            out.push_str(island);
            out.push_str(html);
        }
    }
    insert_before_body_end(&out, NAV_ISLAND_ROOM)
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
        assert_eq!(ids, ["main", "library", "phantasi", "reports", "tapp"]);
    }

    #[test]
    fn blog_item_is_current_and_hidden_modules_are_left_out() {
        let html = island(|module| module != "reports");
        assert!(html.contains("href=\"/blog/\" aria-label=\"[phantasiReading]\" data-tip=\"[phantasiReading]\" aria-current=\"page\""));
        assert!(html.contains("href=\"/\" aria-label=\"[backToHome]\" data-tip=\"[home]\">"));
        assert!(!html.contains("/reports"));
        assert!(html.contains("aria-label=\"[mainNavigation]\""));
        assert_eq!(html.matches("aria-current").count(), 1);
        assert_eq!(html.matches("myriad-nav__mark").count(), 1);
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
