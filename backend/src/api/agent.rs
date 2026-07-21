//! Agent API 端点
//!
//! 提供 AI Agent 自然语言任务编排的 HTTP 接口

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
    Extension, Json,
};
use chrono::Utc;
use futures::stream::Stream;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait,
    PaginatorTrait, QueryFilter, QueryOrder,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
use tokio_stream::StreamExt;

use crate::middleware::auth::Claims;
use crate::models::entities::{agent_messages, agent_sessions, agent_task_presets};
use crate::services::agent::queue::LaneQueue;
use crate::services::agent::run_hub::{create_run, get_run_for_user, AgentRun};
use crate::services::agent::{
    Agent, AgentProgressEvent, AgentResponse, AgentResponseType, RequestContext, TaskState,
    UserAnswer, UserRequest, LANE_QUEUE,
};

/// 等待用户回答的任务上下文
/// process_stream 注册后等待 oneshot 信号；answer_task_question_stream 完成后通过此信号回传结果
struct WaitingTaskCtx {
    /// 任务所有者；take 时必须匹配，防止跨用户抢 oneshot
    user_id: i32,
    /// 后端 run 的进度 sender；answer 阶段继续写入同一个 run hub
    progress_tx: tokio::sync::mpsc::Sender<AgentProgressEvent>,
    /// 单次信号：answer 处理完成后将最终 response Value 发送至此
    done_tx: tokio::sync::oneshot::Sender<serde_json::Value>,
    /// 会话 ID（用于持久化用户的问答消息到 agent_messages）
    session_id: String,
}

static WAITING_TASKS: once_cell::sync::Lazy<
    tokio::sync::RwLock<std::collections::HashMap<String, WaitingTaskCtx>>,
> = once_cell::sync::Lazy::new(|| tokio::sync::RwLock::new(std::collections::HashMap::new()));

/// 仅任务所有者可取出 waiting 上下文；错误用户不 remove，避免抢 oneshot
async fn take_waiting_task(task_id: &str, user_id: i32) -> Option<WaitingTaskCtx> {
    let mut map = WAITING_TASKS.write().await;
    match map.get(task_id) {
        Some(ctx) if ctx.user_id != user_id => {
            tracing::warn!(
                task_id = %task_id,
                caller = user_id,
                owner = ctx.user_id,
                "[Agent API] WAITING_TASKS ownership mismatch"
            );
            None
        }
        Some(_) => map.remove(task_id),
        None => None,
    }
}

fn agent_run_event_is_terminal(event: &AgentProgressEvent) -> bool {
    match event {
        AgentProgressEvent::TaskCompleted { response, .. } => {
            response.get("streamTerminal").and_then(Value::as_bool) == Some(true)
                || response.pointer("/task/status").and_then(Value::as_str)
                    != Some("waiting_for_input")
        }
        AgentProgressEvent::Error { .. } => true,
        _ => false,
    }
}

/// Terminal payload when the wait-loop oneshot is dropped without a normal answer.
/// Re-subscribers must not hang forever on a non-completed run.
fn wait_loop_channel_dropped_response(task_id: &str) -> Value {
    json!({
        "success": false,
        "responseType": "error",
        "message": "任务等待通道已断开",
        "streamTerminal": true,
        "task": {
            "taskId": task_id,
            "status": "failed",
            "progress": 0
        }
    })
}

/// Build the TaskCompleted event published when a wait-loop oneshot is dropped.
fn wait_loop_channel_dropped_event(task_id: &str) -> AgentProgressEvent {
    AgentProgressEvent::TaskCompleted {
        task_id: task_id.to_string(),
        success: false,
        response: Box::new(wait_loop_channel_dropped_response(task_id)),
    }
}

/// Ensure session-message metadata always carries top-level run/task ids for reattach.
/// Merges into an existing JSON object (e.g. ApiResponse value) without dropping fields.
fn session_metadata_with_run_identity(
    base: Option<Value>,
    run_id: &str,
    task_id: &str,
) -> Value {
    let mut meta = match base {
        Some(Value::Object(map)) => Value::Object(map),
        Some(other) => json!({ "data": other }),
        None => json!({}),
    };
    if let Some(obj) = meta.as_object_mut() {
        obj.insert("runId".to_string(), json!(run_id));
        obj.insert("taskId".to_string(), json!(task_id));
        // snake_case aliases for notification / legacy readers
        obj.insert("run_id".to_string(), json!(run_id));
        obj.insert("task_id".to_string(), json!(task_id));
        if !obj.contains_key("task") {
            obj.insert(
                "task".to_string(),
                json!({ "taskId": task_id, "status": "running" }),
            );
        }
    }
    meta
}

fn agent_run_event_stream(run: Arc<AgentRun>) -> impl Stream<Item = Result<Event, Infallible>> {
    async_stream::stream! {
        // 先订阅再读取快照；sequence 去重消除两者之间的竞态。
        // mut: Lagged 时会重新 subscribe 同一 run。
        let mut receiver = run.subscribe();
        let (history, mut last_sequence, already_completed) = run.snapshot().await;

        if !history.iter().any(|envelope| matches!(
            &envelope.event,
            AgentProgressEvent::RunStarted { .. }
        )) {
            let event = AgentProgressEvent::RunStarted {
                run_id: run.run_id().to_string(),
                session_id: run.session_id().map(str::to_string),
            };
            let data = serde_json::to_string(&event).unwrap_or_else(|_| "{}".to_string());
            yield Ok(Event::default().id("0").data(data));
        }

        for envelope in history {
            let data = serde_json::to_string(&envelope.event).unwrap_or_else(|_| "{}".to_string());
            yield Ok(Event::default().id(envelope.sequence.to_string()).data(data));
        }
        if already_completed {
            return;
        }

        let mut registry_poll = tokio::time::interval(Duration::from_secs(2));
        registry_poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                received = receiver.recv() => match received {
                    Ok(envelope) if envelope.sequence > last_sequence => {
                        last_sequence = envelope.sequence;
                        let terminal = agent_run_event_is_terminal(&envelope.event);
                        let data = serde_json::to_string(&envelope.event)
                            .unwrap_or_else(|_| "{}".to_string());
                        yield Ok(Event::default().id(envelope.sequence.to_string()).data(data));
                        if terminal {
                            return;
                        }
                    }
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        // 不关流：从内存快照补发遗漏事件，避免前端必须重开连接。
                        let (history, snap_seq, completed) = run.snapshot().await;
                        for envelope in history {
                            if envelope.sequence <= last_sequence {
                                continue;
                            }
                            last_sequence = envelope.sequence;
                            let terminal = agent_run_event_is_terminal(&envelope.event);
                            let data = serde_json::to_string(&envelope.event)
                                .unwrap_or_else(|_| "{}".to_string());
                            yield Ok(Event::default().id(envelope.sequence.to_string()).data(data));
                            if terminal {
                                return;
                            }
                        }
                        last_sequence = last_sequence.max(snap_seq);
                        // 重新订阅，丢弃 lag 的 receiver
                        receiver = run.subscribe();
                        if completed {
                            return;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                },
                _ = registry_poll.tick() => {
                    for envelope in run.refresh_from_registry().await {
                        if envelope.sequence <= last_sequence {
                            continue;
                        }
                        last_sequence = envelope.sequence;
                        let terminal = agent_run_event_is_terminal(&envelope.event);
                        let data = serde_json::to_string(&envelope.event)
                            .unwrap_or_else(|_| "{}".to_string());
                        yield Ok(Event::default().id(envelope.sequence.to_string()).data(data));
                        if terminal {
                            return;
                        }
                    }
                }
            }
        }
    }
}

// ============ 请求/响应类型 ============

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
    /// 当前页面路由
    pub current_route: Option<String>,
    /// 活跃的平台
    pub active_platforms: Option<Vec<String>>,
    /// 会话 ID（用于多轮对话）
    pub session_id: Option<String>,
    /// 对话历史（用于继续对话模式）
    pub conversation_history: Option<Vec<ConversationMessageApi>>,
    /// 自定义数据
    pub custom_data: Option<Value>,
}

/// 对话消息（API 格式）
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationMessageApi {
    /// 角色: user, assistant, system
    pub role: String,
    /// 消息内容
    pub content: String,
    /// 创建时间
    pub created_at: Option<String>,
}

/// API 响应（增强版）
#[derive(Debug, Clone, Serialize)]
pub struct ApiResponse {
    /// 是否成功
    pub success: bool,
    /// 响应类型
    #[serde(rename = "responseType")]
    pub response_type: String,
    /// 消息
    pub message: String,
    /// 数据
    pub data: Option<Value>,
    /// 数据展示类型提示
    #[serde(rename = "dataDisplay", skip_serializing_if = "Option::is_none")]
    pub data_display: Option<DataDisplayHintApi>,
    /// 后续建议
    pub suggestions: Vec<String>,
    /// 任务信息
    pub task: Option<TaskInfo>,
    /// 敏感操作确认请求
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confirmation: Option<ConfirmationInfo>,
    /// 前端操作指令（路由导航、音乐控制等）
    #[serde(rename = "frontendAction", skip_serializing_if = "Option::is_none")]
    pub frontend_action: Option<Value>,
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
    /// Markdown
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

/// 敏感操作确认信息
#[derive(Debug, Clone, Serialize)]
pub struct ConfirmationInfo {
    /// 确认 ID
    #[serde(rename = "confirmationId")]
    pub confirmation_id: String,
    /// 风险等级
    #[serde(rename = "riskLevel")]
    pub risk_level: String,
    /// 过期时间（秒）
    #[serde(rename = "expiresInSeconds")]
    pub expires_in_seconds: i64,
    /// 待确认的步骤
    #[serde(rename = "pendingSteps")]
    pub pending_steps: Vec<PendingStepInfo>,
}

/// 待确认步骤信息
#[derive(Debug, Clone, Serialize)]
pub struct PendingStepInfo {
    /// 步骤 ID
    #[serde(rename = "stepId")]
    pub step_id: String,
    /// 能力名称
    #[serde(rename = "capabilityName")]
    pub capability_name: String,
    /// 确认消息
    pub message: String,
    /// 影响说明
    pub impact: Vec<String>,
}

/// 任务信息（增强版）
#[derive(Debug, Clone, Serialize)]
pub struct TaskInfo {
    /// 任务 ID
    #[serde(rename = "taskId")]
    pub task_id: String,
    /// 状态
    pub status: String,
    /// 进度 (0-100)
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
    /// 开始时间
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
    /// 图片 URL（ai.image 输出）
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

// ============ SSE 进度事件类型 ============

/// SSE 进度事件（使用 service 层统一类型）
pub type ProgressEvent = AgentProgressEvent;

// ============ 任务预设 API 类型 ============

/// 对话消息（用于 conversation_data）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMessage {
    /// 消息角色: user, assistant, system
    pub role: String,
    /// 消息内容
    pub content: String,
    /// 消息元数据
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
    /// 创建时间
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
    /// 创建时间
    #[serde(rename = "createdAt")]
    pub created_at: String,
    /// 对话标题
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// 对话历史数据
    #[serde(rename = "conversationData", skip_serializing_if = "Option::is_none")]
    pub conversation_data: Option<Vec<ConversationMessage>>,
    /// 是否有对话历史（前端用于判断是否显示"继续对话"按钮）
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
            current_step: None, // 由执行器在运行时设置
            completed_steps,
            total_steps: total_steps.max(completed_steps),
            dynamic_steps_added,
            pending_question,
            step_history,
            execution_trace,
        }
    }
}

fn task_status_name(status: &crate::services::agent::TaskStatus) -> &'static str {
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
    fn agent_response_types_use_public_snake_case_contract() {
        let cases = [
            (AgentResponseType::Answer, "answer"),
            (AgentResponseType::Clarification, "clarification"),
            (
                AgentResponseType::ConfirmationRequired,
                "confirmation_required",
            ),
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
        let meta = session_metadata_with_run_identity(
            Some(answer_payload),
            "run_abc",
            "task_multi",
        );
        assert_eq!(meta.get("runId").and_then(|v| v.as_str()), Some("run_abc"));
        assert_eq!(meta.get("taskId").and_then(|v| v.as_str()), Some("task_multi"));
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
            confirmation: None,
            frontend_action: Some(action.clone()),
        };

        let api_response = ApiResponse::from(response);
        assert_eq!(api_response.frontend_action, Some(action));
    }
}

/// 从输出生成简短摘要
fn summarize_output(output: &Value) -> Option<String> {
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
fn extract_capability_name(step_id: &str) -> String {
    // 步骤 ID 格式通常为 "step_1_platform.bilibili"
    if let Some(cap_part) = step_id.split('_').next_back() {
        // 将 capability id 转换为友好名称
        match cap_part {
            "bilibili" | "platform.bilibili" => "获取 B 站数据".to_string(),
            "steam" | "platform.steam" => "获取 Steam 数据".to_string(),
            "github" | "platform.github" => "获取 GitHub 数据".to_string(),
            "netease" | "platform.netease" => "获取网易云数据".to_string(),
            "bangumi" | "platform.bangumi" => "获取 Bangumi 数据".to_string(),
            "x" | "platform.x" => "获取 X 数据".to_string(),
            "discord" | "platform.discord" => "获取 Discord 数据".to_string(),
            "mal" | "platform.mal" | "myanimelist" | "platform.myanimelist" => {
                "获取 MyAnimeList 数据".to_string()
            }
            "summarize" | "ai.summarize" => "AI 总结".to_string(),
            "analyze" | "ai.analyze" => "AI 分析".to_string(),
            "webSearch" | "ai.webSearch" => "网络搜索".to_string(),
            "discover" | "brew.discover" => "发现 RSS 源".to_string(),
            "subscribe" | "brew.subscribe" => "订阅 RSS 源".to_string(),
            _ => cap_part.replace(['.', '_'], " "),
        }
    } else {
        step_id.to_string()
    }
}

impl From<AgentResponse> for ApiResponse {
    fn from(response: AgentResponse) -> Self {
        let data_display = response.data_display.map(convert_data_display_hint);

        // 转换确认请求
        let confirmation = response.confirmation.map(|req| {
            let expires_in = (req.expires_at - chrono::Utc::now()).num_seconds();
            ConfirmationInfo {
                confirmation_id: req.confirmation_id,
                risk_level: req
                    .pending_steps
                    .iter()
                    .map(|s| &s.risk_level)
                    .max_by_key(|r| match r {
                        crate::services::agent::RiskLevel::Critical => 4,
                        crate::services::agent::RiskLevel::High => 3,
                        crate::services::agent::RiskLevel::Medium => 2,
                        crate::services::agent::RiskLevel::Low => 1,
                        crate::services::agent::RiskLevel::None => 0,
                    })
                    .map(|r| format!("{:?}", r).to_lowercase())
                    .unwrap_or_else(|| "none".to_string()),
                expires_in_seconds: expires_in.max(0),
                pending_steps: req
                    .pending_steps
                    .into_iter()
                    .map(|s| PendingStepInfo {
                        step_id: s.step_id,
                        capability_name: s.capability_name,
                        message: s.confirmation_message,
                        impact: s.impact,
                    })
                    .collect(),
            }
        });

        // 转换前端操作指令
        let frontend_action = response.frontend_action;

        Self {
            success: !matches!(response.response_type, AgentResponseType::Error),
            response_type: agent_response_type_name(&response.response_type).to_string(),
            message: response.message,
            data: response.data,
            data_display,
            suggestions: response.suggestions,
            task: response.task.as_ref().map(TaskInfo::from),
            confirmation,
            frontend_action,
            session_id: None,
        }
    }
}

/// Keep the wire contract aligned with `AgentResponseType`'s serde representation.
/// Debug formatting is not a stable API contract and collapses multi-word variants.
fn agent_response_type_name(response_type: &AgentResponseType) -> &'static str {
    match response_type {
        AgentResponseType::Answer => "answer",
        AgentResponseType::Clarification => "clarification",
        AgentResponseType::ConfirmationRequired => "confirmation_required",
        AgentResponseType::TaskCreated => "task_created",
        AgentResponseType::TaskProgress => "task_progress",
        AgentResponseType::TaskCompleted => "task_completed",
        AgentResponseType::Error => "error",
    }
}

// ============ 辅助函数 ============

/// 输入限制常量
const MAX_INPUT_LEN: usize = 2000;
const MAX_HISTORY_ITEMS: usize = 50;

/// 将 API 层的 ProcessContext 转换为 service 层的 RequestContext
fn build_request_context(ctx: ProcessContext) -> RequestContext {
    let conversation_history = ctx.conversation_history.map(|msgs| {
        msgs.into_iter()
            .take(MAX_HISTORY_ITEMS)
            .map(|m| crate::services::agent::types::ConversationMessage {
                role: m.role,
                content: m.content,
                created_at: m.created_at,
            })
            .collect()
    });

    RequestContext {
        current_route: ctx.current_route,
        active_platforms: ctx.active_platforms.unwrap_or_default(),
        preferences: None,
        session_id: ctx.session_id,
        conversation_history,
        custom_data: ctx.custom_data,
        lane_key: None, // 由 API 层在调用处注入
        run_id: None,   // 由 process_stream 在 create_run 后注入
    }
}

/// 将 DataDisplayHint 从 service 层转换为 API 层类型
fn convert_data_display_hint(hint: crate::services::agent::DataDisplayHint) -> DataDisplayHintApi {
    match hint {
        crate::services::agent::DataDisplayHint::Table { columns, data_path } => {
            DataDisplayHintApi::Table {
                columns: columns
                    .into_iter()
                    .map(|c| ColumnDefApi {
                        field: c.field,
                        title: c.title,
                        width: c.width,
                        sortable: c.sortable,
                    })
                    .collect(),
                data_path,
            }
        }
        crate::services::agent::DataDisplayHint::Chart {
            chart_type,
            x_field,
            y_field,
        } => DataDisplayHintApi::Chart {
            chart_type: format!("{:?}", chart_type).to_lowercase(),
            x_field,
            y_field,
        },
        crate::services::agent::DataDisplayHint::CardList {
            title_field,
            description_field,
            image_field,
        } => DataDisplayHintApi::CardList {
            title_field,
            description_field,
            image_field,
        },
        crate::services::agent::DataDisplayHint::Markdown => DataDisplayHintApi::Markdown,
        crate::services::agent::DataDisplayHint::KeyValue => DataDisplayHintApi::KeyValue,
        crate::services::agent::DataDisplayHint::Timeline {
            time_field,
            content_field,
        } => DataDisplayHintApi::Timeline {
            time_field,
            content_field,
        },
        crate::services::agent::DataDisplayHint::Raw => DataDisplayHintApi::Raw,
    }
}

/// 解析 user_id，返回标准化错误
fn parse_user_id(claims: &Claims) -> Result<i32, (StatusCode, Json<Value>)> {
    claims.sub.parse::<i32>().map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Invalid user" })),
        )
    })
}

/// 解析 user_id 并校验 Agent 可见性/使用权限
async fn parse_user_id_with_agent_access(
    claims: &Claims,
    db: &DatabaseConnection,
) -> Result<i32, (StatusCode, Json<Value>)> {
    let user_id = parse_user_id(claims)?;
    if let Err(msg) = crate::services::agent::ensure_agent_usage_allowed(db, user_id).await {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({ "error": msg, "code": "agent_access_denied" })),
        ));
    }
    Ok(user_id)
}

/// Global Agent management surfaces are restricted to the current administrator.
async fn require_current_admin(
    claims: &Claims,
    db: &DatabaseConnection,
) -> Result<i32, (StatusCode, Json<Value>)> {
    let user_id = parse_user_id(claims)?;
    if !crate::services::agent::user_is_current_admin(db, user_id).await {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "Administrator access required", "code": "admin_required" })),
        ));
    }
    Ok(user_id)
}

/// 验证输入长度
fn validate_input(input: &str) -> Result<(), (StatusCode, Json<Value>)> {
    if input.len() > MAX_INPUT_LEN {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": crate::services::agent::response_agent::input_too_long(MAX_INPUT_LEN)
            })),
        ));
    }
    if input.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": crate::services::agent::response_agent::input_empty() })),
        ));
    }
    Ok(())
}

// ============ API 端点 ============

/// 处理自然语言请求
/// POST /api/agent/process
pub async fn process(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<ProcessRequest>,
) -> Result<Json<ApiResponse>, (StatusCode, Json<Value>)> {
    let user_id = parse_user_id_with_agent_access(&claims, &db).await?;
    validate_input(&req.input)?;

    tracing::info!(
        user_id = user_id,
        input_len = req.input.len(),
        "[Agent API] Processing request"
    );

    let client_session_id = req
        .context
        .as_ref()
        .and_then(|c| c.session_id.as_deref())
        .map(str::to_string);
    let session_id = ensure_session(&db, client_session_id.as_deref(), user_id)
        .await
        .map_err(|error| {
            tracing::error!(%error, "[Agent API] Failed to ensure session");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": error })),
            )
        })?;
    let lane_key = LaneQueue::make_lane_key(user_id, Some(&session_id));
    let conversation_history = load_session_history(&db, &session_id, 20).await;
    if let Err(error) = persist_user_message(&db, &session_id, &req.input).await {
        tracing::warn!(%error, "[Agent API] Failed to persist user message");
    }

    let mut user_request = UserRequest {
        raw_input: req.input,
        timestamp: chrono::Utc::now(),
        user_id,
        context: req.context.map(build_request_context),
    };

    if let Some(ref mut ctx) = user_request.context {
        ctx.lane_key = Some(lane_key.clone());
        ctx.session_id = Some(session_id.clone());
        ctx.conversation_history = if conversation_history.is_empty() {
            None
        } else {
            Some(conversation_history)
        };
    } else {
        user_request.context = Some(RequestContext {
            lane_key: Some(lane_key.clone()),
            session_id: Some(session_id.clone()),
            conversation_history: if conversation_history.is_empty() {
                None
            } else {
                Some(conversation_history)
            },
            ..Default::default()
        });
    }

    // 获取 Lane Queue 执行许可（同一用户串行，全局并发上限 4）
    let _guard = LANE_QUEUE.acquire_timeout(&lane_key, std::time::Duration::from_secs(LaneQueue::DEFAULT_ACQUIRE_TIMEOUT_SECS)).await.map_err(|e| {
        tracing::warn!(error = %e, "[Agent API] Queue acquisition failed");
        (StatusCode::SERVICE_UNAVAILABLE, Json(json!({ "error": e })))
    })?;

    // 创建 Agent 并处理请求
    let agent = Agent::new(db.clone()).await;
    let response = agent.process(user_request).await.map_err(|e| {
        tracing::error!(error = %e, "[Agent API] Processing failed");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e })),
        )
    })?;

    let mut api_response: ApiResponse = response.into();
    let metadata = json!({
        "suggestions": &api_response.suggestions,
        "dataDisplay": &api_response.data_display,
        "frontendAction": &api_response.frontend_action,
        "data": &api_response.data,
    });
    if let Err(error) = persist_assistant_message(
        &db,
        &session_id,
        api_response.task.as_ref().map(|task| task.task_id.as_str()),
        &api_response.message,
        Some(metadata),
    )
    .await
    {
        tracing::warn!(%error, "[Agent API] Failed to persist assistant message");
    }
    api_response.session_id = Some(session_id);

    Ok(Json(api_response))
}

/// 流式处理自然语言请求（带实时进度更新）
/// POST /api/agent/process/stream
pub async fn process_stream(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<ProcessRequest>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, (StatusCode, Json<Value>)> {
    let user_id = parse_user_id_with_agent_access(&claims, &db).await?;
    validate_input(&req.input)?;

    tracing::info!(
        user_id = user_id,
        input_len = req.input.len(),
        "[Agent API] Processing request with streaming"
    );

    let client_session_id = req
        .context
        .as_ref()
        .and_then(|c| c.session_id.as_deref())
        .map(|s| s.to_string());

    // 确保会话存在（自动创建或验证已有会话）
    let session_id = match ensure_session(&db, client_session_id.as_deref(), user_id).await {
        Ok(sid) => sid,
        Err(e) => {
            tracing::warn!("[Agent API] Failed to ensure session: {}", e);
            // 不阻塞主流程，降级为无会话模式
            String::new()
        }
    };

    let has_session = !session_id.is_empty();

    // 从数据库加载会话历史（替代前端传入的 conversation_history）
    let conversation_history = if has_session {
        let history = load_session_history(&db, &session_id, 20).await;
        if !history.is_empty() {
            tracing::info!(
                session_id = %session_id,
                history_count = history.len(),
                "[Agent API] Loaded conversation history from DB"
            );
        }
        Some(history)
    } else {
        None
    };

    if has_session {
        if let Err(e) = persist_user_message(&db, &session_id, &req.input).await {
            tracing::warn!("[Agent API] Failed to persist user message: {}", e);
        }
    }

    let lane_key = LaneQueue::make_lane_key(user_id, Some(&session_id));

    let mut user_request = UserRequest {
        raw_input: req.input,
        timestamp: chrono::Utc::now(),
        user_id,
        context: req.context.map(build_request_context),
    };

    // 将 lane_key 和 session_id 注入到请求上下文
    if let Some(ref mut ctx) = user_request.context {
        ctx.lane_key = Some(lane_key.clone());
        if has_session {
            ctx.session_id = Some(session_id.clone());
        }
        // 用数据库加载的历史覆盖前端传入的（服务端为 source of truth）
        if let Some(history) = conversation_history {
            ctx.conversation_history = Some(history);
        }
    } else {
        let mut new_ctx = RequestContext {
            lane_key: Some(lane_key.clone()),
            ..Default::default()
        };
        if has_session {
            new_ctx.session_id = Some(session_id.clone());
        }
        if let Some(history) = conversation_history {
            new_ctx.conversation_history = Some(history);
        }
        user_request.context = Some(new_ctx);
    }

    // 后端 run 独立于本次 HTTP 连接；前端只订阅事件。
    // 刻意不在 SSE 断连时取消任务：刷新 / reattach 依赖 run 继续存活；
    // 用户中断走 cancelTask API + is_cancelled 协作取消。
    let run = create_run(user_id, has_session.then_some(session_id.clone())).await;
    let run_id_for_meta = run.run_id().to_string();
    // 注入 run_id，供确认手持（confirmation）复用同一 run hub / 通知身份
    if let Some(ref mut ctx) = user_request.context {
        ctx.run_id = Some(run_id_for_meta.clone());
    }

    // Agent/executor 继续使用有背压的 mpsc；独立转发器负责写入 run hub。
    // On TaskCreated, persist runId/taskId into session history so mid-run
    // panel refresh can reattach (criterion 4) before wait/final complete.
    let (tx, rx) = tokio::sync::mpsc::channel::<ProgressEvent>(32);
    let run_for_forwarder = run.clone();
    let session_for_identity = session_id.clone();
    let db_for_identity = db.clone();
    let run_id_for_identity = run_id_for_meta.clone();
    tokio::spawn(async move {
        let mut rx = rx;
        let mut mid_run_identity_persisted = false;
        while let Some(event) = rx.recv().await {
            // Snapshot identity fields before moving event into publish.
            let mid_run_identity = if !mid_run_identity_persisted
                && !session_for_identity.is_empty()
            {
                match &event {
                    AgentProgressEvent::TaskCreated {
                        task_id, message, ..
                    } => Some((task_id.clone(), message.clone())),
                    _ => None,
                }
            } else {
                None
            };

            // Criterion 5: live fanout first — never await DB on this hot path.
            run_for_forwarder.publish(event).await;

            // Criterion 4: best-effort session identity for reattach; fire-and-forget.
            if let Some((task_id, message)) = mid_run_identity {
                mid_run_identity_persisted = true;
                let db = db_for_identity.clone();
                let session_id = session_for_identity.clone();
                let run_id = run_id_for_identity.clone();
                tokio::spawn(async move {
                    let metadata = session_metadata_with_run_identity(
                        Some(json!({
                            "task": {
                                "taskId": task_id,
                                "status": "running",
                            },
                        })),
                        &run_id,
                        &task_id,
                    );
                    let _ = persist_assistant_message(
                        &db,
                        &session_id,
                        Some(&task_id),
                        &message,
                        Some(metadata),
                    )
                    .await;
                });
            }
        }
    });

    // 在后台执行任务
    let db_clone = db.clone();
    let session_id_clone = session_id.clone();
    let queue = LANE_QUEUE.clone();
    // tx 会被移动到 spawn 中，确保 channel 在任务完成前不会关闭
    tokio::spawn(async move {
        // 获取 Lane Queue 执行许可（同一用户串行，全局并发上限 4）
        // 注意：进入 wait-for-input 后必须释放，否则最多 4 个等待任务会堵死全局槽位
        {
            let qs = queue.get_status().await;
            if qs.available_permits == 0 || qs.waiting > 0 {
                let ahead = qs.waiting.saturating_add(1);
                let _ = tx
                    .send(AgentProgressEvent::Progress {
                        progress: 0,
                        completed_steps: 0,
                        total_steps: 0,
                        message: format!("排队中（前方约 {} 个任务）…", ahead),
                    })
                    .await;
            }
        }
        let mut lane_guard = match queue.acquire_timeout(&lane_key, std::time::Duration::from_secs(LaneQueue::DEFAULT_ACQUIRE_TIMEOUT_SECS)).await {
            Ok(guard) => Some(guard),
            Err(e) => {
                let _ = tx
                    .send(AgentProgressEvent::Error {
                        task_id: None,
                        message: e,
                        code: "QUEUE_FULL".to_string(),
                    })
                    .await;
                return;
            }
        };

        let agent = Agent::new(db_clone.clone()).await;

        // 发送 session_id 给前端（让前端后续请求带上）
        if !session_id_clone.is_empty() {
            let _ = tx
                .send(AgentProgressEvent::SessionCreated {
                    session_id: session_id_clone.clone(),
                })
                .await;
        }

        // 使用带进度回调的处理方法
        match agent.process_with_progress(user_request, tx.clone()).await {
            Ok(response) => {
                let api_response: ApiResponse = response.into();
                let task_id = api_response
                    .task
                    .as_ref()
                    .map(|t| t.task_id.clone())
                    .unwrap_or_default();
                let success = api_response.success;

                // 检查任务是否在等待用户输入
                let is_waiting = api_response
                    .task
                    .as_ref()
                    .map(|t| t.status == "waiting_for_input")
                    .unwrap_or(false);

                if is_waiting && !task_id.is_empty() {
                    // 任务需要用户回答。前端已通过 waiting_for_input SSE 事件收到问题。
                    // 释放全局 lane 许可，避免无限等待占满 Semaphore(max=4)。
                    // resume 执行在 answer_stream 中重新获取许可。
                    drop(lane_guard.take());
                    tracing::info!(
                        task_id = %task_id,
                        "[Agent API] Released lane permit while waiting for user input"
                    );

                    // 持久化首次问题消息（含 runId，供刷新后 reattach）
                    if !session_id_clone.is_empty() {
                        let metadata = session_metadata_with_run_identity(
                            Some(json!({
                                "suggestions": &api_response.suggestions,
                                "data": &api_response.data,
                                "task": {
                                    "taskId": task_id,
                                    "status": "waiting_for_input",
                                    "pendingQuestion": api_response.task.as_ref().and_then(|t| t.pending_question.clone()),
                                },
                            })),
                            &run_id_for_meta,
                            &task_id,
                        );
                        let _ = persist_assistant_message(
                            &db_clone,
                            &session_id_clone,
                            Some(&task_id),
                            &api_response.message,
                            Some(metadata),
                        )
                        .await;
                    }

                    loop {
                        let (done_tx, done_rx) =
                            tokio::sync::oneshot::channel::<serde_json::Value>();
                        {
                            let mut map = WAITING_TASKS.write().await;
                            map.insert(
                                task_id.clone(),
                                WaitingTaskCtx {
                                    user_id, // process_stream 的 user_id 在外层 spawn 中可用
                                    progress_tx: tx.clone(),
                                    done_tx,
                                    session_id: session_id_clone.clone(),
                                },
                            );
                        }
                        tracing::info!(task_id = %task_id, "[Agent API] Task waiting for user input, keeping run alive");

                        // 本副本回答通过 oneshot 即时返回；跨副本回答没有本地
                        // WaitingTaskCtx，因此每 2 秒从权威数据库刷新一次。
                        match tokio::time::timeout(tokio::time::Duration::from_secs(2), done_rx)
                            .await
                        {
                            Ok(Ok(response_value)) => {
                                // 检查任务是否仍在等待用户输入（多轮提问）
                                let still_waiting = response_value
                                    .pointer("/task/status")
                                    .and_then(|s| s.as_str())
                                    == Some("waiting_for_input");

                                if still_waiting {
                                    // 仍有新问题需要用户回答，持久化中间状态后继续等待
                                    // 必须带 top-level runId/taskId（与首次 wait 一致），否则
                                    // 多轮后最新消息缺少 run 身份，刷新无法 re-subscribe。
                                    if !session_id_clone.is_empty() {
                                        let msg = response_value
                                            .get("message")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("需要更多信息");
                                        let metadata = session_metadata_with_run_identity(
                                            Some(response_value.clone()),
                                            &run_id_for_meta,
                                            &task_id,
                                        );
                                        let _ = persist_assistant_message(
                                            &db_clone,
                                            &session_id_clone,
                                            Some(&task_id),
                                            msg,
                                            Some(metadata),
                                        )
                                        .await;
                                    }
                                    tracing::info!(
                                        task_id = %task_id,
                                        "[Agent API] Task still waiting after answer, looping for next round"
                                    );
                                    continue;
                                }

                                // 任务真正完成
                                tracing::info!(
                                    "[Agent API] Answer result received, sending TaskCompleted"
                                );
                                let task_success = response_value
                                    .get("success")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(true);

                                // 持久化最终结果（同样带 runId/taskId）
                                if !session_id_clone.is_empty() {
                                    let final_msg = response_value
                                        .get("message")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("任务已完成");
                                    let metadata = session_metadata_with_run_identity(
                                        Some(response_value.clone()),
                                        &run_id_for_meta,
                                        &task_id,
                                    );
                                    let _ = persist_assistant_message(
                                        &db_clone,
                                        &session_id_clone,
                                        Some(&task_id),
                                        final_msg,
                                        Some(metadata),
                                    )
                                    .await;
                                }

                                if let Err(e) = tx
                                    .send(AgentProgressEvent::TaskCompleted {
                                        task_id: task_id.clone(),
                                        success: task_success,
                                        response: Box::new(response_value),
                                    })
                                    .await
                                {
                                    tracing::error!(
                                        task_id = %task_id,
                                        error = %e,
                                        "[Agent API] Failed to publish TaskCompleted to run hub"
                                    );
                                }
                                break;
                            }
                            Ok(Err(_)) => {
                                // done_tx 被丢弃：必须发布终态，否则 re-subscribe 会永久挂起
                                tracing::warn!(
                                    task_id = %task_id,
                                    "[Agent API] Answer sender dropped unexpectedly; terminalizing run"
                                );
                                let _ = take_waiting_task(&task_id, user_id).await;
                                let _ = tx
                                    .send(wait_loop_channel_dropped_event(&task_id))
                                    .await;
                                break;
                            }
                            Err(_) => {
                                tracing::debug!(task_id = %task_id, "[Agent API] Polling persisted waiting task state");
                                let _ = take_waiting_task(&task_id, user_id).await;

                                // 强制查数据库，不能让本副本的 waiting 缓存遮蔽
                                // 另一副本已写入的完成/失败状态。
                                let current_task =
                                    crate::services::agent::executor::refresh_task_for_user(
                                        &task_id, user_id,
                                    )
                                    .await;

                                // 问题过期：干净退出，释放 run，避免永久轮询
                                if let Some(task) = current_task.as_ref() {
                                    if task.status
                                        == crate::services::agent::types::TaskStatus::WaitingForInput
                                    {
                                        let expired = task
                                            .pending_question
                                            .as_ref()
                                            .is_some_and(|q| q.is_expired(chrono::Utc::now()));
                                        if expired {
                                            tracing::warn!(
                                                task_id = %task_id,
                                                "[Agent API] Waiting question expired; closing wait loop"
                                            );
                                            let response_value = json!({
                                                "success": false,
                                                "message": "等待用户输入已超时",
                                                "streamTerminal": true,
                                                "task": {
                                                    "taskId": task_id.clone(),
                                                    "status": "failed"
                                                }
                                            });
                                            let _ = tx
                                                .send(AgentProgressEvent::TaskCompleted {
                                                    task_id: task_id.clone(),
                                                    success: false,
                                                    response: Box::new(response_value),
                                                })
                                                .await;
                                            // 标记任务失败，避免幽灵 waiting
                                            if let Some(mut t) = crate::services::agent::executor::get_task_for_user(
                                                &task_id, user_id,
                                            )
                                            .await
                                            {
                                                t.status = crate::services::agent::types::TaskStatus::Failed;
                                                t.error = Some("等待用户输入已超时".into());
                                                t.completed_at = Some(chrono::Utc::now());
                                                t.pending_question = None;
                                                {
                                                    let mut store =
                                                        crate::services::agent::executor::TASK_STORE
                                                            .write()
                                                            .await;
                                                    store.store(user_id, t.clone());
                                                }
                                                crate::services::agent::executor::persist_task_async(
                                                    user_id, t,
                                                );
                                            }
                                            break;
                                        }
                                    }
                                }

                                if current_task.as_ref().is_some_and(|task| {
                                    matches!(
                                        task.status,
                                        crate::services::agent::types::TaskStatus::Pending
                                            | crate::services::agent::types::TaskStatus::Running
                                            | crate::services::agent::types::TaskStatus::WaitingForInput
                                            | crate::services::agent::types::TaskStatus::Paused
                                    )
                                }) {
                                    // 等待输入不是失败或完成。继续保持后端 run 与回答入口，
                                    // 下一轮重新注册 oneshot；前端是否在线不影响任务状态。
                                    tracing::info!(
                                        task_id = %task_id,
                                        "[Agent API] Task still waiting for input; keeping run alive"
                                    );
                                    continue;
                                }

                                let (response_value, task_success) =
                                    if let Some(task) = current_task {
                                        let task_success = task.status
                                            == crate::services::agent::types::TaskStatus::Completed;
                                        let message = task.error.clone().unwrap_or_else(|| {
                                            if task_success {
                                                "任务已完成".to_string()
                                            } else {
                                                "任务未完成".to_string()
                                            }
                                        });
                                        (
                                            json!({
                                                "success": task_success,
                                                "message": message,
                                                "task": task,
                                            }),
                                            task_success,
                                        )
                                    } else {
                                        (
                                            json!({
                                                "success": false,
                                                "message": "任务状态已不可用",
                                                "task": {
                                                    "taskId": task_id.clone(),
                                                    "status": "failed"
                                                }
                                            }),
                                            false,
                                        )
                                    };

                                if let Err(e) = tx
                                    .send(AgentProgressEvent::TaskCompleted {
                                        task_id: task_id.clone(),
                                        success: task_success,
                                        response: Box::new(response_value),
                                    })
                                    .await
                                {
                                    tracing::error!(
                                        task_id = %task_id,
                                        error = %e,
                                        "[Agent API] Failed to send timeout TaskCompleted"
                                    );
                                }
                                break;
                            }
                        }
                    }
                } else {
                    // 非 waiting 路径：正常流程结束时 guard 会在 spawn 结束时 drop
                    let _ = lane_guard.take();
                    // 正常流程：立即发送 TaskCompleted

                    // 持久化 assistant 消息（含 runId/taskId，供会话恢复）
                    if !session_id_clone.is_empty() {
                        let metadata = json!({
                            "suggestions": &api_response.suggestions,
                            "dataDisplay": &api_response.data_display,
                            "frontendAction": &api_response.frontend_action,
                            "data": &api_response.data,
                            "runId": run_id_for_meta,
                            "taskId": if task_id.is_empty() { Value::Null } else { json!(task_id) },
                        });
                        if let Err(e) = persist_assistant_message(
                            &db_clone,
                            &session_id_clone,
                            if task_id.is_empty() {
                                None
                            } else {
                                Some(&task_id)
                            },
                            &api_response.message,
                            Some(metadata),
                        )
                        .await
                        {
                            tracing::warn!(
                                "[Agent API] Failed to persist assistant message: {}",
                                e
                            );
                        }
                    }

                    let response_value = serde_json::to_value(&api_response)
                        .unwrap_or_else(|_| json!({"error": "serialization failed"}));

                    tracing::info!("[Agent API] Sending TaskCompleted event");
                    let send_result = tx
                        .send(AgentProgressEvent::TaskCompleted {
                            task_id,
                            success,
                            response: Box::new(response_value),
                        })
                        .await;
                    if let Err(e) = send_result {
                        tracing::error!(error = %e, "[Agent API] Failed to send TaskCompleted event");
                    } else {
                        tracing::info!("[Agent API] TaskCompleted event sent successfully");
                    }
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "[Agent API] Processing failed, sending error event");

                // 持久化错误消息
                if !session_id_clone.is_empty() {
                    let _ = persist_assistant_message(
                        &db_clone,
                        &session_id_clone,
                        None,
                        &format!("处理失败: {}", e),
                        Some(json!({"error": true})),
                    )
                    .await;
                }

                let _ = tx
                    .send(AgentProgressEvent::Error {
                        task_id: None,
                        message: e.clone(),
                        code: "PROCESSING_ERROR".to_string(),
                    })
                    .await;
            }
        }
        // tx 在这里被 drop，channel 关闭，SSE 流结束
    });

    // SSE 只是 run hub 的一个订阅者；连接被关闭不会触碰后台 sender 或执行任务。
    let stream = agent_run_event_stream(run);

    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

/// 重新订阅一个已存在的 Agent run。
/// GET /api/agent/runs/{run_id}/stream
pub async fn subscribe_run_stream(
    Extension(claims): Extension<Claims>,
    Path(run_id): Path<String>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, (StatusCode, Json<Value>)> {
    let user_id = parse_user_id(&claims)?;
    let run = get_run_for_user(&run_id, user_id).await.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Run not found, expired, or access denied" })),
        )
    })?;

    Ok(Sse::new(agent_run_event_stream(run))
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

/// 获取任务状态
/// GET /api/agent/tasks/{task_id}
pub async fn get_task(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(task_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let user_id = parse_user_id(&claims)?;

    tracing::debug!(
        user_id = user_id,
        task_id = %task_id,
        "[Agent API] Getting task status"
    );

    // 带所有权校验，防止 IDOR
    let agent = Agent::new(db).await;
    let task = agent.get_task_for_user(&task_id, user_id).await;

    match task {
        Some(task_state) => {
            let trace = task_state.execution_trace.as_ref().map(|t| {
                serde_json::json!({
                    "traceId": t.trace_id,
                    "totalDurationMs": t.total_duration_ms,
                    "tierUsage": t.tier_usage,
                    "steps": t.steps.iter().map(|s| serde_json::json!({
                        "stepId": s.step_id,
                        "capabilityId": s.capability_id,
                        "tierUsed": s.tier_used,
                        "durationMs": s.duration_ms,
                        "success": s.success,
                        "error": s.error,
                    })).collect::<Vec<_>>(),
                })
            });
            Ok(Json(json!({
                "success": true,
                "task": TaskInfo::from(&task_state),
                "results": task_state.step_results,
                "startedAt": task_state.started_at.to_rfc3339(),
                "completedAt": task_state.completed_at.map(|t| t.to_rfc3339()),
                "executionTrace": trace,
            })))
        }
        None => Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Task not found or access denied" })),
        )),
    }
}

/// 获取用户的所有任务
/// GET /api/agent/tasks
/// 任务列表分页参数
#[derive(Debug, Deserialize, Default)]
pub struct TaskListQuery {
    /// 最多返回多少条，默认 20，最大 100
    #[serde(default = "default_task_limit")]
    pub limit: usize,
    /// 偏移量，默认 0
    #[serde(default)]
    pub offset: usize,
}

fn default_task_limit() -> usize {
    20
}

pub async fn list_tasks(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Query(pagination): Query<TaskListQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let user_id = parse_user_id(&claims)?;

    // 限制单次最多返回 100 条
    let limit = pagination.limit.min(100);
    let offset = pagination.offset;

    tracing::debug!(
        user_id = user_id,
        limit = limit,
        offset = offset,
        "[Agent API] Listing user tasks"
    );

    let agent = Agent::new(db).await;
    // get_user_tasks 已按 started_at 降序（最新在前），并合并 DB
    let all_tasks = agent.get_user_tasks(user_id).await;
    let total = all_tasks.len();

    let task_list: Vec<Value> = all_tasks
        .iter()
        .skip(offset)
        .take(limit)
        .map(|t| {
            json!({
                "taskId": t.task_id,
                "recipeId": t.recipe_id,
                "status": task_status_name(&t.status),
                "progress": t.progress,
                "startedAt": t.started_at.to_rfc3339(),
                "completedAt": t.completed_at.map(|time| time.to_rfc3339())
            })
        })
        .collect();

    Ok(Json(json!({
        "success": true,
        "tasks": task_list,
        "total": total,
        "limit": limit,
        "offset": offset,
        "hasMore": offset + limit < total
    })))
}

/// 获取执行追踪列表
/// GET /api/agent/traces?limit=20
pub async fn list_traces(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Query(pagination): Query<TaskListQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let user_id = parse_user_id(&claims)?;
    let limit = pagination.limit.min(50);

    let agent = Agent::new(db).await;
    let all_tasks = agent.get_user_tasks(user_id).await;

    // 只返回有 execution_trace 的已完成任务（all_tasks 已按最新优先）
    let traces: Vec<Value> = all_tasks
        .iter()
        .filter_map(|t| {
            t.execution_trace.as_ref().map(|trace| {
                json!({
                    "traceId": trace.trace_id,
                    "taskId": t.task_id,
                    "recipeId": t.recipe_id,
                    "status": format!("{:?}", t.status).to_lowercase(),
                    "totalDurationMs": trace.total_duration_ms,
                    "tierUsage": trace.tier_usage,
                    "steps": trace.steps.iter().map(|s| json!({
                        "stepId": s.step_id,
                        "capabilityId": s.capability_id,
                        "tierUsed": s.tier_used,
                        "durationMs": s.duration_ms,
                        "success": s.success,
                        "error": s.error,
                    })).collect::<Vec<_>>(),
                    "startedAt": t.started_at.to_rfc3339(),
                    "completedAt": t.completed_at.map(|time| time.to_rfc3339()),
                })
            })
        })
        .take(limit)
        .collect();

    Ok(Json(json!({
        "success": true,
        "traces": traces,
        "total": traces.len(),
    })))
}

/// 获取系统能力列表
/// GET /api/agent/capabilities
pub async fn list_capabilities(
    Extension(_claims): Extension<Claims>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let capabilities = crate::services::agent::get_capabilities_summary().await;

    Ok(Json(json!({
        "success": true,
        "capabilities": capabilities
    })))
}

/// 取消任务
/// POST /api/agent/tasks/{task_id}/cancel
pub async fn cancel_task(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(task_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let user_id = parse_user_id(&claims)?;

    tracing::info!(
        user_id = user_id,
        task_id = %task_id,
        "[Agent API] Cancelling task"
    );

    // 通过 Agent 接口取消（含所有权校验，防止 IDOR）
    let agent = Agent::new(db).await;
    let cancelled = agent.cancel_task_for_user(&task_id, user_id).await;

    if cancelled {
        // 等待输入中的 run 正阻塞在 done_rx；显式取消必须立即唤醒它，
        // 否则通知会在最多十分钟内仍错误显示为“等待回答”。
        if let Some(waiting) = take_waiting_task(&task_id, user_id).await {
            let _ = waiting.done_tx.send(json!({
                "success": false,
                "responseType": "error",
                "message": "任务已取消",
                "task": {
                    "taskId": task_id,
                    "status": "cancelled",
                    "progress": 0
                }
            }));
        }
        Ok(Json(json!({
            "success": true,
            "message": "Task cancellation requested",
            "taskId": task_id
        })))
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Task not found or access denied" })),
        ))
    }
}

/// 回答问题请求
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnswerQuestionRequest {
    /// 问题 ID
    pub question_id: String,
    /// 用户答案
    pub answer: String,
}

/// 回答任务中的问题
/// POST /api/agent/tasks/{task_id}/answer
///
/// 使用 resume_with_answer 从暂停点恢复执行，而不是重新从头处理。
pub async fn answer_task_question(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(task_id): Path<String>,
    Json(req): Json<AnswerQuestionRequest>,
) -> Result<Json<ApiResponse>, (StatusCode, Json<Value>)> {
    let user_id = parse_user_id_with_agent_access(&claims, &db).await?;

    tracing::info!(
        user_id = user_id,
        task_id = %task_id,
        question_id = %req.question_id,
        "[Agent API] Answering task question (resume mode)"
    );

    let agent = Agent::new(db.clone()).await;

    let answer = UserAnswer {
        question_id: req.question_id,
        task_id: task_id.clone(),
        answer: req.answer,
        skipped: false,
    };

    agent
        .resume_task(&task_id, answer, user_id)
        .await
        .map(|response| Json(ApiResponse::from(response)))
        .map_err(|e| {
            tracing::error!(error = %e, "[Agent API] Failed to resume task");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("恢复任务失败: {}", e) })),
            )
        })
}

/// 回答问题（SSE 流式版本）
/// POST /api/agent/tasks/{task_id}/answer/stream
pub async fn answer_task_question_stream(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(task_id): Path<String>,
    Json(req): Json<AnswerQuestionRequest>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, (StatusCode, Json<Value>)> {
    let user_id = parse_user_id_with_agent_access(&claims, &db).await?;

    tracing::info!(
        user_id = user_id,
        task_id = %task_id,
        question_id = %req.question_id,
        "[Agent API] Answering task question (SSE stream mode)"
    );

    let (tx, rx) = tokio::sync::mpsc::channel::<ProgressEvent>(32);

    let db_clone = db.clone();
    tokio::spawn(async move {
        // Resolve session/lane BEFORE take so we match process_stream's session lane.
        let session_from_waiting = {
            let map = WAITING_TASKS.read().await;
            map.get(&task_id)
                .filter(|ctx| ctx.user_id == user_id)
                .map(|ctx| ctx.session_id.clone())
                .filter(|s| !s.is_empty())
        };
        let task_for_lane =
            crate::services::agent::executor::get_task_for_user(&task_id, user_id).await;
        let lane_key = LaneQueue::resolve_answer_lane_key(
            user_id,
            task_for_lane.as_ref().and_then(|t| t.lane_id.as_deref()),
            session_from_waiting.as_deref(),
        );

        // resume 执行前重新获取 lane 许可（process_stream 在 wait-for-input 时已释放）
        let queue = LANE_QUEUE.clone();
        let _lane_guard = match queue.acquire_timeout(&lane_key, std::time::Duration::from_secs(LaneQueue::DEFAULT_ACQUIRE_TIMEOUT_SECS)).await {
            Ok(guard) => guard,
            Err(e) => {
                let _ = tx
                    .send(AgentProgressEvent::Error {
                        task_id: Some(task_id.clone()),
                        message: e,
                        code: "QUEUE_FULL".to_string(),
                    })
                    .await;
                return;
            }
        };

        let agent = Agent::new(db_clone.clone()).await;

        // 从 WAITING_TASKS 获取后端 run 上下文（仅所有者可取，防跨用户抢 oneshot）
        let waiting_ctx = take_waiting_task(&task_id, user_id).await;

        // 持久化用户的回答到会话消息历史（确保后续 Planner 能看到完整对话）
        let ctx_session_id = waiting_ctx
            .as_ref()
            .map(|ctx| ctx.session_id.clone())
            .or(session_from_waiting)
            .unwrap_or_default();
        if !ctx_session_id.is_empty() {
            let _ = persist_user_message(&db_clone, &ctx_session_id, &req.answer).await;
        }

        let answer = UserAnswer {
            question_id: req.question_id,
            task_id: task_id.clone(),
            answer: req.answer,
            skipped: false,
        };

        let resume_tx = waiting_ctx
            .as_ref()
            .map(|ctx| ctx.progress_tx.clone())
            .unwrap_or_else(|| tx.clone());

        match agent
            .resume_task_with_progress(&task_id, answer, user_id, resume_tx)
            .await
        {
            Ok(response) => {
                let api_response: ApiResponse = response.into();
                let final_task_id = api_response
                    .task
                    .as_ref()
                    .map(|t| t.task_id.clone())
                    .unwrap_or_default();
                let success = api_response.success;

                // 检查任务是否仍然在等待用户输入（多轮提问场景）
                let still_waiting = api_response
                    .task
                    .as_ref()
                    .map(|t| t.status == "waiting_for_input")
                    .unwrap_or(false);

                let response_value = serde_json::to_value(&api_response)
                    .unwrap_or_else(|_| json!({"error": "serialization failed"}));

                // 回传结果给 process_stream（如果它在等待）
                // process_stream 的循环会检查 status 决定是否继续等待
                if let Some(ctx) = waiting_ctx {
                    let _ = ctx.done_tx.send(response_value.clone());
                }

                // 在 answer_stream 自己的 SSE 上发送事件
                if still_waiting {
                    // 任务仍在等待：发送 TaskCompleted（携带 pendingQuestion 数据，
                    // 前端 handleAgentResponse 会检测到并显示新问题）
                    // 这里仍然发 TaskCompleted 以便 executeSSERequest resolve
                    let _ = tx
                        .send(AgentProgressEvent::TaskCompleted {
                            task_id: final_task_id,
                            success,
                            response: Box::new(response_value),
                        })
                        .await;
                } else {
                    // 任务真正完成
                    let _ = tx
                        .send(AgentProgressEvent::TaskCompleted {
                            task_id: final_task_id,
                            success,
                            response: Box::new(response_value),
                        })
                        .await;
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "[Agent API] Resume failed");

                // 回传错误给 process_stream
                if let Some(ctx) = waiting_ctx {
                    let _ = ctx.done_tx.send(json!({
                        "success": false,
                        "message": e.clone(),
                        "responseType": "error",
                    }));
                }

                let _ = tx
                    .send(AgentProgressEvent::Error {
                        task_id: Some(task_id),
                        message: e,
                        code: "RESUME_ERROR".to_string(),
                    })
                    .await;
            }
        }
    });

    let stream = tokio_stream::wrappers::ReceiverStream::new(rx).map(|event| {
        let data = serde_json::to_string(&event).unwrap_or_else(|_| "{}".to_string());
        Ok(Event::default().data(data))
    });

    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

/// 提供澄清回答
/// POST /api/agent/clarify
#[derive(Debug, Deserialize)]
pub struct ClarifyRequest {
    /// 原始请求
    pub original_input: String,
    /// 澄清点 ID
    pub clarification_id: String,
    /// 用户选择或回答
    pub answer: String,
    /// 上下文
    #[serde(default)]
    pub context: Option<ProcessContext>,
}

pub async fn clarify(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<ClarifyRequest>,
) -> Result<Json<ApiResponse>, (StatusCode, Json<Value>)> {
    let user_id = parse_user_id_with_agent_access(&claims, &db).await?;
    validate_input(&req.original_input)?;

    tracing::info!(
        user_id = user_id,
        clarification_id = %req.clarification_id,
        "[Agent API] Processing clarification"
    );

    // 将澄清合并到原始请求
    let combined_input = format!("{}\n补充说明：{}", req.original_input, req.answer);

    let user_request = UserRequest {
        raw_input: combined_input,
        timestamp: chrono::Utc::now(),
        user_id,
        context: req.context.map(build_request_context),
    };

    let agent = Agent::new(db).await;
    let response = agent.process(user_request).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e })),
        )
    })?;

    Ok(Json(response.into()))
}

/// 确认敏感操作
/// POST /api/agent/confirm
pub async fn confirm_operation(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<ConfirmRequest>,
) -> Result<Json<ApiResponse>, (StatusCode, Json<Value>)> {
    let user_id = parse_user_id_with_agent_access(&claims, &db).await?;

    if req.confirmed {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Confirmed operations must use /api/agent/confirm/stream",
                "code": "confirmation_stream_required"
            })),
        ));
    }

    tracing::info!(
        confirmation_id = %req.confirmation_id,
        confirmed = req.confirmed,
        user_id = user_id,
        "[Agent API] Processing confirmation"
    );

    let confirmation = crate::services::agent::types::UserConfirmation {
        confirmation_id: req.confirmation_id,
        confirmed: req.confirmed,
        user_note: req.note,
        user_id,
    };

    let agent = Agent::new(db).await;
    let lane_key = agent
        .confirmation_lane_key(&confirmation.confirmation_id, user_id)
        .await
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": error })),
            )
        })?
        .unwrap_or_else(|| LaneQueue::make_lane_key(user_id, None));
    let _guard = LANE_QUEUE.acquire_timeout(&lane_key, std::time::Duration::from_secs(LaneQueue::DEFAULT_ACQUIRE_TIMEOUT_SECS)).await.map_err(|error| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": error })),
        )
    })?;
    let response = agent
        .process_confirmation(confirmation)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e })),
            )
        })?;

    Ok(Json(response.into()))
}

/// 确认敏感操作并通过可重连 run stream 执行续跑。
/// POST /api/agent/confirm/stream
pub async fn confirm_operation_stream(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<ConfirmRequest>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, (StatusCode, Json<Value>)> {
    let user_id = parse_user_id_with_agent_access(&claims, &db).await?;
    let confirmation = crate::services::agent::types::UserConfirmation {
        confirmation_id: req.confirmation_id.clone(),
        confirmed: req.confirmed,
        user_note: req.note,
        user_id,
    };

    // Peek session/lane before consume so the resume run stays attached to the
    // original conversation (history persistence + WAITING_TASKS answers).
    let agent_for_lookup = Agent::new(db.clone()).await;
    let resume_ctx = agent_for_lookup
        .confirmation_resume_context(&req.confirmation_id, user_id)
        .await
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": error })),
            )
        })?;
    let session_id = resume_ctx
        .as_ref()
        .and_then(|ctx| ctx.session_id.clone())
        .filter(|s| !s.is_empty());
    let original_run_id = resume_ctx
        .as_ref()
        .and_then(|ctx| ctx.run_id.clone())
        .filter(|s| !s.is_empty());
    let lane_key = resume_ctx
        .and_then(|ctx| ctx.lane_key)
        .unwrap_or_else(|| LaneQueue::make_lane_key(user_id, session_id.as_deref()));

    // Prefer the original process run so notifications/UI stay on one identity.
    let run = if let Some(ref rid) = original_run_id {
        match get_run_for_user(rid, user_id).await {
            Some(existing) => existing,
            None => create_run(user_id, session_id.clone()).await,
        }
    } else {
        create_run(user_id, session_id.clone()).await
    };
    let run_for_task = run.clone();
    let db_clone = db.clone();
    tokio::spawn(async move {
        // Agent/executor progress events share the same run hub as the SSE subscriber.
        let (tx, mut rx) = tokio::sync::mpsc::channel::<AgentProgressEvent>(32);
        let run_for_forwarder = run_for_task.clone();
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                run_for_forwarder.publish(event).await;
            }
        });

        let agent = Agent::new(db_clone.clone()).await;
        let _guard = match LANE_QUEUE.acquire_timeout(&lane_key, std::time::Duration::from_secs(LaneQueue::DEFAULT_ACQUIRE_TIMEOUT_SECS)).await {
            Ok(guard) => guard,
            Err(error) => {
                let _ = tx
                    .send(AgentProgressEvent::Error {
                        task_id: None,
                        message: error,
                        code: "QUEUE_FULL".to_string(),
                    })
                    .await;
                return;
            }
        };

        if let Some(ref sid) = session_id {
            let _ = tx
                .send(AgentProgressEvent::SessionCreated {
                    session_id: sid.clone(),
                })
                .await;
        }

        match agent.process_confirmation(confirmation).await {
            Ok(response) => {
                let api_response: ApiResponse = response.into();
                let task_id = api_response
                    .task
                    .as_ref()
                    .map(|task| task.task_id.clone())
                    .unwrap_or_default();
                let success = api_response.success;
                let is_waiting = api_response
                    .task
                    .as_ref()
                    .map(|t| t.status == "waiting_for_input")
                    .unwrap_or(false);

                // Persist confirmation result (or missing-param question) into the
                // original session history so refresh keeps the full thread.
                if let Some(ref sid) = session_id {
                    let metadata = json!({
                        "suggestions": &api_response.suggestions,
                        "dataDisplay": &api_response.data_display,
                        "frontendAction": &api_response.frontend_action,
                        "data": &api_response.data,
                        "confirmationResume": true,
                    });
                    if let Err(e) = persist_assistant_message(
                        &db_clone,
                        sid,
                        if task_id.is_empty() {
                            None
                        } else {
                            Some(&task_id)
                        },
                        &api_response.message,
                        Some(metadata),
                    )
                    .await
                    {
                        tracing::warn!(
                            "[Agent API] Failed to persist confirmation result: {}",
                            e
                        );
                    }
                }

                if is_waiting && !task_id.is_empty() {
                    // Surface the first missing-param / Q&A prompt on the run
                    // hub, then keep the run alive for answer/resume rounds.
                    let mut waiting_response = serde_json::to_value(&api_response)
                        .unwrap_or_else(|_| json!({ "success": true, "message": "" }));
                    if let Some(object) = waiting_response.as_object_mut() {
                        object.insert("streamTerminal".to_string(), Value::Bool(false));
                        if let Some(ref sid) = session_id {
                            object.insert("sessionId".to_string(), Value::String(sid.clone()));
                        }
                    }
                    let _ = tx
                        .send(AgentProgressEvent::TaskCompleted {
                            task_id: task_id.clone(),
                            success: true,
                            response: Box::new(waiting_response),
                        })
                        .await;

                    // Keep run alive and register WAITING_TASKS so subsequent
                    // answers (and final reply) stay on this session.
                    loop {
                        let (done_tx, done_rx) =
                            tokio::sync::oneshot::channel::<serde_json::Value>();
                        {
                            let mut map = WAITING_TASKS.write().await;
                            map.insert(
                                task_id.clone(),
                                WaitingTaskCtx {
                                    user_id,
                                    progress_tx: tx.clone(),
                                    done_tx,
                                    session_id: session_id.clone().unwrap_or_default(),
                                },
                            );
                        }
                        tracing::info!(
                            task_id = %task_id,
                            session_id = ?session_id,
                            "[Agent API] Confirmation resume waiting for user input"
                        );

                        match tokio::time::timeout(tokio::time::Duration::from_secs(2), done_rx)
                            .await
                        {
                            Ok(Ok(response_value)) => {
                                let still_waiting = response_value
                                    .pointer("/task/status")
                                    .and_then(|s| s.as_str())
                                    == Some("waiting_for_input");

                                if still_waiting {
                                    if let Some(ref sid) = session_id {
                                        let msg = response_value
                                            .get("message")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("需要更多信息");
                                        let run_id = run_for_task.run_id();
                                        let metadata = session_metadata_with_run_identity(
                                            Some(response_value.clone()),
                                            run_id,
                                            &task_id,
                                        );
                                        let _ = persist_assistant_message(
                                            &db_clone,
                                            sid,
                                            Some(&task_id),
                                            msg,
                                            Some(metadata),
                                        )
                                        .await;
                                    }
                                    continue;
                                }

                                if let Some(ref sid) = session_id {
                                    let msg = response_value
                                        .get("message")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("");
                                    if !msg.is_empty() {
                                        let run_id = run_for_task.run_id();
                                        let metadata = session_metadata_with_run_identity(
                                            Some(response_value.clone()),
                                            run_id,
                                            &task_id,
                                        );
                                        let _ = persist_assistant_message(
                                            &db_clone,
                                            sid,
                                            Some(&task_id),
                                            msg,
                                            Some(metadata),
                                        )
                                        .await;
                                    }
                                }

                                let task_success = response_value
                                    .get("success")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(true);
                                let _ = tx
                                    .send(AgentProgressEvent::TaskCompleted {
                                        task_id: task_id.clone(),
                                        success: task_success,
                                        response: Box::new(response_value),
                                    })
                                    .await;
                                break;
                            }
                            Ok(Err(_)) => {
                                tracing::warn!(
                                    task_id = %task_id,
                                    "[Agent API] Confirmation answer sender dropped; terminalizing run"
                                );
                                let _ = take_waiting_task(&task_id, user_id).await;
                                let _ = tx
                                    .send(wait_loop_channel_dropped_event(&task_id))
                                    .await;
                                break;
                            }
                            Err(_) => {
                                let _ = take_waiting_task(&task_id, user_id).await;
                                let current_task =
                                    crate::services::agent::executor::refresh_task_for_user(
                                        &task_id, user_id,
                                    )
                                    .await;
                                if current_task.as_ref().is_some_and(|task| {
                                    matches!(
                                        task.status,
                                        crate::services::agent::types::TaskStatus::Pending
                                            | crate::services::agent::types::TaskStatus::Running
                                            | crate::services::agent::types::TaskStatus::WaitingForInput
                                            | crate::services::agent::types::TaskStatus::Paused
                                    )
                                }) {
                                    continue;
                                }

                                let (response_value, task_success) =
                                    if let Some(task) = current_task {
                                        let task_success = task.status
                                            == crate::services::agent::types::TaskStatus::Completed;
                                        let message = task.error.clone().unwrap_or_else(|| {
                                            if task_success {
                                                "任务已完成".to_string()
                                            } else {
                                                "任务未完成".to_string()
                                            }
                                        });
                                        (
                                            json!({
                                                "success": task_success,
                                                "message": message,
                                                "task": task,
                                            }),
                                            task_success,
                                        )
                                    } else {
                                        (
                                            json!({
                                                "success": false,
                                                "message": "任务状态已不可用",
                                                "task": {
                                                    "taskId": task_id.clone(),
                                                    "status": "failed"
                                                }
                                            }),
                                            false,
                                        )
                                    };

                                if let Some(ref sid) = session_id {
                                    let msg = response_value
                                        .get("message")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("");
                                    if !msg.is_empty() {
                                        let _ = persist_assistant_message(
                                            &db_clone,
                                            sid,
                                            Some(&task_id),
                                            msg,
                                            Some(response_value.clone()),
                                        )
                                        .await;
                                    }
                                }

                                let _ = tx
                                    .send(AgentProgressEvent::TaskCompleted {
                                        task_id: task_id.clone(),
                                        success: task_success,
                                        response: Box::new(response_value),
                                    })
                                    .await;
                                break;
                            }
                        }
                    }
                } else {
                    let mut response_value = serde_json::to_value(api_response).unwrap_or_else(
                        |_| json!({ "success": false, "message": "Serialization failed" }),
                    );
                    if let Some(object) = response_value.as_object_mut() {
                        object.insert("streamTerminal".to_string(), Value::Bool(true));
                        if let Some(ref sid) = session_id {
                            object.insert("sessionId".to_string(), Value::String(sid.clone()));
                        }
                    }
                    let _ = tx
                        .send(AgentProgressEvent::TaskCompleted {
                            task_id,
                            success,
                            response: Box::new(response_value),
                        })
                        .await;
                }
            }
            Err(error) => {
                if let Some(ref sid) = session_id {
                    let _ = persist_assistant_message(
                        &db_clone,
                        sid,
                        None,
                        &format!("确认执行失败: {}", error),
                        Some(json!({ "error": true, "confirmationResume": true })),
                    )
                    .await;
                }
                let _ = tx
                    .send(AgentProgressEvent::Error {
                        task_id: None,
                        message: error,
                        code: "CONFIRMATION_EXECUTION_FAILED".to_string(),
                    })
                    .await;
            }
        }
    });

    Ok(Sse::new(agent_run_event_stream(run))
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

#[derive(Debug, Deserialize)]
pub struct ConfirmRequest {
    #[serde(rename = "confirmationId")]
    pub confirmation_id: String,
    pub confirmed: bool,
    #[serde(default)]
    pub note: Option<String>,
}

/// 健康检查
/// GET /api/agent/health
pub async fn health() -> Json<Value> {
    Json(json!({
        "status": "ok",
        "service": "agent",
        "version": env!("CARGO_PKG_VERSION")
    }))
}

// ============ 任务预设 API ============

/// 获取任务预设列表
/// GET /api/agent/presets
pub async fn list_presets(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<TaskPresetListResponse>, (StatusCode, Json<Value>)> {
    use sea_orm::QuerySelect;

    let user_id = parse_user_id(&claims)?;

    // 并行发起两次查询，减少串行等待时间
    let (favorites_result, history_result) = tokio::join!(
        agent_task_presets::Entity::find()
            .filter(agent_task_presets::Column::UserId.eq(user_id))
            .filter(agent_task_presets::Column::PresetType.eq("favorite"))
            .order_by_desc(agent_task_presets::Column::LastUsedAt)
            .all(&db),
        agent_task_presets::Entity::find()
            .filter(agent_task_presets::Column::UserId.eq(user_id))
            .filter(agent_task_presets::Column::PresetType.eq("history"))
            .order_by_desc(agent_task_presets::Column::LastUsedAt)
            .limit(20) // DB 层限制，避免拉取全量到内存
            .all(&db),
    );

    let favorites = favorites_result
        .map_err(|e| {
            tracing::error!("[Agent Presets] Failed to fetch favorites: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Failed to fetch favorites" })),
            )
        })?
        .into_iter()
        .map(TaskPresetResponse::from)
        .collect();

    let history = history_result
        .map_err(|e| {
            tracing::error!("[Agent Presets] Failed to fetch history: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Failed to fetch history" })),
            )
        })?
        .into_iter()
        .map(TaskPresetResponse::from)
        .collect();

    Ok(Json(TaskPresetListResponse { favorites, history }))
}

/// 创建或更新任务预设
/// POST /api/agent/presets
pub async fn create_preset(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<CreatePresetRequest>,
) -> Result<Json<TaskPresetResponse>, (StatusCode, Json<Value>)> {
    let user_id = parse_user_id(&claims)?;

    // 验证预设类型
    if req.preset_type != "favorite" && req.preset_type != "history" {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Invalid preset type, must be 'favorite' or 'history'" })),
        ));
    }

    // 输入长度校验
    validate_input(&req.input)?;
    if let Some(ref title) = req.title {
        if title.len() > 200 {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "标题过长，最大 200 字符" })),
            ));
        }
    }
    if let Some(ref summary) = req.intent_summary {
        if summary.len() > 500 {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "摘要过长，最大 500 字符" })),
            ));
        }
    }
    if let Some(ref steps) = req.parsed_steps {
        let steps_size = serde_json::to_string(steps).unwrap_or_default().len();
        if steps_size > 102_400 {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "解析步骤数据过大，最大 100KB" })),
            ));
        }
    }
    if let Some(ref conv) = req.conversation_data {
        if conv.len() > MAX_HISTORY_ITEMS {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": format!("对话历史过长，最大 {} 条", MAX_HISTORY_ITEMS) })),
            ));
        }
    }

    let now = Utc::now().fixed_offset();

    // 检查是否已存在相同的输入
    let existing = agent_task_presets::Entity::find()
        .filter(agent_task_presets::Column::UserId.eq(user_id))
        .filter(agent_task_presets::Column::Input.eq(&req.input))
        .one(&db)
        .await
        .map_err(|e| {
            tracing::error!("[Agent Presets] Failed to check existing preset: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Database error" })),
            )
        })?;

    if let Some(existing_preset) = existing {
        // 更新已存在的预设
        let mut active_model: agent_task_presets::ActiveModel = existing_preset.clone().into();
        // 只有当现有预设是 history 类型时才更新类型（保留 favorite 状态）
        if existing_preset.preset_type == "history" {
            active_model.preset_type = Set(req.preset_type);
        }
        active_model.last_used_at = Set(now);
        // 从原始 Model 获取 use_count 避免 ActiveValue::unwrap() panic
        active_model.use_count = Set(existing_preset.use_count + 1);
        if req.parsed_steps.is_some() {
            active_model.parsed_steps = Set(req.parsed_steps);
        }
        if req.intent_summary.is_some() {
            active_model.intent_summary = Set(req.intent_summary);
        }
        // 更新对话数据
        if req.title.is_some() {
            active_model.title = Set(req.title);
        }
        if req.conversation_data.is_some() {
            active_model.conversation_data = Set(req
                .conversation_data
                .map(|c| serde_json::to_value(&c).unwrap_or_default()));
        }

        let updated = active_model.update(&db).await.map_err(|e| {
            tracing::error!("[Agent Presets] Failed to update preset: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Failed to update preset" })),
            )
        })?;

        return Ok(Json(TaskPresetResponse::from(updated)));
    }

    // 创建新预设
    let new_preset = agent_task_presets::ActiveModel {
        id: sea_orm::ActiveValue::NotSet,
        user_id: Set(user_id),
        input: Set(req.input),
        preset_type: Set(req.preset_type.clone()),
        parsed_steps: Set(req.parsed_steps),
        intent_summary: Set(req.intent_summary),
        last_used_at: Set(now),
        use_count: Set(1),
        created_at: Set(now),
        title: Set(req.title),
        conversation_data: Set(req
            .conversation_data
            .map(|c| serde_json::to_value(&c).unwrap_or_default())),
    };

    let created = new_preset.insert(&db).await.map_err(|e| {
        tracing::error!("[Agent Presets] Failed to create preset: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Failed to create preset" })),
        )
    })?;

    // 如果是历史记录，清理超过 20 条的旧记录
    if req.preset_type == "history" {
        cleanup_old_history(&db, user_id).await;
    }

    Ok(Json(TaskPresetResponse::from(created)))
}

/// 清理超过 20 条的历史记录
///
/// 只查询超出部分的 ID（加 LIMIT+OFFSET），避免拉取全量数据到内存
async fn cleanup_old_history(db: &DatabaseConnection, user_id: i32) {
    use sea_orm::{PaginatorTrait, QuerySelect};

    // 先计算总数，若不超出则跳过
    let count = agent_task_presets::Entity::find()
        .filter(agent_task_presets::Column::UserId.eq(user_id))
        .filter(agent_task_presets::Column::PresetType.eq("history"))
        .count(db)
        .await
        .unwrap_or(0);

    if count <= 20 {
        return;
    }

    // 只查询第 21 条起的 ID，在 DB 层做 LIMIT/OFFSET
    let to_delete_ids: Vec<i32> = agent_task_presets::Entity::find()
        .filter(agent_task_presets::Column::UserId.eq(user_id))
        .filter(agent_task_presets::Column::PresetType.eq("history"))
        .order_by_desc(agent_task_presets::Column::LastUsedAt)
        .offset(20)
        .limit(count)
        .all(db)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|p| p.id)
        .collect();

    if !to_delete_ids.is_empty() {
        let _ = agent_task_presets::Entity::delete_many()
            .filter(agent_task_presets::Column::Id.is_in(to_delete_ids))
            .exec(db)
            .await;
    }
}

/// 删除任务预设（仅限历史类型，收藏类型不允许直接删除）
/// DELETE /api/agent/presets/{id}
pub async fn delete_preset(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(preset_id): Path<i32>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let user_id = parse_user_id(&claims)?;

    // 确保只能删除自己的预设
    let preset = agent_task_presets::Entity::find_by_id(preset_id)
        .filter(agent_task_presets::Column::UserId.eq(user_id))
        .one(&db)
        .await
        .map_err(|e| {
            tracing::error!("[Agent Presets] Failed to find preset: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Database error" })),
            )
        })?;

    let preset = preset.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Preset not found" })),
        )
    })?;

    // 只允许删除历史类型的预设，收藏类型需要先取消收藏
    if preset.preset_type == "favorite" {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Cannot delete favorite preset, please unfavorite first" })),
        ));
    }

    agent_task_presets::Entity::delete_by_id(preset_id)
        .exec(&db)
        .await
        .map_err(|e| {
            tracing::error!("[Agent Presets] Failed to delete preset: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Failed to delete preset" })),
            )
        })?;

    Ok(Json(json!({ "success": true })))
}

/// 切换收藏状态
/// POST /api/agent/presets/{id}/toggle-favorite
pub async fn toggle_favorite(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(preset_id): Path<i32>,
) -> Result<Json<TaskPresetResponse>, (StatusCode, Json<Value>)> {
    let user_id = parse_user_id(&claims)?;

    // 查找预设
    let preset = agent_task_presets::Entity::find_by_id(preset_id)
        .filter(agent_task_presets::Column::UserId.eq(user_id))
        .one(&db)
        .await
        .map_err(|e| {
            tracing::error!("[Agent Presets] Failed to find preset: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Database error" })),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "Preset not found" })),
            )
        })?;

    // 切换类型
    let new_type = if preset.preset_type == "favorite" {
        "history"
    } else {
        "favorite"
    };

    let mut active_model: agent_task_presets::ActiveModel = preset.into();
    active_model.preset_type = Set(new_type.to_string());

    let updated = active_model.update(&db).await.map_err(|e| {
        tracing::error!("[Agent Presets] Failed to toggle favorite: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Failed to toggle favorite" })),
        )
    })?;

    Ok(Json(TaskPresetResponse::from(updated)))
}

/// 更新预设使用时间（每次使用时调用）
/// POST /api/agent/presets/{id}/use
pub async fn use_preset(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(preset_id): Path<i32>,
) -> Result<Json<TaskPresetResponse>, (StatusCode, Json<Value>)> {
    let user_id = parse_user_id(&claims)?;

    let now = Utc::now().fixed_offset();

    // 查找预设
    let preset = agent_task_presets::Entity::find_by_id(preset_id)
        .filter(agent_task_presets::Column::UserId.eq(user_id))
        .one(&db)
        .await
        .map_err(|e| {
            tracing::error!("[Agent Presets] Failed to find preset: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Database error" })),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "Preset not found" })),
            )
        })?;

    let new_use_count = preset.use_count + 1;
    let mut active_model: agent_task_presets::ActiveModel = preset.into();
    active_model.last_used_at = Set(now);
    active_model.use_count = Set(new_use_count);

    let updated = active_model.update(&db).await.map_err(|e| {
        tracing::error!("[Agent Presets] Failed to update use time: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Failed to update preset" })),
        )
    })?;

    Ok(Json(TaskPresetResponse::from(updated)))
}

/// 执行任务预设
/// POST /api/agent/presets/{id}/execute
///
/// 直接执行已保存的预设，跳过意图分析步骤
pub async fn execute_preset(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(preset_id): Path<i32>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, (StatusCode, Json<Value>)> {
    let user_id = parse_user_id_with_agent_access(&claims, &db).await?;

    // 查找预设
    let preset = agent_task_presets::Entity::find_by_id(preset_id)
        .filter(agent_task_presets::Column::UserId.eq(user_id))
        .one(&db)
        .await
        .map_err(|e| {
            tracing::error!("[Agent Presets] Failed to find preset: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Database error" })),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "Preset not found" })),
            )
        })?;

    // 检查是否有保存的 recipe
    let mut recipe: crate::services::agent::types::Recipe = preset
        .parsed_steps
        .as_ref()
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "Preset has no saved recipe, please run the task first" })),
            )
        })?;

    // 🔑 重要：清除保存的 page_context，让步骤重新执行获取最新数据
    // 这确保 "获取最新文章 → AI总结" 这样的流程会获取当时的最新内容
    // 而不是使用保存时的旧数据
    recipe.page_context = None;

    tracing::info!(
        user_id = user_id,
        preset_id = preset_id,
        recipe_id = %recipe.id,
        "[Agent API] Executing preset with saved recipe"
    );

    // 更新使用时间
    let now = Utc::now().fixed_offset();
    let new_use_count = preset.use_count + 1;
    let mut active_model: agent_task_presets::ActiveModel = preset.into();
    active_model.last_used_at = Set(now);
    active_model.use_count = Set(new_use_count);
    let _ = active_model.update(&db).await;

    // 创建进度通道
    let (tx, rx) = tokio::sync::mpsc::channel::<ProgressEvent>(32);

    // 在后台执行任务
    let db_clone = db.clone();
    let queue = LANE_QUEUE.clone();
    let lane_key = LaneQueue::make_lane_key(user_id, None);
    tokio::spawn(async move {
        // 获取 Lane Queue 执行许可
        let _guard = match queue.acquire_timeout(&lane_key, std::time::Duration::from_secs(LaneQueue::DEFAULT_ACQUIRE_TIMEOUT_SECS)).await {
            Ok(guard) => guard,
            Err(e) => {
                let _ = tx
                    .send(ProgressEvent::Error {
                        task_id: None,
                        message: e,
                        code: "QUEUE_FULL".to_string(),
                    })
                    .await;
                return;
            }
        };

        let agent = crate::services::agent::Agent::new(db_clone).await;

        // TaskCreated 在 execute_saved_recipe 内 mint 新 run id 后发送，保证与 task_id 一致
        match agent
            .execute_saved_recipe(&recipe, user_id, tx.clone())
            .await
        {
            Ok(response) => {
                let api_response: ApiResponse = response.into();
                let task_id = api_response
                    .task
                    .as_ref()
                    .map(|t| t.task_id.clone())
                    .unwrap_or_default();
                let success = api_response.success;
                let response_value = serde_json::to_value(&api_response)
                    .unwrap_or_else(|_| json!({"error": "serialization failed"}));
                tracing::info!(
                    "[Agent API] Preset execution completed, sending TaskCompleted event"
                );
                let _ = tx
                    .send(AgentProgressEvent::TaskCompleted {
                        task_id,
                        success,
                        response: Box::new(response_value),
                    })
                    .await;
            }
            Err(e) => {
                tracing::error!(error = %e, "[Agent API] Preset execution failed");
                let _ = tx
                    .send(ProgressEvent::Error {
                        task_id: None,
                        message: e.clone(),
                        code: "EXECUTION_ERROR".to_string(),
                    })
                    .await;
            }
        }
    });

    // 将通道转换为 SSE 流
    let stream = tokio_stream::wrappers::ReceiverStream::new(rx).map(|event| {
        let data = serde_json::to_string(&event).unwrap_or_else(|_| "{}".to_string());
        Ok(Event::default().data(data))
    });

    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

// ============ 队列状态 ============

/// 获取 Lane Queue 状态
async fn queue_status() -> Json<Value> {
    let status = LANE_QUEUE.get_status().await;
    Json(json!({
        "total_lanes": status.total_lanes,
        "max_concurrent": status.max_concurrent,
        "available_permits": status.available_permits,
        "waiting": status.waiting,
    }))
}

// ============ Heartbeat ============

/// 获取所有 Heartbeat 任务状态
async fn heartbeat_tasks(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_current_admin(&claims, &db).await?;
    let manager = crate::services::agent::heartbeat::get_heartbeat().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "Heartbeat not initialized" })),
        )
    })?;

    let tasks = manager.get_tasks().await;
    Ok(Json(json!({ "tasks": tasks })))
}

/// 切换 Heartbeat 任务启用状态
async fn toggle_heartbeat(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(task_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_current_admin(&claims, &db).await?;
    let manager = crate::services::agent::heartbeat::get_heartbeat().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "Heartbeat not initialized" })),
        )
    })?;

    match manager.toggle_task(&task_id).await {
        Some(enabled) => Ok(Json(json!({ "task_id": task_id, "enabled": enabled }))),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("Task '{}' not found", task_id) })),
        )),
    }
}

/// 重新加载 Heartbeat 配置（HEARTBEAT.md 修改后调用）
async fn reload_heartbeat(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_current_admin(&claims, &db).await?;
    let manager = crate::services::agent::heartbeat::get_heartbeat().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "Heartbeat not initialized" })),
        )
    })?;

    manager.reload().await;
    let tasks = manager.get_tasks().await;
    Ok(Json(json!({ "reloaded": true, "task_count": tasks.len() })))
}

#[derive(Debug, Deserialize)]
struct UpdateHeartbeatBody {
    name: Option<String>,
    schedule: Option<String>,
    action: Option<String>,
    enabled: Option<bool>,
}

/// 更新 Heartbeat 任务（name / schedule / action / enabled）
async fn update_heartbeat(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(task_id): Path<String>,
    Json(body): Json<UpdateHeartbeatBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_current_admin(&claims, &db).await?;
    let manager = crate::services::agent::heartbeat::get_heartbeat().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "Heartbeat not initialized" })),
        )
    })?;

    match manager
        .update_task(
            &task_id,
            body.name,
            body.schedule,
            body.action,
            body.enabled,
        )
        .await
    {
        Ok(task) => Ok(Json(json!({ "task": task }))),
        Err(e) if e.contains("not found") => Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": e })),
        )),
        Err(e) => Err((StatusCode::BAD_REQUEST, Json(json!({ "error": e })))),
    }
}

#[derive(Debug, Deserialize)]
struct CreateHeartbeatBody {
    name: String,
    schedule: String,
    action: String,
    #[serde(default = "default_heartbeat_enabled")]
    enabled: bool,
    id: Option<String>,
}

fn default_heartbeat_enabled() -> bool {
    true
}

/// 创建 Heartbeat 任务
async fn create_heartbeat(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<CreateHeartbeatBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_current_admin(&claims, &db).await?;
    let manager = crate::services::agent::heartbeat::get_heartbeat().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "Heartbeat not initialized" })),
        )
    })?;

    match manager
        .add_task(
            body.id,
            body.name,
            body.schedule,
            body.action,
            body.enabled,
        )
        .await
    {
        Ok(task) => Ok(Json(json!({ "task": task }))),
        Err(e) if e.contains("already exists") => Err((
            StatusCode::CONFLICT,
            Json(json!({ "error": e })),
        )),
        Err(e) => Err((StatusCode::BAD_REQUEST, Json(json!({ "error": e })))),
    }
}

/// 删除 Heartbeat 任务
async fn delete_heartbeat(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(task_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_current_admin(&claims, &db).await?;
    let manager = crate::services::agent::heartbeat::get_heartbeat().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "Heartbeat not initialized" })),
        )
    })?;

    match manager.delete_task(&task_id).await {
        Ok(()) => Ok(Json(json!({ "deleted": true, "task_id": task_id }))),
        Err(e) if e.contains("not found") => Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": e })),
        )),
        Err(e) => Err((StatusCode::BAD_REQUEST, Json(json!({ "error": e })))),
    }
}

/// 热重载 MCP 配置（mcp_servers.json）
async fn reload_mcp(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_current_admin(&claims, &db).await?;
    match crate::services::agent::mcp::reload_mcp().await {
        Ok(()) => {
            let tools = if let Some(m) = crate::services::agent::mcp::get_mcp_manager() {
                m.list_tools().await.len()
            } else {
                0
            };
            Ok(Json(json!({ "reloaded": true, "tool_count": tools })))
        }
        Err(e) => Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": e })),
        )),
    }
}

/// MCP 服务器状态列表
async fn mcp_status(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_current_admin(&claims, &db).await?;
    let manager = crate::services::agent::mcp::get_mcp_manager().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "MCP manager not initialized" })),
        )
    })?;
    let servers = manager.list_server_status().await;
    let tools = manager.list_tools().await.len();
    Ok(Json(json!({ "servers": servers, "tool_count": tools })))
}

// ============ Skills & Memory ============

/// 获取可用技能列表
async fn list_skills() -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let registry = crate::services::agent::skill::get_skill_registry().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "Skill registry not initialized" })),
        )
    })?;

    let skills = registry.get_all().await;
    // 获取 skill stats（如果 SkillEvolution 已初始化）
    let stats_map = match crate::services::agent::skill_evolution::get_skill_evolution() {
        Some(evo) => evo.get_all_stats().await,
        None => std::collections::HashMap::new(),
    };

    let skills_json: Vec<Value> = skills
        .iter()
        .map(|s| {
            let (success_count, failure_count) = stats_map
                .get(&s.id)
                .map(|st| (st.success_count, st.failure_count))
                .unwrap_or((0, 0));
            json!({
                "id": s.id,
                "name": s.name,
                "description": s.description,
                "category": s.category,
                "origin": s.origin,
                "successCount": success_count,
                "failureCount": failure_count,
                "tierHint": s.tier_hint,
                "parameters": s.parameters,
            })
        })
        .collect();

    Ok(Json(json!({ "skills": skills_json })))
}

/// 获取记忆条目（当前用户）
async fn list_memories(
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let user_id = parse_user_id(&claims)?;
    let memory = crate::services::agent::memory::get_memory().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "Memory not initialized" })),
        )
    })?;

    let entries = memory.list_recent(50, user_id).await;
    let memories_json: Vec<Value> = entries
        .iter()
        .map(|e| {
            json!({
                "id": e.id,
                "memoryType": e.memory_type,
                "content": e.content,
                "source": e.source,
                "createdAt": e.created_at,
                "tier": e.tier,
                "importance": e.importance,
                "entities": e.entities,
                "relatedCapabilities": e.related_capabilities,
            })
        })
        .collect();

    Ok(Json(json!({ "memories": memories_json })))
}

/// 删除记忆条目（仅本人）
async fn delete_memory(
    Extension(claims): Extension<Claims>,
    Path(memory_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let user_id = parse_user_id(&claims)?;
    let memory = crate::services::agent::memory::get_memory().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "Memory not initialized" })),
        )
    })?;

    if memory.remove_memory(&memory_id, user_id).await {
        Ok(Json(json!({ "success": true })))
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Memory not found" })),
        ))
    }
}

/// 更新记忆条目（仅本人）
async fn update_memory(
    Extension(claims): Extension<Claims>,
    Path(memory_id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let user_id = parse_user_id(&claims)?;
    let content = body["content"].as_str().ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Missing field: content" })),
        )
    })?;

    let memory = crate::services::agent::memory::get_memory().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "Memory not initialized" })),
        )
    })?;

    if memory.update_memory(&memory_id, content, user_id).await {
        Ok(Json(json!({ "success": true })))
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Memory not found" })),
        ))
    }
}

/// 删除技能
async fn delete_skill(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(skill_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_current_admin(&claims, &db).await?;
    let evo = crate::services::agent::skill_evolution::get_skill_evolution().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "Skill evolution not initialized" })),
        )
    })?;

    evo.delete_skill(&skill_id)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))))?;

    Ok(Json(json!({ "success": true })))
}

/// 获取能力缺口报告
async fn list_capability_gaps(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_current_admin(&claims, &db).await?;
    let evo = crate::services::agent::skill_evolution::get_skill_evolution().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "Skill evolution not initialized" })),
        )
    })?;

    let gaps = evo.get_all_gaps().await;
    let significant_count = gaps.iter().filter(|g| g.confidence >= 0.7).count();

    Ok(Json(json!({
        "gaps": gaps,
        "total": gaps.len(),
        "significantCount": significant_count,
    })))
}

// ============ Multi-Agent Routing ============

/// 获取所有 Agent 配置信息
async fn list_agents() -> Json<Value> {
    let router = crate::services::agent::routing::get_router();
    let profiles: Vec<Value> = router
        .get_all_profiles()
        .iter()
        .map(|p| {
            json!({
                "id": p.id,
                "role": p.role,
                "description": p.description,
                "defaultTier": format!("{:?}", p.default_tier),
                "maxConcurrency": p.max_concurrency,
                "capabilityPrefixes": p.capability_prefixes,
            })
        })
        .collect();

    Json(json!({ "agents": profiles }))
}

// ============ Session Control (Steer / Interrupt) ============

/// 中断当前正在执行的任务并替换为新请求
async fn interrupt_session(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let user_id = parse_user_id(&claims)?;
    let new_input = body
        .get("input")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "Missing 'input' field" })),
            )
        })?
        .to_string();

    // 取消当前用户所有非终态任务（running / waiting / paused / pending）
    let agent = Agent::new(db.clone()).await;
    let tasks = agent.get_user_tasks(user_id).await;
    let mut cancelled_count = 0;
    for task in &tasks {
        if crate::services::agent::executor::task_store::is_cancellable_task_status(&task.status)
            && agent.cancel_task_for_user(&task.task_id, user_id).await
        {
            cancelled_count += 1;
            // 立即唤醒 wait-loop，避免通知/run 仍卡在 waiting
            if let Some(waiting) = take_waiting_task(&task.task_id, user_id).await {
                let _ = waiting.done_tx.send(json!({
                    "success": false,
                    "responseType": "error",
                    "message": "任务已取消",
                    "streamTerminal": true,
                    "task": {
                        "taskId": task.task_id,
                        "status": "cancelled",
                        "progress": 0
                    }
                }));
            }
        }
    }

    // 提交新请求（通过 LaneQueue 保护并发）
    let lane_key = LaneQueue::make_lane_key(user_id, None);
    let _guard = LANE_QUEUE
        .acquire_timeout(&lane_key, std::time::Duration::from_secs(LaneQueue::DEFAULT_ACQUIRE_TIMEOUT_SECS))
        .await
        .map_err(|e| (StatusCode::SERVICE_UNAVAILABLE, Json(json!({ "error": e }))))?;

    let request = crate::services::agent::UserRequest {
        raw_input: new_input.clone(),
        timestamp: chrono::Utc::now(),
        user_id,
        context: None,
    };

    let new_agent = Agent::new(db).await;
    match new_agent.process(request).await {
        Ok(response) => Ok(Json(json!({
            "success": true,
            "cancelled_tasks": cancelled_count,
            "response": response,
        }))),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e })),
        )),
    }
}

/// 向当前会话注入补充指令（转向）
async fn steer_session(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let user_id = parse_user_id_with_agent_access(&claims, &db).await?;
    let instruction = body
        .get("instruction")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "Missing 'instruction' field" })),
            )
        })?;

    // 校验指令长度（复用 validate_input 的上限逻辑）
    if instruction.is_empty() || instruction.len() > MAX_INPUT_LEN {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Instruction must be non-empty and within length limits" })),
        ));
    }

    let requested_task_id = body.get("taskId").and_then(Value::as_str);
    let running_tasks: Vec<_> = crate::services::agent::executor::get_user_tasks(user_id)
        .await
        .into_iter()
        .filter(|task| task.status == crate::services::agent::types::TaskStatus::Running)
        .collect();
    let task_id = if let Some(requested) = requested_task_id {
        let task = crate::services::agent::executor::get_task_for_user(requested, user_id)
            .await
            .filter(|task| task.status == crate::services::agent::types::TaskStatus::Running)
            .ok_or_else(|| {
                (
                    StatusCode::NOT_FOUND,
                    Json(json!({ "error": "Running task not found" })),
                )
            })?;
        task.task_id
    } else {
        match running_tasks.as_slice() {
            [task] => task.task_id.clone(),
            [] => {
                return Err((
                    StatusCode::CONFLICT,
                    Json(json!({ "error": "No running task to steer" })),
                ));
            }
            _ => {
                return Err((
                    StatusCode::CONFLICT,
                    Json(json!({ "error": "Multiple tasks are running; taskId is required" })),
                ));
            }
        }
    };

    crate::services::agent::executor::enqueue_steering(&db, &task_id, instruction.to_string())
        .await
        .map_err(|error| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": error, "code": "steering_unavailable" })),
            )
        })?;

    // Keep an audit/session trace after the instruction is accepted for execution.
    if let Some(mem) = crate::services::agent::memory::get_memory() {
        mem.remember(
            &format!("用户中途转向指令: {}", instruction),
            crate::services::agent::memory::MemoryType::SessionInsight,
            user_id,
        )
        .await;
    }

    Ok(Json(json!({
        "success": true,
        "message": "Steering instruction queued for the next step boundary",
        "taskId": task_id,
        "queued": true,
        "instruction": instruction,
    })))
}

// ============ 会话管理 API ============

/// 创建会话
/// POST /api/agent/sessions
pub async fn create_session(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let user_id = parse_user_id(&claims)?;
    let session_id = format!("ses_{}", uuid::Uuid::new_v4().simple());
    let now = Utc::now().fixed_offset();

    let session = agent_sessions::ActiveModel {
        id: Set(session_id.clone()),
        user_id: Set(user_id),
        title: Set(None),
        context: Set(None),
        message_count: Set(0),
        archived: Set(false),
        created_at: Set(now),
        last_active_at: Set(now),
    };

    agent_sessions::Entity::insert(session)
        .exec(&db)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("Failed to create session: {}", e)})),
            )
        })?;

    Ok(Json(json!({
        "id": session_id,
        "title": null,
        "messageCount": 0,
        "lastActiveAt": now.to_rfc3339(),
    })))
}

/// 会话列表查询参数
#[derive(Debug, Deserialize)]
pub struct SessionListQuery {
    #[serde(default = "default_page")]
    pub page: u64,
    #[serde(default = "default_limit")]
    pub limit: u64,
}

fn default_page() -> u64 {
    1
}
fn default_limit() -> u64 {
    20
}

/// 列出最近会话
/// GET /api/agent/sessions
pub async fn list_sessions(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Query(query): Query<SessionListQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let user_id = parse_user_id(&claims)?;

    let sessions = agent_sessions::Entity::find()
        .filter(agent_sessions::Column::UserId.eq(user_id))
        .filter(agent_sessions::Column::Archived.eq(false))
        .order_by_desc(agent_sessions::Column::LastActiveAt)
        .paginate(&db, query.limit)
        .fetch_page(query.page.saturating_sub(1))
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("Failed to list sessions: {}", e)})),
            )
        })?;

    let sessions_json: Vec<Value> = sessions
        .into_iter()
        .map(|s| {
            json!({
                "id": s.id,
                "title": s.title,
                "messageCount": s.message_count,
                "lastActiveAt": s.last_active_at.to_rfc3339(),
                "createdAt": s.created_at.to_rfc3339(),
            })
        })
        .collect();

    Ok(Json(json!({ "sessions": sessions_json })))
}

/// 获取会话消息
/// GET /api/agent/sessions/:id/messages
pub async fn get_session_messages(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(session_id): Path<String>,
    Query(query): Query<SessionListQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let user_id = parse_user_id(&claims)?;

    // 验证会话归属
    let session = agent_sessions::Entity::find_by_id(&session_id)
        .filter(agent_sessions::Column::UserId.eq(user_id))
        .one(&db)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("DB error: {}", e)})),
            )
        })?;

    if session.is_none() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "Session not found"})),
        ));
    }

    let messages = agent_messages::Entity::find()
        .filter(agent_messages::Column::SessionId.eq(&session_id))
        .order_by_asc(agent_messages::Column::CreatedAt)
        .paginate(&db, query.limit)
        .fetch_page(query.page.saturating_sub(1))
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("Failed to list messages: {}", e)})),
            )
        })?;

    let messages_json: Vec<Value> = messages
        .into_iter()
        .map(|m| {
            json!({
                "id": m.id,
                "sessionId": m.session_id,
                "taskId": m.task_id,
                "role": m.role,
                "content": m.content,
                "metadata": m.metadata,
                "createdAt": m.created_at.to_rfc3339(),
            })
        })
        .collect();

    Ok(Json(json!({ "messages": messages_json })))
}

/// 归档会话
/// DELETE /api/agent/sessions/:id
pub async fn archive_session(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(session_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let user_id = parse_user_id(&claims)?;

    let session = agent_sessions::Entity::find_by_id(&session_id)
        .filter(agent_sessions::Column::UserId.eq(user_id))
        .one(&db)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("DB error: {}", e)})),
            )
        })?;

    if session.is_none() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "Session not found"})),
        ));
    }

    let mut active: agent_sessions::ActiveModel = session.unwrap().into();
    active.archived = Set(true);
    active.update(&db).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("Failed to archive session: {}", e)})),
        )
    })?;

    Ok(Json(json!({"success": true})))
}

/// 更新会话标题请求
#[derive(Debug, Deserialize)]
pub struct UpdateSessionRequest {
    pub title: Option<String>,
}

/// 更新会话标题
/// PATCH /api/agent/sessions/:id
pub async fn update_session(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(session_id): Path<String>,
    Json(req): Json<UpdateSessionRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let user_id = parse_user_id(&claims)?;

    let session = agent_sessions::Entity::find_by_id(&session_id)
        .filter(agent_sessions::Column::UserId.eq(user_id))
        .one(&db)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("DB error: {}", e)})),
            )
        })?;

    if session.is_none() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "Session not found"})),
        ));
    }

    let mut active: agent_sessions::ActiveModel = session.unwrap().into();
    if let Some(title) = req.title {
        active.title = Set(Some(title));
    }
    let updated = active.update(&db).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("Failed to update session: {}", e)})),
        )
    })?;

    Ok(Json(json!({
        "id": updated.id,
        "title": updated.title,
        "messageCount": updated.message_count,
        "lastActiveAt": updated.last_active_at.to_rfc3339(),
    })))
}

/// AI 生成会话标题
/// POST /api/agent/sessions/:id/generate-title
pub async fn generate_session_title(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(session_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let user_id = parse_user_id(&claims)?;

    // 验证会话属于当前用户
    let session = agent_sessions::Entity::find_by_id(&session_id)
        .filter(agent_sessions::Column::UserId.eq(user_id))
        .one(&db)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("DB error: {}", e)})),
            )
        })?;

    if session.is_none() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "Session not found"})),
        ));
    }

    // 加载最近几条消息作为标题生成上下文
    let messages = agent_messages::Entity::find()
        .filter(agent_messages::Column::SessionId.eq(&session_id))
        .order_by_asc(agent_messages::Column::CreatedAt)
        .paginate(&db, 4)
        .fetch_page(0)
        .await
        .unwrap_or_default();

    if messages.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "No messages in session"})),
        ));
    }

    let context: String = messages
        .iter()
        .map(|m| {
            format!(
                "{}: {}",
                m.role,
                m.content.chars().take(200).collect::<String>()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    // 调用 AI 生成标题
    let analyzer =
        crate::services::ai::create_ai_analyzer_for_tier(crate::config::ModelTier::Standard).await;

    let title = if let Some(analyzer) = analyzer {
        let prompt = format!(
            "Based on the following conversation, generate a concise session title (5-15 characters, in the same language as the user). \
             Return ONLY the title text, no quotes, no explanation.\n\n{}",
            context
        );
        match analyzer.analyze(&prompt).await {
            Ok(raw) => {
                // 清理：去掉首尾引号、多余空白
                let cleaned = raw
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\u{300c}')
                    .trim_matches('\u{300d}')
                    .trim();
                if cleaned.is_empty() || cleaned.len() > 100 {
                    fallback_title(&messages)
                } else {
                    cleaned.to_string()
                }
            }
            Err(e) => {
                tracing::warn!("[Agent API] AI title generation failed: {}", e);
                fallback_title(&messages)
            }
        }
    } else {
        fallback_title(&messages)
    };

    // 更新数据库
    let mut active: agent_sessions::ActiveModel = session.unwrap().into();
    active.title = Set(Some(title.clone()));
    let _ = active.update(&db).await;

    Ok(Json(json!({ "title": title })))
}

/// 标题降级：截取第一条用户消息
fn fallback_title(messages: &[agent_messages::Model]) -> String {
    messages
        .iter()
        .find(|m| m.role == "user")
        .map(|m| {
            let s: String = m.content.chars().take(47).collect();
            if m.content.chars().count() > 50 {
                format!("{}...", s)
            } else {
                s
            }
        })
        .unwrap_or_else(|| "New conversation".to_string())
}

/// 会话消息持久化辅助函数
async fn persist_user_message(
    db: &DatabaseConnection,
    session_id: &str,
    content: &str,
) -> Result<(), String> {
    let now = Utc::now().fixed_offset();
    let msg = agent_messages::ActiveModel {
        id: sea_orm::ActiveValue::NotSet,
        session_id: Set(session_id.to_string()),
        task_id: Set(None),
        role: Set("user".to_string()),
        content: Set(content.to_string()),
        metadata: Set(None),
        created_at: Set(now),
    };
    agent_messages::Entity::insert(msg)
        .exec(db)
        .await
        .map_err(|e| format!("Failed to persist user message: {}", e))?;
    Ok(())
}

async fn persist_assistant_message(
    db: &DatabaseConnection,
    session_id: &str,
    task_id: Option<&str>,
    content: &str,
    metadata: Option<Value>,
) -> Result<(), String> {
    let now = Utc::now().fixed_offset();
    let msg = agent_messages::ActiveModel {
        id: sea_orm::ActiveValue::NotSet,
        session_id: Set(session_id.to_string()),
        task_id: Set(task_id.map(|s| s.to_string())),
        role: Set("assistant".to_string()),
        content: Set(content.to_string()),
        metadata: Set(metadata),
        created_at: Set(now),
    };
    agent_messages::Entity::insert(msg)
        .exec(db)
        .await
        .map_err(|e| format!("Failed to persist assistant message: {}", e))?;

    // 更新会话消息计数和最后活跃时间
    if let Ok(Some(session)) = agent_sessions::Entity::find_by_id(session_id).one(db).await {
        let mut active: agent_sessions::ActiveModel = session.into();
        active.last_active_at = Set(now);
        // message_count 用 raw SQL 更新可能更好，但这里简单处理
        if let Ok(count) = agent_messages::Entity::find()
            .filter(agent_messages::Column::SessionId.eq(session_id))
            .count(db)
            .await
        {
            active.message_count = Set(count as i32);
        }
        let _ = active.update(db).await;
    }

    Ok(())
}

/// 加载会话历史消息作为对话上下文
async fn load_session_history(
    db: &DatabaseConnection,
    session_id: &str,
    max_messages: u64,
) -> Vec<crate::services::agent::ConversationMessage> {
    use crate::services::agent::ConversationMessage;

    let messages = agent_messages::Entity::find()
        .filter(agent_messages::Column::SessionId.eq(session_id))
        .order_by_desc(agent_messages::Column::CreatedAt)
        .paginate(db, max_messages)
        .fetch_page(0)
        .await
        .unwrap_or_default();

    // 反转为时间正序，assistant 消息附带 metadata 摘要
    messages
        .into_iter()
        .rev()
        .map(|m| {
            let mut content = m.content.clone();
            // 将 metadata 中的关键信息追加到 assistant 内容，让 planner 了解上轮输出
            if m.role == "assistant" {
                if let Some(ref meta) = m.metadata {
                    let mut extras = Vec::new();
                    if let Some(data) = meta.get("data") {
                        if !data.is_null() {
                            // 截取摘要，避免过长
                            let s = data.to_string();
                            if s.len() > 2 && s != "null" {
                                let truncated: String = s.chars().take(500).collect();
                                extras.push(format!("[输出数据: {}]", truncated));
                            }
                        }
                    }
                    if let Some(dd) = meta.get("dataDisplay") {
                        if let Some(display_type) = dd.get("type").and_then(|v| v.as_str()) {
                            extras.push(format!("[展示类型: {}]", display_type));
                        }
                    }
                    if let Some(fa) = meta.get("frontendAction") {
                        if let Some(action) = fa.get("action").and_then(|v| v.as_str()) {
                            extras.push(format!("[前端动作: {}]", action));
                        }
                    }
                    if !extras.is_empty() {
                        content.push_str(&format!("\n{}", extras.join(" ")));
                    }
                }
            }
            ConversationMessage {
                role: m.role,
                content,
                created_at: Some(m.created_at.to_rfc3339()),
            }
        })
        .collect()
}

/// 确保会话存在，如果 session_id 为 None 则自动创建
async fn ensure_session(
    db: &DatabaseConnection,
    session_id: Option<&str>,
    user_id: i32,
) -> Result<String, String> {
    if let Some(sid) = session_id {
        // 验证会话存在且属于当前用户
        if agent_sessions::Entity::find_by_id(sid)
            .filter(agent_sessions::Column::UserId.eq(user_id))
            .one(db)
            .await
            .map_err(|e| format!("DB error: {}", e))?
            .is_some()
        {
            return Ok(sid.to_string());
        }
    }

    // 自动创建新会话
    let new_id = format!("ses_{}", uuid::Uuid::new_v4().simple());
    let now = Utc::now().fixed_offset();
    let session = agent_sessions::ActiveModel {
        id: Set(new_id.clone()),
        user_id: Set(user_id),
        title: Set(None),
        context: Set(None),
        message_count: Set(0),
        archived: Set(false),
        created_at: Set(now),
        last_active_at: Set(now),
    };
    agent_sessions::Entity::insert(session)
        .exec(db)
        .await
        .map_err(|e| format!("Failed to create session: {}", e))?;

    Ok(new_id)
}

// ============ 通知 API ============

/// 通知 SSE 流
async fn notification_stream(
    Extension(claims): Extension<Claims>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, (StatusCode, Json<Value>)> {
    let user_id = parse_user_id(&claims)?;
    let manager = crate::services::agent::notifications::get_notification_manager().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({"error": "Notification system not initialized"})),
    ))?;

    let mut rx = manager.subscribe();
    let (tx, mpsc_rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(32);

    // 发送初始未读计数
    let unread = manager.unread_count_for_user(user_id).await;
    let init_data = json!({"event": "init", "unread_count": unread});
    let _ = tx
        .send(Ok(
            Event::default().data(serde_json::to_string(&init_data).unwrap_or_default())
        ))
        .await;

    // 后台转发 broadcast → mpsc（过滤非当前用户的通知）
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let should_send =
                        crate::services::agent::notifications::event_is_for_user(&event, user_id);
                    if should_send {
                        let data = serde_json::to_string(&event).unwrap_or_default();
                        if tx.send(Ok(Event::default().data(data))).await.is_err() {
                            break;
                        }
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!("Notification stream lagged by {} messages", n);
                    // 告知客户端丢事件，前端应重新 list() 补全历史
                    let resync =
                        crate::services::agent::notifications::NotificationEvent::Resync {
                            lagged_by: n,
                        };
                    let data = serde_json::to_string(&resync).unwrap_or_default();
                    if tx.send(Ok(Event::default().data(data))).await.is_err() {
                        break;
                    }
                    continue;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    let stream = tokio_stream::wrappers::ReceiverStream::new(mpsc_rx);
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(30))))
}

/// 获取历史通知
async fn list_notifications(
    Extension(claims): Extension<Claims>,
    Query(params): Query<NotificationListParams>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let user_id = parse_user_id(&claims)?;
    let manager = crate::services::agent::notifications::get_notification_manager().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({"error": "Notification system not initialized"})),
    ))?;

    let limit = params.limit.unwrap_or(50).min(200);
    let notifications = manager.get_history_for_user(user_id, limit).await;
    let unread = manager.unread_count_for_user(user_id).await;
    let total = manager.total_count_for_user(user_id).await;

    Ok(Json(json!({
        "notifications": notifications,
        "unread_count": unread,
        "total": total,
    })))
}

/// 标记通知已读
async fn mark_notification_read(
    Extension(claims): Extension<Claims>,
    Path(notification_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let user_id = parse_user_id(&claims)?;
    let manager = crate::services::agent::notifications::get_notification_manager().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({"error": "Notification system not initialized"})),
    ))?;

    let found = manager
        .mark_read(&notification_id, user_id)
        .await
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": error})),
            )
        })?;
    Ok(Json(json!({"success": found})))
}

/// 标记全部已读
async fn mark_all_notifications_read(
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let user_id = parse_user_id(&claims)?;
    let manager = crate::services::agent::notifications::get_notification_manager().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({"error": "Notification system not initialized"})),
    ))?;

    manager.mark_all_read(user_id).await.map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": error})),
        )
    })?;
    Ok(Json(json!({"success": true})))
}

/// 删除单条通知
async fn delete_notification(
    Extension(claims): Extension<Claims>,
    Path(notification_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let user_id = parse_user_id(&claims)?;
    let manager = crate::services::agent::notifications::get_notification_manager().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({"error": "Notification system not initialized"})),
    ))?;

    let removed = manager
        .delete_notification(&notification_id, user_id)
        .await
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": error})),
            )
        })?;
    if !removed {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "Notification not found"})),
        ));
    }
    Ok(Json(json!({"success": true})))
}

/// 清空全部通知
async fn clear_notifications(
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let user_id = parse_user_id(&claims)?;
    let manager = crate::services::agent::notifications::get_notification_manager().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({"error": "Notification system not initialized"})),
    ))?;

    let deleted = manager.clear_all(user_id).await.map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": error})),
        )
    })?;
    Ok(Json(json!({"success": true, "deleted": deleted})))
}

#[derive(Deserialize)]
struct NotificationListParams {
    limit: Option<usize>,
}

// ============ Boot recovery for waiting tasks ============

/// After process restart, re-create run hubs and wait-loops for
/// `waiting_for_input` tasks so answer/subscribe keep working and notifications
/// stay consistent. Called once from `main` after `init_task_store`.
pub async fn restore_waiting_runs_after_boot() {
    let waiting =
        crate::services::agent::executor::task_store::list_waiting_tasks_snapshot().await;
    if waiting.is_empty() {
        tracing::info!("[Agent API] Boot restore: no waiting_for_input tasks");
        return;
    }

    tracing::info!(
        count = waiting.len(),
        "[Agent API] Boot restore: re-creating run hubs for waiting tasks"
    );

    for (user_id, mut task) in waiting {
        // Drop already-expired questions immediately so they don't block forever.
        if task
            .pending_question
            .as_ref()
            .is_some_and(|q| q.is_expired(chrono::Utc::now()))
        {
            tracing::warn!(
                task_id = %task.task_id,
                "[Agent API] Boot restore: expiring abandoned waiting question"
            );
            task.status = crate::services::agent::types::TaskStatus::Failed;
            task.error = Some("等待用户输入已超时（服务重启后发现已过期）".into());
            task.completed_at = Some(chrono::Utc::now());
            task.pending_question = None;
            {
                let mut store = crate::services::agent::executor::TASK_STORE.write().await;
                store.store(user_id, task.clone());
            }
            crate::services::agent::executor::persist_task_async(user_id, task);
            continue;
        }

        let session_id = crate::services::agent::executor::task_store::session_id_from_lane_id(
            task.lane_id.as_deref(),
        );
        let run = create_run(user_id, session_id.clone()).await;
        let run_id = run.run_id().to_string();
        let task_id = task.task_id.clone();

        let (tx, mut rx) = tokio::sync::mpsc::channel::<AgentProgressEvent>(32);
        let run_for_forwarder = run.clone();
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                run_for_forwarder.publish(event).await;
            }
        });

        // Surface waiting state so re-subscribers get a usable snapshot.
        if let Some(ref q) = task.pending_question {
            let q_type = serde_json::to_value(&q.question_type)
                .ok()
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_else(|| "free_text".to_string());
            let options = q.options.as_ref().map(|opts| {
                opts.iter()
                    .map(|o| crate::services::agent::types::QuestionOptionCompact {
                        value: o.value.clone(),
                        label: o.label.clone(),
                        description: o.description.clone(),
                    })
                    .collect()
            });
            let _ = tx
                .send(AgentProgressEvent::TaskCreated {
                    task_id: task_id.clone(),
                    message: q.question.clone(),
                    total_steps: 1,
                    step_descriptions: Vec::new(),
                })
                .await;
            let _ = tx
                .send(AgentProgressEvent::WaitingForInput {
                    task_id: task_id.clone(),
                    question_id: q.question_id.clone(),
                    question_type: q_type,
                    question: q.question.clone(),
                    context: if q.context.is_empty() {
                        None
                    } else {
                        Some(q.context.clone())
                    },
                    options,
                    required: q.required,
                    default_value: q.default_value.clone(),
                })
                .await;
        }

        let session_id_loop = session_id.unwrap_or_default();
        tokio::spawn(async move {
            spawn_restored_wait_loop(user_id, task_id, session_id_loop, run_id, tx).await;
        });
    }
}

/// Lightweight wait-loop for boot-restored tasks (same terminal guarantees as process_stream).
async fn spawn_restored_wait_loop(
    user_id: i32,
    task_id: String,
    session_id: String,
    run_id: String,
    tx: tokio::sync::mpsc::Sender<AgentProgressEvent>,
) {
    loop {
        let (done_tx, done_rx) = tokio::sync::oneshot::channel::<serde_json::Value>();
        {
            let mut map = WAITING_TASKS.write().await;
            map.insert(
                task_id.clone(),
                WaitingTaskCtx {
                    user_id,
                    progress_tx: tx.clone(),
                    done_tx,
                    session_id: session_id.clone(),
                },
            );
        }
        tracing::info!(
            task_id = %task_id,
            run_id = %run_id,
            "[Agent API] Restored wait-loop registered"
        );

        match tokio::time::timeout(tokio::time::Duration::from_secs(2), done_rx).await {
            Ok(Ok(response_value)) => {
                let still_waiting = response_value
                    .pointer("/task/status")
                    .and_then(|s| s.as_str())
                    == Some("waiting_for_input");
                if still_waiting {
                    continue;
                }
                let task_success = response_value
                    .get("success")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                let _ = tx
                    .send(AgentProgressEvent::TaskCompleted {
                        task_id: task_id.clone(),
                        success: task_success,
                        response: Box::new(response_value),
                    })
                    .await;
                break;
            }
            Ok(Err(_)) => {
                let _ = take_waiting_task(&task_id, user_id).await;
                let _ = tx
                    .send(wait_loop_channel_dropped_event(&task_id))
                    .await;
                break;
            }
            Err(_) => {
                let _ = take_waiting_task(&task_id, user_id).await;
                let current_task =
                    crate::services::agent::executor::refresh_task_for_user(&task_id, user_id)
                        .await;

                if let Some(task) = current_task.as_ref() {
                    if task.status
                        == crate::services::agent::types::TaskStatus::WaitingForInput
                        && task
                            .pending_question
                            .as_ref()
                            .is_some_and(|q| q.is_expired(chrono::Utc::now()))
                    {
                        let response_value = json!({
                            "success": false,
                            "message": "等待用户输入已超时",
                            "streamTerminal": true,
                            "task": { "taskId": task_id, "status": "failed" }
                        });
                        let _ = tx
                            .send(AgentProgressEvent::TaskCompleted {
                                task_id: task_id.clone(),
                                success: false,
                                response: Box::new(response_value),
                            })
                            .await;
                        if let Some(mut t) =
                            crate::services::agent::executor::get_task_for_user(&task_id, user_id)
                                .await
                        {
                            t.status = crate::services::agent::types::TaskStatus::Failed;
                            t.error = Some("等待用户输入已超时".into());
                            t.completed_at = Some(chrono::Utc::now());
                            t.pending_question = None;
                            {
                                let mut store =
                                    crate::services::agent::executor::TASK_STORE.write().await;
                                store.store(user_id, t.clone());
                            }
                            crate::services::agent::executor::persist_task_async(user_id, t);
                        }
                        break;
                    }
                }

                if current_task.as_ref().is_some_and(|task| {
                    matches!(
                        task.status,
                        crate::services::agent::types::TaskStatus::Pending
                            | crate::services::agent::types::TaskStatus::Running
                            | crate::services::agent::types::TaskStatus::WaitingForInput
                            | crate::services::agent::types::TaskStatus::Paused
                    )
                }) {
                    continue;
                }

                let (response_value, task_success) = if let Some(task) = current_task {
                    let task_success =
                        task.status == crate::services::agent::types::TaskStatus::Completed;
                    let message = task.error.clone().unwrap_or_else(|| {
                        if task_success {
                            "任务已完成".into()
                        } else {
                            "任务未完成".into()
                        }
                    });
                    (
                        json!({ "success": task_success, "message": message, "task": task }),
                        task_success,
                    )
                } else {
                    (
                        json!({
                            "success": false,
                            "message": "任务状态已不可用",
                            "task": { "taskId": task_id, "status": "failed" }
                        }),
                        false,
                    )
                };
                let _ = tx
                    .send(AgentProgressEvent::TaskCompleted {
                        task_id: task_id.clone(),
                        success: task_success,
                        response: Box::new(response_value),
                    })
                    .await;
                break;
            }
        }
    }
}

// ============ 路由构建 ============

use axum::routing::{delete, get, post, put};
use axum::Router;

/// 创建 Agent API 路由
pub fn create_agent_routes() -> Router<DatabaseConnection> {
    use crate::middleware;
    use axum::middleware::from_fn;

    Router::new()
        // 健康检查（公开）
        .route("/health", get(health))
        // 队列状态（需要认证）
        .route(
            "/queue/status",
            get(queue_status).route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        // Heartbeat 任务列表 / 创建（需要认证 + admin）
        .route(
            "/heartbeat",
            get(heartbeat_tasks)
                .post(create_heartbeat)
                .route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        // 切换 Heartbeat 任务启用状态（需要认证）
        .route(
            "/heartbeat/{task_id}/toggle",
            post(toggle_heartbeat).route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        // 更新 / 删除 Heartbeat 任务（需要认证）
        .route(
            "/heartbeat/{task_id}",
            put(update_heartbeat)
                .delete(delete_heartbeat)
                .route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        // 重新加载 Heartbeat 配置（需要认证）
        .route(
            "/heartbeat/reload",
            post(reload_heartbeat).route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        // 热重载 MCP 配置（需要认证）
        .route(
            "/mcp/reload",
            post(reload_mcp).route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        // MCP 服务器状态（需要认证）
        .route(
            "/mcp/status",
            get(mcp_status).route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        // Agent 列表（需要认证）
        .route(
            "/agents",
            get(list_agents).route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        // 能力列表（需要认证）
        .route(
            "/capabilities",
            get(list_capabilities).route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        // 处理自然语言请求（需要认证）
        .route(
            "/process",
            post(process).route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        // 流式处理请求（带实时进度更新）
        .route(
            "/process/stream",
            post(process_stream).route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        // Agent run 状态订阅：GET 可由前端在刷新/断线后安全重连
        .route(
            "/runs/{run_id}/stream",
            get(subscribe_run_stream).route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        // 提供澄清（需要认证）
        .route(
            "/clarify",
            post(clarify).route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        // 确认敏感操作（需要认证）
        .route(
            "/confirm",
            post(confirm_operation).route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        .route(
            "/confirm/stream",
            post(confirm_operation_stream).route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        // 执行追踪列表（需要认证）
        .route(
            "/traces",
            get(list_traces).route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        // 任务列表（需要认证）
        .route(
            "/tasks",
            get(list_tasks).route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        // 任务详情（需要认证）
        .route(
            "/tasks/{task_id}",
            get(get_task).route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        // 取消任务（需要认证）
        .route(
            "/tasks/{task_id}/cancel",
            post(cancel_task).route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        // 回答任务问题（需要认证）
        .route(
            "/tasks/{task_id}/answer",
            post(answer_task_question).route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        // 回答任务问题 SSE 流式（需要认证）
        .route(
            "/tasks/{task_id}/answer/stream",
            post(answer_task_question_stream)
                .route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        // 中断当前会话并替换为新请求（需要认证）
        .route(
            "/session/interrupt",
            post(interrupt_session).route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        // 向当前会话注入转向指令（需要认证）
        .route(
            "/session/steer",
            post(steer_session).route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        // ============ 会话管理路由 ============
        // 创建会话
        .route(
            "/sessions",
            post(create_session).route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        // 列出会话
        .route(
            "/sessions",
            get(list_sessions).route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        // 获取会话消息
        .route(
            "/sessions/{session_id}/messages",
            get(get_session_messages).route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        // 归档会话
        .route(
            "/sessions/{session_id}",
            axum::routing::delete(archive_session)
                .route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        // 更新会话标题
        .route(
            "/sessions/{session_id}",
            axum::routing::patch(update_session)
                .route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        // AI 生成会话标题
        .route(
            "/sessions/{session_id}/generate-title",
            post(generate_session_title).route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        // ============ 任务预设路由 ============
        // 预设列表（需要认证）
        .route(
            "/presets",
            get(list_presets).route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        // 创建预设（需要认证）
        .route(
            "/presets",
            post(create_preset).route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        // 删除预设（需要认证）
        .route(
            "/presets/{preset_id}",
            axum::routing::delete(delete_preset)
                .route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        // 切换收藏状态（需要认证）
        .route(
            "/presets/{preset_id}/toggle-favorite",
            post(toggle_favorite).route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        // 更新使用时间（需要认证）
        .route(
            "/presets/{preset_id}/use",
            post(use_preset).route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        // 执行预设（直接执行已保存的 recipe，跳过意图分析）
        .route(
            "/presets/{preset_id}/execute",
            post(execute_preset).route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        // ============ 技能/记忆路由 ============
        // 技能列表（需要认证）
        .route(
            "/skills",
            get(list_skills).route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        // 删除技能（需要认证）
        .route(
            "/skills/{skill_id}",
            delete(delete_skill).route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        // 记忆列表（需要认证）
        .route(
            "/memory",
            get(list_memories).route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        // 删除记忆（需要认证）
        .route(
            "/memory/{memory_id}",
            delete(delete_memory)
                .put(update_memory)
                .route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        // 能力缺口报告（需要认证）
        .route(
            "/gaps",
            get(list_capability_gaps).route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        // ============ 通知路由 ============
        // 通知 SSE 流
        .route(
            "/notifications/stream",
            get(notification_stream).route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        // 当前用户通知偏好与事件目录
        .route(
            "/notifications/preferences",
            get(crate::api::notification_preferences::get_notification_preferences)
                .put(crate::api::notification_preferences::update_notification_preferences)
                .route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        // 获取历史通知
        .route(
            "/notifications",
            get(list_notifications).route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        // 标记通知已读
        .route(
            "/notifications/{notification_id}/read",
            post(mark_notification_read).route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        // 标记全部已读
        .route(
            "/notifications/read-all",
            post(mark_all_notifications_read)
                .route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        // 删除单条通知
        .route(
            "/notifications/{notification_id}",
            delete(delete_notification).route_layer(from_fn(middleware::auth::auth_middleware)),
        )
        // 清空全部通知
        .route(
            "/notifications/clear",
            post(clear_notifications).route_layer(from_fn(middleware::auth::auth_middleware)),
        )
}
