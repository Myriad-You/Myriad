//! Brew 文章主题打标（关键词版）。
//!
//! 主题 key 是 10 个固定值，前端 `frontend/src/components/brew/logic/topics.ts`
//! 有同一份词表 —— **key 必须一致**，改一边必须改另一边。展示文案走 i18n，
//! 这里只产出 key。
//!
//! 纪律：
//! - 单标签。一篇文章只有一个 topic。
//! - 命中多个主题时按 `TOPIC_SEEDS` 顺序取第一个（确定性，重跑同结果）。
//! - 一个都不命中返回 `None`，写入 NULL。**不要**写「未知」/「其他」——
//!   聚类侧靠 NULL 把这些文章留在源磁贴里。
//!
//! 生产路径只有关键词打标（插入时 `infer_topic_by_keywords`）；没有小时级 AI 批量。

/// 预定义主题及其关键词种子（大小写不敏感，匹配标题 + 摘要前 200 字）。
///
/// 顺序即优先级。宁可漏标（None），不要标错硬塞。
const TOPIC_SEEDS: &[(&str, &[&str])] = &[
    (
        "engineering",
        &[
            "refactor",
            "重构",
            "微服务",
            "单体",
            "code review",
            "代码审查",
            "rust",
            "typescript",
        ],
    ),
    (
        "systems",
        &[
            "linux", "kernel", "tcp", "dns", "sqlite", "postgres", "性能", "perf", "jit",
        ],
    ),
    (
        "ai",
        &[
            "llm",
            "gpt",
            "模型",
            "transformer",
            "agent",
            "embedding",
            "提示词",
        ],
    ),
    ("product", &["设计", "ux", "ui", "独立开发", "产品", "交互"]),
    ("writing", &["中文", "排版", "写作", "播客", "字体"]),
    ("tools", &["效率", "笔记", "工作流", "周刊", "工具"]),
    ("culture", &["生活", "文化", "旅行", "城市"]),
    ("security", &["安全", "漏洞", "cve", "加密", "privacy"]),
    ("oss", &["开源", "github", "license", "社区"]),
    ("hardware", &["芯片", "硬件", "pcb", "制造", "risc-v"]),
];

/// 摘要参与匹配的前缀字符数。
const SUMMARY_MATCH_CHARS: usize = 200;

/// 全部预定义主题 key。
#[allow(dead_code)] // tests only; no production AI tagger
pub fn predefined_topics() -> Vec<&'static str> {
    TOPIC_SEEDS.iter().map(|(key, _)| *key).collect()
}

/// key 是否在预定义表内。
#[allow(dead_code)] // tests only; no production AI tagger
pub fn is_predefined_topic(key: &str) -> bool {
    TOPIC_SEEDS.iter().any(|(k, _)| *k == key)
}

fn strip_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_tag = false;
    for ch in input.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}

/// 关键词打标。命中返回主题 key，否则 `None`（写 NULL）。
pub fn infer_topic_by_keywords(title: &str, summary: Option<&str>) -> Option<&'static str> {
    let summary_plain = summary
        .map(strip_html)
        .map(|s| s.chars().take(SUMMARY_MATCH_CHARS).collect::<String>())
        .unwrap_or_default();

    let haystack = format!("{}\n{}", title, summary_plain).to_lowercase();
    if haystack.trim().is_empty() {
        return None;
    }

    for (key, seeds) in TOPIC_SEEDS {
        if seeds.iter().any(|seed| haystack.contains(seed)) {
            return Some(key);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ten_stable_keys_and_no_other_bucket() {
        let keys = predefined_topics();
        assert_eq!(keys.len(), 10);
        assert_eq!(
            keys,
            vec![
                "engineering",
                "systems",
                "ai",
                "product",
                "writing",
                "tools",
                "culture",
                "security",
                "oss",
                "hardware",
            ]
        );
        assert!(!is_predefined_topic("other"));
        assert!(!is_predefined_topic("misc"));
        assert!(is_predefined_topic("engineering"));
    }

    #[test]
    fn matches_title_and_summary() {
        assert_eq!(
            infer_topic_by_keywords("这次重构把单体拆了", None),
            Some("engineering")
        );
        assert_eq!(
            infer_topic_by_keywords("周末随笔", Some("<p>聊聊 <b>embedding</b> 的取舍</p>")),
            Some("ai")
        );
    }

    #[test]
    fn case_insensitive() {
        assert_eq!(
            infer_topic_by_keywords("Understanding TypeScript", None),
            Some("engineering")
        );
        assert_eq!(infer_topic_by_keywords("CVE 复盘", None), Some("security"));
    }

    #[test]
    fn no_match_returns_none() {
        assert_eq!(infer_topic_by_keywords("今天天气不错", None), None);
        assert_eq!(infer_topic_by_keywords("", None), None);
        assert_eq!(infer_topic_by_keywords("   ", Some("")), None);
    }

    #[test]
    fn single_label_deterministic_by_seed_order() {
        // engineering 在 systems 之前，两边都命中时取前者
        let out = infer_topic_by_keywords("Rust 写的 kernel 模块", None);
        assert_eq!(out, Some("engineering"));
        assert_eq!(infer_topic_by_keywords("Rust 写的 kernel 模块", None), out);
    }

    #[test]
    fn only_first_200_summary_chars() {
        let far = format!("{}kernel", "啊".repeat(400));
        assert_eq!(infer_topic_by_keywords("无关标题", Some(&far)), None);
    }
}
