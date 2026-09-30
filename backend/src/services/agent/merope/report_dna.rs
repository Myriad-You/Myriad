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
use serde_json::{Value, json};

pub use myriad_merope::sanitize_onboarding_tags;
use myriad_merope::{MAX_ONBOARDING_TAG_CHARS, MAX_ONBOARDING_TAGS};

use crate::GLOBAL_DYNAMIC_CONFIG;
use crate::config::ModelTier;
use crate::models::entities::platform_reports;
use crate::services::ai::create_ai_analyzer_for_tier_with_timeout;

use super::onboarding_prompts::TAGS_SYSTEM_PROMPT;

mod evidence;
mod tags;

use evidence::*;
pub use tags::*;

const MAX_REPORT_DNA_REPORTS: usize = 12;
const MAX_REPORT_SUMMARY_CHARS: usize = 800;
const MAX_REPORT_INSIGHT_CHARS: usize = 1_600;
const MAX_REPORT_NOTE_CHARS: usize = 800;
/// Keep in sync with `PERSONA_GENERATION_TIMEOUT_MS` / `MEROPE_PROXY_TIMEOUT_MS`.
const REPORT_DNA_AI_TIMEOUT: Duration = Duration::from_secs(15 * 60);

#[derive(Debug, Clone)]
pub struct DistilledReportDna {
    pub tags: Vec<String>,
    pub report_count: usize,
    pub ai_distilled: bool,
}

#[derive(Debug)]
pub enum DistillReportDnaError {
    Db(DbErr),
}

impl std::fmt::Display for DistillReportDnaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Db(error) => write!(f, "{error}"),
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
        assert!(
            !serde_json::to_string(&bundle.evidence)
                .unwrap()
                .contains("must not leave")
        );
        assert!(
            bundle
                .fallback_seed_keys
                .contains(&"thoughtful".to_string())
        );
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
        assert!(
            !english
                .iter()
                .any(|tag| tag == "慢热" || tag.contains('が'))
        );
        assert!(
            english
                .iter()
                .all(|tag| tag_matches_ui_language(tag, "en-US"))
        );
        assert!(english.len() >= 8);

        assert!(!tag_matches_ui_language("慢热", "en-US"));
        assert!(!tag_matches_ui_language("Night owl", "zh-CN"));
        assert!(tag_matches_ui_language("スロースターター", "ja-JP"));
        assert!(tag_matches_ui_language("夜型OK", "ja-JP"));
        assert!(!tag_matches_ui_language("Night owl", "ja-JP"));
    }
}
