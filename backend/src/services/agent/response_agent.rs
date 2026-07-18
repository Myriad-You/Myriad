//! 响应生成副 Agent
//!
//! 专门负责所有面向用户的文本生成，统一 Agent 的 "说话方式"。
//! 所有用户可见的回复文本都通过此模块生成，确保风格一致且有人情味。
//!
//! 两种模式：
//! - **AI 模式**：调用 AI 模型生成个性化回复（用于最终回复、多步骤汇总）
//! - **模板模式**：同步返回预设的温暖模板（用于实时进度、简单状态）

use serde_json::Value;

use super::identity;
use super::types::AgentProgressEvent;

// ─────────────────────────────────────────────
// 1. AI 驱动的最终回复生成（异步，支持流式）
// ─────────────────────────────────────────────

/// 执行结果的上下文，用于生成最终回复
pub struct ResponseContext<'a> {
    /// 用户原始请求（让 AI 知道该回答什么）
    pub user_request: &'a str,
    /// 成功步骤的结构化 JSON 输出
    pub step_outputs: Vec<StepOutput<'a>>,
    /// 可选的 SSE 进度通道（用于流式推送 token）
    pub progress_tx: Option<&'a tokio::sync::mpsc::Sender<AgentProgressEvent>>,
}

pub struct StepOutput<'a> {
    pub step_id: &'a str,
    pub output: &'a Value,
}

/// AI 驱动的最终回复生成
///
/// 1. 过滤掉 planning 占位输出
/// 2. 构建结构化 JSON 上下文
/// 3. 调用 AI（带人格 SOUL.md）流式生成回复
/// 4. AI 失败时使用智能 fallback
pub async fn generate_final_response(ctx: ResponseContext<'_>) -> String {
    // 过滤掉 skill planning 占位输出，提取有语义的文本内容（不带原始 JSON）
    let step_data: Vec<String> = ctx
        .step_outputs
        .iter()
        .filter_map(|s| {
            if s.output.get("status").and_then(|v| v.as_str()) == Some("planned") {
                return None;
            }
            let text = extract_step_text(s.output);
            if text.is_empty() {
                return None;
            }
            let truncated: String = text.chars().take(3000).collect();
            Some(format!("[{}] {}", s.step_id, truncated))
        })
        .collect();

    if step_data.is_empty() {
        return completion_message();
    }

    // 尝试 AI 生成
    if let Some(msg) = ai_summarize(ctx.user_request, &step_data, ctx.progress_tx).await {
        return msg;
    }

    // AI 不可用：智能 fallback
    smart_fallback(&ctx.step_outputs)
}

/// 为单步骤结果生成最终回复
pub fn generate_single_step_response(result: &Value) -> Option<String> {
    // 优先展示 AI 生成的内容（这些本身就是有人格的）
    if let Some(v) = result
        .get("aiSummary")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        return Some(v.to_string());
    }
    if let Some(v) = result
        .get("reply")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        return Some(v.to_string());
    }
    if let Some(v) = result
        .get("analysis")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        return Some(v.to_string());
    }
    if let Some(v) = result
        .get("summary")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        return Some(v.to_string());
    }
    if let Some(v) = result
        .get("message")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        return Some(v.to_string());
    }

    // 搜索结果
    if let Some(source) = result.get("source").and_then(|v| v.as_str()) {
        if source == "gemini_grounding" || source == "google_search" || source == "local_cache" {
            if let Some(results) = result.get("results").and_then(|v| v.as_array()) {
                if results.is_empty() {
                    return Some("搜索了一圈，没有找到相关结果呢。".to_string());
                }
                let query = result.get("query").and_then(|v| v.as_str()).unwrap_or("");
                return Some(format!(
                    "找到了 {} 条关于「{}」的信息，来看看吧~",
                    results.len(),
                    query
                ));
            }
        }
    }

    // 图片生成
    if result.get("imageUrl").and_then(|v| v.as_str()).is_some() {
        let prompt = result.get("prompt").and_then(|v| v.as_str()).unwrap_or("");
        if !prompt.is_empty() {
            return Some(format!("图片生成好了~ 画的是「{}」，希望你喜欢！", prompt));
        }
        return Some("图片已经生成好了，快看看效果吧~".to_string());
    }

    None // 调用方应使用 completion_message()
}

// ─────────────────────────────────────────────
// 1.5 计划生成后的说明（AI 驱动 + fallback）
// ─────────────────────────────────────────────

/// 计划生成后，向用户说明即将要做什么
///
/// 通过 AI 生成温暖的计划说明，通过 SummaryToken 流式推送。
/// AI 失败时返回模板 fallback。
pub async fn announce_plan(
    user_input: &str,
    step_descriptions: &[String],
    progress_tx: &tokio::sync::mpsc::Sender<AgentProgressEvent>,
) -> String {
    // 先尝试 AI 生成
    if let Some(msg) = ai_announce_plan(user_input, step_descriptions, progress_tx).await {
        return msg;
    }
    // fallback：模板
    plan_announcement_fallback(step_descriptions)
}

/// 模板 fallback（AI 不可用时）
fn plan_announcement_fallback(step_descriptions: &[String]) -> String {
    if step_descriptions.is_empty() {
        return "让我看看……".to_string();
    }
    if step_descriptions.len() == 1 {
        return format!("我先{}，稍等一下", step_descriptions[0]);
    }
    // 直接用箭头串联步骤，一目了然
    let flow: String = step_descriptions.join(" → ");
    format!("我的计划：{}\n这就开始", flow)
}

/// AI 生成计划说明（流式推送）
async fn ai_announce_plan(
    user_input: &str,
    step_descriptions: &[String],
    progress_tx: &tokio::sync::mpsc::Sender<AgentProgressEvent>,
) -> Option<String> {
    use crate::config::ModelTier;
    use crate::services::ai::create_ai_analyzer_for_tier;

    let analyzer = create_ai_analyzer_for_tier(ModelTier::Standard).await?;

    let soul = identity::get_identity()
        .await
        .and_then(|id| id.soul)
        .unwrap_or_default();
    let soul: String = soul.chars().take(2000).collect();

    let steps_list = step_descriptions
        .iter()
        .enumerate()
        .map(|(i, d)| format!("{}. {}", i + 1, d))
        .collect::<Vec<_>>()
        .join("\n");

    let prompt = format!(
        "{soul}\n\n\
         User: \"{user_input}\"\n\n\
         Your plan:\n{steps_list}\n\n\
         Now tell the user what you're about to do. Rules:\n\
         - Be SPECIFIC: mention the concrete things you'll do (e.g. \"查东京天气，再找最近好看的动画\"), not vague summaries.\n\
         - Be direct and concise. 1-2 sentences max.\n\
         - Match the user's language.\n\
         - Do NOT use numbered lists, bullet points, or \"1. 2. 3.\" format.\n\
         - Do NOT use filler phrases like \"好的\" \"没问题\" \"马上开始\" \"让我来\" at the start.\n\
         - Sound like a real person, not a customer service bot.",
        soul = soul,
        user_input = user_input,
        steps_list = steps_list,
    );

    let tx = progress_tx.clone();
    match analyzer
        .analyze_stream(&prompt, |token| {
            let _ = tx.try_send(AgentProgressEvent::SummaryToken {
                token: token.to_string(),
                done: false,
            });
            true
        })
        .await
    {
        Ok(full_text) if !full_text.trim().is_empty() => {
            let _ = tx.try_send(AgentProgressEvent::SummaryToken {
                token: String::new(),
                done: true,
            });
            Some(full_text.trim().to_string())
        }
        Ok(_) => None,
        Err(e) => {
            tracing::warn!("[ResponseAgent] Plan announcement streaming failed: {}", e);
            None
        }
    }
}

/// 单步骤开始时的描述文本
///
/// 返回简洁描述，不加"正在"前缀（前端负责展示格式如"第X步 描述"）
pub fn describe_step_start(step_description: &str) -> String {
    step_description.to_string()
}

/// 并行步骤开始时的描述文本
pub fn describe_parallel_step_start(step_description: &str) -> String {
    step_description.to_string()
}

// ─────────────────────────────────────────────
// 2. 同步模板消息（用于实时进度、状态等）
// ─────────────────────────────────────────────

/// 任务完成
pub fn completion_message() -> String {
    "好了，都处理完啦~".to_string()
}

/// 任务失败
pub fn error_message(err: &str) -> String {
    format!("抱歉，这次没能完成你的请求：{}", err)
}

/// 执行遇到问题
pub fn execution_error(err: &str) -> String {
    format!("抱歉，执行时遇到了问题：{}", err)
}

/// 部分完成
pub fn partial_completion(success: usize, total: usize, errors: &[String]) -> String {
    let mut msg = format!(
        "完成了大部分工作（{}/{}），不过有些步骤没能顺利执行",
        success, total
    );
    if !errors.is_empty() {
        msg.push_str(&format!("：{}", errors.join("；")));
    }
    msg
}

/// 需要更多信息
pub fn need_more_info() -> String {
    "需要你补充一些信息~".to_string()
}

/// 正在执行
pub fn in_progress(name: &str) -> String {
    format!("正在处理「{}」...", name)
}

/// saved recipe 完成
pub fn recipe_completed(name: &str) -> String {
    format!("「{}」执行完成~", name)
}

/// saved recipe 失败
pub fn recipe_failed(name: &str, err: &str) -> String {
    format!("「{}」没能执行成功：{}", name, err)
}

/// 步骤进度摘要（实时显示给用户的步骤状态，同步，不调用 AI）
pub fn summarize_step_output(output: &Value) -> Option<String> {
    if let Some(obj) = output.as_object() {
        // 图片生成
        if obj.get("imageUrl").and_then(|v| v.as_str()).is_some() {
            let prompt = obj.get("prompt").and_then(|v| v.as_str()).unwrap_or("");
            if !prompt.is_empty() {
                return Some(format!("生成图片: {}", prompt));
            }
            return Some("图片生成完成".to_string());
        }
        // 有 message 字段直接用
        if let Some(msg) = obj.get("message").and_then(|v| v.as_str()) {
            let chars: Vec<char> = msg.chars().collect();
            if chars.len() > 80 {
                return Some(format!("{}...", chars[..80].iter().collect::<String>()));
            }
            return Some(msg.to_string());
        }
        // 数量统计
        if let Some(count) = obj.get("total").and_then(|v| v.as_i64()) {
            return Some(format!("获取了 {} 条结果", count));
        }
        if let Some(arr) = obj.get("feeds").and_then(|v| v.as_array()) {
            return Some(format!("找到 {} 个订阅源", arr.len()));
        }
        if let Some(arr) = obj.get("items").and_then(|v| v.as_array()) {
            return Some(format!("获取了 {} 条数据", arr.len()));
        }
        // AI 摘要
        if let Some(summary) = obj.get("aiSummary").and_then(|v| v.as_str()) {
            let chars: Vec<char> = summary.chars().collect();
            if chars.len() > 80 {
                return Some(format!("{}...", chars[..80].iter().collect::<String>()));
            }
            return Some(summary.to_string());
        }
        // 搜索结果
        if let Some(results) = obj.get("results").and_then(|v| v.as_array()) {
            let query = obj.get("query").and_then(|v| v.as_str()).unwrap_or("");
            if !query.is_empty() {
                return Some(format!("搜索「{}」得到 {} 条结果", query, results.len()));
            }
            return Some(format!("搜索得到 {} 条结果", results.len()));
        }
        // 通用：显示有意义的字段名
        let meaningful_keys: Vec<&str> = obj
            .keys()
            .map(|k| k.as_str())
            .filter(|k| !["status", "provider", "model", "cached"].contains(k))
            .take(3)
            .collect();
        if !meaningful_keys.is_empty() {
            return Some(format!("已获取数据 ({})", meaningful_keys.join(", ")));
        }
        return Some("处理完成".to_string());
    }
    if let Some(arr) = output.as_array() {
        return Some(format!("获取了 {} 条记录", arr.len()));
    }
    if let Some(s) = output.as_str() {
        let chars: Vec<char> = s.chars().collect();
        if chars.len() > 80 {
            return Some(format!("{}...", chars[..80].iter().collect::<String>()));
        }
        return Some(s.to_string());
    }
    if let Some(b) = output.as_bool() {
        return Some(if b {
            "操作成功".to_string()
        } else {
            "操作未成功".to_string()
        });
    }
    None
}

// ─────────────────────────────────────────────
// 3. 内部实现
// ─────────────────────────────────────────────

/// 从步骤输出中提取有语义的文本（喂给 AI summarizer 的原料）
///
/// 注意：这个函数的输出是给 AI 看的，不是直接展示给用户。
/// 所以即使是 Gemini 的 raw JSON 格式 aiSummary 也可以保留 — AI 能理解并提炼。
fn extract_step_text(output: &Value) -> String {
    let mut parts: Vec<String> = Vec::new();

    // 提取文本字段
    let text_keys = ["analysis", "aiSummary", "reply", "summary", "message"];
    for key in &text_keys {
        if let Some(text) = output
            .get(key)
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        {
            parts.push(text.to_string());
            // analysis / aiSummary / reply 通常已覆盖所有语义，取到就够了
            break;
        }
    }

    // prompt.generate 结果
    if let Some(prompt) = output
        .get("prompt")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        parts.push(format!(
            "生成了图像提示词: {}",
            prompt.chars().take(200).collect::<String>()
        ));
    }

    // 图片
    if let Some(url) = output
        .get("imageUrl")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        parts.push(format!("已生成图片: {}", url));
    }

    // 搜索结果：只取数量和查询词
    if parts.is_empty() {
        if let Some(results) = output.get("results").and_then(|v| v.as_array()) {
            let query = output.get("query").and_then(|v| v.as_str()).unwrap_or("");
            if !query.is_empty() {
                parts.push(format!("搜索「{}」得到 {} 条结果", query, results.len()));
            } else {
                parts.push(format!("获取了 {} 条记录", results.len()));
            }
        }
    }

    parts.join("\n")
}

/// AI 汇总（带人格，支持流式）
async fn ai_summarize(
    user_request: &str,
    step_data: &[String],
    progress_tx: Option<&tokio::sync::mpsc::Sender<AgentProgressEvent>>,
) -> Option<String> {
    use crate::config::ModelTier;
    use crate::services::ai::create_ai_analyzer_for_tier;

    let analyzer = match create_ai_analyzer_for_tier(ModelTier::Standard).await {
        Some(a) => a,
        None => {
            tracing::warn!(
                "[ResponseAgent] Standard AI analyzer not available, falling back to template"
            );
            return None;
        }
    };

    let soul = identity::get_identity()
        .await
        .and_then(|id| id.soul)
        .unwrap_or_default();
    let soul: String = soul.chars().take(2000).collect();

    let steps_text = step_data
        .iter()
        .enumerate()
        .map(|(i, s)| format!("{}. {}", i + 1, s))
        .collect::<Vec<_>>()
        .join("\n");
    let steps_text: String = steps_text.chars().take(6000).collect();

    tracing::debug!(
        steps_count = step_data.len(),
        prompt_len = steps_text.len(),
        "[ResponseAgent] AI summarize: {} steps, prompt ~{} chars",
        step_data.len(),
        steps_text.len()
    );

    let prompt = format!(
        "{soul}\n\n\
         用户的请求：「{user_request}」\n\n\
         你为了回答这个请求，执行了多个步骤，以下是各步骤产出的原始素材：\n\
         {steps_text}\n\n\
         现在请基于这些素材，直接回复用户。要求：\n\
         - 你的回复就是最终呈现给用户的内容，直接回答用户的请求，不要有「以下是…」「根据…」之类的前缀\n\
         - 步骤素材是你的参考资料，提炼关键信息写成自然流畅的回复，不要照搬原文\n\
         - 回复长度匹配内容丰富度：简单结果 1-2 句话，丰富内容可以用几段\n\
         - 包含具体的名字、数字、事实，不要笼统\n\
         - 用用户使用的语言回复\n\
         - 不要提及步骤编号、JSON、技术细节\n\
         - 如果生成了图片，在末尾自然地提一下",
        soul = soul,
        user_request = user_request,
        steps_text = steps_text,
    );

    if let Some(tx) = progress_tx {
        let tx = tx.clone();
        match analyzer
            .analyze_stream(&prompt, |token| {
                let _ = tx.try_send(AgentProgressEvent::SummaryToken {
                    token: token.to_string(),
                    done: false,
                });
                true
            })
            .await
        {
            Ok(full_text) if !full_text.trim().is_empty() => {
                let _ = tx.try_send(AgentProgressEvent::SummaryToken {
                    token: String::new(),
                    done: true,
                });
                Some(full_text.trim().to_string())
            }
            Ok(_) => None,
            Err(e) => {
                tracing::warn!("[ResponseAgent] Streaming summary failed: {}", e);
                None
            }
        }
    } else {
        match analyzer.analyze(&prompt).await {
            Ok(msg) if !msg.trim().is_empty() => Some(msg.trim().to_string()),
            Ok(_) => None,
            Err(e) => {
                tracing::warn!("[ResponseAgent] Summary failed: {}", e);
                None
            }
        }
    }
}

/// AI 不可用时的智能 fallback
///
/// 在多步骤链（如 webSearch → ai.analyze → prompt.generate → ai.image）中，
/// 后续步骤已经消化了前面步骤的输出。因此只取最有语义的一段文本避免冗余拼接。
fn smart_fallback(step_outputs: &[StepOutput<'_>]) -> String {
    // 1. 优先查找 ai.analyze / ai.chat 等 AI 处理步骤的输出（这些是最终语义内容）
    //    跳过搜索原始数据和中间产物。
    //
    //    注意：step_outputs 按 step_id 字母序排列，不是执行顺序，
    //    所以不能依赖 .rev() 来获取"最后一步"。改用语义优先级筛选。
    let mut best_text: Option<String> = None;

    // 第一轮：找 analysis（ai.analyze 产出的深度分析/介绍）
    for s in step_outputs.iter() {
        let o = s.output;
        if let Some(text) = o
            .get("analysis")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        {
            best_text = Some(text.to_string());
            break;
        }
    }
    // 第二轮：找 reply（ai.chat 产出的回复）
    if best_text.is_none() {
        for s in step_outputs.iter() {
            if let Some(text) = s
                .output
                .get("reply")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                best_text = Some(text.to_string());
                break;
            }
        }
    }
    // 第三轮：找 summary（ai.summarize 产出的摘要）
    if best_text.is_none() {
        for s in step_outputs.iter() {
            if let Some(text) = s
                .output
                .get("summary")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                best_text = Some(text.to_string());
                break;
            }
        }
    }
    // 最后：aiSummary（搜索引擎的 AI 摘要，仅在没有更好内容时使用）
    if best_text.is_none() {
        // 检查是否有 AI 处理步骤（analysis/reply/summary 的产出方）
        // 如果有，说明搜索步骤的数据已被消化，其 aiSummary 是冗余的
        let has_ai_processed = step_outputs.iter().any(|s| {
            let o = s.output;
            o.get("analysis")
                .and_then(|v| v.as_str())
                .is_some_and(|s| !s.is_empty())
                || o.get("reply")
                    .and_then(|v| v.as_str())
                    .is_some_and(|s| !s.is_empty())
                || o.get("summary")
                    .and_then(|v| v.as_str())
                    .is_some_and(|s| !s.is_empty())
        });
        for s in step_outputs.iter() {
            let o = s.output;
            // 如果已有 AI 处理输出，跳过搜索步骤的 aiSummary（避免展示 raw JSON）
            if has_ai_processed && o.get("results").and_then(|v| v.as_array()).is_some() {
                continue;
            }
            if let Some(text) = o
                .get("aiSummary")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                best_text = Some(text.to_string());
                break;
            }
        }
    }

    let mut parts: Vec<String> = Vec::new();
    if let Some(text) = best_text {
        parts.push(text);
    }

    // 2. 统计图片数量并追加提示
    let image_count = step_outputs
        .iter()
        .filter(|s| s.output.get("imageUrl").and_then(|v| v.as_str()).is_some())
        .count();
    if image_count > 0 {
        parts.push(format!(
            "已为你生成 {} 张图片，点击可查看大图~",
            image_count
        ));
    }

    if parts.is_empty() {
        completion_message()
    } else {
        parts.join("\n\n")
    }
}

// ─────────────────────────────────────────────
// 4. 对话与交互模板
// ─────────────────────────────────────────────

/// 默认问候
pub fn greeting() -> String {
    "你好！有什么我可以帮你的吗？".to_string()
}

/// 正在理解请求
pub fn understanding_request() -> String {
    "正在理解你的请求...".to_string()
}

/// 正在规划步骤
pub fn planning_steps() -> String {
    "正在规划执行步骤...".to_string()
}

/// 进度完成
pub fn done_status() -> String {
    "完成".to_string()
}

/// 不支持的操作
pub fn unsupported_operation() -> String {
    "不支持此操作".to_string()
}

/// 需要更多信息（详细版，用于 Clarify 分流）
pub fn need_clarification() -> String {
    "我需要更多信息来理解你的请求".to_string()
}

/// 未能成功执行
pub fn not_executed() -> String {
    "未能成功执行".to_string()
}

/// 默认建议
pub fn default_suggestions() -> Vec<String> {
    vec![
        "搜索最新的科技新闻".to_string(),
        "查看我的 Steam 游戏".to_string(),
    ]
}

/// 升级策略进度
pub fn escalation_status(hint: &str) -> String {
    format!("正在升级策略：{}", hint)
}

/// 升级重试
pub fn escalation_retry() -> String {
    "升级重试".to_string()
}

/// 单参数提问
pub fn ask_single_param(desc: &str) -> String {
    format!("请告诉我{}", desc)
}

/// 多参数提问
#[allow(dead_code)] // kept for multi-param free-text fallback if needed
pub fn ask_multiple_params(prompts: &str) -> String {
    format!("在开始之前，我需要了解一些信息：\n{}", prompts)
}

/// 正在执行预设任务
pub fn executing_preset(name: &str) -> String {
    format!("正在执行预设任务：{}", name)
}

// ─────────────────────────────────────────────
// 5. 确认对话框模板
// ─────────────────────────────────────────────

/// 操作已取消
pub fn operation_cancelled() -> String {
    "操作已取消".to_string()
}

/// 确认已过期
pub fn confirmation_expired() -> String {
    "确认请求已过期，请重新发起操作".to_string()
}

/// 确认不存在或已处理
pub fn confirmation_not_found() -> String {
    "确认请求不存在或已处理".to_string()
}

/// 取消后建议
pub fn cancel_suggestions() -> Vec<String> {
    vec!["查看其他操作".to_string()]
}

/// 重新执行建议
pub fn retry_suggestions() -> Vec<String> {
    vec!["重新执行".to_string()]
}

/// 重新发起操作建议
pub fn retry_operation_suggestions() -> Vec<String> {
    vec!["重新发起操作".to_string()]
}

/// 确认对话框建议按钮
pub fn confirmation_suggestions() -> Vec<String> {
    vec![
        "确认执行".to_string(),
        "取消操作".to_string(),
        "查看详情".to_string(),
    ]
}

/// 风险等级前缀
pub fn risk_prefix(level: &str) -> &'static str {
    match level {
        "critical" => "⚠️ 危险操作",
        "high" => "🔴 高风险操作",
        "medium" => "🟡 敏感操作",
        "low" => "🟢 需确认操作",
        _ => "操作确认",
    }
}

/// 风险等级影响描述
pub fn risk_impact(level: &str) -> Vec<String> {
    match level {
        "critical" => vec![
            "⚠️ 此操作为系统级敏感操作".to_string(),
            "⚠️ 操作不可逆，请谨慎确认".to_string(),
        ],
        "high" => vec![
            "🔴 此操作可能导致数据丢失".to_string(),
            "🔴 操作完成后无法撤销".to_string(),
        ],
        "medium" => vec!["🟡 此操作将修改数据或系统配置".to_string()],
        "low" => vec!["🟢 此操作影响较小，可以恢复".to_string()],
        _ => vec![],
    }
}

/// 目标平台描述
pub fn target_platform(platform: &str) -> String {
    format!("目标平台: {}", platform)
}

/// 目标 URL 描述
pub fn target_url(url: &str) -> String {
    format!("目标 URL: {}", url)
}

/// 确认对话框完整消息
pub fn confirmation_dialog(prefix: &str, step_names: &str, impact_text: &str) -> String {
    format!(
        "{}: 即将执行 {}。\n\n{}\n\n请确认是否继续执行？",
        prefix, step_names, impact_text
    )
}

/// 确认选项 — 是
pub fn yes_label() -> String {
    "是".to_string()
}

/// 确认选项 — 否
pub fn no_label() -> String {
    "否".to_string()
}

/// 确认消息默认格式
pub fn will_execute(name: &str) -> String {
    format!("此操作将执行 {}", name)
}

// ─────────────────────────────────────────────
// 6. 任务生命周期
// ─────────────────────────────────────────────

/// 任务已被取消（SSE 消息）
pub fn task_cancelled() -> String {
    "任务已被取消".to_string()
}

/// 任务已被用户取消（错误字段）
pub fn task_cancelled_by_user() -> String {
    "任务已被用户取消".to_string()
}

/// 任务因服务重启中断
pub fn task_interrupted() -> String {
    "任务因服务重启而中断，请重新提交".to_string()
}

/// 步骤执行超时
pub fn step_timeout(capability: &str, secs: u64) -> String {
    format!("能力 '{}' 执行超时（{}秒）", capability, secs)
}

/// 步骤执行出错 — 用户选择题
pub fn step_error_question(error: &str) -> String {
    format!("执行过程中遇到问题：{}，你希望如何处理？", error)
}

/// 步骤执行出错 — 标题
pub fn step_error_title() -> String {
    "执行步骤时发生错误".to_string()
}

// ─────────────────────────────────────────────
// 7. 操作结果消息
// ─────────────────────────────────────────────

/// 订阅成功
pub fn subscribe_success(name: &str, count: usize) -> String {
    format!("成功订阅「{}」，已获取 {} 篇文章", name, count)
}

/// 内容已保存
pub fn content_saved(title: &str) -> String {
    format!("内容已保存: {}", title)
}

/// 刷新任务已提交
pub fn refresh_submitted(platform: &str) -> String {
    format!("刷新任务已提交: {}", platform)
}

/// 刷新提交失败
pub fn refresh_submit_failed(err: &str) -> String {
    format!("提交失败: {}", err)
}

/// 刷新提交汇总
pub fn refresh_submitted_summary(submitted: usize, total: usize) -> String {
    format!("已提交 {}/{} 个平台的刷新任务", submitted, total)
}

/// 提醒已创建
pub fn reminder_created(title: &str) -> String {
    format!("提醒已创建: {}", title)
}

/// 提醒时间
pub fn reminder_time(datetime: &str) -> String {
    format!("将在 {} 提醒你", datetime)
}

/// 笔记已保存
pub fn note_saved(title: &str) -> String {
    format!("笔记已保存: {}", title)
}

/// 书签已保存
pub fn bookmark_saved(title: &str) -> String {
    format!("书签已保存: {}", title)
}

/// 定时任务已创建
pub fn scheduled_task_created(name: &str) -> String {
    format!("定时任务已创建: {}", name)
}

/// 网络搜索回退结果
pub fn web_search_fallback(count: usize) -> String {
    format!(
        "数据库中未找到相关文章，已通过 AI 联网搜索获取 {} 条结果",
        count
    )
}

/// 未找到符合条件的文章
pub fn no_articles_found(criteria: &str) -> String {
    format!("未找到符合条件的文章（{}）", criteria)
}

/// 未在已订阅源中找到
pub fn not_found_in_feeds(query: &str) -> String {
    format!(
        "未在已订阅源中找到「{}」，可以使用 brew.discover 从 RSSHub 路由中搜索",
        query
    )
}

/// 未能找到匹配的 RSS 源
pub fn no_rss_found() -> String {
    "未能找到匹配的 RSS 源".to_string()
}

/// 未找到歌单
pub fn playlist_not_found() -> String {
    "没有找到相关歌单，请尝试其他关键词".to_string()
}

/// 找到歌单
pub fn playlist_found(count: usize, keyword: &str) -> String {
    format!("找到 {} 个「{}」相关歌单", count, keyword)
}

/// 未找到订阅源或作者
pub fn feed_ambiguous(name: &str) -> String {
    format!("未找到名为「{}」的订阅源或作者", name)
}

/// 订阅源建议
pub fn feed_hint(suggestions: &str) -> String {
    format!("你是不是想找: {}?", suggestions)
}

/// 搜索结果
pub fn search_results_found(count: usize, query: &str) -> String {
    format!("找到 {} 条与 '{}' 相关的结果", count, query)
}

/// 活跃平台
pub fn active_platforms(count: usize) -> String {
    format!("活跃在 {} 个平台", count)
}

/// TTS 未配置
pub fn tts_not_configured() -> String {
    "TTS 服务未配置。请在设置中配置语音合成服务后重试。".to_string()
}

/// 图片生成完成但无法提取 URL
pub fn image_generated_no_url() -> String {
    "图片生成完成，但无法提取图片 URL，请查看任务详情".to_string()
}

/// API Key 未配置
pub fn api_key_not_configured(service: &str) -> String {
    format!("{} API Key 未配置", service)
}

/// 不支持的平台名称
pub fn unsupported_platform(platform: &str) -> String {
    format!("不支持的平台名称: {}", platform)
}

/// 输入过长
pub fn input_too_long(max: usize) -> String {
    format!("输入过长，最大允许 {} 字符", max)
}

/// 输入不能为空
pub fn input_empty() -> String {
    "输入不能为空".to_string()
}

/// 所有订阅 URL 都失败
pub fn subscribe_all_failed(tried: usize, last_error: &str) -> String {
    format!(
        "尝试了 {} 个源都无法订阅。最后一个错误: {}",
        tried, last_error
    )
}

/// 文章摘要占位
pub fn article_summary_placeholder(source: &str, title: &str) -> String {
    format!(
        "这是一篇来自 {} 的文章：{}。点击阅读原文获取完整内容。",
        source, title
    )
}

/// 搜索空提示
pub fn search_empty_hint() -> String {
    "请提供搜索关键词。系统支持搜索的内容包括：Steam 游戏、Bilibili 追番、Bangumi 收藏、MyAnimeList 列表、GitHub 仓库、网易云音乐播放记录。".to_string()
}

/// 搜索无结果
pub fn search_no_results(query: &str) -> String {
    format!(
        "在你的数据中没有找到与 '{}' 相关的内容。\n\n系统目前只能搜索你已同步的平台数据：\n- Steam 游戏库\n- Bilibili 追番\n- Bangumi 收藏\n- MyAnimeList 动画/漫画列表\n- GitHub 仓库\n- 网易云音乐\n\n如果你想搜索网络新闻或其他外部内容，这个功能暂不支持。",
        query
    )
}

/// 数据返回摘要
pub fn data_returned(count: usize) -> String {
    format!("返回 {} 条数据", count)
}

/// 处理记录摘要
pub fn records_processed(count: u64) -> String {
    format!("处理了 {} 条记录", count)
}

/// 字段返回摘要
pub fn fields_returned(count: usize) -> String {
    format!("返回 {} 个字段", count)
}

/// 操作成功/失败（布尔结果）
pub fn bool_result(success: bool) -> String {
    if success {
        "成功".to_string()
    } else {
        "失败".to_string()
    }
}
