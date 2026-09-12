//! Pure error classification and param-fix projection for agent retries.
//!
//! Call sites (executor retry loop / DAG path) keep sleep, SSE, breaker, and
//! step re-execution. Domain owns:
//! - error category from message + capability + params
//! - whether the failure is retryable
//! - param fix suggestions and apply-fix projection
//! - delay multiplier and optional prepend-capability hints
//!
//! No Axum, sqlx, reqwest, or network I/O.

use serde_json::Value;
use std::collections::HashMap;

pub use myriad_agent_rules::{
    analyze_error, apply_param_fixes, ErrorAnalysis, ErrorCategory, ParamFix,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_content_policy_detection() {
        let params: HashMap<String, Value> = [(
            "prompt".into(),
            Value::String("test prompt with sex content".into()),
        )]
        .into_iter()
        .collect();
        let analysis = analyze_error(
            "Prompt contains disallowed content: sex",
            "ai.image",
            &params,
        );
        assert_eq!(analysis.category, ErrorCategory::ContentPolicy);
        assert!(analysis.retryable);
        assert!(!analysis.param_fixes.is_empty());
    }

    #[test]
    fn test_missing_param_detection() {
        let params = HashMap::new();
        let analysis = analyze_error(
            "Missing playlistId parameter. Use netease.searchPlaylist first.",
            "music.playlist",
            &params,
        );
        assert_eq!(analysis.category, ErrorCategory::MissingParameter);
        assert!(analysis.retryable);
        assert_eq!(
            analysis.suggested_prepend_capability.as_deref(),
            Some("netease.searchPlaylist")
        );
    }

    #[test]
    fn test_rate_limit_detection() {
        let params = HashMap::new();
        let analysis = analyze_error("429 Too Many Requests", "ai.image", &params);
        assert_eq!(analysis.category, ErrorCategory::RateLimited);
        assert!(analysis.retryable);
        assert!(analysis.delay_multiplier > 1.0);
    }

    #[test]
    fn test_apply_fixes_remove_from_prompt() {
        let params: HashMap<String, Value> = [(
            "prompt".into(),
            Value::String("a beautiful sex scene in studio".into()),
        )]
        .into_iter()
        .collect();
        let fixes: HashMap<String, ParamFix> = [(
            "prompt".into(),
            ParamFix::RemoveFromPrompt(vec!["sex ".into()]),
        )]
        .into_iter()
        .collect();
        let result = apply_param_fixes(&params, &fixes);
        let prompt = result.get("prompt").unwrap().as_str().unwrap();
        assert!(!prompt.contains("sex"));
        assert!(prompt.contains("beautiful"));
        assert!(prompt.contains("studio"));
    }

    #[test]
    fn test_apply_fixes_set_value_and_append_system() {
        let params: HashMap<String, Value> =
            [("systemPrompt".into(), Value::String("base rules".into()))]
                .into_iter()
                .collect();
        let fixes: HashMap<String, ParamFix> = [
            (
                "action".into(),
                ParamFix::SetValue(serde_json::json!("play")),
            ),
            (
                "_append_system".into(),
                ParamFix::AppendToParam("keep it tasteful".into()),
            ),
        ]
        .into_iter()
        .collect();
        let result = apply_param_fixes(&params, &fixes);
        assert_eq!(result.get("action").and_then(|v| v.as_str()), Some("play"));
        let system = result.get("systemPrompt").unwrap().as_str().unwrap();
        assert!(system.contains("base rules"));
        assert!(system.contains("keep it tasteful"));
    }

    // 收窄后的 Content Policy 测试

    #[test]
    fn test_content_policy_narrowed_blocked() {
        let params = HashMap::new();
        // "blocked" 单独出现不应匹配 ContentPolicy
        let analysis = analyze_error("Request blocked by firewall", "ai.image", &params);
        assert_ne!(
            analysis.category,
            ErrorCategory::ContentPolicy,
            "'blocked by firewall' should NOT be ContentPolicy"
        );

        // "content blocked" 应匹配
        let analysis = analyze_error("content blocked by safety filter", "ai.image", &params);
        assert_eq!(analysis.category, ErrorCategory::ContentPolicy);
    }

    #[test]
    fn test_content_policy_narrowed_not_allowed() {
        let params = HashMap::new();
        // "not allowed" 单独出现不应匹配 ContentPolicy
        let analysis = analyze_error("Method not allowed (405)", "ai.image", &params);
        assert_ne!(
            analysis.category,
            ErrorCategory::ContentPolicy,
            "'Method not allowed' should NOT be ContentPolicy"
        );

        // "image not allowed" 应匹配
        let analysis = analyze_error("This image is not allowed", "ai.image", &params);
        assert_eq!(analysis.category, ErrorCategory::ContentPolicy);
    }

    #[test]
    fn test_service_unavailable_detection() {
        let params = HashMap::new();
        let analysis = analyze_error("Connection timeout after 30s", "ai.image", &params);
        assert_eq!(analysis.category, ErrorCategory::ServiceUnavailable);
        assert!(analysis.retryable);
        assert!(analysis.delay_multiplier >= 3.0);
    }

    #[test]
    fn test_not_found_detection() {
        let params = HashMap::new();
        let analysis = analyze_error(
            "404 Not Found: resource does not exist",
            "data.read",
            &params,
        );
        assert_eq!(analysis.category, ErrorCategory::NotFound);
        assert!(!analysis.retryable);
    }

    #[test]
    fn test_permission_denied_detection() {
        let params = HashMap::new();
        let analysis = analyze_error("403 Forbidden: permission denied", "data.write", &params);
        assert_eq!(analysis.category, ErrorCategory::PermissionDenied);
        assert!(!analysis.retryable);
    }

    #[test]
    fn test_parse_error_detection() {
        let params = HashMap::new();
        let analysis = analyze_error("failed to parse invalid json", "ai.analyze", &params);
        assert_eq!(analysis.category, ErrorCategory::ParseError);
        assert!(analysis.retryable);
    }

    #[test]
    fn test_image_content_policy() {
        let params: HashMap<String, Value> = [("prompt".into(), Value::String("test".into()))]
            .into_iter()
            .collect();
        let analysis = analyze_error(
            "image generation failed: content moderation violation",
            "ai.image",
            &params,
        );
        assert_eq!(analysis.category, ErrorCategory::ContentPolicy);
    }

    // ── 配置缺失 / API Key：不可重试 ──

    #[test]
    fn test_gemini_api_key_not_configured_non_retryable() {
        let params = HashMap::new();
        // leftover Chinese copy; classifier still matches `contains("API Key 未配置")`
        let analysis = analyze_error("Gemini API Key 未配置", "ai.webSearch", &params);
        assert_eq!(analysis.category, ErrorCategory::Configuration);
        assert!(
            !analysis.retryable,
            "API Key 未配置必须非重试，避免烧 global_retry_budget"
        );
        assert!(analysis.suggested_prepend_capability.is_none());
    }

    #[test]
    fn test_api_key_not_configured_english_non_retryable() {
        let params = HashMap::new();
        let analysis = analyze_error(
            "Gemini API Key is not configured",
            "ai.groundingSearch",
            &params,
        );
        assert_eq!(analysis.category, ErrorCategory::Configuration);
        assert!(!analysis.retryable);
    }

    #[test]
    fn test_tts_not_configured_non_retryable() {
        let params = HashMap::new();
        let analysis = analyze_error(
            "TTS 服务未配置。请在设置中配置语音合成服务后重试。",
            "ai.tts",
            &params,
        );
        assert_eq!(analysis.category, ErrorCategory::Configuration);
        assert!(!analysis.retryable);
    }

    #[test]
    fn test_ai_analyzer_not_configured_non_retryable() {
        let params = HashMap::new();
        let analysis = analyze_error("AI analyzer not configured", "ai.analyze", &params);
        assert_eq!(analysis.category, ErrorCategory::Configuration);
        assert!(!analysis.retryable);
    }

    #[test]
    fn test_previous_attempts_gemini_key_still_non_retryable() {
        // 模拟重试历史文案，确保仍被识别为配置错误而非 Unknown
        let params = HashMap::new();
        let analysis = analyze_error(
            "previous 1 attempts: Gemini API Key 未配置",
            "ai.webSearch",
            &params,
        );
        assert_eq!(analysis.category, ErrorCategory::Configuration);
        assert!(!analysis.retryable);
    }

    #[test]
    fn test_missing_music_control_action_sets_play() {
        let params = HashMap::new();
        let analysis = analyze_error("Missing action parameter", "music.control", &params);
        assert_eq!(analysis.category, ErrorCategory::MissingParameter);
        assert!(analysis.retryable);
        assert!(matches!(
            analysis.param_fixes.get("action"),
            Some(ParamFix::SetValue(_))
        ));
        let fixed = apply_param_fixes(&params, &analysis.param_fixes);
        assert_eq!(fixed.get("action").and_then(|v| v.as_str()), Some("play"));
    }

    #[test]
    fn test_unknown_error_is_retryable_with_multiplier() {
        let params = HashMap::new();
        let analysis = analyze_error(
            "something completely unexpected blew up",
            "ai.chat",
            &params,
        );
        assert_eq!(analysis.category, ErrorCategory::Unknown);
        assert!(analysis.retryable);
        assert!(analysis.delay_multiplier >= 2.0);
        assert!(analysis.description.contains("Unknown error"));
    }
}
