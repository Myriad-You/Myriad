//! Thin adapter: error classification / param fixes live in
//! [`crate::services::agent::error_analyzer_pure`].
//!
//! Executor retry / DAG paths keep using `ErrorAnalyzer::{analyze,apply_fixes}`
//! so call sites stay stable while pure rules remain I/O-free and unit-tested.

pub use crate::services::agent::error_analyzer_pure::{ErrorAnalysis, ErrorCategory, ParamFix};

use crate::services::agent::error_analyzer_pure::{analyze_error, apply_param_fixes};
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

    /// 将错误分析的修复建议应用到参数上。
    pub fn apply_fixes(
        params: &HashMap<String, Value>,
        fixes: &HashMap<String, ParamFix>,
    ) -> HashMap<String, Value> {
        apply_param_fixes(params, fixes)
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

    #[test]
    fn adapter_apply_fixes_matches_pure() {
        let params: HashMap<String, Value> = [(
            "prompt".into(),
            Value::String("remove sex word here".into()),
        )]
        .into_iter()
        .collect();
        let fixes: HashMap<String, ParamFix> = [(
            "prompt".into(),
            ParamFix::RemoveFromPrompt(vec!["sex ".into()]),
        )]
        .into_iter()
        .collect();
        let via_adapter = ErrorAnalyzer::apply_fixes(&params, &fixes);
        let via_pure = apply_param_fixes(&params, &fixes);
        assert_eq!(via_adapter, via_pure);
        assert!(!via_adapter
            .get("prompt")
            .and_then(|v| v.as_str())
            .unwrap()
            .contains("sex"));
    }
}
