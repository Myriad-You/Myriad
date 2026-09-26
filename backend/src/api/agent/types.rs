//! Agent API — types
use super::*;

// 请求/响应类型

/// 处理请求
#[derive(Debug, Deserialize)]
pub struct ProcessRequest {
    /// 用户的自然语言输入
    pub input: String,
    /// 可选的上下文信息
    #[serde(default)]
    pub context: Option<ProcessContext>,
}

/// 处理上下文
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProcessContext {
    /// 缺省 Work。`chat` 走严格 Lite 人设对话，不进 Planner / Executor。
    pub mode: Option<crate::services::agent::AgentInteractionMode>,
    /// 当前页面路由
    pub current_route: Option<String>,
    /// 活跃的平台
    pub active_platforms: Option<Vec<String>>,
    /// 会话 ID（用于多轮对话）
    pub session_id: Option<String>,
    /// 对话历史。有 session 时服务端以 DB 为准，丢弃客户端这份。
    pub conversation_history: Option<Vec<ConversationMessageApi>>,
    /// 自定义数据
    pub custom_data: Option<Value>,
    /// 已 Accepted、正在进入 Work 的提案 id（用户接单或意识引擎接单都写）。
    pub intention_id: Option<String>,
    pub autonomy_permission_cap: Option<Vec<String>>,
    /// Semantic live-face snapshot. Extra keys are dropped at the sanitizer.
    #[serde(default)]
    pub rig_state: Option<serde_json::Value>,
    /// A channel group turn. Set only by the server: never read from a
    /// request body, because it decides whose memories may be said aloud.
    #[serde(skip)]
    pub group: Option<GroupTurn>,
    /// A private IM chat turn. Set only by the server, like `group`.
    #[serde(skip)]
    pub channel_chat: Option<crate::services::agent::types::ChannelChat>,
}

/// Where a channel group turn happens and what the group said lately.
#[derive(Debug, Clone)]
pub struct GroupTurn {
    /// `telegram:<chat_id>`.
    pub venue: String,
    /// Recent group lines, oldest first; others' lines are `name：text`.
    pub transcript: Vec<crate::services::agent::types::ConversationMessage>,
    /// Nobody addressed her: she is chiming in, and why.
    pub chime: Option<String>,
    /// Who spoke to her, as the group knows them.
    pub speaker: String,
}

/// 对话消息（API 格式）
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationMessageApi {
    /// 角色
    pub role: String,
    /// 消息内容
    pub content: String,
    pub created_at: Option<String>,
}

/// API 响应
#[derive(Debug, Clone, Serialize)]
pub struct ApiResponse {
    /// 是否成功
    pub success: bool,
    /// 响应类型
    #[serde(rename = "responseType")]
    pub response_type: String,
    pub message: String,
    pub data: Option<Value>,
    /// 数据展示类型提示
    #[serde(rename = "dataDisplay", skip_serializing_if = "Option::is_none")]
    pub data_display: Option<DataDisplayHintApi>,
    /// 后续建议
    pub suggestions: Vec<String>,
    /// 任务信息
    pub task: Option<TaskInfo>,
    /// 前端操作指令（路由导航、音乐控制等）
    #[serde(rename = "frontendAction", skip_serializing_if = "Option::is_none")]
    pub frontend_action: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub performance: Option<crate::services::agent::merope::PerformanceDirective>,
    #[serde(rename = "sessionId", skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

/// 数据展示类型提示（API 版本）
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DataDisplayHintApi {
    /// 表格展示
    Table {
        columns: Vec<ColumnDefApi>,
        #[serde(rename = "dataPath", skip_serializing_if = "Option::is_none")]
        data_path: Option<String>,
    },
    /// 图表展示
    Chart {
        #[serde(rename = "chartType")]
        chart_type: String,
        #[serde(rename = "xField")]
        x_field: String,
        #[serde(rename = "yField")]
        y_field: String,
    },
    /// 卡片列表
    CardList {
        #[serde(rename = "titleField")]
        title_field: String,
        #[serde(rename = "descriptionField", skip_serializing_if = "Option::is_none")]
        description_field: Option<String>,
        #[serde(rename = "imageField", skip_serializing_if = "Option::is_none")]
        image_field: Option<String>,
    },
    Markdown,
    /// 键值对
    KeyValue,
    /// 时间线
    Timeline {
        #[serde(rename = "timeField")]
        time_field: String,
        #[serde(rename = "contentField")]
        content_field: String,
    },
    /// 原始 JSON
    Raw,
}

/// 表格列定义（API 版本）
#[derive(Debug, Clone, Serialize)]
pub struct ColumnDefApi {
    pub field: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    pub sortable: bool,
}

/// 任务信息
#[derive(Debug, Clone, Serialize)]
pub struct TaskInfo {
    /// 任务 ID
    #[serde(rename = "taskId")]
    pub task_id: String,
    pub status: String,
    /// 进度百分比（来自 `TaskState.progress`）
    pub progress: u8,
    /// 错误信息
    pub error: Option<String>,
    /// 当前执行步骤信息
    #[serde(rename = "currentStep", skip_serializing_if = "Option::is_none")]
    pub current_step: Option<StepInfo>,
    /// 已完成步骤数
    #[serde(rename = "completedSteps")]
    pub completed_steps: u32,
    /// 总步骤数（包含动态生成的）
    #[serde(rename = "totalSteps")]
    pub total_steps: u32,
    /// 动态生成的步骤数
    #[serde(rename = "dynamicStepsAdded")]
    pub dynamic_steps_added: u32,
    /// 待回答的问题（如果有）
    #[serde(rename = "pendingQuestion", skip_serializing_if = "Option::is_none")]
    pub pending_question: Option<QuestionSummary>,
    /// 步骤执行历史
    #[serde(rename = "stepHistory", skip_serializing_if = "Vec::is_empty")]
    pub step_history: Vec<StepExecution>,
    /// 执行追踪（包含 tier 使用、总耗时等）
    #[serde(rename = "executionTrace", skip_serializing_if = "Option::is_none")]
    pub execution_trace: Option<Value>,
}

/// 当前步骤信息
#[derive(Debug, Clone, Serialize)]
pub struct StepInfo {
    /// 步骤 ID
    #[serde(rename = "stepId")]
    pub step_id: String,
    /// 能力名称（用户友好的描述）
    #[serde(rename = "capabilityName")]
    pub capability_name: String,
    /// 步骤描述
    pub description: String,
    #[serde(rename = "startedAt")]
    pub started_at: String,
}

/// 步骤执行记录
#[derive(Debug, Clone, Serialize)]
pub struct StepExecution {
    /// 步骤 ID
    #[serde(rename = "stepId")]
    pub step_id: String,
    /// 能力名称
    #[serde(rename = "capabilityName")]
    pub capability_name: String,
    /// 执行状态
    pub status: String,
    /// 执行时长（毫秒）
    #[serde(rename = "durationMs", skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// 输出摘要（简短描述，非完整数据）
    #[serde(rename = "outputSummary", skip_serializing_if = "Option::is_none")]
    pub output_summary: Option<String>,
    /// 错误信息（步骤失败时）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// 图片 URL（输出里的 url / imageUrl）
    #[serde(rename = "imageUrl", skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
    /// 是否为动态生成的步骤
    #[serde(rename = "isDynamic")]
    pub is_dynamic: bool,
}

/// 问题摘要
#[derive(Debug, Clone, Serialize)]
pub struct QuestionSummary {
    /// 问题 ID
    #[serde(rename = "questionId")]
    pub question_id: String,
    /// 问题类型
    #[serde(rename = "questionType")]
    pub question_type: String,
    /// 问题文本
    pub question: String,
    /// 上下文
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    /// 选项（如果有）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<QuestionOptionApi>>,
    /// 是否必须回答
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
    /// 默认值
    #[serde(rename = "defaultValue", skip_serializing_if = "Option::is_none")]
    pub default_value: Option<String>,
}

/// 问题选项（API 层）
#[derive(Debug, Clone, Serialize)]
pub struct QuestionOptionApi {
    pub value: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

// SSE 进度事件类型

/// SSE 进度事件（使用 service 层统一类型）
pub type ProgressEvent = AgentProgressEvent;

// 任务预设 API 类型

/// 对话消息（用于 conversation_data）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMessage {
    /// 消息角色
    pub role: String,
    /// 消息内容
    pub content: String,
    /// 消息元数据
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
    #[serde(rename = "createdAt")]
    pub created_at: String,
}

/// 任务预设响应
#[derive(Debug, Clone, Serialize)]
pub struct TaskPresetResponse {
    /// 预设 ID
    pub id: i32,
    /// 用户输入的原始文本
    pub input: String,
    /// 预设类型: 'favorite' 或 'history'
    #[serde(rename = "presetType")]
    pub preset_type: String,
    /// 解析后的步骤
    #[serde(rename = "parsedSteps", skip_serializing_if = "Option::is_none")]
    pub parsed_steps: Option<Value>,
    /// 意图摘要
    #[serde(rename = "intentSummary", skip_serializing_if = "Option::is_none")]
    pub intent_summary: Option<String>,
    /// 最后使用时间
    #[serde(rename = "lastUsedAt")]
    pub last_used_at: String,
    /// 使用次数
    #[serde(rename = "useCount")]
    pub use_count: i32,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    /// 对话标题
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// 对话历史数据
    #[serde(rename = "conversationData", skip_serializing_if = "Option::is_none")]
    pub conversation_data: Option<Vec<ConversationMessage>>,
    /// 是否有非空 conversation_data
    #[serde(rename = "hasConversation")]
    pub has_conversation: bool,
}

/// 创建任务预设请求
#[derive(Debug, Deserialize)]
pub struct CreatePresetRequest {
    /// 用户输入的原始文本
    pub input: String,
    /// 预设类型: 'favorite' 或 'history'
    #[serde(rename = "presetType")]
    pub preset_type: String,
    /// 解析后的步骤（可选）
    #[serde(rename = "parsedSteps")]
    pub parsed_steps: Option<Value>,
    /// 意图摘要（可选）
    #[serde(rename = "intentSummary")]
    pub intent_summary: Option<String>,
    /// 对话标题（可选）
    pub title: Option<String>,
    /// 对话历史数据（可选）
    #[serde(rename = "conversationData")]
    pub conversation_data: Option<Vec<ConversationMessage>>,
}

/// 任务预设列表响应
#[derive(Debug, Serialize)]
pub struct TaskPresetListResponse {
    /// 收藏列表
    pub favorites: Vec<TaskPresetResponse>,
    /// 历史记录列表
    pub history: Vec<TaskPresetResponse>,
}

impl From<agent_task_presets::Model> for TaskPresetResponse {
    fn from(model: agent_task_presets::Model) -> Self {
        // 解析 conversation_data
        let conversation_data: Option<Vec<ConversationMessage>> = model
            .conversation_data
            .as_ref()
            .and_then(|j| serde_json::from_value(j.clone()).ok());

        let has_conversation = conversation_data
            .as_ref()
            .map(|c| !c.is_empty())
            .unwrap_or(false);

        Self {
            id: model.id,
            input: model.input,
            preset_type: model.preset_type,
            parsed_steps: model.parsed_steps,
            intent_summary: model.intent_summary,
            last_used_at: model.last_used_at.to_rfc3339(),
            use_count: model.use_count,
            created_at: model.created_at.to_rfc3339(),
            title: model.title,
            conversation_data,
            has_conversation,
        }
    }
}

impl From<&TaskState> for TaskInfo {
    fn from(state: &TaskState) -> Self {
        // 计算已完成步骤数
        let completed_steps = state.step_results.values().filter(|r| r.success).count() as u32;

        // 获取动态步骤数
        let dynamic_steps_added = state
            .execution_context
            .as_ref()
            .map(|ctx| ctx.dynamic_steps_generated as u32)
            .unwrap_or(0);

        // 计算总步骤数
        let total_steps = state.step_results.len() as u32
            + state
                .execution_context
                .as_ref()
                .map(|ctx| ctx.pending_dynamic_steps.len() as u32)
                .unwrap_or(0);

        // 构建步骤执行历史
        let step_history: Vec<StepExecution> = state
            .step_results
            .iter()
            .map(|(step_id, result)| {
                let output_summary = result.output.as_ref().and_then(|o| {
                    // 生成简短摘要
                    summarize_output(o)
                });

                StepExecution {
                    step_id: step_id.clone(),
                    capability_name: extract_capability_name(step_id),
                    status: if result.success {
                        "completed".to_string()
                    } else {
                        "failed".to_string()
                    },
                    duration_ms: Some(result.duration_ms),
                    output_summary,
                    error: if result.success {
                        None
                    } else {
                        result.error.clone()
                    },
                    image_url: result.output.as_ref().and_then(|o| {
                        crate::services::agent::executor::utils::extract_image_url(o)
                    }),
                    is_dynamic: step_id.starts_with("dynamic_"),
                }
            })
            .collect();

        // 待回答问题
        let pending_question = state.pending_question.as_ref().map(|q| QuestionSummary {
            question_id: q.question_id.clone(),
            question_type: serde_json::to_value(&q.question_type)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_else(|| "free_text".to_string()),
            question: q.question.clone(),
            context: if q.context.is_empty() {
                None
            } else {
                Some(q.context.clone())
            },
            options: q.options.as_ref().map(|opts| {
                opts.iter()
                    .map(|o| QuestionOptionApi {
                        value: o.value.clone(),
                        label: o.label.clone(),
                        description: o.description.clone(),
                    })
                    .collect()
            }),
            required: Some(q.required),
            default_value: q.default_value.clone(),
        });

        // 序列化执行追踪
        let execution_trace = state
            .execution_trace
            .as_ref()
            .and_then(|et| serde_json::to_value(et).ok());

        Self {
            task_id: state.task_id.clone(),
            status: task_status_name(&state.status).to_string(),
            progress: state.progress,
            error: state.error.clone(),
            current_step: None,
            completed_steps,
            total_steps: total_steps.max(completed_steps),
            dynamic_steps_added,
            pending_question,
            step_history,
            execution_trace,
        }
    }
}

pub(crate) fn task_status_name(status: &crate::services::agent::TaskStatus) -> &'static str {
    use crate::services::agent::TaskStatus;
    match status {
        TaskStatus::Pending => "pending",
        TaskStatus::Running => "running",
        TaskStatus::WaitingForInput => "waiting_for_input",
        TaskStatus::Paused => "paused",
        TaskStatus::Completed => "completed",
        TaskStatus::Failed => "failed",
        TaskStatus::Cancelled => "cancelled",
    }
}

fn is_task_envelope(output: &Value) -> bool {
    matches!(
        output,
        Value::Object(map)
            if map.get("format").and_then(Value::as_str).is_some() && map.contains_key("value")
    )
}

/// 从输出生成简短摘要
pub(crate) fn summarize_output(output: &Value) -> Option<String> {
    if is_task_envelope(output) {
        return crate::services::agent::response_agent::summarize_step_output(output);
    }
    match output {
        Value::String(s) => {
            let char_count = s.chars().count();
            if char_count > 100 {
                Some(format!("{}...", s.chars().take(100).collect::<String>()))
            } else {
                Some(s.clone())
            }
        }
        Value::Array(arr) => Some(crate::services::agent::response_agent::data_returned(
            arr.len(),
        )),
        Value::Object(obj) => {
            if let Some(msg) = obj.get("message").and_then(|v| v.as_str()) {
                Some(msg.to_string())
            } else if let Some(count) = obj.get("count").and_then(|v| v.as_u64()) {
                Some(crate::services::agent::response_agent::records_processed(
                    count,
                ))
            } else if let Some(items) = obj.get("items").and_then(|v| v.as_array()) {
                Some(crate::services::agent::response_agent::data_returned(
                    items.len(),
                ))
            } else {
                Some(crate::services::agent::response_agent::fields_returned(
                    obj.len(),
                ))
            }
        }
        Value::Bool(b) => Some(crate::services::agent::response_agent::bool_result(*b)),
        Value::Number(n) => Some(n.to_string()),
        Value::Null => None,
    }
}

/// 从步骤 ID 提取能力名称
pub(crate) fn extract_capability_name(step_id: &str) -> String {
    // 取 `step_id` 最后一个 `_` 分段，再映射友好名
    if let Some(cap_part) = step_id.split('_').next_back() {
        // 最后一段对表；对不上则把 `.`/`_` 换成空格
        match cap_part {
            "bilibili" | "platform.bilibili" => "Loading Bilibili data".to_string(),
            "steam" | "platform.steam" => "Loading Steam data".to_string(),
            "github" | "platform.github" => "Loading GitHub data".to_string(),
            "netease" | "platform.netease" => "Loading NetEase data".to_string(),
            "bangumi" | "platform.bangumi" => "Loading Bangumi data".to_string(),
            "x" | "platform.x" => "Loading X data".to_string(),
            "discord" | "platform.discord" => "Loading Discord data".to_string(),
            "mal" | "platform.mal" | "myanimelist" | "platform.myanimelist" => {
                "Loading MyAnimeList data".to_string()
            }
            "summarize" | "ai.summarize" => "Summarizing".to_string(),
            "analyze" | "ai.analyze" => "Analyzing".to_string(),
            "webSearch" | "ai.webSearch" => "Searching the web".to_string(),
            "discover" | "phantasi.discover" => "Discovering feeds".to_string(),
            "subscribe" | "phantasi.subscribe" => "Subscribing to a feed".to_string(),
            _ => cap_part.replace(['.', '_'], " "),
        }
    } else {
        step_id.to_string()
    }
}

impl From<AgentResponse> for ApiResponse {
    fn from(response: AgentResponse) -> Self {
        let data_display = response.data_display.map(convert_data_display_hint);

        let frontend_action = response.frontend_action;
        let performance = response.performance;

        Self {
            success: !matches!(response.response_type, AgentResponseType::Error),
            response_type: agent_response_type_name(&response.response_type).to_string(),
            message: response.message,
            data: response.data,
            data_display,
            suggestions: response.suggestions,
            task: response.task.as_ref().map(TaskInfo::from),
            frontend_action,
            performance,
            session_id: None,
        }
    }
}

/// Keep the wire contract aligned with `AgentResponseType`'s serde representation.
/// Debug formatting is not a stable API contract and collapses multi-word variants.
pub(crate) fn agent_response_type_name(response_type: &AgentResponseType) -> &'static str {
    match response_type {
        AgentResponseType::Answer => "answer",
        AgentResponseType::Clarification => "clarification",
        AgentResponseType::TaskCreated => "task_created",
        AgentResponseType::TaskProgress => "task_progress",
        AgentResponseType::TaskCompleted => "task_completed",
        AgentResponseType::Error => "error",
    }
}

#[cfg(test)]
mod api_contract_tests {
    use super::*;

    #[test]
    fn task_status_uses_public_snake_case_contract() {
        assert_eq!(
            task_status_name(&crate::services::agent::TaskStatus::WaitingForInput),
            "waiting_for_input"
        );
    }

    #[test]
    fn task_info_output_summary_unwraps_ai_envelope() {
        assert_eq!(
            summarize_output(&json!({
                "format": "json",
                "value": { "analysis": "分析正文", "type": "custom" },
                "contextProvenance": []
            }))
            .as_deref(),
            Some("分析正文")
        );
        assert_eq!(
            summarize_output(&json!({
                "format": "text",
                "value": "回复正文",
                "contextProvenance": []
            }))
            .as_deref(),
            Some("回复正文")
        );
        let image = summarize_output(&json!({
            "format": "image",
            "value": {
                "url": "https://example.invalid/a.png",
                "width": 1024,
                "height": 768
            },
            "contextProvenance": []
        }))
        .expect("image summary");
        assert!(image.contains("Image"), "{image}");
        assert_eq!(
            summarize_output(&json!({ "count": 5 })).as_deref(),
            Some("Processed 5 records")
        );
    }

    #[test]
    fn agent_response_types_use_public_snake_case_contract() {
        let cases = [
            (AgentResponseType::Answer, "answer"),
            (AgentResponseType::Clarification, "clarification"),
            (AgentResponseType::TaskCreated, "task_created"),
            (AgentResponseType::TaskProgress, "task_progress"),
            (AgentResponseType::TaskCompleted, "task_completed"),
            (AgentResponseType::Error, "error"),
        ];
        for (response_type, expected) in cases {
            assert_eq!(agent_response_type_name(&response_type), expected);
        }
    }

    #[test]
    fn confirmation_continuation_can_terminate_its_run_while_task_waits() {
        let event = AgentProgressEvent::TaskCompleted {
            task_id: "task_waiting".to_string(),
            success: true,
            response: Box::new(json!({
                "streamTerminal": true,
                "task": { "status": "waiting_for_input" }
            })),
        };
        assert!(agent_run_event_is_terminal(&event));
    }

    #[test]
    fn wait_loop_oneshot_drop_publishes_terminal_event() {
        let event = wait_loop_channel_dropped_event("task_orphaned");
        assert!(
            agent_run_event_is_terminal(&event),
            "oneshot drop must terminalize the run for re-subscribers"
        );
        match &event {
            AgentProgressEvent::TaskCompleted {
                success, response, ..
            } => {
                assert!(!*success);
                assert_eq!(
                    response.pointer("/task/status").and_then(|v| v.as_str()),
                    Some("failed")
                );
                assert_eq!(
                    response.get("streamTerminal").and_then(|v| v.as_bool()),
                    Some(true)
                );
            }
            other => panic!("expected TaskCompleted, got {:?}", other),
        }
    }

    #[test]
    fn answer_lane_key_matches_process_session_lane() {
        // Regression: answer stream must not use user-only key when session exists.
        let process_key = LaneQueue::make_lane_key(42, Some("sess_live"));
        let answer_key =
            LaneQueue::resolve_answer_lane_key(42, Some(&process_key), Some("ignored"));
        assert_eq!(answer_key, process_key);
        let answer_from_session = LaneQueue::resolve_answer_lane_key(42, None, Some("sess_live"));
        assert_eq!(answer_from_session, process_key);
    }

    #[test]
    fn multi_round_wait_metadata_keeps_top_level_run_and_task_ids() {
        // Simulates answer path value (ApiResponse-shaped) without identity fields.
        let answer_payload = json!({
            "success": true,
            "message": "还需要补充一点",
            "task": {
                "taskId": "task_multi",
                "status": "waiting_for_input",
                "pendingQuestion": { "questionId": "q2", "question": "再确认？" }
            }
        });
        let meta =
            session_metadata_with_run_identity(Some(answer_payload), "run_abc", "task_multi");
        assert_eq!(meta.get("runId").and_then(|v| v.as_str()), Some("run_abc"));
        assert_eq!(
            meta.get("taskId").and_then(|v| v.as_str()),
            Some("task_multi")
        );
        assert_eq!(
            meta.pointer("/task/status").and_then(|v| v.as_str()),
            Some("waiting_for_input")
        );
        // Original fields preserved
        assert_eq!(
            meta.get("message").and_then(|v| v.as_str()),
            Some("还需要补充一点")
        );
    }

    #[test]
    fn frontend_action_payload_is_preserved_without_field_loss() {
        let action = json!({
            "type": "music_load_playlist",
            "playlistId": "12345",
            "source": "netease",
            "autoPlay": true,
            "commands": [{ "action": "click", "target": "play" }],
            "value": 0.75,
        });
        let response = AgentResponse {
            response_type: AgentResponseType::Answer,
            message: "ok".to_string(),
            data: None,
            data_display: None,
            suggestions: vec![],
            task: None,
            frontend_action: Some(action.clone()),
            performance: None,
        };

        let api_response = ApiResponse::from(response);
        assert_eq!(api_response.frontend_action, Some(action));
    }
}
