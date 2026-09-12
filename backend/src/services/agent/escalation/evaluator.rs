//! 结果评估器
//!
//! Evaluate structured result flags and counts; result text is never a control signal.
//!
//! 1. **失败标志**：读取能力返回的 notFound / noMatch 标志
//! 2. **数据源验证**：检查数据获取步骤是否返回了有效数据
//! 3. **升级门控**：本地域默认禁止联网，仅 allow_web_search 才建议

use serde_json::Value;

/// 评估上下文（用于门控联网搜索等升级建议）
#[derive(Debug, Clone, Default)]
pub struct EvaluationContext {
    /// 本次执行涉及的能力 ID 列表
    pub capability_ids: Vec<String>,
    /// 显式允许联网搜索升级
    pub allow_web_search: bool,
}

/// 评估结果
#[derive(Debug, Clone)]
pub struct Evaluation {
    /// 结果是否满足目标
    pub is_satisfied: bool,
    /// 满意度分数 (0.0 - 1.0)
    pub satisfaction_score: f32,
    /// 不满意的原因
    pub reason: Option<String>,
    /// 建议尝试联网搜索
    pub suggests_web_search: bool,
    /// 建议优先使用本地能力（brew.page / search.fuzzy 等）
    pub suggests_local_alternatives: bool,
    /// 从 choices/suggestions 抽出的可重试实体名（源名/作者等）
    pub suggested_retry_values: Vec<String>,
    /// 建议的改进方向
    pub improvement_hints: Vec<String>,
    /// 检测到的失败模式
    pub failure_patterns: Vec<FailurePattern>,
}

/// 失败模式类型
#[derive(Debug, Clone, PartialEq)]
pub enum FailurePattern {
    /// 数据源返回空结果
    EmptyDataSource,
    /// 检测到"未找到"标志
    NotFound,
    /// 检测到"无匹配"标志
    NoMatch,
    /// 数据数量为 0
    ZeroCount,
}

impl Default for Evaluation {
    fn default() -> Self {
        Self {
            is_satisfied: true,
            satisfaction_score: 1.0,
            reason: None,
            suggests_web_search: false,
            suggests_local_alternatives: false,
            suggested_retry_values: Vec::new(),
            improvement_hints: Vec::new(),
            failure_patterns: Vec::new(),
        }
    }
}

/// 生产路径优先 `evaluate_with_context`；本方法供无能力上下文的简化调用与单测。
#[cfg(test)]
pub fn evaluate_result(result: &Value) -> Evaluation {
    evaluate_with_context(result, &EvaluationContext::default())
}

/// 带上下文的结果评估（门控 webSearch 升级）
pub fn evaluate_with_context(result: &Value, ctx: &EvaluationContext) -> Evaluation {
    let result = crate::services::agent::ai_process_pure::task_inner_value(result);
    let mut eval = Evaluation::default();

    // 第一层：失败模式检测
    detect_failure_patterns(&mut eval, result);
    if !eval.failure_patterns.is_empty() {
        eval.is_satisfied = false;
        eval.satisfaction_score = 0.0;
        eval.reason = Some(describe_failure_patterns(&eval.failure_patterns));
        apply_escalation_policy(&mut eval, result, ctx);
        return eval;
    }

    // 第二层：数据源验证
    let data_count = count_actual_data(result);
    if data_count == 0 {
        eval.is_satisfied = false;
        eval.satisfaction_score = 0.1;
        eval.failure_patterns.push(FailurePattern::ZeroCount);
        eval.reason = Some("No usable data was returned".to_string());
        apply_escalation_policy(&mut eval, result, ctx);
        return eval;
    }

    eval
}

/// 按本地域 / allow_web_search 决定是否建议联网搜索
fn apply_escalation_policy(eval: &mut Evaluation, result: &Value, ctx: &EvaluationContext) {
    // 提取可重试实体建议（无论是否本地域，供 replan 使用）
    eval.suggested_retry_values = extract_suggested_retry_values(result);

    let is_local = is_local_data_domain(&ctx.capability_ids);
    // generateReadingList 空结果默认本地域：禁止空→web cascade（除非 allow_web_search）
    let is_reading_list_empty = is_generate_reading_list_empty(result, &ctx.capability_ids);

    // 已有实体建议：优先用建议值重试 brew，绝不 webSearch
    if !eval.suggested_retry_values.is_empty()
        && (is_local
            || ctx.capability_ids.iter().any(|id| id.starts_with("brew."))
            || ctx.capability_ids.is_empty())
    {
        eval.suggests_web_search = false;
        eval.suggests_local_alternatives = true;
        eval.improvement_hints.push(suggestion_retry_hint(
            &eval.suggested_retry_values,
            &ctx.capability_ids,
        ));
        eval.improvement_hints
            .extend(local_data_hints(&ctx.capability_ids));
        return;
    }

    if (is_local || is_reading_list_empty) && !ctx.allow_web_search {
        eval.suggests_web_search = false;
        eval.suggests_local_alternatives = true;
        if is_reading_list_empty {
            eval.improvement_hints.push(
                "brew.generateReadingList had no local match. Widen keyword/daysBack, use brew.items / search.fuzzy, or subscribe to more feeds. Do not cascade an empty result to ai.webSearch unless allowWebSearch=true."
                    .to_string(),
            );
        }
        eval.improvement_hints
            .extend(local_data_hints(&ctx.capability_ids));
        return;
    }

    // 非本地域，或显式允许联网
    eval.suggests_web_search = true;
    eval.improvement_hints
        .push("Not enough local data. Try a web search.".to_string());
}

/// generateReadingList 空阅读列表（含无能力上下文但结构匹配）
fn is_generate_reading_list_empty(result: &Value, capability_ids: &[String]) -> bool {
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

/// 从 choices/suggestions 提取可重试实体名（源名、作者等）
pub fn extract_suggested_retry_values(result: &Value) -> Vec<String> {
    let Some(obj) = result.as_object() else {
        return Vec::new();
    };

    let mut values = Vec::new();

    // choices[].value 优先（结构化）
    if let Some(choices) = obj.get("choices").and_then(|v| v.as_array()) {
        for c in choices {
            if let Some(v) = c.get("value").and_then(|v| v.as_str()) {
                let v = v.trim();
                if !v.is_empty() && is_entity_suggestion(v) {
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
                    && is_entity_suggestion(s)
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
fn suggestion_retry_hint(values: &[String], capability_ids: &[String]) -> String {
    let joined = values.join(" / ");
    let cap = capability_ids
        .iter()
        .find(|id| id.starts_with("brew."))
        .map(|s| s.as_str())
        .unwrap_or("brew.items");
    format!(
        "Previous {cap} returned notFound and already has close suggestions. Retry the same brew capability with sourceName/name/query/author set to one of: {joined}. Do not switch to ai.webSearch / ai.groundingSearch."
    )
}

/// 本地数据域能力：brew.* / platform.* / search.fuzzy 等
pub fn is_local_data_capability(id: &str) -> bool {
    id.starts_with("brew.")
        || id.starts_with("platform.")
        || id == "search.fuzzy"
        || id.starts_with("config.get")
        || id == "library.read"
        || id == "library.search"
}

fn is_local_data_domain(capability_ids: &[String]) -> bool {
    if capability_ids.is_empty() {
        return false;
    }
    // 全部为本地数据域，或仅混入 summarize/analyze 类后处理
    capability_ids.iter().all(|id| {
        is_local_data_capability(id)
            || id == "ai.summarize"
            || id == "ai.analyze"
            || id == "ai.extract"
            || id.starts_with("router.")
            || id.starts_with("ui.")
    }) && capability_ids.iter().any(|id| is_local_data_capability(id))
}

/// 本地替代方案提示（replan 优先 brew.page / search.fuzzy / brew.items）
fn local_data_hints(capability_ids: &[String]) -> Vec<String> {
    let mut hints = Vec::new();
    let has_brew =
        capability_ids.iter().any(|c| c.starts_with("brew.")) || capability_ids.is_empty();

    if has_brew {
        hints.push(
            "Prefer brew.page / brew.items (widen limit, drop strict filters) or search.fuzzy for local lookup. Do not use ai.webSearch / ai.groundingSearch."
                .to_string(),
        );
        hints.push(
            "If feeds or articles are empty, ask the user, list close matches, or use brew.discover / subscribe. Do not search the public web."
                .to_string(),
        );
    }

    if capability_ids
        .iter()
        .any(|c| c == "search.fuzzy" || c.starts_with("platform."))
    {
        hints.push(
            "Local cache or fuzzy search returned nothing. Widen the keyword, list close matches, or ask the user. Do not switch to a web search."
                .to_string(),
        );
    }

    if hints.is_empty() {
        hints.push(
            "Not enough data. Ask the user or adjust the query. Do not web-search blindly."
                .to_string(),
        );
    }
    hints
}

/// 检测各种失败模式
fn detect_failure_patterns(eval: &mut Evaluation, result: &Value) {
    // 1. 检测空数据源
    if is_data_source_empty(result) {
        eval.failure_patterns.push(FailurePattern::EmptyDataSource);
    }

    // 2. 检测"未找到"标志
    if has_not_found_marker(result) {
        eval.failure_patterns.push(FailurePattern::NotFound);
    }

    // 3. 检测"无匹配"标志
    if has_no_match_marker(result) {
        eval.failure_patterns.push(FailurePattern::NoMatch);
    }

    // 5. 检测显式的 count=0
    if has_zero_count(result) {
        eval.failure_patterns.push(FailurePattern::ZeroCount);
    }
}

/// 检测数据源是否为空
fn is_data_source_empty(result: &Value) -> bool {
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

/// 检测"未找到"标志
fn has_not_found_marker(result: &Value) -> bool {
    // 检查 notFound 标志
    if let Value::Object(obj) = result {
        if let Some(Value::Bool(true)) = obj.get("notFound") {
            return true;
        }
        if let Some(Value::Bool(true)) = obj.get("not_found") {
            return true;
        }
    }

    false
}

/// 检测"无匹配"标志
fn has_no_match_marker(result: &Value) -> bool {
    if let Value::Object(obj) = result {
        // 检查 ambiguous 或 noMatch 标志
        if let Some(Value::Bool(true)) = obj.get("ambiguous") {
            return true;
        }
        if let Some(Value::Bool(true)) = obj.get("noMatch") {
            return true;
        }
    }

    false
}

/// 检测显式的 count=0
fn has_zero_count(result: &Value) -> bool {
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
fn count_actual_data(result: &Value) -> usize {
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

            usize::from(!obj.is_empty())
        }
        Value::String(s) if !s.is_empty() => 1,
        _ => 0,
    }
}

/// 描述失败模式
fn describe_failure_patterns(patterns: &[FailurePattern]) -> String {
    let descriptions: Vec<&str> = patterns
        .iter()
        .map(|p| match p {
            FailurePattern::EmptyDataSource => "data source returned empty",
            FailurePattern::NotFound => "detected a not-found marker",
            FailurePattern::NoMatch => "detected a no-match marker",
            FailurePattern::ZeroCount => "result count is 0",
        })
        .collect();

    descriptions.join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn content_words_do_not_trigger_replanning() {
        for text in [
            "How to fix 404 Not Found",
            "不存在的骑士",
            "検索結果は0件というメッセージを直す",
            "placeholder and TODO: implement examples",
            "未找到、无法匹配、简化实现都是本文介绍的错误信息",
        ] {
            for result in [
                json!({"items": [{"title": text, "content": text}], "total": 1}),
                json!({"summary": text, "style": "brief"}),
                json!({"format": "json", "value": {"summary": text}, "contextProvenance": []}),
                json!(text),
            ] {
                let eval = evaluate_result(&result);
                assert!(
                    eval.is_satisfied,
                    "content is not a failure marker: {result}"
                );
                assert!(eval.failure_patterns.is_empty());
                assert!(!eval.suggests_web_search);
            }
        }
    }

    #[test]
    fn structured_failures_are_detected_inside_task_envelopes() {
        for result in [
            json!({"notFound": true, "message": "No matching source"}),
            json!({"noMatch": true}),
            json!({"ambiguous": true}),
            json!({"items": [], "total": 0}),
        ] {
            let envelope = json!({"format": "json", "value": result, "contextProvenance": []});
            let eval = evaluate_with_context(
                &envelope,
                &EvaluationContext {
                    capability_ids: vec!["brew.items".into()],
                    allow_web_search: false,
                },
            );
            assert!(
                !eval.is_satisfied,
                "structured failure must be retained: {envelope}"
            );
            assert!(!eval.suggests_web_search);
            assert!(eval.suggests_local_alternatives);
        }
    }

    #[test]
    fn nested_article_fields_do_not_become_top_level_control_flags() {
        let result = json!({"items": [{"notFound": true, "count": 0, "title": "An API example"}], "total": 1});
        assert!(evaluate_result(&result).is_satisfied);
    }

    #[test]
    fn test_detect_empty_data_source() {
        let result = json!({
            "items": [],
            "total": 0
        });

        let eval = evaluate_result(&result);

        assert!(!eval.is_satisfied);
        assert!(eval
            .failure_patterns
            .contains(&FailurePattern::EmptyDataSource));
        // 无能力上下文时仍可建议联网（非本地域默认）
        assert!(eval.suggests_web_search);
    }

    #[test]
    fn test_valid_summary_passes() {
        let result = json!({
            "summary": "这是关于政治新闻的总结。最近的政治动态包括：1. 某国举行大选；2. 国际峰会召开；3. 新政策出台。这些事件对全球政治格局产生了重要影响。",
            "items": [
                {"title": "大选新闻", "content": "..."},
                {"title": "峰会报道", "content": "..."}
            ]
        });

        let eval = evaluate_result(&result);

        assert!(eval.is_satisfied, "有效的总结应该通过验证");
        assert!(eval.failure_patterns.is_empty());
    }

    #[test]
    fn test_zero_count_detected() {
        let result = json!({
            "total": 0,
            "items": []
        });

        let eval = evaluate_result(&result);

        assert!(!eval.is_satisfied);
        assert!(
            eval.failure_patterns.contains(&FailurePattern::ZeroCount)
                || eval
                    .failure_patterns
                    .contains(&FailurePattern::EmptyDataSource)
        );
    }

    #[test]
    fn test_local_brew_empty_does_not_suggest_web_search() {
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
        let eval = evaluate_with_context(&result, &ctx);

        assert!(!eval.is_satisfied);
        assert!(!eval.suggests_web_search);
        assert!(eval.suggests_local_alternatives);
        let joined = eval.improvement_hints.join(" ");
        assert!(
            joined.contains("brew.page")
                || joined.contains("brew.items")
                || joined.contains("search.fuzzy"),
            "replan 应优先本地 brew/search: {:?}",
            eval.improvement_hints
        );
        assert!(!joined.contains("ai.webSearch") || joined.contains("Do not use ai.webSearch"));
    }

    #[test]
    fn test_search_fuzzy_zero_does_not_suggest_web_search() {
        let result = json!({
            "results": [],
            "total": 0
        });
        let ctx = EvaluationContext {
            capability_ids: vec!["search.fuzzy".into()],
            allow_web_search: false,
        };
        let eval = evaluate_with_context(&result, &ctx);

        assert!(!eval.is_satisfied);
        assert!(!eval.suggests_web_search);
        assert!(eval.suggests_local_alternatives);
    }

    #[test]
    fn test_brew_plus_summarize_still_local_domain() {
        let result = json!({ "items": [], "total": 0 });
        let ctx = EvaluationContext {
            capability_ids: vec!["brew.items".into(), "ai.summarize".into()],
            allow_web_search: false,
        };
        let eval = evaluate_with_context(&result, &ctx);

        assert!(!eval.suggests_web_search);
        assert!(eval.suggests_local_alternatives);
    }

    #[test]
    fn test_allow_web_search_flag_overrides_local_gate() {
        let result = json!({ "items": [], "total": 0 });
        let ctx = EvaluationContext {
            capability_ids: vec!["brew.generateReadingList".into()],
            allow_web_search: true,
        };
        let eval = evaluate_with_context(&result, &ctx);

        assert!(!eval.is_satisfied);
        assert!(
            eval.suggests_web_search,
            "显式 allow_web_search 时应允许联网"
        );
    }

    #[test]
    fn test_non_local_empty_still_suggests_web_search() {
        let result = json!({ "items": [], "total": 0 });
        let ctx = EvaluationContext {
            capability_ids: vec!["external.news".into()],
            allow_web_search: false,
        };
        let eval = evaluate_with_context(&result, &ctx);

        assert!(!eval.is_satisfied);
        assert!(eval.suggests_web_search);
    }

    #[test]
    fn test_is_local_data_capability() {
        assert!(is_local_data_capability("brew.sources"));
        assert!(is_local_data_capability("brew.items"));
        assert!(is_local_data_capability("platform.read"));
        assert!(is_local_data_capability("search.fuzzy"));
        assert!(is_local_data_capability("brew.generateReadingList"));
        assert!(!is_local_data_capability("ai.webSearch"));
        assert!(!is_local_data_capability("ai.image"));
    }

    #[test]
    fn test_generate_reading_list_empty_does_not_cascade_to_web() {
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
        let eval = evaluate_with_context(&result, &ctx);

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
        let result = json!({
            "readingList": [],
            "totalMatched": 0,
            "message": "未找到符合条件的文章"
        });
        // 无 capability 上下文时仍按 readingList 结构门控
        let eval = evaluate_result(&result);
        assert!(!eval.is_satisfied);
        assert!(!eval.suggests_web_search);
        assert!(eval.suggests_local_alternatives);
    }

    #[test]
    fn test_not_found_with_suggestions_prefers_brew_retry() {
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
        let eval = evaluate_with_context(&result, &ctx);

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
            joined.contains("Do not") && joined.contains("webSearch"),
            "必须明确禁止 webSearch: {:?}",
            eval.improvement_hints
        );
    }

    #[test]
    fn test_not_found_suggestions_override_allow_web_when_entity_present() {
        // 即便 allow_web_search=true，有实体建议时仍优先 brew 重试
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
        let eval = evaluate_with_context(&result, &ctx);

        assert!(!eval.suggests_web_search);
        assert_eq!(eval.suggested_retry_values, vec!["MyFeed".to_string()]);
    }

    #[test]
    fn test_extract_suggested_retry_values_filters_tips() {
        let result = json!({
            "suggestions": [
                "检查 Gemini API Key 是否已配置",
                "zhilu",
                "尝试更换关键词"
            ]
        });
        let values = extract_suggested_retry_values(&result);
        assert_eq!(values, vec!["zhilu".to_string()]);
    }
}
