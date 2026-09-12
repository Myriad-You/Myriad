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

/// 步骤超时的下限与上限（秒）。上限对齐生图 HTTP 客户端的 15 分钟。
pub const STEP_TIMEOUT_MIN_SECS: u64 = 10;
pub const STEP_TIMEOUT_MAX_SECS: u64 = 900;
/// AI 能力的保底（秒）：生图与 Pro 长文经常超过一分钟。
pub const AI_STEP_TIMEOUT_FLOOR_SECS: u64 = 300;
/// Skill 子步骤的地板（秒）。子步骤大多是小操作，但不能因此把声明需要更久的
/// 能力也压到这里——`ai.image` 声明 300 秒、`model3d.generate` 声明 180 秒。
pub const SKILL_SUB_STEP_MIN_SECS: u64 = 60;

/// 能力没有声明预估时长时的类别兜底（秒）。
pub fn category_timeout_fallback_secs(
    category: &crate::services::agent::types::CapabilityCategory,
) -> u64 {
    use crate::services::agent::types::CapabilityCategory;
    match category {
        CapabilityCategory::AiProcess | CapabilityCategory::ResourceCreate => 300,
        CapabilityCategory::ExternalIntegration => 30,
        _ => 30,
    }
}

/// 一个步骤的超时（秒）。
///
/// 显式值优先；否则按能力声明的预估时长取 3 倍缓冲；两者都没有就用类别兜底。
/// 最后给 AI 能力抬一个保底。
///
/// Skill DAG 子步骤也走 `step_timeout_secs`，禁止 `timeout_ms: Some(60000)`。
pub fn step_timeout_secs(
    explicit_ms: Option<u64>,
    estimated_duration_ms: Option<u64>,
    category_fallback_secs: u64,
    requires_ai: bool,
) -> u64 {
    let secs = explicit_ms
        .map(|ms| (ms / 1000).clamp(STEP_TIMEOUT_MIN_SECS, STEP_TIMEOUT_MAX_SECS))
        .unwrap_or_else(|| {
            estimated_duration_ms
                .map(|ms| (ms * 3 / 1000).clamp(STEP_TIMEOUT_MIN_SECS, STEP_TIMEOUT_MAX_SECS))
                .unwrap_or(category_fallback_secs)
        });
    if requires_ai && secs < AI_STEP_TIMEOUT_FLOOR_SECS {
        AI_STEP_TIMEOUT_FLOOR_SECS
    } else {
        secs
    }
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
/// 编辑距离 ≤ max(较短串长度/2, 2) 则认为相似
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
    let inner = crate::services::agent::ai_process_pure::task_inner_value(output);
    inner
        .as_object()
        .and_then(|obj| obj.get("url").or_else(|| obj.get("imageUrl")))
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
mod step_timeout_tests {
    use super::*;

    #[test]
    fn explicit_value_wins_and_is_clamped() {
        assert_eq!(step_timeout_secs(Some(45_000), Some(1_000), 30, false), 45);
        assert_eq!(
            step_timeout_secs(Some(1), None, 30, false),
            STEP_TIMEOUT_MIN_SECS
        );
        assert_eq!(
            step_timeout_secs(Some(9_999_000), None, 30, false),
            STEP_TIMEOUT_MAX_SECS
        );
    }

    #[test]
    fn declared_duration_gets_a_three_times_buffer() {
        assert_eq!(step_timeout_secs(None, Some(10_000), 30, false), 30);
        assert_eq!(step_timeout_secs(None, None, 30, false), 30);
    }

    /// 声明需要很久的能力必须真的拿到那么久（3× 声明时长）。
    #[test]
    fn long_capabilities_keep_their_declared_budget() {
        assert_eq!(step_timeout_secs(None, Some(300_000), 300, true), 900);
        assert_eq!(step_timeout_secs(None, Some(180_000), 300, false), 540);
    }

    /// Skill 子步骤既要有地板，也不能盖掉能力自己声明的预算。
    #[test]
    fn skill_sub_steps_floor_short_work_without_capping_long_work() {
        let sub_step = |est, fallback, ai| {
            step_timeout_secs(None, est, fallback, ai).max(SKILL_SUB_STEP_MIN_SECS)
        };
        // 小操作抬到地板
        assert_eq!(sub_step(Some(100), 30, false), SKILL_SUB_STEP_MIN_SECS);
        // 能力声明更久时以声明为准
        assert_eq!(sub_step(Some(300_000), 300, true), 900); // ai.image
        assert_eq!(sub_step(Some(180_000), 300, false), 540); // model3d.generate
        assert_eq!(sub_step(Some(120_000), 300, false), 360); // model3d.rig
    }

    /// 产物会被执行的提示词，必须给第三方内容划边界。
    ///
    /// 这三处的输入里都有 `ai.webSearch` / `web.scrape` / `brew.article` 抓回来
    /// 的正文，或 TAPP 自己渲染的 DOM——都是别人能写的字；而它们的输出分别是
    /// 执行步骤和 click/input 计划。少一处边界，正文里一句「忽略以上」就通到
    /// 执行层。
    #[test]
    fn prompts_that_yield_executable_plans_frame_untrusted_input() {
        for (label, source, expected) in [
            (
                "动态步骤生成",
                include_str!("executor/path_ai_helpers.rs"),
                1,
            ),
            (
                "Skill 内部 DAG",
                include_str!("executor/execute_step.rs"),
                1,
            ),
            (
                "UI / 页面动作计划",
                include_str!("executor/handlers/ui_control.rs"),
                2,
            ),
        ] {
            assert_eq!(
                source.matches("untrusted_block(").count(),
                expected,
                "{label} 的第三方输入没有全部带边界"
            );
        }
    }

    /// 产物只是文字、但输入同样来自公网的那一档。
    ///
    /// `ai.summarize` / `ai.analyze` 的 `data` 由 Planner 用 `dataFrom` 从上游
    /// 接过来，常常就是 `ai.webSearch` 的结果。执行不了动作，但正文里的祈使句
    /// 仍然能改写「总结成什么」。
    #[test]
    fn text_only_prompts_over_fetched_content_are_framed_too() {
        let ai_process = include_str!("executor/handlers/ai_process.rs");
        assert_eq!(
            ai_process.matches("untrusted_block(").count(),
            2,
            "ai.summarize / ai.analyze 的上游输入没有全部带边界"
        );
    }

    /// 手抠花括号的地方必须收敛到共享实现上。
    ///
    /// `raw.find('{')` + `raw.rfind('}')` 再 `&raw[start..=end]`，少了区间校验
    /// 就会在「最后一个 `}` 出现在第一个 `{` 之前」时 panic——模型被 max_tokens
    /// 截断就可能长这样。共享实现带校验，也认 ```json 围栏。
    #[test]
    fn json_extraction_is_not_rehand_rolled() {
        for (label, source) in [
            ("Skill DAG", include_str!("executor/execute_step.rs")),
            ("动态步骤", include_str!("executor/path_ai_helpers.rs")),
            ("追问判定", include_str!("executor/resume_and_dynamic.rs")),
            ("记忆提取", include_str!("memory/manager.rs")),
            ("报告 DNA", include_str!("merope/report_dna.rs")),
        ] {
            assert!(
                !source.contains("rfind('}')") && !source.contains("rfind(']')"),
                "{label} 又自己抠了一遍花括号"
            );
        }
    }

    /// DAG 子步骤走 `step_timeout_secs`，禁止 `timeout_ms: Some(60000)`。
    #[test]
    fn the_dag_path_derives_its_timeout_instead_of_hardcoding_one() {
        let dag = include_str!("executor/execute_step.rs");
        assert!(
            !dag.contains("timeout_ms: Some(60000)"),
            "Skill 子步骤又把超时写死了"
        );
        assert_eq!(
            dag.matches("step_timeout_secs(").count(),
            2,
            "消费端与 DAG 构建各调一次；数量变了说明有人另开了第三套算法"
        );
    }

    #[test]
    fn ai_capabilities_never_fall_under_the_floor() {
        assert_eq!(
            step_timeout_secs(None, Some(3_000), 300, true),
            AI_STEP_TIMEOUT_FLOOR_SECS
        );
        // 非 AI 能力不受这个保底影响
        assert_eq!(step_timeout_secs(None, Some(3_000), 30, false), 10);
    }
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
            extract_image_url(&json!({
                "format": "image",
                "value": { "url": "https://x/b.png", "width": 1, "height": 1 }
            }))
            .as_deref(),
            Some("https://x/b.png")
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
            Some("Got 10 results".to_string())
        );
        assert_eq!(
            summarize_output(&json!([1, 2, 3])),
            Some("Got 3 records".to_string())
        );
    }
}
