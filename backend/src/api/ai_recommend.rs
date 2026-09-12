use axum::Json;
use rand::RngExt;
use serde::{Deserialize, Serialize};

use crate::error::HttpError;

#[derive(Debug, Deserialize)]
pub struct IconRecommendRequest {
    pub platform_name: String,
}

#[derive(Debug, Serialize)]
pub struct IconRecommendResponse {
    pub icon_type: String,            // always "react-icons"
    pub icon_library: Option<String>, // "fa" / "si" / "fa6"
    pub icon_name: Option<String>,    // e.g. SiSinaweibo (weibo is not FaWeibo)
    pub icon_url: Option<String>,     // always None
    pub color_suggestion: String,     // 建议的主题色
    pub url_pattern: Option<String>,  // URL模式建议，如 "https://weibo.com/u/{username}"
}

/// Rule-match a react-icon from the platform name (`contains`, first hit).
/// No LLM. No URL fallback.
pub async fn recommend_icon(
    Json(payload): Json<IconRecommendRequest>,
) -> Result<Json<IconRecommendResponse>, HttpError> {
    let platform_name = payload.platform_name.to_lowercase();

    // 规则匹配（`match_platform_icon`）
    let recommendation = match_platform_icon(&platform_name);

    Ok(Json(recommendation))
}

/// 平台图标匹配规则
fn match_platform_icon(platform: &str) -> IconRecommendResponse {
    // Mixed Si*/Fa* table (not si-only). Order is load-bearing (`contains`).
    // 注意：匹配顺序很重要！更具体的关键词（如 "xbox"）必须放在更通用的关键词（如 "x"）之前
    // 因为匹配使用的是 contains() 方法
    let social_platforms: Vec<(&str, &str, &str, &str)> = vec![
        // (关键词, 图标名, 颜色, URL模式)
        (
            "微博",
            "SiSinaweibo",
            "#E6162D",
            "https://weibo.com/u/{username}",
        ),
        (
            "weibo",
            "SiSinaweibo",
            "#E6162D",
            "https://weibo.com/u/{username}",
        ),
        (
            "twitter",
            "SiTwitter",
            "#1DA1F2",
            "https://twitter.com/{username}",
        ),
        // Xbox 必须放在 X 之前，否则 "xbox" 会被 "x" 匹配
        (
            "xbox",
            "FaXbox",
            "#107C10",
            "https://account.xbox.com/profile?gamertag={username}",
        ),
        ("x", "SiX", "#000000", "https://x.com/{username}"),
        (
            "facebook",
            "SiFacebook",
            "#1877F2",
            "https://facebook.com/{username}",
        ),
        (
            "instagram",
            "SiInstagram",
            "#E4405F",
            "https://instagram.com/{username}",
        ),
        (
            "youtube",
            "SiYoutube",
            "#FF0000",
            "https://youtube.com/@{username}",
        ),
        (
            "tiktok",
            "SiTiktok",
            "#000000",
            "https://tiktok.com/@{username}",
        ),
        (
            "抖音",
            "SiTiktok",
            "#000000",
            "https://www.douyin.com/user/{username}",
        ),
        (
            "linkedin",
            "FaLinkedin",
            "#0A66C2",
            "https://linkedin.com/in/{username}",
        ),
        (
            "reddit",
            "SiReddit",
            "#FF4500",
            "https://reddit.com/user/{username}",
        ),
        (
            "discord",
            "SiDiscord",
            "#5865F2",
            "https://discord.gg/{username}",
        ),
        (
            "telegram",
            "SiTelegram",
            "#26A5E4",
            "https://t.me/{username}",
        ),
        (
            "whatsapp",
            "SiWhatsapp",
            "#25D366",
            "https://wa.me/{username}",
        ),
        (
            "snapchat",
            "SiSnapchat",
            "#FFFC00",
            "https://snapchat.com/add/{username}",
        ),
        (
            "pinterest",
            "SiPinterest",
            "#E60023",
            "https://pinterest.com/{username}",
        ),
        (
            "twitch",
            "SiTwitch",
            "#9146FF",
            "https://twitch.tv/{username}",
        ),
        (
            "spotify",
            "SiSpotify",
            "#1DB954",
            "https://open.spotify.com/user/{username}",
        ),
        ("apple", "SiApple", "#000000", ""),
        ("google", "SiGoogle", "#4285F4", ""),
        ("microsoft", "SiMicrosoft", "#5E5E5E", ""),
        (
            "amazon",
            "FaAmazon",
            "#FF9900",
            "https://amazon.com/shop/{username}",
        ),
        (
            "douban",
            "SiDouban",
            "#007722",
            "https://www.douban.com/people/{username}",
        ),
        (
            "豆瓣",
            "SiDouban",
            "#007722",
            "https://www.douban.com/people/{username}",
        ),
        (
            "zhihu",
            "SiZhihu",
            "#0084FF",
            "https://www.zhihu.com/people/{username}",
        ),
        (
            "知乎",
            "SiZhihu",
            "#0084FF",
            "https://www.zhihu.com/people/{username}",
        ),
        ("qq", "SiQq", "#12B7F5", ""),     // QQ蓝色
        ("腾讯qq", "SiQq", "#12B7F5", ""), // QQ蓝色
        ("wechat", "SiWechat", "#07C160", ""),
        ("微信", "SiWechat", "#07C160", ""),
        ("baidu", "SiBaidu", "#2932E1", ""), // 百度蓝色
        ("百度", "SiBaidu", "#2932E1", ""),  // 百度蓝色
        (
            "xiaohongshu",
            "SiXiaohongshu",
            "#FF2442",
            "https://www.xiaohongshu.com/user/profile/{username}",
        ),
        (
            "小红书",
            "SiXiaohongshu",
            "#FF2442",
            "https://www.xiaohongshu.com/user/profile/{username}",
        ),
        (
            "gitlab",
            "SiGitlab",
            "#FC6D26",
            "https://gitlab.com/{username}",
        ),
        (
            "bitbucket",
            "SiBitbucket",
            "#0052CC",
            "https://bitbucket.org/{username}",
        ),
        (
            "codepen",
            "SiCodepen",
            "#000000",
            "https://codepen.io/{username}",
        ),
        (
            "dribbble",
            "SiDribbble",
            "#EA4C89",
            "https://dribbble.com/{username}",
        ),
        (
            "behance",
            "SiBehance",
            "#1769FF",
            "https://behance.net/{username}",
        ),
        (
            "deviantart",
            "SiDeviantart",
            "#05CC47",
            "https://deviantart.com/{username}",
        ),
        (
            "medium",
            "SiMedium",
            "#000000",
            "https://medium.com/@{username}",
        ),
        (
            "substack",
            "SiSubstack",
            "#FF6719",
            "https://{username}.substack.com",
        ),
        (
            "patreon",
            "SiPatreon",
            "#FF424D",
            "https://patreon.com/{username}",
        ),
        ("ko-fi", "SiKofi", "#FF5E5B", "https://ko-fi.com/{username}"),
        (
            "buymeacoffee",
            "SiBuymeacoffee",
            "#FFDD00",
            "https://buymeacoffee.com/{username}",
        ),
        (
            "mastodon",
            "SiMastodon",
            "#6364FF",
            "https://mastodon.social/@{username}",
        ),
        (
            "threads",
            "SiThreads",
            "#000000",
            "https://threads.net/@{username}",
        ),
        (
            "bluesky",
            "SiBluesky",
            "#0085FF",
            "https://bsky.app/profile/{username}",
        ),
        (
            "misskey",
            "SiMisskey",
            "#86B300",
            "https://misskey.io/@{username}",
        ),
        (
            "soundcloud",
            "SiSoundcloud",
            "#FF5500",
            "https://soundcloud.com/{username}",
        ),
        (
            "bandcamp",
            "SiBandcamp",
            "#629AA9",
            "https://{username}.bandcamp.com",
        ),
        (
            "itch.io",
            "SiItchdotio",
            "#FA5C5C",
            "https://{username}.itch.io",
        ),
        ("playstation", "SiPlaystation", "#003791", ""),
        ("nintendo", "SiNintendoswitch", "#E60012", ""),
        ("epic games", "SiEpicgames", "#313131", ""),
        ("origin", "SiOrigin", "#F56C2D", ""),
        (
            "anilist",
            "SiAnilist",
            "#02A9FF",
            "https://anilist.co/user/{username}",
        ),
        (
            "myanimelist",
            "SiMyanimelist",
            "#2E51A2",
            "https://myanimelist.net/profile/{username}",
        ),
        (
            "letterboxd",
            "SiLetterboxd",
            "#00D735",
            "https://letterboxd.com/{username}",
        ),
        (
            "trakt",
            "SiTrakt",
            "#ED1C24",
            "https://trakt.tv/users/{username}",
        ),
        (
            "goodreads",
            "SiGoodreads",
            "#553B08",
            "https://goodreads.com/{username}",
        ),
        (
            "lastfm",
            "SiLastdotfm",
            "#D51007",
            "https://last.fm/user/{username}",
        ),
        (
            "last.fm",
            "SiLastdotfm",
            "#D51007",
            "https://last.fm/user/{username}",
        ),
        (
            "pixiv",
            "SiPixiv",
            "#0096FA",
            "https://pixiv.net/users/{username}",
        ),
        (
            "artstation",
            "SiArtstation",
            "#13AFF0",
            "https://artstation.com/{username}",
        ),
        (
            "flickr",
            "SiFlickr",
            "#0063DC",
            "https://flickr.com/people/{username}",
        ),
        (
            "500px",
            "Si500px",
            "#0099E5",
            "https://500px.com/p/{username}",
        ),
        (
            "unsplash",
            "SiUnsplash",
            "#000000",
            "https://unsplash.com/@{username}",
        ),
        (
            "producthunt",
            "SiProducthunt",
            "#DA552F",
            "https://producthunt.com/@{username}",
        ),
        (
            "hackernews",
            "SiYcombinator",
            "#FF6600",
            "https://news.ycombinator.com/user?id={username}",
        ),
        (
            "stackoverflow",
            "SiStackoverflow",
            "#F58025",
            "https://stackoverflow.com/users/{username}",
        ),
        (
            "dev.to",
            "SiDevdotto",
            "#0A0A0A",
            "https://dev.to/{username}",
        ),
        (
            "hashnode",
            "SiHashnode",
            "#2962FF",
            "https://{username}.hashnode.dev",
        ),
        (
            "kaggle",
            "SiKaggle",
            "#20BEFF",
            "https://kaggle.com/{username}",
        ),
        (
            "huggingface",
            "SiHuggingface",
            "#FFD21E",
            "https://huggingface.co/{username}",
        ),
        (
            "figma",
            "SiFigma",
            "#F24E1E",
            "https://figma.com/@{username}",
        ),
        (
            "notion",
            "SiNotion",
            "#000000",
            "https://notion.so/{username}",
        ),
        (
            "afdian",
            "FaCoffee",
            "#946CE6",
            "https://afdian.com/a/{username}",
        ),
        (
            "爱发电",
            "FaCoffee",
            "#946CE6",
            "https://afdian.com/a/{username}",
        ),
        (
            "acfun",
            "FaPlayCircle",
            "#FD4C5D",
            "https://www.acfun.cn/u/{username}",
        ),
        (
            "a站",
            "FaPlayCircle",
            "#FD4C5D",
            "https://www.acfun.cn/u/{username}",
        ),
        (
            "niconico",
            "SiNiconico",
            "#231F20",
            "https://www.nicovideo.jp/user/{username}",
        ),
        (
            "fc2",
            "FaGlobe",
            "#FF6600",
            "https://{username}.blog.fc2.com",
        ),
        (
            "line",
            "SiLine",
            "#00B900",
            "https://line.me/ti/p/{username}",
        ),
        ("kakao", "SiKakaotalk", "#FFCD00", ""),
        (
            "naver",
            "SiNaver",
            "#03C75A",
            "https://blog.naver.com/{username}",
        ),
        ("vk", "SiVk", "#4C75A3", "https://vk.com/{username}"),
        (
            "ok",
            "SiOdnoklassniki",
            "#EE8208",
            "https://ok.ru/profile/{username}",
        ),
        (
            "tumblr",
            "SiTumblr",
            "#36465D",
            "https://{username}.tumblr.com",
        ),
        (
            "wordpress",
            "SiWordpress",
            "#21759B",
            "https://{username}.wordpress.com",
        ),
        (
            "blogger",
            "SiBlogger",
            "#FF5722",
            "https://{username}.blogspot.com",
        ),
        (
            "lofter",
            "FaBlog",
            "#2B5F82",
            "https://{username}.lofter.com",
        ),
        (
            "网易lofter",
            "FaBlog",
            "#2B5F82",
            "https://{username}.lofter.com",
        ),
        (
            "v2ex",
            "FaComments",
            "#0A0A0A",
            "https://v2ex.com/member/{username}",
        ),
        (
            "nga",
            "FaComments",
            "#6B6B6B",
            "https://bbs.nga.cn/nuke.php?func=ucp&uid={username}",
        ),
        (
            "贴吧",
            "SiBaidu",
            "#4879BD",
            "https://tieba.baidu.com/home/main?id={username}",
        ),
        (
            "tieba",
            "SiBaidu",
            "#4879BD",
            "https://tieba.baidu.com/home/main?id={username}",
        ),
        ("alipay", "FaAlipay", "#1677FF", ""),
        ("支付宝", "FaAlipay", "#1677FF", ""),
    ];

    // 检查是否匹配社交平台
    for (name, icon, color, url_pattern) in &social_platforms {
        if platform.contains(name) {
            // 根据图标名称前缀自动判断图标库
            let icon_library = if icon.starts_with("Si") {
                "si"
            } else if icon.starts_with("Fa6") {
                "fa6"
            } else {
                "fa"
            };

            return IconRecommendResponse {
                icon_type: "react-icons".to_string(),
                icon_library: Some(icon_library.to_string()),
                icon_name: Some(icon.to_string()),
                icon_url: None,
                color_suggestion: color.to_string(),
                url_pattern: if url_pattern.is_empty() {
                    None
                } else {
                    Some(url_pattern.to_string())
                },
            };
        }
    }

    // Second table: mixed fa / si / fa6 (not Font Awesome only).
    // 元组格式: (关键词, 图标名, 颜色, URL模式, 图标库)
    let fa_keywords: Vec<(&str, &str, &str, &str, &str)> = vec![
        ("音乐", "FaMusic", "#FF6B6B", "", "fa"),
        ("music", "FaMusic", "#FF6B6B", "", "fa"),
        ("视频", "FaVideo", "#4ECDC4", "", "fa"),
        ("video", "FaVideo", "#4ECDC4", "", "fa"),
        ("直播", "FaBroadcastTower", "#F7B731", "", "fa"),
        ("live", "FaBroadcastTower", "#F7B731", "", "fa"),
        ("博客", "FaGrip", "var(--color-primary)", "", "fa6"),
        ("blog", "FaGrip", "var(--color-primary)", "", "fa6"),
        ("论坛", "FaComments", "#0FB9B1", "", "fa"),
        ("forum", "FaComments", "#0FB9B1", "", "fa"),
        ("游戏", "FaGamepad", "#EE5A6F", "", "fa"),
        ("game", "FaGamepad", "#EE5A6F", "", "fa"),
        ("商店", "FaShoppingCart", "#F79F1F", "", "fa"),
        ("shop", "FaShoppingCart", "#F79F1F", "", "fa"),
        ("邮箱", "SiMaildotru", "#0078D4", "mailto:{username}", "si"),
        ("email", "SiMaildotru", "#0078D4", "mailto:{username}", "si"),
        ("新闻", "FaNewspaper", "#2C3E50", "", "fa"),
        ("news", "FaNewspaper", "#2C3E50", "", "fa"),
        ("照片", "FaCamera", "#FDA7DF", "", "fa"),
        ("photo", "FaCamera", "#FDA7DF", "", "fa"),
        ("书籍", "FaBook", "#C23616", "", "fa"),
        ("book", "FaBook", "#C23616", "", "fa"),
        ("教育", "FaGraduationCap", "#0652DD", "", "fa"),
        ("education", "FaGraduationCap", "#0652DD", "", "fa"),
        ("个人网站", "FaGlobe", "#6366F1", "https://{username}", "fa"),
        ("website", "FaGlobe", "#6366F1", "https://{username}", "fa"),
        ("主页", "FaHome", "#6366F1", "https://{username}", "fa"),
        ("homepage", "FaHome", "#6366F1", "https://{username}", "fa"),
    ];

    for (keyword, icon, color, url_pattern, library) in &fa_keywords {
        if platform.contains(keyword) {
            return IconRecommendResponse {
                icon_type: "react-icons".to_string(),
                icon_library: Some(library.to_string()),
                icon_name: Some(icon.to_string()),
                icon_url: None,
                color_suggestion: color.to_string(),
                url_pattern: if url_pattern.is_empty() {
                    None
                } else {
                    Some(url_pattern.to_string())
                },
            };
        }
    }

    // 未知平台：返回随机图标和随机颜色
    let random_icons = [
        "FaGlobe",
        "FaStar",
        "FaHeart",
        "FaRocket",
        "FaBolt",
        "FaGem",
        "FaCrown",
        "FaFeather",
        "FaLeaf",
        "FaPaperPlane",
        "FaCompass",
        "FaAnchor",
        "FaMoon",
        "FaSun",
        "FaCloud",
        "FaFire",
        "FaSnowflake",
        "FaUmbrella",
        "FaMountain",
        "FaWater",
    ];

    let random_colors = [
        "#6366F1", "#8B5CF6", "#EC4899", "#EF4444", "#F97316", "#EAB308", "#22C55E", "#14B8A6",
        "#06B6D4", "#3B82F6", "#A855F7", "#D946EF", "#F43F5E", "#FB7185", "#34D399", "#2DD4BF",
        "#38BDF8", "#818CF8", "#C084FC", "#F472B6",
    ];

    let mut rng = rand::rng();
    let icon = random_icons[rng.random_range(0..random_icons.len())];
    let color = random_colors[rng.random_range(0..random_colors.len())];

    IconRecommendResponse {
        icon_type: "react-icons".to_string(),
        icon_library: Some("fa".to_string()),
        icon_name: Some(icon.to_string()),
        icon_url: None,
        color_suggestion: color.to_string(),
        url_pattern: None,
    }
}
