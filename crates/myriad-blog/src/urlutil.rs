//! 地址判断。只看字符串，不发请求。

/// 以单个 `/` 开头的地址改成 `{origin}` 绝对地址。
/// `//`、`#`、已经带协议的地址不动。`origin` 为空时也不改，避免写出残缺地址。
pub fn absolutize_root_url(url: &str, origin: &str) -> String {
    let trimmed = url.trim();
    if !is_root_relative(trimmed) {
        return url.to_string();
    }
    let origin = origin.trim().trim_end_matches('/');
    if origin.is_empty() {
        return url.to_string();
    }
    format!("{origin}{trimmed}")
}

pub(crate) fn rewrite_srcset(value: &str, origin: &str) -> String {
    let mut parts = Vec::new();
    for candidate in value.split(',') {
        let candidate = candidate.trim();
        if candidate.is_empty() {
            continue;
        }
        let mut tokens = candidate.split_whitespace();
        let Some(url) = tokens.next() else {
            continue;
        };
        let url = absolutize_root_url(url, origin);
        let desc: Vec<&str> = tokens.collect();
        if desc.is_empty() {
            parts.push(url);
        } else {
            parts.push(format!("{url} {}", desc.join(" ")));
        }
    }
    parts.join(", ")
}

pub(crate) fn is_javascript_url(value: &str) -> bool {
    let compact: String = value
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .collect();
    compact
        .get(.."javascript:".len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("javascript:"))
}

pub(crate) fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

fn is_root_relative(url: &str) -> bool {
    url.starts_with('/') && !url.starts_with("//")
}

/// 查询参数里的一个值。和 `encodeURIComponent` 留的字符一样，嵌入页用 `URLSearchParams` 读回原样。
pub(crate) fn encode_query_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}
