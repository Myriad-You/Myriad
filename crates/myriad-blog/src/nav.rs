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

/// 导航岛本身。`shown` 按模块键判断访客能不能看到这一项；`text` 按文案键取字，
/// 除了条目的键还会问 `mainNavigation`。
pub fn nav_island_html(shown: impl Fn(&str) -> bool, text: impl Fn(&str) -> String) -> String {
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
        "<nav id=\"{NAV_ISLAND_ID}\" class=\"myriad-nav\" aria-label=\"{}\"><div class=\"myriad-nav__island\">{links}</div></nav>",
        escape_attr(&text("mainNavigation")),
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
        nav_island_html(shown, |key| format!("[{key}]"))
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
        let html = nav_island_html(|_| true, |_| "a\"<b>".into());
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
    fn head_waits_for_the_island_and_carries_both_transition_halves() {
        let head = nav_island_head();
        assert!(head.contains("<link rel=\"expect\" href=\"#myriad-nav\" blocking=\"render\">"));
        assert!(head.contains("@view-transition"));
        assert!(head.contains("view-transition-name: myriad-blog"));
    }
}
