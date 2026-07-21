//! 结果评估器
//!
//! 评估任务执行结果是否**真正满足**用户的原始目标
//!
//! ## 验证层次
//!
//! 1. **失败模式检测**：识别"未找到"、"无结果"等语义失败
//! 2. **数据源验证**：检查数据获取步骤是否返回了有效数据
//! 3. **目标匹配验证**：确保结果真正回答了用户的问题
//! 4. **升级门控**：本地数据域 / stub 空实现不得建议 ai.webSearch

use serde_json::Value;

/// 评估上下文（用于门控联网搜索等升级建议）
#[derive(Debug, Clone, Default)]
pub struct EvaluationContext {
    /// 本次执行涉及的能力 ID 列表
    pub capability_ids: Vec<String>,
    /// 显式允许联网搜索升级（如 generateReadingList + allowWebSearch 标志，
    /// 或明确的外部调研意图）
    pub allow_web_search: bool,
}

/// 评估结果
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Evaluation {
    /// 结果是否满足目标
    pub is_satisfied: bool,
    /// 满意度分数 (0.0 - 1.0)
    pub satisfaction_score: f32,
    /// 不满意的原因
    pub reason: Option<String>,
    /// 建议尝试联网搜索
    pub suggests_web_search: bool,
    /// 建议扩大搜索范围
    pub suggests_expand_scope: bool,
    /// 建议优先使用本地能力（brew.page / search.fuzzy 等）
    pub suggests_local_alternatives: bool,
    /// 本地 notFound 返回的可重试实体建议（源名/作者等），replan 应优先用这些值重试 brew
    pub suggested_retry_values: Vec<String>,
    /// 建议的改进方向
    pub improvement_hints: Vec<String>,
    /// 检测到的失败模式
    pub failure_patterns: Vec<FailurePattern>,
}

/// 失败模式类型
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub enum FailurePattern {
    /// 数据源返回空结果
    EmptyDataSource,
    /// 检测到"未找到"语义
    NotFoundSemantic,
    /// 检测到"无匹配"语义
    NoMatchSemantic,
    /// 结果与查询不相关
    IrrelevantResult,
    /// AI 总结了"没有数据"
    SummarizedNothing,
    /// 数据数量为 0
    ZeroCount,
    /// 检测到 stub / 空实现（数据路径未接通）
    StubOrHollowSuccess,
}

impl Default for Evaluation {
    fn default() -> Self {
        Self {
            is_satisfied: true,
            satisfaction_score: 1.0,
            reason: None,
            suggests_web_search: false,
            suggests_expand_scope: false,
            suggests_local_alternatives: false,
            suggested_retry_values: Vec::new(),
            improvement_hints: Vec::new(),
            failure_patterns: Vec::new(),
        }
    }
}

/// 结果评估器
pub struct ResultEvaluator {
    /// 最小可接受的数据条数
    min_data_count: usize,
}

impl ResultEvaluator {
    pub fn new() -> Self {
        Self { min_data_count: 1 }
    }

    /// 评估结果（无需 ParsedIntent，仅检测失败模式和数据充足性）
    ///
    /// 用于 Planner 管线的简化评估，不依赖旧的 ParsedIntent 类型。
    /// 生产路径优先 `evaluate_with_context`；本方法供无能力上下文的简化调用与单测。
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn evaluate_result(&self, result: &Value) -> Evaluation {
        self.evaluate_with_context(result, &EvaluationContext::default())
    }

    /// 带上下文的结果评估（门控 webSearch 升级）
    pub fn evaluate_with_context(&self, result: &Value, ctx: &EvaluationContext) -> Evaluation {
        let mut eval = Evaluation::default();

        // 第一层：失败模式检测
        self.detect_failure_patterns(&mut eval, result);
        if !eval.failure_patterns.is_empty() {
            eval.is_satisfied = false;
            eval.satisfaction_score = 0.0;
            eval.reason = Some(self.describe_failure_patterns(&eval.failure_patterns));
            self.apply_escalation_policy(&mut eval, result, ctx);
            return eval;
        }

        // 第二层：数据源验证
        let data_count = self.count_actual_data(result);
        if data_count < self.min_data_count {
            eval.is_satisfied = false;
            eval.satisfaction_score = 0.1;
            eval.failure_patterns.push(FailurePattern::ZeroCount);
            eval.reason = Some(format!(
                "数据不足：找到 {} 条有效数据，需要至少 {} 条",
                data_count, self.min_data_count
            ));
            self.apply_escalation_policy(&mut eval, result, ctx);
            return eval;
        }

        // 通用目标评估
        self.evaluate_generic_goal(&mut eval, result);
        if !eval.is_satisfied {
            self.apply_escalation_policy(&mut eval, result, ctx);
        }
        eval
    }

    /// 根据 stub / 本地数据域 / 白名单 决定是否建议联网搜索
    fn apply_escalation_policy(
        &self,
        eval: &mut Evaluation,
        result: &Value,
        ctx: &EvaluationContext,
    ) {
        // 提取 notFound 建议值（无论是否本地域，供 replan 使用）
        eval.suggested_retry_values = self.extract_suggested_retry_values(result);

        let is_stub = eval
            .failure_patterns
            .contains(&FailurePattern::StubOrHollowSuccess)
            || self.has_stub_markers(result);
        let is_local = self.is_local_data_domain(&ctx.capability_ids);
        // generateReadingList 空结果默认本地域：禁止空→web cascade（除非 allow_web_search）
        let is_reading_list_empty = self.is_generate_reading_list_empty(result, &ctx.capability_ids);

        if is_stub {
            eval.suggests_web_search = false;
            eval.suggests_local_alternatives = true;
            eval.improvement_hints.push(
                "检测到数据路径未接通（stub/空实现），请修复数据源或改用已实现的本地能力，不要升级到 ai.webSearch"
                    .to_string(),
            );
            eval.improvement_hints
                .extend(self.local_data_hints(&ctx.capability_ids));
            return;
        }

        // notFound + 实体建议：优先用建议值重试 brew，绝不 webSearch
        if !eval.suggested_retry_values.is_empty()
            && (is_local
                || ctx
                    .capability_ids
                    .iter()
                    .any(|id| id.starts_with("brew."))
                || ctx.capability_ids.is_empty())
        {
            eval.suggests_web_search = false;
            eval.suggests_local_alternatives = true;
            eval.suggests_expand_scope = true;
            eval.improvement_hints
                .push(self.suggestion_retry_hint(&eval.suggested_retry_values, &ctx.capability_ids));
            eval.improvement_hints
                .extend(self.local_data_hints(&ctx.capability_ids));
            return;
        }

        if (is_local || is_reading_list_empty) && !ctx.allow_web_search {
            eval.suggests_web_search = false;
            eval.suggests_local_alternatives = true;
            eval.suggests_expand_scope = true;
            if is_reading_list_empty {
                eval.improvement_hints.push(
                    "brew.generateReadingList 本地无匹配：请放宽 keyword/daysBack、改用 brew.items / search.fuzzy，或引导订阅更多源；禁止空结果级联到 ai.webSearch（除非参数 allowWebSearch=true）"
                        .to_string(),
                );
            }
            eval.improvement_hints
                .extend(self.local_data_hints(&ctx.capability_ids));
            return;
        }

        // 非本地域，或显式允许联网
        eval.suggests_web_search = true;
        eval.improvement_hints
            .push("本地数据不足，尝试联网搜索".to_string());
    }

    /// generateReadingList 空阅读列表（含无能力上下文但结构匹配）
    fn is_generate_reading_list_empty(&self, result: &Value, capability_ids: &[String]) -> bool {
        let cap_is_grl = capability_ids
            .iter()
            .any(|id| id == "brew.generateReadingList");
        let looks_like_grl = result
            .as_object()
            .map(|o| o.contains_key("readingList") || o.contains_key("totalMatched"))
            .unwrap_or(false);
        if !(cap_is_grl || looks_like_grl) {
            return false;
        }
        if let Some(obj) = result.as_object() {
            if let Some(Value::Array(arr)) = obj.get("readingList") {
                if arr.is_empty() {
                    return true;
                }
            }
            if let Some(n) = obj.get("totalMatched").and_then(|v| v.as_i64()) {
                if n == 0 {
                    return true;
                }
            }
        }
        false
    }

    /// 从 notFound / ambiguous 结果提取可重试的实体建议（源名、作者等）
    pub fn extract_suggested_retry_values(&self, result: &Value) -> Vec<String> {
        let Some(obj) = result.as_object() else {
            return Vec::new();
        };

        let mut values = Vec::new();

        // choices[].value 优先（结构化）
        if let Some(choices) = obj.get("choices").and_then(|v| v.as_array()) {
            for c in choices {
                if let Some(v) = c.get("value").and_then(|v| v.as_str()) {
                    let v = v.trim();
                    if !v.is_empty() && Self::is_entity_suggestion(v) {
                        values.push(v.to_string());
                    }
                }
            }
        }

        // suggestions: string 数组（源名/作者），过滤掉操作提示类长句
        if let Some(sugs) = obj.get("suggestions").and_then(|v| v.as_array()) {
            for s in sugs {
                if let Some(s) = s.as_str() {
                    let s = s.trim();
                    if !s.is_empty()
                        && Self::is_entity_suggestion(s)
                        && !values.iter().any(|existing| existing == s)
                    {
                        values.push(s.to_string());
                    }
                }
            }
        }

        values.truncate(5);
        values
    }

    /// 判断 suggestions 项是否为可重试实体名（而非「检查 API Key」类操作提示）
    fn is_entity_suggestion(s: &str) -> bool {
        let t = s.trim();
        if t.is_empty() {
            return false;
        }
        // 操作提示通常较长
        if t.chars().count() > 40 {
            return false;
        }
        let lower = t.to_lowercase();
        const TIP_MARKERS: &[&str] = &[
            "检查",
            "尝试",
            "订阅更多",
            "api key",
            "api",
            "配置",
            "gemini",
            "更换关键词",
            "请提供",
            "未返回",
            "联网",
        ];
        !TIP_MARKERS.iter().any(|m| lower.contains(m))
    }

    /// replan 提示：用建议值重试 brew，而不是 webSearch
    fn suggestion_retry_hint(&self, values: &[String], capability_ids: &[String]) -> String {
        let joined = values.join(" / ");
        let cap = capability_ids
            .iter()
            .find(|id| id.starts_with("brew."))
            .map(|s| s.as_str())
            .unwrap_or("brew.items");
        format!(
            "前次 {cap} 返回 notFound 且已有相近建议；请用同一 brew 能力重试，参数 sourceName/name/query/author 设为建议值之一：{joined}。禁止改用 ai.webSearch / ai.groundingSearch"
        )
    }

    /// 本地数据域能力：brew.* / platform.* / search.fuzzy 等
    pub fn is_local_data_capability(id: &str) -> bool {
        id.starts_with("brew.")
            || id.starts_with("platform.")
            || id == "search.fuzzy"
            || id == "fuzzy.search"
            || id.starts_with("config.get")
            || id == "library.read"
            || id == "library.search"
    }

    fn is_local_data_domain(&self, capability_ids: &[String]) -> bool {
        if capability_ids.is_empty() {
            return false;
        }
        // 全部为本地数据域，或仅混入 summarize/analyze 类后处理
        capability_ids.iter().all(|id| {
            Self::is_local_data_capability(id)
                || id == "ai.summarize"
                || id == "ai.analyze"
                || id == "ai.extract"
                || id.starts_with("router.")
                || id.starts_with("ui.")
        }) && capability_ids
            .iter()
            .any(|id| Self::is_local_data_capability(id))
    }

    /// 本地替代方案提示（replan 优先 brew.page / search.fuzzy / brew.items）
    fn local_data_hints(&self, capability_ids: &[String]) -> Vec<String> {
        let mut hints = Vec::new();
        let has_brew = capability_ids
            .iter()
            .any(|c| c.starts_with("brew."))
            || capability_ids.is_empty();

        if has_brew {
            hints.push(
                "优先改用 brew.page / brew.items（放宽 limit、去掉过严 filter）或 search.fuzzy 做本地检索，禁止使用 ai.webSearch / ai.groundingSearch"
                    .to_string(),
            );
            hints.push(
                "若订阅源或文章为空，请向用户澄清、列出相近建议，或引导 brew.discover / 订阅，而不是联网搜索公开网页"
                    .to_string(),
            );
        }

        if capability_ids
            .iter()
            .any(|c| c == "search.fuzzy" || c == "fuzzy.search" || c.starts_with("platform."))
        {
            hints.push(
                "本地缓存/模糊搜索无结果时，请放宽关键词、列出相近建议或向用户澄清，不要改用联网搜索"
                    .to_string(),
            );
        }

        if hints.is_empty() {
            hints.push("数据不足，请向用户澄清需求或调整查询参数，不要盲目联网搜索".to_string());
        }
        hints
    }

    /// stub / 空实现标记
    pub fn has_stub_markers(&self, result: &Value) -> bool {
        let text = self.extract_all_text(result).to_lowercase();
        const STUB_MARKERS: &[&str] = &[
            "require database integration",
            "requires database integration",
            "need database integration",
            "not yet implemented",
            "not implemented",
            "todo: implement",
            "stub implementation",
            "placeholder",
            "尚未实现",
            "暂未实现",
            "需要数据库集成",
            "简化实现",
        ];
        STUB_MARKERS.iter().any(|m| text.contains(m))
    }

    /// 检测各种失败模式
    fn detect_failure_patterns(&self, eval: &mut Evaluation, result: &Value) {
        // 0. stub / 空实现（hollow success）
        if self.has_stub_markers(result) {
            eval.failure_patterns
                .push(FailurePattern::StubOrHollowSuccess);
        }

        // 1. 检测空数据源
        if self.is_data_source_empty(result) {
            eval.failure_patterns.push(FailurePattern::EmptyDataSource);
        }

        // 2. 检测"未找到"语义
        if self.has_not_found_semantic(result) {
            eval.failure_patterns.push(FailurePattern::NotFoundSemantic);
        }

        // 3. 检测"无匹配"语义
        if self.has_no_match_semantic(result) {
            eval.failure_patterns.push(FailurePattern::NoMatchSemantic);
        }

        // 4. 检测 AI 总结了"没有数据"
        if self.is_summarized_nothing(result) {
            eval.failure_patterns
                .push(FailurePattern::SummarizedNothing);
        }

        // 5. 检测显式的 count=0
        if self.has_zero_count(result) {
            eval.failure_patterns.push(FailurePattern::ZeroCount);
        }
    }

    /// 检测数据源是否为空
    fn is_data_source_empty(&self, result: &Value) -> bool {
        if let Value::Object(obj) = result {
            // 检查 items 数组
            if let Some(Value::Array(arr)) = obj.get("items") {
                if arr.is_empty() {
                    return true;
                }
            }

            // 检查 results 数组
            if let Some(Value::Array(arr)) = obj.get("results") {
                if arr.is_empty() {
                    return true;
                }
            }

            // 检查 data 数组
            if let Some(Value::Array(arr)) = obj.get("data") {
                if arr.is_empty() {
                    return true;
                }
            }

            // 检查 sources 数组（brew.sources）
            if let Some(Value::Array(arr)) = obj.get("sources") {
                if arr.is_empty() {
                    return true;
                }
            }

            // brew.generateReadingList
            if let Some(Value::Array(arr)) = obj.get("readingList") {
                if arr.is_empty() {
                    return true;
                }
            }
        }

        matches!(result, Value::Array(arr) if arr.is_empty())
    }

    /// 检测"未找到"语义
    fn has_not_found_semantic(&self, result: &Value) -> bool {
        // 检查 notFound 标志
        if let Value::Object(obj) = result {
            if let Some(Value::Bool(true)) = obj.get("notFound") {
                return true;
            }
            if let Some(Value::Bool(true)) = obj.get("not_found") {
                return true;
            }
        }

        // 在文本中检测"未找到"模式
        let text = self.extract_all_text(result).to_lowercase();
        let not_found_patterns = [
            "未找到",
            "没有找到",
            "无结果",
            "不存在",
            "找不到",
            "not found",
            "no results",
            "no data",
            "nothing found",
            "見つかりません",
            "見つからない",
            "結果なし",
            "検索結果は0件",
        ];

        not_found_patterns.iter().any(|p| text.contains(p))
    }

    /// 检测"无匹配"语义
    fn has_no_match_semantic(&self, result: &Value) -> bool {
        if let Value::Object(obj) = result {
            // 检查 ambiguous 或 noMatch 标志
            if let Some(Value::Bool(true)) = obj.get("ambiguous") {
                return true;
            }
            if let Some(Value::Bool(true)) = obj.get("noMatch") {
                return true;
            }
        }

        let text = self.extract_all_text(result).to_lowercase();
        let no_match_patterns = [
            "没有匹配",
            "无法匹配",
            "不匹配",
            "搜索结果具有歧义",
            "未命中",
        ];

        no_match_patterns.iter().any(|p| text.contains(p))
    }

    /// 检测 AI 是否总结了"没有数据"
    fn is_summarized_nothing(&self, result: &Value) -> bool {
        if let Value::Object(obj) = result {
            // 获取 summary 文本
            let summary = obj
                .get("summary")
                .and_then(|v| v.as_str())
                .or_else(|| obj.get("aiSummary").and_then(|v| v.as_str()))
                .unwrap_or("");

            if summary.is_empty() {
                return false;
            }

            let summary_lower = summary.to_lowercase();

            // 检测总结中的"无数据"模式
            let nothing_patterns = [
                "未找到",
                "没有找到",
                "无结果",
                "结果为0",
                "结果总数为0",
                "0条",
                "零条",
                "not found",
                "no results",
                "empty",
                "notfound: true",
                "`notfound: true`",
                "notfound",
            ];

            if nothing_patterns.iter().any(|p| summary_lower.contains(p)) {
                return true;
            }

            // 检测总结是否主要在描述"没有内容"
            let negative_indicators = [
                "但未找到",
                "但没有",
                "搜索无结果",
                "没有相关",
                "未能找到",
                "无法找到",
            ];

            // 如果总结中有多个负面指标，认为是在总结"没有数据"
            let negative_count = negative_indicators
                .iter()
                .filter(|p| summary_lower.contains(*p))
                .count();

            negative_count >= 1
        } else {
            false
        }
    }

    /// 检测显式的 count=0
    fn has_zero_count(&self, result: &Value) -> bool {
        if let Value::Object(obj) = result {
            // 检查 total
            if let Some(total) = obj.get("total") {
                if let Some(n) = total.as_i64() {
                    if n == 0 {
                        return true;
                    }
                }
            }

            // 检查 count
            if let Some(count) = obj.get("count") {
                if let Some(n) = count.as_i64() {
                    if n == 0 {
                        return true;
                    }
                }
            }

            // 检查 resultCount / totalMatched
            if let Some(count) = obj.get("resultCount").or_else(|| obj.get("totalMatched")) {
                if let Some(n) = count.as_i64() {
                    if n == 0 {
                        return true;
                    }
                }
            }
        }

        false
    }

    /// 统计实际的数据条数
    fn count_actual_data(&self, result: &Value) -> usize {
        match result {
            Value::Array(arr) => arr.len(),
            Value::Object(obj) => {
                // 优先检查数组类型的字段
                for key in &[
                    "items",
                    "results",
                    "data",
                    "records",
                    "entries",
                    "sources",
                    "readingList",
                ] {
                    if let Some(Value::Array(arr)) = obj.get(*key) {
                        return arr.len();
                    }
                }

                // 检查 total/count/totalMatched 字段
                if let Some(n) = obj
                    .get("total")
                    .or_else(|| obj.get("count"))
                    .or_else(|| obj.get("totalMatched"))
                    .and_then(|v| v.as_u64())
                {
                    return n as usize;
                }

                // 如果是包含内容的对象，视为 1 条数据
                // 但要排除只有 summary 的情况（因为 summary 可能是对"空数据"的总结）
                let has_only_summary =
                    obj.len() <= 2 && (obj.contains_key("summary") || obj.contains_key("style"));

                if has_only_summary {
                    0
                } else if !obj.is_empty() {
                    1
                } else {
                    0
                }
            }
            Value::String(s) if !s.is_empty() => 1,
            _ => 0,
        }
    }

    /// 描述失败模式
    fn describe_failure_patterns(&self, patterns: &[FailurePattern]) -> String {
        let descriptions: Vec<&str> = patterns
            .iter()
            .map(|p| match p {
                FailurePattern::EmptyDataSource => "数据源返回空结果",
                FailurePattern::NotFoundSemantic => "检测到「未找到」标记",
                FailurePattern::NoMatchSemantic => "检测到「无匹配」标记",
                FailurePattern::IrrelevantResult => "结果与查询不相关",
                FailurePattern::SummarizedNothing => "AI 总结显示没有有效数据",
                FailurePattern::ZeroCount => "结果数量为 0",
                FailurePattern::StubOrHollowSuccess => "检测到 stub/空实现（数据路径未接通）",
            })
            .collect();

        descriptions.join("；")
    }

    /// 通用目标评估
    fn evaluate_generic_goal(&self, eval: &mut Evaluation, result: &Value) {
        // 简单检查：是否有实质内容
        let data_count = self.count_actual_data(result);
        if data_count == 0 {
            eval.is_satisfied = false;
            eval.satisfaction_score = 0.0;
            eval.reason = Some("没有返回有效数据".to_string());
        }
    }

    /// 提取所有文本内容
    fn extract_all_text(&self, result: &Value) -> String {
        match result {
            Value::String(s) => s.clone(),
            Value::Array(arr) => arr
                .iter()
                .map(|v| self.extract_all_text(v))
                .collect::<Vec<_>>()
                .join(" "),
            Value::Object(obj) => {
                let mut texts = Vec::new();

                // 提取所有字符串值
                for (_key, value) in obj {
                    match value {
                        Value::String(s) => texts.push(s.clone()),
                        Value::Array(_) | Value::Object(_) => {
                            texts.push(self.extract_all_text(value));
                        }
                        _ => {}
                    }
                }

                texts.join(" ")
            }
            _ => String::new(),
        }
    }
}

impl Default for ResultEvaluator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_detect_empty_data_source() {
        let evaluator = ResultEvaluator::new();
        let result = json!({
            "items": [],
            "total": 0
        });

        let eval = evaluator.evaluate_result(&result);

        assert!(!eval.is_satisfied);
        assert!(eval
            .failure_patterns
            .contains(&FailurePattern::EmptyDataSource));
        // 无能力上下文时仍可建议联网（非本地域默认）
        assert!(eval.suggests_web_search);
    }

    #[test]
    fn test_detect_not_found_semantic() {
        let evaluator = ResultEvaluator::new();
        let result = json!({
            "notFound": true,
            "message": "未找到相关内容"
        });

        let eval = evaluator.evaluate_result(&result);

        assert!(!eval.is_satisfied);
        assert!(eval
            .failure_patterns
            .contains(&FailurePattern::NotFoundSemantic));
    }

    #[test]
    fn test_detect_summarized_nothing() {
        let evaluator = ResultEvaluator::new();

        // 这是用户遇到的实际情况：AI 总结了"没有找到"
        let result = json!({
            "summary": "搜索关键词「政治」，但未找到匹配的订阅源或作者（notFound: true，结果总数为0）。系统提示搜索结果具有歧义或未命中，明确指出未找到名为\"政治\"的相关内容。",
            "style": "brief"
        });

        let eval = evaluator.evaluate_result(&result);

        assert!(!eval.is_satisfied, "应该检测到总结了空数据");
        assert!(
            eval.failure_patterns
                .contains(&FailurePattern::SummarizedNothing),
            "应该包含 SummarizedNothing 模式: {:?}",
            eval.failure_patterns
        );
        assert!(eval.suggests_web_search, "无本地域上下文时应建议联网搜索");
    }

    #[test]
    fn test_valid_summary_passes() {
        let evaluator = ResultEvaluator::new();
        let result = json!({
            "summary": "这是关于政治新闻的总结。最近的政治动态包括：1. 某国举行大选；2. 国际峰会召开；3. 新政策出台。这些事件对全球政治格局产生了重要影响。",
            "items": [
                {"title": "大选新闻", "content": "..."},
                {"title": "峰会报道", "content": "..."}
            ]
        });

        let eval = evaluator.evaluate_result(&result);

        assert!(eval.is_satisfied, "有效的总结应该通过验证");
        assert!(eval.failure_patterns.is_empty());
    }

    #[test]
    fn test_zero_count_detected() {
        let evaluator = ResultEvaluator::new();
        let result = json!({
            "total": 0,
            "items": []
        });

        let eval = evaluator.evaluate_result(&result);

        assert!(!eval.is_satisfied);
        assert!(
            eval.failure_patterns.contains(&FailurePattern::ZeroCount)
                || eval
                    .failure_patterns
                    .contains(&FailurePattern::EmptyDataSource)
        );
    }

    #[test]
    fn test_japanese_not_found() {
        let evaluator = ResultEvaluator::new();
        let result = json!({
            "message": "検索結果は0件です。見つかりませんでした。"
        });

        let eval = evaluator.evaluate_result(&result);

        assert!(!eval.is_satisfied);
        assert!(eval
            .failure_patterns
            .contains(&FailurePattern::NotFoundSemantic));
    }

    #[test]
    fn test_real_world_failure_case() {
        // 这是用户实际遇到的失败案例
        let evaluator = ResultEvaluator::new();
        let result = json!({
            "style": "brief",
            "summary": "这是一份搜索结果的JSON数据，主要内容如下：\n\n1.  **搜索状态**：用户搜索关键词\"政治\"，但未找到匹配的订阅源或作者（`notFound: true`，结果总数为0）。\n2.  **异常原因**：系统提示搜索结果具有歧义或未命中，明确指出未找到名为\"政治\"的相关内容。"
        });

        let eval = evaluator.evaluate_result(&result);

        assert!(!eval.is_satisfied, "这个案例应该被检测为失败");
        assert!(
            eval.failure_patterns
                .contains(&FailurePattern::SummarizedNothing)
                || eval
                    .failure_patterns
                    .contains(&FailurePattern::NotFoundSemantic),
            "应该检测到失败模式: {:?}",
            eval.failure_patterns
        );
        assert!(eval.suggests_web_search, "无本地域上下文时应建议升级到联网搜索");
    }

    // ── 门控：stub / 本地数据域不得建议 webSearch ──

    #[test]
    fn test_stub_brew_sources_does_not_suggest_web_search() {
        let evaluator = ResultEvaluator::new();
        // 与 data_read::execute_brew_sources 当前 stub 返回一致
        let result = json!({
            "sources": [],
            "total": 0,
            "message": "Brew sources require database integration"
        });

        let ctx = EvaluationContext {
            capability_ids: vec!["brew.sources".into()],
            allow_web_search: false,
        };
        let eval = evaluator.evaluate_with_context(&result, &ctx);

        assert!(!eval.is_satisfied);
        assert!(
            eval.failure_patterns
                .contains(&FailurePattern::StubOrHollowSuccess),
            "应识别 stub: {:?}",
            eval.failure_patterns
        );
        assert!(
            !eval.suggests_web_search,
            "stub 本地数据不得建议 ai.webSearch"
        );
        assert!(eval.suggests_local_alternatives);
        assert!(
            eval.improvement_hints
                .iter()
                .any(|h| h.contains("brew.page") || h.contains("search.fuzzy") || h.contains("stub")),
            "应提示本地替代: {:?}",
            eval.improvement_hints
        );
    }

    #[test]
    fn test_stub_markers_gate_web_even_without_capability_ids() {
        let evaluator = ResultEvaluator::new();
        let result = json!({
            "total": 0,
            "message": "RSSHub instances require database integration"
        });
        let eval = evaluator.evaluate_result(&result);

        assert!(!eval.is_satisfied);
        assert!(!eval.suggests_web_search, "stub 标记本身应阻断 webSearch");
        assert!(eval.suggests_local_alternatives);
    }

    #[test]
    fn test_local_brew_empty_does_not_suggest_web_search() {
        let evaluator = ResultEvaluator::new();
        let result = json!({
            "items": [],
            "total": 0,
            "notFound": true,
            "message": "未找到相关文章"
        });
        let ctx = EvaluationContext {
            capability_ids: vec!["brew.items".into()],
            allow_web_search: false,
        };
        let eval = evaluator.evaluate_with_context(&result, &ctx);

        assert!(!eval.is_satisfied);
        assert!(!eval.suggests_web_search);
        assert!(eval.suggests_local_alternatives);
        assert!(eval.suggests_expand_scope);
        let joined = eval.improvement_hints.join(" ");
        assert!(
            joined.contains("brew.page") || joined.contains("brew.items") || joined.contains("search.fuzzy"),
            "replan 应优先本地 brew/search: {:?}",
            eval.improvement_hints
        );
        assert!(!joined.contains("ai.webSearch") || joined.contains("禁止使用 ai.webSearch"));
    }

    #[test]
    fn test_search_fuzzy_zero_does_not_suggest_web_search() {
        let evaluator = ResultEvaluator::new();
        let result = json!({
            "results": [],
            "total": 0
        });
        let ctx = EvaluationContext {
            capability_ids: vec!["search.fuzzy".into()],
            allow_web_search: false,
        };
        let eval = evaluator.evaluate_with_context(&result, &ctx);

        assert!(!eval.is_satisfied);
        assert!(!eval.suggests_web_search);
        assert!(eval.suggests_local_alternatives);
    }

    #[test]
    fn test_brew_plus_summarize_still_local_domain() {
        let evaluator = ResultEvaluator::new();
        let result = json!({ "items": [], "total": 0 });
        let ctx = EvaluationContext {
            capability_ids: vec!["brew.items".into(), "ai.summarize".into()],
            allow_web_search: false,
        };
        let eval = evaluator.evaluate_with_context(&result, &ctx);

        assert!(!eval.suggests_web_search);
        assert!(eval.suggests_local_alternatives);
    }

    #[test]
    fn test_allow_web_search_flag_overrides_local_gate() {
        let evaluator = ResultEvaluator::new();
        let result = json!({ "items": [], "total": 0 });
        let ctx = EvaluationContext {
            capability_ids: vec!["brew.generateReadingList".into()],
            allow_web_search: true,
        };
        let eval = evaluator.evaluate_with_context(&result, &ctx);

        assert!(!eval.is_satisfied);
        assert!(
            eval.suggests_web_search,
            "显式 allow_web_search 时应允许联网"
        );
    }

    #[test]
    fn test_non_local_empty_still_suggests_web_search() {
        let evaluator = ResultEvaluator::new();
        let result = json!({ "items": [], "total": 0 });
        let ctx = EvaluationContext {
            capability_ids: vec!["external.news".into()],
            allow_web_search: false,
        };
        let eval = evaluator.evaluate_with_context(&result, &ctx);

        assert!(!eval.is_satisfied);
        assert!(eval.suggests_web_search);
    }

    #[test]
    fn test_is_local_data_capability() {
        assert!(ResultEvaluator::is_local_data_capability("brew.sources"));
        assert!(ResultEvaluator::is_local_data_capability("brew.items"));
        assert!(ResultEvaluator::is_local_data_capability("platform.read"));
        assert!(ResultEvaluator::is_local_data_capability("search.fuzzy"));
        assert!(ResultEvaluator::is_local_data_capability("brew.generateReadingList"));
        assert!(!ResultEvaluator::is_local_data_capability("ai.webSearch"));
        assert!(!ResultEvaluator::is_local_data_capability("ai.image"));
    }

    #[test]
    fn test_generate_reading_list_empty_does_not_cascade_to_web() {
        let evaluator = ResultEvaluator::new();
        let result = json!({
            "readingList": [],
            "totalMatched": 0,
            "listName": "政治",
            "criteria": "政治",
            "message": "未找到符合条件的文章",
            "suggestions": [
                "检查 Gemini API Key 是否已配置",
                "尝试更换关键词",
                "订阅更多相关的 RSS 源"
            ]
        });
        let ctx = EvaluationContext {
            capability_ids: vec!["brew.generateReadingList".into()],
            allow_web_search: false,
        };
        let eval = evaluator.evaluate_with_context(&result, &ctx);

        assert!(!eval.is_satisfied);
        assert!(
            !eval.suggests_web_search,
            "generateReadingList 空结果不得级联 webSearch: hints={:?}",
            eval.improvement_hints
        );
        assert!(eval.suggests_local_alternatives);
        // 操作提示不应被当成实体重试值
        assert!(
            eval.suggested_retry_values.is_empty(),
            "API/配置类 tips 不是 brew 实体建议: {:?}",
            eval.suggested_retry_values
        );
        let joined = eval.improvement_hints.join(" ");
        assert!(
            joined.contains("generateReadingList") || joined.contains("brew.items"),
            "应提示本地 brew 路径: {:?}",
            eval.improvement_hints
        );
    }

    #[test]
    fn test_generate_reading_list_empty_structure_without_cap_id() {
        let evaluator = ResultEvaluator::new();
        let result = json!({
            "readingList": [],
            "totalMatched": 0,
            "message": "未找到符合条件的文章"
        });
        // 无 capability 上下文时仍按 readingList 结构门控
        let eval = evaluator.evaluate_result(&result);
        assert!(!eval.is_satisfied);
        assert!(!eval.suggests_web_search);
        assert!(eval.suggests_local_alternatives);
    }

    #[test]
    fn test_not_found_with_suggestions_prefers_brew_retry() {
        let evaluator = ResultEvaluator::new();
        // 与 brew.items 未命中时返回形状一致
        let result = json!({
            "items": [],
            "total": 0,
            "notFound": true,
            "searchedFor": "天利",
            "ambiguous": true,
            "suggestions": ["天利一下", "天利博客"],
            "choices": [
                { "value": "天利一下", "label": "天利一下" },
                { "value": "天利博客", "label": "天利博客" }
            ],
            "hint": "你是不是想找: 天利一下, 天利博客?"
        });
        let ctx = EvaluationContext {
            capability_ids: vec!["brew.items".into()],
            allow_web_search: false,
        };
        let eval = evaluator.evaluate_with_context(&result, &ctx);

        assert!(!eval.is_satisfied);
        assert!(!eval.suggests_web_search, "有本地建议时禁止 webSearch");
        assert!(eval.suggests_local_alternatives);
        assert_eq!(
            eval.suggested_retry_values,
            vec!["天利一下".to_string(), "天利博客".to_string()]
        );
        let joined = eval.improvement_hints.join(" ");
        assert!(
            joined.contains("天利一下") && joined.contains("sourceName"),
            "replan 应要求用建议值重试 brew: {:?}",
            eval.improvement_hints
        );
        assert!(
            joined.contains("禁止") && joined.contains("webSearch"),
            "必须明确禁止 webSearch: {:?}",
            eval.improvement_hints
        );
    }

    #[test]
    fn test_not_found_suggestions_override_allow_web_when_entity_present() {
        // 即便 allow_web_search=true，本地 notFound+实体建议仍优先 brew 重试（不烧 key）
        let evaluator = ResultEvaluator::new();
        let result = json!({
            "items": [],
            "total": 0,
            "notFound": true,
            "suggestions": ["MyFeed"],
            "choices": [{ "value": "MyFeed", "label": "MyFeed" }]
        });
        let ctx = EvaluationContext {
            capability_ids: vec!["brew.items".into()],
            allow_web_search: true,
        };
        let eval = evaluator.evaluate_with_context(&result, &ctx);

        assert!(!eval.suggests_web_search);
        assert_eq!(eval.suggested_retry_values, vec!["MyFeed".to_string()]);
    }

    #[test]
    fn test_extract_suggested_retry_values_filters_tips() {
        let evaluator = ResultEvaluator::new();
        let result = json!({
            "suggestions": [
                "检查 Gemini API Key 是否已配置",
                "zhilu",
                "尝试更换关键词"
            ]
        });
        let values = evaluator.extract_suggested_retry_values(&result);
        assert_eq!(values, vec!["zhilu".to_string()]);
    }
}
