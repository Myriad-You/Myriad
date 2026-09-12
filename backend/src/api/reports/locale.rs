//! Host UI locale for platform reports.
//!
//! Interactive generate follows `X-Myriad-Locale` / `Accept-Language`.
//! Missing headers (auto-regen / header-less generate-all) reuse the last
//! stored report locale, else the account locale, else en-US.

use axum::http::{header, HeaderMap};
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use serde_json::Value;

/// Header-less auto-regen / generate-all default (matches host UI fallback).
pub const DEFAULT_AUTO_REGEN_LOCALE: &str = "en-US";

/// Exact host UI tags stored on `users.locale`. Other values are treated as unset.
pub fn parse_stored_ui_locale(raw: &str) -> Option<&'static str> {
    match raw.trim() {
        "zh-CN" => Some("zh-CN"),
        "zh-TW" => Some("zh-TW"),
        "en-US" => Some("en-US"),
        "ja-JP" => Some("ja-JP"),
        "ko-KR" => Some("ko-KR"),
        "fr-FR" => Some("fr-FR"),
        "de-DE" => Some("de-DE"),
        _ => None,
    }
}

pub async fn locale_from_user(db: &impl ConnectionTrait, user_id: i32) -> Option<&'static str> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT locale FROM users WHERE id = $1",
            vec![user_id.into()],
        ))
        .await
        .ok()
        .flatten()?;
    row.try_get::<Option<String>>("", "locale")
        .ok()
        .flatten()
        .as_deref()
        .and_then(parse_stored_ui_locale)
}

fn map_language_tag(raw: &str) -> Option<&'static str> {
    let tag = raw.trim().replace('_', "-");
    if tag.is_empty() {
        return None;
    }
    if let Some(exact) = parse_stored_ui_locale(&tag) {
        return Some(exact);
    }
    let lower = tag.to_ascii_lowercase();
    if lower.starts_with("zh-tw")
        || lower.starts_with("zh-hk")
        || lower.starts_with("zh-mo")
        || lower.contains("hant")
    {
        Some("zh-TW")
    } else if lower.starts_with("zh") {
        Some("zh-CN")
    } else if lower.starts_with("ja") {
        Some("ja-JP")
    } else if lower.starts_with("ko") {
        Some("ko-KR")
    } else if lower.starts_with("fr") {
        Some("fr-FR")
    } else if lower.starts_with("de") {
        Some("de-DE")
    } else if lower.starts_with("en") {
        Some("en-US")
    } else {
        None
    }
}

fn pick_from_language_list(raw: &str) -> Option<&'static str> {
    let mut items: Vec<(f64, usize, &str)> = raw
        .split(',')
        .enumerate()
        .filter_map(|(index, part)| {
            let mut bits = part.split(';');
            let tag = bits.next()?.trim();
            if tag.is_empty() {
                return None;
            }
            let mut q = 1.0_f64;
            for param in bits {
                let param = param.trim();
                if let Some(value) = param
                    .strip_prefix("q=")
                    .or_else(|| param.strip_prefix("Q="))
                {
                    if let Ok(parsed) = value.parse::<f64>() {
                        q = parsed.clamp(0.0, 1.0);
                    }
                }
            }
            Some((q, index, tag))
        })
        .filter(|item| item.0 > 0.0)
        .collect();
    items.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.1.cmp(&b.1))
    });
    items
        .into_iter()
        .find_map(|(_, _, tag)| map_language_tag(tag))
}

/// Map a BCP 47 tag or Accept-Language list onto a host UI locale.
/// Unknown / empty → `None`. Cases live in `shared/host_locale_cases.json`.
pub fn parse_host_locale(raw: &str) -> Option<&'static str> {
    let tag = raw.trim();
    if tag.is_empty() {
        return None;
    }
    if let Some(exact) = parse_stored_ui_locale(tag) {
        return Some(exact);
    }
    pick_from_language_list(tag)
}

pub fn normalize_report_locale(raw: &str) -> &'static str {
    parse_host_locale(raw).unwrap_or(DEFAULT_AUTO_REGEN_LOCALE)
}

/// `None` when the request carries no locale signal — caller should reuse the
/// last stored report language instead of inventing en-US.
fn locale_from_cookie(headers: &HeaderMap) -> Option<&'static str> {
    let cookie = headers.get(header::COOKIE)?.to_str().ok()?;
    for part in cookie.split(';') {
        let part = part.trim();
        let Some(value) = part
            .strip_prefix("locale=")
            .or_else(|| part.strip_prefix("Locale="))
        else {
            continue;
        };
        let value = value.trim();
        if !value.is_empty() && value.len() <= 32 {
            return Some(normalize_report_locale(value));
        }
    }
    None
}

/// Request-scoped host UI locale. Missing signal → `en-US`.
pub fn host_locale_from_headers(headers: &HeaderMap) -> &'static str {
    locale_from_headers(headers).unwrap_or(DEFAULT_AUTO_REGEN_LOCALE)
}

pub fn locale_from_headers(headers: &HeaderMap) -> Option<&'static str> {
    if let Some(v) = headers
        .get("x-myriad-locale")
        .and_then(|h| h.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty() && s.len() <= 32)
    {
        return Some(normalize_report_locale(v));
    }
    if let Some(from_cookie) = locale_from_cookie(headers) {
        return Some(from_cookie);
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

#[allow(dead_code)]
pub fn pick<'a>(locale: &str, zh: &'a str, ja: &'a str, en: &'a str) -> &'a str {
    match normalize_report_locale(locale) {
        "ja-JP" => ja,
        "en-US" => en,
        "zh-CN" | "zh-TW" => zh,
        _ => en,
    }
}

pub fn missing_platform_data_message(locale: &str) -> String {
    crate::i18n::reports(locale, "missingPlatformData")
}

pub fn generate_none_message(locale: &str) -> String {
    crate::i18n::reports(locale, "generateNone")
}

/// Instruction block: keep field names / enum keys; write user-visible copy in `locale`.
pub fn language_rule(locale: &str) -> String {
    let loc = normalize_report_locale(locale);
    format!(
        "Write every user-visible string in {loc}: summary, insights, vibe, taste_profile, mood_keywords.tag, \
danmaku, channel_type, gamer_type, hunter_type, role_profile, engagement_level, \
signature_topics, interest_circles.name, guild_takes.take. \
Keep field names and enum keys (hardcore|casual|balanced) unchanged. Keep work titles, account names, and guild names as they appear in Data. \
Do not mix other scripts into these strings."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn shared_host_locale_cases() {
        let spec: Value =
            serde_json::from_str(include_str!("../../../../shared/host_locale_cases.json"))
                .expect("shared/host_locale_cases.json");
        for case in spec["parse"].as_array().expect("parse") {
            let input = case["input"].as_str().expect("input");
            let expected = case["output"].as_str();
            assert_eq!(
                parse_host_locale(input),
                expected,
                "parse_host_locale({input:?})"
            );
            assert_eq!(
                normalize_report_locale(input),
                expected.unwrap_or(DEFAULT_AUTO_REGEN_LOCALE),
                "normalize_report_locale({input:?})"
            );
        }
    }

    #[test]
    fn stored_ui_locale_is_exact() {
        assert_eq!(parse_stored_ui_locale("ja-JP"), Some("ja-JP"));
        assert_eq!(parse_stored_ui_locale(" zh-CN "), Some("zh-CN"));
        assert_eq!(parse_stored_ui_locale("zh-TW"), Some("zh-TW"));
        assert_eq!(parse_stored_ui_locale("ko-KR"), Some("ko-KR"));
        assert_eq!(parse_stored_ui_locale("fr-FR"), Some("fr-FR"));
        assert_eq!(parse_stored_ui_locale("de-DE"), Some("de-DE"));
        assert_eq!(parse_stored_ui_locale("zh"), None);
        assert_eq!(parse_stored_ui_locale(""), None);
    }

    #[test]
    fn header_prefers_x_myriad_locale() {
        let mut headers = HeaderMap::new();
        headers.insert("accept-language", HeaderValue::from_static("en-US"));
        headers.insert("x-myriad-locale", HeaderValue::from_static("ja-JP"));
        assert_eq!(locale_from_headers(&headers), Some("ja-JP"));
    }

    #[test]
    fn header_prefers_locale_cookie_over_accept_language() {
        let mut headers = HeaderMap::new();
        headers.insert("accept-language", HeaderValue::from_static("en-US"));
        headers.insert(
            "cookie",
            HeaderValue::from_static("theme=dark; locale=zh-TW"),
        );
        assert_eq!(locale_from_headers(&headers), Some("zh-TW"));
    }

    #[test]
    fn headerless_request_has_no_locale_signal() {
        assert_eq!(locale_from_headers(&HeaderMap::new()), None);
        assert_eq!(host_locale_from_headers(&HeaderMap::new()), "en-US");
    }

    #[test]
    fn host_locale_normalizes_traditional_tags() {
        let mut headers = HeaderMap::new();
        headers.insert("x-myriad-locale", HeaderValue::from_static("zh-HK"));
        assert_eq!(host_locale_from_headers(&headers), "zh-TW");
        headers.insert(
            "accept-language",
            HeaderValue::from_static("it,zh-HK;q=0.8"),
        );
        headers.remove("x-myriad-locale");
        assert_eq!(host_locale_from_headers(&headers), "zh-TW");
    }

    #[test]
    fn missing_data_message_follows_locale() {
        assert!(missing_platform_data_message("zh-CN").contains("可用数据"));
        assert!(missing_platform_data_message("zh-TW").contains("可用資料"));
        assert!(missing_platform_data_message("en-US").contains("usable data"));
        assert!(missing_platform_data_message("ja-JP").contains("データ"));
        assert!(missing_platform_data_message("ko-KR").contains("데이터"));
        assert!(missing_platform_data_message("fr-FR").contains("données"));
        assert!(missing_platform_data_message("de-DE").contains("Daten"));
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
