//! Onboarding helpers: strict-Lite name roll; Pro persona draft and visual design.
//! Prompts live in `onboarding_prompts`.

use serde_json::{Map, Value, json};
use std::time::Duration;

use crate::GLOBAL_DYNAMIC_CONFIG;
use crate::config::ModelTier;
use crate::services::ai::{
    create_ai_analyzer_for_tier_with_timeout, create_strict_lite_ai_analyzer_with_timeout,
};
use crate::services::ai_config::get_ai_config_for_tier;
use crate::services::ai_cost_ledger::record_ai_call_from_attribution;
use crate::services::analyzer::{AiProvider, OutputBudget, openai_chat_completions_url};
use crate::services::gemini_media;
use crate::services::http_client::get_long_running_client;
use crate::services::image_generation::ImageReference;

use super::onboarding_prompts::{
    IMPORT_PERSONA_SYSTEM_PROMPT, PERSONA_SYSTEM_PROMPT, name_system_prompt,
    observe_portrait_visual_prompt, visual_design_system_prompt,
};
use super::report_dna::sanitize_onboarding_tags_for_language;

mod names;
mod vision;

pub use names::*;
pub use vision::*;

/// Keep in sync with `PERSONA_GENERATION_TIMEOUT_MS` / `MEROPE_PROXY_TIMEOUT_MS`.
const ONBOARDING_AI_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const PERSONA_ATTEMPTS: u8 = 2;
const MAX_PERSONA_LIST_ITEMS: usize = 12;
const MAX_PERSONA_LIST_ITEM_CHARS: usize = 180;
const MIN_SUMMARY_CHARS: usize = 80;
const MIN_TEMPERAMENT_ITEMS: usize = 5;
const MIN_PAIR_ITEMS: usize = 4;
const MIN_GUIDANCE_CHARS: usize = 36;

#[derive(Debug)]
pub enum OnboardingAiError {
    AnalyzerUnavailable,
    ProviderFailed(String),
    UnusableResponse(&'static str),
    LanguageMismatch,
}

impl OnboardingAiError {
    pub fn public_detail(&self) -> Option<&str> {
        match self {
            Self::ProviderFailed(detail) if !detail.trim().is_empty() => Some(detail),
            Self::UnusableResponse(reason) => Some(reason),
            _ => None,
        }
    }
}

impl std::fmt::Display for OnboardingAiError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AnalyzerUnavailable => formatter.write_str("analyzer unavailable"),
            Self::ProviderFailed(detail) if detail.trim().is_empty() => {
                formatter.write_str("provider failed")
            }
            Self::ProviderFailed(detail) => write!(formatter, "provider failed: {detail}"),
            Self::UnusableResponse(reason) => write!(formatter, "unusable response ({reason})"),
            Self::LanguageMismatch => formatter.write_str("language mismatch"),
        }
    }
}

pub async fn suggest_persona(
    name: &str,
    language: &str,
    selected_tags: &[String],
    gender: &str,
    extra_requirements: &str,
) -> Result<Value, OnboardingAiError> {
    retry_unusable(PERSONA_ATTEMPTS, "persona draft was unusable", || {
        suggest_persona_once(name, language, selected_tags, gender, extra_requirements)
    })
    .await
}

async fn suggest_persona_once(
    name: &str,
    language: &str,
    selected_tags: &[String],
    gender: &str,
    extra_requirements: &str,
) -> Result<Value, OnboardingAiError> {
    let tags = sanitize_onboarding_tags_for_language(selected_tags, language);
    let fallback = myriad_merope::fallback_persona_draft(name, language, &tags);
    let input = json!({
        "pipeline": "onboarding/persona",
        "task": "design_character_persona",
        "rollId": format!("p{}", uuid::Uuid::new_v4().simple()),
        "name": name.chars().take(50).collect::<String>(),
        "language": language,
        "genderPresentation": normalize_gender(gender),
        "selectedTags": tags,
        "extraRequirements": extra_requirements.chars().take(500).collect::<String>(),
        "fullness": {
            "summaryChars": "80-220",
            "temperamentCount": "5-8",
            "likesCount": "4-6",
            "likesAreHabitsNotScenes": true,
            "drivesCount": "4-6",
            "guidance": "1-2 sentences each for socialStyle and speechStyle",
            "noLiterarySludge": true,
        },
    })
    .to_string();
    let raw = run_onboarding_call(PERSONA_SYSTEM_PROMPT, &input).await?;
    let parsed = parse_json_object(&raw).ok_or(OnboardingAiError::UnusableResponse(
        "persona draft was not valid JSON",
    ))?;
    if !persona_draft_meets_generation_quality(&parsed) {
        tracing::warn!(language, "persona draft failed fullness checks");
        return Err(OnboardingAiError::UnusableResponse(
            "persona draft failed fullness checks",
        ));
    }
    if !persona_matches_ui_language(&parsed, language) {
        tracing::warn!(language, "persona draft failed language check");
        return Err(OnboardingAiError::UnusableResponse(
            "persona draft failed language check",
        ));
    }
    if myriad_merope::persona_has_literary_sludge(&parsed) {
        tracing::warn!(language, "persona draft failed literary-sludge check");
        return Err(OnboardingAiError::UnusableResponse(
            "persona draft used literary sludge",
        ));
    }
    let persona = myriad_merope::sanitize_persona_draft(&parsed, &fallback)
        .filter(myriad_merope::persona_draft_is_complete)
        .ok_or_else(|| {
            tracing::warn!(language, "persona draft failed sanitize/complete check");
            OnboardingAiError::UnusableResponse("persona draft failed sanitize/complete check")
        })?;
    Ok(persona)
}

pub async fn import_persona(
    name: &str,
    language: &str,
    gender: &str,
    source: &str,
) -> Result<Value, OnboardingAiError> {
    retry_unusable(PERSONA_ATTEMPTS, "imported persona was unusable", || {
        import_persona_once(name, language, gender, source)
    })
    .await
}

async fn import_persona_once(
    name: &str,
    language: &str,
    gender: &str,
    source: &str,
) -> Result<Value, OnboardingAiError> {
    let source = source.trim();
    if source.is_empty() {
        return Err(OnboardingAiError::UnusableResponse(
            "import source was empty",
        ));
    }
    let fallback = myriad_merope::fallback_persona_draft(name, language, &[]);
    let input = json!({
        "pipeline": "onboarding/persona-import",
        "task": "import_character_persona",
        "name": name.chars().take(50).collect::<String>(),
        "language": language,
        "genderPresentation": normalize_gender(gender),
        "source": source.chars().take(6_000).collect::<String>(),
        // 重试时换一个，否则第二次会照抄第一次那份没过闸的稿。
        "rollId": format!("i{}", uuid::Uuid::new_v4().simple()),
    })
    .to_string();
    let raw = run_onboarding_call(IMPORT_PERSONA_SYSTEM_PROMPT, &input).await?;
    let parsed = parse_json_object(&raw).ok_or(OnboardingAiError::UnusableResponse(
        "imported persona was not valid JSON",
    ))?;
    if !persona_draft_meets_generation_quality(&parsed) {
        return Err(OnboardingAiError::UnusableResponse(
            "imported persona failed fullness checks",
        ));
    }
    if !persona_matches_ui_language(&parsed, language) {
        return Err(OnboardingAiError::UnusableResponse(
            "imported persona failed language check",
        ));
    }
    if myriad_merope::persona_has_literary_sludge(&parsed) {
        return Err(OnboardingAiError::UnusableResponse(
            "imported persona used literary sludge",
        ));
    }
    myriad_merope::sanitize_persona_draft(&parsed, &fallback)
        .filter(myriad_merope::persona_draft_is_complete)
        .ok_or(OnboardingAiError::UnusableResponse(
            "imported persona failed sanitize/complete check",
        ))
}

/// 只重试「模型这一把没写好」。
///
/// 供应商不可用、调用本身失败，重试只会拖长等待并多烧一次钱——那两种直接
/// 上抛，交给上层去说「模型没配好」。
async fn retry_unusable<T, F, Fut>(
    attempts: u8,
    fallback_reason: &'static str,
    mut once: F,
) -> Result<T, OnboardingAiError>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, OnboardingAiError>>,
{
    let mut last_error = OnboardingAiError::UnusableResponse(fallback_reason);
    for attempt in 0..attempts {
        match once().await {
            Ok(value) => return Ok(value),
            Err(
                error @ (OnboardingAiError::AnalyzerUnavailable
                | OnboardingAiError::ProviderFailed(_)),
            ) => return Err(error),
            Err(error) => {
                tracing::warn!(attempt, %error, "onboarding draft was unusable; retrying");
                last_error = error;
            }
        }
    }
    Err(last_error)
}

async fn run_onboarding_call(system: &str, input: &str) -> Result<String, OnboardingAiError> {
    run_onboarding_call_on_tier(ModelTier::Pro, system, input).await
}

async fn run_onboarding_call_on_tier(
    tier: ModelTier,
    system: &str,
    input: &str,
) -> Result<String, OnboardingAiError> {
    if matches!(tier, ModelTier::Pro) {
        let config = crate::GLOBAL_DYNAMIC_CONFIG.read().await;
        if !config.pro_enabled {
            return Err(OnboardingAiError::AnalyzerUnavailable);
        }
    }
    let Some(analyzer) =
        create_ai_analyzer_for_tier_with_timeout(tier, Some(ONBOARDING_AI_TIMEOUT)).await
    else {
        return Err(OnboardingAiError::AnalyzerUnavailable);
    };
    let owner = crate::services::ai_cost_ledger::resolve_site_owner_id()
        .await
        .map_err(|_| OnboardingAiError::AnalyzerUnavailable)?;
    match crate::services::ai_cost_ledger::with_site_ai_ledger(
        owner,
        "merope",
        "onboarding",
        analyzer.analyze_with_system(system, input),
    )
    .await
    {
        Ok(raw) if !raw.trim().is_empty() => Ok(raw),
        Ok(_) => {
            tracing::warn!(?tier, "onboarding model returned empty text");
            Err(OnboardingAiError::ProviderFailed(
                "onboarding model returned empty text".into(),
            ))
        }
        Err(error) => {
            tracing::error!(%error, ?tier, "onboarding model call failed");
            Err(OnboardingAiError::ProviderFailed(error.to_string()))
        }
    }
}

fn normalize_gender(value: &str) -> &str {
    match value {
        "female" | "male" | "nonbinary" | "unspecified" => value,
        _ => "unspecified",
    }
}

fn parse_json_object(raw: &str) -> Option<Value> {
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    serde_json::from_str(&raw[start..=end]).ok()
}

fn persona_matches_ui_language(value: &Value, language: &str) -> bool {
    let source = value.get("persona").unwrap_or(value);
    let mut text = String::new();
    for key in ["summary", "socialStyle", "speechStyle"] {
        if let Some(part) = source.get(key).and_then(Value::as_str) {
            text.push_str(part);
        }
    }
    for key in ["temperament", "likes", "drives", "traits"] {
        if let Some(items) = list_from_value(source, &[key]) {
            for item in items {
                text.push_str(&item);
            }
        }
    }
    let mut latin = 0usize;
    let mut cjk = 0usize;
    for ch in text.chars() {
        if ch.is_ascii_alphabetic() {
            latin += 1;
        } else if is_cjk_han(ch) || is_hiragana(ch) || is_katakana_letter(ch) {
            cjk += 1;
        }
    }
    let total = latin + cjk;
    if total < 8 {
        return true;
    }
    match language {
        "en-US" => latin * 2 >= total,
        _ => cjk * 2 >= total,
    }
}

fn persona_draft_meets_generation_quality(value: &Value) -> bool {
    let Some(source) = value.get("persona").unwrap_or(value).as_object() else {
        return false;
    };
    let summary_ready = source
        .get("summary")
        .and_then(Value::as_str)
        .is_some_and(|summary| summary.trim().chars().count() >= MIN_SUMMARY_CHARS);
    let temperament_ready = list_from(source, &["temperament", "traits"])
        .is_some_and(|items| items.len() >= MIN_TEMPERAMENT_ITEMS);
    let likes_ready =
        list_from(source, &["likes"]).is_some_and(|items| items.len() >= MIN_PAIR_ITEMS);
    let drives_ready = list_from(source, &["drives", "motivations"])
        .is_some_and(|items| items.len() >= MIN_PAIR_ITEMS);
    let social_ready = ["socialStyle", "social_style"]
        .iter()
        .find_map(|key| source.get(*key).and_then(Value::as_str))
        .is_some_and(|value| value.trim().chars().count() >= MIN_GUIDANCE_CHARS);
    let speech_ready = ["speechStyle", "speech_style", "voice"]
        .iter()
        .find_map(|key| source.get(*key).and_then(Value::as_str))
        .is_some_and(|value| value.trim().chars().count() >= MIN_GUIDANCE_CHARS);
    summary_ready
        && temperament_ready
        && likes_ready
        && drives_ready
        && social_ready
        && speech_ready
}

fn list_from(source: &Map<String, Value>, keys: &[&str]) -> Option<Vec<String>> {
    let value = keys.iter().find_map(|key| source.get(*key))?;
    list_value(value)
}

fn list_from_value(source: &Value, keys: &[&str]) -> Option<Vec<String>> {
    let value = keys.iter().find_map(|key| source.get(*key))?;
    list_value(value)
}

fn list_value(value: &Value) -> Option<Vec<String>> {
    let raw = match value {
        Value::Array(items) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect::<Vec<_>>(),
        // Items are separated by 、 (or ; in English); a comma is inside one.
        Value::String(value) => value
            .split(['、', ';', '/', '|'])
            .map(str::to_string)
            .collect(),
        _ => return None,
    };
    let mut sanitized = Vec::new();
    for item in raw {
        let item = bounded_text(&item, MAX_PERSONA_LIST_ITEM_CHARS);
        if item.is_empty() || sanitized.iter().any(|existing| existing == &item) {
            continue;
        }
        sanitized.push(item);
        if sanitized.len() == MAX_PERSONA_LIST_ITEMS {
            break;
        }
    }
    Some(sanitized)
}

fn bounded_text(value: &str, max_chars: usize) -> String {
    value
        .trim()
        .chars()
        .filter(|character| !character.is_control())
        .take(max_chars)
        .collect()
}

#[cfg(test)]
#[path = "onboarding_ai_tests.rs"]
mod tests;

#[cfg(test)]
mod live {
    /// A write-up imported with the import prompt, on the site's everyday
    /// model (onboarding itself asks the Pro tier), printed with whether it
    /// passes the draft gates: MEROPE_IMPORT_SOURCE is a file holding it;
    /// never kept.
    #[tokio::test]
    #[ignore = "asks the site's model"]
    async fn a_write_up_imported() {
        let _db = crate::services::agent::semantic_eval::load_configured_lite().await;
        let source =
            std::fs::read_to_string(std::env::var("MEROPE_IMPORT_SOURCE").unwrap()).unwrap();
        let input = serde_json::json!({
            "pipeline": "onboarding/persona-import",
            "task": "import_character_persona",
            "name": "若泉 绮羽",
            "language": "zh-CN",
            "genderPresentation": "female",
            "source": source,
            "rollId": "i1",
        })
        .to_string();
        let model =
            super::super::call::Ask::new(super::super::call::Voice::HersAtLength, 1, "onboarding")
                .within(std::time::Duration::from_secs(120))
                .model()
                .await
                .unwrap();
        let raw = model
            .text(super::IMPORT_PERSONA_SYSTEM_PROMPT, &input)
            .await
            .unwrap();
        let parsed = super::parse_json_object(&raw).expect("JSON");
        println!("{}", serde_json::to_string_pretty(&parsed).unwrap());
        println!(
            "full: {}  language: {}  sludge or quoted lines: {}",
            super::persona_draft_meets_generation_quality(&parsed),
            super::persona_matches_ui_language(&parsed, "zh-CN"),
            myriad_merope::persona_has_literary_sludge(&parsed)
        );
    }
}
