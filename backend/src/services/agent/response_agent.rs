//! 响应生成副 Agent
//!
//! Work 完成/确认/进度等面向用户的文案。Chat 正文不走 `generate_final_response`；流结束走 `finish_stream`。
//!
//! 两种模式：
//! - **AI 模式**：调用 AI 模型生成个性化回复（用于最终回复、多步骤汇总）
//! - **模板模式**：同步返回预设的温暖模板（用于实时进度、简单状态）

use serde_json::Value;

use super::types::AgentProgressEvent;
use crate::services::analyzer::StreamDelta;

pub(crate) async fn emit_stream_delta(
    tx: &tokio::sync::mpsc::Sender<AgentProgressEvent>,
    delta: StreamDelta,
) {
    let event = match delta {
        StreamDelta::Reasoning(token) => AgentProgressEvent::ThinkingToken { token, done: false },
        StreamDelta::Text(token) => AgentProgressEvent::SummaryToken { token, done: false },
    };
    let _ = tx.send(event).await;
}

/// Seal ThinkingToken and SummaryToken with empty `done: true` events.
pub(crate) async fn finish_stream(tx: &tokio::sync::mpsc::Sender<AgentProgressEvent>) {
    let _ = tx
        .send(AgentProgressEvent::ThinkingToken {
            token: String::new(),
            done: true,
        })
        .await;
    let _ = tx
        .send(AgentProgressEvent::SummaryToken {
            token: String::new(),
            done: true,
        })
        .await;
}

// ─────────────────────────────────────────────
// 1. AI 驱动的最终回复生成（异步，支持流式）
// ─────────────────────────────────────────────

// ─────────────────────────────────────────────
// 2. 同步模板消息（用于实时进度、状态等）
// ─────────────────────────────────────────────

/// 步骤进度摘要（实时显示给用户的步骤状态，同步，不调用 AI）
pub fn summarize_step_output(output: &Value) -> Option<String> {
    if let Some(obj) = output.as_object() {
        // 图片生成
        let inner = crate::services::agent::ai_process_pure::task_inner_value(output);
        if inner
            .get("url")
            .or_else(|| inner.get("imageUrl"))
            .and_then(|v| v.as_str())
            .is_some()
            || obj.get("imageUrl").and_then(|v| v.as_str()).is_some()
        {
            let prompt = obj.get("prompt").and_then(|v| v.as_str()).unwrap_or("");
            if !prompt.is_empty() {
                return Some(format!("Generating an image: {prompt}"));
            }
            return Some("Image generated".to_string());
        }
        if let Some(text) = inner.as_str().filter(|s| !s.is_empty()) {
            let chars: Vec<char> = text.chars().collect();
            if chars.len() > 80 {
                return Some(format!("{}...", chars[..80].iter().collect::<String>()));
            }
            return Some(text.to_string());
        }
        if let Some(summary) = inner.get("summary").and_then(|v| v.as_str()) {
            let chars: Vec<char> = summary.chars().collect();
            if chars.len() > 80 {
                return Some(format!("{}...", chars[..80].iter().collect::<String>()));
            }
            return Some(summary.to_string());
        }
        if let Some(analysis) = inner.get("analysis").and_then(|v| v.as_str()) {
            let chars: Vec<char> = analysis.chars().collect();
            if chars.len() > 80 {
                return Some(format!("{}...", chars[..80].iter().collect::<String>()));
            }
            return Some(analysis.to_string());
        }
        // 有 message 字段直接用
        if let Some(msg) = inner
            .get("message")
            .or_else(|| obj.get("message"))
            .and_then(|v| v.as_str())
        {
            let chars: Vec<char> = msg.chars().collect();
            if chars.len() > 80 {
                return Some(format!("{}...", chars[..80].iter().collect::<String>()));
            }
            return Some(msg.to_string());
        }
        // 数量统计
        if let Some(count) = obj.get("total").and_then(|v| v.as_i64()) {
            return Some(format!("Got {count} results"));
        }
        if let Some(arr) = obj.get("feeds").and_then(|v| v.as_array()) {
            return Some(format!("Found {} feeds", arr.len()));
        }
        if let Some(arr) = obj.get("items").and_then(|v| v.as_array()) {
            return Some(format!("Got {} items", arr.len()));
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
                return Some(format!(
                    "Search \"{query}\" returned {} results",
                    results.len()
                ));
            }
            return Some(format!("Search returned {} results", results.len()));
        }
        // 通用：显示有意义的字段名
        let meaningful_keys: Vec<&str> = obj
            .keys()
            .map(|k| k.as_str())
            .filter(|k| !["status", "provider", "model", "cached"].contains(k))
            .take(3)
            .collect();
        if !meaningful_keys.is_empty() {
            return Some(format!("Loaded data ({})", meaningful_keys.join(", ")));
        }
        return Some("Done.".to_string());
    }
    if let Some(arr) = output.as_array() {
        return Some(format!("Got {} records", arr.len()));
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
            "Succeeded".to_string()
        } else {
            "Failed".to_string()
        });
    }
    None
}

// ─────────────────────────────────────────────
// 3. 内部实现
// ─────────────────────────────────────────────

// ─────────────────────────────────────────────
// 4. 对话与交互模板
// ─────────────────────────────────────────────

/// 正在理解请求
pub fn understanding_request() -> String {
    "Understanding your request…".to_string()
}

/// 进度完成
pub fn done_status() -> String {
    "Done.".to_string()
}

// ─────────────────────────────────────────────
// 5. 确认对话框模板
// ─────────────────────────────────────────────

/// 确认选项 — 是
pub fn yes_label() -> String {
    "Yes".to_string()
}

/// 确认选项 — 否
pub fn no_label() -> String {
    "No".to_string()
}

/// 确认消息默认格式
pub fn will_execute(name: &str) -> String {
    format!("This will run {name}")
}

// ─────────────────────────────────────────────
// 6. 任务生命周期
// ─────────────────────────────────────────────

/// 任务已被用户取消（错误字段）
pub fn task_cancelled_by_user() -> String {
    "The task was cancelled".to_string()
}

/// 任务因服务重启中断
pub fn task_interrupted() -> String {
    "The task was interrupted".to_string()
}

/// 步骤执行超时
pub fn step_timeout(_capability: &str, _secs: u64) -> String {
    "The step timed out".to_string()
}

// ─────────────────────────────────────────────
// 7. 操作结果消息
// ─────────────────────────────────────────────

/// 订阅成功
pub fn subscribe_success(name: &str, count: usize) -> String {
    format!("Subscribed to {name} and loaded {count} articles")
}

/// 内容已保存
pub fn content_saved(title: &str) -> String {
    format!("Saved: {title}")
}

/// 刷新任务已提交
pub fn refresh_submitted(platform: &str) -> String {
    format!("Refresh queued: {platform}")
}

/// 刷新提交失败
pub fn refresh_submit_failed(_err: &str) -> String {
    "Failed to submit refresh".to_string()
}

/// 刷新提交汇总
pub fn refresh_submitted_summary(submitted: usize, total: usize) -> String {
    format!("Queued refresh for {submitted}/{total} platforms")
}

/// 提醒已创建
pub fn reminder_created(title: &str) -> String {
    format!("Reminder created: {title}")
}

/// 提醒时间
pub fn reminder_time(datetime: &str) -> String {
    format!("Will remind you at {datetime}")
}

/// 笔记已保存
pub fn note_saved(title: &str) -> String {
    format!("Note saved: {title}")
}

/// 书签已保存
pub fn bookmark_saved(title: &str) -> String {
    format!("Bookmark saved: {title}")
}

/// 定时任务已创建
pub fn scheduled_task_created(name: &str) -> String {
    format!("Scheduled task created: {name}")
}

/// 网络搜索回退结果
pub fn web_search_fallback(_count: usize) -> String {
    "No matching articles were found; used a web search instead".to_string()
}

/// 未找到符合条件的文章
pub fn no_articles_found(_criteria: &str) -> String {
    "No matching articles were found".to_string()
}

/// 未在已订阅源中找到
pub fn not_found_in_feeds(_query: &str) -> String {
    "That was not found in subscribed feeds".to_string()
}

/// 未能找到匹配的 RSS 源
pub fn no_rss_found() -> String {
    "No matching RSS feed was found".to_string()
}

/// 未找到歌单
pub fn playlist_not_found() -> String {
    "No matching playlist was found".to_string()
}

/// 找到歌单
pub fn playlist_found(count: usize, keyword: &str) -> String {
    format!("Found {count} playlists for \"{keyword}\"")
}

/// 未找到订阅源或作者
pub fn feed_ambiguous(_name: &str) -> String {
    "That feed or author was not found".to_string()
}

/// 订阅源建议
pub fn feed_hint(suggestions: &str) -> String {
    format!("Did you mean: {suggestions}?")
}

/// 搜索结果
pub fn search_results_found(count: usize, query: &str) -> String {
    format!("Found {count} results for '{query}'")
}

/// 活跃平台
pub fn active_platforms(count: usize) -> String {
    format!("Active on {count} platforms")
}

/// API Key 未配置
pub fn api_key_not_configured(_service: &str) -> String {
    "This service is not configured".to_string()
}

/// 不支持的平台名称
pub fn unsupported_platform(_platform: &str) -> String {
    "That platform is not supported".to_string()
}

/// 输入过长
pub fn input_too_long(_max: usize) -> String {
    "Input is too long".to_string()
}

/// 输入不能为空
pub fn input_empty() -> String {
    "Input is empty".to_string()
}

/// 所有订阅 URL 都失败
pub fn subscribe_all_failed(_tried: usize, _last_error: &str) -> String {
    "Could not subscribe to any of the feeds".to_string()
}

/// 搜索空提示
pub fn search_empty_hint() -> String {
    "Enter a search term. Search covers Steam games, Bilibili following, Bangumi collections, MyAnimeList lists, GitHub repositories, and NetEase listening history.".to_string()
}

/// 搜索无结果
pub fn search_no_results(query: &str) -> String {
    format!(
        "Nothing matching '{query}' was found in your synced data.\n\nSearch only covers:\n- Steam library\n- Bilibili following\n- Bangumi collections\n- MyAnimeList anime/manga\n- GitHub repositories\n- NetEase Music\n\nWeb news and other external search are not supported here."
    )
}

/// 数据返回摘要
pub fn data_returned(count: usize) -> String {
    format!("Returned {count} items")
}

/// 处理记录摘要
pub fn records_processed(count: u64) -> String {
    format!("Processed {count} records")
}

/// 字段返回摘要
pub fn fields_returned(count: usize) -> String {
    format!("Returned {count} fields")
}

/// 操作成功/失败（布尔结果）
pub fn bool_result(success: bool) -> String {
    if success {
        "Succeeded".to_string()
    } else {
        "Failed".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn summarize_step_output_uses_envelope_analysis() {
        let text = summarize_step_output(&json!({
            "format": "json",
            "value": { "analysis": "分析正文", "type": "custom" },
            "contextProvenance": []
        }))
        .expect("progress copy");
        assert_eq!(text, "分析正文");
        assert!(!text.contains("format"));
    }
}
