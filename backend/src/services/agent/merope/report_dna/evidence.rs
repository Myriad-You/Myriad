//! Reading each platform's latest report into bounded evidence, and the seed labels it suggests.

use super::*;

#[derive(Debug, Clone)]
pub(super) struct ReportDnaSource {
    pub(super) platform: String,
    pub(super) report: Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ReportDnaEvidence {
    pub(super) platform: String,
    pub(super) summary: String,
    pub(super) insights: Vec<String>,
    pub(super) note: String,
    pub(super) structured_labels: Vec<String>,
}

#[derive(Debug, Clone)]
pub(super) struct ReportDnaBundle {
    pub(super) report_count: usize,
    pub(super) platforms: Vec<String>,
    pub(super) evidence: Vec<ReportDnaEvidence>,
    pub(super) fallback_seed_keys: Vec<String>,
}

pub(super) fn build_report_dna_bundle(sources: &[ReportDnaSource]) -> ReportDnaBundle {
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
        fallback_seed_keys: infer_seed_keys(&evidence),
        evidence,
    }
}

pub(super) fn report_evidence(source: &ReportDnaSource) -> ReportDnaEvidence {
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

pub(super) fn string_field(value: &Value, keys: &[&str], limit: usize) -> String {
    bounded_plain_text(
        keys.iter()
            .find_map(|key| value.get(key).and_then(Value::as_str))
            .unwrap_or_default(),
        limit,
    )
}

pub(super) fn string_list_field(value: &Value, keys: &[&str], limit: usize) -> Vec<String> {
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

pub(super) fn bounded_plain_text(value: &str, limit: usize) -> String {
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

pub(super) fn collect_labels(value: Option<&Value>, output: &mut Vec<String>) {
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

pub(super) fn push_label(output: &mut Vec<String>, value: &str) {
    let value = value.trim().chars().take(32).collect::<String>();
    if value.chars().count() >= 2
        && !output
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(&value))
    {
        output.push(value);
    }
}

pub(super) fn infer_seed_keys(evidence: &[ReportDnaEvidence]) -> Vec<String> {
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
