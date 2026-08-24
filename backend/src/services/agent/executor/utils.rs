//! Thin adapter re-exporting pure executor helpers from
//! [`crate::services::agent::executor_utils_pure`].
//!
//! Existing call sites keep `executor::utils::*` imports.

// Re-export full pure surface for existing call sites (not all symbols used here).
#[allow(unused_imports)]
pub use crate::services::agent::executor_utils_pure::{
    best_loose_match, brew_category_token_matches, extract_image_url, is_valid_platform,
    levenshtein_similar, loose_text_match, normalize_brew_category_filter,
    normalize_brew_source_type_filter, summarize_output, truncate_str, validate_platform_name,
    MatchKind, VALID_PLATFORMS,
};

#[cfg(test)]
mod adapter_tests {
    use super::*;

    #[test]
    fn adapter_reexports_shipped_pure_helpers() {
        assert!(VALID_PLATFORMS.contains(&"steam"));
        assert_eq!(truncate_str("hello world", 5), "hello");
        assert_eq!(normalize_brew_category_filter("friends"), "友情链接");
        assert_eq!(loose_text_match("akiday", "akiday"), Some(MatchKind::Exact));
        let img = extract_image_url(&serde_json::json!({"imageUrl": "https://x/a.png"}));
        assert_eq!(img.as_deref(), Some("https://x/a.png"));
        assert_eq!(
            extract_image_url(&serde_json::json!({
                "imageUrl": "/api/brew/image-cache/ab/abcd.png"
            }))
            .as_deref(),
            Some("/api/brew/image-cache/ab/abcd.png")
        );
        assert!(
            extract_image_url(&serde_json::json!({"imageUrl": "data:image/png;base64,xx"}))
                .is_none()
        );
    }
}
