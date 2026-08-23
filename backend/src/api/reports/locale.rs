//! Host UI locale for platform reports.
//!
//! Interactive generate follows `X-Myriad-Locale` / `Accept-Language`.
//! Missing headers (auto-regen / header-less generate-all) reuse the last
//! stored report locale, else zh-CN.

use axum::http::{header, HeaderMap};
use serde_json::Value;

/// Historical reports and header-less auto-regen.
pub const DEFAULT_AUTO_REGEN_LOCALE: &str = "zh-CN";

pub fn normalize_report_locale(raw: &str) -> &'static str {
    let tag = raw
        .split(',')
        .next()
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim();
    let lower = tag.to_ascii_lowercase();
    if lower.starts_with("zh") {
        "zh-CN"
    } else if lower.starts_with("ja") {
        "ja-JP"
    } else if lower.starts_with("en") {
        "en-US"
    } else if tag.is_empty() {
        DEFAULT_AUTO_REGEN_LOCALE
    } else {
        "en-US"
    }
}

/// `None` when the request carries no locale signal — caller should reuse the
/// last stored report language instead of inventing en-US.
pub fn locale_from_headers(headers: &HeaderMap) -> Option<&'static str> {
    if let Some(v) = headers
        .get("x-myriad-locale")
        .and_then(|h| h.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty() && s.len() <= 32)
    {
        return Some(normalize_report_locale(v));
    }
    if let Some(al) = headers
        .get(header::ACCEPT_LANGUAGE)
        .and_then(|h| h.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return Some(normalize_report_locale(al));
    }
    None
}

/// Same unwrap as public report reads: string JSON and `{ report: {…} }` envelopes.
pub fn unwrap_stored_report_json(report: Value) -> Value {
    let report = match report {
        Value::String(s) => serde_json::from_str(&s).unwrap_or(Value::String(s)),
        other => other,
    };
    match &report {
        Value::Object(map)
            if map.contains_key("report")
                && !map.contains_key("card_visuals")
                && map.get("report").map(|v| v.is_object()).unwrap_or(false) =>
        {
            map.get("report").cloned().unwrap_or(report)
        }
        _ => report,
    }
}

pub fn locale_from_stored_report(report: &Value) -> Option<&'static str> {
    match report {
        Value::String(s) => {
            let parsed = serde_json::from_str(s).ok()?;
            locale_from_stored_report(&parsed)
        }
        Value::Object(map)
            if map.contains_key("report")
                && !map.contains_key("card_visuals")
                && map.get("report").is_some_and(|v| v.is_object()) =>
        {
            read_locale_field(map.get("report")?)
        }
        _ => read_locale_field(report),
    }
}

fn read_locale_field(report: &Value) -> Option<&'static str> {
    report
        .get("locale")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(normalize_report_locale)
}

pub fn pick<'a>(locale: &str, zh: &'a str, ja: &'a str, en: &'a str) -> &'a str {
    match normalize_report_locale(locale) {
        "ja-JP" => ja,
        "en-US" => en,
        _ => zh,
    }
}

pub fn missing_platform_data_message(locale: &str) -> String {
    pick(
        locale,
        "该平台还没有可用数据。请先成功抓取后再生成报告。",
        "このプラットフォームのデータがまだありません。先に取得してからレポートを生成してください。",
        "This platform has no usable data yet. Fetch it first, then generate the report.",
    )
    .to_string()
}

pub fn generate_none_message(locale: &str) -> String {
    pick(
        locale,
        "未能生成报告。请确保已获取平台数据。",
        "レポートを生成できませんでした。先にプラットフォームデータを取得してください。",
        "Could not generate a report. Fetch the platform data first.",
    )
    .to_string()
}

/// Instruction block: keep field names / enum keys; write user-visible copy in `locale`.
pub fn language_rule(locale: &str) -> String {
    let loc = normalize_report_locale(locale);
    format!(
        "用户可见文案必须用 {loc} 写完：summary、insights、vibe、taste_profile、mood_keywords.tag、\
danmaku、channel_type、gamer_type、hunter_type、role_profile、engagement_level、\
signature_topics、interest_circles.name、guild_takes.take。\
字段名和枚举键（hardcore|casual|balanced）保持原样。作品名、账号名、服名按 Data 原文。\
这些字符串里不要夹杂其他文种。"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn normalizes_host_tags() {
        assert_eq!(normalize_report_locale("zh-CN"), "zh-CN");
        assert_eq!(normalize_report_locale("zh"), "zh-CN");
        assert_eq!(normalize_report_locale("ja-JP,ja;q=0.9"), "ja-JP");
        assert_eq!(normalize_report_locale("en-GB"), "en-US");
        assert_eq!(normalize_report_locale(""), "zh-CN");
        assert_eq!(normalize_report_locale("fr-FR"), "en-US");
    }

    #[test]
    fn header_prefers_x_myriad_locale() {
        let mut headers = HeaderMap::new();
        headers.insert("accept-language", HeaderValue::from_static("en-US"));
        headers.insert("x-myriad-locale", HeaderValue::from_static("ja-JP"));
        assert_eq!(locale_from_headers(&headers), Some("ja-JP"));
    }

    #[test]
    fn headerless_request_has_no_locale_signal() {
        assert_eq!(locale_from_headers(&HeaderMap::new()), None);
    }

    #[test]
    fn missing_data_message_follows_locale() {
        assert!(missing_platform_data_message("zh-CN").contains("可用数据"));
        assert!(missing_platform_data_message("en-US").contains("usable data"));
        assert!(missing_platform_data_message("ja-JP").contains("データ"));
        assert!(!missing_platform_data_message("en-US").contains("cache/raw"));
    }

    #[test]
    fn stored_report_locale_normalizes() {
        let persisted = serde_json::json!({
            "platform": "steam",
            "summary": "x",
            "insights": [],
            "card_visuals": {},
            "created_at": "t",
            "locale": "en-US"
        });
        assert_eq!(locale_from_stored_report(&persisted), Some("en-US"));
        assert_eq!(
            locale_from_stored_report(&serde_json::json!({ "summary": "旧报告" })),
            None
        );
        let encoded = serde_json::to_string(&persisted).unwrap();
        assert_eq!(
            locale_from_stored_report(&Value::String(encoded)),
            Some("en-US")
        );
        let envelope = serde_json::json!({
            "report": {
                "summary": "x",
                "card_visuals": {},
                "locale": "ja-JP"
            }
        });
        assert_eq!(locale_from_stored_report(&envelope), Some("ja-JP"));
        assert_eq!(
            unwrap_stored_report_json(envelope.clone())["locale"],
            "ja-JP"
        );
    }
}
