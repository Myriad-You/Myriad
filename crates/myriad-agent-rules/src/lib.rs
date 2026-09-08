//! Pure Agent rules: prompt caps, brew write policy, error classification.
//!
//! No I/O. Backend services re-export moved symbols so existing imports compile.

pub mod brew;
pub mod data_read;
pub mod error;
pub mod external;
pub mod image;
pub mod plan_contract;
pub mod prompt;
pub mod retry;
pub mod schedule;
pub mod semantic;
pub mod steering;
pub mod task;

pub use brew::{
    clamp_update_interval_minutes, collect_subscribe_url_candidates, extract_and_prioritize_feeds,
    feed_priority_score, is_disallowed_subscribe_host, is_disallowed_subscribe_ip,
    platform_write_cap_error, platform_write_items_over_cap, sanitize_feed_name,
    take_feed_urls_to_try, validate_subscribe_url_policy, MAX_FEED_NAME_LEN, MAX_FEED_URLS,
    MAX_PLATFORM_WRITE_ITEMS, MAX_UPDATE_INTERVAL, MIN_UPDATE_INTERVAL,
};
pub use data_read::{
    extract_json_array_from_ai_response, extract_json_object_from_ai_response,
    parse_rsshub_radar_rules, project_time_info, weekday_zh,
};
pub use error::{analyze_error, apply_param_fixes, ErrorAnalysis, ErrorCategory, ParamFix};
pub use external::{
    classify_outbound_fetch, compress_and_truncate_text, first_i64_param, first_string_param,
    hitokoto_type, http_body_exceeds_limit, http_body_size_error, http_content_length_error,
    http_fetch_method, match_mcp_capability_id, mcp_arguments, optional_string_param,
    parse_http_body_value, parse_mcp_capability_id, sanitize_http_headers, scrape_max_length,
    scrape_selector, scrape_should_skip_tag, HTTP_FETCH_MAX_BODY_BYTES, SCRAPE_SKIP_TAGS,
    WEB_SCRAPE_DEFAULT_MAX_LENGTH, WEB_SCRAPE_MAX_HTML_BYTES,
};
pub use image::{
    clamp_image_dim, parse_image_dim, resolve_image_dimensions, resolve_image_prompt,
    resolve_image_size, resolve_negative_prompt, DEFAULT_IMAGE_HEIGHT, DEFAULT_IMAGE_WIDTH,
    IMAGE_DIM_MAX, IMAGE_DIM_MIN,
};
pub use plan_contract::{
    plan_image_size_rule, plan_step_cap_rule, MAX_PLAN_STEPS, PLAN_DATA_FLOW_RULE,
    PLAN_DEPENDENCY_RULE,
};
pub use prompt::{
    append_memory_to_system_prompt, merge_system_prompt, sanitize_prompt_input,
    take_recent_conversation_messages, untrusted_block,
};
pub use retry::{
    compute_retry_delay_ms, format_retry_final_error, prepend_step_id, should_retry_step,
    FailureStrategy, RetryConfig, RETRY_BASE_DELAY_FLOOR_MS, RETRY_DEFAULT_BASE_DELAY_MS,
    RETRY_DELAY_CAP_MS,
};
pub use schedule::{
    build_schedule_config, extract_raw_backend_actions, heartbeat_task_id,
    heartbeat_update_has_fields, parse_brew_schedule_action, parse_execution_target,
    parse_schedule_type, AgentExecutionTarget, AgentScheduleType, BrewScheduleAction,
};
pub use semantic::{
    capability_needs_conversation_context, capability_needs_memory, extract_semantic_text,
    task_inner_value,
};
pub use steering::{
    append_instruction, inject_directive_to_params, inject_steering_to_params, with_system_guidance,
};
pub use task::{
    is_cancellable_task_status, is_terminal_past_retention, is_waiting_input_timed_out,
    lane_id_from_user_session, session_id_from_lane_id, session_id_from_lane_key,
    status_counts_from_iter, task_status_from_db_str, task_status_to_db_str,
    waiting_input_timeout_error, TaskStatus, TERMINAL_RETENTION_HOURS, WAITING_INPUT_TIMEOUT_HOURS,
};

/// Shared Unicode-scalar cap for user-authored model text (chat, generate, analyze).
pub const USER_TEXT_MAX_CHARS: usize = 32680;

/// Cap for user text injected into model prompts (analyze instruction, chat, search).
pub const SANITIZE_PROMPT_MAX_CHARS: usize = USER_TEXT_MAX_CHARS;

/// Unicode scalar cap. Must stay <= `image_generation::MAX_PROMPT_CHARS` (32680).
/// Counted with `chars()`, not bytes — CJK prompts are 3 bytes per character.
pub const IMAGE_PROMPT_MAX_CHARS: usize = USER_TEXT_MAX_CHARS;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_caps_share_the_user_authored_limit() {
        assert_eq!(SANITIZE_PROMPT_MAX_CHARS, USER_TEXT_MAX_CHARS);
        assert_eq!(IMAGE_PROMPT_MAX_CHARS, USER_TEXT_MAX_CHARS);
        const { assert!(USER_TEXT_MAX_CHARS > 0) }
        let over: String = "x".repeat(IMAGE_PROMPT_MAX_CHARS + 1);
        assert!(over.chars().count() > IMAGE_PROMPT_MAX_CHARS);
    }
}
