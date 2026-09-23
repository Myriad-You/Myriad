//! AI 处理能力处理器
//!
//! 处理 ai.summarize, ai.analyze, ai.chat, ai.groundingSearch 等 AI 类能力。
//! 纯 prompt/steering/image 规则见 [`crate::services::agent::ai_process_pure`]。

use super::HandlerContext;
use crate::GLOBAL_DYNAMIC_CONFIG;
use crate::models::entities::phantasi_items;
use crate::services::agent::ai_process_pure::{
    IMAGE_PROMPT_MAX_CHARS, USER_TEXT_MAX_CHARS, append_memory_to_system_prompt,
    capability_needs_conversation_context, capability_needs_memory, extract_semantic_text,
    inject_directive_to_params, inject_steering_to_params, merge_system_prompt,
    resolve_image_dimensions, resolve_image_prompt, sanitize_prompt_input,
    take_recent_conversation_messages, task_image_envelope, task_json_envelope, task_text_envelope,
    with_system_guidance,
};
use crate::services::agent::data_read_pure::extract_json_array_from_ai_response;
use crate::services::agent::external_pure::classify_outbound_fetch;
use crate::services::data_paths::platform_filtered_file;
use myriad_agent_rules::untrusted_block;
use sea_orm::EntityTrait;
use serde_json::{Value, json};
use std::collections::HashMap;

fn ai_step_failed(label: &str, error: impl std::fmt::Display) -> String {
    let detail = error.to_string();
    tracing::error!(error = %detail, label, "AI step failed");
    classify_outbound_fetch(label, &detail)
}

/// 注入执行上下文到 AI 参数：角色身份、记忆、对话历史
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

    // 2. 注入记忆上下文（`capability_needs_memory`）
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

    // 3. 注入对话历史（`capability_needs_conversation_context`，params 已有 `context` 则跳过）
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
    // speech.tts 走独立 TTS（OpenAI / Gemini / MiniMax / 腾讯），不依赖 AI analyzer
    if capability_id == "speech.tts" {
        return execute_speech_tts(params).await;
    }
    if capability_id == "seo.generate" {
        return super::seo::execute_seo_generate(params, ctx).await;
    }

    let analyzer = ctx.ai_analyzer.ok_or("AI analyzer not configured")?;

    // 注入角色身份 / 记忆 / 对话到 params
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
            crate::services::agent::web_search::execute_capability(&params).await
        }
        "phantasiai.annotate" => execute_phantasiai_annotate(&params, analyzer, ctx).await,
        "phantasiai.podcast" => execute_phantasiai_podcast(&params, analyzer, ctx).await,
        "speech.tts" => execute_speech_tts(&params).await,
        "smart.filter" => execute_smart_filter(&params, analyzer).await,
        "compare.content" => execute_compare_content(&params, analyzer).await,
        "icon.recommend" => execute_icon_recommend(&params).await,
        "prompt.generate" => execute_prompt_generate(&params, analyzer).await,
        "translate.text" => execute_translate_text(&params, analyzer).await,
        "code.explain" => execute_code_explain(&params, analyzer).await,
        "ai.image" => execute_ai_image(&params, ctx).await,
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
            "a detailed summary",
            "Write a structured summary with main points, key arguments, and a conclusion. Use clear paragraphs.",
        ),
        "bullet" => (
            "a bullet summary",
            "Return a bullet list, one point per line (start with -). Extract the 5–10 most important points.",
        ),
        _ => (
            "a brief summary",
            "Summarize the core in 2–3 sentences. Keep only the most important information.",
        ),
    };

    let length_hint = match max_length {
        Some(n) => format!("Keep the summary under {n} characters."),
        None => String::new(),
    };

    let input_str = extract_semantic_text(&input);
    // 截断过长的输入，避免 token 溢出
    let truncated_input: String = input_str.chars().take(USER_TEXT_MAX_CHARS).collect();
    // `data` 由 Planner 用 `dataFrom` 从上游步骤接过来，里面可能是 webSearch
    // 或 scrape 抓回来的正文。产物只是文字不是动作，但仍然不能让正文里的
    // 祈使句改写「总结」这件事本身。
    let truncated_input = untrusted_block("input", &truncated_input);
    let focus = params
        .get("focus")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let focus_hint = focus
        .map(|f| format!("Also focus on: {}\n", f))
        .unwrap_or_default();

    let prompt = with_system_guidance(
        params,
        format!(
            "You are a professional content analyst. Produce {} of the following.\n\n\
            {}\n\
            {}\n\
            {}\n\
            Reply in the same language as the source.\n\n\
            Content:\n{}",
            style_instruction, format_guide, length_hint, focus_hint, truncated_input
        ),
    );

    let result = analyzer.analyze(&prompt).await.map_err(|e| {
        tracing::error!(error = %e, "AI summarize failed");
        "AI generation failed".to_string()
    })?;

    Ok(task_json_envelope(json!({
        "summary": result,
        "style": style
    })))
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

    // 提取输入文本；非对象会落到 pretty JSON
    let input_text = extract_semantic_text(&input);
    let truncated_input: String = input_text.chars().take(USER_TEXT_MAX_CHARS).collect();
    // `data` 是上游输出，来源不可信（summarize 用 `input` 标签）。
    let truncated_input = untrusted_block("data", &truncated_input);

    // 当 Planner 提供了具体 instruction 时，instruction 是主要驱动指令，
    // 数据分析模板仅作为无 instruction 时的 fallback。
    // 这避免了"介绍一个角色"被套进"数据分析报告"框架的问题。
    let prompt = with_system_guidance(
        params,
        if let Some(inst) = instruction {
            let safe_inst: String = sanitize_prompt_input(inst);
            format!(
                "Follow the instruction below and answer with what the user needs.\n\n\
                Instruction: {}\n\n\
                Data:\n{}",
                safe_inst, truncated_input
            )
        } else {
            match analysis_type {
                "trend" => format!(
                    "You are a data analyst. Find trends and patterns in the data below.\n\n\
                    Requirements:\n\
                    1. Identify rising/falling trends\n\
                    2. Call out outliers or turning points\n\
                    3. Suggest possible causes\n\
                    4. Offer a short forecast\n\n\
                    Data:\n{}",
                    truncated_input
                ),
                "sentiment" => format!(
                    "You are a sentiment analyst. Judge the tone of the text below.\n\n\
                    Requirements:\n\
                    1. Overall sentiment (positive/neutral/negative) and confidence\n\
                    2. Key sentiment words and phrases\n\
                    3. If there are several topics, score each\n\
                    4. Summarize the sentiment mix\n\n\
                    Content:\n{}",
                    truncated_input
                ),
                "compare" => format!(
                    "You are a comparison analyst. Compare the data below.\n\n\
                    Requirements:\n\
                    1. List key dimensions for each item\n\
                    2. Compare sameness and difference per dimension\n\
                    3. Summarize the main gaps and overlaps\n\
                    4. Give a conclusion and advice\n\n\
                    Data:\n{}",
                    truncated_input
                ),
                _ => format!(
                    "You are a data analyst. Analyze the data below and give insights.\n\n\
                    Requirements:\n\
                    1. Overall shape of the data\n\
                    2. 3–5 key findings\n\
                    3. Highlights or problems worth noticing\n\
                    4. Actionable advice\n\n\
                    Data:\n{}",
                    truncated_input
                ),
            }
        },
    );

    let result = analyzer.analyze(&prompt).await.map_err(|e| {
        tracing::error!(error = %e, "AI analysis failed");
        "AI generation failed".to_string()
    })?;

    Ok(task_json_envelope(json!({
        "analysis": result,
        "type": analysis_type
    })))
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
            "\nUser preferences:\n{}",
            serde_json::to_string_pretty(&preferences).unwrap_or_default()
        )
    } else {
        String::new()
    };

    let prompt = with_system_guidance(
        params,
        format!(
            "You are a recommendation specialist. Based on the user data below, recommend {} items they may like.\n\n\
            Requirements:\n\
            1. Each item has a name and a reason\n\
            2. Diversify across their interests\n\
            3. Prefer related items they may not have found yet\n\
            4. Return a JSON array only: [{{\"name\": \"...\", \"reason\": \"...\"}}]\n\
            {}\n\n\
            User data:\n{}",
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

    Ok(task_json_envelope(json!({
        "recommendations": recommendations,
        "count": count
    })))
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
        .unwrap_or("You are Agent, Myriad's AI assistant. Be concise and useful.");

    let context = params.get("context").and_then(|v| v.as_array());

    let mut full_prompt = format!("System prompt: {}\n\n", system_prompt);

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

    full_prompt.push_str(&format!("User: {}\n\nReply:", message));

    let result = analyzer.analyze(&full_prompt).await.map_err(|e| {
        tracing::error!(error = %e, "AI chat failed");
        "AI generation failed".to_string()
    })?;

    Ok(task_text_envelope(result))
}

/// 验证平台名称白名单，防止路径穿越
use crate::services::agent::executor::utils::validate_platform_name;

// Phantasiai 能力

async fn execute_phantasiai_annotate(
    params: &HashMap<String, Value>,
    analyzer: &crate::services::analyzer::AiAnalyzer,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let item_id = params
        .get("itemId")
        .and_then(|v| v.as_i64())
        .ok_or("Missing itemId parameter")?;

    // 从数据库获取文章实际内容
    let item = phantasi_items::Entity::find_by_id(item_id as i32)
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
    // 按字符上限截断文章前缀
    let truncated_content: String = content.chars().take(USER_TEXT_MAX_CHARS).collect();

    let prompt = format!(
        "You are a reading-comprehension assistant. Write detailed annotations for the article below.\n\n\
        Title: {}\n\
        Content:\n{}\n\n\
        Produce these kinds of notes:\n\
        1. Term — jargon, abbreviations, technical concepts\n\
        2. Background — context the reader needs\n\
        3. Extension — related topics\n\n\
        Write explanations in the same language as the article.\n\
        Return a JSON array only, no markdown. Each item:\n\
        {{\"type\": \"term|background|extension\", \"term\": \"keyword\", \"explanation\": \"explanation\"}}",
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
                "term": "AI annotation",
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

async fn execute_phantasiai_podcast(
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
    let item = phantasi_items::Entity::find_by_id(item_id as i32)
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
    let author = item.author.as_deref().unwrap_or("Unknown");

    let style_desc = match style {
        "professional" => {
            "Professional, formal business-podcast tone. Precise language; cite figures when useful."
        }
        "educational" => "Educational and plain. Use analogies and examples.",
        _ => "Casual two-host chat. Warm, natural, humor allowed.",
    };

    let prompt = format!(
        "You are a podcast script writer. Turn the article below into a two-host dialogue.\n\n\
        Title: {}\n\
        Author: {}\n\
        Content:\n{}\n\n\
        Style: {}\n\n\
        Format:\n\
        - Dialogue between hosts A and B\n\
        - Structure: intro (set up the topic) → discussion (cover the article's points) → close (takeaways)\n\
        - Each turn starts with A: or B:\n\
        - A steers the topic; B adds points and questions\n\
        - Stay faithful to the article; do not invent facts\n\
        - About 800–1500 characters\n\
        - Write in the same language as the article\n\n\
        Output the script only.",
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
    use crate::services::standalone_tts::{TtsApiRequest, synthesize_standalone_tts};

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

    // Agent schema 语速是倍率（缺省 1.0）；[0.5, 2.0] 映射为 (s-1)*2（1.0→0.0），其余原样传给 TTS。
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

    let (input, output) = crate::services::ai_cost_ledger::estimate_tts_tokens(text);
    crate::services::analyzer::request_budget::charge_units(
        input.max(0) as u64 + output.max(0) as u64,
    )
    .map_err(|error| error.to_string())?;
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

    let filtered_file = platform_filtered_file(platform);

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

    let raw_file = crate::services::data_paths::platform_raw_file(platform); // platform 已经过白名单校验
    if let Ok(content) = tokio::fs::read_to_string(&raw_file).await {
        if let Ok(raw_data) = serde_json::from_str::<Value>(&content) {
            let raw_str = serde_json::to_string_pretty(&raw_data).unwrap_or_default();
            let truncated: String = raw_str.chars().take(USER_TEXT_MAX_CHARS).collect();
            // 超长则追加 truncated 标记（不解析 JSON 边界）
            let safe_truncated = if truncated.len() < raw_str.len() {
                format!("{}... (truncated)", truncated)
            } else {
                truncated
            };

            let prompt = format!(
                "You are a platform data analyst. Analyze the {} data below, extract key facts, and classify them.\n\n\
                Requirements:\n\
                1. Activity metrics (counts, frequency)\n\
                2. Content types and preference mix\n\
                3. Call out valuable data points\n\
                4. Return structured JSON\n\n\
                Data:\n{}",
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

    let cache_file = platform_filtered_file(platform);

    if let Ok(content) = tokio::fs::read_to_string(&cache_file).await {
        if let Ok(data) = serde_json::from_str::<Value>(&content) {
            let data_str = serde_json::to_string_pretty(&data).unwrap_or_default();
            let truncated: String = data_str.chars().take(USER_TEXT_MAX_CHARS).collect();

            let time_range = match (start_date, end_date) {
                (Some(s), Some(e)) => format!("Time range: {s} to {e}"),
                (Some(s), None) => format!("From: {s}"),
                (None, Some(e)) => format!("Until: {e}"),
                _ => "Time range: all available data".to_string(),
            };

            let prompt = format!(
                "You are a data analyst. Analyze this {} platform snapshot and give insights.\n\n\
                {}\n\n\
                This is a snapshot at the current time. Use only what is visible:\n\
                1. Volume and content mix\n\
                2. Preferences and interests\n\
                3. Activity level\n\
                4. Highlights worth noticing\n\n\
                Data:\n{}",
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
        Some("anime") => {
            "low quality, worst quality, blurry, deformed, ugly, bad anatomy, bad hands, extra fingers, missing fingers, extra limbs, bad proportions, watermark, text, signature, 3d, realistic"
        }
        Some("photo") => {
            "low quality, blurry, deformed, ugly, bad anatomy, cartoon, anime, drawing, painting, watermark, text"
        }
        _ => {
            "low quality, worst quality, blurry, deformed, ugly, bad anatomy, bad hands, extra fingers, missing fingers, extra limbs, bad proportions, watermark, text, signature"
        }
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
        .unwrap_or("en-US");
    let source_lang = params.get("sourceLang").and_then(|v| v.as_str());

    let prompt = with_system_guidance(
        params,
        format!(
            "Translate the following text into {}.\n\n{}\n\nOutput the translation only.",
            match target_lang {
                "zh-CN" | "zh" => "Simplified Chinese",
                "zh-TW" => "Traditional Chinese",
                "en" => "English",
                "ja" => "Japanese",
                "ko" => "Korean",
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
            "Explain what this {} code does and how it works:\n\n```{}\n{}\n```\n\n\
            Cover: overall purpose, main steps, and key variables.",
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
        "simple"
    } else if code.len() < 500 {
        "medium"
    } else {
        "complex"
    };

    Ok(json!({
        "code": code.chars().take(200).collect::<String>() + if code.len() > 200 { "..." } else { "" },
        "language": language,
        "explanation": result,
        "complexity": complexity
    }))
}

// AI 图片生成

async fn execute_ai_image(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
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
    let (width, height) = (generated.width, generated.height);
    let persisted = crate::services::image_generation::persist_generated_with_status(
        ctx.db,
        crate::services::media::task_media_context(ctx.user_id, ctx.user_id).with_producer_key(
            ctx.task_id
                .as_deref()
                .map(|id| format!("agent:{id}:image"))
                .unwrap_or_default(),
        ),
        generated,
        "generated",
    )
    .await
    .map_err(|error| error.to_string())?;

    Ok(task_image_envelope(&persisted.url, width, height))
}
