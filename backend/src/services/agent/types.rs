//! Agent 请求、意图与 Planner 类型。

use crate::config::ModelTier;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// 一轮输入进入哪条运行时路径。
///
/// Work 保留完整 Planner / Executor；Chat 只是人设对话，不得因为
/// 内容像指令就悄悄进入工具执行。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentInteractionMode {
    #[default]
    Work,
    Chat,
}

impl AgentInteractionMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Work => "work",
            Self::Chat => "chat",
        }
    }
}

/// 用户原始请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserRequest {
    /// 原始自然语言输入
    pub raw_input: String,
    /// 请求时间
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// 用户 ID
    pub user_id: i32,
    /// 可选的上下文数据
    pub context: Option<RequestContext>,
}

/// 请求上下文
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RequestContext {
    /// 运行时路径。缺省 Work（`#[serde(default)]`）。
    #[serde(default)]
    pub interaction_mode: AgentInteractionMode,
    /// 当前页面/路由
    pub current_route: Option<String>,
    /// 活跃的平台
    #[serde(default)]
    pub active_platforms: Vec<String>,
    /// 用户偏好
    pub preferences: Option<Value>,
    /// 会话 ID（用于多轮对话）
    pub session_id: Option<String>,
    /// 对话历史（用于继续对话模式）
    pub conversation_history: Option<Vec<ConversationMessage>>,
    /// 自定义数据（如当前阅读的文章内容）
    pub custom_data: Option<Value>,
    /// Lane key（由 LaneQueue 分配，用于队列追踪）
    #[serde(default)]
    pub lane_key: Option<String>,
    /// 当前后端 run id（确认续跑时复用同一 run，避免通知身份漂移）
    #[serde(default)]
    pub run_id: Option<String>,
    /// 本轮 Work 的提案 id（用户接单或意识引擎接单都写）。
    #[serde(default)]
    pub source_intent_id: Option<String>,
    /// Extra ceiling for autonomy-accepted Work. Intersected with current
    /// granted permissions at execute time. Never a secret.
    #[serde(default)]
    pub autonomy_permission_cap: Option<Vec<String>>,
    /// Semantic live-face snapshot from the client. Event-scoped, never a driver.
    #[serde(default)]
    pub rig_state: Option<myriad_merope::RigStateSummary>,
}

/// 对话消息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMessage {
    /// 角色: user, assistant, system
    pub role: String,
    /// 消息内容
    pub content: String,
    #[serde(default)]
    pub created_at: Option<String>,
}

/// 意图动作类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum IntentAction {
    /// 查询类动作
    Query,
    /// 汇总/总结
    Summarize,
    /// 分析
    Analyze,
    /// 监控/追踪
    Monitor,
    Create,
    Update,
    Delete,
    /// 比较
    Compare,
    /// 推荐
    Recommend,
    /// 导出
    Export,
    /// 执行/运行
    Execute,
    /// 导航/打开
    Navigate,
    /// 控制（如媒体播放器控制）
    Control,
    /// 未知动作
    Unknown(String),
}

/// 输出格式
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    Text,
    Json,
    Markdown,
    Html,
    Chart,
}

// 能力注册相关类型

/// 系统能力定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capability {
    /// 能力 ID
    pub id: String,
    /// 能力名称
    pub name: String,
    /// 能力描述
    pub description: String,
    /// 能力类别
    pub category: CapabilityCategory,
    /// 支持的动作列表
    pub supported_actions: Vec<IntentAction>,
    /// 输入参数规格
    pub input_schema: Value,
    /// 输出格式规格
    pub output_schema: Value,
    /// 能力声明的权限串（执行时对照授予权限）
    pub required_permissions: Vec<String>,
    /// 是否需要 AI
    pub requires_ai: bool,
    /// 估计执行时间（毫秒）
    pub estimated_duration_ms: Option<u64>,
    /// 是否需要二次确认（敏感操作）
    #[serde(default)]
    pub requires_confirmation: bool,
    /// 确认提示信息
    #[serde(default)]
    pub confirmation_message: Option<String>,
    /// 风险等级
    #[serde(default)]
    pub risk_level: RiskLevel,
}

/// 风险等级
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    /// 无风险
    #[default]
    None,
    /// 低风险 - 可逆操作
    Low,
    /// 中风险 - 可能影响数据
    Medium,
    /// 高风险 - 不可逆操作
    High,
    /// 危险 - 系统级敏感操作
    Critical,
}

/// 能力类别
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityCategory {
    /// 数据读取
    DataRead,
    /// 数据写入
    DataWrite,
    /// AI 处理
    AiProcess,
    /// 资源创建
    ResourceCreate,
    /// 系统操作
    SystemOp,
    /// 外部集成
    ExternalIntegration,
    /// UI 控制/交互
    UiControl,
}

// 方案（Recipe）相关类型

/// 执行方案
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recipe {
    /// 方案 ID
    pub id: String,
    /// 方案名称
    pub name: String,
    /// 原始请求
    pub original_request: String,
    /// 执行类型
    pub execution_type: ExecutionType,
    /// 执行步骤
    pub steps: Vec<RecipeStep>,
    /// 预期输出格式
    pub expected_output: OutputFormat,
    /// 估计总时长（毫秒）
    pub estimated_duration_ms: u64,
    pub created_at: chrono::DateTime<chrono::Utc>,
    #[serde(default)]
    pub metadata: HashMap<String, Value>,
    /// 页面上下文（当前阅读的文章等）
    #[serde(default)]
    pub page_context: Option<Value>,
    /// 对话历史上下文（用于继续对话模式）
    #[serde(default)]
    pub conversation_context: Option<Vec<ConversationMessage>>,
    /// Lane key（用于队列追踪）
    #[serde(default)]
    pub lane_key: Option<String>,
    #[serde(default)]
    pub autonomy_permission_cap: Option<Vec<String>>,
}

/// 执行类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionType {
    /// 即时执行（生产路径一律 Instant，含多步 / 等待输入）
    Instant,
    /// 持续监控（变体存在；生产路径不构造）
    Continuous,
    /// 资源创建（变体存在；生产路径不构造）
    Creation,
    /// 批处理（变体存在；生产路径不构造）
    Batch,
}

/// 方案步骤
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecipeStep {
    /// 步骤 ID
    pub id: String,
    /// 步骤序号
    pub order: u32,
    /// 使用的能力 ID
    pub capability_id: String,
    /// 动作
    pub action: String,
    /// 输入参数
    #[serde(default)]
    pub params: HashMap<String, Value>,
    /// 依赖的步骤 ID 列表
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// 失败处理策略
    pub on_failure: FailureStrategy,
    /// 重试配置
    pub retry: Option<RetryConfig>,
    /// 超时时间（毫秒）
    pub timeout_ms: Option<u64>,
    /// 模型层级（可选，覆盖 TierRouter 自动推断）
    #[serde(default)]
    pub model_tier: Option<ModelTier>,
    /// 动态步骤生成器（步骤完成后触发）
    #[serde(default)]
    pub generator: Option<StepGenerator>,
}

pub use myriad_agent_rules::{FailureStrategy, RetryConfig};

// AI Recipe 生成相关类型

/// LLM 生成的单个 recipe 步骤
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiRecipeStep {
    /// 步骤 ID（如 "step_1"）
    pub id: String,
    /// 能力 ID（如 "ai.summarize"）
    pub capability_id: String,
    /// 动作（如 "summarize"）
    pub action: String,
    /// 参数（AI 根据 schema 生成）
    #[serde(default)]
    pub params: HashMap<String, Value>,
    /// 依赖的步骤 ID 列表
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// 失败策略：`"skip"` → Skip，其余（含 `"abort"`）→ Abort
    #[serde(default = "default_on_failure")]
    pub on_failure: String,
    /// 重试配置
    #[serde(default)]
    pub retry: Option<RetryConfig>,
    /// 超时时间（毫秒）
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

fn default_on_failure() -> String {
    "abort".to_string()
}

impl AiRecipeStep {
    /// 转为 `RecipeStep`：写入 `order` / `model_tier`；`on_failure` 仅 `"skip"`→Skip，其余 Abort；`timeout_ms` 缺省 30000。
    pub fn into_recipe_step(self, order: u32, tier: Option<ModelTier>) -> RecipeStep {
        let failure_strategy = match self.on_failure.as_str() {
            "skip" => FailureStrategy::Skip,
            _ => FailureStrategy::Abort,
        };

        RecipeStep {
            id: self.id,
            order,
            capability_id: self.capability_id,
            action: self.action,
            params: self.params,
            depends_on: self.depends_on,
            on_failure: failure_strategy,
            retry: self.retry,
            timeout_ms: self.timeout_ms.or(Some(30000)),
            model_tier: tier,
            generator: None,
        }
    }
}

// Planner 输出类型

/// Planner 输出（合并意图分析 + Recipe 生成为单次 Pro AI 调用）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerOutput {
    /// 输出状态
    pub status: PlannerStatus,
    /// 置信度（缺省 0.8）
    #[serde(default = "default_confidence")]
    pub confidence: f32,
    /// AI 推理说明
    #[serde(default)]
    pub reasoning: Option<String>,
    /// 执行步骤（status=plan 时使用）
    #[serde(default)]
    pub steps: Vec<AiRecipeStep>,
    /// 澄清信息（status=clarify 时使用）
    #[serde(default)]
    pub clarification: Option<PlannerClarification>,
    /// 不支持原因（status=unsupported 时使用）
    #[serde(default)]
    pub unsupported_reason: Option<String>,
    /// 直接回复（status=chat 时使用）
    #[serde(default)]
    pub chat_reply: Option<String>,
}

fn default_confidence() -> f32 {
    0.8
}

/// Planner 状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PlannerStatus {
    /// 生成执行计划
    Plan,
    /// 需要用户澄清
    Clarify,
    /// 不支持的请求
    Unsupported,
    /// 直接对话回复（无需调用能力）
    Chat,
}

/// Planner 澄清信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerClarification {
    /// 澄清消息
    pub message: String,
    /// 可选的选项
    #[serde(default)]
    pub options: Vec<String>,
}

// 执行状态相关类型

/// 任务执行状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskState {
    /// 任务 ID
    pub task_id: String,
    /// 方案 ID
    pub recipe_id: String,
    /// 当前状态
    pub status: TaskStatus,
    /// 当前步骤索引
    pub current_step: usize,
    /// 步骤执行结果
    #[serde(default)]
    pub step_results: HashMap<String, StepResult>,
    pub started_at: chrono::DateTime<chrono::Utc>,
    /// 完成时间
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    /// 错误信息
    pub error: Option<String>,
    /// 进度百分比（`update_progress` 按 current_step/total_steps×100）
    pub progress: u8,
    /// 待用户回答的问题（当状态为 WaitingForInput 时）
    #[serde(default)]
    pub pending_question: Option<UserQuestion>,
    /// 执行上下文（序列化，供任务持久化与动态步骤恢复）
    #[serde(default)]
    pub execution_context: Option<ExecutionContext>,
    /// Lane ID（用于队列追踪）
    #[serde(default)]
    pub lane_id: Option<String>,
    /// 执行追踪（可观测性）
    #[serde(default)]
    pub execution_trace: Option<ExecutionTrace>,
    /// 原始 Recipe（用于 resume_with_answer 恢复执行）
    #[serde(default)]
    pub recipe: Option<Recipe>,
}

/// 执行追踪 — 记录整个任务的执行链路
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExecutionTrace {
    /// 追踪 ID
    pub trace_id: String,
    /// 各步骤追踪
    pub steps: Vec<StepTrace>,
    /// 总耗时（毫秒）
    pub total_duration_ms: u64,
    /// 各 tier 使用次数
    pub tier_usage: std::collections::HashMap<String, u32>,
    /// 主 Agent（Planner）的原始决策
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planner_decision: Option<PlannerDecisionInfo>,
}

/// Planner 决策快照（嵌入到 ExecutionTrace 中持久化）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerDecisionInfo {
    /// Planner 状态
    pub status: String,
    /// AI 推理说明
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    /// 置信度
    pub confidence: f32,
    /// 计划的步骤摘要
    #[serde(rename = "plannedSteps", alias = "planned_steps")]
    pub planned_steps: Vec<PlannerStepSummary>,
}

/// Planner 规划的单步摘要
///
/// `capabilityId` 为 camelCase（SSE / FE debug）；其余字段 snake_case。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerStepSummary {
    pub id: String,
    #[serde(rename = "capabilityId", alias = "capability_id")]
    pub capability_id: String,
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

/// 步骤追踪 — 记录单步执行详情
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepTrace {
    pub step_id: String,
    pub capability_id: String,
    pub tier_used: String,
    pub duration_ms: u64,
    pub success: bool,
    pub error: Option<String>,
    /// 主 Agent 对此步骤的指令（step.action）
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub action: String,
    /// 步骤参数快照（`serde_json::to_value(&step.params)`，不脱敏）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
    /// 输出预览（`StepTrace` 写入为 `None`；截断在 `StepDebug`）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_preview: Option<String>,
    /// 是否为动态生成的步骤
    #[serde(default)]
    pub is_dynamic: bool,
}

/// 等待用户输入的默认 TTL（`DEFAULT_QUESTION_TTL_MINUTES` = 30）。缺少 `expires_at` 时按 `created_at` + 此值补齐。
pub const DEFAULT_QUESTION_TTL_MINUTES: i64 = 30;

/// 用户问题 - Agent 向用户提出的澄清问题
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserQuestion {
    /// 问题 ID
    pub question_id: String,
    /// 问题类型
    pub question_type: QuestionType,
    /// 问题文本
    pub question: String,
    /// 问题上下文（为什么要问这个问题）
    pub context: String,
    /// 可选项（用于选择类问题）
    pub options: Option<Vec<QuestionOption>>,
    /// 是否必须回答
    pub required: bool,
    /// 默认值
    pub default_value: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl UserQuestion {
    /// 保证有过期时间：缺失时用 created_at + DEFAULT_QUESTION_TTL_MINUTES。
    pub fn ensure_expires_at(&mut self) {
        if self.expires_at.is_none() {
            self.expires_at =
                Some(self.created_at + chrono::Duration::minutes(DEFAULT_QUESTION_TTL_MINUTES));
        }
    }

    /// 是否已过期（无 expires_at 时按默认 TTL 从 created_at 推算）。
    pub fn is_expired(&self, now: chrono::DateTime<chrono::Utc>) -> bool {
        let exp = self.expires_at.unwrap_or_else(|| {
            self.created_at + chrono::Duration::minutes(DEFAULT_QUESTION_TTL_MINUTES)
        });
        now > exp
    }
}

/// 问题类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum QuestionType {
    /// 自由文本输入
    FreeText,
    /// 单选
    SingleChoice,
    /// 多选
    MultipleChoice,
    /// 是/否确认
    Confirmation,
    /// 数值输入
    Numeric,
    /// 日期选择
    Date,
}

/// 问题选项
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionOption {
    /// 选项值
    pub value: String,
    /// 显示文本
    pub label: String,
    /// 选项描述
    pub description: Option<String>,
}

/// 用户回答
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserAnswer {
    /// 问题 ID
    pub question_id: String,
    /// 任务 ID
    pub task_id: String,
    /// 回答内容
    pub answer: String,
    /// 是否跳过（用户选择不回答）
    pub skipped: bool,
}

pub use myriad_agent_rules::TaskStatus;

/// 步骤执行结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResult {
    /// 步骤 ID
    pub step_id: String,
    /// 是否成功
    pub success: bool,
    /// 输出数据
    pub output: Option<Value>,
    /// 错误信息
    pub error: Option<String>,
    /// 执行时长（毫秒）
    pub duration_ms: u64,
    /// 重试次数
    pub retry_count: u32,
}

// Agent 响应类型

/// Agent 响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResponse {
    /// 响应类型
    pub response_type: AgentResponseType,
    /// 消息内容
    pub message: String,
    /// 结果数据
    pub data: Option<Value>,
    /// 数据展示类型提示（帮助前端选择合适的渲染方式）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_display: Option<DataDisplayHint>,
    /// 后续建议
    pub suggestions: Vec<String>,
    /// 任务状态
    pub task: Option<TaskState>,
    /// 确认请求信息（当 response_type 为 ConfirmationRequired 时）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confirmation: Option<ConfirmationRequest>,
    /// 前端操作指令（路由导航、页面元素交互等）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frontend_action: Option<Value>,
    /// 先填 `local_directive`；Lite 精炼走 SSE `PerformancePlan`。Driver 仍在客户端。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub performance: Option<super::merope::PerformanceDirective>,
}

impl AgentResponse {
    /// Whether this response represents a completed, unblocked operation.
    /// Transport success alone is insufficient for unattended Heartbeat jobs.
    pub fn is_successful_outcome(&self) -> bool {
        if !matches!(
            self.response_type,
            AgentResponseType::Answer | AgentResponseType::TaskCompleted
        ) {
            return false;
        }

        if let Some(task) = &self.task {
            if task.status != TaskStatus::Completed
                || task.step_results.values().any(|result| !result.success)
            {
                return false;
            }
        }

        !self.data.as_ref().is_some_and(|data| {
            data.get("blocked").and_then(Value::as_bool) == Some(true)
                || data.get("unsupported").and_then(Value::as_bool) == Some(true)
        })
    }
}

/// 数据展示类型提示
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataDisplayHint {
    /// 表格展示
    Table {
        /// 列定义
        columns: Vec<ColumnDef>,
        /// 前端数据路径（后端原样转 API，不求值）
        #[serde(default)]
        data_path: Option<String>,
    },
    /// 图表展示
    Chart {
        /// 图表类型
        chart_type: ChartType,
        /// X 轴字段
        x_field: String,
        /// Y 轴字段
        y_field: String,
    },
    /// 卡片列表
    CardList {
        /// 标题字段
        title_field: String,
        /// 描述字段
        description_field: Option<String>,
        /// 图片字段
        image_field: Option<String>,
    },
    /// Markdown 富文本
    Markdown,
    /// 键值对展示
    KeyValue,
    /// 时间线
    Timeline {
        /// 时间字段
        time_field: String,
        /// 内容字段
        content_field: String,
    },
    /// 原始 JSON
    Raw,
}

/// 表格列定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnDef {
    /// 字段名
    pub field: String,
    /// 显示标题
    pub title: String,
    /// 列宽
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    /// 是否可排序
    #[serde(default)]
    pub sortable: bool,
}

/// 图表类型
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChartType {
    Line,
    Bar,
    Pie,
    Area,
}

/// Agent 响应类型
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentResponseType {
    /// 直接回答
    Answer,
    /// 需要澄清
    Clarification,
    /// 需要确认（敏感操作）
    ConfirmationRequired,
    /// 任务已创建
    TaskCreated,
    /// 任务进度更新
    TaskProgress,
    /// 任务完成
    TaskCompleted,
    /// 错误
    Error,
}

/// 确认请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfirmationRequest {
    /// 确认 ID（用于后续确认/取消）
    pub confirmation_id: String,
    /// 待确认的配方 ID
    pub recipe_id: String,
    /// 需要确认的步骤
    pub pending_steps: Vec<PendingConfirmation>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

/// 待确认的步骤
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingConfirmation {
    /// 步骤 ID
    pub step_id: String,
    /// 能力 ID
    pub capability_id: String,
    /// 能力名称
    pub capability_name: String,
    /// 操作描述
    pub description: String,
    /// 风险等级
    pub risk_level: RiskLevel,
    /// 确认提示
    pub confirmation_message: String,
    /// 操作影响说明
    pub impact: Vec<String>,
}

/// 用户确认响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserConfirmation {
    /// 确认 ID
    pub confirmation_id: String,
    /// 是否确认执行
    pub confirmed: bool,
    /// 用户备注（可选）
    pub user_note: Option<String>,
    /// 操作发起者的用户 ID（用于归属校验）
    #[serde(default)]
    pub user_id: i32,
}

impl Default for Capability {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            description: String::new(),
            category: CapabilityCategory::DataRead,
            supported_actions: vec![],
            input_schema: serde_json::json!({}),
            output_schema: serde_json::json!({}),
            required_permissions: vec![],
            requires_ai: false,
            estimated_duration_ms: None,
            requires_confirmation: false,
            confirmation_message: None,
            risk_level: RiskLevel::None,
        }
    }
}

impl Recipe {
    #[cfg(test)]
    pub fn new(name: &str, original_request: &str, execution_type: ExecutionType) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            original_request: original_request.to_string(),
            execution_type,
            steps: Vec::new(),
            expected_output: OutputFormat::Text,
            estimated_duration_ms: 0,
            created_at: chrono::Utc::now(),
            metadata: HashMap::new(),
            page_context: None,
            conversation_context: None,
            lane_key: None,
            autonomy_permission_cap: None,
        }
    }
}

impl TaskState {
    pub fn new(recipe: &Recipe) -> Self {
        // task_id 必须与 TaskCreated SSE / 前端 cancel·steer 使用的 id 一致。
        // 执行路径在创建 Recipe 时已为每次运行生成唯一 id；保存的预设在执行前会重新 mint。
        Self {
            task_id: recipe.id.clone(),
            recipe_id: recipe.id.clone(),
            status: TaskStatus::Pending,
            current_step: 0,
            step_results: HashMap::new(),
            started_at: chrono::Utc::now(),
            completed_at: None,
            error: None,
            progress: 0,
            pending_question: None,
            execution_context: None,
            lane_id: None,
            execution_trace: None,
            recipe: Some(recipe.clone()),
        }
    }

    /// 设置待回答的问题，并将状态改为等待输入
    pub fn set_pending_question(&mut self, mut question: UserQuestion) {
        question.ensure_expires_at();
        self.pending_question = Some(question);
        self.status = TaskStatus::WaitingForInput;
    }

    /// 清除待回答问题，恢复执行状态
    pub fn clear_pending_question(&mut self) {
        self.pending_question = None;
        self.status = TaskStatus::Running;
    }

    pub fn update_progress(&mut self, total_steps: usize) {
        if total_steps > 0 {
            self.progress = ((self.current_step as f32 / total_steps as f32) * 100.0) as u8;
        }
    }
}

// 动态任务更新机制

/// 步骤生成器类型
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepGenerator {
    /// 基于 UI 分析结果生成交互步骤
    UiInteractionFromAnalysis {
        /// 源步骤 ID（读该步 `elements`）
        source_step: String,
        /// 要执行的操作描述
        operation_intent: String,
    },
    /// 基于列表数据生成迭代步骤
    IterateFromList {
        /// 源步骤 ID
        source_step: String,
        /// 每项要执行的能力
        item_capability: String,
    },
    /// 基于条件分支
    ConditionalBranch {
        /// 条件表达式
        condition: String,
        /// 满足条件时的步骤
        if_true: Vec<RecipeStep>,
        /// 不满足条件时的步骤
        if_false: Vec<RecipeStep>,
    },
    /// AI 智能生成（根据上下文让 AI 决定下一步）
    AiGenerated {
        /// AI 上下文提示
        context_prompt: String,
        /// 可选的能力范围限制
        capability_scope: Option<Vec<String>>,
    },
}

/// 执行上下文 - 在步骤间传递的动态上下文
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ExecutionContext {
    /// 所有步骤的输出
    pub step_outputs: HashMap<String, Value>,
    /// 发现的 UI 元素
    pub discovered_ui_elements: Option<Value>,
    /// 当前交互目标
    pub interaction_target: Option<InteractionTarget>,
    /// 待执行的动态步骤队列
    pub pending_dynamic_steps: Vec<RecipeStep>,
    /// 已生成的动态步骤数量
    pub dynamic_steps_generated: usize,
    /// 执行变量（可被后续步骤引用）
    pub variables: HashMap<String, Value>,
    /// 原始用户请求（用于 AI 分析时参考）
    pub original_request: String,
    /// 用户意图描述
    pub user_intent: String,
    /// 已回答的问题
    pub answered_questions: HashMap<String, String>,
    /// 执行决策历史（用于追踪 AI 的决策过程）
    pub decision_history: Vec<ExecutionDecision>,
    /// 页面上下文（当前阅读的文章等）
    pub page_context: Option<Value>,
    /// 对话历史上下文（用于「继续对话」功能）
    /// 包含之前的对话消息，让 AI 能够理解上下文
    #[serde(default)]
    pub conversation_context: Option<Vec<ConversationMessage>>,
    /// 角色身份上下文（Orchestrator 注入）
    /// key = `format!("{:?}", role)`（`AgentRole` Debug）；value = 对应 worker 身份文本
    #[serde(default)]
    pub role_contexts: HashMap<String, String>,
    /// Extra ceiling for autonomy-accepted Work. Names only.
    #[serde(default)]
    pub autonomy_permission_cap: Option<Vec<String>>,
    /// 全局重试预算剩余（跨 resume 保持；缺省 5）
    #[serde(default = "default_retry_budget")]
    pub retry_budget_remaining: u32,
    /// 待提问队列（DAG 中多个问题排队，每次 resume 后检查是否还有待问问题）
    #[serde(default)]
    pub pending_questions: Vec<UserQuestion>,
    /// 记忆上下文摘要（Executor 初始化时召回，供 AI 步骤参考）
    #[serde(default)]
    pub memory_context: Option<String>,
    /// `queue_dynamic_steps` 写入的步骤 ID（技能展开 / 动态分析 / 重试前置）；执行时补敏感确认。
    #[serde(default)]
    pub dynamic_step_ids: std::collections::HashSet<String>,
}

fn default_retry_budget() -> u32 {
    5
}

impl Default for ExecutionContext {
    fn default() -> Self {
        Self {
            step_outputs: HashMap::new(),
            discovered_ui_elements: None,
            interaction_target: None,
            pending_dynamic_steps: Vec::new(),
            dynamic_steps_generated: 0,
            variables: HashMap::new(),
            original_request: String::new(),
            user_intent: String::new(),
            answered_questions: HashMap::new(),
            decision_history: Vec::new(),
            page_context: None,
            conversation_context: None,
            role_contexts: HashMap::new(),
            retry_budget_remaining: 5,
            pending_questions: Vec::new(),
            memory_context: None,
            dynamic_step_ids: std::collections::HashSet::new(),
            autonomy_permission_cap: None,
        }
    }
}

/// 执行决策 - 记录 AI 在执行过程中的决策
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionDecision {
    /// 决策时间
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// 决策类型
    pub decision_type: DecisionType,
    /// 决策描述
    pub description: String,
    /// 决策依据
    pub reasoning: String,
    /// 相关步骤 ID
    pub related_step: Option<String>,
}

/// 决策类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DecisionType {
    /// 生成新步骤
    GenerateSteps,
    /// 跳过步骤
    SkipStep,
    /// 修改参数
    ModifyParams,
    /// 询问用户
    AskUser,
    /// 使用默认值
    UseDefault,
    /// 终止执行
    Abort,
    /// 重试
    Retry,
}

/// 交互目标
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractionTarget {
    /// 目标元素 ID
    pub element_id: String,
    /// 元素类型
    pub element_type: String,
    /// 要执行的操作
    pub operation: String,
    /// 操作参数
    pub params: Option<Value>,
}

impl ExecutionContext {
    /// 从用户请求创建完整上下文（包含对话历史和页面上下文）
    pub fn from_request_full(
        request: &str,
        intent: &str,
        page_context: Option<Value>,
        conversation_context: Option<Vec<ConversationMessage>>,
    ) -> Self {
        Self {
            original_request: request.to_string(),
            user_intent: intent.to_string(),
            page_context,
            conversation_context,
            ..Default::default()
        }
    }

    /// 添加步骤输出
    pub fn add_output(&mut self, step_id: &str, output: Value) {
        self.step_outputs.insert(step_id.to_string(), output);
    }

    /// 获取所有步骤输出（用于向后续步骤共享已有结果）
    pub fn get_all_outputs(&self) -> &HashMap<String, Value> {
        &self.step_outputs
    }

    /// 设置变量
    pub fn set_var(&mut self, key: &str, value: Value) {
        self.variables.insert(key.to_string(), value);
    }

    /// 添加待执行的动态步骤（`MAX_DYNAMIC_QUEUE`=15 累计 `dynamic_steps_generated`，ID 去重，自依赖整步拒绝）
    pub fn queue_dynamic_steps(&mut self, steps: Vec<RecipeStep>) {
        const MAX_DYNAMIC_QUEUE: usize = 15;
        let remaining = MAX_DYNAMIC_QUEUE.saturating_sub(self.dynamic_steps_generated);
        if remaining == 0 {
            tracing::warn!(
                "[ExecutionContext] Dynamic steps queue full ({}/{}), rejecting {} new steps",
                self.dynamic_steps_generated,
                MAX_DYNAMIC_QUEUE,
                steps.len()
            );
            return;
        }

        // 收集已有的 step_id（已完成 + 待执行队列）
        let existing_ids: std::collections::HashSet<&str> = self
            .step_outputs
            .keys()
            .map(|s| s.as_str())
            .chain(self.pending_dynamic_steps.iter().map(|s| s.id.as_str()))
            .collect();

        let accepted: Vec<RecipeStep> = steps
            .into_iter()
            .take(remaining)
            .filter(|step| {
                // 去重：跳过 ID 已存在的步骤
                if existing_ids.contains(step.id.as_str()) {
                    tracing::warn!(
                        step_id = %step.id,
                        "[ExecutionContext] Rejecting dynamic step with duplicate ID"
                    );
                    return false;
                }
                // 自依赖：depends_on 含自身则整步拒绝
                if step.depends_on.iter().any(|d| d == &step.id) {
                    tracing::warn!(
                        step_id = %step.id,
                        "[ExecutionContext] Dynamic step has self-dependency, rejecting"
                    );
                    return false;
                }
                true
            })
            .collect();

        let accepted_count = accepted.len();
        self.dynamic_steps_generated += accepted_count;
        for step in &accepted {
            self.dynamic_step_ids.insert(step.id.clone());
        }
        self.pending_dynamic_steps.extend(accepted);
    }

    /// 是否在 `dynamic_step_ids`（技能展开 / 动态分析 / 重试前置）
    pub fn is_dynamic_step(&self, step_id: &str) -> bool {
        self.dynamic_step_ids.contains(step_id)
    }

    /// 取出下一个待执行的动态步骤
    pub fn pop_dynamic_step(&mut self) -> Option<RecipeStep> {
        if !self.pending_dynamic_steps.is_empty() {
            Some(self.pending_dynamic_steps.remove(0))
        } else {
            None
        }
    }

    /// 检查是否有待执行的动态步骤
    pub fn has_pending_steps(&self) -> bool {
        !self.pending_dynamic_steps.is_empty()
    }

    /// 记录用户回答
    pub fn record_answer(&mut self, question_id: &str, answer: &str) {
        self.answered_questions
            .insert(question_id.to_string(), answer.to_string());
    }

    /// 记录执行决策
    pub fn record_decision(
        &mut self,
        decision_type: DecisionType,
        description: &str,
        reasoning: &str,
        step_id: Option<&str>,
    ) {
        self.decision_history.push(ExecutionDecision {
            timestamp: chrono::Utc::now(),
            decision_type,
            description: description.to_string(),
            reasoning: reasoning.to_string(),
            related_step: step_id.map(|s| s.to_string()),
        });
    }
}

impl UserQuestion {
    /// 创建自由文本问题
    pub fn free_text(question: &str, context: &str, required: bool) -> Self {
        Self {
            question_id: uuid::Uuid::new_v4().to_string(),
            question_type: QuestionType::FreeText,
            question: question.to_string(),
            context: context.to_string(),
            options: None,
            required,
            default_value: None,
            created_at: chrono::Utc::now(),
            expires_at: Some(
                chrono::Utc::now() + chrono::Duration::minutes(DEFAULT_QUESTION_TTL_MINUTES),
            ),
        }
    }

    /// 创建单选问题
    pub fn single_choice(
        question: &str,
        context: &str,
        options: Vec<QuestionOption>,
        required: bool,
    ) -> Self {
        Self {
            question_id: uuid::Uuid::new_v4().to_string(),
            question_type: QuestionType::SingleChoice,
            question: question.to_string(),
            context: context.to_string(),
            options: Some(options),
            required,
            default_value: None,
            created_at: chrono::Utc::now(),
            expires_at: Some(
                chrono::Utc::now() + chrono::Duration::minutes(DEFAULT_QUESTION_TTL_MINUTES),
            ),
        }
    }

    /// 创建确认问题
    pub fn confirmation(question: &str, context: &str) -> Self {
        Self {
            question_id: uuid::Uuid::new_v4().to_string(),
            question_type: QuestionType::Confirmation,
            question: question.to_string(),
            context: context.to_string(),
            options: Some(vec![
                QuestionOption {
                    value: "yes".to_string(),
                    label: crate::services::agent::response_agent::yes_label(),
                    description: None,
                },
                QuestionOption {
                    value: "no".to_string(),
                    label: crate::services::agent::response_agent::no_label(),
                    description: None,
                },
            ]),
            required: true,
            default_value: None,
            created_at: chrono::Utc::now(),
            expires_at: Some(
                chrono::Utc::now() + chrono::Duration::minutes(DEFAULT_QUESTION_TTL_MINUTES),
            ),
        }
    }
}

#[cfg(test)]
mod question_ttl_tests {
    use super::*;

    #[test]
    fn ensure_expires_at_fills_default_ttl() {
        let mut q = UserQuestion {
            question_id: "q1".into(),
            question_type: QuestionType::FreeText,
            question: "x".into(),
            context: String::new(),
            options: None,
            required: true,
            default_value: None,
            created_at: chrono::Utc::now() - chrono::Duration::minutes(5),
            expires_at: None,
        };
        assert!(!q.is_expired(chrono::Utc::now()));
        q.ensure_expires_at();
        assert!(q.expires_at.is_some());
        let exp = q.expires_at.unwrap();
        assert!(exp > chrono::Utc::now());
        assert_eq!(
            (exp - q.created_at).num_minutes(),
            DEFAULT_QUESTION_TTL_MINUTES
        );
    }

    #[test]
    fn is_expired_uses_default_when_missing() {
        let q = UserQuestion {
            question_id: "q2".into(),
            question_type: QuestionType::FreeText,
            question: "old".into(),
            context: String::new(),
            options: None,
            required: false,
            default_value: None,
            created_at: chrono::Utc::now()
                - chrono::Duration::minutes(DEFAULT_QUESTION_TTL_MINUTES + 1),
            expires_at: None,
        };
        assert!(q.is_expired(chrono::Utc::now()));
    }
}

impl QuestionOption {
    pub fn new(value: &str, label: &str) -> Self {
        Self {
            value: value.to_string(),
            label: label.to_string(),
            description: None,
        }
    }

    pub fn with_description(mut self, description: &str) -> Self {
        self.description = Some(description.to_string());
        self
    }
}

// SSE 进度事件

/// Agent 进度事件（用于 SSE 实时推送）
///
/// executor / Chat / API 都会构造；api 层再序列化成 SSE。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentProgressEvent {
    /// 后端已接管本次运行。run_id 在执行任务 ID 产生前就可用，
    /// 前端断线后可用它重新订阅，而不会重新发起任务。
    RunStarted {
        #[serde(rename = "runId")]
        run_id: String,
        #[serde(rename = "sessionId", skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
    },
    /// 任务已创建
    TaskCreated {
        #[serde(rename = "taskId")]
        task_id: String,
        message: String,
        #[serde(rename = "totalSteps")]
        total_steps: u32,
        /// 各步骤的描述（用于前端展示执行计划）
        #[serde(rename = "stepDescriptions", skip_serializing_if = "Vec::is_empty")]
        step_descriptions: Vec<String>,
    },
    /// 任务已分配给多个 Agent（多 Agent 协作时发送）
    TaskAssigned {
        #[serde(rename = "taskId")]
        task_id: String,
        /// Agent 分配详情
        assignment: Box<super::routing::TaskAssignment>,
    },
    /// 步骤开始
    StepStarted {
        #[serde(rename = "stepId")]
        step_id: String,
        #[serde(rename = "stepIndex")]
        step_index: u32,
        #[serde(rename = "totalSteps")]
        total_steps: u32,
        #[serde(rename = "capabilityName")]
        capability_name: String,
        description: String,
    },
    /// 步骤完成
    StepCompleted {
        #[serde(rename = "stepId")]
        step_id: String,
        #[serde(rename = "stepIndex")]
        step_index: u32,
        success: bool,
        #[serde(rename = "durationMs")]
        duration_ms: u64,
        #[serde(rename = "outputSummary", skip_serializing_if = "Option::is_none")]
        output_summary: Option<String>,
        /// 步骤输出里的 `url` / `imageUrl`
        #[serde(rename = "imageUrl", skip_serializing_if = "Option::is_none")]
        image_url: Option<String>,
        /// 本步要立刻执行的前端动作（不要等整份 recipe 结束）
        #[serde(rename = "frontendActions", skip_serializing_if = "Vec::is_empty")]
        frontend_actions: Vec<Value>,
    },
    /// 进度更新
    Progress {
        progress: u8,
        #[serde(rename = "completedSteps")]
        completed_steps: u32,
        #[serde(rename = "totalSteps")]
        total_steps: u32,
        message: String,
    },
    /// 步骤重试中（智能重试：分析错误后修改参数重试）
    StepRetrying {
        #[serde(rename = "stepId")]
        step_id: String,
        #[serde(rename = "stepIndex")]
        step_index: u32,
        #[serde(rename = "retryCount")]
        retry_count: u32,
        #[serde(rename = "maxRetries")]
        max_retries: u32,
        /// 重试原因（错误分析结果）
        reason: String,
    },
    /// 任务完成（response 为序列化后的 ApiResponse JSON）
    TaskCompleted {
        #[serde(rename = "taskId")]
        task_id: String,
        success: bool,
        /// 已序列化的 API 响应（由 api 层填充）
        response: Box<Value>,
    },
    /// 需要用户输入（Agent 向用户提问）
    WaitingForInput {
        #[serde(rename = "taskId")]
        task_id: String,
        #[serde(rename = "questionId")]
        question_id: String,
        #[serde(rename = "questionType")]
        question_type: String,
        question: String,
        /// 问题上下文（为什么要问这个问题）
        #[serde(skip_serializing_if = "Option::is_none")]
        context: Option<String>,
        /// 选项（选择类问题）
        #[serde(skip_serializing_if = "Option::is_none")]
        options: Option<Vec<QuestionOptionCompact>>,
        /// 是否必填
        #[serde(default)]
        required: bool,
        /// 默认值（供前端预填）
        #[serde(rename = "defaultValue", skip_serializing_if = "Option::is_none")]
        default_value: Option<String>,
    },
    /// 会话已创建/确认（通知前端 session_id）
    SessionCreated {
        #[serde(rename = "sessionId")]
        session_id: String,
    },
    /// 会话标题已生成（AI 并行生成）
    SessionTitleUpdated { title: String },
    /// 面向用户的流式正文 token（Chat / response_agent），不是 Planner 推理。
    SummaryToken {
        /// 文本片段
        token: String,
        /// 是否为最后一个 token
        done: bool,
    },
    /// 模型思考链流式 token（reasoning_content / thought parts）
    ///
    /// 和 SummaryToken 分开：思考过程进气泡的过程区，不能写进正文。
    ThinkingToken { token: String, done: bool },
    /// A low-latency semantic motion plan. It may precede the final response.
    PerformancePlan {
        performance: super::merope::PerformanceDirective,
    },
    /// Mood/activity for live-face UI sync.
    MeropeStateChanged {
        mood: super::merope::MoodTransition,
        activity: String,
    },
    /// Chat-only temporary wardrobe overlay. `null` returns to the worn set.
    OutfitOverlay {
        #[serde(rename = "outfitId")]
        outfit_id: Option<String>,
    },
    /// Chat 流式路径发出的播放器控制（`ChatMusicAction::as_str()`）。
    MusicControl { action: String },
    /// 错误
    Error {
        #[serde(rename = "taskId", skip_serializing_if = "Option::is_none")]
        task_id: Option<String>,
        message: String,
        code: String,
    },
    /// 主 Agent（Planner）决策 — 调试用
    PlannerDecision {
        /// Planner 输出状态
        status: String,
        /// AI 推理
        #[serde(skip_serializing_if = "Option::is_none")]
        reasoning: Option<String>,
        /// 置信度
        confidence: f32,
        /// 规划的步骤列表
        steps: Vec<PlannerStepSummary>,
        /// 用户原始请求
        #[serde(rename = "userRequest")]
        user_request: String,
    },
    /// 子 Agent 步骤执行详情 — 调试用
    StepDebug {
        #[serde(rename = "stepId")]
        step_id: String,
        /// 步骤阶段：`"start"` / `"complete"` / `"expired"`
        phase: String,
        #[serde(rename = "capabilityId")]
        capability_id: String,
        /// 主 Agent 给此步骤的指令
        #[serde(skip_serializing_if = "Option::is_none")]
        directive: Option<String>,
        /// 用户原始请求
        #[serde(rename = "userRequest", skip_serializing_if = "Option::is_none")]
        user_request: Option<String>,
        /// `build_debug_params(&step.params)`（截断原 params，不是 resolve 后）
        #[serde(skip_serializing_if = "Option::is_none")]
        params: Option<Value>,
        /// 执行输出预览（`complete` 才有）
        #[serde(rename = "outputPreview", skip_serializing_if = "Option::is_none")]
        output_preview: Option<String>,
        /// 是否动态步骤
        #[serde(rename = "isDynamic")]
        is_dynamic: bool,
        /// 耗时 ms（`complete` 才有）
        #[serde(rename = "durationMs", skip_serializing_if = "Option::is_none")]
        duration_ms: Option<u64>,
        /// 是否成功（`complete` / `expired`）
        #[serde(skip_serializing_if = "Option::is_none")]
        success: Option<bool>,
        /// 错误信息（`complete` 失败或 `expired`）
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
}

/// SSE 事件中的精简选项（value、label，可选 description）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionOptionCompact {
    pub value: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[cfg(test)]
mod interaction_mode_tests {
    use super::*;

    #[test]
    fn legacy_request_context_defaults_to_work() {
        let context: RequestContext = serde_json::from_value(serde_json::json!({})).unwrap();
        assert_eq!(context.interaction_mode, AgentInteractionMode::Work);
    }

    #[test]
    fn chat_mode_round_trips_as_snake_case() {
        let encoded = serde_json::to_value(AgentInteractionMode::Chat).unwrap();
        assert_eq!(encoded, serde_json::json!("chat"));
        let decoded: AgentInteractionMode = serde_json::from_value(encoded).unwrap();
        assert_eq!(decoded, AgentInteractionMode::Chat);
    }
}
