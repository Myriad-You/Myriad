//! Planner 模块
//!
//! 合并意图分析 + 方案生成为单次 Pro AI 调用。
//! 直接输出可执行的 Recipe 步骤。

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

/// Planner — 单次 Pro AI 调用完成意图理解 + 执行规划
pub struct Planner {
    /// Pro 层级 AI 分析器
    ai_analyzer: Option<AiAnalyzer>,
    /// 语言检测器
    language_detector: LanguageDetector,
}

impl Planner {
    /// 创建 Planner（使用 Pro 层级）
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

    /// 主入口：用户请求 → PlannerOutput
    #[allow(dead_code)]
    pub async fn plan(&self, request: &UserRequest) -> Result<PlannerOutput, String> {
        self.plan_internal(request, None, None, None).await
    }

    pub async fn plan_for(
        &self,
        request: &UserRequest,
        granted: &HashSet<String>,
    ) -> Result<PlannerOutput, String> {
        self.plan_internal(request, None, None, Some(granted)).await
    }

    /// Live SSE path: reasoning deltas go out while the planner JSON is still forming.
    #[allow(dead_code)]
    pub async fn plan_with_progress(
        &self,
        request: &UserRequest,
        progress_tx: &tokio::sync::mpsc::Sender<AgentProgressEvent>,
    ) -> Result<PlannerOutput, String> {
        self.plan_internal(request, None, Some(progress_tx), None)
            .await
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

    #[allow(dead_code)]
    pub async fn replan_with_progress(
        &self,
        request: &UserRequest,
        escalation_hint: &str,
        progress_tx: &tokio::sync::mpsc::Sender<AgentProgressEvent>,
    ) -> Result<PlannerOutput, String> {
        self.plan_internal(request, Some(escalation_hint), Some(progress_tx), None)
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
        // 尝试获取 AI（支持热加载配置）
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

        // 走提供商原生的结构化输出：由 API 层保证返回是合法 JSON，
        // 而不是靠 prompt 里的「请只输出 JSON」再从自由文本里抠花括号。
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
                    "我理解了你的请求，但生成执行计划时出现问题：{}。请尝试更具体地描述你想做什么。",
                    e
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
    /// 全部无法命中。此前当前时间戳排在第一段，意味着每分钟都会让整个 prompt 前缀
    /// 失配，而最大的两块（能力索引 ~113 条 + 190 行规则）恰好排在最后，永远进不了
    /// 缓存。
    ///
    /// 现在拆成两段拼接：
    /// - `stable`：身份 / 用户偏好 / 协作团队 / 能力索引 / 规则——只在部署配置、
    ///   Skill 注册表或 MCP 工具列表变化时才变
    /// - `volatile`：环境（时间+语言）/ 记忆 / 教训 / 推荐 Skill / 执行记录 /
    ///   升级上下文——每次请求都可能不同
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

        // 1. 身份（全局 SOUL.md）
        let speaking_soul = crate::services::agent::identity::get_speaking_soul().await;
        let global_identity = identity::get_identity().await;
        let role_prompt = speaking_soul
            .as_deref()
            .or_else(|| global_identity.as_ref().and_then(|id| id.role_prompt()));
        // 兜底身份此前是死代码：它的条件是 `sections.is_empty()`，而环境段总是先被
        // 压入，所以没有 SOUL.md 时 prompt 里根本不含身份段。
        stable.push(match role_prompt {
            Some(role) => format!("## 身份\n{}", role),
            None => {
                "## 身份\n你是 Agent，一个智能 AI 助手。你能理解用户的自然语言请求并规划执行步骤。"
                    .to_string()
            }
        });

        // 1.2. 用户偏好（USER.md）
        if let Some(ref id) = global_identity {
            if let Some(user_ctx) = id.user_context() {
                stable.push(format!("## 用户偏好\n{}", user_ctx));
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
                "## 自治授权上限\n本轮办事只能使用当前授予权限与自治授权的交集，不得规划需要其他权限的步骤。允许的权限：\n{}",
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
                    "## 协作团队\n你可以调度以下专业 Agent 的能力：\n{}",
                    summaries
                ));
            }
        }

        // 2. 记忆系统（多维召回：语义 + 能力 + 实体 + 教训）
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
                            format!(" ({}天前)", days)
                        } else {
                            format!(" ({}个月前)", days / 30)
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
                    "## 参考记忆\n以下是历史记忆，仅供参考。当用户请求包含「最近」「最新」「目前」「现在」等时效性词汇时，\
                    必须通过搜索获取实时信息，不要用历史记忆中的旧结论替代。\n<memory_context>\n{}\n</memory_context>",
                    mem_lines.join("\n")
                ));
            }
            if !lesson_lines.is_empty() {
                volatile.push(format!(
                    "## 注意事项（历史教训）\n<lessons>\n{}\n</lessons>",
                    lesson_lines.join("\n")
                ));
            }
        }

        // 3. 能力索引（含相关 Skill）
        let compact_index = get_compact_index_for_grants(granted).await;
        stable.push(format!(
            "## 可用能力（紧凑索引）\n\
             条目字段：`id` 能力 ID、`h` 用途、`p` 必需参数、`o` 该能力的输出字段。\n\
             引用前序步骤输出时，优先按 `o` 写出精确字段——\
             `\"dataFrom\": \"search.results\"` 而不是 `\"dataFrom\": \"search\"`；\
             只有确实需要整个输出对象时才引用步骤 ID 本身。\n\
             ```json\n{}\n```",
            serde_json::to_string_pretty(&compact_index).unwrap_or_default()
        ));

        // 4. 输出格式与规则（纯常量，稳定段的最后一块）
        stable.push(PLANNER_RULES.to_string());

        // —— 以下按请求变化，排在缓存前缀之后 ——

        // 5. 相关 Skill 预过滤（语义匹配 top-5，随用户输入变化）
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
                            format!(" (参数: {})", sm.skill.parameters.join(", "))
                        };
                        format!(
                            "- **skill:{}** (相关度 {:.0}%) — {}{}",
                            sm.skill.id,
                            sm.relevance * 100.0,
                            sm.skill.description,
                            params_hint,
                        )
                    })
                    .collect();
                volatile.push(format!(
                    "## 推荐 Skill\n以下 Skill 与用户请求高度相关，可优先考虑使用：\n{}",
                    skill_lines.join("\n")
                ));
            }
        }

        // 6. 近期执行摘要（从对话历史中提取能力使用记录）
        if let Some(ref context) = request.context {
            if let Some(ref history) = context.conversation_history {
                let exec_summary = Self::extract_execution_summary(history);
                if !exec_summary.is_empty() {
                    volatile.push(format!("## 本轮对话执行记录\n{}", exec_summary));
                }
            }
        }

        // 7. 升级提示
        if let Some(hint) = escalation_hint {
            volatile.push(format!("## 升级上下文\n前次执行结果不满意。{}", hint));
        }

        // 8. 环境上下文（时间、语言）——放在最后：时间戳每分钟都变，
        // 排在前面会让后续所有内容都失去缓存前缀；放末尾还能让语言指令离输出更近
        let now = chrono::Local::now();
        let lang_instruction = match language {
            super::intent::keywords::Language::Chinese => "请用中文回复。",
            super::intent::keywords::Language::English => "Please respond in English.",
            super::intent::keywords::Language::Japanese => "日本語で返信してください。",
        };
        volatile.push(format!(
            "## 环境\n当前时间：{}\n{}",
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
                prompt.push_str(&format!("\n当前页面：{} ({})", page_desc, route));
            }
            if !context.active_platforms.is_empty() {
                prompt.push_str(&format!(
                    "\n活跃平台：{}",
                    context.active_platforms.join(", ")
                ));
            }

            // 对话历史（直接从 conversation_history 读取，不再走 custom_data hack）
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
                        "\n\n请注意：用户可能在引用之前对话中提到的内容，注意理解代词和上下文指代。",
                    );
                }
            }

            // 页面上下文
            if let Some(custom_data) = &context.custom_data {
                let has_page = custom_data.get("pageContent").is_some();
                if has_page {
                    prompt.push_str("\n页面上下文可用：true。总结/分析用 \"contentFrom\": \"__page_context__\"；page.content / page.understand 用 \"contextFrom\": \"__page_context__\"（不要写成 inputFrom）");
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
                                    .push_str("\n  （图片像素未随请求发送，只能看到文件名和类型）");
                            }
                        }
                        prompt.push_str("\n</user_attachments>");
                    }
                }

                // 用户偏好
                if let Some(prefs) = custom_data.get("user_preferences") {
                    if let Some(prefs_obj) = prefs.as_object() {
                        if !prefs_obj.is_empty() {
                            prompt.push_str("\n\n用户历史偏好：");
                            if let Some(action) =
                                prefs_obj.get("preferred_action").and_then(|v| v.as_str())
                            {
                                prompt.push_str(&format!("\n- 常用操作：{}", action));
                            }
                            if let Some(platforms) = prefs_obj
                                .get("preferred_platforms")
                                .and_then(|v| v.as_array())
                            {
                                let names: Vec<&str> =
                                    platforms.iter().filter_map(|p| p.as_str()).collect();
                                if !names.is_empty() {
                                    prompt.push_str(&format!("\n- 常用平台：{}", names.join(", ")));
                                }
                            }
                        }
                    }
                }
            }
        }

        if let Some(hint) = escalation_hint {
            prompt.push_str(&format!("\n\n[升级提示] {}", hint));
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
                timeout_ms: Some(30000),
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
                || content.contains("failed");

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
/// 注意 `steps[].params` 是自由 map——能力各自的参数由 `validate_and_convert_steps`
/// 按 capability 的 `input_schema` 校验，不在这里穷举。它同时意味着这份 schema
/// 无法翻译成 Gemini 的 `responseSchema` 方言（Gemini 不接受没有 `properties` 的
/// OBJECT），Gemini 上只生效 `responseMimeType: application/json`；OpenAI 侧走
/// 非 strict 的 `json_schema`，两边都能保证返回是合法 JSON。
fn planner_output_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "status": {
                "type": "string",
                "enum": ["plan", "clarify", "unsupported", "chat"],
                "description": "本次判断的请求类型"
            },
            "confidence": {
                "type": "number",
                "minimum": 0.0,
                "maximum": 1.0
            },
            "reasoning": {
                "type": "string",
                "description": "简要说明判断思路"
            },
            "steps": {
                "type": "array",
                "description": "status=plan 时的执行步骤，最多 8 个",
                "maxItems": 8,
                "items": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string" },
                        "capability_id": {
                            "type": "string",
                            "description": "必须来自可用能力索引的 id"
                        },
                        "action": {
                            "type": "string",
                            "description": "对这个步骤的具体指令"
                        },
                        "params": {
                            "type": "object",
                            "description": "能力入参；引用前序步骤用 xxxFrom: \"step_id.字段\""
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
                "description": "status=clarify 时的提问",
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

/// 路由描述
fn describe_route(route: &str) -> &'static str {
    match route.trim_matches('/') {
        "brew" => "Brew 订阅页面",
        "tapp" | "tapps" => "Tapp 应用页面",
        "library" => "资料库页面",
        "config" | "settings" => "设置页面",
        route if route.starts_with("platform/bilibili") => "B站数据页面",
        route if route.starts_with("platform/steam") => "Steam 游戏页面",
        route if route.starts_with("platform/github") => "GitHub 页面",
        route if route.starts_with("platform/netease") => "网易云音乐页面",
        route if route.starts_with("platform/") => "平台页面",
        "report" | "reports" => "报告页面",
        "" | "dashboard" => "仪表盘首页",
        _ => "未知页面",
    }
}

/// Planner 规则与输出格式（注入到系统 prompt）
const PLANNER_RULES: &str = r#"## 规则

你需要分析用户请求，判断其类型，并输出对应的 JSON 响应。

### 请求类型判断

1. **plan**: 用户需要执行某个操作（查询数据、生成内容、控制功能等）→ 输出执行步骤
2. **chat**: 用户在闲聊、问问题、不需要调用任何能力 → 直接回复
3. **clarify**: 用户请求模糊，无法确定意图 → 请求澄清
4. **unsupported**: 用户请求超出能力范围 → 解释原因

### 主动征询意图识别（极其重要）

当用户**明确要求你提问或征求意见**时（"问我"、"问问我"、"你问一下我"、"让我选"、"给我选项"、"我来决定"、"先问问我的意见"），**必须使用 clarify**，绝不能用 chat。
- 这些表达是用户主动要求参与决策，不是闲聊
- clarification.message 里写出你需要了解的具体问题
- clarification.options 里给出 3-5 个合理选项供用户选择
- 结合对话历史和上次执行结果，让问题和选项尽量具体，不要泛泛而问

### 执行步骤规则（status=plan 时）

1. `capability_id` 必须匹配可用能力索引中的 ID。能力索引中 `"p"` 字段列出了必需参数，务必包含
2. 如果可用能力中有 `skill:xxx` 类型恰好匹配用户意图，优先使用 Skill（它封装了完整的多步骤编排）
3. **Skill 单次调用原则**：同一个 `skill:xxx` 在整个计划中最多出现一次。如果用户要求多张图/多个变体/一些/一批，通过 Skill 的参数传达数量和变体需求（如 `"count": 3`、`"variations": ["场景A", "场景B"]`），由 Skill 内部自行编排多轮生成。**绝不允许**把同一个 Skill 在步骤列表里重复调用多次
4. `params` 根据能力描述和 `"p"` 参数列表推断合理值
5. **❗ xxxFrom 必须配合 depends_on**：使用 `"xxxFrom": "step_id"` 引用其他步骤输出时，**必须同时在 `depends_on` 中声明该步骤**。例如 `"dataFrom": "search"` → `"depends_on": ["search"]`。缺少 depends_on 会导致步骤并行执行、引用为 null
5.1 **优先引用具体字段**：`"xxxFrom"` 支持 `"step_id.字段名"`，字段名取自能力索引的 `o` 列表。例如 `ai.webSearch` 的 `o` 含 `results`，就写 `"dataFrom": "search.results"`。引用整个步骤（`"search"`）只在需要完整输出对象时使用
6. 如果页面上下文可用：总结/分析用 `"contentFrom": "__page_context__"`；`page.content` / `page.understand` 用 `"contextFrom": "__page_context__"`（不要写成 `inputFrom`，这两个能力读的是 `context`）
7. `on_failure` 策略：
   - 数据获取步骤用 `"abort"`（后续步骤依赖数据，获取失败则无法继续）
   - AI 处理步骤可用 `"skip"`（非关键性分析/总结可跳过）
   - 如果步骤是其他步骤的 `depends_on` 数据源，必须 `"abort"`
8. `timeout_ms`: 数据获取 15000，AI 处理 30000，图片生成 60000
9. 可选字段：`"retry": {"max_attempts": 2, "delay_ms": 1000, "exponential_backoff": true}` — 对网络请求类步骤建议添加
10. 可选字段：`"model_tier": "pro"` — 需要高质量分析/创作时指定 pro，普通任务省略即可

### ❗ 步骤最小化原则（极其重要）

- **步骤数量绝对上限 8 个**，但大多数请求应在 1-3 步内完成
- **简单请求**（查询、搜索、生成一张图、问答）→ 1-2 步
- **中等请求**（搜索+分析、获取+总结）→ 2-3 步
- **复杂请求**（多平台对比、多步骤工作流）→ 3-6 步
- 每步都必须有明确且不可替代的作用，不允许「为了形式」增加步骤
- **当一个能力就能完成时，绝不拆成多步**
- step.action 字段是你对这个步骤的直接命令，必须具体、明确、与用户请求直接相关

### Skill 单次调用铁律

- **同一个 `skill:xxx` 在整个计划中只能出现一次**，重复调用同一 Skill 是严重错误
- 用户要求多张图/多个变体/一些/一批时，通过 params 传达（如 `"count": 3`、`"variations": ["场景A", "场景B", "场景C"]`），Skill 内部自行编排
- 举例：用户说"帮我生成一些XX图片" → 一个 `skill:xxx` 步骤 + params 中 `"count": 3` 或 `"variations": [...]`，而不是 3 个重复的 skill 步骤

### 数量意图识别

用户请求中包含数量词时：
- **明确单数**（"一张"、"一个"）→ 1 个步骤
- **指定具体数量**（"三张"、"5个"）→ 通过 params 传达数量，不要拆分为多步骤
- **无数量词或模糊词**（“帮我生成图”、“一些”、“几个”）→ **默认单个**，不要自行展开为多个

### 多目标与并行

**执行引擎会自动并行执行所有 `depends_on` 为空的步骤。** 因此：
- 互相独立、无数据依赖的步骤 → `depends_on: []`（引擎自动并行）
- 只有当步骤 B 需要步骤 A 的输出时 → `depends_on: ["step_a"]`

仅当用户**明确列举**多个目标时（"B站和Steam"、"搜索X和Y"）为每个目标生成独立步骤。
- "所有平台" → 最多展开 3 个主要平台
- 如果后续还有汇总需求，汇总步骤 `depends_on` 所有并行步骤

### 对比意图

用户表达对比、比较、PK 等意图时（"对比"、"比一比"、"哪个更好"、"有什么区别"），执行计划必须包含：
1. **并行数据获取**：为每个对比目标生成独立的数据获取步骤（`depends_on: []`，引擎自动并行）
2. **对比分析步骤**：一个 `compare.content` 或 `ai.analyze`（analysisType="custom"）步骤，`depends_on` 全部获取步骤，将获取结果作为对比输入

### 串联意图（A 然后 B）

用户在单句中表达连续操作时（"搜索并总结"、"找到后订阅"、"获取数据然后分析"、"翻译完再朗读"），必须拆解为**有依赖关系**的多步骤：
- 前置步骤执行数据获取或处理
- 后续步骤通过 `depends_on` 引用前置步骤 ID，并用 `"xxxFrom": "step_id"` 传递数据
- 不要合并为单步骤，每个动词对应一个能力调用

### 时间表达映射

用户请求包含时间修饰词时，转化为对应参数（`since`、`daysBack` 等）：
- “最近”/“近期” → `daysBack: 7`
- “今天” → `since` 当天零点
- “本周” → `daysBack: 7`
- “本月” → `daysBack: 30`
- 无时间修饰 → 使用默认值

### 后续/修改请求

用户发出后续请求（"换个XX"、"再来一个"、"改一下"、"不满意"、"不够XX"）时：
- 结合对话历史理解意图
- 用户给了反馈但未指定具体修改方向 → status="clarify"，给出修改选项
- 用户给了反馈且要求被征询意见（"问我"、"你问问我"）→ status="clarify"，必须提问
- 明确指定了具体修改方向时才直接执行（status="plan"）

### 指代消歧与上下文引用

用户使用代词时（“这个”、“它”、“刚才那个”），结合页面上下文和对话历史解析指代。
如果歧义无法解决，使用 status="clarify" 提问。

### 图片生成规则（prompt.generate + ai.image）

生成图片时，在 `prompt.generate` 的 `description` 中提供详尽描述：
- 已知角色必须写出完整视觉特征（发型发色、瞳色、服装细节、标志性元素）
- 昵称/简称必须展开为完整角色描述
- 包含场景、氛围、构图
- 绝不要只写简短标题

**分辨率由 `ai.image` 的 `width` / `height` 决定（像素，256–2048，省略则默认 1024×1024）**：
- 用户明确给了数字（"512"、"1024x768"、"1920×1080"）→ 按数字填 `width`/`height`
- 用户要竖图/手机壁纸/肖像 → 建议 `width: 768, height: 1024`（或 768×1344）
- 用户要横图/桌面壁纸/风景 → 建议 `width: 1024, height: 768`（或 1344×768）
- 用户要方图/头像/图标，或未提尺寸 → 省略尺寸（走默认 1024）或 `1024, 1024`
- **不要**把宽高写进 prompt 文本；写在 `ai.image` 的 params 里
- `width`/`height` 与 `promptFrom` 可同时存在

**当需要引用前置步骤的输出作为描述来源时**，使用 `xxxFrom` 约定：
- `descriptionFrom: "step_id"` — 从指定步骤的输出中提取文本作为 description
- `titleFrom: "step_id"` — 从指定步骤的输出中提取文本作为 title
- 与 `dataFrom` 一样，引擎会自动解析引用并提取文本内容

典型链式计划（搜索 → 分析 → 生成提示词 → 生成图片）：
```
search(ai.webSearch) → analyze(ai.analyze, dataFrom:"search") → gen_prompt(prompt.generate, descriptionFrom:"analyze") → image(ai.image, promptFrom:"gen_prompt", width?, height?)
```

### 输出格式

严格输出 JSON，不要包含 markdown 标记：

```
{
  "status": "plan" | "clarify" | "unsupported" | "chat",
  "confidence": 0.0-1.0,
  "reasoning": "简要说明你的判断思路",

  // status=plan 时（示例：搜索→分析→生成提示词→生成图片）:
  "steps": [
    {
      "id": "search",
      "capability_id": "ai.webSearch",
      "action": "搜索角色信息",
      "params": { "query": "..." },
      "depends_on": [],
      "on_failure": "abort",
      "timeout_ms": 15000
    },
    {
      "id": "analyze",
      "capability_id": "ai.analyze",
      "action": "分析并介绍角色",
      "params": { "dataFrom": "search", "instruction": "根据搜索结果介绍该角色..." },
      "depends_on": ["search"],
      "on_failure": "skip",
      "timeout_ms": 30000
    },
    {
      "id": "gen_prompt",
      "capability_id": "prompt.generate",
      "action": "生成角色图片提示词",
      "params": { "descriptionFrom": "analyze" },
      "depends_on": ["analyze"],
      "timeout_ms": 15000
    },
    {
      "id": "gen_image",
      "capability_id": "ai.image",
      "action": "生成角色图片",
      "params": { "promptFrom": "gen_prompt", "width": 768, "height": 1024 },
      "depends_on": ["gen_prompt"],
      "timeout_ms": 60000
    }
  ],

  // status=clarify 时:
  "clarification": {
    "message": "需要澄清的问题",
    "options": ["选项1", "选项2"]
  },

  // status=unsupported 时:
  "unsupported_reason": "不支持的原因",

  // status=chat 时:
  "chat_reply": "直接回复内容"
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

    #[test]
    fn schema_step_cap_matches_the_planner_rules() {
        // PLANNER_RULES tells the model 8 steps is the hard ceiling and
        // validate_and_convert_steps truncates there; the schema must agree.
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
        assert!(prefix.contains("## 身份"), "identity must be cacheable");
        assert!(
            prefix.contains("## 可用能力（紧凑索引）"),
            "the capability index is the largest block and must be cacheable"
        );
        assert!(
            prefix.contains("## 规则"),
            "PLANNER_RULES must be cacheable"
        );
        // The language instruction is what diverges, and it belongs after them.
        assert!(!prefix.contains("请用中文回复。"));
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

        let rules = prompt.find("## 规则").expect("rules section");
        let escalation = prompt.find("## 升级上下文").expect("escalation section");
        let environment = prompt.find("## 环境").expect("environment section");
        let capabilities = prompt
            .find("## 可用能力（紧凑索引）")
            .expect("capability index");

        assert!(capabilities < rules, "index precedes rules");
        assert!(rules < escalation, "escalation is request-specific");
        assert!(escalation < environment, "the timestamp goes last");
    }

    #[tokio::test]
    async fn identity_falls_back_when_no_soul_file_is_loaded() {
        // The previous fallback was unreachable: it was guarded on
        // `sections.is_empty()` while the environment section had already been
        // pushed, so a deployment without SOUL.md got no identity at all.
        let prompt = test_planner()
            .build_system_prompt(
                &request("你好"),
                crate::services::agent::intent::keywords::Language::Chinese,
                None,
                None,
            )
            .await;
        assert!(prompt.contains("## 身份"));
        assert!(
            prompt.contains("你是 Agent") || prompt.contains("You are Agent"),
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
        assert!(prompt.contains("图片像素未随请求发送"));
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
