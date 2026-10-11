//! What she keeps of a line, the rules of it: the compact form an event or
//! a chat line is written down in (secrets and JSON never kept), and whether
//! a chat line is worth a diary entry.

const CHAT_DIARY_MIN_CHARS: usize = 8;
const CHAT_DIARY_GAP_MINUTES: i64 = 20;

/// Whether a chat line goes in her diary: long enough once compacted, and
/// the last chat entry `last_chat_age_minutes` ago, if any, not too recent.
pub fn should_write_chat_diary(text: &str, last_chat_age_minutes: Option<i64>) -> bool {
    let summary = compact_summary(text);
    if summary.chars().count() < CHAT_DIARY_MIN_CHARS {
        return false;
    }
    last_chat_age_minutes.is_none_or(|age| age >= CHAT_DIARY_GAP_MINUTES)
}

pub fn compact_summary(summary: &str) -> String {
    redact_event_text(summary)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(240)
        .collect()
}

pub fn redact_event_text(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return String::new();
    }
    let mut skip_next = false;
    let mut out = Vec::new();
    for token in trimmed.split_whitespace() {
        if skip_next {
            skip_next = false;
            continue;
        }
        let lower = token.to_ascii_lowercase();
        if lower == "bearer" || lower.starts_with("bearer") {
            skip_next = lower == "bearer";
            continue;
        }
        if let Some(cleaned) = redact_token(token) {
            out.push(cleaned);
        }
    }
    out.join(" ")
}

fn redact_token(token: &str) -> Option<String> {
    let stripped = if let Some(scheme) = token.find("://") {
        let after_scheme = &token[scheme + 3..];
        if let Some(query) = after_scheme.find('?') {
            token[..scheme + 3 + query].to_string()
        } else {
            token.to_string()
        }
    } else {
        token.to_string()
    };
    let lower = stripped.to_ascii_lowercase();
    if lower.contains("api_key=")
        || lower.contains("access_token=")
        || lower.contains("refresh_token=")
        || lower.contains("secret=")
        || lower.contains("password=")
        || lower.starts_with("bearer")
    {
        return None;
    }
    Some(stripped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_summary_drops_json_and_secret_shaped_tokens() {
        assert!(compact_summary("{\"token\":\"abc\"}").is_empty());
        assert_eq!(
            compact_summary("Steam 刷新失败 https://api.example/sync?access_token=abcd"),
            "Steam 刷新失败 https://api.example/sync"
        );
        assert_eq!(compact_summary("抓取失败 Bearer eyJhbGciOi"), "抓取失败");
        assert_eq!(compact_summary("  Steam  解锁了成就  "), "Steam 解锁了成就");
    }

    #[test]
    fn a_chat_line_goes_in_the_diary_when_long_enough_and_not_too_soon() {
        assert!(!should_write_chat_diary("嗯", None));
        assert!(should_write_chat_diary("今天晚上想打会独立游戏", None));
        assert!(!should_write_chat_diary("今天晚上想打会独立游戏", Some(5)));
        assert!(should_write_chat_diary("今天晚上想打会独立游戏", Some(20)));
        // Counted after compacting: a secret alone is nothing to keep.
        assert!(!should_write_chat_diary(
            "Bearer eyJhbGciOiJIUzI1NiJ9",
            None
        ));
    }
}
