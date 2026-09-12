//! Recipe 步骤校验与转换
//!
//! 提供 `validate_and_convert_steps`：校验 AI/Planner 生成的步骤并转换为 RecipeStep。

use super::tier_router::TierRouter;
use super::types::*;
use crate::config::ModelTier;
/// 步骤数量上限。提示词、Planner schema 和这里的截断必须是同一个数。
use myriad_agent_rules::MAX_PLAN_STEPS as MAX_STEPS;
use std::collections::HashMap;

/// 根据 capability_id 建议 model_tier（复用 TierRouter 逻辑）
/// 不使用 LLM 的能力返回 None，不参与 tier 标注
fn suggest_tier(capability_id: &str) -> Option<ModelTier> {
    if !TierRouter::requires_llm(capability_id) {
        return None;
    }
    Some(TierRouter::resolve_with_override(capability_id, None))
}

/// 校验 AI 生成的步骤并转换为 RecipeStep（供 Planner 复用）
pub fn validate_and_convert_steps(
    ai_steps: Vec<AiRecipeStep>,
    reasoning: Option<String>,
    cap_schemas: &[Capability],
) -> Result<Vec<RecipeStep>, String> {
    if ai_steps.is_empty() {
        return Err("AI generated zero steps".to_string());
    }

    if ai_steps.len() > MAX_STEPS {
        tracing::warn!(
            count = ai_steps.len(),
            max = MAX_STEPS,
            "[validate_and_convert] Too many steps, truncating"
        );
    }
    let ai_steps: Vec<AiRecipeStep> = ai_steps.into_iter().take(MAX_STEPS).collect();

    // Skill 去重：同一个 skill:xxx 只保留第一次出现，后续合并为 variations 参数
    let ai_steps = {
        let mut seen_skills: HashMap<String, usize> = HashMap::new();
        let mut deduped: Vec<AiRecipeStep> = Vec::new();
        for step in ai_steps {
            if step.capability_id.starts_with("skill:") {
                if let Some(&first_idx) = seen_skills.get(&step.capability_id) {
                    // 合并到第一个同 skill 步骤：将此步骤的 action 追加到 variations
                    tracing::warn!(
                        capability_id = %step.capability_id,
                        duplicate_id = %step.id,
                        merged_into = %deduped[first_idx].id,
                        "[validate_and_convert] Merging duplicate skill call into first occurrence"
                    );
                    let first = &mut deduped[first_idx];
                    let variation = serde_json::Value::String(step.action.clone());
                    match first.params.get_mut("variations") {
                        Some(v) if v.is_array() => {
                            v.as_array_mut().unwrap().push(variation);
                        }
                        _ => {
                            // 将原始 action 也加入 variations
                            let original = serde_json::Value::String(first.action.clone());
                            first.params.insert(
                                "variations".to_string(),
                                serde_json::Value::Array(vec![original, variation]),
                            );
                        }
                    }
                    // 增加 count
                    let current_count = first
                        .params
                        .get("count")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(1);
                    first
                        .params
                        .insert("count".to_string(), serde_json::json!(current_count + 1));
                } else {
                    seen_skills.insert(step.capability_id.clone(), deduped.len());
                    deduped.push(step);
                }
            } else {
                deduped.push(step);
            }
        }
        deduped
    };

    let cap_ids: std::collections::HashSet<&str> =
        cap_schemas.iter().map(|c| c.id.as_str()).collect();
    let step_ids: std::collections::HashSet<String> =
        ai_steps.iter().map(|s| s.id.clone()).collect();

    // step_id 唯一性检查
    if step_ids.len() != ai_steps.len() {
        return Err("Duplicate step IDs detected".to_string());
    }

    let mut steps = Vec::new();

    for (idx, ai_step) in ai_steps.into_iter().enumerate() {
        // capability_id 验证（允许 skill: 和 mcp. 前缀通过）
        let is_skill = ai_step.capability_id.starts_with("skill:");
        let is_mcp = ai_step.capability_id.starts_with("mcp.");
        if !is_skill && !is_mcp && !cap_ids.contains(ai_step.capability_id.as_str()) {
            tracing::warn!(
                capability_id = %ai_step.capability_id,
                "[validate_and_convert] Unknown capability_id"
            );
            return Err(format!("Unknown capability_id: {}", ai_step.capability_id));
        }

        // depends_on 引用验证
        for dep in &ai_step.depends_on {
            if !step_ids.contains(dep) {
                return Err(format!(
                    "Step '{}' depends on non-existent step '{}'",
                    ai_step.id, dep
                ));
            }
        }

        // Schema param check: warn only; skipped when capability_id is not in cap_schemas.
        if let Some(cap) = cap_schemas.iter().find(|c| c.id == ai_step.capability_id) {
            // 检查 required params 是否存在
            if let Some(required) = cap.input_schema.get("required").and_then(|v| v.as_array()) {
                for req_val in required {
                    if let Some(req_name) = req_val.as_str() {
                        let has_direct = ai_step.params.contains_key(req_name);
                        let has_from = ai_step.params.contains_key(&format!("{}From", req_name));
                        if !has_direct && !has_from {
                            tracing::warn!(
                                capability = %ai_step.capability_id,
                                missing_param = %req_name,
                                "[validate_and_convert] Missing required parameter"
                            );
                        }
                    }
                }
            }
            // 检查多余参数
            if let Some(props) = cap.input_schema.get("properties") {
                for key in ai_step.params.keys() {
                    if key.ends_with("From") {
                        continue;
                    }
                    if props.get(key).is_none() {
                        tracing::warn!(
                            capability = %ai_step.capability_id,
                            param = %key,
                            "[validate_and_convert] Unknown param (not in schema)"
                        );
                    }
                }
            }
        }

        // timeout_ms 范围钳制
        let tier = suggest_tier(&ai_step.capability_id);
        let mut recipe_step = ai_step.into_recipe_step(idx as u32, tier);
        if let Some(timeout) = recipe_step.timeout_ms {
            recipe_step.timeout_ms = Some(timeout.clamp(1_000, 120_000));
        }
        steps.push(recipe_step);
    }

    // xxxFrom → depends_on 自动推断：扫描 params 中的 xxxFrom 引用，补全缺失的 depends_on
    {
        // 收集所有有效 step_id
        let valid_ids: std::collections::HashSet<String> =
            steps.iter().map(|s| s.id.clone()).collect();

        for step in &mut steps {
            for (key, value) in &step.params {
                if !key.ends_with("From") {
                    continue;
                }
                // xxxFrom 的值可能是 "step_id" 或 "step_id.field"
                let ref_str = match value.as_str() {
                    Some(s) => s,
                    None => continue,
                };
                let dep_id = ref_str.split('.').next().unwrap_or("");
                if dep_id.is_empty() || dep_id == step.id {
                    continue;
                }
                if valid_ids.contains(dep_id) {
                    if !step.depends_on.contains(&dep_id.to_string()) {
                        tracing::info!(
                            step_id = %step.id,
                            param = %key,
                            inferred_dep = %dep_id,
                            "[validate_and_convert] Auto-inferred depends_on from xxxFrom reference"
                        );
                        step.depends_on.push(dep_id.to_string());
                    }
                } else {
                    tracing::warn!(
                        step_id = %step.id,
                        param = %key,
                        ref_id = %dep_id,
                        "[validate_and_convert] xxxFrom references non-existent step, will resolve to null at runtime"
                    );
                }
            }
        }
    }

    // DAG 环检测
    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    for step in &steps {
        in_degree.entry(step.id.as_str()).or_insert(0);
        for dep in &step.depends_on {
            in_degree.entry(dep.as_str()).or_insert(0); // 确保依赖也在 map 里
            *in_degree.entry(step.id.as_str()).or_insert(0) += 1;
        }
    }
    let mut queue: Vec<&str> = in_degree
        .iter()
        .filter(|(_, &deg)| deg == 0)
        .map(|(&id, _)| id)
        .collect();
    let mut visited = 0;
    while let Some(node) = queue.pop() {
        visited += 1;
        for step in &steps {
            if step.depends_on.iter().any(|d| d == node) {
                let deg = in_degree.get_mut(step.id.as_str()).unwrap();
                *deg -= 1;
                if *deg == 0 {
                    queue.push(step.id.as_str());
                }
            }
        }
    }
    if visited < steps.len() {
        return Err("Circular dependency detected in AI-generated steps".to_string());
    }

    if let Some(reasoning) = &reasoning {
        tracing::info!(reasoning = %reasoning, "[validate_and_convert] Recipe reasoning");
    }

    Ok(steps)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_cap(id: &str) -> Capability {
        Capability {
            id: id.to_string(),
            name: id.to_string(),
            description: String::new(),
            category: CapabilityCategory::AiProcess,
            supported_actions: vec![],
            input_schema: json!({"type": "object", "properties": {}, "required": []}),
            output_schema: json!({}),
            required_permissions: vec![],
            requires_ai: false,
            estimated_duration_ms: None,
            ..Default::default()
        }
    }

    fn make_ai_step(id: &str, cap: &str, params: serde_json::Value) -> AiRecipeStep {
        AiRecipeStep {
            id: id.to_string(),
            capability_id: cap.to_string(),
            action: "test".to_string(),
            params: params
                .as_object()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .collect(),
            depends_on: vec![],
            on_failure: "abort".to_string(),
            retry: None,
            timeout_ms: None,
        }
    }

    #[test]
    fn test_basic_validation() {
        let caps = vec![make_cap("ai.image")];
        let steps = vec![make_ai_step("s1", "ai.image", json!({"prompt": "cat"}))];
        let result = validate_and_convert_steps(steps, None, &caps);
        assert!(result.is_ok());
        let steps = result.unwrap();
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].id, "s1");
    }

    #[test]
    fn test_xxxfrom_auto_depends_on() {
        let caps = vec![make_cap("ai.image"), make_cap("ai.analyze")];
        let steps = vec![
            make_ai_step("s1", "ai.image", json!({"prompt": "cat"})),
            make_ai_step("s2", "ai.analyze", json!({"dataFrom": "s1"})),
        ];
        let result = validate_and_convert_steps(steps, None, &caps).unwrap();
        assert!(
            result[1].depends_on.contains(&"s1".to_string()),
            "s2 should auto-depend on s1 via dataFrom"
        );
    }

    #[test]
    fn test_xxxfrom_invalid_reference_warns() {
        let caps = vec![make_cap("ai.analyze")];
        let steps = vec![make_ai_step(
            "s1",
            "ai.analyze",
            json!({"dataFrom": "nonexistent"}),
        )];
        let result = validate_and_convert_steps(steps, None, &caps).unwrap();
        assert!(
            result[0].depends_on.is_empty(),
            "invalid xxxFrom should not add depends_on"
        );
    }

    #[test]
    fn test_circular_dependency_detection() {
        let caps = vec![make_cap("ai.image")];
        let mut s1 = make_ai_step("s1", "ai.image", json!({}));
        let mut s2 = make_ai_step("s2", "ai.image", json!({}));
        s1.depends_on = vec!["s2".to_string()];
        s2.depends_on = vec!["s1".to_string()];
        let result = validate_and_convert_steps(vec![s1, s2], None, &caps);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Circular dependency"));
    }

    #[test]
    fn test_skill_deduplication() {
        let caps = vec![make_cap("skill:img_gen")];
        let s1 = make_ai_step("s1", "skill:img_gen", json!({}));
        let s2 = make_ai_step("s2", "skill:img_gen", json!({}));
        let result = validate_and_convert_steps(vec![s1, s2], None, &caps).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].params.contains_key("variations"));
    }

    #[test]
    fn test_duplicate_step_id() {
        let caps = vec![make_cap("ai.image")];
        let s1 = make_ai_step("same_id", "ai.image", json!({}));
        let s2 = make_ai_step("same_id", "ai.image", json!({}));
        let result = validate_and_convert_steps(vec![s1, s2], None, &caps);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Duplicate step ID"));
    }
}
