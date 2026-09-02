//! AI 处理能力处理器
//!
//! 处理 ai.summarize, ai.analyze, ai.chat, ai.groundingSearch 等 AI 类能力。
//! 纯 prompt/steering/image 规则见 [`crate::services::agent::ai_process_pure`]。

use super::HandlerContext;
use crate::models::entities::brew_items;
use crate::services::agent::ai_process_pure::{
    append_memory_to_system_prompt, capability_needs_conversation_context, capability_needs_memory,
    extract_semantic_text, inject_directive_to_params, inject_steering_to_params,
    merge_system_prompt, resolve_image_dimensions, resolve_image_prompt, sanitize_prompt_input,
    take_recent_conversation_messages, with_system_guidance, IMAGE_PROMPT_MAX_CHARS,
    USER_TEXT_MAX_CHARS,
};
use crate::services::agent::data_read_pure::extract_json_array_from_ai_response;
use crate::services::agent::external_pure::classify_outbound_fetch;
use crate::GLOBAL_DYNAMIC_CONFIG;
use sea_orm::EntityTrait;
use serde_json::{json, Value};
use std::collections::HashMap;

fn ai_step_failed(label: &str, error: impl std::fmt::Display) -> String {
    let detail = error.to_string();
    tracing::error!(error = %detail, label, "AI step failed");
    classify_outbound_fetch(label, &detail)
}

/// 注入执行上下文到 AI 参数：角色身份 + 对话历史
fn inject_role_identity(
    capability_id: &str,
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> HashMap<String, Value> {
    let mut params = params.clone();

    let exec_ctx = match &ctx.execution_context {
        Some(ec) => ec,
        None => return params,
    };

    // 1. 注入角色身份到 systemPrompt
    if !exec_ctx.role_contexts.is_empty() {
        let router = crate::services::agent::routing::get_router();
        let role = router.route_capability(capability_id);
        let role_key = format!("{:?}", role);

        if let Some(role_identity) = exec_ctx.role_contexts.get(&role_key) {
            let existing = params
                .get("systemPrompt")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            params.insert(
                "systemPrompt".to_string(),
                Value::String(merge_system_prompt(existing, role_identity)),
            );
        }
    }

    // 2. 注入记忆上下文（对话/分析/推荐类能力，帮助 AI 基于用户历史偏好生成回复）
    if capability_needs_memory(capability_id) {
        if let Some(ref mem_ctx) = exec_ctx.memory_context {
            let existing = params
                .get("systemPrompt")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            params.insert(
                "systemPrompt".to_string(),
                Value::String(append_memory_to_system_prompt(existing, mem_ctx)),
            );
        }
    }

    // 3. 注入对话历史（仅对话/分析类能力需要，纯处理类不注入）
    if capability_needs_conversation_context(capability_id) && !params.contains_key("context") {
        if let Some(ref history) = exec_ctx.conversation_context {
            if !history.is_empty() {
                // 限制最近 20 条，与 Planner 保持一致
                let recent = take_recent_conversation_messages(history, 20);
                let ctx_array: Vec<Value> = recent
                    .iter()
                    .map(|msg| {
                        json!({
                            "role": msg.role,
                            "content": msg.content,
                        })
                    })
                    .collect();
                params.insert("context".to_string(), Value::Array(ctx_array));
            }
        }
    }

    params
}

/// 执行 AI 处理能力
pub async fn execute(
    capability_id: &str,
    action: &str,
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    // speech.tts 走腾讯云语音服务，不依赖 AI analyzer
    if capability_id == "speech.tts" {
        return execute_speech_tts(params).await;
    }

    let analyzer = ctx.ai_analyzer.ok_or("AI analyzer not configured")?;

    // 注入角色身份上下文到 systemPrompt（如果 Orchestrator 提供了角色 identity）
    let mut params = inject_role_identity(capability_id, params, ctx);

    // 从 __directive (Planner 主 Agent 的具体指令) 和 __user_request 提取上下文
    // 用于补充 AI handler 缺失的具体指令
    let directive = params
        .remove("__directive")
        .and_then(|v| v.as_str().map(String::from));
    let user_request = params
        .remove("__user_request")
        .and_then(|v| v.as_str().map(String::from));
    let steering = params
        .remove("__steering")
        .and_then(|v| v.as_str().map(String::from));

    // 将主 Agent 指令注入到对应的 handler 参数中
    if let Some(ref dir) = directive {
        inject_directive_to_params(capability_id, dir, user_request.as_deref(), &mut params);
    }
    if let Some(ref instruction) = steering {
        inject_steering_to_params(capability_id, instruction, &mut params);
    }

    match capability_id {
        "ai.summarize" => execute_ai_summarize(&params, analyzer).await,
        "ai.analyze" => execute_ai_analyze(&params, analyzer).await,
        "ai.recommend" => execute_ai_recommend(&params, analyzer).await,
        "ai.chat" => execute_ai_chat(&params, analyzer).await,
        "ai.webSearch" | "ai.groundingSearch" => {
            execute_gemini_grounding_search_wrapper(&params).await
        }
        "brewlia.annotate" => execute_brewlia_annotate(&params, analyzer, ctx).await,
        "brewlia.podcast" => execute_brewlia_podcast(&params, analyzer, ctx).await,
        "speech.tts" => execute_speech_tts(&params).await,
        "smart.filter" => execute_smart_filter(&params, analyzer).await,
        "compare.content" => execute_compare_content(&params, analyzer).await,
        "icon.recommend" => execute_icon_recommend(&params).await,
        "prompt.generate" => execute_prompt_generate(&params, analyzer).await,
        "translate.text" => execute_translate_text(&params, analyzer).await,
        "code.explain" => execute_code_explain(&params, analyzer).await,
        "ai.image" => execute_ai_image(&params).await,
        _ => Err(format!(
            "Unknown AI capability: {} (action: {})",
            capability_id, action
        )),
    }
}

// AI 核心能力

async fn execute_ai_summarize(
    params: &HashMap<String, Value>,
    analyzer: &crate::services::analyzer::AiAnalyzer,
) -> Result<Value, String> {
    let input = params
        .get("content")
        .or_else(|| params.get("items"))
        .or_else(|| params.get("input"))
        .or_else(|| params.get("data"))
        .cloned()
        .unwrap_or(json!(null));
    let style = params
        .get("style")
        .and_then(|v| v.as_str())
        .unwrap_or("brief");
    let max_length = params.get("maxLength").and_then(|v| v.as_u64());

    let (style_instruction, format_guide) = match style {
        "detailed" => (
            "详细总结",
            "请提供完整的结构化总结，包含主要观点、关键论据和结论。使用清晰的段落结构。",
        ),
        "bullet" => (
            "要点式总结",
            "请以要点列表形式返回，每个要点一行（使用 - 开头），提取 5-10 个最重要的要点。",
        ),
        _ => ("简要总结", "请用 2-3 句话概括核心内容，抓住最关键的信息。"),
    };

    let length_hint = match max_length {
        Some(n) => format!("总结长度不超过 {} 字。", n),
        None => String::new(),
    };

    let input_str = extract_semantic_text(&input);
    // 截断过长的输入，避免 token 溢出
    let truncated_input: String = input_str.chars().take(USER_TEXT_MAX_CHARS).collect();
    let focus = params
        .get("focus")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let focus_hint = focus
        .map(|f| format!("额外关注点：{}\n", f))
        .unwrap_or_default();

    let prompt = with_system_guidance(
        params,
        format!(
            "你是一个专业的内容分析师。请对以下内容进行{}。\n\n\
            {}\n\
            {}\n\
            {}\n\
            请使用与原文相同的语言回复。\n\n\
            内容：\n{}",
            style_instruction, format_guide, length_hint, focus_hint, truncated_input
        ),
    );

    let result = analyzer.analyze(&prompt).await.map_err(|e| {
        tracing::error!(error = %e, "AI summarize failed");
        "AI generation failed".to_string()
    })?;

    Ok(json!({
        "summary": result,
        "style": style
    }))
}

async fn execute_ai_analyze(
    params: &HashMap<String, Value>,
    analyzer: &crate::services::analyzer::AiAnalyzer,
) -> Result<Value, String> {
    let input = params.get("data").cloned().unwrap_or(json!(null));
    let analysis_type = params
        .get("analysisType")
        .and_then(|v| v.as_str())
        .unwrap_or("general");
    let instruction = params.get("instruction").and_then(|v| v.as_str());

    // 智能提取输入数据的文本内容，避免把原始 JSON 数组丢给 AI
    let input_text = extract_semantic_text(&input);
    let truncated_input: String = input_text.chars().take(USER_TEXT_MAX_CHARS).collect();

    // 当 Planner 提供了具体 instruction 时，instruction 是主要驱动指令，
    // 数据分析模板仅作为无 instruction 时的 fallback。
    // 这避免了"介绍一个角色"被套进"数据分析报告"框架的问题。
    let prompt = with_system_guidance(
        params,
        if let Some(inst) = instruction {
            let safe_inst: String = sanitize_prompt_input(inst);
            format!(
                "请根据以下指示处理数据，直接回复用户需要的内容。\n\n\
                指示：{}\n\n\
                数据：\n{}",
                safe_inst, truncated_input
            )
        } else {
            match analysis_type {
                "trend" => format!(
                    "你是一个数据分析专家。请分析以下数据中的趋势和模式。\n\n\
                    要求：\n\
                    1. 识别数据中的增长/下降趋势\n\
                    2. 指出异常值或转折点\n\
                    3. 提供可能的原因解释\n\
                    4. 给出趋势预测\n\n\
                    数据：\n{}",
                    truncated_input
                ),
                "sentiment" => format!(
                    "你是一个情感分析专家。请分析以下内容的情感倾向。\n\n\
                    要求：\n\
                    1. 判断整体情感（正面/中性/负面）及置信度\n\
                    2. 识别关键情感词汇和表达\n\
                    3. 如果有多个主题，分别分析每个主题的情感\n\
                    4. 总结情感分布\n\n\
                    内容：\n{}",
                    truncated_input
                ),
                "compare" => format!(
                    "你是一个数据比较分析专家。请对以下数据进行对比分析。\n\n\
                    要求：\n\
                    1. 列出各项数据的关键维度\n\
                    2. 逐维度对比异同\n\
                    3. 总结主要差异和共同点\n\
                    4. 给出比较结论和建议\n\n\
                    数据：\n{}",
                    truncated_input
                ),
                _ => format!(
                    "你是一个数据分析专家。请深入分析以下数据并提供洞察。\n\n\
                    要求：\n\
                    1. 概括数据的整体特征\n\
                    2. 提取 3-5 个关键发现\n\
                    3. 指出值得注意的亮点或问题\n\
                    4. 给出可行的建议\n\n\
                    数据：\n{}",
                    truncated_input
                ),
            }
        },
    );

    let result = analyzer.analyze(&prompt).await.map_err(|e| {
        tracing::error!(error = %e, "AI analysis failed");
        "AI generation failed".to_string()
    })?;

    Ok(json!({
        "analysis": result,
        "type": analysis_type
    }))
}

async fn execute_ai_recommend(
    params: &HashMap<String, Value>,
    analyzer: &crate::services::analyzer::AiAnalyzer,
) -> Result<Value, String> {
    let context = params.get("context").cloned().unwrap_or(json!({}));
    let preferences = params.get("preferences").cloned().unwrap_or(json!({}));
    let count = params.get("count").and_then(|v| v.as_u64()).unwrap_or(5);

    let context_str = serde_json::to_string_pretty(&context).unwrap_or_default();
    let truncated_context: String = context_str.chars().take(USER_TEXT_MAX_CHARS).collect();

    let prefs_str = if preferences != json!({}) {
        format!(
            "\n用户偏好：\n{}",
            serde_json::to_string_pretty(&preferences).unwrap_or_default()
        )
    } else {
        String::new()
    };

    let prompt = with_system_guidance(
        params,
        format!(
            "你是一个个性化推荐专家。基于以下用户数据，推荐 {} 个用户可能感兴趣的内容。\n\n\
            要求：\n\
            1. 每条推荐包含名称和推荐理由\n\
            2. 推荐应多样化，覆盖用户的不同兴趣点\n\
            3. 优先推荐与用户已有偏好相关但可能尚未发现的内容\n\
            4. 请直接返回 JSON 数组格式：[{{\"name\": \"...\", \"reason\": \"...\"}}]\n\
            {}\n\n\
            用户数据：\n{}",
            count, prefs_str, truncated_context
        ),
    );

    let result = analyzer.analyze(&prompt).await.map_err(|e| {
        tracing::error!(error = %e, "AI recommendation failed");
        "AI generation failed".to_string()
    })?;

    // 尝试解析 JSON 数组，否则回退到文本
    let recommendations: Value = {
        let arr = extract_json_array_from_ai_response(&result);
        if arr.is_empty() {
            json!(result)
        } else {
            json!(arr)
        }
    };

    Ok(json!({
        "recommendations": recommendations,
        "count": count
    }))
}

async fn execute_ai_chat(
    params: &HashMap<String, Value>,
    analyzer: &crate::services::analyzer::AiAnalyzer,
) -> Result<Value, String> {
    let message = params
        .get("message")
        .and_then(|v| v.as_str())
        .ok_or("Missing message parameter")?;
    let message = sanitize_prompt_input(message);

    let system_prompt = params
        .get("systemPrompt")
        .and_then(|v| v.as_str())
        .unwrap_or("你是 Agent，Myriad 平台的 AI 助手。你友好、博学，擅长帮助用户处理各种问题。回复时保持简洁和有用。");

    let context = params.get("context").and_then(|v| v.as_array());

    let mut full_prompt = format!("系统提示：{}\n\n", system_prompt);

    if let Some(history) = context {
        // 限制对话历史条数，防止 token 超限和费用滥用
        for msg in history
            .iter()
            .rev()
            .take(50)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
        {
            if let (Some(role), Some(content)) = (
                msg.get("role").and_then(|v| v.as_str()),
                msg.get("content").and_then(|v| v.as_str()),
            ) {
                full_prompt.push_str(&format!("{}：{}\n", role, content));
            }
        }
    }

    full_prompt.push_str(&format!("用户：{}\n\n请回复：", message));

    let result = analyzer.analyze(&full_prompt).await.map_err(|e| {
        tracing::error!(error = %e, "AI chat failed");
        "AI generation failed".to_string()
    })?;

    Ok(json!({
        "reply": result
    }))
}

/// Gemini Grounding Search 包装器
async fn execute_gemini_grounding_search_wrapper(
    params: &HashMap<String, Value>,
) -> Result<Value, String> {
    let query = params
        .get("query")
        .and_then(|v| v.as_str())
        .ok_or("Missing query parameter")?;

    let search_type = params
        .get("searchType")
        .and_then(|v| v.as_str())
        .unwrap_or("general");

    let max_results = params
        .get("maxResults")
        .and_then(|v| v.as_u64())
        .unwrap_or(5) as usize;

    let (ai_text, results) =
        execute_gemini_grounding_search(query, search_type, max_results).await?;

    Ok(json!({
        "success": true,
        "query": query,
        "searchType": search_type,
        "aiSummary": ai_text,
        "results": results,
        "totalResults": results.len()
    }))
}

/// 验证平台名称白名单，防止路径穿越
use crate::services::agent::executor::utils::validate_platform_name;

/// 使用 Gemini Grounding (Google Search) 进行联网搜索
async fn execute_gemini_grounding_search(
    query: &str,
    search_type: &str,
    max_results: usize,
) -> Result<(String, Vec<Value>), String> {
    let config = GLOBAL_DYNAMIC_CONFIG.read().await;
    let (api_key, model) = config
        .resolve_gemini_grounding()
        .ok_or(crate::services::agent::response_agent::api_key_not_configured("Gemini"))?;
    drop(config);

    // 清洗用户输入，防止 Prompt Injection
    let safe_query = sanitize_prompt_input(query);

    // 构建搜索提示词
    let search_prompt = match search_type {
        "rss_source" => format!(
            "搜索「{}」的 RSS 或 Atom 订阅源地址。\n\
            要求：\n\
            1. 返回可直接访问的 RSS/Atom feed URL\n\
            2. 优先返回官方 RSS 源\n\
            3. 也可以返回 RSSHub (rsshub.app) 提供的路由\n\
            4. 最多返回 {} 个结果\n\n\
            请以 JSON 数组格式返回，每个元素包含：\n\
            - name: 源名称\n\
            - url: RSS/Atom feed URL\n\
            - description: 简要说明\n\
            - source: 来源（official/rsshub/third-party）",
            safe_query, max_results
        ),
        "api_docs" => format!(
            "搜索「{}」的官方 API 文档链接。最多返回 {} 个结果。\n\
            以 JSON 数组格式返回，每个元素包含：name, url, description",
            safe_query, max_results
        ),
        _ => format!(
            "搜索关于「{}」的信息，最多返回 {} 个相关结果。\n\
            以 JSON 数组格式返回结果。",
            safe_query, max_results
        ),
    };

    // 构建 Gemini API 请求（带 Google Search grounding）
    let request_body = json!({
        "contents": [{
            "parts": [{
                "text": search_prompt
            }]
        }],
        "tools": [{
            "google_search": {}
        }],
        "generationConfig": {
            "temperature": 0.1,
            "maxOutputTokens": 2048
        }
    });

    let url = crate::services::http_client::GeminiApiUrl::generate_content_url(&model).await;

    let client = crate::services::http_client::get_gemini_grounding_client().await;

    tracing::info!(
        "Calling Gemini Grounding Search for query length={}",
        safe_query.len()
    );

    let response = client
        .post(&url)
        .header("Content-Type", "application/json")
        .header("x-goog-api-key", &api_key)
        .json(&request_body)
        .send()
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Gemini API request failed");
            "AI generation failed".to_string()
        })?;

    const GEMINI_MAX_BODY: usize = 2 * 1024 * 1024;
    if !response.status().is_success() {
        let status = response.status();
        let error_bytes =
            crate::services::outbound_security::read_limited_body(response, 64 * 1024)
                .await
                .unwrap_or_default();
        let error_text = String::from_utf8_lossy(&error_bytes);
        tracing::error!(status = %status, body = %error_text, "Gemini API error");
        return Err("AI generation failed".to_string());
    }

    let body_bytes =
        crate::services::outbound_security::read_limited_body(response, GEMINI_MAX_BODY)
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "Failed to read Gemini response");
                "AI generation failed".to_string()
            })?;
    let response_json: Value = serde_json::from_slice(&body_bytes).map_err(|e| {
        tracing::error!(error = %e, "Failed to parse Gemini response");
        "AI generation failed".to_string()
    })?;

    // 提取 AI 回复内容
    let ai_text = response_json
        .get("candidates")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("content"))
        .and_then(|c| c.get("parts"))
        .and_then(|p| p.get(0))
        .and_then(|p| p.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or("");

    // 尝试从回复中提取 JSON 数组
    let mut results = extract_json_array_from_ai_response(ai_text);

    // 提取 grounding 元数据中的搜索结果
    if let Some(grounding_metadata) = response_json
        .get("candidates")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("groundingMetadata"))
    {
        if let Some(chunks) = grounding_metadata
            .get("groundingChunks")
            .and_then(|c| c.as_array())
        {
            for chunk in chunks {
                if let Some(web) = chunk.get("web") {
                    let uri = web.get("uri").and_then(|u| u.as_str()).unwrap_or("");
                    let title = web.get("title").and_then(|t| t.as_str()).unwrap_or("");

                    if results.is_empty() && !uri.is_empty() {
                        results.push(json!({
                            "name": title,
                            "url": uri,
                            "description": format!("来源: {}", title),
                            "source": "google_search"
                        }));
                    }
                }
            }
        }
    }

    // 如果仍然没有结果，返回 AI 的文本回复作为单个结果
    if results.is_empty() && !ai_text.is_empty() {
        results.push(json!({
            "name": "AI 搜索结果",
            "description": ai_text,
            "source": "gemini_grounding"
        }));
    }

    Ok((ai_text.to_string(), results))
}

// Brewlia 能力

async fn execute_brewlia_annotate(
    params: &HashMap<String, Value>,
    analyzer: &crate::services::analyzer::AiAnalyzer,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let item_id = params
        .get("itemId")
        .and_then(|v| v.as_i64())
        .ok_or("Missing itemId parameter")?;

    // 从数据库获取文章实际内容
    let item = brew_items::Entity::find_by_id(item_id as i32)
        .one(ctx.db)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Agent ai_process database error");
            "Database error".to_string()
        })?
        .ok_or_else(|| format!("Article with ID {} not found", item_id))?;

    let title = &item.title;
    let content = item
        .content
        .as_deref()
        .unwrap_or_else(|| item.summary.as_deref().unwrap_or(""));
    // 截断过长文章，保留核心内容
    let truncated_content: String = content.chars().take(USER_TEXT_MAX_CHARS).collect();

    let prompt = format!(
        "你是一个专业的阅读理解助手。请为以下文章生成详细的阅读注释。\n\n\
        文章标题：{}\n\
        文章内容：\n{}\n\n\
        请生成以下类型的注释：\n\
        1. 关键术语解释 — 文章中的专业术语、缩写、技术概念等\n\
        2. 背景知识补充 — 帮助读者理解的相关背景信息\n\
        3. 延伸阅读建议 — 相关主题和概念\n\n\
        请以 JSON 数组格式返回，每个元素包含：\n\
        {{\"type\": \"term|background|extension\", \"term\": \"关键词\", \"explanation\": \"解释内容\"}}\n\n\
        请直接返回 JSON 数组，不要包含 markdown 标记。",
        title, truncated_content
    );

    let result = analyzer
        .analyze(&prompt)
        .await
        .map_err(|error| ai_step_failed("Annotation generation failed", error))?;

    let annotations: Value = {
        let arr = extract_json_array_from_ai_response(&result);
        if arr.is_empty() {
            json!([{
                "type": "note",
                "term": "AI 生成注释",
                "explanation": result
            }])
        } else {
            json!(arr)
        }
    };

    Ok(json!({
        "annotations": annotations,
        "fromCache": false,
        "itemId": item_id
    }))
}

async fn execute_brewlia_podcast(
    params: &HashMap<String, Value>,
    analyzer: &crate::services::analyzer::AiAnalyzer,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let item_id = params
        .get("itemId")
        .and_then(|v| v.as_i64())
        .ok_or("Missing itemId parameter")?;

    let style = params
        .get("style")
        .and_then(|v| v.as_str())
        .unwrap_or("casual");

    // 从数据库获取文章实际内容
    let item = brew_items::Entity::find_by_id(item_id as i32)
        .one(ctx.db)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Agent ai_process database error");
            "Database error".to_string()
        })?
        .ok_or_else(|| format!("Article with ID {} not found", item_id))?;

    let title = &item.title;
    let content = item
        .content
        .as_deref()
        .unwrap_or_else(|| item.summary.as_deref().unwrap_or(""));
    let truncated_content: String = content.chars().take(USER_TEXT_MAX_CHARS).collect();
    let author = item.author.as_deref().unwrap_or("未知");

    let style_desc = match style {
        "professional" => "专业、正式的商业播客风格。使用严谨的语言，适当引用数据",
        "educational" => "教育性质、通俗易懂的讲解风格。多用类比和举例帮助理解",
        _ => "轻松、对话式的闲聊风格。语气亲切自然，可以加入幽默元素",
    };

    let prompt = format!(
        "你是一个专业的播客编剧。请将以下文章转换为双人播客对话文稿。\n\n\
        文章标题：{}\n\
        文章作者：{}\n\
        文章内容：\n{}\n\n\
        风格要求：{}\n\n\
        格式要求：\n\
        - 两个主持人（A 和 B）的对话\n\
        - 结构：开场介绍（简要引出话题）→ 正文讨论（深入探讨文章要点）→ 结尾总结（核心观点回顾）\n\
        - 每段对话以 A：或 B：开头\n\
        - 对话应自然流畅，A 主要负责引导话题，B 负责补充观点和提问\n\
        - 忠实于原文内容，不要编造文章中没有的事实\n\
        - 总长度约 800-1500 字\n\n\
        请直接输出对话文稿。",
        title, author, truncated_content, style_desc
    );

    let result = analyzer
        .analyze(&prompt)
        .await
        .map_err(|error| ai_step_failed("Podcast script generation failed", error))?;

    // 基于中文平均语速约 200 字/分钟估算
    let estimated_duration = result.chars().count() as f64 / 200.0;

    Ok(json!({
        "script": result,
        "duration": estimated_duration,
        "style": style,
        "itemId": item_id
    }))
}

// 其他 AI 能力

async fn execute_speech_tts(params: &HashMap<String, Value>) -> Result<Value, String> {
    use crate::services::standalone_tts::{synthesize_standalone_tts, TtsApiRequest};

    let text = params
        .get("text")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Missing text for speech".to_string())?;

    let voice_type = params
        .get("voice")
        .or_else(|| params.get("voice_type"))
        .or_else(|| params.get("voiceType"))
        .and_then(|v| {
            v.as_i64()
                .or_else(|| v.as_u64().map(|u| u as i64))
                .or_else(|| v.as_str().and_then(|s| s.parse::<i64>().ok()))
        })
        .map(|v| v as i32);

    // Agent schema uses speed as relative multiplier (default 1.0).
    // Product / Tencent API expects speed in roughly [-2, 6]; map 1.0 → 0.0.
    let speed = params.get("speed").and_then(|v| v.as_f64()).map(|s| {
        let mapped = if (0.5..=2.0).contains(&s) {
            (s - 1.0) * 2.0
        } else {
            s
        };
        mapped as f32
    });

    let codec = params
        .get("codec")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let volume = params
        .get("volume")
        .and_then(|v| v.as_f64())
        .map(|v| v as f32);

    let sample_rate = params
        .get("sample_rate")
        .or_else(|| params.get("sampleRate"))
        .and_then(|v| v.as_i64().or_else(|| v.as_u64().map(|u| u as i64)))
        .map(|v| v as i32);

    let emotion = params
        .get("emotion")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let force_regenerate = params
        .get("force_regenerate")
        .or_else(|| params.get("forceRegenerate"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let request = TtsApiRequest {
        text: text.to_string(),
        voice_type,
        speed,
        volume,
        codec: codec.clone(),
        sample_rate,
        emotion,
        force_regenerate,
    };

    let response = synthesize_standalone_tts(&request).await?;
    let audio = response
        .audio
        .ok_or_else(|| "Speech service returned no audio".to_string())?;

    let estimated_duration_sec = text.chars().count() as f64 / 200.0 * 60.0;
    let codec_out = codec.unwrap_or_else(|| "mp3".to_string());

    Ok(json!({
        "success": true,
        "audio": audio,
        "audioBase64": audio,
        "codec": codec_out,
        "sessionId": response.session_id,
        "cached": response.cached,
        "duration": estimated_duration_sec,
        "voice": voice_type,
        "textLength": text.chars().count(),
        "frontendAction": {
            "type": "play_audio",
            "params": {
                "audioBase64": audio,
                "codec": codec_out
            },
            "timestamp": chrono::Utc::now().timestamp_millis()
        }
    }))
}

async fn execute_smart_filter(
    params: &HashMap<String, Value>,
    analyzer: &crate::services::analyzer::AiAnalyzer,
) -> Result<Value, String> {
    let platform_raw = params
        .get("platform")
        .and_then(|v| v.as_str())
        .unwrap_or("all");

    // 白名单校验，防止路径穿越
    let platform = validate_platform_name(platform_raw)?;

    let filtered_file = format!("cache/platforms/{}_filtered.json", platform);

    if let Ok(content) = tokio::fs::read_to_string(&filtered_file).await {
        if let Ok(data) = serde_json::from_str::<Value>(&content) {
            return Ok(json!({
                "platform": platform,
                "status": "cached",
                "data": data.get("content_analysis").cloned().unwrap_or(json!({})),
                "message": "Using cached filtered data"
            }));
        }
    }

    let raw_file = format!("cache/raw/{}.json", platform); // platform 已经过白名单校验
    if let Ok(content) = tokio::fs::read_to_string(&raw_file).await {
        if let Ok(raw_data) = serde_json::from_str::<Value>(&content) {
            let raw_str = serde_json::to_string_pretty(&raw_data).unwrap_or_default();
            let truncated: String = raw_str.chars().take(USER_TEXT_MAX_CHARS).collect();
            // 检查截断是否在 JSON 中间，尝试保持完整性
            let safe_truncated = if truncated.len() < raw_str.len() {
                format!("{}... (数据已截断)", truncated)
            } else {
                truncated
            };

            let prompt = format!(
                "你是一个平台数据分析专家。请分析以下 {} 平台的数据，提取关键信息并进行分类。\n\n\
                要求：\n\
                1. 提取用户活跃度指标（数量、频率等）\n\
                2. 识别内容类型和偏好分布\n\
                3. 标注有价值的数据点\n\
                4. 返回结构化的 JSON 结果\n\n\
                数据：\n{}",
                platform, safe_truncated
            );

            let result = analyzer
                .analyze(&prompt)
                .await
                .map_err(|error| ai_step_failed("Smart filter failed", error))?;

            return Ok(json!({
                "platform": platform,
                "status": "analyzed",
                "analysis": result
            }));
        }
    }

    Err(format!("No data available for platform: {}", platform))
}

async fn execute_compare_content(
    params: &HashMap<String, Value>,
    analyzer: &crate::services::analyzer::AiAnalyzer,
) -> Result<Value, String> {
    let platform_raw = params
        .get("platform")
        .and_then(|v| v.as_str())
        .unwrap_or("all");

    // 白名单校验，防止路径穿越
    let platform = validate_platform_name(platform_raw)?;
    let start_date = params.get("startDate").and_then(|v| v.as_str());
    let end_date = params.get("endDate").and_then(|v| v.as_str());

    let cache_file = format!("cache/platforms/{}_filtered.json", platform);

    if let Ok(content) = tokio::fs::read_to_string(&cache_file).await {
        if let Ok(data) = serde_json::from_str::<Value>(&content) {
            let data_str = serde_json::to_string_pretty(&data).unwrap_or_default();
            let truncated: String = data_str.chars().take(USER_TEXT_MAX_CHARS).collect();

            let time_range = match (start_date, end_date) {
                (Some(s), Some(e)) => format!("时间范围：{} 到 {}", s, e),
                (Some(s), None) => format!("起始时间：{}", s),
                (None, Some(e)) => format!("截止时间：{}", e),
                _ => "时间范围：全部可用数据".to_string(),
            };

            let prompt = format!(
                "你是一个数据分析专家。请分析以下 {} 平台的数据快照，提供洞察和分析。\n\n\
                {}\n\n\
                注意：这是当前时间点的数据快照。请基于数据中可见的信息进行分析：\n\
                1. 数据量和内容分布概况\n\
                2. 用户的内容偏好和兴趣方向\n\
                3. 活跃度评估\n\
                4. 值得关注的发现或亮点\n\n\
                数据：\n{}",
                platform, time_range, truncated
            );

            let result = analyzer
                .analyze(&prompt)
                .await
                .map_err(|error| ai_step_failed("Content comparison failed", error))?;

            return Ok(json!({
                "platform": platform,
                "period": { "start": start_date, "end": end_date },
                "analysis": result
            }));
        }
    }

    Err(format!("No data available for comparison: {}", platform))
}

async fn execute_icon_recommend(params: &HashMap<String, Value>) -> Result<Value, String> {
    let platform_name = params
        .get("platformName")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let platform_lower = platform_name.to_lowercase();

    let (icon_name, color) =
        if platform_lower.contains("bilibili") || platform_lower.contains("b站") {
            ("SiBilibili", "#00A1D6")
        } else if platform_lower.contains("steam") {
            ("SiSteam", "#000000")
        } else if platform_lower.contains("github") {
            ("SiGithub", "#181717")
        } else if platform_lower.contains("netease") || platform_lower.contains("网易") {
            ("SiNeteasecloudmusic", "#C20C0C")
        } else if platform_lower.contains("twitter") || platform_lower.contains("x") {
            ("SiX", "#000000")
        } else if platform_lower.contains("youtube") {
            ("SiYoutube", "#FF0000")
        } else if platform_lower.contains("spotify") {
            ("SiSpotify", "#1DB954")
        } else if platform_lower.contains("discord") {
            ("SiDiscord", "#5865F2")
        } else {
            ("FaGlobe", "#6B7280")
        };

    Ok(json!({
        "platformName": platform_name,
        "iconType": "react-icons",
        "iconName": icon_name,
        "colorSuggestion": color
    }))
}

async fn execute_prompt_generate(
    params: &HashMap<String, Value>,
    analyzer: &crate::services::analyzer::AiAnalyzer,
) -> Result<Value, String> {
    let title = params.get("title").and_then(|v| v.as_str()).unwrap_or("");
    let summary = params.get("summary").and_then(|v| v.as_str()).unwrap_or("");
    let description = params
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let category = params.get("category").and_then(|v| v.as_str());
    let style = params.get("style").and_then(|v| v.as_str());

    let style_hint = match style {
        Some(s) => format!("Preferred style: {}", s),
        None => match category {
            Some("anime") => {
                "Preferred style: high quality anime illustration, anime key visual".to_string()
            }
            Some("photo") => "Preferred style: photorealistic, 8k UHD, DSLR".to_string(),
            _ => "Preferred style: detailed digital art, high quality".to_string(),
        },
    };

    // 构建丰富的上下文信息
    let mut context_parts: Vec<String> = Vec::new();
    if !title.is_empty() {
        context_parts.push(format!("Subject/Title: {}", title));
    }
    if !summary.is_empty() {
        context_parts.push(format!("Summary: {}", summary));
    }
    if !description.is_empty() {
        context_parts.push(format!("Detailed description: {}", description));
    }
    context_parts.push(style_hint);

    let prompt = with_system_guidance(
        params,
        format!(
            "You are an expert AI image prompt engineer. Your task is to generate a highly detailed, \
            accurate image generation prompt (for Stable Diffusion / DALL-E / Flux) based on the following request.\n\n\
            {}\n\n\
            CRITICAL INSTRUCTIONS:\n\
            1. If the subject is a known character (from anime, games, manga, etc.), you MUST use your knowledge \
            to include their EXACT visual features: specific hair color and style, eye color, signature outfit/clothing \
            details, accessories, and any unique physical traits. Do NOT guess or generalize — be precise.\n\
            2. Describe the character's appearance in meticulous detail: hairstyle, hair color, eye color (heterochromia if applicable), \
            clothing (specific garments, colors, patterns, accessories like hats/ribbons/capes), body pose, and expression.\n\
            3. Include composition details: background scene, lighting (e.g. dramatic rim lighting, soft sunlight), \
            camera angle (close-up, full body, portrait), atmosphere and mood.\n\
            4. Include quality boosting tags: masterpiece, best quality, highly detailed, sharp focus, etc.\n\
            5. The prompt must be in English. Be as specific and descriptive as possible.\n\
            6. Maximum {IMAGE_PROMPT_MAX_CHARS} characters.\n\n\
            Output ONLY the raw prompt text. No explanations, no markdown, no quotes, no formatting.",
            context_parts.join("\n")
        ),
    );

    let result = analyzer
        .analyze(&prompt)
        .await
        .map_err(|error| ai_step_failed("Prompt generation failed", error))?;

    // 清理：去除 AI 可能添加的引号和多余空白
    let cleaned = result.trim().trim_matches('"').trim_matches('`').trim();

    // 根据类别生成更合适的 negative prompt
    let negative_prompt = match category {
        Some("anime") => "low quality, worst quality, blurry, deformed, ugly, bad anatomy, bad hands, extra fingers, missing fingers, extra limbs, bad proportions, watermark, text, signature, 3d, realistic",
        Some("photo") => "low quality, blurry, deformed, ugly, bad anatomy, cartoon, anime, drawing, painting, watermark, text",
        _ => "low quality, worst quality, blurry, deformed, ugly, bad anatomy, bad hands, extra fingers, missing fingers, extra limbs, bad proportions, watermark, text, signature",
    };

    Ok(json!({
        "prompt": cleaned,
        "negativePrompt": negative_prompt,
        "title": title
    }))
}

async fn execute_translate_text(
    params: &HashMap<String, Value>,
    analyzer: &crate::services::analyzer::AiAnalyzer,
) -> Result<Value, String> {
    let text = params
        .get("text")
        .and_then(|v| v.as_str())
        .ok_or("Missing text parameter")?;
    let target_lang = params
        .get("targetLang")
        .and_then(|v| v.as_str())
        .unwrap_or("zh-CN");
    let source_lang = params.get("sourceLang").and_then(|v| v.as_str());

    let prompt = with_system_guidance(
        params,
        format!(
            "请将以下文本翻译成{}：\n\n{}\n\n直接输出翻译结果。",
            match target_lang {
                "zh-CN" | "zh" => "简体中文",
                "zh-TW" => "繁体中文",
                "en" => "英文",
                "ja" => "日文",
                "ko" => "韩文",
                _ => target_lang,
            },
            text
        ),
    );

    let result = analyzer
        .analyze(&prompt)
        .await
        .map_err(|error| ai_step_failed("Translation failed", error))?;

    Ok(json!({
        "originalText": text,
        "translated": result.trim(),
        "targetLang": target_lang,
        "sourceLang": source_lang
    }))
}

async fn execute_code_explain(
    params: &HashMap<String, Value>,
    analyzer: &crate::services::analyzer::AiAnalyzer,
) -> Result<Value, String> {
    let code = params
        .get("code")
        .and_then(|v| v.as_str())
        .ok_or("Missing code parameter")?;
    let language = params.get("language").and_then(|v| v.as_str());

    let prompt = with_system_guidance(
        params,
        format!(
            "请解释以下{}代码的功能和逻辑：\n\n```{}\n{}\n```\n\n\
            请包含：代码整体功能、主要逻辑步骤、关键变量说明。",
            language.unwrap_or(""),
            language.unwrap_or(""),
            code
        ),
    );

    let result = analyzer
        .analyze(&prompt)
        .await
        .map_err(|error| ai_step_failed("Code explanation failed", error))?;

    let complexity = if code.len() < 100 {
        "简单"
    } else if code.len() < 500 {
        "中等"
    } else {
        "复杂"
    };

    Ok(json!({
        "code": code.chars().take(200).collect::<String>() + if code.len() > 200 { "..." } else { "" },
        "language": language,
        "explanation": result,
        "complexity": complexity
    }))
}

// AI 图片生成

async fn execute_ai_image(params: &HashMap<String, Value>) -> Result<Value, String> {
    let prompt = resolve_image_prompt(params)?;
    let (width, height) = resolve_image_dimensions(params);

    let dynamic = GLOBAL_DYNAMIC_CONFIG.read().await;
    let config = crate::services::image_generation::config_from_dynamic(&dynamic)
        .map_err(|error| error.to_string())?;
    drop(dynamic);

    let generated =
        crate::services::image_generation::generate_image(&config, &prompt, width, height, None)
            .await
            .map_err(|error| error.to_string())?;
    let image_url = crate::services::image_generation::persist_generated(&generated)
        .await
        .map_err(|error| error.to_string())?;

    Ok(json!({
        "imageUrl": image_url,
        "width": generated.width,
        "height": generated.height,
        "provider": config.provider,
        "prompt": prompt,
    }))
}
