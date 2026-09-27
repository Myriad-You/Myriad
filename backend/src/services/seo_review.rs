//! Scheduled Agent SEO review + owner-confirmed apply.
//!
//! Heartbeat inspects and drafts. Saving happens only when the owner taps Apply
//! on the notification (`api::seo_review`) or confirms `seo.apply` in chat.

use std::collections::HashMap;

use sea_orm::DatabaseConnection;
use serde_json::{Value, json};

use crate::config::ModelTier;
use crate::services::ai::create_ai_analyzer_for_tier;
use crate::services::seo_copy::{GenerateSiteSeoRequest, generate_site_seo_copy_with_db};

const DESC_CAP: usize = 200;
const KEYWORDS_CAP: usize = 300;
const INTRO_CAP: usize = 500;

const JUDGE_SYSTEM: &str = r#"You judge whether Agent SEO/GEO settings copy still matches one personal site.
Return ONLY one JSON object. No markdown.
Keys: site_description, site_keywords, site_ai_intro, why.
Each of the three fields is the string "ok" or "bad".
why is one sentence in the same language as the current fields.

bad if: empty while Public surface has writing/notes/apps; contradicts Public surface (wrong modules or outdated topics); welcome-to-this-site / marketing fluff.
ok if accurate and current.
Do not mention robots, sitemap, visibility, or llms.txt in why."#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeoReviewOutcome {
    Unchanged,
    Draft {
        why: String,
        site_description: Option<String>,
        site_keywords: Option<String>,
        site_ai_intro: Option<String>,
    },
}

#[derive(Debug, Clone, Default)]
struct FieldFlags {
    description: bool,
    keywords: bool,
    ai_intro: bool,
    why: String,
}

impl FieldFlags {
    fn any(&self) -> bool {
        self.description || self.keywords || self.ai_intro
    }
}

pub async fn apply_site_seo_fields(
    db: &DatabaseConnection,
    site_description: Option<&str>,
    site_keywords: Option<&str>,
    site_ai_intro: Option<&str>,
) -> Result<Vec<String>, String> {
    let mut updates = HashMap::new();
    let mut saved = Vec::new();
    cap_optional(
        site_description,
        "site_description",
        DESC_CAP,
        &mut updates,
        &mut saved,
    );
    cap_optional(
        site_keywords,
        "site_keywords",
        KEYWORDS_CAP,
        &mut updates,
        &mut saved,
    );
    cap_optional(
        site_ai_intro,
        "site_ai_intro",
        INTRO_CAP,
        &mut updates,
        &mut saved,
    );
    if updates.is_empty() {
        return Err(
            "Provide site_description, site_keywords, and/or site_ai_intro to save".to_string(),
        );
    }

    let service = crate::services::config_service::ConfigService::new(db.clone());
    if let Err(error) = service.update_configs(updates).await {
        tracing::error!(%error, "seo apply failed to write configurations");
        return Err(format!(
            "Failed to save site SEO fields in configurations: {error}"
        ));
    }
    match service.load_config().await {
        Ok(config) => {
            *crate::GLOBAL_DYNAMIC_CONFIG.write().await = config;
        }
        Err(error) => {
            tracing::warn!(%error, "seo apply saved but could not reload dynamic config");
        }
    }
    Ok(saved)
}

/// Inspect → judge → draft. Never writes. Caller notifies; owner Apply writes.
pub async fn run_scheduled_seo_review(db: &DatabaseConnection) -> Result<SeoReviewOutcome, String> {
    let inspect = crate::services::public_site::public_geo_inspect(db).await;
    let branding = inspect.get("branding").cloned().unwrap_or(json!({}));
    let title = string_field(&branding, "title");
    let description = string_field(&branding, "description");
    let keywords = string_field(&branding, "keywords");
    let ai_intro = string_field(&branding, "ai_intro");
    let facts = inspect
        .get("facts")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let has_content = inspect_has_content(&inspect);

    let mut flags = heuristic_flags(&description, &keywords, &ai_intro, has_content);
    if let Some(judged) = judge_with_ai(
        &title,
        &description,
        &keywords,
        &ai_intro,
        &facts,
        has_content,
    )
    .await
    {
        flags.description |= judged.description;
        flags.keywords |= judged.keywords;
        flags.ai_intro |= judged.ai_intro;
        if !judged.why.trim().is_empty() {
            flags.why = judged.why;
        }
    }
    if flags.why.trim().is_empty() {
        flags.why = heuristic_why(
            &flags,
            crate::services::seo_copy::resolve_language("", &title, &description),
        );
    }
    if !flags.any() {
        return Ok(SeoReviewOutcome::Unchanged);
    }

    let mut fields = Vec::new();
    if flags.description {
        fields.push("site_description".to_string());
    }
    if flags.keywords {
        fields.push("site_keywords".to_string());
    }
    if flags.ai_intro {
        fields.push("site_ai_intro".to_string());
    }

    let generated = generate_site_seo_copy_with_db(
        db,
        GenerateSiteSeoRequest {
            site_title: title,
            site_description: description.clone(),
            hint: String::new(),
            language: String::new(),
            fields,
        },
    )
    .await?;

    let site_description = take_if_changed(generated.site_description, &description);
    let site_keywords = take_if_changed(generated.site_keywords, &keywords);
    let site_ai_intro = take_if_changed(generated.site_ai_intro, &ai_intro);
    if site_description.is_none() && site_keywords.is_none() && site_ai_intro.is_none() {
        return Ok(SeoReviewOutcome::Unchanged);
    }

    Ok(SeoReviewOutcome::Draft {
        why: flags.why,
        site_description,
        site_keywords,
        site_ai_intro,
    })
}

fn cap_optional(
    value: Option<&str>,
    key: &str,
    max_chars: usize,
    updates: &mut HashMap<String, Value>,
    saved: &mut Vec<String>,
) {
    let Some(raw) = value else {
        return;
    };
    let capped: String = raw.chars().take(max_chars).collect();
    updates.insert(key.to_string(), json!(capped));
    saved.push(key.to_string());
}

fn string_field(obj: &Value, key: &str) -> String {
    obj.get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn inspect_has_content(inspect: &Value) -> bool {
    ["writing", "notes", "apps"].iter().any(|key| {
        inspect
            .get(*key)
            .and_then(Value::as_array)
            .is_some_and(|rows| !rows.is_empty())
    })
}

fn looks_like_fluff(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    value.contains("欢迎访问")
        || value.contains("歡迎訪問")
        || value.contains("本站提供")
        || lower.contains("welcome to this")
        || lower.contains("welcome to my website")
}

fn field_heuristic_bad(value: &str, has_content: bool) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return has_content;
    }
    looks_like_fluff(trimmed)
}

fn heuristic_flags(
    description: &str,
    keywords: &str,
    ai_intro: &str,
    has_content: bool,
) -> FieldFlags {
    FieldFlags {
        description: field_heuristic_bad(description, has_content),
        keywords: field_heuristic_bad(keywords, has_content),
        ai_intro: field_heuristic_bad(ai_intro, has_content),
        why: String::new(),
    }
}

fn heuristic_why(flags: &FieldFlags, language: &str) -> String {
    match language {
        "zh" | "zh-TW" => {
            if flags.description && flags.keywords && flags.ai_intro {
                "公开简介对不上现在的自有内容。".to_string()
            } else if flags.description {
                "站点描述空了或还在套话，对不上公开内容。".to_string()
            } else if flags.ai_intro {
                "AI 简介空了或过时了，对不上公开内容。".to_string()
            } else {
                "关键词空了或对不上公开内容。".to_string()
            }
        }
        "ja" => "公開中の紹介文が、いまの公開コンテンツとずれています。".to_string(),
        "ko" => "공개 소개문이 지금 공개된 글과 맞지 않습니다.".to_string(),
        "fr" => "Le texte public ne correspond plus à vos contenus visibles.".to_string(),
        "de" => "Die öffentlichen Texte passen nicht mehr zu den sichtbaren Inhalten.".to_string(),
        _ => "Public SEO copy no longer matches your guest-visible writing.".to_string(),
    }
}

fn normalize_copy(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn take_if_changed(generated: Option<String>, current: &str) -> Option<String> {
    let draft = generated?.trim().to_string();
    if draft.is_empty() {
        return None;
    }
    if normalize_copy(&draft) == normalize_copy(current) {
        return None;
    }
    Some(draft)
}

async fn judge_with_ai(
    title: &str,
    description: &str,
    keywords: &str,
    ai_intro: &str,
    facts: &str,
    has_content: bool,
) -> Option<FieldFlags> {
    let analyzer = create_ai_analyzer_for_tier(ModelTier::Lite).await?;
    let language = crate::services::seo_copy::resolve_language("", title, description);
    let surface = if facts.trim().is_empty() {
        "(none listed)"
    } else {
        facts
    };
    let user = format!(
        "Language for why: {language}\n\
Has guest-visible writing/notes/apps: {has_content}\n\n\
Current fields:\n\
- site_description: {description}\n\
- site_keywords: {keywords}\n\
- site_ai_intro: {ai_intro}\n\n\
Public surface:\n{surface}\n\n\
Return JSON with site_description, site_keywords, site_ai_intro as ok or bad, and why."
    );
    let owner = crate::services::ai_cost_ledger::resolve_site_owner_id()
        .await
        .ok()?;
    match crate::services::ai_cost_ledger::with_site_ai_ledger(
        owner,
        "seo",
        "judge",
        analyzer.analyze_with_system(JUDGE_SYSTEM, &user),
    )
    .await
    {
        Ok(raw) => parse_judge_json(&raw),
        Err(error) => {
            tracing::warn!(%error, "SEO review judge failed");
            None
        }
    }
}

fn parse_judge_json(raw: &str) -> Option<FieldFlags> {
    let cleaned = raw
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```JSON")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    let v: Value = match serde_json::from_str(cleaned) {
        Ok(v) => v,
        Err(_) => {
            let start = cleaned.find('{')?;
            let end = cleaned.rfind('}')?;
            if end <= start {
                return None;
            }
            serde_json::from_str(cleaned.get(start..=end)?).ok()?
        }
    };
    let bad = |key: &str| {
        v.get(key)
            .and_then(Value::as_str)
            .is_some_and(|s| s.eq_ignore_ascii_case("bad"))
    };
    Some(FieldFlags {
        description: bad("site_description"),
        keywords: bad("site_keywords"),
        ai_intro: bad("site_ai_intro"),
        why: v
            .get("why")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fluff_and_empty() {
        assert!(looks_like_fluff("欢迎访问我的主页"));
        assert!(field_heuristic_bad("", true));
        assert!(!field_heuristic_bad("", false));
        assert!(!field_heuristic_bad(
            "独立开发者，写 Rust 与个人手帐。",
            true
        ));
    }

    #[test]
    fn unchanged_drafts_dropped() {
        assert!(take_if_changed(Some("  Hello world  ".into()), "Hello world").is_none());
        assert_eq!(
            take_if_changed(Some("New copy".into()), "Old copy").as_deref(),
            Some("New copy")
        );
        assert!(take_if_changed(Some("   ".into()), "").is_none());
    }

    #[test]
    fn parse_judge_reads_bad_fields() {
        let raw = r#"{"site_description":"bad","site_keywords":"ok","site_ai_intro":"bad","why":"描述还在欢迎访问"}"#;
        let j = parse_judge_json(raw).unwrap();
        assert!(j.description && !j.keywords && j.ai_intro);
        assert!(j.why.contains("欢迎"));
    }

    #[test]
    fn inspect_content_from_arrays() {
        let empty = json!({"writing": [], "notes": [], "apps": []});
        assert!(!inspect_has_content(&empty));
        let notes = json!({"writing": [], "notes": [{"title": "a"}], "apps": []});
        assert!(inspect_has_content(&notes));
    }
}
