
/// Minimal query-value encoding (letters, digits, -_.~ pass through).
pub(crate) fn urlencoding_lite(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for b in s.as_bytes() {
        match *b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char);
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

// X 分享文案工具（无网络）

/// 免费账号常用上限；Premium 可更长，这里作为默认安全截断阈值
pub const X_SHARE_DEFAULT_MAX_LEN: usize = 280;

/// 组装分享文案：优先 `text`，否则 `title` + `summary`，再拼 hashtags 与 url
pub fn compose_x_share_text(
    text: Option<&str>,
    title: Option<&str>,
    summary: Option<&str>,
    url: Option<&str>,
    hashtags: &[String],
    max_len: usize,
) -> String {
    let max_len = if max_len == 0 {
        X_SHARE_DEFAULT_MAX_LEN
    } else {
        max_len
    };

    let mut parts: Vec<String> = Vec::new();

    let main = text
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            let t = title.map(str::trim).filter(|s| !s.is_empty());
            let s = summary.map(str::trim).filter(|s| !s.is_empty());
            match (t, s) {
                (Some(t), Some(s)) => Some(format!("{}\n\n{}", t, s)),
                (Some(t), None) => Some(t.to_string()),
                (None, Some(s)) => Some(s.to_string()),
                (None, None) => None,
            }
        });

    if let Some(main) = main {
        parts.push(main);
    }

    let tags: Vec<String> = hashtags
        .iter()
        .map(|t| t.trim().trim_start_matches('#').trim())
        .filter(|t| !t.is_empty())
        .map(|t| format!("#{}", t.replace(' ', "")))
        .collect();
    if !tags.is_empty() {
        parts.push(tags.join(" "));
    }

    if let Some(u) = url.map(str::trim).filter(|s| !s.is_empty()) {
        parts.push(u.to_string());
    }

    let composed = parts.join("\n\n");
    truncate_x_share_text(&composed, max_len)
}

/// 按 Unicode 标量截断，避免切在组合字符中间出乱码；末尾加 …
pub fn truncate_x_share_text(text: &str, max_len: usize) -> String {
    if max_len == 0 {
        return String::new();
    }
    let count = text.chars().count();
    if count <= max_len {
        return text.to_string();
    }
    if max_len == 1 {
        return "…".to_string();
    }
    let keep = max_len - 1;
    let truncated: String = text.chars().take(keep).collect();
    format!("{}…", truncated.trim_end())
}

/// 生成 Web Intent 链接（无需 API 写权限，打开浏览器即可发帖）
pub fn build_x_intent_url(text: &str, url: Option<&str>) -> String {
    let mut params = vec![("text", text.to_string())];
    if let Some(u) = url.map(str::trim).filter(|s| !s.is_empty()) {
        // Intent 支持独立 url 参数；若 text 里已含链接也可只放 text
        params.push(("url", u.to_string()));
    }
    let query = params
        .iter()
        .map(|(k, v)| format!("{}={}", k, urlencoding::encode(v)))
        .collect::<Vec<_>>()
        .join("&");
    format!("https://x.com/intent/tweet?{}", query)
}

#[cfg(test)]
mod x_share_tests {
    use super::*;

    #[test]
    fn compose_prefers_explicit_text() {
        let s = compose_x_share_text(
            Some("  hello  "),
            Some("title"),
            Some("summary"),
            Some("https://example.com"),
            &["Myriad".into(), "#Rust".into()],
            280,
        );
        assert!(s.starts_with("hello"));
        assert!(s.contains("#Myriad"));
        assert!(s.contains("#Rust"));
        assert!(s.contains("https://example.com"));
    }

    #[test]
    fn compose_from_title_summary() {
        let s = compose_x_share_text(None, Some("周报"), Some("本周写了 X 接入"), None, &[], 280);
        assert_eq!(s, "周报\n\n本周写了 X 接入");
    }

    #[test]
    fn truncate_respects_unicode() {
        let s = truncate_x_share_text("你好世界ABC", 3);
        assert_eq!(s.chars().count(), 3);
        assert!(s.ends_with('…'));
    }

    #[test]
    fn intent_url_encodes_text() {
        let url = build_x_intent_url("hello world #test", Some("https://ex.com/a b"));
        assert!(url.starts_with("https://x.com/intent/tweet?"));
        assert!(url.contains("text=hello%20world"));
        assert!(url.contains("url=https"));
    }
}
