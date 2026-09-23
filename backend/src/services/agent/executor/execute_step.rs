// Executor single-step execution

use crate::services::agent::ai_process_pure::USER_TEXT_MAX_CHARS;
use crate::services::agent::capability::get_registry;
use crate::services::agent::executor_utils_pure::{
    SKILL_SUB_STEP_MIN_SECS, category_timeout_fallback_secs, step_timeout_secs,
};
use crate::services::agent::external_pure::classify_outbound_fetch;
use crate::services::agent::types::{self, *};
use myriad_agent_rules::{
    MAX_PLAN_STEPS, PLAN_DATA_FLOW_RULE, PLAN_DEPENDENCY_RULE,
    extract_json_object_from_ai_response, plan_image_size_rule, plan_step_cap_rule,
    untrusted_block,
};
use serde_json::{Value, json};
use std::collections::HashMap;

use super::Executor;
use super::executor_footer::*;
use super::handlers::HandlerContext;
use super::utils;
use super::{take_steering, truncate_str};

impl Executor {
    /// 执行单个步骤
    pub(crate) async fn execute_step(
        &self,
        step: &RecipeStep,
        context: &mut ExecutionContext,
        handler_ctx: &HandlerContext<'_>,
    ) -> Result<Value, String> {
        if let Some(task_id) = handler_ctx.task_id.as_deref() {
            let steering = take_steering(handler_ctx.db, task_id).await;
            if !steering.is_empty() {
                let combined = steering.join("\n");
                context.variables.insert(
                    "_steering_instruction".to_string(),
                    Value::String(combined.clone()),
                );
                context.user_intent = if context.user_intent.is_empty() {
                    combined.clone()
                } else {
                    format!("{}\nSteering: {}", context.user_intent, combined)
                };
                tracing::info!(
                    task_id = %task_id,
                    step_id = %step.id,
                    "[Executor] Applied steering instruction at step boundary"
                );
            }
        }

        // Skill 执行：以 "skill:" 开头的 capability_id 由 Skill 系统处理
        if let Some(skill_id) = step.capability_id.strip_prefix("skill:") {
            return self
                .execute_skill_step(skill_id, step, context, handler_ctx)
                .await;
        }

        // 获取能力定义
        let capability =
            crate::services::agent::capability::get_capability_by_id(&step.capability_id)
                .await
                .ok_or_else(|| format!("Unknown capability: {}", step.capability_id))?;

        // 权限校验：授予权限（自主上限存在时一并重读 grant）对照 capability.required_permissions
        if !capability.required_permissions.is_empty()
            || handler_ctx.autonomy_permission_cap.is_some()
        {
            let user_perms =
                crate::services::agent::get_user_permissions(handler_ctx.db, handler_ctx.user_id)
                    .await;
            let granted: Vec<String> = user_perms.into_iter().collect();
            let grant = if handler_ctx.autonomy_permission_cap.is_some() {
                crate::services::agent::consciousness::AutonomyGrantStore::new(
                    handler_ctx.db.clone(),
                )
                .find(handler_ctx.user_id)
                .await
                .ok()
                .flatten()
            } else {
                None
            };
            if let Some(error) =
                crate::services::agent::consciousness::autonomy_execute_permission_error(
                    handler_ctx.user_id,
                    grant.as_ref(),
                    &granted,
                    handler_ctx.autonomy_permission_cap.as_deref(),
                    &step.capability_id,
                    &capability.required_permissions,
                )
            {
                return Err(error);
            }
        }

        // 动态步骤补检：`should_block_unconfirmed_dynamic_step` 为 true 时
        // 返回 "This step needs confirmation first"。
        if context.is_dynamic_step(&step.id) {
            if let Some((_, risk)) =
                crate::services::agent::capability::capability_requires_confirmation_async(
                    &step.capability_id,
                )
                .await
            {
                let blocked =
                    Self::should_block_unconfirmed_dynamic_step(handler_ctx.user_id, risk);
                if blocked {
                    tracing::warn!(
                        step_id = %step.id,
                        capability = %step.capability_id,
                        risk = ?risk,
                        user_id = handler_ctx.user_id,
                        "[Executor] Blocked unconfirmed high-risk dynamic step"
                    );
                    return Err("This step needs confirmation first".to_string());
                }
            }
        }

        // 解析参数
        let (mut resolved_params, unresolved) =
            self.resolve_params(&step.params, &context.step_outputs);

        // 注入主 Agent 的具体指令：step.action 是 Planner 对这个子步骤的直接命令
        // 让 AI handler 知道「主 Agent 要求我做什么」，而不是自行发挥
        if !step.action.is_empty() && !resolved_params.contains_key("__directive") {
            resolved_params.insert("__directive".to_string(), json!(step.action));
        }
        // 注入用户原始请求，让 handler 知道最终用户的意图
        if !context.original_request.is_empty() && !resolved_params.contains_key("__user_request") {
            resolved_params.insert(
                "__user_request".to_string(),
                json!(context.original_request),
            );
        }
        if let Some(steering) = context.variables.get("_steering_instruction") {
            resolved_params
                .entry("__steering".to_string())
                .or_insert_with(|| steering.clone());
        }

        if !unresolved.is_empty() {
            tracing::warn!(
                step_id = %step.id,
                unresolved = ?unresolved,
                "[Executor] Some parameters could not be resolved"
            );
        }

        // 应用能力特定的参数回退逻辑
        self.apply_capability_param_fallbacks(
            &step.capability_id,
            &mut resolved_params,
            &context.step_outputs,
        );
        inject_request_context_params(
            &step.capability_id,
            &mut resolved_params,
            context.variables.get("_current_route"),
            context.page_context.as_ref(),
            context.variables.get("_music_status"),
            context.variables.get("_window_state"),
        );

        // 显式 timeout_ms 优先，否则按能力声明推断；AI 能力再抬一个保底。
        // 规则本体在 `executor_utils_pure::step_timeout_secs`，Skill 内部 DAG
        // 走同一份，不再各算各的。
        let timeout_secs = step_timeout_secs(
            step.timeout_ms,
            capability.estimated_duration_ms,
            category_timeout_fallback_secs(&capability.category),
            capability.requires_ai,
        );
        let capability_category = capability.category.clone();

        tracing::debug!(
            step_id = %step.id,
            capability = %step.capability_id,
            timeout_secs = timeout_secs,
            params = ?resolved_params,
            "[Executor] Executing step"
        );

        // 分发到具体 handler（超时 + 执行中取消轮询，避免长步骤只能等步间检查）
        let output = execute_capability_with_timeout_and_cancel(
            &step.capability_id,
            &step.action,
            &capability_category,
            &resolved_params,
            handler_ctx,
            timeout_secs,
            handler_ctx.task_id.as_deref(),
        )
        .await?;

        Self::apply_output_contract(step, &capability, &output)?;

        Ok(output)
    }

    /// Breach (wrong JSON type / missing required) fails the step.
    /// `ContractViolation::Drift` 只 `tracing::warn` 后 `Ok(())`。
    ///
    /// MCP output schemas use the Work loop's JSON Schema validator; this
    /// builtin contract checker does not interpret their dynamic schemas.
    pub(crate) fn apply_output_contract(
        step: &RecipeStep,
        capability: &Capability,
        output: &Value,
    ) -> Result<(), String> {
        if step.capability_id.starts_with("mcp.") {
            return Ok(());
        }

        let Some(violation) = crate::services::agent::capability::check_output_contract(
            &capability.output_schema,
            output,
        ) else {
            return Ok(());
        };

        if violation.is_fatal() {
            tracing::error!(
                step_id = %step.id,
                capability = %step.capability_id,
                violation = violation.message(),
                "[Executor] Step output breaches its declared contract"
            );
            return Err(violation.message().to_string());
        }
        tracing::warn!(
            step_id = %step.id,
            capability = %step.capability_id,
            violation = violation.message(),
            "[Executor] Capability output_schema has drifted from its handler"
        );
        Ok(())
    }

    /// 执行 Skill 步骤
    ///
    /// Skill 的 full_instructions 包含执行策略（自然语言描述的步骤编排），
    /// 通过 AI 将其转化为具体的能力调用序列并动态注入执行上下文。
    pub(crate) async fn execute_skill_step(
        &self,
        skill_id: &str,
        step: &RecipeStep,
        context: &mut ExecutionContext,
        handler_ctx: &HandlerContext<'_>,
    ) -> Result<Value, String> {
        // 从注册表加载 Skill
        let registry = crate::services::agent::skill::get_skill_registry()
            .ok_or("Skill registry not initialized")?;
        let skill = registry
            .get(skill_id)
            .await
            .ok_or_else(|| format!("Unknown skill: {}", skill_id))?;

        tracing::info!(
            skill_id = skill_id,
            skill_name = %skill.name,
            "[Executor] Executing skill"
        );

        let user_perms =
            crate::services::agent::get_user_permissions(handler_ctx.db, handler_ctx.user_id).await;
        if !crate::services::agent::skill::skill_covered_by_grants(&skill, Some(&user_perms)).await
        {
            return Err(format!("Skill '{}' is not available", skill.name));
        }

        // 检查 gating（前置条件）
        if !skill.gating.capabilities.is_empty() {
            let cap_registry = get_registry();
            for required_cap in &skill.gating.capabilities {
                if cap_registry.get(required_cap).is_none() {
                    return Err(format!(
                        "Skill '{}' requires capability '{}' which is not available",
                        skill.name, required_cap
                    ));
                }
            }
        }

        // 用 AI 将 Skill instructions 转化为执行计划
        let analyzer = handler_ctx
            .ai_analyzer
            .ok_or("AI analyzer not available for skill execution")?;

        // 构建可用能力列表（仅 Skill gating 中声明的 + 通用 AI 能力）
        let available_caps = {
            let cap_registry = get_registry();
            let mut caps_desc = String::new();
            let gating_caps = &skill.gating.capabilities;
            let all_caps = cap_registry.get_all();
            for cap in &all_caps {
                if gating_caps.contains(&cap.id) || cap.id.starts_with("ai.") {
                    if !crate::services::agent::capability::capability_covered_by_grants(
                        cap,
                        Some(&user_perms),
                    ) {
                        continue;
                    }
                    if !crate::services::agent::consciousness::required_permissions_within_cap(
                        &cap.required_permissions,
                        handler_ctx.autonomy_permission_cap.as_deref(),
                    ) {
                        continue;
                    }
                    // 提取 required params
                    let params_hint = cap
                        .input_schema
                        .get("required")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        })
                        .unwrap_or_default();
                    caps_desc.push_str(&format!(
                        "- `{}`: {} (params: {})\n",
                        cap.id,
                        cap.description,
                        if params_hint.is_empty() {
                            "none required"
                        } else {
                            &params_hint
                        }
                    ));
                }
            }
            caps_desc
        };

        // 构建用户上下文（原始请求 + 对话历史摘要 + 中途转向指令）
        let user_context = {
            let mut ctx_parts = Vec::new();
            ctx_parts.push(format!("Original request: {}", context.original_request));
            // Steering taken at the skill planning step boundary must reshape the plan.
            if let Some(steering) = context
                .variables
                .get("_steering_instruction")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                ctx_parts.push(format!(
                    "Latest steering (follow this; adjust later step params and goals): {}",
                    steering
                ));
            } else if !context.user_intent.is_empty()
                && context.user_intent != context.original_request
            {
                // user_intent may already include "Steering: ..." from the step boundary.
                if context.user_intent.contains("Steering:") {
                    ctx_parts.push(format!("Updated user intent: {}", context.user_intent));
                }
            }
            if let Some(conv) = &context.conversation_context {
                let recent: Vec<String> = conv
                    .iter()
                    .rev()
                    .take(3)
                    .rev()
                    .map(|m| {
                        format!(
                            "[{}]: {}",
                            m.role,
                            m.content
                                .chars()
                                .take(USER_TEXT_MAX_CHARS)
                                .collect::<String>()
                        )
                    })
                    .collect();
                if !recent.is_empty() {
                    ctx_parts.push(format!("Recent conversation:\n{}", recent.join("\n")));
                }
            }
            ctx_parts.join("\n\n")
        };

        // 将 ${param} 槽位替换为 step.params 中的实际值
        let resolved_instructions = {
            let mut text = skill.full_instructions.clone();
            for (key, value) in &step.params {
                let placeholder = format!("${{{}}}", key);
                let replacement = match value {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                text = text.replace(&placeholder, &replacement);
            }
            text
        };

        tracing::info!(
            skill_id = skill_id,
            step_params = %serde_json::to_string(&step.params).unwrap_or_default(),
            has_context = !user_context.is_empty(),
            params_resolved = resolved_instructions != skill.full_instructions,
            "[Executor] Skill planning with params and context"
        );

        // 收集已有的搜索/分析输出（避免重复 webSearch，并让 Skill AI 知道上游数据）
        let prior_knowledge = {
            let mut knowledge_parts = Vec::new();
            for (out_id, out_val) in context.get_all_outputs() {
                // 收集各类有意义的输出（搜索摘要、分析结果等）
                if let Some(text) = prior_step_text(out_val) {
                    let truncated: String = text.chars().take(USER_TEXT_MAX_CHARS).collect();
                    knowledge_parts.push(format!("[{}] {}", out_id, truncated));
                }
            }
            if knowledge_parts.is_empty() {
                String::new()
            } else {
                // 上游数据里有 webSearch / scrape 抓回来的正文，而这个提示词的
                // 产物是要被执行的步骤。带边界进来，别让正文里的祈使句直通执行层。
                format!(
                    "\n## Existing upstream data (reuse it; do not search or analyze the same content again)\n{}\n",
                    untrusted_block("upstream_output", &knowledge_parts.join("\n\n"))
                )
            }
        };

        // 处理 count/variations 参数 → 注入到 Skill prompt
        let count_hint = {
            let count = step
                .params
                .get("count")
                .and_then(|v| v.as_u64())
                .unwrap_or(1);
            let variations = step.params.get("variations").and_then(|v| v.as_array());
            if count > 1 || variations.is_some() {
                let mut hint = format!(
                    "\n## Count and variants\nThe user wants {} distinct results.",
                    count
                );
                if let Some(vars) = variations {
                    let descs: Vec<String> = vars
                        .iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect();
                    hint.push_str(&format!(
                        "\nVariant descriptions:\n{}",
                        descs
                            .iter()
                            .enumerate()
                            .map(|(i, d)| format!("{}. {}", i + 1, d))
                            .collect::<Vec<_>>()
                            .join("\n")
                    ));
                }
                hint.push_str(
                    "\n\n**Important**: run search/research once and share the result across variants. \
                Emit a separate prompt.generate + ai.image pair for each variant.\n",
                );
                hint
            } else {
                String::new()
            }
        };

        let prompt = format!(
            "You are Myriad's DAG planner.\n\
             Your job: given the Skill policy and the user request, output a JSON execution plan (a directed acyclic graph of steps).\n\n\
             ## Skill: {name}\n{desc}\n\n\
             ## User context\n{context}\n\n\
             ## Available capabilities (use only these capability_id values)\n{caps}\n\
             ## Strategy\n{instructions}\n\n\
             ## User params\n{params}\n\
             {prior_knowledge}\
             {count_hint}\n\
             ---\n\n\
             # Rules\n\n\
             ## 1. Structure\n\
             1. `capability_id` must come from the list above\n\
             2. Every step needs a unique `id` (short, e.g. `search_info`, `gen_prompt_1`)\n\
             3. Every step needs a `depends_on` array (use `[]` when there is no dependency)\n\
             4. `action` is the concrete goal of that step (shown to the user)\n\
             5. {step_cap}Return pure JSON, no markdown wrapper\n\n\
             ## 2. Search first\n\
             6. **Intel first**: when the task needs external knowledge you are not sure about (character looks, event details, specialist facts), \
             **you must ai.webSearch first**. Every later step depends on it. Search exists so later generation is accurate.\n\
             7. At most one search step in the whole plan; variants share that result. \
             If existing search results already cover it, do not search again.\n\n\
             ## 3. depends_on = order\n{dependency}\n\n\
             ## 4. Data flow (xxxFrom)\n{data_flow}\n\n\
             ## 5. ai.image size\n{image_size}\n\n\
             ---\n\n\
             # Example (3 character images that need a search; portrait)\n\n\
             ```json\n\
             {{\n\
               \"steps\": [\n\
                 {{\"id\": \"search\",      \"capability_id\": \"ai.webSearch\",   \"action\": \"Search the character's looks\",      \"params\": {{\"query\": \"...\"}},             \"depends_on\": []}},\n\
                 {{\"id\": \"prompt_1\",    \"capability_id\": \"prompt.generate\", \"action\": \"Write variant 1 prompt\",       \"params\": {{\"description\": \"...\"}},       \"depends_on\": [\"search\"]}},\n\
                 {{\"id\": \"prompt_2\",    \"capability_id\": \"prompt.generate\", \"action\": \"Write variant 2 prompt\",       \"params\": {{\"description\": \"...\"}},       \"depends_on\": [\"search\"]}},\n\
                 {{\"id\": \"prompt_3\",    \"capability_id\": \"prompt.generate\", \"action\": \"Write variant 3 prompt\",       \"params\": {{\"description\": \"...\"}},       \"depends_on\": [\"search\"]}},\n\
                 {{\"id\": \"img_1\",       \"capability_id\": \"ai.image\",        \"action\": \"Generate variant 1 image\",         \"params\": {{\"promptFrom\": \"prompt_1\", \"width\": 768, \"height\": 1024}},   \"depends_on\": [\"prompt_1\"]}},\n\
                 {{\"id\": \"img_2\",       \"capability_id\": \"ai.image\",        \"action\": \"Generate variant 2 image\",         \"params\": {{\"promptFrom\": \"prompt_2\", \"width\": 768, \"height\": 1024}},   \"depends_on\": [\"prompt_2\"]}},\n\
                 {{\"id\": \"img_3\",       \"capability_id\": \"ai.image\",        \"action\": \"Generate variant 3 image\",         \"params\": {{\"promptFrom\": \"prompt_3\", \"width\": 768, \"height\": 1024}},   \"depends_on\": [\"prompt_3\"]}}\n\
               ]\n\
             }}\n\
             ```\n\
             Flow: search (alone) → prompt_1+prompt_2+prompt_3 (parallel) → each img starts as soon as its prompt finishes\n\n\
             Return JSON only. No explanation.",
            name = skill.name,
            desc = skill.description,
            context = user_context,
            caps = available_caps,
            instructions = resolved_instructions,
            params = serde_json::to_string_pretty(&step.params).unwrap_or_default(),
            prior_knowledge = prior_knowledge,
            count_hint = count_hint,
            step_cap = plan_step_cap_rule(),
            dependency = PLAN_DEPENDENCY_RULE,
            data_flow = PLAN_DATA_FLOW_RULE,
            image_size = plan_image_size_rule(),
        );

        let ai_result = analyzer.analyze(&prompt).await.map_err(|error| {
            tracing::error!(%error, "Skill AI planning failed");
            classify_outbound_fetch("Skill AI planning failed", &error.to_string())
        })?;

        tracing::debug!(
            skill_id = skill_id,
            ai_plan_len = ai_result.len(),
            "[Executor] Skill AI planning result: {}",
            ai_result.chars().take(500).collect::<String>()
        );

        // 解析 AI 返回的步骤计划（从文本中提取 JSON）
        let plan: Value = {
            // 尝试找到 JSON 块
            let text = ai_result.trim();
            let extracted = extract_json_object_from_ai_response(text);
            let json_str = extracted.as_deref().unwrap_or(text);
            match serde_json::from_str(json_str) {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!(
                        skill_id = skill_id,
                        error = %e,
                        response_preview = truncate_str(json_str, 200),
                        "[Executor] Skill AI JSON parse failed"
                    );
                    return Err(format!(
                        "Skill '{}' AI planning returned unparseable response: {}",
                        skill.name, e
                    ));
                }
            }
        };

        let planned_steps = plan
            .get("steps")
            .and_then(|s| s.as_array())
            .cloned()
            .unwrap_or_default();

        if planned_steps.is_empty() {
            return Err(format!(
                "Skill '{}' AI planning returned no executable steps",
                skill.name
            ));
        }

        // 步骤上限
        let planned_steps: Vec<&Value> = planned_steps.iter().take(MAX_PLAN_STEPS).collect();

        // 验证并构建动态步骤（两遍扫描：第一遍建立 id 映射，第二遍解析引用）
        let cap_registry = get_registry();
        // AI step id → 实际 step id 映射（用于 depends_on 和 xxxFrom 引用重写）
        let mut id_map: HashMap<String, String> = HashMap::new();

        // 预注入父级步骤 ID → 自身映射，让 skill 子步骤的 xxxFrom 能引用父级输出
        // 父级步骤已完成，不需要在 DAG 中等待，但 xxxFrom 参数解析需要它们
        let parent_step_ids: std::collections::HashSet<String> =
            context.get_all_outputs().keys().cloned().collect();
        for parent_id in &parent_step_ids {
            id_map.insert(parent_id.clone(), parent_id.clone());
        }
        // 记录哪些步骤被跳过（gating/无效），用于依赖断裂检测
        let mut skipped_indices: std::collections::HashSet<usize> =
            std::collections::HashSet::new();
        // 被跳过步骤的 plan id（pass 2 检测依赖断裂）
        let mut skipped_ai_ids: std::collections::HashSet<String> =
            std::collections::HashSet::new();

        // 第一遍：验证 capability_id + 建立完整 id_map
        for (i, planned) in planned_steps.iter().enumerate() {
            let cap_id = planned
                .get("capability_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            // 验证 capability_id
            if cap_id.is_empty()
                || (!cap_id.starts_with("skill:")
                    && !cap_id.starts_with("mcp.")
                    && cap_registry.get(cap_id).is_none())
            {
                tracing::warn!(
                    skill_id = skill_id,
                    step_index = i,
                    invalid_cap = cap_id,
                    "[Executor] Skill step generated invalid capability_id, skipping"
                );
                if i == 0 {
                    return Err(format!(
                        "Skill '{}' generated invalid first step capability: '{}'",
                        skill_id, cap_id
                    ));
                }
                skipped_indices.insert(i);
                // 记录被跳过步骤的 AI id，用于依赖断裂检测
                if let Some(ai_id) = planned.get("id").and_then(|v| v.as_str()) {
                    if !ai_id.is_empty() {
                        skipped_ai_ids.insert(ai_id.to_string());
                    }
                }
                continue;
            }

            // Gating 校验
            if !skill.gating.capabilities.is_empty()
                && !cap_id.starts_with("ai.")
                && !cap_id.starts_with("skill:")
                && !cap_id.starts_with("mcp.")
                && !skill.gating.capabilities.contains(&cap_id.to_string())
            {
                tracing::warn!(
                    skill_id = skill_id,
                    cap_id = cap_id,
                    "[Executor] Skill step uses capability outside gating scope, skipping"
                );
                skipped_indices.insert(i);
                if let Some(ai_id) = planned.get("id").and_then(|v| v.as_str()) {
                    if !ai_id.is_empty() {
                        skipped_ai_ids.insert(ai_id.to_string());
                    }
                }
                continue;
            }

            // 预注册 AI step id → 实际 step id 映射（确保前向引用可解析）
            let ai_step_id = planned.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let step_id = format!("{}_{}_step_{}", step.id, skill_id, i);
            if !ai_step_id.is_empty() {
                id_map.insert(ai_step_id.to_string(), step_id);
            }
        }

        // 第二遍：解析引用 + 构建动态步骤
        let mut dynamic_steps = Vec::with_capacity(planned_steps.len());

        for (i, planned) in planned_steps.iter().enumerate() {
            if skipped_indices.contains(&i) {
                continue;
            }

            let cap_id = planned
                .get("capability_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let action = planned
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("execute");
            let params: HashMap<String, Value> = planned
                .get("params")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            let ai_step_id = planned.get("id").and_then(|v| v.as_str()).unwrap_or("");

            // depends_on: 重写 AI step id + 检测依赖断裂
            let mut has_broken_dep = false;
            let depends_on = planned
                .get("depends_on")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|v| {
                    let dep = v.as_str()?;
                    // 检查是否引用了被跳过的步骤
                    if skipped_ai_ids.contains(dep) {
                        tracing::warn!(
                            dependency = %dep,
                            step_id = ai_step_id,
                            "[Executor] Skill sub-step depends on skipped step '{}', data flow broken",
                            dep
                        );
                        has_broken_dep = true;
                        return None;
                    }
                    // 父级步骤已完成，无需在 DAG 中等待（数据通过 xxxFrom 参数解析获取）
                    if parent_step_ids.contains(dep) {
                        return None;
                    }
                    if let Some(actual) = id_map.get(dep) {
                        Some(actual.clone())
                    } else {
                        tracing::warn!(
                            dependency = %dep,
                            "[Executor] Skill sub-step depends_on references unknown step id '{}', dropping dependency",
                            dep
                        );
                        None
                    }
                }).collect::<Vec<_>>())
                .unwrap_or_default();

            if has_broken_dep {
                tracing::warn!(
                    skill_id = skill_id,
                    step_id = ai_step_id,
                    "[Executor] Skill step depends on a skipped step, data flow may be incomplete"
                );
            }

            // 重写 params 中的 xxxFrom 引用
            let resolved_params: HashMap<String, Value> = params.into_iter().map(|(k, v)| {
                if k.ends_with("From") {
                    if let Some(ref_str) = v.as_str() {
                        let parts: Vec<&str> = ref_str.splitn(2, '.').collect();
                        if let Some(actual_id) = id_map.get(parts[0]) {
                            let new_ref = if parts.len() > 1 {
                                format!("{}.{}", actual_id, parts[1])
                            } else {
                                actual_id.clone()
                            };
                            return (k, Value::String(new_ref));
                        }
                        tracing::warn!(
                            param = %k,
                            reference = %ref_str,
                            "[Executor] xxxFrom references unknown step id '{}', passing literal value",
                            parts[0]
                        );
                    }
                }
                (k, v)
            }).collect();

            let on_failure = if i == 0 && !cap_id.starts_with("ai.") {
                types::FailureStrategy::Abort
            } else {
                types::FailureStrategy::Skip
            };

            let step_id = format!("{}_{}_step_{}", step.id, skill_id, i);

            // 子步骤超时：`step_timeout_secs` 再与 `SKILL_SUB_STEP_MIN_SECS` 取较大值。
            let sub_step_timeout_ms = cap_registry
                .get(cap_id)
                .map(|capability| {
                    step_timeout_secs(
                        None,
                        capability.estimated_duration_ms,
                        category_timeout_fallback_secs(&capability.category),
                        capability.requires_ai,
                    )
                })
                .unwrap_or(0)
                .max(SKILL_SUB_STEP_MIN_SECS)
                * 1000;

            dynamic_steps.push(RecipeStep {
                id: step_id,
                order: (step.order * 100) + (i as u32),
                capability_id: cap_id.to_string(),
                action: action.to_string(),
                params: resolved_params,
                depends_on,
                on_failure,
                retry: None,
                timeout_ms: Some(sub_step_timeout_ms),
                model_tier: skill.tier_hint.as_ref().map(|h| h.to_model_tier()),
                generator: None,
            });
        }

        if dynamic_steps.is_empty() {
            return Err(format!(
                "Skill '{}' all generated steps had invalid capability_ids",
                skill.name
            ));
        }

        let steps_count = dynamic_steps.len();
        // 收集子步骤 ID，供下游 resolve_path_reference 聚合真实输出
        let substep_ids: Vec<String> = dynamic_steps.iter().map(|s| s.id.clone()).collect();
        context.queue_dynamic_steps(dynamic_steps);

        tracing::info!(
            skill_id = skill_id,
            dynamic_steps = steps_count,
            "[Executor] Skill generated {} dynamic steps",
            steps_count
        );

        let last_output = json!({
            "skill": skill_id,
            "status": "planned",
            "dynamic_steps_generated": steps_count,
            "__substep_ids": substep_ids,
        });

        Ok(last_output)
    }

    /// 构建 StepDebug 事件用的参数预览（截断超长字符串）
    pub(crate) fn build_debug_params(params: &HashMap<String, Value>) -> Option<Value> {
        let mut p = params.clone();
        for v in p.values_mut() {
            if let Some(s) = v.as_str() {
                if s.len() > 500 {
                    // truncate_str 保证不在多字节字符中间截断
                    *v = json!(format!(
                        "{}...({}chars)",
                        utils::truncate_str(s, 500),
                        s.len()
                    ));
                }
            }
        }
        serde_json::to_value(&p).ok()
    }

    /// 能力特定回退：`context.reference` 用 stepId/path 解析；
    /// `music.playlist` 缺 playlistId 时取 recommendedPlaylistId 或 playlists[0].id。
    pub(crate) fn apply_capability_param_fallbacks(
        &self,
        capability_id: &str,
        params: &mut HashMap<String, Value>,
        previous_outputs: &HashMap<String, Value>,
    ) {
        if capability_id == "context.reference" {
            if let Some(ref_str) = context_reference_source(params) {
                if let Some(value) = self.resolve_path_reference(&ref_str, previous_outputs) {
                    let transform = params.get("transform").and_then(Value::as_str);
                    params.insert(
                        "value".to_string(),
                        apply_reference_transform(value, transform),
                    );
                }
            }
        }
        if capability_id == "music.playlist" {
            let has_playlist_id = params
                .get("playlistId")
                .map(|v| {
                    v.as_str().map(|s| !s.is_empty()).unwrap_or(false)
                        || v.as_i64().is_some()
                        || v.as_u64().is_some()
                })
                .unwrap_or(false);

            if !has_playlist_id {
                tracing::info!(
                    "[Executor] music.playlist missing playlistId, searching previous outputs"
                );
                for output in previous_outputs.values() {
                    if let Some(pid) = output
                        .get("recommendedPlaylistId")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                    {
                        tracing::info!(playlist_id = %pid, "[Executor] Found recommendedPlaylistId");
                        params.insert("playlistId".to_string(), json!(pid));
                        break;
                    }
                    if let Some(first) = output
                        .get("playlists")
                        .and_then(|p| p.as_array())
                        .and_then(|arr| arr.first())
                    {
                        let id_opt = first.get("id").and_then(|id| {
                            id.as_i64()
                                .map(|n| n.to_string())
                                .or_else(|| id.as_str().map(String::from))
                        });
                        if let Some(id_str) = id_opt {
                            tracing::info!(playlist_id = %id_str, "[Executor] Found playlistId from playlists[0]");
                            params.insert("playlistId".to_string(), json!(id_str));
                            break;
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn resolve_params(
        &self,
        params: &HashMap<String, Value>,
        previous_outputs: &HashMap<String, Value>,
    ) -> (HashMap<String, Value>, Vec<String>) {
        let mut resolved = HashMap::new();
        let mut unresolved = Vec::new();

        for (key, value) in params {
            if key.ends_with("From") {
                if let Some(ref_str) = value.as_str() {
                    let resolved_value = self.resolve_path_reference(ref_str, previous_outputs);
                    if let Some(output) = resolved_value {
                        let new_key = key.trim_end_matches("From").to_string();

                        // 数据型参数（data, content, input, context, items）保留完整对象，
                        // 因为下游 handler（如 ai.analyze）有自己的 extract_semantic_text 逻辑
                        // 能处理结构化数据（如 results 数组、嵌套字段等）。
                        //
                        // 文本型参数（title, description, summary, prompt 等）提取语义文本，
                        // 因为下游期望的是纯字符串。
                        let is_data_param = matches!(
                            new_key.as_str(),
                            "data" | "content" | "input" | "context" | "items"
                        );

                        // ID 型参数（playlistId、songId、id 等）：引用结构化输出时
                        // 必须提取真实 ID，不能走语义文本提取。
                        let is_id_param = new_key == "id"
                            || new_key.ends_with("Id")
                            || new_key.ends_with("ID")
                            || new_key.ends_with("_id");

                        let final_value = if output.is_string() || is_data_param {
                            Some(output)
                        } else if is_id_param && (output.is_object() || output.is_array()) {
                            // 提取失败按未解析处理：让步骤显式报错，
                            // 而不是塞进错误的值静默错下去
                            Self::extract_id_from_output(&output, &new_key)
                        } else if output.is_object() {
                            let text = Self::extract_text_from_output(&output);
                            if text.is_empty() {
                                Some(output)
                            } else {
                                Some(Value::String(text))
                            }
                        } else {
                            Some(output)
                        };

                        match final_value {
                            Some(v) => {
                                resolved.insert(new_key, v);
                                continue;
                            }
                            None => unresolved.push(format!("{}: {}", key, ref_str)),
                        }
                    } else {
                        unresolved.push(format!("{}: {}", key, ref_str));
                    }
                }
            }

            resolved.insert(key.clone(), value.clone());
        }

        (resolved, unresolved)
    }

    /// 从结构化步骤输出中提取 ID 型参数值。
    /// 优先级：同名字段 → 顶层 id → 常见列表字段第一项的 id / 同名字段。
    /// 数组输入取第一个元素递归提取
    pub(crate) fn extract_id_from_output(output: &Value, param_key: &str) -> Option<Value> {
        fn id_like(v: &Value) -> bool {
            v.is_string() || v.is_number()
        }

        match output {
            Value::Array(arr) => arr
                .first()
                .and_then(|first| Self::extract_id_from_output(first, param_key)),
            Value::Object(obj) => {
                // 1. 同名字段（如 playlistId）
                if let Some(v) = obj.get(param_key).filter(|v| id_like(v)) {
                    return Some(v.clone());
                }
                // 2. 顶层 id
                if let Some(v) = obj.get("id").filter(|v| id_like(v)) {
                    return Some(v.clone());
                }
                // 3. 常见列表字段的第一项（搜索类输出：playlists/results/…）
                for list_key in ["playlists", "results", "items", "list", "songs", "data"] {
                    if let Some(first) = obj
                        .get(list_key)
                        .and_then(|v| v.as_array())
                        .and_then(|a| a.first())
                    {
                        if let Some(v) = first.get(param_key).filter(|v| id_like(v)) {
                            return Some(v.clone());
                        }
                        if let Some(v) = first.get("id").filter(|v| id_like(v)) {
                            return Some(v.clone());
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// 从步骤输出的 JSON 对象中提取主要文本内容
    pub(crate) fn extract_text_from_output(output: &Value) -> String {
        let output = crate::services::agent::ai_process_pure::task_inner_value(output);
        if let Some(text) = output.as_str().filter(|s| !s.is_empty()) {
            return text.to_string();
        }
        let text_keys = [
            "analysis",
            "reply",
            "aiSummary",
            "summary",
            "description",
            "message",
            "content",
            "prompt",
        ];
        if let Some(obj) = output.as_object() {
            for key in &text_keys {
                if let Some(text) = obj.get(*key).and_then(|v| v.as_str()) {
                    if !text.is_empty() {
                        return text.to_string();
                    }
                }
            }
        }
        String::new()
    }

    /// 解析路径引用
    pub(crate) fn resolve_path_reference(
        &self,
        ref_str: &str,
        previous_outputs: &HashMap<String, Value>,
    ) -> Option<Value> {
        // 支持 "step_id" 或 "step_id.path.to.value" 或 "step_id.array[0].field"
        let parts: Vec<&str> = ref_str.splitn(2, '.').collect();
        let step_id = parts[0];

        let output = previous_outputs.get(step_id)?;

        // Skill 步骤特殊处理：输出中 __substep_ids 表示这是技能占位符，
        // 用最后一个已完成子步骤的真实输出替代无意义的 "planned" 元数据
        let effective_output =
            if let Some(substep_ids) = output.get("__substep_ids").and_then(|v| v.as_array()) {
                let mut last: Option<&Value> = None;
                for id_val in substep_ids {
                    if let Some(id) = id_val.as_str() {
                        if let Some(sub_output) = previous_outputs.get(id) {
                            last = Some(sub_output);
                        }
                    }
                }
                match last {
                    Some(sub_out) => std::borrow::Cow::Borrowed(sub_out),
                    None => std::borrow::Cow::Borrowed(output),
                }
            } else {
                std::borrow::Cow::Borrowed(output)
            };

        if parts.len() == 1 {
            return Some(effective_output.into_owned());
        }

        // 解析路径
        let path = parts[1];
        Self::get_value_by_path(&effective_output, path)
    }

    /// 通过路径获取值。信封步骤先按外壳取，没有再拆 `value`。
    pub(crate) fn get_value_by_path(value: &Value, path: &str) -> Option<Value> {
        lookup_path(value, path).or_else(|| {
            lookup_path(
                crate::services::agent::ai_process_pure::task_inner_value(value),
                path,
            )
        })
    }
}

fn prior_step_text(out_val: &Value) -> Option<&str> {
    let inner = crate::services::agent::ai_process_pure::task_inner_value(out_val);
    inner
        .get("aiSummary")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            inner
                .get("analysis")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
        })
        .or_else(|| {
            inner
                .get("reply")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
        })
        .or_else(|| inner.as_str().filter(|s| !s.is_empty()))
}

fn lookup_path(value: &Value, path: &str) -> Option<Value> {
    let mut current = value;

    for segment in path.split('.') {
        if let Some(bracket_pos) = segment.find('[') {
            let field_name = &segment[..bracket_pos];
            if !segment.ends_with(']') || bracket_pos + 1 >= segment.len() - 1 {
                return None;
            }
            let index_str = &segment[bracket_pos + 1..segment.len() - 1];

            if !field_name.is_empty() {
                current = current.get(field_name)?;
            }

            let index: usize = index_str.parse().ok()?;
            current = current.get(index)?;
        } else {
            current = current.get(segment)?;
        }
    }

    Some(current.clone())
}

/// `context.reference` stepId + optional path → `step_id` or `step_id.path`.
fn context_reference_source(params: &HashMap<String, Value>) -> Option<String> {
    let step_id = params
        .get("stepId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    match params
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(path) => Some(format!("{step_id}.{path}")),
        None => Some(step_id.to_string()),
    }
}

fn apply_reference_transform(value: Value, transform: Option<&str>) -> Value {
    match transform.unwrap_or("none") {
        "stringify" => Value::String(value.to_string()),
        "parse" => value
            .as_str()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or(value),
        "join" => match &value {
            Value::Array(items) => Value::String(
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            _ => value,
        },
        "first" => match &value {
            Value::Array(items) => items.first().cloned().unwrap_or(Value::Null),
            _ => value,
        },
        "last" => match &value {
            Value::Array(items) => items.last().cloned().unwrap_or(Value::Null),
            _ => value,
        },
        _ => value,
    }
}

/// Fill `currentPath` / `context` from the turn request when the planner omitted
/// them. `router.state` declares an empty input schema, so without this it
/// always reports `/`.
pub(crate) fn inject_request_context_params(
    capability_id: &str,
    params: &mut HashMap<String, Value>,
    current_route: Option<&Value>,
    page_context: Option<&Value>,
    music_status: Option<&Value>,
    window_state: Option<&Value>,
) {
    let needs_path = matches!(
        capability_id,
        "router.state" | "page.content" | "page.understand" | "page.interact"
    );
    if needs_path {
        let missing = params
            .get("currentPath")
            .is_none_or(|v| v.as_str().map(str::trim).unwrap_or("").is_empty());
        if missing {
            if let Some(route) = current_route
                .cloned()
                .filter(|v| v.as_str().map(str::trim).is_some_and(|s| !s.is_empty()))
            {
                params.insert("currentPath".to_string(), route);
            }
        }
    }
    if matches!(capability_id, "page.content" | "page.understand")
        && !params.contains_key("context")
    {
        if let Some(page) = page_context.cloned() {
            params.insert("context".to_string(), page);
        }
    }
    if capability_id == "music.status" && !params.contains_key("status") {
        if let Some(status) = music_status.cloned() {
            params.insert("status".to_string(), status);
        }
    }
    if capability_id == "tapp.windows" && !params.contains_key("windowState") {
        if let Some(state) = window_state.cloned() {
            params.insert("windowState".to_string(), state);
        }
    }
}

#[cfg(test)]
mod output_contract_tests {
    use super::*;

    fn step(capability_id: &str) -> RecipeStep {
        RecipeStep {
            id: "s1".to_string(),
            order: 0,
            capability_id: capability_id.to_string(),
            action: String::new(),
            params: HashMap::new(),
            depends_on: vec![],
            on_failure: FailureStrategy::Abort,
            retry: None,
            timeout_ms: None,
            model_tier: None,
            generator: None,
        }
    }

    fn capability(capability_id: &str, output_schema: Value) -> Capability {
        Capability {
            id: capability_id.to_string(),
            output_schema,
            ..Default::default()
        }
    }

    fn summarize_schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "summary": { "type": "string" },
                "keyPoints": { "type": "array" }
            }
        })
    }

    #[test]
    fn breach_fails_the_step_and_drift_does_not() {
        let cap = capability("ai.summarize", summarize_schema());
        let id = step("ai.summarize");
        assert!(Executor::apply_output_contract(&id, &cap, &json!({ "summary": "ok" })).is_ok());
        assert!(Executor::apply_output_contract(&id, &cap, &json!({ "summary": 42 })).is_err());
        assert!(
            Executor::apply_output_contract(&id, &cap, &json!({ "message": "no declared field" }))
                .is_ok()
        );
        assert!(Executor::apply_output_contract(&id, &cap, &json!({})).is_ok());
        assert!(Executor::apply_output_contract(&id, &cap, &json!("not an object")).is_err());
    }

    #[test]
    fn extract_text_and_path_unwrap_task_envelope() {
        let envelope = json!({
            "format": "json",
            "value": { "analysis": "分析正文", "type": "custom" },
            "contextProvenance": []
        });
        assert_eq!(Executor::extract_text_from_output(&envelope), "分析正文");
        assert_eq!(
            Executor::extract_text_from_output(&json!({
                "format": "text",
                "value": "回复正文",
                "contextProvenance": []
            })),
            "回复正文"
        );
        assert_eq!(
            Executor::get_value_by_path(&envelope, "analysis")
                .and_then(|v| v.as_str().map(str::to_string)),
            Some("分析正文".to_string())
        );
        assert_eq!(
            Executor::get_value_by_path(&envelope, "value.analysis")
                .and_then(|v| v.as_str().map(str::to_string)),
            Some("分析正文".to_string())
        );
        assert_eq!(
            Executor::get_value_by_path(&envelope, "format")
                .and_then(|v| v.as_str().map(str::to_string)),
            Some("json".to_string())
        );
        assert_eq!(prior_step_text(&envelope), Some("分析正文"));
        assert_eq!(
            prior_step_text(&json!({
                "format": "text",
                "value": "回复正文",
                "contextProvenance": []
            })),
            Some("回复正文")
        );
        assert_eq!(
            prior_step_text(&json!({
                "aiSummary": "搜索摘要",
                "results": []
            })),
            Some("搜索摘要")
        );
    }

    #[test]
    fn mcp_tools_are_exempt_from_the_contract() {
        assert!(
            Executor::apply_output_contract(
                &step("mcp.docs.lookup"),
                &capability("mcp.docs.lookup", json!({ "type": "string" })),
                &json!({ "content": [{ "type": "text" }] }),
            )
            .is_ok()
        );
    }

    #[test]
    fn context_reference_source_and_transform() {
        let mut params = HashMap::new();
        params.insert("stepId".to_string(), json!("search"));
        assert_eq!(context_reference_source(&params).as_deref(), Some("search"));
        params.insert("path".to_string(), json!("results[0].title"));
        assert_eq!(
            context_reference_source(&params).as_deref(),
            Some("search.results[0].title")
        );

        let joined = apply_reference_transform(json!(["a", "b"]), Some("join"));
        assert_eq!(joined, json!("a\nb"));
        let first = apply_reference_transform(json!([1, 2, 3]), Some("first"));
        assert_eq!(first, json!(1));
        let parsed = apply_reference_transform(json!("{\"k\":1}"), Some("parse"));
        assert_eq!(parsed["k"], 1);
    }

    #[test]
    fn request_route_fills_router_state_current_path() {
        let mut params = HashMap::new();
        inject_request_context_params(
            "router.state",
            &mut params,
            Some(&json!("/library")),
            None,
            None,
            None,
        );
        assert_eq!(
            params.get("currentPath").and_then(Value::as_str),
            Some("/library")
        );
    }

    #[test]
    fn page_snapshot_fills_page_content_context() {
        let mut params = HashMap::new();
        let snapshot = json!({ "content": "正文", "title": "标题" });
        inject_request_context_params(
            "page.content",
            &mut params,
            Some(&json!("/phantasi")),
            Some(&snapshot),
            None,
            None,
        );
        assert_eq!(
            params.get("currentPath").and_then(Value::as_str),
            Some("/phantasi")
        );
        assert_eq!(params.get("context"), Some(&snapshot));
    }

    #[test]
    fn explicit_current_path_is_not_overwritten() {
        let mut params = HashMap::new();
        params.insert("currentPath".to_string(), json!("/tapp"));
        inject_request_context_params(
            "router.state",
            &mut params,
            Some(&json!("/library")),
            None,
            None,
            None,
        );
        assert_eq!(
            params.get("currentPath").and_then(Value::as_str),
            Some("/tapp")
        );
    }

    #[test]
    fn music_and_window_snapshots_fill_status_params() {
        let mut params = HashMap::new();
        let music = json!({ "isPlaying": true, "title": "song" });
        let windows = json!({ "windows": [], "windowCount": 0 });
        inject_request_context_params(
            "music.status",
            &mut params,
            None,
            None,
            Some(&music),
            Some(&windows),
        );
        assert_eq!(params.get("status"), Some(&music));
        let mut window_params = HashMap::new();
        inject_request_context_params(
            "tapp.windows",
            &mut window_params,
            None,
            None,
            Some(&music),
            Some(&windows),
        );
        assert_eq!(window_params.get("windowState"), Some(&windows));
    }

    #[test]
    fn capabilities_without_a_declared_schema_are_unconstrained() {
        assert!(
            Executor::apply_output_contract(
                &step("router.navigate"),
                &capability("router.navigate", json!({})),
                &json!({ "whatever": true }),
            )
            .is_ok()
        );
    }
}
