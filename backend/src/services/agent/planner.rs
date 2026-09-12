//! Planner 模块
//!
//! 合并意图分析 + 方案生成一次调用。请求 Pro 档；档关或模型留空时 resolve_ai_config 回落到 Standard。无密钥才是 None。
//! 输出 PlannerOutput；plan 的步骤是 AiRecipeStep，落地前还要校验/转换。

use std::collections::{HashMap, HashSet};

use crate::config::ModelTier;
use crate::services::agent::capability::{
    capability_covered_by_grants, get_capabilities_by_ids, get_compact_index_for_grants,
};
use crate::services::agent::identity;
use crate::services::agent::intent::keywords::LanguageDetector;
use crate::services::agent::memory;
use crate::services::agent::recipe::validate_and_convert_steps;
use crate::services::agent::types::*;
use crate::services::ai::create_ai_analyzer_for_tier;
use crate::services::analyzer::{AiAnalyzer, StreamDelta};
use myriad_agent_rules::{
    plan_image_size_rule, plan_step_cap_rule, MAX_PLAN_STEPS, PLAN_DATA_FLOW_RULE,
    PLAN_DEPENDENCY_RULE,
};

/// Planner — 一次调用完成意图理解 + 执行规划（请求 Pro 档，配置可回落到 Standard）
pub struct Planner {
    /// 请求 Pro 档的分析器（可能实际跑 Standard）
    ai_analyzer: Option<AiAnalyzer>,
    /// 语言检测器
    language_detector: LanguageDetector,
}

impl Planner {
    /// 创建 Planner：请求 Pro 档分析器；档关/模型空则回落 Standard
    pub async fn new() -> Self {
        let ai_analyzer = create_ai_analyzer_for_tier(ModelTier::Pro).await;
        if ai_analyzer.is_some() {
            tracing::info!("[Planner] Pro AI analyzer initialized");
        } else {
            tracing::warn!("[Planner] Pro AI analyzer NOT available — will use fallback");
        }
        Self {
            ai_analyzer,
            language_detector: LanguageDetector::new(),
        }
    }

    pub async fn plan_for(
        &self,
        request: &UserRequest,
        granted: &HashSet<String>,
    ) -> Result<PlannerOutput, String> {
        self.plan_internal(request, None, None, Some(granted)).await
    }

    pub async fn plan_with_progress_for(
        &self,
        request: &UserRequest,
        progress_tx: &tokio::sync::mpsc::Sender<AgentProgressEvent>,
        granted: &HashSet<String>,
    ) -> Result<PlannerOutput, String> {
        self.plan_internal(request, None, Some(progress_tx), Some(granted))
            .await
    }

    pub async fn replan_with_progress_for(
        &self,
        request: &UserRequest,
        escalation_hint: &str,
        progress_tx: &tokio::sync::mpsc::Sender<AgentProgressEvent>,
        granted: &HashSet<String>,
    ) -> Result<PlannerOutput, String> {
        self.plan_internal(
            request,
            Some(escalation_hint),
            Some(progress_tx),
            Some(granted),
        )
        .await
    }

    /// 内部规划逻辑
    async fn plan_internal(
        &self,
        request: &UserRequest,
        escalation_hint: Option<&str>,
        progress_tx: Option<&tokio::sync::mpsc::Sender<AgentProgressEvent>>,
        granted: Option<&HashSet<String>>,
    ) -> Result<PlannerOutput, String> {
        // Pro 分析器未持有时再创建一次；已持有的不会按新配置热加载。
        let runtime_analyzer;
        let ai_ref = if self.ai_analyzer.is_some() {
            self.ai_analyzer.as_ref()
        } else {
            runtime_analyzer = create_ai_analyzer_for_tier(ModelTier::Pro).await;
            runtime_analyzer.as_ref()
        };

        let Some(ai_analyzer) = ai_ref else {
            tracing::info!("[Planner] No AI available, using fallback");
            return Ok(self.fallback_plan(request));
        };

        let input = &request.raw_input;
        let language = self.language_detector.detect(input);

        // 构建 prompt
        let system_prompt = self
            .build_system_prompt(request, language, escalation_hint, granted)
            .await;
        let user_prompt = self.build_user_prompt(request, escalation_hint);

        // 优先结构化 JSON；parse_response 仍保留 fence/花括号回退。
        let schema = planner_output_schema();
        let mut response = None;
        for attempt in 0..2 {
            let result = if let Some(tx) = progress_tx {
                ai_analyzer
                    .analyze_json_streaming(
                        &system_prompt,
                        &user_prompt,
                        PLANNER_SCHEMA_NAME,
                        Some(&schema),
                        |delta| {
                            let tx = tx.clone();
                            async move {
                                if let StreamDelta::Reasoning(token) = delta {
                                    let _ = tx
                                        .send(AgentProgressEvent::ThinkingToken {
                                            token,
                                            done: false,
                                        })
                                        .await;
                                }
                                true
                            }
                        },
                    )
                    .await
            } else {
                ai_analyzer
                    .analyze_json(
                        &system_prompt,
                        &user_prompt,
                        PLANNER_SCHEMA_NAME,
                        Some(&schema),
                    )
                    .await
            };
            match result {
                Ok(text) => {
                    response = Some(text);
                    break;
                }
                Err(error) => {
                    if attempt == 0 {
                        tracing::warn!(%error, "[Planner] AI call failed, retrying once");
                        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
                    } else {
                        tracing::warn!(
                            %error,
                            "[Planner] AI call failed after retry, using fallback plan"
                        );
                        return Ok(self.fallback_plan(request));
                    }
                }
            }
        }
        let response = response.expect("loop sets response or returns");

        // 解析响应
        let mut output = self.parse_response(&response)?;

        // 校验步骤（如果是 plan 状态）
        if output.status == PlannerStatus::Plan && !output.steps.is_empty() {
            let autonomy_cap = request
                .context
                .as_ref()
                .and_then(|context| context.autonomy_permission_cap.as_deref());
            if let Err(e) = self
                .validate_steps(&mut output, autonomy_cap, granted)
                .await
            {
                tracing::warn!(error = %e, "[Planner] Step validation failed, trying to recover");
                // 验证失败时降级为 chat
                output.status = PlannerStatus::Chat;
                output.chat_reply = Some(format!(
                    "I understood the request, but planning failed: {e}. Please describe what you want more specifically."
                ));
                output.steps.clear();
            }
        }

        Ok(output)
    }

    /// 构建系统 prompt
    ///
    /// **段落顺序按「跨请求是否稳定」排，不按叙事顺序排。** OpenAI 的自动 prompt
    /// caching 和 Gemini 的 context caching 都是前缀匹配：前缀一旦出现差异，后面
    /// 全部无法命中。
    ///
    /// 两段拼接：
    /// - `stable`：身份 / USER.md / 协作团队 / 授予权限过滤后的能力索引 / 规则（索引随 granted 变）
    /// - `volatile`：Merope 称呼 / 自治上限 / 记忆 / 教训 / 推荐 Skill / 执行记录 / 升级上下文 / 环境
    ///
    /// 副作用是语言指令移到了末尾，离模型的输出更近，指令跟随反而更稳。
    async fn build_system_prompt(
        &self,
        request: &UserRequest,
        language: super::intent::keywords::Language,
        escalation_hint: Option<&str>,
        granted: Option<&HashSet<String>>,
    ) -> String {
        let mut stable: Vec<String> = Vec::new();
        let mut volatile: Vec<String> = Vec::new();

        // 1. 身份（Merope 开时为人设，否则 SOUL.md）
        let speaking_soul = crate::services::agent::identity::get_speaking_soul().await;
        let global_identity = identity::get_identity().await;
        let role_prompt = speaking_soul
            .as_deref()
            .or_else(|| global_identity.as_ref().and_then(|id| id.role_prompt()));
        // 无 SOUL.md 时仍写入默认身份段（排进 `stable`）。
        stable.push(match role_prompt {
            Some(role) => format!("## Identity\n{}", role),
            None => {
                "## Identity\nYou are Agent, an AI assistant. You understand natural-language requests and plan steps."
                    .to_string()
            }
        });

        // 1.2. 用户偏好（USER.md）
        if let Some(ref id) = global_identity {
            if let Some(user_ctx) = id.user_context() {
                stable.push(format!("## User preferences\n{}", user_ctx));
            }
        }

        volatile.extend(crate::services::agent::merope::speaking_prompt(request.user_id).await);

        if let Some(cap) = request
            .context
            .as_ref()
            .and_then(|context| context.autonomy_permission_cap.as_ref())
            .filter(|cap| !cap.is_empty())
        {
            volatile.push(format!(
                "## Autonomy cap\nThis turn may only use the intersection of granted permissions and autonomy. Do not plan steps that need other permissions. Allowed:\n{}",
                cap.iter()
                    .map(|permission| format!("- {permission}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }

        // 1.5. 多 Agent 角色概览（注入 worker 身份摘要）
        if let Some(mgr) = identity::get_identity_manager() {
            let summaries = mgr.get_role_summaries().await;
            if !summaries.is_empty() {
                stable.push(format!(
                    "## Team\nYou can dispatch these specialist agents:\n{}",
                    summaries
                ));
            }
        }

        // 2. 记忆系统（语义 + 实体 + 教训）
        if let Some(mem) = memory::get_memory() {
            let mut mem_lines: Vec<String> = Vec::new();

            // 2a. TF-IDF 语义相关记忆（长期+中期）
            let semantic_memories = mem
                .recall_with_params(memory::RecallQuery {
                    query: request.raw_input.clone(),
                    limit: 4,
                    tier_filter: Some(vec![
                        memory::MemoryTier::LongTerm,
                        memory::MemoryTier::MediumTerm,
                    ]),
                    user_id: Some(request.user_id),
                    ..Default::default()
                })
                .await;
            for m in &semantic_memories {
                let tier_tag = match m.tier {
                    memory::MemoryTier::LongTerm => "📌",
                    memory::MemoryTier::MediumTerm => "📝",
                    memory::MemoryTier::ShortTerm => "💬",
                };
                // 计算记忆年龄，帮助 Planner 判断时效性
                let age_tag = chrono::DateTime::parse_from_rfc3339(&m.created_at)
                    .map(|dt| {
                        let days = (chrono::Utc::now() - dt.with_timezone(&chrono::Utc)).num_days();
                        if days <= 1 {
                            String::new()
                        } else if days < 30 {
                            format!(" ({} days ago)", days)
                        } else {
                            format!(" ({} months ago)", days / 30)
                        }
                    })
                    .unwrap_or_default();
                // 截断过长的记忆内容，防止上下文爆炸
                let content: String = m.content.chars().take(300).collect();
                let truncated = if m.content.chars().count() > 300 {
                    format!("{}...", content)
                } else {
                    content
                };
                mem_lines.push(format!("- {}{} {}", tier_tag, age_tag, truncated));
            }

            // 2b. 实体相关记忆（从用户输入提取的实体）
            let entity_memories = mem
                .recall_by_entity(&request.raw_input, 3, request.user_id)
                .await;
            for m in &entity_memories {
                if !semantic_memories.iter().any(|sm| sm.id == m.id) {
                    mem_lines.push(format!("- 🏷️ {}", m.content));
                }
            }

            // 2c. 执行教训（EffectivePattern + ExecutionLesson）
            let lesson_memories = mem
                .recall_with_params(memory::RecallQuery {
                    query: request.raw_input.clone(),
                    limit: 3,
                    type_filter: Some(vec![
                        memory::MemoryType::ExecutionLesson,
                        memory::MemoryType::EffectivePattern,
                    ]),
                    user_id: Some(request.user_id),
                    ..Default::default()
                })
                .await;
            let mut lesson_lines: Vec<String> = Vec::new();
            for m in &lesson_memories {
                if !semantic_memories.iter().any(|sm| sm.id == m.id) {
                    lesson_lines.push(format!("- ⚠️ {}", m.content));
                }
            }

            if !mem_lines.is_empty() {
                volatile.push(format!(
                    "## Reference memory\nHistorical memory for reference only. If the request uses recent / latest / now / 最近 / 最新 / 目前 / 现在, fetch live information. Do not replace it with old conclusions from memory.\n<memory_context>\n{}\n</memory_context>",
                    mem_lines.join("\n")
                ));
            }
            if !lesson_lines.is_empty() {
                volatile.push(format!(
                    "## Notes (lessons)\n<lessons>\n{}\n</lessons>",
                    lesson_lines.join("\n")
                ));
            }
        }

        // 3. 能力索引（授予权限过滤，含 Skill/MCP）
        let compact_index = get_compact_index_for_grants(granted).await;
        stable.push(format!(
            "## Available capabilities (compact index)\n\
             Fields: `id` capability id, `h` purpose, `p` required params, `o` output fields\
             (see the data-flow rule below).\n\
             ```json\n{}\n```",
            serde_json::to_string_pretty(&compact_index).unwrap_or_default()
        ));

        // 4. 输出格式与规则（只由常量组装，稳定段的最后一块）
        stable.push(planner_rules());

        // —— 以下按请求变化，排在缓存前缀之后 ——

        // 5. 相关 Skill 预过滤（子串+分词重叠 top-5，再按 granted 过滤）
        if let Some(registry) = super::skill::get_skill_registry() {
            let relevant = registry.get_relevant_skills(&request.raw_input, 5).await;
            let mut filtered = Vec::new();
            for sm in relevant {
                if super::skill::skill_covered_by_grants(&sm.skill, granted).await {
                    filtered.push(sm);
                }
            }
            if !filtered.is_empty() {
                let skill_lines: Vec<String> = filtered
                    .iter()
                    .map(|sm| {
                        let params_hint = if sm.skill.parameters.is_empty() {
                            String::new()
                        } else {
                            format!(" (params: {})", sm.skill.parameters.join(", "))
                        };
                        format!(
                            "- **skill:{}** (relevance {:.0}%) — {}{}",
                            sm.skill.id,
                            sm.relevance * 100.0,
                            sm.skill.description,
                            params_hint,
                        )
                    })
                    .collect();
                volatile.push(format!(
                    "## Recommended skills\nThese skills match the request closely. Prefer them:\n{}",
                    skill_lines.join("\n")
                ));
            }
        }

        // 6. 近期执行摘要（从对话历史中提取能力使用记录）
        if let Some(ref context) = request.context {
            if let Some(ref history) = context.conversation_history {
                let exec_summary = Self::extract_execution_summary(history);
                if !exec_summary.is_empty() {
                    volatile.push(format!("## Execution log this turn\n{}", exec_summary));
                }
            }
        }

        // 7. 升级提示
        if let Some(hint) = escalation_hint {
            volatile.push(format!(
                "## Escalation context\nThe previous run was not good enough. {}",
                hint
            ));
        }

        // 8. 环境上下文（时间、语言）——放在最后：时间戳每分钟都变，
        // 排在前面会让后续所有内容都失去缓存前缀；放末尾还能让语言指令离输出更近
        let now = chrono::Local::now();
        let lang_instruction = match language {
            super::intent::keywords::Language::Chinese => "Reply in Chinese.",
            super::intent::keywords::Language::English => "Please respond in English.",
            super::intent::keywords::Language::Japanese => "Reply in Japanese.",
        };
        volatile.push(format!(
            "## Environment\nCurrent time: {}\n{}",
            now.format("%Y-%m-%d %H:%M (%A)"),
            lang_instruction
        ));

        stable
            .into_iter()
            .chain(volatile)
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// 构建用户 prompt（使用结构化边界防止提示词注入）
    fn build_user_prompt(&self, request: &UserRequest, escalation_hint: Option<&str>) -> String {
        let mut prompt = format!("<user_request>\n{}\n</user_request>", request.raw_input);

        if let Some(context) = &request.context {
            if let Some(route) = &context.current_route {
                let page_desc = describe_route(route);
                prompt.push_str(&format!("\nCurrent page: {} ({})", page_desc, route));
            }
            if !context.active_platforms.is_empty() {
                prompt.push_str(&format!(
                    "\nActive platforms: {}",
                    context.active_platforms.join(", ")
                ));
            }

            // 对话历史来自 `context.conversation_history`。
            if let Some(history) = &context.conversation_history {
                if !history.is_empty() {
                    prompt.push_str("\n\n<conversation_history>");
                    for msg in history
                        .iter()
                        .rev()
                        .take(20)
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                    {
                        prompt.push_str(&format!("\n{}：{}", msg.role, msg.content));
                    }
                    prompt.push_str("\n</conversation_history>");
                    prompt.push_str(
                        "\n\nNote: the user may be referring to earlier turns. Resolve pronouns from that context.",
                    );
                }
            }

            // 页面上下文
            if let Some(custom_data) = &context.custom_data {
                if let Some(page) = custom_data.get("pageContent") {
                    if let Some(title) = page.get("title").and_then(|v| v.as_str()) {
                        let title: String = title.chars().take(120).collect();
                        if !title.trim().is_empty() {
                            prompt.push_str(&format!("\nLooking at: {}", title.trim()));
                        }
                    }
                    prompt.push_str("\nPage context available: true. Summarize/analyze with \"contentFrom\": \"__page_context__\"; page.content / page.understand with \"contextFrom\": \"__page_context__\" (do not write inputFrom)");
                }
                if let Some(now_playing) = now_playing_line(custom_data.get("musicStatus")) {
                    prompt.push_str(&format!("\n{now_playing}"));
                }

                if let Some(attachments) = custom_data.get("attachments").and_then(|v| v.as_array())
                {
                    if !attachments.is_empty() {
                        prompt.push_str("\n\n<user_attachments>");
                        for att in attachments {
                            let name = att.get("name").and_then(|v| v.as_str()).unwrap_or("file");
                            let mime = att.get("mime").and_then(|v| v.as_str()).unwrap_or("");
                            let size = att.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
                            prompt.push_str(&format!("\n- {} ({}, {} bytes)", name, mime, size));
                            if let Some(text) = att.get("text").and_then(|v| v.as_str()) {
                                if !text.is_empty() {
                                    prompt.push_str("\n  <excerpt>\n");
                                    prompt.push_str(text);
                                    prompt.push_str("\n  </excerpt>");
                                }
                            } else if mime.starts_with("image/") {
                                prompt
                                    .push_str("\n  (image pixels were not sent; only the file name and type are visible)");
                            }
                        }
                        prompt.push_str("\n</user_attachments>");
                    }
                }

                // 用户偏好
                if let Some(prefs) = custom_data.get("user_preferences") {
                    if let Some(prefs_obj) = prefs.as_object() {
                        if !prefs_obj.is_empty() {
                            prompt.push_str("\n\nUser history preferences:");
                            if let Some(action) =
                                prefs_obj.get("preferred_action").and_then(|v| v.as_str())
                            {
                                prompt.push_str(&format!("\n- Usual action: {}", action));
                            }
                            if let Some(platforms) = prefs_obj
                                .get("preferred_platforms")
                                .and_then(|v| v.as_array())
                            {
                                let names: Vec<&str> =
                                    platforms.iter().filter_map(|p| p.as_str()).collect();
                                if !names.is_empty() {
                                    prompt.push_str(&format!(
                                        "\n- Usual platforms: {}",
                                        names.join(", ")
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }

        if let Some(hint) = escalation_hint {
            prompt.push_str(&format!("\n\n[Escalation hint] {}", hint));
        }

        prompt
    }

    /// 解析 AI 响应为 PlannerOutput
    fn parse_response(&self, response: &str) -> Result<PlannerOutput, String> {
        let trimmed = response.trim();

        // 提取 JSON（处理 markdown fence）
        let json_str = if trimmed.starts_with("```") {
            let start = trimmed
                .find('{')
                .ok_or("No JSON object found in AI response")?;
            let end = trimmed
                .rfind('}')
                .ok_or("No closing brace found in AI response")?;
            &trimmed[start..=end]
        } else if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}')) {
            &trimmed[start..=end]
        } else {
            // 纯文本回复 → 作为 chat
            return Ok(PlannerOutput {
                status: PlannerStatus::Chat,
                confidence: 0.9,
                reasoning: None,
                steps: vec![],
                clarification: None,
                unsupported_reason: None,
                chat_reply: Some(trimmed.to_string()),
            });
        };

        match serde_json::from_str::<PlannerOutput>(json_str) {
            Ok(output) => Ok(output),
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    preview = &response[..response.len().min(200)],
                    "[Planner] Failed to parse response as PlannerOutput"
                );
                // 解析失败 → 作为 chat 回复
                Ok(PlannerOutput {
                    status: PlannerStatus::Chat,
                    confidence: 0.5,
                    reasoning: None,
                    steps: vec![],
                    clarification: None,
                    unsupported_reason: None,
                    chat_reply: Some(trimmed.to_string()),
                })
            }
        }
    }
}

/// Whether a recipe/planner step is covered by the current grant set.
pub(crate) async fn capability_allowed_for_grants(
    capability_id: &str,
    params: &HashMap<String, serde_json::Value>,
    granted: &HashSet<String>,
) -> Result<(), String> {
    if let Some(skill_id) = capability_id.strip_prefix("skill:") {
        let allowed = match super::skill::get_skill_registry() {
            Some(registry) => match registry.get(skill_id).await {
                Some(skill) => super::skill::skill_covered_by_grants(&skill, Some(granted)).await,
                None => false,
            },
            None => false,
        };
        if !allowed {
            return Err(format!("capability '{capability_id}' is not available"));
        }
        return Ok(());
    }
    if capability_id.starts_with("mcp.") {
        if !granted.contains("mcp:execute") {
            return Err(format!("capability '{capability_id}' is not available"));
        }
        return Ok(());
    }
    match get_capabilities_by_ids(&[capability_id.to_string()])
        .await
        .into_iter()
        .next()
    {
        Some(cap) => {
            if !capability_covered_by_grants(&cap, Some(granted)) {
                return Err(format!("capability '{capability_id}' is not available"));
            }
        }
        None => {
            return Err(format!("capability '{capability_id}' is not available"));
        }
    }
    if capability_id == "scheduler.create" {
        crate::services::agent::scheduler_create_actions_within_grants(params, granted)?;
    }
    Ok(())
}

impl Planner {
    /// 校验步骤（加载完整 capability schema 验证）
    async fn validate_steps(
        &self,
        output: &mut PlannerOutput,
        autonomy_cap: Option<&[String]>,
        granted: Option<&HashSet<String>>,
    ) -> Result<(), String> {
        let cap_ids: Vec<String> = output
            .steps
            .iter()
            .map(|s| s.capability_id.clone())
            .collect();
        let cap_schemas = get_capabilities_by_ids(&cap_ids).await;

        // 尝试用公共函数验证
        let test_steps = output.steps.clone();
        let reasoning = output.reasoning.clone();
        validate_and_convert_steps(test_steps, reasoning, &cap_schemas)?;

        if let Some(granted) = granted {
            for step in &output.steps {
                capability_allowed_for_grants(&step.capability_id, &step.params, granted).await?;
            }
        }

        if autonomy_cap.is_some() {
            for capability in &cap_schemas {
                if !crate::services::agent::consciousness::required_permissions_within_cap(
                    &capability.required_permissions,
                    autonomy_cap,
                ) {
                    return Err(format!(
                        "capability '{}' exceeds the autonomy permission cap",
                        capability.id
                    ));
                }
            }
        }

        Ok(())
    }

    /// 无 AI 时的规则兜底
    fn fallback_plan(&self, request: &UserRequest) -> PlannerOutput {
        let input = request.raw_input.to_lowercase();

        // 简单的关键词匹配
        if input.contains("你好")
            || input.contains("hello")
            || input.contains("hi")
            || input.contains("嗨")
        {
            return PlannerOutput {
                status: PlannerStatus::Chat,
                confidence: 0.9,
                reasoning: Some("Simple greeting".to_string()),
                steps: vec![],
                clarification: None,
                unsupported_reason: None,
                chat_reply: Some(crate::services::agent::response_agent::greeting()),
            };
        }

        // 默认：用 ai.chat 能力作为兜底
        PlannerOutput {
            status: PlannerStatus::Plan,
            confidence: 0.5,
            reasoning: Some("Fallback: no AI available, routing to ai.chat".to_string()),
            steps: vec![AiRecipeStep {
                id: "step_1".to_string(),
                capability_id: "ai.chat".to_string(),
                action: "chat".to_string(),
                params: {
                    let mut p = HashMap::new();
                    p.insert(
                        "message".to_string(),
                        serde_json::Value::String(request.raw_input.clone()),
                    );
                    p
                },
                depends_on: vec![],
                on_failure: "abort".to_string(),
                retry: None,
                timeout_ms: Some(300_000),
            }],
            clarification: None,
            unsupported_reason: None,
            chat_reply: None,
        }
    }

    /// 从对话历史中提取近期执行摘要
    ///
    /// 扫描 assistant 消息中的执行结果标记，构建简洁的能力使用记录。
    /// 帮助 Planner 了解当前会话中已经执行过什么、结果如何。
    fn extract_execution_summary(history: &[ConversationMessage]) -> String {
        let mut summaries: Vec<String> = Vec::new();

        // 仅检查最近 10 条 assistant 消息
        for msg in history
            .iter()
            .rev()
            .filter(|m| m.role == "assistant")
            .take(10)
        {
            let content = &msg.content;
            // 检测常见的执行结果标记词
            let has_exec_markers = content.contains("执行")
                || content.contains("获取")
                || content.contains("生成")
                || content.contains("分析")
                || content.contains("搜索")
                || content.contains("completed")
                || content.contains("failed")
                || content.contains("Got ")
                || content.contains("Searched")
                || content.contains("Generated")
                || content.contains("Analyzed");

            if has_exec_markers && content.len() > 10 {
                // 截取摘要（最多 120 字符）
                let preview: String = content.chars().take(120).collect();
                let suffix = if content.chars().count() > 120 {
                    "..."
                } else {
                    ""
                };
                summaries.push(format!("- {}{}", preview, suffix));
            }
        }

        summaries.reverse();
        // 最多保留最近 5 条
        if summaries.len() > 5 {
            summaries.drain(..summaries.len() - 5);
        }
        summaries.join("\n")
    }
}

/// 结构化输出的 schema 名（OpenAI `response_format.json_schema.name`）
const PLANNER_SCHEMA_NAME: &str = "planner_output";

/// [`PlannerOutput`] 的 JSON Schema，交给提供商做结构化输出约束。
///
/// 字段名与 `PlannerOutput` / `AiRecipeStep` 的 serde 表示一一对应（两者都用
/// Rust 字段名，未做 rename）。
///
/// `steps[].params` 是自由 map，这里不穷举；`validate_and_convert_steps` 对 input_schema 只 warn，缺参不失败。
/// 这份 schema 无法翻译成 Gemini 的 `responseSchema` 方言（Gemini 不接受没有 `properties` 的
/// OBJECT），Gemini 上只生效 `responseMimeType: application/json`；OpenAI 侧走
/// 非 strict 的 `json_schema`，两边都能保证返回是合法 JSON。
fn planner_output_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "status": {
                "type": "string",
                "enum": ["plan", "clarify", "unsupported", "chat"],
                "description": "Classified request type"
            },
            "confidence": {
                "type": "number",
                "minimum": 0.0,
                "maximum": 1.0
            },
            "reasoning": {
                "type": "string",
                "description": "Brief rationale"
            },
            "steps": {
                "type": "array",
                "description": format!("Steps when status=plan, at most {MAX_PLAN_STEPS}"),
                "maxItems": MAX_PLAN_STEPS,
                "items": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string" },
                        "capability_id": {
                            "type": "string",
                            "description": "Must be an id from the available-capability index"
                        },
                        "action": {
                            "type": "string",
                            "description": "Concrete command for this step"
                        },
                        "params": {
                            "type": "object",
                            "description": "Capability params; cite a prior step with xxxFrom: \"step_id.field\""
                        },
                        "depends_on": {
                            "type": "array",
                            "items": { "type": "string" }
                        },
                        "on_failure": {
                            "type": "string",
                            "enum": ["abort", "skip"]
                        },
                        "retry": {
                            "type": "object",
                            "properties": {
                                "max_attempts": { "type": "integer" },
                                "delay_ms": { "type": "integer" },
                                "exponential_backoff": { "type": "boolean" }
                            },
                            "required": ["max_attempts", "delay_ms", "exponential_backoff"]
                        },
                        "timeout_ms": { "type": "integer" }
                    },
                    "required": ["id", "capability_id", "action", "params", "depends_on"]
                }
            },
            "clarification": {
                "type": "object",
                "description": "Question when status=clarify",
                "properties": {
                    "message": { "type": "string" },
                    "options": {
                        "type": "array",
                        "items": { "type": "string" }
                    }
                },
                "required": ["message"]
            },
            "unsupported_reason": { "type": "string" },
            "chat_reply": { "type": "string" }
        },
        "required": ["status", "confidence"]
    })
}

fn now_playing_line(music: Option<&serde_json::Value>) -> Option<String> {
    let music = music?;
    let song = music.get("currentSong")?;
    if song.is_null() {
        return None;
    }
    let name: String = song
        .get("name")
        .or_else(|| song.get("title"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .chars()
        .take(80)
        .collect();
    let artist: String = song
        .get("artist")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .chars()
        .take(80)
        .collect();
    if name.trim().is_empty() {
        return None;
    }
    let playing = music
        .get("isPlaying")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let label = if playing { "Playing" } else { "Paused" };
    Some(if artist.trim().is_empty() {
        format!("{label}：{}", name.trim())
    } else {
        format!("{label}：{} — {}", name.trim(), artist.trim())
    })
}

/// 路由描述
fn describe_route(route: &str) -> &'static str {
    match route.trim_matches('/') {
        "brew" => "Brew feeds page",
        "tapp" | "tapps" => "Tapp apps page",
        "library" => "Library page",
        "config" | "settings" => "Settings page",
        route if route.starts_with("platform/bilibili") => "Bilibili data page",
        route if route.starts_with("platform/steam") => "Steam games page",
        route if route.starts_with("platform/github") => "GitHub page",
        route if route.starts_with("platform/netease") => "NetEase Music page",
        route if route.starts_with("platform/") => "Platform page",
        "report" | "reports" => "Reports page",
        "" | "dashboard" => "Home dashboard",
        _ => "Unknown page",
    }
}

/// 把共享的引擎契约填进 Planner 规则模板。
///
/// 结果只由常量决定，所以仍然逐字稳定，能进 provider 的前缀缓存。
fn planner_rules() -> String {
    PLANNER_RULES_TEMPLATE
        .replace("{{DEPENDENCY}}", PLAN_DEPENDENCY_RULE)
        .replace("{{DATA_FLOW}}", PLAN_DATA_FLOW_RULE)
        .replace("{{STEP_CAP}}", &plan_step_cap_rule())
        .replace("{{IMAGE_SIZE}}", &plan_image_size_rule())
}

/// Planner 规则与输出格式（注入到系统 prompt）。
///
/// 四块 `{{…}}` 由 planner_rules() 用 plan_contract 常量替换；DAG 提示词必须引用同一份字。
const PLANNER_RULES_TEMPLATE: &str = r#"## Rules

Analyze the user request, classify it, and output the matching JSON.

### Request type

1. **plan**: the user needs an action (query data, generate content, control a feature, …) → output steps
2. **chat**: small talk, a question, or anything that needs no capability → reply directly
3. **clarify**: the request is too vague to act on → ask for clarification
4. **unsupported**: the request is out of scope → explain why

### Ask-me-first (critical)

When the user **explicitly asks you to question them or seek their opinion** ("问我", "问问我", "你问一下我", "让我选", "给我选项", "我来决定", "先问问我的意见", "ask me", "let me choose"), **you must use clarify**, never chat.
- These phrases mean the user wants to take part in the decision; they are not small talk
- Write the concrete question in clarification.message
- Put 3–5 reasonable options in clarification.options
- Ground the question and options in conversation history and the last run; do not ask vaguely

### Step rules (status=plan)

1. `capability_id` must match an id in the available-capability index. Include every required param listed in `"p"`
2. If a `skill:xxx` capability matches the intent, prefer that Skill (it already wraps a multi-step flow)
3. **Call each Skill at most once.** The same `skill:xxx` may appear only once in the whole plan. If the user wants several images / variants / a batch, pass quantity through Skill params (e.g. `"count": 3`, `"variations": ["scene A", "scene B"]`) and let the Skill orchestrate internally. **Never** repeat the same Skill as multiple steps. Example: "帮我生成一些XX图片" → one `skill:xxx` step with `"count": 3`, not three Skill steps
4. Infer `params` from the capability description and the `"p"` list
5. If page context is available: summarize/analyze with `"contentFrom": "__page_context__"`; `page.content` / `page.understand` with `"contextFrom": "__page_context__"` (do not write `inputFrom`; those capabilities read `context`)
6. `on_failure`:
   - data-fetch steps: `"abort"` (later steps need the data)
   - AI steps: `"skip"` is allowed (non-critical analysis/summary)
   - if this step is a `depends_on` source for others, it must `"abort"`
7. `timeout_ms`: data fetch 15000, AI 300000, image generation 900000
8. Optional: `"retry": {"max_attempts": 2, "delay_ms": 1000, "exponential_backoff": true}` — add this on network-fetch steps
9. Optional: `"model_tier": "pro"` — set pro for high-quality analysis/creation; omit for ordinary tasks

### Data flow (xxxFrom)

{{DATA_FLOW}}

### Keep the step count down (critical)

- {{STEP_CAP}}
- Most requests should finish in 1–3 steps
- **Simple** (query, search, one image, Q&A) → 1–2 steps
- **Medium** (search+analyze, fetch+summarize) → 2–3 steps
- **Complex** (multi-platform compare, multi-step workflow) → 3–6 steps
- Every step must have a distinct, irreplaceable job. Do not add steps for form
- **If one capability can do it, do not split it**
- step.action is your direct command for that step; keep it concrete and tied to the request

### Quantity

When the request contains a quantity:
- **Explicit singular** ("一张", "一个", "one") → 1 step
- **No quantity or a vague word** ("帮我生成图", "一些", "几个", "some") → **default to one**; do not expand into several yourself

### Dependencies and parallelism

{{DEPENDENCY}}

Only when the user **explicitly lists** several targets ("B站和Steam", "search X and Y") emit one step per target.
- "所有平台" / "all platforms" → expand at most 3 main platforms
- If a later step needs to merge them, that step `depends_on` every parallel step

### Compare intent

When the user wants a comparison, PK, or "which is better" ("对比", "比一比", "哪个更好", "有什么区别"), the plan must include:
1. **Parallel fetches**: one data-fetch step per target, `depends_on: []`
2. **Compare step**: one `compare.content` or `ai.analyze` (analysisType="custom") step that `depends_on` every fetch and uses those results as input

### Sequence (A then B)

When one sentence chains actions ("搜索并总结", "找到后订阅", "获取数据然后分析", "翻译完再朗读"), split into **dependent** steps:
- Earlier steps fetch or transform data
- Later steps `depends_on` those ids and pass data with `"xxxFrom": "step_id"`
- Do not collapse into one step; each verb is one capability call

### Time phrases

Map time modifiers to params (`since`, `daysBack`, …):
- “最近” / “近期” / recent → `daysBack: 7`
- “今天” / today → `since` midnight today
- “本周” / this week → `daysBack: 7`
- “本月” / this month → `daysBack: 30`
- no time phrase → defaults

### Follow-ups / edits

When the user follows up ("换个XX", "再来一个", "改一下", "不满意", "不够XX"):
- Read intent from conversation history
- Feedback without a concrete direction → status="clarify" with edit options
- Feedback plus ask-me-first ("问我", "你问问我") → status="clarify", you must ask
- Only plan directly (status="plan") when they named a concrete change

### Pronouns and context

When the user uses a pronoun (“这个”, “它”, “刚才那个”, "this", "it"), resolve it from page context and history.
If you cannot, status="clarify".

### Image generation (prompt.generate + ai.image)

When generating an image, put a detailed description in `prompt.generate`'s `description`:
- Known characters need full visual traits (hair, eyes, outfit, signature details)
- Expand nicknames/short names into a full character description
- Include scene, mood, composition
- Never a short title only

{{IMAGE_SIZE}}

`prompt.generate` sources also use `xxxFrom`: `descriptionFrom` fills description, `titleFrom` fills title.

Typical chain (search → analyze → prompt → image):
```
search(ai.webSearch) → analyze(ai.analyze, dataFrom:"search") → gen_prompt(prompt.generate, descriptionFrom:"analyze") → image(ai.image, promptFrom:"gen_prompt", width?, height?)
```

### Output format

Output JSON only. No markdown fences:

```
{
  "status": "plan" | "clarify" | "unsupported" | "chat",
  "confidence": 0.0-1.0,
  "reasoning": "brief rationale",

  // status=plan (example: search→analyze→prompt→image):
  "steps": [
    {
      "id": "search",
      "capability_id": "ai.webSearch",
      "action": "Search for the character",
      "params": { "query": "..." },
      "depends_on": [],
      "on_failure": "abort",
      "timeout_ms": 15000
    },
    {
      "id": "analyze",
      "capability_id": "ai.analyze",
      "action": "Introduce the character",
      "params": { "dataFrom": "search", "instruction": "Introduce this character from the search results..." },
      "depends_on": ["search"],
      "on_failure": "skip",
      "timeout_ms": 300000
    },
    {
      "id": "gen_prompt",
      "capability_id": "prompt.generate",
      "action": "Write an image prompt for the character",
      "params": { "descriptionFrom": "analyze" },
      "depends_on": ["analyze"],
      "timeout_ms": 15000
    },
    {
      "id": "gen_image",
      "capability_id": "ai.image",
      "action": "Generate the character image",
      "params": { "promptFrom": "gen_prompt", "width": 768, "height": 1024 },
      "depends_on": ["gen_prompt"],
      "timeout_ms": 900000
    }
  ],

  // status=clarify:
  "clarification": {
    "message": "the question to ask",
    "options": ["option 1", "option 2"]
  },

  // status=unsupported:
  "unsupported_reason": "why this is unsupported",

  // status=chat:
  "chat_reply": "the spoken reply"
}
```"#;

#[cfg(test)]
mod tests {
    use super::*;

    /// A full planner answer, as the model is asked to produce it.
    fn plan_response() -> serde_json::Value {
        serde_json::json!({
            "status": "plan",
            "confidence": 0.9,
            "reasoning": "搜索后分析",
            "steps": [{
                "id": "search",
                "capability_id": "ai.webSearch",
                "action": "搜索角色信息",
                "params": { "query": "x" },
                "depends_on": [],
                "on_failure": "abort",
                "timeout_ms": 15000
            }, {
                "id": "analyze",
                "capability_id": "ai.analyze",
                "action": "分析结果",
                "params": { "dataFrom": "search.results" },
                "depends_on": ["search"],
                "on_failure": "skip",
                "retry": {
                    "max_attempts": 2,
                    "delay_ms": 1000,
                    "exponential_backoff": true
                }
            }]
        })
    }

    #[test]
    fn schema_matches_what_planner_output_deserializes() {
        // The schema is handed to the provider as the response contract, so a
        // document that satisfies it must also deserialize into PlannerOutput.
        let parsed: PlannerOutput =
            serde_json::from_value(plan_response()).expect("schema-shaped response must parse");
        assert_eq!(parsed.status, PlannerStatus::Plan);
        assert_eq!(parsed.steps.len(), 2);
        assert_eq!(parsed.steps[1].depends_on, vec!["search".to_string()]);
        assert_eq!(
            parsed.steps[1]
                .params
                .get("dataFrom")
                .and_then(|v| v.as_str()),
            Some("search.results")
        );
        assert!(parsed.steps[1].retry.is_some());
    }

    #[test]
    fn schema_declares_every_field_planner_output_reads() {
        let schema = planner_output_schema();
        let properties = schema["properties"]
            .as_object()
            .expect("object schema with properties");
        for field in [
            "status",
            "confidence",
            "reasoning",
            "steps",
            "clarification",
            "unsupported_reason",
            "chat_reply",
        ] {
            assert!(
                properties.contains_key(field),
                "missing `{field}` in schema"
            );
        }

        let step = &schema["properties"]["steps"]["items"]["properties"];
        for field in [
            "id",
            "capability_id",
            "action",
            "params",
            "depends_on",
            "on_failure",
            "retry",
            "timeout_ms",
        ] {
            assert!(
                step.get(field).is_some(),
                "missing step field `{field}` in schema"
            );
        }
    }

    #[test]
    fn schema_status_enum_covers_every_planner_status() {
        let declared = planner_output_schema()["properties"]["status"]["enum"].clone();
        let declared = declared.as_array().expect("status enum");
        for status in [
            PlannerStatus::Plan,
            PlannerStatus::Clarify,
            PlannerStatus::Unsupported,
            PlannerStatus::Chat,
        ] {
            let serialized = serde_json::to_value(&status).expect("serialize status");
            assert!(
                declared.contains(&serialized),
                "status {serialized} missing from the schema enum"
            );
        }
        assert_eq!(
            declared.len(),
            4,
            "schema enum has drifted from PlannerStatus"
        );
    }

    /// 模板里的四个占位符必须全部被填掉。漏一个就会把 `{{DATA_FLOW}}` 这种
    /// 字面量发给模型。
    #[test]
    fn planner_rules_fill_every_shared_slot() {
        let rules = planner_rules();
        assert!(!rules.contains("{{"), "unfilled slot left in PLANNER_RULES");
        assert!(rules.contains(PLAN_DEPENDENCY_RULE));
        assert!(rules.contains(PLAN_DATA_FLOW_RULE));
        assert!(rules.contains(&plan_step_cap_rule()));
        assert!(rules.contains(&plan_image_size_rule()));
    }

    /// Planner 和 Skill 内部 DAG 面对同一个引擎，调度语义必须逐字同一份。
    /// DAG 提示词必须引用共享契约槽，不得把契约正文再抄一遍。
    #[test]
    fn both_plan_prompts_quote_the_same_engine_contract() {
        let dag = include_str!("executor/execute_step.rs");
        for slot in ["{dependency}", "{data_flow}", "{image_size}", "{step_cap}"] {
            assert!(dag.contains(slot), "DAG 提示词没有引用共享契约的 {slot}");
        }
        for restated in [
            "黄金法则",
            "整数像素 256",
            "$$variable$$ 语法或模板占位符",
            "最多 8 个步骤",
            "The engine schedules by",
            "At most 8 steps",
            "Never use `$$variable$$`",
        ] {
            assert!(
                !dag.contains(restated),
                "DAG 提示词又把共享契约抄了一遍：{restated}"
            );
        }
        let template = PLANNER_RULES_TEMPLATE;
        for restated in [
            "256–2048",
            "绝对上限 8 个",
            "引擎自动并行",
            "The engine schedules by",
        ] {
            assert!(
                !template.contains(restated),
                "PLANNER_RULES 又把共享契约抄了一遍：{restated}"
            );
        }
    }

    /// 断言落在组装完的整份提示词上（能力索引的 `o` 字段引用也在同一份里）。
    #[tokio::test]
    async fn the_assembled_prompt_states_each_shared_rule_once() {
        let planner = test_planner();
        let prompt = planner
            .build_system_prompt(
                &request("帮我搜一下再总结"),
                crate::services::agent::intent::keywords::Language::Chinese,
                None,
                None,
            )
            .await;
        for fragment in [PLAN_DEPENDENCY_RULE, PLAN_DATA_FLOW_RULE] {
            assert_eq!(
                prompt.matches(fragment).count(),
                1,
                "共享规则在整份提示词里出现了不止一次"
            );
        }
        assert_eq!(
            prompt.matches("search.results").count(),
            1,
            "「按 o 写精确字段」这条在整份提示词里被写了不止一次"
        );
    }

    /// 步骤上限在提示词、schema 和真正截断的地方是同一个数。
    #[test]
    fn every_step_cap_comes_from_one_constant() {
        assert_eq!(
            planner_output_schema()["properties"]["steps"]["maxItems"],
            MAX_PLAN_STEPS
        );
        assert!(
            include_str!("recipe.rs").contains("MAX_PLAN_STEPS as MAX_STEPS"),
            "recipe.rs 的截断必须用共享常量，不能再私有一个 8"
        );
        assert!(
            include_str!("executor/execute_step.rs").contains("take(MAX_PLAN_STEPS)"),
            "DAG 的截断必须用共享常量"
        );
    }

    #[test]
    fn schema_step_cap_matches_the_planner_rules() {
        assert_eq!(
            planner_output_schema()["properties"]["steps"]["maxItems"],
            8
        );
    }

    fn test_planner() -> Planner {
        Planner {
            ai_analyzer: None,
            language_detector: crate::services::agent::intent::keywords::LanguageDetector::new(),
        }
    }

    fn request(raw_input: &str) -> UserRequest {
        UserRequest {
            raw_input: raw_input.to_string(),
            timestamp: chrono::Utc::now(),
            user_id: 1,
            context: None,
        }
    }

    /// Longest shared prefix, which is exactly what a provider's prefix cache
    /// can reuse between two requests.
    fn shared_prefix<'a>(a: &'a str, b: &str) -> &'a str {
        let shared = a
            .char_indices()
            .zip(b.chars())
            .take_while(|((_, x), y)| x == y)
            .last()
            .map(|((index, ch), _)| index + ch.len_utf8())
            .unwrap_or(0);
        &a[..shared]
    }

    #[tokio::test]
    async fn stable_sections_come_before_anything_request_specific() {
        let planner = test_planner();
        let chinese = planner
            .build_system_prompt(
                &request("帮我看看最新的订阅文章"),
                crate::services::agent::intent::keywords::Language::Chinese,
                None,
                None,
            )
            .await;
        let english = planner
            .build_system_prompt(
                &request("summarize my newest feed items"),
                crate::services::agent::intent::keywords::Language::English,
                None,
                None,
            )
            .await;

        // Two unrelated requests must still share the whole stable block, or the
        // capability index and the rules never reach a provider prefix cache.
        let prefix = shared_prefix(&chinese, &english);
        assert!(prefix.contains("## Identity"), "identity must be cacheable");
        assert!(
            prefix.contains("## Available capabilities (compact index)"),
            "the capability index is the largest block and must be cacheable"
        );
        assert!(
            prefix.contains("## Rules"),
            "PLANNER_RULES must be cacheable"
        );
        // The language instruction is what diverges, and it belongs after them.
        assert!(!prefix.contains("Reply in Chinese."));
    }

    #[tokio::test]
    async fn volatile_sections_come_after_the_stable_block() {
        let planner = test_planner();
        let prompt = planner
            .build_system_prompt(
                &request("换一个说法"),
                crate::services::agent::intent::keywords::Language::Chinese,
                Some("上次没有找到数据"),
                None,
            )
            .await;

        let rules = prompt.find("## Rules").expect("rules section");
        let escalation = prompt
            .find("## Escalation context")
            .expect("escalation section");
        let environment = prompt.find("## Environment").expect("environment section");
        let capabilities = prompt
            .find("## Available capabilities (compact index)")
            .expect("capability index");

        assert!(capabilities < rules, "index precedes rules");
        assert!(rules < escalation, "escalation is request-specific");
        assert!(escalation < environment, "the timestamp goes last");
    }

    #[tokio::test]
    async fn identity_falls_back_when_no_soul_file_is_loaded() {
        // 无 SOUL.md 时 prompt 仍含默认身份段。
        let prompt = test_planner()
            .build_system_prompt(
                &request("你好"),
                crate::services::agent::intent::keywords::Language::Chinese,
                None,
                None,
            )
            .await;
        assert!(prompt.contains("## Identity"));
        assert!(
            prompt.contains("You are Agent"),
            "identity names the product Agent"
        );
    }

    #[test]
    fn user_prompt_includes_attachment_excerpts() {
        let prompt = test_planner().build_user_prompt(
            &UserRequest {
                raw_input: "看看这个".to_string(),
                timestamp: chrono::Utc::now(),
                user_id: 1,
                context: Some(RequestContext {
                    custom_data: Some(serde_json::json!({
                        "attachments": [
                            {
                                "name": "n.txt",
                                "mime": "text/plain",
                                "size": 4,
                                "text": "hello"
                            },
                            {
                                "name": "a.png",
                                "mime": "image/png",
                                "size": 12
                            }
                        ]
                    })),
                    ..Default::default()
                }),
            },
            None,
        );
        assert!(prompt.contains("<user_attachments>"));
        assert!(prompt.contains("n.txt"));
        assert!(prompt.contains("hello"));
        assert!(prompt.contains("a.png"));
        assert!(prompt.contains("image pixels were not sent"));
    }

    #[test]
    fn user_prompt_names_page_title_and_now_playing() {
        let prompt = test_planner().build_user_prompt(
            &UserRequest {
                raw_input: "这首和歌呢".to_string(),
                timestamp: chrono::Utc::now(),
                user_id: 1,
                context: Some(RequestContext {
                    custom_data: Some(serde_json::json!({
                        "pageContent": {
                            "type": "brew_article",
                            "title": "Harbour Notes",
                            "content": "long body",
                            "sourceUrl": "https://example.test/secret"
                        },
                        "musicStatus": {
                            "isPlaying": true,
                            "currentSong": {
                                "name": "Night",
                                "artist": "Lantern",
                                "url": "https://example.test/secret.mp3"
                            }
                        }
                    })),
                    ..Default::default()
                }),
            },
            None,
        );
        assert!(prompt.contains("Looking at: Harbour Notes"));
        assert!(prompt.contains("Page context available: true"));
        assert!(prompt.contains("Playing：Night — Lantern"));
        assert!(!prompt.contains("example.test"));
        assert!(!prompt.contains("long body"));
    }

    #[test]
    fn structured_response_needs_no_brace_scraping() {
        // With provider-enforced JSON the response is the document itself.
        let planner = Planner {
            ai_analyzer: None,
            language_detector: crate::services::agent::intent::keywords::LanguageDetector::new(),
        };
        let raw = serde_json::to_string(&plan_response()).expect("serialize");
        let parsed = planner.parse_response(&raw).expect("parse");
        assert_eq!(parsed.status, PlannerStatus::Plan);
        assert_eq!(parsed.steps.len(), 2);
    }

    #[test]
    fn prose_wrapped_json_still_parses_on_the_fallback_path() {
        // Endpoints that reject response_format fall back to prompt-only mode,
        // where the model may still wrap the document in a fence.
        let planner = Planner {
            ai_analyzer: None,
            language_detector: crate::services::agent::intent::keywords::LanguageDetector::new(),
        };
        let raw = format!(
            "好的，这是计划：\n```json\n{}\n```",
            serde_json::to_string(&plan_response()).expect("serialize")
        );
        let parsed = planner.parse_response(&raw).expect("parse");
        assert_eq!(parsed.status, PlannerStatus::Plan);
    }

    #[tokio::test]
    async fn capability_allowed_for_grants_hides_ungranted_tools() {
        let mut granted = HashSet::new();
        granted.insert("ai:chat".to_string());
        assert!(
            capability_allowed_for_grants("ai.chat", &HashMap::new(), &granted)
                .await
                .is_ok()
        );
        assert!(
            capability_allowed_for_grants("speech.tts", &HashMap::new(), &granted)
                .await
                .is_err()
        );
        assert!(
            capability_allowed_for_grants("not.a.tool", &HashMap::new(), &granted)
                .await
                .is_err()
        );
    }
}
