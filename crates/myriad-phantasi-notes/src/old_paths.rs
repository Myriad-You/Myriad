//! 从别的博客搬来的文章的旧地址。站长填的和访客请求的都规范成同一个样子再比：
//! 只留路径、去掉结尾斜杠、百分号编码统一（能解成 UTF-8 的就解开）。
//!
//! 只做字符串规则，不读库。同一条旧地址不能被两篇文章占用，那一条由写库的地方查。

/// 每篇最多几条旧地址。
pub const MAX_OLD_PATHS: usize = 20;
/// 规范化后每条最多多少字节。
pub const MAX_OLD_PATH_BYTES: usize = 512;
/// 规范化之前的输入上限。再长的就不是地址了，不往下解。
const MAX_RAW_BYTES: usize = 4096;

/// 站内自己用的第一段路径。旧地址落在这些下面会把站点自己的页面或接口跳走。
/// 比较时不分大小写。
const RESERVED_FIRST_SEGMENTS: &[&str] = &[
    "api",
    "health",
    "healthz",
    "ready",
    "internal",
    "_updater",
    "_proxy",
    "sitemap.xml",
    "robots.txt",
    "llms.txt",
    ".well-known",
    "nodeinfo",
    "inbox",
    "users",
    "media",
    "phantasi",
    "assets",
    "embed",
    "library",
    "studio",
    "config",
    "agent",
    "login",
    "register",
    "setup",
    "tapp",
];

/// 旧地址为什么不收。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OldPathError {
    /// 不是一条能用的路径（空的、带 `..`、带控制字符）。
    Invalid,
    /// 和站内地址撞车：根路径、`/blog/posts/` 下、接口和站点页面。
    Reserved,
    /// 超出 [`MAX_OLD_PATH_BYTES`]。
    TooLong,
    /// 超出 [`MAX_OLD_PATHS`]。
    TooMany,
}

/// 哪一条不收、为什么。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OldPathRejection {
    pub error: OldPathError,
    pub entry: String,
}

impl OldPathRejection {
    /// 给站长看的一句话。
    pub fn message(&self) -> String {
        let entry: String = self.entry.chars().take(80).collect();
        match self.error {
            OldPathError::Invalid => format!("\"{entry}\" is not a valid old address"),
            OldPathError::Reserved => {
                format!("\"{entry}\" is an address this site uses itself")
            }
            OldPathError::TooLong => format!(
                "\"{entry}\" is too long for an old address (at most {MAX_OLD_PATH_BYTES} bytes)"
            ),
            OldPathError::TooMany => {
                format!("A note can have at most {MAX_OLD_PATHS} old addresses")
            }
        }
    }
}

/// 站长填的一条旧地址：完整网址或路径都行，只取路径。站内地址不收。
pub fn normalize_old_path(raw: &str) -> Result<String, OldPathError> {
    let path = canonical_path(raw)?;
    if is_reserved_old_path(&path) {
        return Err(OldPathError::Reserved);
    }
    Ok(path)
}

/// 站长填的整组旧地址。空行跳过，重复的只留一条，顺序照填的。
pub fn normalize_old_paths<S: AsRef<str>>(list: &[S]) -> Result<Vec<String>, OldPathRejection> {
    let mut out: Vec<String> = Vec::new();
    for raw in list {
        let raw = raw.as_ref();
        if raw.trim().is_empty() {
            continue;
        }
        let path = normalize_old_path(raw).map_err(|error| OldPathRejection {
            error,
            entry: raw.trim().to_string(),
        })?;
        if !out.contains(&path) {
            out.push(path);
        }
    }
    if out.len() > MAX_OLD_PATHS {
        return Err(OldPathRejection {
            error: OldPathError::TooMany,
            entry: String::new(),
        });
    }
    Ok(out)
}

/// 访客请求的路径按同样的规则规范化，拿去查表。站内地址照样能算出来，只是表里不会有。
pub fn old_path_lookup_key(path: &str) -> Option<String> {
    canonical_path(path).ok()
}

/// 站内自己用的地址。`path` 是规范化之后的。
pub fn is_reserved_old_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    if lower == "/"
        || lower == "/blog"
        || lower == "/blog/posts"
        || lower.starts_with("/blog/posts/")
    {
        return true;
    }
    let first = lower
        .trim_start_matches('/')
        .split('/')
        .next()
        .unwrap_or_default();
    RESERVED_FIRST_SEGMENTS.contains(&first)
}

fn canonical_path(raw: &str) -> Result<String, OldPathError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(OldPathError::Invalid);
    }
    if raw.len() > MAX_RAW_BYTES {
        return Err(OldPathError::TooLong);
    }
    let rest = strip_origin(raw);
    let rest = rest.split(['?', '#']).next().unwrap_or_default();
    if rest.contains('\\') || rest.chars().any(char::is_control) {
        return Err(OldPathError::Invalid);
    }
    let bytes = percent_decode(rest)?;
    // 解不成 UTF-8 的（比如按 GBK 编码的旧地址）把高位字节编回去：两边都走这一步，照样对得上。
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(error) => encode_high_bytes(&error.into_bytes()),
    };
    let mut out = String::with_capacity(text.len() + 1);
    for segment in text.split('/').filter(|segment| !segment.is_empty()) {
        if segment == "." || segment == ".." {
            return Err(OldPathError::Invalid);
        }
        out.push('/');
        out.push_str(segment);
    }
    if out.is_empty() {
        out.push('/');
    }
    if out.len() > MAX_OLD_PATH_BYTES {
        return Err(OldPathError::TooLong);
    }
    Ok(out)
}

/// `https://old.example/a/b` → `/a/b`；`//old.example/a` 同样。不带协议的当路径。
fn strip_origin(raw: &str) -> &str {
    let lower_prefix = raw
        .get(..8)
        .map(str::to_ascii_lowercase)
        .unwrap_or_else(|| raw.to_ascii_lowercase());
    let after_scheme = if lower_prefix.starts_with("https://") {
        &raw[8..]
    } else if lower_prefix.starts_with("http://") {
        &raw[7..]
    } else if let Some(rest) = raw.strip_prefix("//") {
        rest
    } else {
        return raw;
    };
    match after_scheme.find(['/', '?', '#']) {
        Some(at) => &after_scheme[at..],
        None => "",
    }
}

/// 解开百分号编码。解出来是 `/ ? # %` 的保持编码（大写），免得改变路径的结构；
/// 解出控制字符的不收；落单的 `%` 记成 `%25`。
fn percent_decode(text: &str) -> Result<Vec<u8>, OldPathError> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'%' {
            let hex = bytes
                .get(index + 1..index + 3)
                .filter(|pair| pair.iter().all(u8::is_ascii_hexdigit))
                .and_then(|pair| std::str::from_utf8(pair).ok())
                .and_then(|pair| u8::from_str_radix(pair, 16).ok());
            match hex {
                Some(value) if matches!(value, b'/' | b'?' | b'#' | b'%') => {
                    out.extend_from_slice(format!("%{value:02X}").as_bytes());
                    index += 3;
                }
                Some(value) if value < 0x20 || value == 0x7f => {
                    return Err(OldPathError::Invalid);
                }
                Some(value) => {
                    out.push(value);
                    index += 3;
                }
                None => {
                    out.extend_from_slice(b"%25");
                    index += 1;
                }
            }
            continue;
        }
        out.push(byte);
        index += 1;
    }
    Ok(out)
}

fn encode_high_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 3);
    for &byte in bytes {
        if byte < 0x80 {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_urls_and_paths_meet_in_one_form() {
        for raw in [
            "/2024/05/hello/",
            "/2024/05/hello",
            "2024/05/hello/",
            "https://old.example/2024/05/hello/",
            "HTTP://old.example/2024/05/hello?replytocom=3#c",
            "//old.example/2024//05/hello/",
            "  /2024/05/hello/  ",
        ] {
            assert_eq!(
                normalize_old_path(raw).as_deref(),
                Ok("/2024/05/hello"),
                "{raw}"
            );
        }
        assert_eq!(
            old_path_lookup_key("/2024/05/hello/").as_deref(),
            Some("/2024/05/hello")
        );
    }

    #[test]
    fn percent_encoding_is_unified() {
        let typed = normalize_old_path("/posts/你好-世界/").unwrap();
        assert_eq!(typed, "/posts/你好-世界");
        assert_eq!(
            old_path_lookup_key("/posts/%E4%BD%A0%E5%A5%BD-%e4%b8%96%e7%95%8c/").as_deref(),
            Some(typed.as_str())
        );
        assert_eq!(
            normalize_old_path("https://old.example/posts/%E4%BD%A0%E5%A5%BD-%E4%B8%96%E7%95%8C/"),
            Ok(typed)
        );
        // 编码过的斜杠不变成路径分隔；十六进制统一大写。
        assert_eq!(normalize_old_path("/a%2fb").as_deref(), Ok("/a%2Fb"));
        assert_eq!(normalize_old_path("/a%20b").as_deref(), Ok("/a b"));
        assert_eq!(normalize_old_path("/100%").as_deref(), Ok("/100%25"));
        assert_eq!(normalize_old_path("/a%+1").as_deref(), Ok("/a%25+1"));
        // 不是 UTF-8 的编码（GBK 的「你好」）两边都编回同一个样子。
        assert_eq!(
            normalize_old_path("/%c4%e3%ba%c3/").as_deref(),
            Ok("/%C4%E3%BA%C3")
        );
        assert_eq!(
            old_path_lookup_key("/%C4%E3%BA%C3").as_deref(),
            Some("/%C4%E3%BA%C3")
        );
    }

    #[test]
    fn site_addresses_are_refused() {
        for raw in [
            "/",
            "https://old.example",
            "https://old.example/",
            "/api/blog/redirect",
            "/API/x",
            "/health",
            "/blog",
            "/blog/",
            "/blog/posts/hello/",
            "/Blog/Posts/x",
            "/library",
            "/tapp/run/x",
            "/users/me",
            "/.well-known/webfinger",
            "/media/assets/a",
        ] {
            assert_eq!(
                normalize_old_path(raw),
                Err(OldPathError::Reserved),
                "{raw}"
            );
        }
        // 老博客本来就在 /blog/ 下的照样能填。
        assert_eq!(
            normalize_old_path("/blog/2024/05/hello/").as_deref(),
            Ok("/blog/2024/05/hello")
        );
        assert_eq!(normalize_old_path("/apis/x").as_deref(), Ok("/apis/x"));
    }

    #[test]
    fn junk_and_oversized_entries_are_refused() {
        assert_eq!(normalize_old_path("   "), Err(OldPathError::Invalid));
        assert_eq!(normalize_old_path("/a/../b"), Err(OldPathError::Invalid));
        assert_eq!(normalize_old_path("/a\\b"), Err(OldPathError::Invalid));
        assert_eq!(normalize_old_path("/a%0Ab"), Err(OldPathError::Invalid));
        let long = format!("/{}", "a".repeat(MAX_OLD_PATH_BYTES));
        assert_eq!(normalize_old_path(&long), Err(OldPathError::TooLong));
        let fits = format!("/{}", "a".repeat(MAX_OLD_PATH_BYTES - 1));
        assert!(normalize_old_path(&fits).is_ok());
    }

    #[test]
    fn lists_skip_blanks_dedupe_and_cap() {
        let list = normalize_old_paths(&["/a/", "", "  ", "/a", "https://x.test/b"]).unwrap();
        assert_eq!(list, vec!["/a".to_string(), "/b".to_string()]);
        let many: Vec<String> = (0..=MAX_OLD_PATHS).map(|i| format!("/p{i}")).collect();
        assert_eq!(
            normalize_old_paths(&many).unwrap_err().error,
            OldPathError::TooMany
        );
        assert_eq!(
            normalize_old_paths(&many[..MAX_OLD_PATHS]).unwrap().len(),
            MAX_OLD_PATHS
        );
        let bad = normalize_old_paths(&["/ok", "/api/x"]).unwrap_err();
        assert_eq!(bad.error, OldPathError::Reserved);
        assert_eq!(bad.entry, "/api/x");
        assert!(bad.message().contains("/api/x"));
    }
}
