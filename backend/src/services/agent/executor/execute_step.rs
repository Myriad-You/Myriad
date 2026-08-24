// Executor single-step execution

use crate::services::agent::ai_process_pure::USER_TEXT_MAX_CHARS;
use crate::services::agent::capability::get_registry;
use crate::services::agent::types::{self, *};
use serde_json::{json, Value};
use std::collections::HashMap;

use super::executor_footer::*;
use super::handlers::HandlerContext;
use super::utils;
use super::Executor;
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

        // 权限校验：检查 capability 声明的 required_permissions
        if !capability.required_permissions.is_empty() {
            let user_perms =
                crate::services::agent::get_user_permissions(handler_ctx.db, handler_ctx.user_id)
                    .await;
            for perm in &capability.required_permissions {
                if !user_perms.contains(perm) {
                    return Err(format!(
                        "权限不足：执行 '{}' 需要 '{}' 权限",
                        step.capability_id, perm
                    ));
                }
            }
        }

        // 敏感操作补检：动态子步骤（技能展开/动态分析生成）绕过了 Planner 层的
        // check_sensitive_steps 确认流程。策略与 system_sensitive_gate 对齐：
        // - 系统任务：Medium 自动放行；High/Critical 硬拦并返回清晰 blocked 文案
        // - 交互用户：Medium+ 一律硬拦直至确认
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

        // 根据能力类别和预估时长确定超时（秒），预估时长取3倍作为缓冲
        // 优先使用 RecipeStep 指定的 timeout_ms，否则用能力声明推断
        let timeout_secs = step
            .timeout_ms
            .map(|ms| (ms / 1000).clamp(10, 300))
            .unwrap_or_else(|| {
                capability
                    .estimated_duration_ms
                    .map(|ms| (ms * 3 / 1000).clamp(10, 300))
                    .unwrap_or_else(|| match &capability.category {
                        CapabilityCategory::AiProcess | CapabilityCategory::ResourceCreate => 120,
                        CapabilityCategory::ExternalIntegration => 30,
                        _ => 30,
                    })
            });
        // AI 类能力最少给 60 秒（Pro 模型处理复杂输入+长文生成经常需要 40-50s）
        let timeout_secs = if capability.requires_ai && timeout_secs < 60 {
            60
        } else {
            timeout_secs
        };
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

        Self::report_output_contract(step, &capability, &output);

        Ok(output)
    }

    /// 校验步骤输出是否符合能力声明的 `output_schema`，**只上报、不判失败**。
    ///
    /// 最初的版本把 Breach（声明字段类型不符）判为步骤失败，理由是「声明和实现
    /// 不一致必然是 bug」。但错的一方可能是**声明**：`output_schema` 在此之前从未
    /// 被任何代码读取，注册表里的声明基本是照着愿望写的。仅在 16 个 AI 能力里就
    /// 查出 2 处类型写错（`ai.analyze` 的 `analysis` 实为字符串、`ai.recommend`
    /// 的 `recommendations` 在退化路径上是字符串）——按 12% 的错误率推算，未采样的
    /// 路径几乎必然还有。判失败就等于让一处声明笔误直接打挂一条正常功能。
    ///
    /// 另外 `required` 在全部 output_schema 里出现 0 次，所以「只对缺 required
    /// 致命」也是空条件，起不到兜底作用。
    ///
    /// 因此运行时只记录，`Breach` / `Drift` 的区分留给 CI：
    /// `output_contract` 的样本输出表断言被覆盖的能力不得出现 Breach。等注册表
    /// 的声明被逐个校准干净，再把这里翻回判失败。
    ///
    /// MCP 工具的 output_schema 是本地合成的占位（`{"type": "string"}`），不是
    /// 外部服务的真实契约，完全不参与校验。
    fn report_output_contract(step: &RecipeStep, capability: &Capability, output: &Value) {
        if step.capability_id.starts_with("mcp.") {
            return;
        }

        let Some(violation) = crate::services::agent::capability::check_output_contract(
            &capability.output_schema,
            output,
        ) else {
            return;
        };

        if violation.is_fatal() {
            tracing::warn!(
                step_id = %step.id,
                capability = %step.capability_id,
                violation = violation.message(),
                "[Executor] Step output breaches its declared contract (reported, not enforced)"
            );
        } else {
            tracing::warn!(
                step_id = %step.id,
                capability = %step.capability_id,
                violation = violation.message(),
                "[Executor] Capability output_schema has drifted from its handler"
            );
        }
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

        // 检查 gating（前置条件）
        if !skill.gating.capabilities.is_empty() {
            let cap_registry = get_registry().await;
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
            let cap_registry = get_registry().await;
            let mut caps_desc = String::new();
            let gating_caps = &skill.gating.capabilities;
            let all_caps = cap_registry.get_all();
            for cap in &all_caps {
                if gating_caps.contains(&cap.id) || cap.id.starts_with("ai.") {
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
                        "- `{}`: {} (参数: {})\n",
                        cap.id,
                        cap.description,
                        if params_hint.is_empty() {
                            "无必需"
                        } else {
                            &params_hint
                        }
                    ));
                }
            }
            drop(cap_registry);
            caps_desc
        };

        // 构建用户上下文（原始请求 + 对话历史摘要 + 中途转向指令）
        let user_context = {
            let mut ctx_parts = Vec::new();
            ctx_parts.push(format!("用户原始请求: {}", context.original_request));
            // Steering taken at the skill planning step boundary must reshape the plan.
            if let Some(steering) = context
                .variables
                .get("_steering_instruction")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                ctx_parts.push(format!(
                    "用户最新转向指令（优先遵循，调整后续步骤参数与目标）：{}",
                    steering
                ));
            } else if !context.user_intent.is_empty()
                && context.user_intent != context.original_request
            {
                // user_intent may already include "Steering: ..." from the step boundary.
                if context.user_intent.contains("Steering:") {
                    ctx_parts.push(format!("更新后的用户意图: {}", context.user_intent));
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
                    ctx_parts.push(format!("最近对话:\n{}", recent.join("\n")));
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
                let text = if let Some(summary) = out_val.get("aiSummary").and_then(|v| v.as_str())
                {
                    Some(summary)
                } else if let Some(analysis) = out_val.get("analysis").and_then(|v| v.as_str()) {
                    Some(analysis)
                } else {
                    out_val.get("reply").and_then(|v| v.as_str())
                };
                if let Some(text) = text {
                    let truncated: String = text.chars().take(USER_TEXT_MAX_CHARS).collect();
                    knowledge_parts.push(format!("[{}] {}", out_id, truncated));
                }
            }
            if knowledge_parts.is_empty() {
                String::new()
            } else {
                format!(
                    "\n## 已有上游数据（直接复用，不要重复搜索或分析相同内容）\n{}\n",
                    knowledge_parts.join("\n\n")
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
                let mut hint = format!("\n## 数量与变体要求\n用户需要 {} 组不同的结果。", count);
                if let Some(vars) = variations {
                    let descs: Vec<String> = vars
                        .iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect();
                    hint.push_str(&format!(
                        "\n变体描述：\n{}",
                        descs
                            .iter()
                            .enumerate()
                            .map(|(i, d)| format!("{}. {}", i + 1, d))
                            .collect::<Vec<_>>()
                            .join("\n")
                    ));
                }
                hint.push_str(
                    "\n\n**重要**：搜索/调研步骤只执行一次，结果被所有变体共享。\
                为每个变体分别生成独立的 prompt.generate + ai.image 步骤对。\n",
                );
                hint
            } else {
                String::new()
            }
        };

        let prompt = format!(
            "你是 Myriad 的 DAG 执行计划编排器。\n\
             你的任务：根据 Skill 策略和用户需求，输出一个 JSON 执行计划（步骤的有向无环图）。\n\
             引擎会根据 `depends_on` 自动调度：依赖已满足的步骤**立即并行启动**，无需你操心并行逻辑。\n\
             你只需把依赖关系写对。\n\n\
             ## Skill: {name}\n{desc}\n\n\
             ## 用户上下文\n{context}\n\n\
             ## 可用能力（只能使用这些 capability_id）\n{caps}\n\
             ## 执行策略\n{instructions}\n\n\
             ## 用户参数\n{params}\n\
             {prior_knowledge}\
             {count_hint}\n\
             ---\n\n\
             # 规则\n\n\
             ## 一、结构\n\
             1. `capability_id` 只能从上面的列表选择\n\
             2. 每个 step 必须有唯一 `id`（简短标识，如 `search_info`, `gen_prompt_1`）\n\
             3. 每个 step 必须有 `depends_on` 数组（无依赖写 `[]`）\n\
             4. `action` 字段写该步骤的具体目标（展示给用户看）\n\
             5. 最多 8 个步骤。只返回纯 JSON，不要 markdown 包裹\n\n\
             ## 二、搜索优先\n\
             6. **情报优先**：当任务涉及你不完全确定的外部知识（角色外貌、事件细节、专业信息等），\
             **必须先 ai.webSearch 获取情报**，所有后续步骤都依赖它。搜索是为了让后续生成更准确。\n\
             7. 搜索步骤全计划最多 1 个，多变体共享搜索结果。\
             如「已有搜索结果」已包含所需信息，则不再搜索。\n\n\
             ## 三、依赖 = 执行顺序\n\
             8. `depends_on: []` → 立即执行（与其他无依赖步骤并行）\n\
             9. `depends_on: [\"X\"]` → 等待 X 完成后执行（多个步骤 depends_on 同一个 X → 它们同时并行）\n\
             10. **黄金法则**：如果 step B 需要用到 step A 的输出/情报 → B 必须 `depends_on: [\"A\"]`\n\n\
             ## 四、数据流（xxxFrom）\n\
             11. 后续步骤使用前序输出：在 params 中写 `\"<字段名>From\": \"<step_id>\"`\n\
             12. 引擎自动把 `promptFrom: \"gen_prompt_1\"` 解析为：取 gen_prompt_1 的输出注入到 `prompt` 参数\n\
             13. **严禁** $$variable$$ 语法或模板占位符。params 值要么是具体文本，要么用 xxxFrom 引用\n\n\
             ## 五、ai.image 分辨率\n\
             14. 可选 `width`/`height`（整数像素 256–2048，省略默认 1024）。与 `promptFrom` 可同写\n\
             15. 用户口述尺寸、竖图/横图/壁纸时务必传入；不要把宽高塞进 prompt 文本\n\
             16. 建议：竖图 768×1024，横图 1024×768，方图省略或 1024×1024\n\n\
             ---\n\n\
             # 示例（3 张角色图，需要搜索角色信息；竖图）\n\n\
             ```json\n\
             {{\n\
               \"steps\": [\n\
                 {{\"id\": \"search\",      \"capability_id\": \"ai.webSearch\",   \"action\": \"搜索角色外貌特征\",      \"params\": {{\"query\": \"...\"}},             \"depends_on\": []}},\n\
                 {{\"id\": \"prompt_1\",    \"capability_id\": \"prompt.generate\", \"action\": \"生成变体1提示词\",       \"params\": {{\"description\": \"...\"}},       \"depends_on\": [\"search\"]}},\n\
                 {{\"id\": \"prompt_2\",    \"capability_id\": \"prompt.generate\", \"action\": \"生成变体2提示词\",       \"params\": {{\"description\": \"...\"}},       \"depends_on\": [\"search\"]}},\n\
                 {{\"id\": \"prompt_3\",    \"capability_id\": \"prompt.generate\", \"action\": \"生成变体3提示词\",       \"params\": {{\"description\": \"...\"}},       \"depends_on\": [\"search\"]}},\n\
                 {{\"id\": \"img_1\",       \"capability_id\": \"ai.image\",        \"action\": \"生成变体1图片\",         \"params\": {{\"promptFrom\": \"prompt_1\", \"width\": 768, \"height\": 1024}},   \"depends_on\": [\"prompt_1\"]}},\n\
                 {{\"id\": \"img_2\",       \"capability_id\": \"ai.image\",        \"action\": \"生成变体2图片\",         \"params\": {{\"promptFrom\": \"prompt_2\", \"width\": 768, \"height\": 1024}},   \"depends_on\": [\"prompt_2\"]}},\n\
                 {{\"id\": \"img_3\",       \"capability_id\": \"ai.image\",        \"action\": \"生成变体3图片\",         \"params\": {{\"promptFrom\": \"prompt_3\", \"width\": 768, \"height\": 1024}},   \"depends_on\": [\"prompt_3\"]}}\n\
               ]\n\
             }}\n\
             ```\n\
             执行流：search(独占) → prompt_1+prompt_2+prompt_3(并行) → 各自的 img 在 prompt 完成后立即启动\n\n\
             只返回 JSON，不要任何解释文字。",
            name = skill.name,
            desc = skill.description,
            context = user_context,
            caps = available_caps,
            instructions = resolved_instructions,
            params = serde_json::to_string_pretty(&step.params).unwrap_or_default(),
            prior_knowledge = prior_knowledge,
            count_hint = count_hint,
        );

        let ai_result = analyzer
            .analyze(&prompt)
            .await
            .map_err(|e| format!("Skill AI planning failed: {}", e))?;

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
            let json_str = if let Some(start) = text.find('{') {
                if let Some(end) = text.rfind('}') {
                    &text[start..=end]
                } else {
                    text
                }
            } else {
                text
            };
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
        let planned_steps: Vec<&Value> = planned_steps.iter().take(8).collect();

        // 验证并构建动态步骤（两遍扫描：第一遍建立 id 映射，第二遍解析引用）
        let cap_registry = get_registry().await;
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
        // 被跳过的 AI step id 集合（用于 pass 2 检测依赖断裂）
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

            dynamic_steps.push(RecipeStep {
                id: step_id,
                order: (step.order * 100) + (i as u32),
                capability_id: cap_id.to_string(),
                action: action.to_string(),
                params: resolved_params,
                depends_on,
                on_failure,
                retry: None,
                timeout_ms: Some(60000),
                model_tier: skill.tier_hint.as_ref().map(|h| h.to_model_tier()),
                generator: None,
            });
        }
        drop(cap_registry);

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

    /// 解析参数中的引用
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

    /// 能力特定的参数回退：当 resolve_params 无法填充某个必要参数时，
    /// 从前置步骤输出中尝试语义搜索。每个能力的回退逻辑集中在此处，
    /// 避免污染主执行路径。
    pub(crate) fn apply_capability_param_fallbacks(
        &self,
        capability_id: &str,
        params: &mut HashMap<String, Value>,
        previous_outputs: &HashMap<String, Value>,
    ) {
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

                        // 数据型参数（data, content, input, context）保留完整对象，
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
                        // 必须提取真实 ID，绝不能走语义文本提取——后者会把
                        // message 提示文案当 ID 用（歌单播放曾因此拿到
                        // "找到 10 个…" 字符串，任务"成功"但前端加载必败）
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
        self.get_value_by_path(&effective_output, path)
    }

    /// 通过路径获取值
    pub(crate) fn get_value_by_path(&self, value: &Value, path: &str) -> Option<Value> {
        let mut current = value;

        for segment in path.split('.') {
            // 检查是否有数组索引（如 field[0]）
            if let Some(bracket_pos) = segment.find('[') {
                let field_name = &segment[..bracket_pos];
                // 确保有闭合的 ] 且索引部分非空
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

    // 动态步骤生成系统
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

    /// The reporter never fails a step, whatever it finds. Enforcement lives in
    /// CI (see `output_contract`'s sample-output table) until the registry's
    /// declarations have been verified against their handlers.
    #[test]
    fn reporting_never_fails_a_step() {
        for output in [
            json!({ "summary": "ok" }),
            json!({ "summary": 42 }),
            json!({ "message": "no declared field present" }),
            json!({}),
            json!("not an object at all"),
        ] {
            Executor::report_output_contract(
                &step("ai.summarize"),
                &capability("ai.summarize", summarize_schema()),
                &output,
            );
        }
    }

    #[test]
    fn mcp_tools_are_exempt_from_the_contract() {
        // `mcp_capability` synthesizes `{"type": "string"}` locally; it is not a
        // contract the external server ever agreed to.
        Executor::report_output_contract(
            &step("mcp.docs.lookup"),
            &capability("mcp.docs.lookup", json!({ "type": "string" })),
            &json!({ "content": [{ "type": "text" }] }),
        );
    }

    #[test]
    fn capabilities_without_a_declared_schema_are_unconstrained() {
        Executor::report_output_contract(
            &step("router.navigate"),
            &capability("router.navigate", json!({})),
            &json!({ "whatever": true }),
        );
    }
}
