//! Pure helpers for executor_utils_pure.

use serde_json::Value;

/// 安全截断 UTF-8 字符串到指定字节长度（不会在多字节字符中间截断）
pub fn truncate_str(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// 支持的平台名称列表
pub const VALID_PLATFORMS: &[&str] = &[
    "steam", "bilibili", "github", "youtube", "netease", "bangumi", "x", "discord", "mal", "xbox",
    "psn",
];

/// 验证平台名称是否在白名单中（含 "all"），返回 Result
pub fn validate_platform_name(platform: &str) -> Result<&str, String> {
    if platform == "all" || VALID_PLATFORMS.contains(&platform) {
        Ok(platform)
    } else {
        Err(crate::services::agent::response_agent::unsupported_platform(platform))
    }
}

/// 验证平台名称是否在白名单中（不含 "all"），返回 bool
pub fn is_valid_platform(platform: &str) -> bool {
    VALID_PLATFORMS.contains(&platform)
}

/// 简单的 Levenshtein 相似度检查（用于模糊匹配）
/// 如果两个字符串的编辑距离小于较短字符串长度的一半，认为相似
pub fn levenshtein_similar(a: &str, b: &str) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }

    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let m = a_chars.len();
    let n = b_chars.len();

    // 如果长度差距太大，直接返回不相似
    if m.abs_diff(n) > m.min(n) {
        return false;
    }

    // 防御：对超长字符串截断以防 O(m*n) 爆炸
    let m = m.min(500);
    let n = n.min(500);

    // 两行滚动 DP（O(n) 内存而非 O(m*n)）
    let mut prev = (0..=n).collect::<Vec<usize>>();
    let mut curr = vec![0usize; n + 1];

    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            let cost = if a_chars[i - 1] == b_chars[j - 1] {
                0
            } else {
                1
            };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    let distance = prev[n];
    let threshold = m.min(n) / 2;

    distance <= threshold.max(2)
}

/// How a needle matched a haystack field (exact → contains → fuzzy).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MatchKind {
    Fuzzy = 1,
    Contains = 2,
    Exact = 3,
}

impl MatchKind {
    pub fn as_str(self) -> &'static str {
        match self {
            MatchKind::Exact => "exact",
            MatchKind::Contains => "contains",
            MatchKind::Fuzzy => "fuzzy",
        }
    }
}

/// Loose case-insensitive name match: exact → contains either way → Levenshtein.
/// Empty needle or haystack never matches.
pub fn loose_text_match(haystack: &str, needle: &str) -> Option<MatchKind> {
    if haystack.is_empty() || needle.is_empty() {
        return None;
    }
    let h = haystack.to_lowercase();
    let n = needle.to_lowercase();
    if h == n {
        return Some(MatchKind::Exact);
    }
    if h.contains(&n) || n.contains(&h) {
        return Some(MatchKind::Contains);
    }
    if levenshtein_similar(&h, &n) {
        return Some(MatchKind::Fuzzy);
    }
    None
}

/// Best loose match across multiple text fields (skips empty fields).
pub fn best_loose_match(fields: &[&str], needle: &str) -> Option<MatchKind> {
    let mut best: Option<MatchKind> = None;
    for field in fields {
        if let Some(kind) = loose_text_match(field, needle) {
            best = Some(match best {
                Some(prev) if prev > kind => prev,
                _ => kind,
            });
            if best == Some(MatchKind::Exact) {
                break;
            }
        }
    }
    best
}

/// Normalize brew category filter aliases (frontend ids / colloquial → DB value).
/// e.g. `friends` / `friendlink` / `友链` → `友情链接`.
pub fn normalize_brew_category_filter(category: &str) -> String {
    let c = category.trim();
    let lower = c.to_lowercase();
    match lower.as_str() {
        "friends" | "friend" | "friendlink" | "friend-link" | "friend_link" | "friend-links"
        | "friend_links" | "友链" => "友情链接".to_string(),
        "mine" | "me" | "我的" => "我".to_string(),
        _ => c.to_string(),
    }
}

/// Normalize `sourceType` filter; friend-link aliases map to `link`.
pub fn normalize_brew_source_type_filter(source_type: &str) -> String {
    let lower = source_type.trim().to_lowercase();
    match lower.as_str() {
        "link" | "friendlink" | "friend" | "friends" | "friend-link" | "friend_link" | "友链"
        | "友情链接" => "link".to_string(),
        "rss" | "feed" | "atom" => "rss".to_string(),
        "brewlia" | "ai" => "brewlia".to_string(),
        _ => lower,
    }
}

/// Multi-category token match for comma-separated `category` fields
/// (e.g. `"友情链接, 技术"` matches `"友情链接"` or `"技术"`, not substring `"科"`).
pub fn brew_category_token_matches(source_category: &str, category_name: &str) -> bool {
    let cat = category_name.trim();
    if cat.is_empty() {
        return false;
    }
    let sc = source_category.trim();
    if sc.is_empty() {
        return false;
    }
    let cat_lower = cat.to_lowercase();
    sc.split(',')
        .map(|t| t.trim().to_lowercase())
        .filter(|t| !t.is_empty())
        .any(|t| t == cat_lower)
}

/// 从步骤输出中提取图片 URL（如果存在）
pub fn extract_image_url(output: &Value) -> Option<String> {
    output
        .as_object()
        .and_then(|obj| obj.get("imageUrl"))
        .and_then(|v| v.as_str())
        .filter(|url| {
            url.starts_with("http://") || url.starts_with("https://") || url.starts_with("/api/")
        })
        .map(|s| s.to_string())
}

/// 简化输出摘要（用于实时进度显示）— 委托给 response_agent
pub fn summarize_output(output: &Value) -> Option<String> {
    crate::services::agent::response_agent::summarize_step_output(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_levenshtein_similar() {
        assert!(levenshtein_similar("hello", "hallo"));
        assert!(levenshtein_similar("test", "tset"));
        assert!(!levenshtein_similar("abc", "xyz"));
        assert!(!levenshtein_similar("", "test"));
    }

    #[test]
    fn truncate_str_respects_utf8_char_boundaries() {
        assert_eq!(truncate_str("hello", 5), "hello");
        assert_eq!(truncate_str("hello world", 5), "hello");
        let s = "你好世界";
        assert_eq!(truncate_str(s, 0), "");
        assert_eq!(truncate_str(s, 3), "你");
        assert_eq!(truncate_str(s, 5), "你");
        assert_eq!(truncate_str(s, 6), "你好");
    }

    #[test]
    fn platform_name_whitelist_all_vs_single() {
        assert_eq!(validate_platform_name("steam").unwrap(), "steam");
        assert_eq!(validate_platform_name("all").unwrap(), "all");
        assert!(validate_platform_name("not-a-platform").is_err());
        assert!(is_valid_platform("steam"));
        assert!(!is_valid_platform("all"));
        assert!(!is_valid_platform("not-a-platform"));
    }

    #[test]
    fn test_loose_text_match_order() {
        assert_eq!(loose_text_match("akiday", "akiday"), Some(MatchKind::Exact));
        assert_eq!(
            loose_text_match("Akiday Blog", "akiday"),
            Some(MatchKind::Contains)
        );
        assert_eq!(
            loose_text_match("akiday", "Akiday Blog"),
            Some(MatchKind::Contains)
        );
        // typo / near miss
        assert_eq!(loose_text_match("akiday", "akday"), Some(MatchKind::Fuzzy));
        assert_eq!(loose_text_match("akiday", "zzzzzz"), None);
        assert_eq!(loose_text_match("", "akiday"), None);
        assert_eq!(loose_text_match("akiday", ""), None);
    }

    #[test]
    fn test_best_loose_match_prefers_exact_name() {
        let kind = best_loose_match(
            &["https://example.com/akiday", "友情链接", "akiday"],
            "akiday",
        );
        assert_eq!(kind, Some(MatchKind::Exact));
        assert_eq!(kind.unwrap().as_str(), "exact");
    }

    #[test]
    fn test_normalize_brew_category_friendlink() {
        assert_eq!(normalize_brew_category_filter("friends"), "友情链接");
        assert_eq!(normalize_brew_category_filter("friendlink"), "友情链接");
        assert_eq!(normalize_brew_category_filter("友链"), "友情链接");
        assert_eq!(normalize_brew_category_filter("友情链接"), "友情链接");
        assert_eq!(normalize_brew_category_filter("技术"), "技术");
        assert_eq!(normalize_brew_category_filter("mine"), "我");
    }

    #[test]
    fn extract_image_url_accepts_http_and_local_api_paths() {
        assert_eq!(
            extract_image_url(&json!({"imageUrl": "https://x/a.png"})).as_deref(),
            Some("https://x/a.png")
        );
        assert_eq!(
            extract_image_url(&json!({"imageUrl": "/api/brew/image-cache/ab/abcd.png"})).as_deref(),
            Some("/api/brew/image-cache/ab/abcd.png")
        );
        assert!(extract_image_url(&json!({"imageUrl": "data:image/png;base64,xx"})).is_none());
        assert!(extract_image_url(&json!({"imageUrl": "javascript:alert(1)"})).is_none());
    }

    #[test]
    fn test_normalize_brew_source_type_friendlink() {
        assert_eq!(normalize_brew_source_type_filter("link"), "link");
        assert_eq!(normalize_brew_source_type_filter("friendlink"), "link");
        assert_eq!(normalize_brew_source_type_filter("友情链接"), "link");
        assert_eq!(normalize_brew_source_type_filter("rss"), "rss");
        assert_eq!(normalize_brew_source_type_filter("brewlia"), "brewlia");
    }

    #[test]
    fn test_brew_category_token_matches_friendlink() {
        assert!(brew_category_token_matches("友情链接", "友情链接"));
        assert!(brew_category_token_matches("友情链接, 技术", "友情链接"));
        assert!(brew_category_token_matches("技术, 友情链接", "友情链接"));
        assert!(brew_category_token_matches(
            "技术, 友情链接, 生活",
            "友情链接"
        ));
        assert!(!brew_category_token_matches("技术", "友情链接"));
        // substring of a token must not match
        assert!(!brew_category_token_matches("科学技术", "技术"));
    }

    #[test]
    fn test_summarize_output() {
        use serde_json::json;

        assert_eq!(
            summarize_output(&json!({"message": "成功"})),
            Some("成功".to_string())
        );
        assert_eq!(
            summarize_output(&json!({"total": 10})),
            Some("获取了 10 条结果".to_string())
        );
        assert_eq!(
            summarize_output(&json!([1, 2, 3])),
            Some("获取了 3 条记录".to_string())
        );
    }
}
