/// Step-1 bubble pool + user selection cap (aligned with experimental 12–20 AI pool / ≤28 picks).
pub const MAX_ONBOARDING_TAGS: usize = 28;
pub const MAX_ONBOARDING_TAG_CHARS: usize = 24;

pub fn sanitize_onboarding_tags(tags: &[String]) -> Vec<String> {
    let mut sanitized = Vec::new();
    for tag in tags {
        let candidate = tag.trim();
        if candidate.is_empty()
            || candidate.chars().count() > MAX_ONBOARDING_TAG_CHARS
            || candidate.chars().any(char::is_control)
            || sanitized
                .iter()
                .any(|existing: &String| existing.eq_ignore_ascii_case(candidate))
        {
            continue;
        }
        sanitized.push(candidate.to_string());
        if sanitized.len() == MAX_ONBOARDING_TAGS {
            break;
        }
    }
    sanitized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_and_bounds_multilingual_onboarding_tags() {
        let tags = vec![
            " gentle ".to_string(),
            "GENTLE".to_string(),
            "好奇".to_string(),
            "bad\nvalue".to_string(),
            "x".repeat(MAX_ONBOARDING_TAG_CHARS + 1),
            "playful".to_string(),
        ];
        assert_eq!(
            sanitize_onboarding_tags(&tags),
            vec!["gentle", "好奇", "playful"]
        );
    }
}
