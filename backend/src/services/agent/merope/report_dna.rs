//! Distill personality tags from platform reports.
//!
//! The tag-distillation rules live here, not in `myriad-merope`.
//! The shared onboarding sanitizer comes from the crate.
//!
//! Latest report per platform; evidence is summary / insights / notes /
//! structured labels. Pro writes spoken temperament tags via
//! `onboarding_prompts::TAGS_SYSTEM_PROMPT`. Visual assets stay out.

use myriad_agent_rules::extract_json_object_from_ai_response;
use std::collections::HashSet;
use std::time::Duration;

use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseBackend, DatabaseConnection, DbErr, EntityTrait,
    QueryFilter, QueryOrder, Statement,
};
use serde::Serialize;
use serde_json::{json, Value};

pub use myriad_merope::sanitize_onboarding_tags;
use myriad_merope::{MAX_ONBOARDING_TAGS, MAX_ONBOARDING_TAG_CHARS};

use crate::config::ModelTier;
use crate::models::entities::platform_reports;
use crate::services::ai::create_ai_analyzer_for_tier_with_timeout;
use crate::GLOBAL_DYNAMIC_CONFIG;

use super::onboarding_prompts::TAGS_SYSTEM_PROMPT;

const MAX_REPORT_DNA_REPORTS: usize = 12;
const MAX_REPORT_SUMMARY_CHARS: usize = 800;
const MAX_REPORT_INSIGHT_CHARS: usize = 1_600;
const MAX_REPORT_NOTE_CHARS: usize = 800;
/// Keep in sync with `PERSONA_GENERATION_TIMEOUT_MS` / `MEROPE_PROXY_TIMEOUT_MS`.
const REPORT_DNA_AI_TIMEOUT: Duration = Duration::from_secs(15 * 60);

#[derive(Debug, Clone)]
struct ReportDnaSource {
    platform: String,
    report: Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReportDnaEvidence {
    platform: String,
    summary: String,
    insights: Vec<String>,
    note: String,
    structured_labels: Vec<String>,
}

#[derive(Debug, Clone)]
struct ReportDnaBundle {
    report_count: usize,
    platforms: Vec<String>,
    evidence: Vec<ReportDnaEvidence>,
    fallback_seed_keys: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct DistilledReportDna {
    pub tags: Vec<String>,
    pub report_count: usize,
    pub ai_distilled: bool,
}

#[derive(Debug)]
pub enum DistillReportDnaError {
    Db(DbErr),
    #[allow(dead_code)]
    AnalyzerUnavailable,
    #[allow(dead_code)]
    ProviderFailed,
    #[allow(dead_code)]
    EmptyResponse,
}

impl std::fmt::Display for DistillReportDnaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Db(error) => write!(f, "{error}"),
            Self::AnalyzerUnavailable => write!(f, "onboarding analyzer unavailable"),
            Self::ProviderFailed => write!(f, "report dna provider failed"),
            Self::EmptyResponse => write!(f, "report dna empty response"),
        }
    }
}

impl From<DbErr> for DistillReportDnaError {
    fn from(value: DbErr) -> Self {
        Self::Db(value)
    }
}

pub const MIN_PERSONA_REPORTS: usize = 3;

fn is_chunk_platform(platform: &str) -> bool {
    platform
        .rsplit_once("_chunk_")
        .is_some_and(|(base, suffix)| {
            !base.is_empty() && suffix.chars().all(|character| character.is_ascii_digit())
        })
}

/// Distinct platforms with a real report. Chunks and the `all` rollup do not count.
///
/// Only the `platform` column is read; do not load `metadata` or `report`.
///
/// The chunk rule stays in Rust rather than becoming a SQL regex: it is the
/// same predicate the rest of this module uses, and one copy of it is enough.
pub async fn count_report_platforms(
    database: &DatabaseConnection,
    user_id: i32,
) -> Result<usize, DbErr> {
    let rows = database
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT DISTINCT platform FROM platform_reports \
             WHERE user_id = $1 AND platform <> 'all'",
            [user_id.into()],
        ))
        .await?;
    let mut seen = HashSet::new();
    for row in rows {
        let platform: String = row.try_get("", "platform")?;
        if is_chunk_platform(&platform) {
            continue;
        }
        seen.insert(platform);
    }
    Ok(seen.len())
}

async fn collect_report_dna_bundle(
    database: &DatabaseConnection,
    user_id: i32,
) -> Result<ReportDnaBundle, DbErr> {
    let rows = platform_reports::Entity::find()
        .filter(platform_reports::Column::UserId.eq(user_id))
        .filter(platform_reports::Column::Platform.ne("all"))
        .order_by_desc(platform_reports::Column::CreatedAt)
        .order_by_desc(platform_reports::Column::Id)
        .all(database)
        .await?;
    let mut seen = HashSet::new();
    let mut sources = Vec::new();
    for row in rows {
        if is_chunk_platform(&row.platform) {
            continue;
        }
        if !seen.insert(row.platform.clone()) {
            continue;
        }
        sources.push(ReportDnaSource {
            platform: row.platform,
            report: row.report,
        });
        if sources.len() >= MAX_REPORT_DNA_REPORTS {
            break;
        }
    }
    Ok(build_report_dna_bundle(&sources))
}

/// Pro writes the deck when it can. Any model miss falls back to a shuffled
/// local deck so the onboarding page is never stuck on a 502.
pub async fn distill_report_dna(
    database: &DatabaseConnection,
    user_id: i32,
    language: &str,
    regenerate: bool,
) -> Result<DistilledReportDna, DistillReportDnaError> {
    let bundle = collect_report_dna_bundle(database, user_id).await?;
    if bundle.report_count == 0 {
        return Ok(DistilledReportDna {
            tags: Vec::new(),
            report_count: 0,
            ai_distilled: false,
        });
    }

    let call_id = uuid::Uuid::new_v4().to_string();
    let target_tag_count = if bundle.report_count >= 3 { 16 } else { 12 };
    let evidence_tags = localize_report_seed_keys(&bundle.fallback_seed_keys, language);
    let fallback = || fallback_tag_deck(&evidence_tags, language, &call_id, target_tag_count);
    let report_count = bundle.report_count;
    let fallback_ok = || DistilledReportDna {
        tags: fallback(),
        report_count,
        ai_distilled: false,
    };

    {
        let config = GLOBAL_DYNAMIC_CONFIG.read().await;
        if !config.pro_enabled {
            return Ok(fallback_ok());
        }
    }
    let Some(analyzer) =
        create_ai_analyzer_for_tier_with_timeout(ModelTier::Pro, Some(REPORT_DNA_AI_TIMEOUT)).await
    else {
        return Ok(fallback_ok());
    };
    let prompt = json!({
        "pipeline": "onboarding/tags",
        "language": language,
        "reportCount": bundle.report_count,
        "platforms": bundle.platforms,
        "targetTagCount": target_tag_count,
        "minTagCount": 8,
        "maxTagCount": 16,
        "regenerate": regenerate,
        "callId": call_id,
        "evidence": bundle.evidence,
    })
    .to_string();
    let result = crate::services::ai_cost_ledger::with_site_ai_ledger(
        user_id,
        "merope",
        "report_dna",
        analyzer.analyze_with_system(TAGS_SYSTEM_PROMPT, &prompt),
    )
    .await;
    match result {
        Ok(raw) => {
            let tags = parse_ai_tags(&raw);
            if tags.is_empty() {
                tracing::warn!(
                    user_id,
                    regenerate,
                    raw_chars = raw.chars().count(),
                    "Report DNA Pro returned no usable tags"
                );
                return Ok(fallback_ok());
            }
            Ok(DistilledReportDna {
                ai_distilled: true,
                tags: complete_ai_tag_deck(&tags, language, target_tag_count),
                report_count,
            })
        }
        Err(error) => {
            tracing::warn!(%error, user_id, regenerate, "Report DNA Pro distillation failed");
            Ok(fallback_ok())
        }
    }
}

const PERSONA_POOL_KEYS: &[&str] = &[
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

fn seed_shuffle<T>(items: &mut [T], seed: &str) {
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

fn complete_ai_tag_deck(primary: &[String], language: &str, target: usize) -> Vec<String> {
    let target = target.clamp(8, MAX_ONBOARDING_TAGS);
    let mut result = Vec::with_capacity(target);
    for tag in primary {
        if tag_matches_ui_language(tag, language) {
            unique_push(&mut result, tag);
        }
        if result.len() >= target {
            break;
        }
    }
    // Only pad to the playable minimum. Do not flood a good short deck
    // with generic pool leftovers just to hit targetTagCount.
    const MIN_PLAYABLE: usize = 8;
    if result.len() < MIN_PLAYABLE {
        for extra in persona_pool_labels(language) {
            unique_push(&mut result, &extra);
            if result.len() >= MIN_PLAYABLE {
                break;
            }
        }
    }
    sanitize_onboarding_tags_for_language(&result, language)
}

fn fallback_tag_deck(
    evidence_tags: &[String],
    language: &str,
    seed: &str,
    target: usize,
) -> Vec<String> {
    let target = target.clamp(8, MAX_ONBOARDING_TAGS);
    let mut result = Vec::with_capacity(target);
    for tag in evidence_tags {
        if tag_matches_ui_language(tag, language) {
            unique_push(&mut result, tag);
        }
    }
    for extra in persona_pool_labels(language) {
        unique_push(&mut result, &extra);
    }
    seed_shuffle(&mut result, seed);
    result.truncate(target);
    sanitize_onboarding_tags_for_language(&result, language)
}

fn traditional_ui_language(language: &str) -> bool {
    let lower = language.trim().to_ascii_lowercase().replace('_', "-");
    lower.starts_with("zh-tw")
        || lower.starts_with("zh-hk")
        || lower.starts_with("zh-mo")
        || lower.contains("hant")
}

fn zh_seed_label(key: &str, traditional: bool) -> Option<&'static str> {
    Some(match (key, traditional) {
        ("thoughtful", false) => "善于思考",
        ("thoughtful", true) => "善於思考",
        ("curious", _) => "好奇",
        ("creative", false) => "有创造力",
        ("creative", true) => "有創造力",
        ("calm", false) => "沉静",
        ("calm", true) => "沉靜",
        ("playful", false) => "活泼",
        ("playful", true) => "活潑",
        ("focused", false) => "专注",
        ("focused", true) => "專注",
        ("independent", false) => "独立",
        ("independent", true) => "獨立",
        ("social", false) => "重视连接",
        ("social", true) => "重視連結",
        ("persistent", false) => "有韧性",
        ("persistent", true) => "有韌性",
        ("expressive", false) => "善于表达",
        ("expressive", true) => "善於表達",
        ("bound", false) => "边界感强",
        ("bound", true) => "邊界感強",
        ("loyal", false) => "朋友不多但很铁",
        ("loyal", true) => "朋友不多但很鐵",
        ("perfect", false) => "完美主义",
        ("perfect", true) => "完美主義",
        ("solo", false) => "独处才放松",
        ("solo", true) => "獨處才放鬆",
        ("order", false) => "讨厌混乱",
        ("order", true) => "討厭混亂",
        ("warm", false) => "外冷内热",
        ("warm", true) => "外冷內熱",
        ("near", _) => "想交心又怕熟",
        ("deep_focus", _) => "做事很沉",
        ("night", false) => "夜猫子",
        ("night", true) => "夜貓子",
        ("soft", false) => "嘴硬心软",
        ("soft", true) => "嘴硬心軟",
        ("space", false) => "礼貌但疏离",
        ("space", true) => "禮貌但疏離",
        ("feel", false) => "情绪来了先憋着",
        ("feel", true) => "情緒來了先憋著",
        ("arranged", false) => "喜欢把事情安排妥",
        ("arranged", true) => "喜歡把事情安排妥",
        ("slow_warm", false) => "慢热",
        ("slow_warm", true) => "慢熱",
        ("shy", _) => "社恐",
        ("try_hard", false) => "认真起来很拼",
        ("try_hard", true) => "認真起來很拼",
        _ => return None,
    })
}

fn localize_report_seed_keys(keys: &[String], language: &str) -> Vec<String> {
    keys.iter()
        .filter_map(|key| {
            let label = match language {
                value if traditional_ui_language(value) => zh_seed_label(key, true)?,
                value if value.starts_with("zh") => zh_seed_label(key, false)?,
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
                    "warm" => "Cool face, warm core",
                    "near" => "Wants closeness, wary",
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

fn looks_like_job_or_identity_label(label: &str) -> bool {
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
    let lower = label.to_lowercase();
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
        || lower.contains("engineer")
        || lower.contains("programmer")
        || lower.contains("manager")
        || lower.contains("teacher")
        || lower.contains("doctor")
        || lower.contains("nurse")
        || lower.contains("lawyer")
        || lower.contains("student")
        || lower.contains("designer")
        || label.contains("教師")
        || label.contains("医者")
        || label.contains("看護師")
        || label.contains("弁護士")
}

fn looks_like_media_catalog_label(label: &str) -> bool {
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
        "アニメ",
        "ゲーム",
        "マンガ",
    ];
    NEEDLES.iter().any(|needle| lower.contains(needle))
}

fn is_reasonable_persona_tag(label: &str) -> bool {
    let label = label.trim();
    let chars = label.chars().count();
    if !(2..=MAX_ONBOARDING_TAG_CHARS).contains(&chars) {
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

fn sanitize_report_dna_tags(tags: &[String]) -> Vec<String> {
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
    sanitize_onboarding_tags(&filtered)
}

pub fn sanitize_onboarding_tags_for_language(tags: &[String], language: &str) -> Vec<String> {
    sanitize_onboarding_tags(tags)
        .into_iter()
        .filter(|tag| tag_matches_ui_language(tag, language))
        .collect()
}

pub(crate) fn tag_matches_ui_language(label: &str, language: &str) -> bool {
    let has_latin = label.chars().any(|ch| ch.is_ascii_alphabetic());
    let has_kana = label
        .chars()
        .any(|ch| matches!(ch, '\u{3041}'..='\u{3096}' | '\u{30A1}'..='\u{30FA}' | '\u{30FC}'));
    let has_han = label.chars().any(|ch| {
        matches!(ch, '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' | '\u{F900}'..='\u{FAFF}')
    });
    match language {
        "en-US" => has_latin && !has_han && !has_kana,
        "ja-JP" => has_han || has_kana,
        _ => has_han && !has_latin && !has_kana,
    }
}

fn build_report_dna_bundle(sources: &[ReportDnaSource]) -> ReportDnaBundle {
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

fn parse_ai_tags(raw: &str) -> Vec<String> {
    let Some(json) = extract_json_object_from_ai_response(raw) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<Value>(&json) else {
        return Vec::new();
    };
    let Some(items) = value
        .get("tags")
        .or_else(|| value.get("seeds"))
        .or_else(|| value.get("labels"))
    else {
        return Vec::new();
    };
    let tags = match items {
        Value::Array(items) => items
            .iter()
            .take(36)
            .filter_map(|item| {
                item.as_str()
                    .or_else(|| {
                        item.as_object().and_then(|object| {
                            ["label", "name", "tag", "text", "value"]
                                .iter()
                                .find_map(|key| object.get(*key).and_then(Value::as_str))
                        })
                    })
                    .map(str::trim)
                    .filter(|label| !label.is_empty())
                    .map(str::to_string)
            })
            .collect::<Vec<_>>(),
        Value::String(value) => value
            .split(['、', '，', '\n', ';', '|'])
            .map(str::trim)
            .filter(|label| !label.is_empty())
            .take(36)
            .map(str::to_string)
            .collect(),
        _ => return Vec::new(),
    };
    sanitize_report_dna_tags(&tags)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 数报告平台数只该读 `platform` 这一列。
    #[test]
    fn counting_platforms_reads_one_column_not_whole_reports() {
        let source = include_str!("report_dna.rs");
        let body = source
            .split("pub async fn count_report_platforms(")
            .nth(1)
            .and_then(|rest| rest.split("\n}").next())
            .expect("count_report_platforms body");
        assert!(
            body.contains("SELECT DISTINCT platform"),
            "只要平台名，不要报告正文"
        );
        assert!(!body.contains(".all(database)"), "别再把整行拉回来数字符串");
        // chunk 规则留在 Rust，一份就够；翻译成 SQL 正则等于开第二份真相。
        assert!(body.contains("is_chunk_platform"));
    }

    #[test]
    fn chunk_platforms_never_count_as_a_platform() {
        assert!(is_chunk_platform("steam_chunk_1"));
        assert!(is_chunk_platform("steam_chunk_12"));
        // 后缀为空时 `all()` 在空迭代器上为真；`is_chunk_platform("steam_chunk_")` 为真。
        assert!(is_chunk_platform("steam_chunk_"));
        assert!(!is_chunk_platform("steam"));
        assert!(!is_chunk_platform("_chunk_1"));
        assert!(!is_chunk_platform("steam_chunk_x"));
    }

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
        assert_eq!(bundle.evidence[0].structured_labels, ["Rust"]);
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
        assert_eq!(
            localize_report_seed_keys(&keys, "zh-TW"),
            ["好奇", "有韌性", "慢熱"]
        );
        assert_ne!(
            localize_report_seed_keys(&keys, "zh-TW"),
            localize_report_seed_keys(&keys, "zh-CN")
        );
    }

    #[test]
    fn parses_json_wrapped_ai_tags_and_rejects_roles() {
        let tags = parse_ai_tags(
            "```json\n{\"tags\":[{\"label\":\"Curious\"},\"Software engineer\"]}\n```",
        );
        assert_eq!(tags, ["Curious"]);
        assert_eq!(
            parse_ai_tags("{\"tags\":\"Curious、慢热、Software engineer\"}"),
            ["Curious", "慢热"]
        );
    }

    #[test]
    fn recognizes_only_numbered_chunk_platforms() {
        assert!(is_chunk_platform("github_chunk_2"));
        assert!(!is_chunk_platform("github_chunk_latest"));
        assert!(!is_chunk_platform("github"));
    }

    #[test]
    fn drops_job_and_media_labels() {
        assert!(looks_like_job_or_identity_label("软件工程师"));
        assert!(looks_like_job_or_identity_label("教師"));
        assert!(looks_like_job_or_identity_label("Teacher"));
        assert!(looks_like_job_or_identity_label("Student"));
        assert!(looks_like_media_catalog_label("独立游戏"));
        assert!(looks_like_media_catalog_label("ゲーム好き"));
        assert!(is_reasonable_persona_tag("慢热"));
        assert!(!is_reasonable_persona_tag("程序员"));
    }

    #[test]
    fn fallback_tags_survive_sanitizers_in_each_ui_language() {
        let keys: Vec<String> = PERSONA_POOL_KEYS
            .iter()
            .map(|key| (*key).to_string())
            .collect();
        for language in ["zh-CN", "zh-TW", "ja-JP", "en-US"] {
            let labels = localize_report_seed_keys(&keys, language);
            assert_eq!(labels.len(), PERSONA_POOL_KEYS.len(), "{language}");
            let kept = sanitize_report_dna_tags(&labels);
            assert_eq!(kept.len(), labels.len(), "{language}");
            assert!(
                labels
                    .iter()
                    .all(|label| tag_matches_ui_language(label, language)),
                "{language}"
            );
        }
    }

    #[test]
    fn drops_wrong_script_tags_and_pads_from_pool() {
        let mixed = vec!["慢热".into(), "Night owl".into(), "境界線がはっきり".into()];
        let english = complete_ai_tag_deck(&mixed, "en-US", 8);
        assert!(english.contains(&"Night owl".to_string()));
        assert!(!english
            .iter()
            .any(|tag| tag == "慢热" || tag.contains('が')));
        assert!(english
            .iter()
            .all(|tag| tag_matches_ui_language(tag, "en-US")));
        assert!(english.len() >= 8);

        assert!(!tag_matches_ui_language("慢热", "en-US"));
        assert!(!tag_matches_ui_language("Night owl", "zh-CN"));
        assert!(tag_matches_ui_language("スロースターター", "ja-JP"));
        assert!(tag_matches_ui_language("夜型OK", "ja-JP"));
        assert!(!tag_matches_ui_language("Night owl", "ja-JP"));
    }
}
