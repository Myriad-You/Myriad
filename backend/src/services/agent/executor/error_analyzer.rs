//! Thin adapter: error classification / param fixes live in
//! [`crate::services::agent::error_analyzer_pure`].
//!
//! Serial retry calls `analyze_error` plus `apply_param_fixes`. This adapter
//! is `ErrorAnalyzer::analyze`; param fixes stay in `error_analyzer_pure`.

pub use crate::services::agent::error_analyzer_pure::{ErrorAnalysis, ErrorCategory, ParamFix};

use crate::services::agent::error_analyzer_pure::analyze_error;
use serde_json::Value;
use std::collections::HashMap;

/// Facade over pure error analysis (no I/O).
pub struct ErrorAnalyzer;

impl ErrorAnalyzer {
    /// 分析执行错误，返回分类和修复建议。
    pub fn analyze(
        error: &str,
        capability_id: &str,
        params: &HashMap<String, Value>,
    ) -> ErrorAnalysis {
        analyze_error(error, capability_id, params)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Adapter must forward to the shipped pure surface (not re-implement).
    #[test]
    fn adapter_analyze_matches_pure_for_config_error() {
        let params = HashMap::new();
        let via_adapter = ErrorAnalyzer::analyze("Gemini API Key 未配置", "ai.webSearch", &params);
        let via_pure = analyze_error("Gemini API Key 未配置", "ai.webSearch", &params);
        assert_eq!(via_adapter.category, via_pure.category);
        assert_eq!(via_adapter.retryable, via_pure.retryable);
        assert_eq!(via_adapter.category, ErrorCategory::Configuration);
        assert!(!via_adapter.retryable);
    }
}
