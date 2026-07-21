//! 执行错误分析器
//!
//! 在重试前分析错误消息，将错误分类并生成参数修复建议。
//! 使系统能够"理解"错误原因并智能调整重试策略，而不是盲目重复相同的执行。

use serde_json::Value;
use std::collections::HashMap;

use super::utils::truncate_str;

/// 错误分类
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub enum ErrorCategory {
    /// 内容策略违规（如 NSFW 内容被拒）
    ContentPolicy,
    /// 参数缺失或无效
    MissingParameter,
    /// 参数值格式/范围错误
    InvalidParameter,
    /// API 速率限制
    RateLimited,
    /// 服务暂时不可用（网络超时等）
    ServiceUnavailable,
    /// API 响应解析失败
    ParseError,
    /// 权限不足
    PermissionDenied,
    /// 资源未找到（404 等）
    NotFound,
    /// 配置缺失（API Key 未配置等）—— 不可重试，不应消耗 retry budget
    Configuration,
    /// 未知错误
    Unknown,
}

/// 错误分析结果
#[derive(Debug, Clone)]
pub struct ErrorAnalysis {
    /// 错误分类
    pub category: ErrorCategory,
    /// 是否值得重试（修改参数后）
    pub retryable: bool,
    /// 参数修改建议
    pub param_fixes: HashMap<String, ParamFix>,
    /// 人类可读的分析描述
    pub description: String,
    /// 建议的重试延迟倍率（1.0 = 正常，5.0 = 长等待）
    pub delay_multiplier: f64,
    /// 建议在重试前先执行的能力（如缺少数据时先搜索）
    pub suggested_prepend_capability: Option<String>,
    /// 建议的前置步骤参数（避免生成空参数的无效步骤）
    pub suggested_prepend_params: HashMap<String, Value>,
}

/// 参数修复动作
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum ParamFix {
    /// 从提示词中移除指定关键词/模式
    RemoveFromPrompt(Vec<String>),
    /// 替换提示词中的内容
    ReplaceInPrompt { from: String, to: String },
    /// 设置参数到指定值
    SetValue(Value),
    /// 追加文本到已有参数
    AppendToParam(String),
}

/// 错误分析器
pub struct ErrorAnalyzer;

impl ErrorAnalyzer {
    /// 分析执行错误，返回分类和修复建议
    pub fn analyze(
        error: &str,
        capability_id: &str,
        params: &HashMap<String, Value>,
    ) -> ErrorAnalysis {
        let error_lower = error.to_lowercase();

        // 0. 配置缺失 / API Key 未配置 —— 不可重试，不消耗 global_retry_budget
        // 必须在 Unknown 默认分支之前，且优先于通用 missing-param（避免被当成可修参数）
        if Self::is_configuration_error(error, &error_lower) {
            return ErrorAnalysis {
                category: ErrorCategory::Configuration,
                retryable: false,
                param_fixes: HashMap::new(),
                description: format!(
                    "配置缺失（不可重试）: {}",
                    truncate_str(error, 100)
                ),
                delay_multiplier: 1.0,
                suggested_prepend_capability: None,
                suggested_prepend_params: HashMap::new(),
            };
        }

        // 1. 内容策略违规
        if let Some(analysis) = Self::check_content_policy(&error_lower, capability_id, params) {
            return analysis;
        }

        // 2. 参数缺失
        if let Some(analysis) = Self::check_missing_param(&error_lower, capability_id, params) {
            return analysis;
        }

        // 3. 速率限制
        if Self::is_rate_limited(&error_lower) {
            return ErrorAnalysis {
                category: ErrorCategory::RateLimited,
                retryable: true,
                param_fixes: HashMap::new(),
                description: "API 速率限制，等待后重试".into(),
                delay_multiplier: 5.0,
                suggested_prepend_capability: None,
                suggested_prepend_params: HashMap::new(),
            };
        }

        // 4. 服务不可用 / 网络问题
        if Self::is_service_unavailable(&error_lower) {
            return ErrorAnalysis {
                category: ErrorCategory::ServiceUnavailable,
                retryable: true,
                param_fixes: HashMap::new(),
                description: "服务暂时不可用，等待后重试".into(),
                delay_multiplier: 3.0,
                suggested_prepend_capability: None,
                suggested_prepend_params: HashMap::new(),
            };
        }

        // 5. 响应解析失败
        if Self::is_parse_error(&error_lower) {
            return ErrorAnalysis {
                category: ErrorCategory::ParseError,
                retryable: true,
                param_fixes: HashMap::new(),
                description: "API 响应解析失败，重试可能产生有效响应".into(),
                delay_multiplier: 1.5,
                suggested_prepend_capability: None,
                suggested_prepend_params: HashMap::new(),
            };
        }

        // 6. 权限问题（不可重试）
        if Self::is_permission_error(&error_lower) {
            return ErrorAnalysis {
                category: ErrorCategory::PermissionDenied,
                retryable: false,
                param_fixes: HashMap::new(),
                description: "权限不足，需要用户授权".into(),
                delay_multiplier: 1.0,
                suggested_prepend_capability: None,
                suggested_prepend_params: HashMap::new(),
            };
        }

        // 7. 资源未找到
        if Self::is_not_found(&error_lower) {
            return ErrorAnalysis {
                category: ErrorCategory::NotFound,
                retryable: false,
                param_fixes: HashMap::new(),
                description: "请求的资源不存在".into(),
                delay_multiplier: 1.0,
                suggested_prepend_capability: None,
                suggested_prepend_params: HashMap::new(),
            };
        }

        // 默认：未知错误，允许一次重试
        ErrorAnalysis {
            category: ErrorCategory::Unknown,
            retryable: true,
            param_fixes: HashMap::new(),
            description: format!("未知错误: {}", truncate_str(error, 100)),
            delay_multiplier: 2.0,
            suggested_prepend_capability: None,
            suggested_prepend_params: HashMap::new(),
        }
    }

    /// API Key / 服务未配置 —— 重试无效，且不应被当成 Unknown 烧掉预算
    fn is_configuration_error(error: &str, error_lower: &str) -> bool {
        // response_agent::api_key_not_configured → "{service} API Key 未配置"
        if error.contains("API Key 未配置") || error_lower.contains("api key 未配置") {
            return true;
        }
        if error_lower.contains("api key not configured")
            || error_lower.contains("api_key not configured")
            || error_lower.contains("api key is not configured")
            || error_lower.contains("missing api key")
            || error_lower.contains("no api key")
            || error_lower.contains("api key is empty")
            || error_lower.contains("api key missing")
        {
            return true;
        }
        // 通用「未配置 / not configured」（TTS、AI analyzer 等）
        if error.contains("未配置") || error_lower.contains("not configured") {
            return true;
        }
        // 英文配置缺失常见写法
        if error_lower.contains("is not set") && error_lower.contains("key") {
            return true;
        }
        false
    }

    /// 检测内容策略违规并生成修复建议
    fn check_content_policy(
        error_lower: &str,
        capability_id: &str,
        params: &HashMap<String, Value>,
    ) -> Option<ErrorAnalysis> {
        let is_content_violation = error_lower.contains("disallowed content")
            || error_lower.contains("content policy")
            || error_lower.contains("safety filter")
            || error_lower.contains("content blocked")
            || error_lower.contains("nsfw")
            || error_lower.contains("inappropriate")
            || error_lower.contains("violat")
            // PixAI / image generation specific patterns（收窄匹配避免误判网络/权限错误）
            || error_lower.contains("moderation")
            || error_lower.contains("unsafe content")
            || error_lower.contains("content not allowed")
            || error_lower.contains("prohibited content")
            || error_lower.contains("sensitive content")
            || (error_lower.contains("blocked") && (error_lower.contains("content") || error_lower.contains("prompt") || error_lower.contains("filter")))
            || (error_lower.contains("not allowed") && (error_lower.contains("content") || error_lower.contains("prompt") || error_lower.contains("image")))
            || (error_lower.contains("pixai") && error_lower.contains("failed")
                && (error_lower.contains("prompt") || error_lower.contains("content")));

        if !is_content_violation {
            return None;
        }

        let mut param_fixes = HashMap::new();

        // 对于图像生成，清理提示词中的敏感内容
        if capability_id == "ai.image" || capability_id == "prompt.generate" {
            let prompt_key = if params.contains_key("prompt") {
                "prompt"
            } else if params.contains_key("description") {
                "description"
            } else if params.contains_key("input") {
                "input"
            } else if params.contains_key("text") {
                "text"
            } else {
                "prompt"
            };

            // 提取被拒的具体内容关键词
            let sensitive_words = Self::extract_sensitive_keywords(error_lower);
            if !sensitive_words.is_empty() {
                param_fixes.insert(
                    prompt_key.to_string(),
                    ParamFix::RemoveFromPrompt(sensitive_words.clone()),
                );
            }

            // 追加安全修饰语
            param_fixes.insert(
                "_append_system".to_string(),
                ParamFix::AppendToParam(
                    "Ensure the output is tasteful, artistic, and avoids explicit or suggestive content. Focus on aesthetic beauty, elegant composition, and emotional atmosphere.".to_string()
                ),
            );
        }

        Some(ErrorAnalysis {
            category: ErrorCategory::ContentPolicy,
            retryable: true,
            param_fixes,
            description: "内容策略违规，尝试清理敏感内容后重试".into(),
            delay_multiplier: 1.0,
            suggested_prepend_capability: None,
            suggested_prepend_params: HashMap::new(),
        })
    }

    /// 从错误消息中提取被标记的敏感关键词
    fn extract_sensitive_keywords(error_lower: &str) -> Vec<String> {
        let mut keywords = Vec::new();

        // 匹配 "disallowed content: xxx, yyy" 模式（提取所有逗号分隔的关键词）
        if let Some(pos) = error_lower.find("disallowed content:") {
            let prefix = "disallowed content:";
            let after = &error_lower[pos + prefix.len()..];
            // 截取到句号/分号/换行为止的整个短语
            let phrase = after
                .trim()
                .split(['.', ';', '\n'])
                .next()
                .unwrap_or("")
                .trim();
            // 按逗号拆分每个关键词
            for part in phrase.split(',') {
                let word = part.trim().trim_start_matches("and ").trim();
                if !word.is_empty() && word.len() < 50 {
                    keywords.push(word.to_string());
                }
            }
        }

        // 通用敏感关键词列表（会被从 prompt 中移除）
        let common_sensitive = [
            "sex",
            "nude",
            "naked",
            "explicit",
            "nsfw",
            "erotic",
            "pornographic",
        ];
        for w in &common_sensitive {
            if error_lower.contains(w) {
                keywords.push(w.to_string());
            }
        }

        keywords.sort();
        keywords.dedup();
        keywords
    }

    /// 检测参数缺失错误并生成修复建议
    fn check_missing_param(
        error_lower: &str,
        capability_id: &str,
        params: &HashMap<String, Value>,
    ) -> Option<ErrorAnalysis> {
        let is_missing = (error_lower.contains("missing")
            && (error_lower.contains("parameter") || error_lower.contains("param")))
            || error_lower.contains("缺少")
            || error_lower.contains("需要提供");

        if !is_missing {
            return None;
        }

        let mut param_fixes = HashMap::new();
        let mut suggested_prepend = None;
        let mut suggested_prepend_params = HashMap::new();

        // 特定能力的参数修复规则
        match capability_id {
            // 常见问题：缺少 playlistId → 建议先搜索播放列表
            "music.playlist" if error_lower.contains("playlistid") => {
                suggested_prepend = Some("netease.searchPlaylist".to_string());
                // 从原始参数中提取搜索关键词
                let keyword = params
                    .get("keyword")
                    .or_else(|| params.get("query"))
                    .or_else(|| params.get("name"))
                    .cloned()
                    .unwrap_or_else(|| Value::String("推荐歌单".to_string()));
                suggested_prepend_params.insert("keyword".to_string(), keyword);
            }
            "music.control" if error_lower.contains("action") => {
                param_fixes.insert(
                    "action".to_string(),
                    ParamFix::SetValue(serde_json::json!("play")),
                );
            }
            "brew.discover"
                if error_lower.contains("url") || error_lower.contains("query") =>
            {
                param_fixes.insert(
                    "query".to_string(),
                    ParamFix::SetValue(serde_json::json!("*")),
                );
            }
            _ => {}
        }

        Some(ErrorAnalysis {
            category: ErrorCategory::MissingParameter,
            retryable: !param_fixes.is_empty() || suggested_prepend.is_some(),
            param_fixes,
            description: format!("参数缺失: {}", truncate_str(error_lower, 80)),
            delay_multiplier: 1.0,
            suggested_prepend_capability: suggested_prepend,
            suggested_prepend_params,
        })
    }

    fn is_rate_limited(error_lower: &str) -> bool {
        error_lower.contains("rate limit")
            || error_lower.contains("too many requests")
            || error_lower.contains("429")
            || error_lower.contains("quota exceeded")
            || error_lower.contains("throttl")
    }

    fn is_service_unavailable(error_lower: &str) -> bool {
        error_lower.contains("service unavailable")
            || error_lower.contains("503")
            || error_lower.contains("502")
            || error_lower.contains("connection refused")
            || error_lower.contains("timeout")
            || error_lower.contains("timed out")
            || error_lower.contains("network error")
            || error_lower.contains("temporarily unavailable")
    }

    fn is_parse_error(error_lower: &str) -> bool {
        error_lower.contains("parse")
            || error_lower.contains("deserializ")
            || error_lower.contains("invalid json")
            || error_lower.contains("unexpected token")
            || error_lower.contains("failed to parse")
    }

    fn is_permission_error(error_lower: &str) -> bool {
        error_lower.contains("permission denied")
            || error_lower.contains("unauthorized")
            || error_lower.contains("403")
            || error_lower.contains("权限不足")
            || error_lower.contains("access denied")
    }

    fn is_not_found(error_lower: &str) -> bool {
        error_lower.contains("not found")
            || error_lower.contains("404")
            || error_lower.contains("no such")
            || error_lower.contains("does not exist")
    }

    /// 将错误分析的修复建议应用到参数上
    pub fn apply_fixes(
        params: &HashMap<String, Value>,
        fixes: &HashMap<String, ParamFix>,
    ) -> HashMap<String, Value> {
        let mut new_params = params.clone();

        for (param_name, fix) in fixes {
            // 跳过元指令（以 _ 开头的不是真实参数）
            if param_name.starts_with('_') {
                // _append_system: 追加到 systemPrompt
                if param_name == "_append_system" {
                    if let ParamFix::AppendToParam(text) = fix {
                        let existing = new_params
                            .get("systemPrompt")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        new_params.insert(
                            "systemPrompt".to_string(),
                            Value::String(format!("{}\n\n{}", existing, text)),
                        );
                    }
                }
                continue;
            }

            match fix {
                ParamFix::RemoveFromPrompt(words) => {
                    if let Some(Value::String(text)) = new_params.get(param_name) {
                        let mut cleaned = text.clone();
                        for word in words {
                            // 大小写不敏感移除（UTF-8 安全：通过 replacen 匹配小写副本再按位置切原始串）
                            let word_lower = word.to_lowercase();
                            loop {
                                let lower = cleaned.to_lowercase();
                                if let Some(byte_pos) = lower.find(&word_lower) {
                                    // byte_pos 在 lower 中找到，但 to_lowercase 可能改变字节长度
                                    // 安全做法：找到匹配的字符范围
                                    let char_start = lower[..byte_pos].chars().count();
                                    let char_len = word_lower.chars().count();
                                    let before: String = cleaned.chars().take(char_start).collect();
                                    let after: String =
                                        cleaned.chars().skip(char_start + char_len).collect();
                                    cleaned = format!("{}{}", before, after);
                                } else {
                                    break;
                                }
                            }
                        }
                        new_params.insert(param_name.to_string(), Value::String(cleaned));
                    }
                }
                ParamFix::ReplaceInPrompt { from, to } => {
                    if let Some(Value::String(text)) = new_params.get(param_name) {
                        // 大小写不敏感替换（与 RemoveFromPrompt 一致）
                        let from_lower = from.to_lowercase();
                        let mut result = text.clone();
                        loop {
                            let lower = result.to_lowercase();
                            if let Some(byte_pos) = lower.find(&from_lower) {
                                let char_start = lower[..byte_pos].chars().count();
                                let char_len = from_lower.chars().count();
                                let before: String = result.chars().take(char_start).collect();
                                let after: String =
                                    result.chars().skip(char_start + char_len).collect();
                                result = format!("{}{}{}", before, to, after);
                            } else {
                                break;
                            }
                        }
                        new_params.insert(param_name.to_string(), Value::String(result));
                    }
                }
                ParamFix::SetValue(val) => {
                    new_params.insert(param_name.to_string(), val.clone());
                }
                ParamFix::AppendToParam(text) => {
                    let existing = new_params
                        .get(param_name)
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let appended = if existing.is_empty() {
                        text.clone()
                    } else {
                        format!("{} {}", existing, text)
                    };
                    new_params.insert(param_name.to_string(), Value::String(appended));
                }
            }
        }

        new_params
    }
}

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
        let analysis = ErrorAnalyzer::analyze(
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
        let analysis = ErrorAnalyzer::analyze(
            "Missing playlistId parameter. Use netease.searchPlaylist first.",
            "music.playlist",
            &params,
        );
        assert_eq!(analysis.category, ErrorCategory::MissingParameter);
    }

    #[test]
    fn test_rate_limit_detection() {
        let params = HashMap::new();
        let analysis = ErrorAnalyzer::analyze("429 Too Many Requests", "ai.image", &params);
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
        let result = ErrorAnalyzer::apply_fixes(&params, &fixes);
        let prompt = result.get("prompt").unwrap().as_str().unwrap();
        assert!(!prompt.contains("sex"));
    }

    // ======== 收窄后的 Content Policy 测试 ========

    #[test]
    fn test_content_policy_narrowed_blocked() {
        let params = HashMap::new();
        // "blocked" 单独出现不应匹配 ContentPolicy
        let analysis = ErrorAnalyzer::analyze("Request blocked by firewall", "ai.image", &params);
        assert_ne!(
            analysis.category,
            ErrorCategory::ContentPolicy,
            "'blocked by firewall' should NOT be ContentPolicy"
        );

        // "content blocked" 应匹配
        let analysis =
            ErrorAnalyzer::analyze("content blocked by safety filter", "ai.image", &params);
        assert_eq!(analysis.category, ErrorCategory::ContentPolicy);
    }

    #[test]
    fn test_content_policy_narrowed_not_allowed() {
        let params = HashMap::new();
        // "not allowed" 单独出现不应匹配 ContentPolicy
        let analysis = ErrorAnalyzer::analyze("Method not allowed (405)", "ai.image", &params);
        assert_ne!(
            analysis.category,
            ErrorCategory::ContentPolicy,
            "'Method not allowed' should NOT be ContentPolicy"
        );

        // "image not allowed" 应匹配
        let analysis = ErrorAnalyzer::analyze("This image is not allowed", "ai.image", &params);
        assert_eq!(analysis.category, ErrorCategory::ContentPolicy);
    }

    #[test]
    fn test_service_unavailable_detection() {
        let params = HashMap::new();
        let analysis = ErrorAnalyzer::analyze("Connection timeout after 30s", "ai.image", &params);
        assert_eq!(analysis.category, ErrorCategory::ServiceUnavailable);
        assert!(analysis.retryable);
    }

    #[test]
    fn test_not_found_detection() {
        let params = HashMap::new();
        let analysis = ErrorAnalyzer::analyze(
            "404 Not Found: resource does not exist",
            "data.read",
            &params,
        );
        assert_eq!(analysis.category, ErrorCategory::NotFound);
    }

    #[test]
    fn test_pixai_content_policy() {
        let params: HashMap<String, Value> = [("prompt".into(), Value::String("test".into()))]
            .into_iter()
            .collect();
        let analysis = ErrorAnalyzer::analyze(
            "PixAI image generation failed: content moderation violation",
            "ai.image",
            &params,
        );
        assert_eq!(analysis.category, ErrorCategory::ContentPolicy);
    }

    // ── 配置缺失 / API Key：不可重试 ──

    #[test]
    fn test_gemini_api_key_not_configured_non_retryable() {
        let params = HashMap::new();
        // 与 response_agent::api_key_not_configured("Gemini") 一致
        let analysis =
            ErrorAnalyzer::analyze("Gemini API Key 未配置", "ai.webSearch", &params);
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
        let analysis = ErrorAnalyzer::analyze(
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
        let analysis = ErrorAnalyzer::analyze(
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
        let analysis =
            ErrorAnalyzer::analyze("AI analyzer not configured", "ai.analyze", &params);
        assert_eq!(analysis.category, ErrorCategory::Configuration);
        assert!(!analysis.retryable);
    }

    #[test]
    fn test_previous_attempts_gemini_key_still_non_retryable() {
        // 模拟重试历史文案，确保仍被识别为配置错误而非 Unknown
        let params = HashMap::new();
        let analysis = ErrorAnalyzer::analyze(
            "previous 1 attempts: Gemini API Key 未配置",
            "ai.webSearch",
            &params,
        );
        assert_eq!(analysis.category, ErrorCategory::Configuration);
        assert!(!analysis.retryable);
    }
}
