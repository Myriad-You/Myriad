//! Pro onboarding helpers: name roll + structured persona draft.
//! Prompts live in `onboarding_prompts`. Visual identity / outfits stay out.

use serde_json::{json, Map, Value};
use std::time::Duration;

use crate::config::ModelTier;
use crate::services::ai::create_ai_analyzer_for_tier_with_timeout;

use super::onboarding_prompts::{NAME_SYSTEM_PROMPT, PERSONA_SYSTEM_PROMPT};
use super::report_dna::sanitize_onboarding_tags_for_language;

const ONBOARDING_AI_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_PERSONA_LIST_ITEMS: usize = 12;
const MAX_PERSONA_LIST_ITEM_CHARS: usize = 180;
const MAX_PERSONA_GUIDANCE_CHARS: usize = 800;
const MIN_SUMMARY_CHARS: usize = 80;
const MIN_TEMPERAMENT_ITEMS: usize = 5;
const MIN_PAIR_ITEMS: usize = 4;
const MIN_GUIDANCE_CHARS: usize = 36;

#[derive(Debug)]
pub enum OnboardingAiError {
    AnalyzerUnavailable,
    ProviderFailed,
    UnusableResponse,
}

pub async fn suggest_display_name(
    selected_tags: &[String],
    gender: &str,
    avoid_name: Option<&str>,
    language: &str,
) -> Result<String, OnboardingAiError> {
    let seeds = sanitize_onboarding_tags_for_language(selected_tags, language);
    let avoid = avoid_name
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.chars().take(40).collect::<String>());
    let input = json!({
        "pipeline": "onboarding/name",
        "task": "recommend_display_name",
        "rollId": format!("n{}", uuid::Uuid::new_v4().simple()),
        "language": language,
        "genderPresentation": normalize_gender(gender),
        "selectedTags": seeds,
        "avoidName": avoid.clone().unwrap_or_default(),
        "nameSchool": match language {
            "zh-CN" => "liyue-xianzhou-meaning-first",
            "ja-JP" => "inazuma-meaning-first",
            _ => "word-name-etymology-first",
        },
    })
    .to_string();
    let raw = run_onboarding_call(NAME_SYSTEM_PROMPT, &input).await?;
    let name = parse_display_name_suggestion(&raw, avoid.as_deref(), language)
        .ok_or(OnboardingAiError::UnusableResponse)?;
    Ok(name)
}

pub async fn suggest_persona(
    name: &str,
    language: &str,
    selected_tags: &[String],
    gender: &str,
    extra_requirements: &str,
) -> Result<Value, OnboardingAiError> {
    let tags = sanitize_onboarding_tags_for_language(selected_tags, language);
    let fallback = fallback_persona_draft(name, language, &tags);
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
            "summaryChars": "80-280",
            "temperamentCount": "5-8",
            "likesCount": "4-6",
            "drivesCount": "4-6",
            "guidance": "2-4 sentences each for socialStyle and speechStyle",
        },
    })
    .to_string();
    let raw = run_onboarding_call(PERSONA_SYSTEM_PROMPT, &input).await?;
    let parsed = parse_json_object(&raw).ok_or(OnboardingAiError::UnusableResponse)?;
    if !persona_draft_is_complete(&parsed) || !persona_matches_ui_language(&parsed, language) {
        return Err(OnboardingAiError::UnusableResponse);
    }
    let persona = sanitize_persona_draft(&parsed, &fallback)
        .ok_or(OnboardingAiError::UnusableResponse)?;
    Ok(persona)
}

async fn run_onboarding_call(
    system: &str,
    input: &str,
) -> Result<String, OnboardingAiError> {
    {
        let config = crate::GLOBAL_DYNAMIC_CONFIG.read().await;
        if !config.pro_enabled {
            return Err(OnboardingAiError::AnalyzerUnavailable);
        }
    }
    let Some(analyzer) =
        create_ai_analyzer_for_tier_with_timeout(ModelTier::Pro, Some(ONBOARDING_AI_TIMEOUT)).await
    else {
        return Err(OnboardingAiError::AnalyzerUnavailable);
    };
    match analyzer.analyze_with_system(system, input).await {
        Ok(raw) if !raw.trim().is_empty() => Ok(raw),
        _ => Err(OnboardingAiError::ProviderFailed),
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

fn parse_display_name_suggestion(raw: &str, avoid: Option<&str>, language: &str) -> Option<String> {
    let parsed = parse_json_object(raw)?;
    let candidates = if let Some(name) = parsed.get("name").and_then(Value::as_str) {
        vec![name.to_string()]
    } else if let Some(arr) = parsed.get("names").and_then(Value::as_array) {
        arr.iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect()
    } else {
        return None;
    };
    let avoid_norm = avoid.map(str::trim).filter(|v| !v.is_empty());
    for candidate in candidates {
        let cleaned = sanitize_display_name_candidate(&candidate, language);
        if cleaned.is_empty() {
            continue;
        }
        if avoid_norm.is_some_and(|avoid| cleaned == avoid) {
            continue;
        }
        return Some(cleaned);
    }
    None
}

fn sanitize_display_name_candidate(raw: &str, language: &str) -> String {
    let trimmed = raw
        .chars()
        .filter(|ch| !ch.is_control())
        .collect::<String>();
    let trimmed = trimmed.trim().trim_matches(|ch: char| {
        matches!(
            ch,
            '"' | '\'' | '「' | '」' | '『' | '』' | '《' | '》' | ' '
        )
    });
    let collapsed = trimmed.split_whitespace().collect::<Vec<_>>().join("");
    match language {
        "en-US" => sanitize_latin_display_name(&collapsed),
        "ja-JP" => sanitize_cjk_display_name(&collapsed, true),
        _ => sanitize_cjk_display_name(&collapsed, false),
    }
}

fn sanitize_latin_display_name(value: &str) -> String {
    if !value.chars().all(|ch| ch.is_ascii_alphabetic()) {
        return String::new();
    }
    let count = value.chars().count();
    if !(2..=12).contains(&count) {
        return String::new();
    }
    if value.chars().filter(|ch| ch.is_ascii_uppercase()).count() > 1 {
        return String::new();
    }
    if is_blocked_en_display_name(value) {
        return String::new();
    }
    value.to_string()
}

fn is_blocked_en_display_name(value: &str) -> bool {
    const BLOCKED: &[&str] = &[
        "robin", "sunday", "firefly", "sparkle", "stelle", "caelus", "jean", "diluc",
        "amber", "lisa", "maris", "cael", "liora",
    ];
    let lower = value.to_ascii_lowercase();
    BLOCKED.iter().any(|blocked| lower == *blocked)
}

fn is_blocked_ja_display_name(value: &str) -> bool {
    const BLOCKED: &[&str] = &["綾華", "绫华", "万葉", "万叶", "宵宮", "宵宫", "早柚", "神子", "雷電", "雷电"];
    BLOCKED.iter().any(|blocked| value == *blocked || value.contains(blocked))
}

fn is_blocked_zh_display_name(value: &str) -> bool {
    const BLOCKED: &[&str] = &[
        "甘雨", "刻晴", "钟离", "行秋", "重云", "香菱", "凝光", "北斗", "辛焱",
        "云堇", "夜兰", "申鹤", "胡桃", "七七", "瑶瑶", "白术", "闲云", "魈",
        "景元", "丹恒", "符玄", "镜流", "彦卿", "素裳", "青雀", "停云", "驭空",
        "罗刹", "三月七", "花火", "黄泉", "流萤", "知更鸟", "藿藿", "寒鸦",
        "雪衣", "银狼", "姬子", "澄羽", "岚音", "星语", "月璃", "玄霄", "墨染",
        "夜雪", "凌霄",
    ];
    BLOCKED
        .iter()
        .any(|blocked| value == *blocked || (blocked.chars().count() >= 2 && value.contains(blocked)))
}

fn is_cjk_han(ch: char) -> bool {
    matches!(
        ch,
        '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' | '\u{F900}'..='\u{FAFF}'
    )
}

fn is_hiragana(ch: char) -> bool {
    matches!(ch, '\u{3041}'..='\u{3096}')
}

fn is_katakana_letter(ch: char) -> bool {
    matches!(ch, '\u{30A1}'..='\u{30FA}' | '\u{30FC}')
}

fn sanitize_cjk_display_name(value: &str, japanese: bool) -> String {
    let count = value.chars().count();
    if count < 2 {
        return String::new();
    }
    let script_ok = value.chars().all(|ch| {
        if japanese {
            is_cjk_han(ch) || is_hiragana(ch) || is_katakana_letter(ch)
        } else {
            is_cjk_han(ch)
        }
    });
    if !script_ok {
        return String::new();
    }
    if !japanese && (value.starts_with('阿') || value.starts_with('小')) {
        return String::new();
    }
    if value.contains("小姐") || value.contains("大人") {
        return String::new();
    }
    if !japanese && is_blocked_zh_display_name(value) {
        return String::new();
    }
    if japanese
        && (value.ends_with("ちゃん")
            || value.ends_with("くん")
            || value.ends_with("さん")
            || value.ends_with('様')
            || is_blocked_ja_display_name(value))
    {
        return String::new();
    }
    value.chars().take(6).collect()
}

fn fallback_persona_draft(name: &str, language: &str, tags: &[String]) -> Value {
    let tags = sanitize_onboarding_tags_for_language(tags, language);
    json!({
        "summary": fallback_summary(name, language, &tags),
        "temperament": tags,
        "likes": [],
        "drives": [],
        "socialStyle": "",
        "speechStyle": "",
    })
}

fn sanitize_persona_draft(value: &Value, fallback: &Value) -> Option<Value> {
    let source = value.get("persona").unwrap_or(value).as_object()?;
    let mut result = fallback.as_object()?.clone();
    replace_text(&mut result, source, "summary", &["summary"], 1_200);
    replace_list(
        &mut result,
        source,
        "temperament",
        &["temperament", "traits"],
    );
    replace_list(&mut result, source, "likes", &["likes"]);
    replace_list(&mut result, source, "drives", &["drives", "motivations"]);
    replace_text(
        &mut result,
        source,
        "socialStyle",
        &["socialStyle", "social_style"],
        MAX_PERSONA_GUIDANCE_CHARS,
    );
    replace_text(
        &mut result,
        source,
        "speechStyle",
        &["speechStyle", "speech_style", "voice"],
        MAX_PERSONA_GUIDANCE_CHARS,
    );
    Some(Value::Object(result))
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

fn persona_draft_is_complete(value: &Value) -> bool {
    let Some(source) = value.get("persona").unwrap_or(value).as_object() else {
        return false;
    };
    let summary_ready = source
        .get("summary")
        .and_then(Value::as_str)
        .is_some_and(|summary| summary.trim().chars().count() >= MIN_SUMMARY_CHARS);
    let temperament_ready = list_from(source, &["temperament", "traits"])
        .is_some_and(|items| items.len() >= MIN_TEMPERAMENT_ITEMS);
    let likes_ready = list_from(source, &["likes"]).is_some_and(|items| items.len() >= MIN_PAIR_ITEMS);
    let drives_ready =
        list_from(source, &["drives", "motivations"]).is_some_and(|items| items.len() >= MIN_PAIR_ITEMS);
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

fn flatten_persona_text(persona: &Value) -> String {
    let source = persona.get("persona").unwrap_or(persona);
    let mut lines = Vec::new();
    if let Some(items) = list_from_value(source, &["temperament", "traits"]) {
        if !items.is_empty() {
            lines.push(format!("气质：{}", items.join("、")));
        }
    }
    if let Some(items) = list_from_value(source, &["likes"]) {
        if !items.is_empty() {
            lines.push(format!("喜好：{}", items.join("、")));
        }
    }
    if let Some(items) = list_from_value(source, &["drives"]) {
        if !items.is_empty() {
            lines.push(format!("驱动力：{}", items.join("、")));
        }
    }
    if let Some(text) = source
        .get("socialStyle")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("社交：{text}"));
    }
    if let Some(text) = source
        .get("speechStyle")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("表达：{text}"));
    }
    if let Some(summary) = source
        .get("summary")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.push(summary.to_string());
    }
    lines.join("\n")
}

fn fallback_summary(name: &str, language: &str, tags: &[String]) -> String {
    let name = bounded_text(name, 50);
    let display = if name.is_empty() { "Arael" } else { name.as_str() };
    let traits = tags.iter().take(5).cloned().collect::<Vec<_>>();
    if traits.is_empty() {
        return match language {
            "ja-JP" => format!("{display}は、これから個性を育てていく生命です。"),
            "en-US" => format!("{display} is a life whose personality will grow through shared experiences."),
            _ => format!("{display}是一个会在相处中逐渐形成独特个性的生命。"),
        };
    }
    match language {
        "ja-JP" => format!("{display}は、{}という気質を持つ生命です。", traits.join("、")),
        "en-US" => format!("{display} is a life with a {} temperament.", traits.join(", ")),
        _ => format!("{display}是一个带有{}气质的生命。", traits.join("、")),
    }
}

fn replace_text(
    target: &mut Map<String, Value>,
    source: &Map<String, Value>,
    target_key: &str,
    source_keys: &[&str],
    max_chars: usize,
) {
    if let Some(value) = source_keys
        .iter()
        .find_map(|key| source.get(*key).and_then(Value::as_str))
        .map(|value| bounded_text(value, max_chars))
        .filter(|value| !value.is_empty())
    {
        target.insert(target_key.to_string(), json!(value));
    }
}

fn replace_list(
    target: &mut Map<String, Value>,
    source: &Map<String, Value>,
    target_key: &str,
    source_keys: &[&str],
) {
    if let Some(values) = list_from(source, source_keys).filter(|values| !values.is_empty()) {
        target.insert(target_key.to_string(), json!(values));
    }
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
        Value::String(value) => value
            .split(['、', '，', ';', '/', '|'])
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
mod tests {
    use super::*;

    #[test]
    fn sanitize_display_name_follows_ui_language() {
        assert!(sanitize_display_name_candidate("Alice", "zh-CN").is_empty());
        assert!(sanitize_display_name_candidate("阿强", "zh-CN").is_empty());
        assert!(sanitize_display_name_candidate("小美", "zh-CN").is_empty());
        assert_eq!(sanitize_display_name_candidate("晚衡", "zh-CN"), "晚衡");
        assert_eq!(sanitize_display_name_candidate("「听白」", "zh-CN"), "听白");
        assert!(sanitize_display_name_candidate("澄羽", "zh-CN").is_empty());
        assert!(sanitize_display_name_candidate("甘雨", "zh-CN").is_empty());
        assert!(sanitize_display_name_candidate("景元", "zh-CN").is_empty());
        assert_eq!(sanitize_display_name_candidate("Alice", "en-US"), "Alice");
        assert!(sanitize_display_name_candidate("NightOwl", "en-US").is_empty());
        assert!(sanitize_display_name_candidate("Robin", "en-US").is_empty());
        assert!(sanitize_display_name_candidate("澄羽", "en-US").is_empty());
        assert_eq!(sanitize_display_name_candidate("あおい", "ja-JP"), "あおい");
        assert_eq!(sanitize_display_name_candidate("雪見", "ja-JP"), "雪見");
        assert!(sanitize_display_name_candidate("宵宮", "ja-JP").is_empty());
        assert!(sanitize_display_name_candidate("Alice", "ja-JP").is_empty());
        assert!(sanitize_display_name_candidate("澄羽A", "zh-CN").is_empty());
        assert!(sanitize_display_name_candidate("澄羽Ａ", "zh-CN").is_empty());
        assert!(sanitize_display_name_candidate("澄羽・", "zh-CN").is_empty());
        assert!(sanitize_display_name_candidate("Aoi雪", "ja-JP").is_empty());
        assert_eq!(sanitize_display_name_candidate("ハナ", "ja-JP"), "ハナ");
        assert_eq!(sanitize_display_name_candidate("サリー", "ja-JP"), "サリー");
        assert!(sanitize_display_name_candidate("葵ちゃん", "ja-JP").is_empty());
    }

    #[test]
    fn persona_parser_requires_real_character_content() {
        let parsed = parse_json_object(
            r#"{"persona":{"summary":"安静但会认真回应对自己重要的事情。想靠近，又把话说得很短。认定谁值得之后，锋会收起来，把事情一件件安排妥。不爱解释自己为什么忽然变软，也不肯把私人节奏交给别人来定。","temperament":["克制","细心","慢热","嘴硬心软","边界感强"],"likes":["夜里听雨","把桌面重新排好","长时间安静地做事","把一件小事做到位"],"drives":["理解彼此","守住边界","把节奏握在自己手里","对认定的人认真"],"socialStyle":"先听，不抢着说话。熟了之后才会把句子拉长，把真正在意的人留在自己定的距离里。","speechStyle":"话少，用词干净。对在意的人会把锋收起来，把事说清楚，也不用漂亮句子掩饰不耐烦。"}}"#,
        )
        .unwrap();
        assert!(persona_draft_is_complete(&parsed));
        assert!(persona_matches_ui_language(&parsed, "zh-CN"));
        assert!(!persona_matches_ui_language(&parsed, "en-US"));
        assert!(!persona_draft_is_complete(&json!({
            "persona": { "summary": "只有一句，没有性格数组" }
        })));
        assert!(!persona_draft_is_complete(&json!({
            "persona": {
                "summary": "安静但会认真回应对自己重要的事情。",
                "temperament": ["克制"]
            }
        })));
        assert!(!persona_draft_is_complete(&json!({
            "persona": {
                "summary": "安静但会认真回应对自己重要的事情。想靠近，又把门留一条缝。",
                "temperament": ["克制","细心","慢热","嘴硬心软","边界感强"],
                "likes": ["夜里听雨","把桌面重新排好","长时间安静地做事","把一件小事做到位"],
                "drives": ["理解彼此","守住边界","把节奏握在自己手里","对认定的人认真"],
                "socialStyle": "先听，不抢着说话。熟了之后才会把句子拉长，把真正在意的人留在自己定的距离里。",
                "speechStyle": "话少，用词干净。对在意的人会把锋收起来，把事说清楚，也不用漂亮句子掩饰不耐烦。"
            }
        })));
        let english = json!({
            "persona": {
                "summary": "Quiet, but answers the things that matter.",
                "temperament": ["restrained", "careful"],
                "likes": ["rain"],
                "drives": ["understand people"],
                "socialStyle": "does not interrupt",
                "speechStyle": "brief and warm"
            }
        });
        assert!(persona_matches_ui_language(&english, "en-US"));
        assert!(!persona_matches_ui_language(&english, "ja-JP"));
    }

    #[test]
    fn flatten_keeps_character_fields_and_drops_looks() {
        let text = flatten_persona_text(&json!({
            "summary": "话少，认真。",
            "temperament": ["克制"],
            "likes": ["雨声"],
            "visualIdentity": { "hairShape": "短发" }
        }));
        assert!(text.contains("气质：克制"));
        assert!(text.contains("话少，认真。"));
        assert!(!text.contains("短发"));
    }
}
