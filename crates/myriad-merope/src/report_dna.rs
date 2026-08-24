use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub const MAX_REPORT_DNA_REPORTS: usize = 12;
pub const MAX_REPORT_SUMMARY_CHARS: usize = 800;
pub const MAX_REPORT_INSIGHT_CHARS: usize = 1_600;
pub const MAX_REPORT_NOTE_CHARS: usize = 800;

#[derive(Debug, Clone)]
pub struct ReportDnaSource {
    pub platform: String,
    pub report: Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportDnaEvidence {
    pub platform: String,
    pub summary: String,
    pub insights: Vec<String>,
    pub note: String,
    pub structured_labels: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ReportDnaBundle {
    pub report_count: usize,
    pub platforms: Vec<String>,
    pub fingerprint: String,
    pub evidence: Vec<ReportDnaEvidence>,
    pub fallback_seed_keys: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReportDnaProvenance {
    pub fingerprint: String,
    pub report_count: usize,
    pub platforms: Vec<String>,
}

pub fn build_report_dna_bundle(sources: &[ReportDnaSource]) -> ReportDnaBundle {
    let evidence = sources
        .iter()
        .take(MAX_REPORT_DNA_REPORTS)
        .map(report_evidence)
        .collect::<Vec<_>>();
    let platforms = evidence
        .iter()
        .map(|item| item.platform.clone())
        .collect::<Vec<_>>();
    ReportDnaBundle {
        report_count: evidence.len(),
        platforms,
        fingerprint: evidence_fingerprint(&evidence),
        fallback_seed_keys: infer_seed_keys(&evidence),
        evidence,
    }
}

/// Spoken-persona pool for AI-off / thin-response padding only.
pub const PERSONA_POOL_KEYS: &[&str] = &[
    "thoughtful",
    "curious",
    "creative",
    "calm",
    "playful",
    "focused",
    "independent",
    "social",
    "persistent",
    "expressive",
    "bound",
    "loyal",
    "perfect",
    "solo",
    "order",
    "warm",
    "near",
    "deep_focus",
    "night",
    "soft",
    "space",
    "feel",
    "arranged",
    "slow_warm",
    "shy",
    "try_hard",
];

/// Deterministic PRNG shuffle — same seed ⇒ same order.
pub fn seed_shuffle<T>(items: &mut [T], seed: &str) {
    if items.len() <= 1 {
        return;
    }
    let mut state = seed.bytes().fold(0u32, |acc, byte| {
        acc.wrapping_mul(33).wrapping_add(byte as u32)
    });
    if state == 0 {
        state = 1;
    }
    for i in (1..items.len()).rev() {
        // xorshift32
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        let j = (state as usize) % (i + 1);
        items.swap(i, j);
    }
}

fn unique_push(out: &mut Vec<String>, tag: &str) {
    let tag = tag.trim();
    if tag.is_empty() {
        return;
    }
    if !out
        .iter()
        .any(|existing| existing.eq_ignore_ascii_case(tag))
    {
        out.push(tag.to_string());
    }
}

fn persona_pool_labels(language: &str) -> Vec<String> {
    localize_report_seed_keys(
        &PERSONA_POOL_KEYS
            .iter()
            .map(|key| (*key).to_string())
            .collect::<Vec<_>>(),
        language,
    )
}

/// Finalize **AI-generated** tags for the bubble deck.
///
/// Keeps the model output intact (dedupe + sanitize + cap). Only pads from the
/// local persona pool when the model returned too few labels — never discards
/// AI tags to make room for seed-sampling.
pub fn complete_ai_tag_deck(primary: &[String], language: &str, target: usize) -> Vec<String> {
    let target = target.clamp(8, crate::MAX_ONBOARDING_TAGS);
    let mut result = Vec::with_capacity(target);
    for tag in primary {
        unique_push(&mut result, tag);
        if result.len() >= target {
            break;
        }
    }
    if result.len() < target {
        for extra in persona_pool_labels(language) {
            unique_push(&mut result, &extra);
            if result.len() >= target {
                break;
            }
        }
    }
    crate::sanitize_onboarding_tags(&result)
}

/// Local fallback deck when AI is off / failed / rate-limited.
///
/// Evidence keys first, then persona pool. `seed` only shuffles order so a
/// retry without AI is not a frozen list — this is **not** "regenerate".
pub fn fallback_tag_deck(
    evidence_tags: &[String],
    language: &str,
    seed: &str,
    target: usize,
) -> Vec<String> {
    let target = target.clamp(8, crate::MAX_ONBOARDING_TAGS);
    let mut result = Vec::with_capacity(target);
    for tag in evidence_tags {
        unique_push(&mut result, tag);
    }
    for extra in persona_pool_labels(language) {
        unique_push(&mut result, &extra);
    }
    seed_shuffle(&mut result, seed);
    result.truncate(target);
    crate::sanitize_onboarding_tags(&result)
}

/// @deprecated Prefer [`complete_ai_tag_deck`] or [`fallback_tag_deck`].
/// Kept as an alias of fallback for any external call sites.
pub fn sample_tag_deck(
    primary: &[String],
    language: &str,
    seed: &str,
    target: usize,
) -> Vec<String> {
    fallback_tag_deck(primary, language, seed, target)
}

pub fn localize_report_seed_keys(keys: &[String], language: &str) -> Vec<String> {
    keys.iter()
        .filter_map(|key| {
            let label = match language {
                value if value.starts_with("zh") => match key.as_str() {
                    "thoughtful" => "善于思考",
                    "curious" => "好奇",
                    "creative" => "有创造力",
                    "calm" => "沉静",
                    "playful" => "活泼",
                    "focused" => "专注",
                    "independent" => "独立",
                    "social" => "重视连接",
                    "persistent" => "有韧性",
                    "expressive" => "善于表达",
                    // Richer persona fallbacks (spoken kernel seeds).
                    "bound" => "边界感强",
                    "loyal" => "朋友不多但很铁",
                    "perfect" => "完美主义",
                    "solo" => "独处才放松",
                    "order" => "讨厌混乱",
                    "warm" => "外冷内热",
                    "near" => "想交心又怕熟",
                    "deep_focus" => "做事很沉",
                    "night" => "夜猫子",
                    "soft" => "嘴硬心软",
                    "space" => "礼貌但疏离",
                    "feel" => "情绪来了先憋着",
                    "arranged" => "喜欢把事情安排妥",
                    "slow_warm" => "慢热",
                    "shy" => "社恐",
                    "try_hard" => "认真起来很拼",
                    _ => return None,
                },
                value if value.starts_with("ja") => match key.as_str() {
                    "thoughtful" => "思慮深い",
                    "curious" => "好奇心旺盛",
                    "creative" => "創造的",
                    "calm" => "穏やか",
                    "playful" => "遊び好き",
                    "focused" => "集中力がある",
                    "independent" => "自立心がある",
                    "social" => "つながりを大切にする",
                    "persistent" => "粘り強い",
                    "expressive" => "表現豊か",
                    "bound" => "境界線がはっきり",
                    "loyal" => "友は少ないが深い",
                    "perfect" => "完璧主義",
                    "solo" => "一人の時間が必要",
                    "order" => "混沌が苦手",
                    "warm" => "一見クールで実は温かい",
                    "near" => "近づきたいけど怖い",
                    "deep_focus" => "没頭すると止まらない",
                    "night" => "夜型",
                    "soft" => "口は堅いが心は柔らかい",
                    "space" => "礼儀正しい距離感",
                    "feel" => "感情をすぐ出さない",
                    "arranged" => "段取り好き",
                    "slow_warm" => "スロースターター",
                    "shy" => "人見知り",
                    "try_hard" => "本気になると粘る",
                    _ => return None,
                },
                _ => match key.as_str() {
                    "thoughtful" => "Thoughtful",
                    "curious" => "Curious",
                    "creative" => "Creative",
                    "calm" => "Calm",
                    "playful" => "Playful",
                    "focused" => "Focused",
                    "independent" => "Independent",
                    "social" => "Connection-minded",
                    "persistent" => "Persistent",
                    "expressive" => "Expressive",
                    "bound" => "Clear boundaries",
                    "loyal" => "Few but loyal friends",
                    "perfect" => "Perfectionist streak",
                    "solo" => "Recharges alone",
                    "order" => "Dislikes chaos",
                    "warm" => "Cool outside, warm inside",
                    "near" => "Wants closeness carefully",
                    "deep_focus" => "Goes deep when working",
                    "night" => "Night owl",
                    "soft" => "Blunt mouth, soft heart",
                    "space" => "Polite but distant",
                    "feel" => "Holds feelings first",
                    "arranged" => "Likes things sorted",
                    "slow_warm" => "Slow to warm up",
                    "shy" => "Socially cautious",
                    "try_hard" => "Goes all-in when serious",
                    _ => return None,
                },
            };
            Some(label.to_string())
        })
        .collect()
}

/// Jobs / majors / demographics — not persona kernel seeds (from experimental DNA).
pub fn looks_like_job_or_identity_label(label: &str) -> bool {
    let label = label.trim();
    if label.is_empty() {
        return false;
    }
    const EXACT: &[&str] = &[
        "学生",
        "大学生",
        "研究生",
        "上班族",
        "打工人",
        "社畜",
        "程序员",
        "码农",
        "工程师",
        "设计师",
        "产品经理",
        "自由职业",
        "创业者",
        "老板",
        "老师",
        "教师",
        "医生",
        "护士",
        "律师",
        "会计",
        "司机",
        "主播",
        "博主",
        "网红",
        "up主",
        "UP主",
        "北漂",
        "沪漂",
        "男生",
        "女生",
        "男人",
        "女人",
    ];
    if EXACT.iter().any(|blocked| label == *blocked) {
        return true;
    }
    label.contains("工程师")
        || label.contains("程序员")
        || label.contains("架构师")
        || label.contains("开发者")
        || label.contains("分析师")
        || label.contains("设计师")
        || label.contains("产品经理")
        || label.contains("职业")
        || label.contains("专业")
        || label.contains("学历")
        || label.contains("本科")
        || label.contains("硕士")
        || label.contains("博士")
        || label.contains("engineer")
        || label.contains("programmer")
        || label.contains("manager")
}

/// Media catalog / platform crumbs — not temperament seeds.
pub fn looks_like_media_catalog_label(label: &str) -> bool {
    let lower = label.trim().to_lowercase();
    const NEEDLES: &[&str] = &[
        "二次元",
        "动画",
        "动漫",
        "游戏",
        "摇滚",
        "开放世界",
        "独立游戏",
        "账号",
        "用户",
        "报告",
        "平台",
        "http",
        "www",
        ".com",
        "anime",
        "game",
        "report",
        "platform",
    ];
    NEEDLES.iter().any(|needle| lower.contains(needle))
}

/// Spoken persona seed: 2–14 chars, not job/media/literary sludge.
pub fn is_reasonable_persona_tag(label: &str) -> bool {
    let label = label.trim();
    let chars = label.chars().count();
    if !(2..=14).contains(&chars) {
        return false;
    }
    if label.chars().any(|c| c.is_ascii_digit()) {
        return false;
    }
    if looks_like_job_or_identity_label(label) || looks_like_media_catalog_label(label) {
        return false;
    }
    const LITERARY: &[&str] = &[
        "质感",
        "美学",
        "信仰",
        "虔诚",
        "月光",
        "余温",
        "藏锋",
        "证明存在",
        "消化情绪",
        "取自",
        "像把",
        "在心里",
    ];
    if LITERARY.iter().any(|blocked| label.contains(blocked)) {
        return false;
    }
    if label.contains('(') || label.contains(')') || label.contains('（') || label.contains('）')
    {
        return false;
    }
    true
}

pub fn sanitize_report_dna_tags(tags: &[String]) -> Vec<String> {
    const BLOCKED: &[&str] = &[
        "engineer",
        "programmer",
        "student",
        "designer",
        "manager",
        "report",
        "platform",
        "工程师",
        "程序员",
        "学生",
        "设计师",
        "产品经理",
        "报告",
        "平台",
        "エンジニア",
        "プログラマー",
        "学生",
        "レポート",
        "プラットフォーム",
    ];
    let filtered = tags
        .iter()
        .filter(|tag| {
            let normalized = tag.trim().to_lowercase();
            if BLOCKED.iter().any(|blocked| normalized.contains(blocked)) {
                return false;
            }
            is_reasonable_persona_tag(tag)
        })
        .cloned()
        .collect::<Vec<_>>();
    crate::sanitize_onboarding_tags(&filtered)
}

pub fn report_dna_json(tags: &[String], provenance: &ReportDnaProvenance) -> Value {
    json!({
        "fingerprint": provenance.fingerprint,
        "reportCount": provenance.report_count,
        "platforms": provenance.platforms,
        "rawReportsStored": false,
        "selectedTags": tags,
    })
}

fn report_evidence(source: &ReportDnaSource) -> ReportDnaEvidence {
    let summary = string_field(
        &source.report,
        &["summary", "overview"],
        MAX_REPORT_SUMMARY_CHARS,
    );
    let insights = string_list_field(
        &source.report,
        &["insights", "highlights"],
        MAX_REPORT_INSIGHT_CHARS,
    );
    let note = string_field(
        &source.report,
        &["note", "analysis_note", "takeaway"],
        MAX_REPORT_NOTE_CHARS,
    );
    let card = source.report.get("card_visuals").unwrap_or(&Value::Null);
    let mut structured_labels = Vec::new();
    for root in [&source.report, card] {
        for key in [
            "favorite_tags",
            "mood_keywords",
            "top_genres",
            "community_tags",
            "interest_circles",
            "signature_topics",
            "languages",
        ] {
            collect_labels(root.get(key), &mut structured_labels);
        }
        for key in ["taste_profile", "player_type", "role_profile", "vibe"] {
            if let Some(value) = root.get(key).and_then(Value::as_str) {
                push_label(&mut structured_labels, value);
            }
        }
    }
    structured_labels.truncate(24);
    ReportDnaEvidence {
        platform: source.platform.trim().chars().take(48).collect(),
        summary,
        insights,
        note,
        structured_labels,
    }
}

fn string_field(value: &Value, keys: &[&str], limit: usize) -> String {
    bounded_plain_text(
        keys.iter()
            .find_map(|key| value.get(key).and_then(Value::as_str))
            .unwrap_or_default(),
        limit,
    )
}

fn string_list_field(value: &Value, keys: &[&str], limit: usize) -> Vec<String> {
    let mut remaining = limit;
    let mut output = Vec::new();
    let values = keys
        .iter()
        .find_map(|key| value.get(key).and_then(Value::as_array));
    for item in values
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .take(16)
    {
        if remaining == 0 {
            break;
        }
        let item = bounded_plain_text(item, remaining.min(240));
        remaining = remaining.saturating_sub(item.chars().count());
        if !item.is_empty() {
            output.push(item);
        }
    }
    output
}

fn bounded_plain_text(value: &str, limit: usize) -> String {
    let mut output = String::new();
    let mut inside_tag = false;
    let mut pending_space = false;
    let mut output_chars = 0;
    for character in value.trim().chars() {
        match character {
            '<' => {
                inside_tag = true;
                pending_space = !output.is_empty();
            }
            '>' if inside_tag => inside_tag = false,
            _ if inside_tag || character.is_control() => {}
            _ if character.is_whitespace() => pending_space = !output.is_empty(),
            _ => {
                if pending_space && output_chars < limit {
                    output.push(' ');
                    output_chars += 1;
                }
                pending_space = false;
                if output_chars >= limit {
                    break;
                }
                output.push(character);
                output_chars += 1;
            }
        }
    }
    output
}

fn collect_labels(value: Option<&Value>, output: &mut Vec<String>) {
    let Some(value) = value else {
        return;
    };
    match value {
        Value::String(value) => {
            for part in value.split(['、', '，', ',', '/', '|']) {
                push_label(output, part);
            }
        }
        Value::Array(values) => {
            for value in values.iter().take(24) {
                match value {
                    Value::String(value) => push_label(output, value),
                    Value::Object(object) => {
                        if let Some(value) = ["tag", "name", "label", "genre"]
                            .iter()
                            .find_map(|key| object.get(*key).and_then(Value::as_str))
                        {
                            push_label(output, value);
                        }
                    }
                    _ => {}
                }
            }
        }
        Value::Object(values) => {
            for key in values.keys().take(24) {
                push_label(output, key);
            }
        }
        _ => {}
    }
}

fn push_label(output: &mut Vec<String>, value: &str) {
    let value = value.trim().chars().take(32).collect::<String>();
    if value.chars().count() >= 2
        && !output
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(&value))
    {
        output.push(value);
    }
}

fn evidence_fingerprint(evidence: &[ReportDnaEvidence]) -> String {
    let encoded = serde_json::to_vec(evidence).unwrap_or_default();
    let digest = Sha256::digest(encoded);
    let prefix = digest[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("sha256-{prefix}")
}

fn infer_seed_keys(evidence: &[ReportDnaEvidence]) -> Vec<String> {
    let searchable = evidence
        .iter()
        .flat_map(|item| {
            std::iter::once(item.platform.as_str())
                .chain(std::iter::once(item.summary.as_str()))
                .chain(item.insights.iter().map(String::as_str))
                .chain(std::iter::once(item.note.as_str()))
                .chain(item.structured_labels.iter().map(String::as_str))
        })
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let mappings: [(&str, &[&str]); 10] = [
        (
            "thoughtful",
            &["策略", "分析", "技术", "code", "github", "strategy"],
        ),
        (
            "curious",
            &["探索", "广泛", "多样", "discover", "varied", "science"],
        ),
        (
            "creative",
            &["创作", "艺术", "设计", "music", "art", "creative"],
        ),
        (
            "calm",
            &["沉静", "治愈", "舒缓", "ambient", "calm", "quiet"],
        ),
        (
            "playful",
            &["幽默", "娱乐", "游戏", "game", "fun", "comedy"],
        ),
        ("focused", &["硬核", "专注", "深度", "hardcore", "focused"]),
        ("independent", &["独立", "小众", "indie", "solo"]),
        (
            "social",
            &["社区", "互动", "朋友", "community", "social", "discord"],
        ),
        (
            "persistent",
            &["长期", "收藏", "成就", "complete", "collection", "streak"],
        ),
        (
            "expressive",
            &["表达", "发帖", "评论", "write", "post", "vocal"],
        ),
    ];
    let mut keys = mappings
        .iter()
        .filter(|(_, needles)| needles.iter().any(|needle| searchable.contains(needle)))
        .map(|(key, _)| (*key).to_string())
        .take(12)
        .collect::<Vec<_>>();
    // Spoken persona kernel seeds so Lite-off / AI-fail still yields a pickable pool.
    const PERSONA_FALLBACKS: &[&str] = &[
        "bound",
        "loyal",
        "perfect",
        "solo",
        "order",
        "warm",
        "near",
        "deep_focus",
        "night",
        "soft",
        "space",
        "feel",
        "arranged",
        "slow_warm",
        "shy",
        "try_hard",
        "curious",
        "thoughtful",
        "creative",
        "calm",
    ];
    for fallback in PERSONA_FALLBACKS {
        if keys.len() >= 16 {
            break;
        }
        if !keys.iter().any(|key| key == fallback) {
            keys.push((*fallback).to_string());
        }
    }
    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_report_evidence_and_never_copies_unknown_fields() {
        let bundle = build_report_dna_bundle(&[ReportDnaSource {
            platform: "github".into(),
            report: json!({
                "summary": "s".repeat(900),
                "insights": ["code and open source", "i".repeat(1800)],
                "secret": "must not leave the report",
                "card_visuals": { "languages": [{"name": "Rust"}] }
            }),
        }]);
        assert_eq!(bundle.report_count, 1);
        assert_eq!(bundle.evidence[0].summary.chars().count(), 800);
        assert!(
            bundle.evidence[0]
                .insights
                .iter()
                .map(|value| value.chars().count())
                .sum::<usize>()
                <= MAX_REPORT_INSIGHT_CHARS
        );
        assert!(!serde_json::to_string(&bundle.evidence)
            .unwrap()
            .contains("must not leave"));
        assert!(bundle
            .fallback_seed_keys
            .contains(&"thoughtful".to_string()));
    }

    #[test]
    fn localizes_deterministic_fallback_tags() {
        let keys = vec!["curious".into(), "persistent".into(), "slow_warm".into()];
        assert_eq!(
            localize_report_seed_keys(&keys, "zh-CN"),
            ["好奇", "有韧性", "慢热"]
        );
        assert_eq!(
            localize_report_seed_keys(&keys, "ja-JP"),
            ["好奇心旺盛", "粘り強い", "スロースターター"]
        );
    }

    #[test]
    fn fallback_seed_pool_is_dense_enough_for_step1_bubbles() {
        let bundle = build_report_dna_bundle(&[ReportDnaSource {
            platform: "steam".into(),
            report: json!({
                "summary": "游戏与独立探索",
                "insights": ["hardcore collection streak"],
            }),
        }]);
        // Even a thin report should still pad to a pickable pool (≥12).
        assert!(
            bundle.fallback_seed_keys.len() >= 12,
            "got {}",
            bundle.fallback_seed_keys.len()
        );
        let zh = localize_report_seed_keys(&bundle.fallback_seed_keys, "zh-CN");
        assert!(zh.len() >= 12, "localized got {}", zh.len());
    }

    #[test]
    fn complete_ai_tag_deck_keeps_model_labels() {
        let primary: Vec<String> = (0..16).map(|i| format!("AI词条{i}")).collect();
        let deck = complete_ai_tag_deck(&primary, "zh-CN", 16);
        assert_eq!(deck.len(), 16);
        // Full AI list must survive — no seed sampling that drops model tags.
        for i in 0..16 {
            assert!(
                deck.iter().any(|t| t == &format!("AI词条{i}")),
                "missing AI词条{i} in {deck:?}"
            );
        }
    }

    #[test]
    fn complete_ai_tag_deck_pads_when_short() {
        let primary = vec!["好奇".into(), "专注".into()];
        let deck = complete_ai_tag_deck(&primary, "zh-CN", 12);
        assert_eq!(deck.len(), 12);
        assert!(deck.contains(&"好奇".to_string()));
        assert!(deck.contains(&"专注".to_string()));
    }

    #[test]
    fn fallback_tag_deck_is_stable_for_same_seed() {
        let evidence = vec!["好奇".into(), "专注".into()];
        let a = fallback_tag_deck(&evidence, "zh-CN", "seed-aaa", 16);
        let b = fallback_tag_deck(&evidence, "zh-CN", "seed-aaa", 16);
        assert_eq!(a, b);
        assert_eq!(a.len(), 16);
        // Different seed only reorders/pads — not a substitute for AI regenerate.
        let c = fallback_tag_deck(&evidence, "zh-CN", "seed-bbb", 16);
        assert_eq!(c.len(), 16);
    }

    #[test]
    fn rejects_roles_and_report_artifacts_from_ai_tags() {
        let tags = sanitize_report_dna_tags(&[
            "慢热".into(),
            "Software engineer".into(),
            "铁路工程师".into(),
            "平台用户".into(),
            "二次元".into(),
        ]);
        assert_eq!(tags, ["慢热"]);
        assert!(looks_like_job_or_identity_label("铁路工程师"));
        assert!(is_reasonable_persona_tag("边界感强"));
        assert!(!is_reasonable_persona_tag("取自过夜"));
        assert!(!is_reasonable_persona_tag("在心里"));
    }
}
