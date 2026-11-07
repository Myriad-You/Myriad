//! 一段里只有一个视频或音乐链接时，把它换成播放器。
//!
//! 笔记消毒器会剥掉手写的 iframe，所以这里是博客上唯一出现外站框的地方：地址由下面的
//! 模板拼出，链接里只取得到 id，放行的主机和 `BLOG_CSP` 的 `frame-src` 是同一份。
//! 框跑在 sandbox 里，不给 `allow-top-navigation`，播放器不能把整页带走。

use crate::urlutil::escape_html;

/// 外站播放器的主机，测试保证 `BLOG_CSP` 的 `frame-src` 和这里一致。
#[cfg(test)]
const EMBED_HOSTS: [&str; 3] = [
    "https://player.bilibili.com",
    "https://music.163.com",
    "https://www.youtube-nocookie.com",
];

struct Embed {
    title: &'static str,
    src: String,
    /// 音乐是一条窄窄的播放条，不是 16:9 的视频框。
    audio: bool,
}

/// 把只含一个可嵌入链接的段落换成播放器。别的内容一个字不动。
pub(crate) fn embed_bare_links(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(start) = rest.find("<p>") {
        let inner_start = start + "<p>".len();
        let Some(len) = rest[inner_start..].find("</p>") else {
            break;
        };
        let end = inner_start + len + "</p>".len();
        out.push_str(&rest[..start]);
        match bare_link(&rest[inner_start..inner_start + len]).and_then(|url| embed_for(&url)) {
            Some(embed) => out.push_str(&frame_html(&embed)),
            None => out.push_str(&rest[start..end]),
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// 段落的全部内容就是一个地址时返回它：纯文本，或者只包着一层 `<a>`。
fn bare_link(inner: &str) -> Option<String> {
    let inner = inner.trim();
    let url = if let Some(anchor) = inner.strip_prefix("<a ") {
        let (attrs, text) = anchor.strip_suffix("</a>")?.split_once('>')?;
        if text.contains('<') {
            return None;
        }
        let (_, href) = attrs.split_once("href=\"")?;
        href.split_once('"')?.0
    } else if inner.contains('<') || inner.contains(char::is_whitespace) {
        return None;
    } else {
        inner
    };
    Some(url.replace("&amp;", "&"))
}

fn embed_for(url: &str) -> Option<Embed> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let (host, path) = rest.split_once('/')?;
    // 网易云的网页地址把路由放在 `#` 后面。
    let path = path.strip_prefix("#/").unwrap_or(path);
    let path = path.split_once('#').map_or(path, |(path, _)| path);
    let (path, query) = path.split_once('?').unwrap_or((path, ""));
    match host.to_ascii_lowercase().as_str() {
        "www.bilibili.com" | "bilibili.com" | "m.bilibili.com" => {
            let id = path.strip_prefix("video/")?.trim_end_matches('/');
            (id.starts_with("BV") && is_id(id, 16)).then(|| Embed {
                title: "Bilibili",
                src: format!("https://player.bilibili.com/player.html?bvid={id}&autoplay=0"),
                audio: false,
            })
        }
        "music.163.com" | "y.music.163.com" => {
            let id = query_value(query, "id")?;
            (path.trim_end_matches('/').ends_with("song")
                && !id.is_empty()
                && id.len() <= 20
                && id.bytes().all(|byte| byte.is_ascii_digit()))
            .then(|| Embed {
                title: "NetEase Cloud Music",
                src: format!(
                    "https://music.163.com/outchain/player?type=2&id={id}&auto=0&height=66"
                ),
                audio: true,
            })
        }
        "www.youtube.com" | "youtube.com" | "m.youtube.com" => (path.trim_end_matches('/')
            == "watch")
            .then(|| query_value(query, "v"))
            .flatten()
            .and_then(youtube),
        "youtu.be" => youtube(path.trim_end_matches('/')),
        _ => None,
    }
}

fn youtube(id: &str) -> Option<Embed> {
    (id.len() == 11 && is_id(id, 11)).then(|| Embed {
        title: "YouTube",
        src: format!("https://www.youtube-nocookie.com/embed/{id}"),
        audio: false,
    })
}

fn query_value<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    query
        .split('&')
        .find_map(|pair| pair.split_once('=').filter(|(name, _)| *name == key))
        .map(|(_, value)| value)
}

/// 只放字母、数字、`-`、`_`：id 会进地址，别的字符一律不认。
fn is_id(id: &str, max: usize) -> bool {
    !id.is_empty()
        && id.len() <= max
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn frame_html(embed: &Embed) -> String {
    let class = if embed.audio {
        "myriad-embed myriad-embed--audio"
    } else {
        "myriad-embed"
    };
    format!(
        "<div class=\"{class}\"><iframe src=\"{}\" title=\"{}\" loading=\"lazy\" \
         sandbox=\"allow-scripts allow-same-origin allow-popups allow-presentation\" \
         allow=\"fullscreen; encrypted-media; picture-in-picture\" allowfullscreen \
         referrerpolicy=\"strict-origin-when-cross-origin\"></iframe></div>",
        escape_html(&embed.src),
        embed.title,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src(html: &str) -> Option<String> {
        let out = embed_bare_links(html);
        let (_, rest) = out.split_once("<iframe src=\"")?;
        Some(rest.split_once('"')?.0.replace("&amp;", "&"))
    }

    #[test]
    fn a_paragraph_that_is_only_a_link_becomes_a_player() {
        assert_eq!(
            src("<p>https://www.bilibili.com/video/BV1GJ411x7h7/?spm_id=1</p>").as_deref(),
            Some("https://player.bilibili.com/player.html?bvid=BV1GJ411x7h7&autoplay=0")
        );
        assert_eq!(
            src("<p><a href=\"https://music.163.com/#/song?id=1824045033\" rel=\"noopener noreferrer nofollow\">歌</a></p>")
                .as_deref(),
            Some("https://music.163.com/outchain/player?type=2&id=1824045033&auto=0&height=66")
        );
        assert_eq!(
            src("<p>https://www.youtube.com/watch?t=3&amp;v=dQw4w9WgXcQ</p>").as_deref(),
            Some("https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ")
        );
        assert_eq!(
            src("<p>https://youtu.be/dQw4w9WgXcQ</p>").as_deref(),
            Some("https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ")
        );
    }

    #[test]
    fn everything_else_stays_as_written() {
        for html in [
            "<p>看这个 https://www.bilibili.com/video/BV1GJ411x7h7/</p>",
            "<p><a href=\"https://youtu.be/dQw4w9WgXcQ\"><img src=\"/a.png\"></a></p>",
            "<p>https://evil.example/video/BV1GJ411x7h7/</p>",
            "<p>https://www.bilibili.com.evil.example/video/BV1GJ411x7h7/</p>",
            "<p>https://www.bilibili.com/read/cv1</p>",
            "<pre><code>https://youtu.be/dQw4w9WgXcQ</code></pre>",
        ] {
            assert_eq!(embed_bare_links(html), html);
        }
    }

    #[test]
    fn an_id_cannot_carry_anything_into_the_address() {
        for html in [
            "<p>https://www.bilibili.com/video/BV1\"onload=x/</p>",
            "<p>https://youtu.be/dQw4w9WgX&amp;q</p>",
            "<p>https://music.163.com/song?id=1\"x</p>",
            "<p>https://music.163.com/song?id=abc</p>",
        ] {
            assert_eq!(src(html), None, "{html}");
        }
    }

    #[test]
    fn the_frame_cannot_take_the_page_away() {
        let out = embed_bare_links("<p>https://youtu.be/dQw4w9WgXcQ</p>");
        assert!(out.contains("sandbox=\""));
        assert!(!out.contains("allow-top-navigation"));
    }

    #[test]
    fn the_csp_lets_in_exactly_the_players() {
        let frames = crate::BLOG_CSP
            .split(';')
            .map(str::trim)
            .find_map(|directive| directive.strip_prefix("frame-src "))
            .unwrap();
        let hosts: Vec<&str> = frames.split(' ').filter(|host| *host != "'self'").collect();
        assert_eq!(hosts, EMBED_HOSTS);
    }
}
