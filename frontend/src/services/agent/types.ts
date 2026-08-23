/**
 * Agent 服务类型定义
 *
 * AI 驱动的自然语言任务编排系统类型
 */

// 处理上下文

/** 处理上下文 */
export interface ProcessContext {
  /** 当前页面路由 */
  currentRoute?: string
  /** 活跃的平台 */
  activePlatforms?: string[]
  /** 会话 ID */
  sessionId?: string
  /** 自定义数据 */
  customData?: Record<string, unknown>
}

/** 处理请求 */
export interface ProcessRequest {
  /** 用户自然语言输入 */
  input: string
  /** 上下文信息 */
  context?: ProcessContext
}

/** 澄清请求 */
export interface ClarifyRequest {
  /** 原始请求 */
  originalInput: string
  /** 澄清点 ID */
  clarificationId: string
  /** 用户回答 */
  answer: string
  /** 上下文 */
  context?: ProcessContext
}

// 任务相关

/** 任务状态 */
export type TaskStatus =
  | 'pending'
  | 'running'
  | 'waiting_for_input'
  | 'paused'
  | 'completed'
  | 'failed'
  | 'cancelled'

/** 任务上的待回答问题（与后端 TaskInfo.pendingQuestion 对齐） */
export interface TaskPendingQuestion {
  questionId: string
  questionType: string
  question: string
  context?: string
  options?: Array<{ value: string; label: string; description?: string }>
  required?: boolean
  defaultValue?: string
}

/** 任务信息 */
export interface TaskInfo {
  taskId: string
  status: TaskStatus
  progress: number
  error?: string
  pendingQuestion?: TaskPendingQuestion
}

/** 任务详情 */
export interface TaskDetail {
  taskId: string
  recipeId: string
  status: string
  progress: number
  startedAt: string
  completedAt?: string
  results?: Record<string, unknown>
  pendingQuestion?: TaskPendingQuestion
  runId?: string
}

// 澄清相关

/** 澄清类型 */
export type ClarificationType =
  'time_range' | 'target' | 'action' | 'missing_parameter' | 'ambiguity'

/** 澄清点 */
export interface ClarificationPoint {
  id: string
  type: ClarificationType
  question: string
  options: string[]
  default?: string
}

// 响应相关

/** 响应类型 */
export type AgentResponseType =
  | 'answer'
  | 'clarification'
  | 'confirmation_required'
  | 'task_created'
  | 'task_progress'
  | 'task_completed'
  | 'error'

export type PerformancePhase =
  | 'reaction'
  | 'delivery'
  | 'outcome'
  | 'proactive'
  | 'mood'

export interface PerformanceBaseline {
  expression: 'withdrawn' | 'subdued' | 'steady' | 'warm'
  posture: 'closed' | 'neutral' | 'open'
  motionEnergy: number
  attention: number
}

export interface PerformanceCue {
  intent:
    | 'greet'
    | 'respond'
    | 'question'
    | 'delight'
    | 'emphasize'
    | 'listen'
    | 'notify'
  atMs: number
  intensity: number
  tempo: number
  fadeInMs: number
  fadeOutMs: number
  interrupt: 'replace' | 'queue' | 'if-lower'
}

export interface PerformanceDirective {
  phase: PerformancePhase
  moodRevision: number
  plan: {
    baseline?: PerformanceBaseline
    cues: PerformanceCue[]
  }
}

export interface MoodTransition {
  before: number
  after: number
  bandBefore: 'floor' | 'low' | 'normal' | 'high'
  bandAfter: 'floor' | 'low' | 'normal' | 'high'
  delta: number
  cause: string
  revision: number
}

/** Agent 响应 */
export interface AgentResponse {
  success: boolean
  responseType: AgentResponseType
  message: string
  data?: unknown
  /** 数据展示提示 */
  dataDisplay?: DataDisplayHint
  suggestions: string[]
  task?: TaskInfo
  /** 敏感操作确认信息 */
  confirmation?: ConfirmationInfo
  /** 前端操作指令 */
  frontendAction?: FrontendAction
  /** Strict-Lite semantic performance; contains no raw rig driver values. */
  performance?: PerformanceDirective
  sessionId?: string
}

/** 待确认的敏感步骤 */
export interface ConfirmationStep {
  stepId: string
  capabilityName: string
  message: string
  impact: string[]
}

/** 敏感操作确认信息 */
export interface ConfirmationInfo {
  confirmationId: string
  riskLevel: string
  expiresInSeconds: number
  pendingSteps: ConfirmationStep[]
}

// SSE 进度事件

/** 后端已接管运行；断线后使用 runId 重新订阅，不重复创建任务。 */
export interface RunStartedEvent {
  type: 'run_started'
  runId: string
  sessionId?: string
}

/** 任务创建事件 */
export interface TaskCreatedEvent {
  type: 'task_created'
  taskId: string
  message: string
  totalSteps: number
  /** 各步骤的描述（用于展示执行计划） */
  stepDescriptions?: string[]
  /** 队列位置，0=立即执行 */
  queuePosition?: number
  /** 触发的技能 ID */
  skillId?: string
  /** 触发的技能名称 */
  skillName?: string
}

/** 步骤开始事件 */
export interface StepStartedEvent {
  type: 'step_started'
  stepId: string
  stepIndex: number
  totalSteps: number
  capabilityName: string
  description: string
  /** 能力分类，用于显示角色图标 */
  capabilityCategory?: string
  /** 重试次数（0=首次） */
  retryAttempt?: number
}

/** 步骤完成事件 */
export interface StepCompletedEvent {
  type: 'step_completed'
  stepId: string
  stepIndex: number
  success: boolean
  durationMs: number
  outputSummary?: string
  /** 实际使用的模型层级 */
  tierUsed?: 'pro' | 'standard'
  /** 是否经历了降级（Pro→Standard） */
  degraded?: boolean
  /** 图片生成结果 URL */
  imageUrl?: string
}

/** 步骤重试事件（智能重试：分析错误后修改参数） */
export interface StepRetryingEvent {
  type: 'step_retrying'
  stepId: string
  stepIndex: number
  retryCount: number
  maxRetries: number
  /** 重试原因（错误分析结果） */
  reason: string
}

/** 进度更新事件 */
export interface ProgressUpdateEvent {
  type: 'progress'
  progress: number
  completedSteps: number
  totalSteps: number
  message: string
}

/** 任务完成事件 */
export interface TaskCompletedEvent {
  type: 'task_completed'
  taskId: string
  success: boolean
  response: AgentResponse
}

/** 等待用户输入事件 */
export interface WaitingForInputEvent {
  type: 'waiting_for_input'
  taskId: string
  questionId: string
  questionType: string
  question: string
  context?: string
  options?: Array<{ value: string; label: string; description?: string }>
  required: boolean
  defaultValue?: string
}

/** 错误事件 */
export interface ErrorEvent {
  type: 'error'
  taskId?: string
  message: string
  code: string
}

/** AI 总结流式 token 事件 */
export interface SummaryTokenEvent {
  type: 'summary_token'
  token: string
  done: boolean
}

export interface PerformancePlanEvent {
  type: 'performance_plan'
  performance: PerformanceDirective
}

export interface LifeStateChangedEvent {
  type: 'life_state_changed'
  mood: MoodTransition
  activity: string
}

/** 任务分配事件（多 Agent 协作时发送） */
export interface TaskAssignedEvent {
  type: 'task_assigned'
  taskId: string
  assignment: TaskAssignment
}

/** 主 Agent（Planner）决策调试事件 */
export interface PlannerDecisionEvent {
  type: 'planner_decision'
  status: string
  reasoning?: string
  confidence: number
  steps: Array<{
    id: string
    capabilityId: string
    action: string
    params?: Record<string, unknown>
  }>
  userRequest: string
}

/** 子 Agent 步骤执行调试事件 */
export interface StepDebugEvent {
  type: 'step_debug'
  stepId: string
  phase: 'start' | 'complete'
  capabilityId: string
  directive?: string
  userRequest?: string
  params?: Record<string, unknown>
  outputPreview?: string
  isDynamic: boolean
  durationMs?: number
  success?: boolean
  error?: string
}

/** 任务分配详情 */
export interface TaskAssignment {
  agents: AgentAssignment[]
  totalAgents: number
  isMultiAgent: boolean
  tierMix: 'pro' | 'standard' | 'mixed'
}

/** 单个 Agent 分配 */
export interface AgentAssignment {
  role: AgentRole
  agentId: string
  displayName: string
  icon: string
  tier: 'Pro' | 'Standard'
  capabilities: string[]
}

/** Agent 角色 */
export type AgentRole =
  | 'orchestrator'
  | 'data_worker'
  | 'content_worker'
  | 'creative_worker'
  | 'system_worker'

/** 会话创建/确认事件 */
export interface SessionCreatedEvent {
  type: 'session_created'
  sessionId: string
}

/** 会话标题已生成事件 */
export interface SessionTitleUpdatedEvent {
  type: 'session_title_updated'
  title: string
}

/** 所有进度事件类型 */
export type ProgressEvent =
  | RunStartedEvent
  | TaskCreatedEvent
  | TaskAssignedEvent
  | StepStartedEvent
  | StepCompletedEvent
  | StepRetryingEvent
  | ProgressUpdateEvent
  | TaskCompletedEvent
  | WaitingForInputEvent
  | ErrorEvent
  | SessionCreatedEvent
  | SessionTitleUpdatedEvent
  | SummaryTokenEvent
  | PerformancePlanEvent
  | LifeStateChangedEvent
  | PlannerDecisionEvent
  | StepDebugEvent

/** 进度回调函数 */
export type ProgressCallback = (event: ProgressEvent) => void

// 数据展示

/** 列定义 */
export interface ColumnDef {
  field: string
  title: string
  width?: number
  sortable: boolean
}

/** 数据展示类型提示 */
export type DataDisplayHint =
  | { type: 'table'; columns: ColumnDef[]; dataPath?: string }
  | { type: 'chart'; chartType: string; xField: string; yField: string }
  | {
      type: 'card_list'
      titleField: string
      descriptionField?: string
      imageField?: string
    }
  | { type: 'markdown' }
  | { type: 'key_value' }
  | { type: 'timeline'; timeField: string; contentField: string }
  | { type: 'raw' }

// 前端动作

/** 前端动作类型 */
export type FrontendActionType =
  | 'query_windows'
  | 'open_window'
  | 'close_window'
  | 'focus_window'
  | 'agent_interaction'
  | 'navigate'
  | 'page_interact'
  | 'brew_open_article'
  | 'music_control'
  | 'music_get_status'
  | 'music_load_playlist'
  | 'reading_list'

/** 窗口目标 */
export interface WindowTarget {
  windowId?: string
  tappId?: string
  tappName?: string
  position?: 'active' | 'left' | 'right' | 'next' | 'previous'
}

/** 页面元素目标 */
export interface PageElementTarget {
  selector?: string
  testId?: string
  text?: string
  ariaLabel?: string
  role?: string
  index?: number
}

/** 滚动选项 */
export interface ScrollOptions {
  direction?: 'top' | 'bottom' | 'left' | 'right'
  offset?: number
  smooth?: boolean
}

/** 等待条件 */
export interface WaitCondition {
  visible?: boolean
  timeout?: number
}

/** 阅读列表载荷 */
export interface ReadingListPayload {
  items?: Array<{
    id: number
    title: string
    source_name: string
    published_at: string
    reason?: string
  }>
  name?: string
}

/** 前端操作指令 */
export interface FrontendAction {
  type: FrontendActionType
  target?: WindowTarget | PageElementTarget
  tappId?: string
  windowId?: string
  interactionId?: string
  script?: string
  timestamp: number
  /** Multi-window geometry (ignored by single-window navigate fallback) */
  size?: { width?: number; height?: number }
  position?: { x?: number; y?: number }
  /** 操作数据 */
  data?: Record<string, unknown>
  /** 导航路径 */
  path?: string
  /** 路由参数 */
  params?: Record<string, unknown>
  /** 查询参数 */
  query?: Record<string, unknown>
  /** 完整路径 */
  fullPath?: string
  /** 控制动作 */
  action?: string
  /** 控制值 */
  value?: string | number | boolean
  /** 是否替换历史记录 */
  replace?: boolean
  /** 歌单ID */
  playlistId?: string
  /** 音乐来源 */
  source?: string
  /** 是否自动播放 */
  autoPlay?: boolean
  selector?: string
  /** 滚动选项 */
  scrollOptions?: ScrollOptions
  /** 等待条件 */
  waitFor?: WaitCondition
  /** 阅读列表载荷 */
  payload?: ReadingListPayload
  /** 筛选条件描述 */
  criteria?: string
}

// 能力定义

/** 能力定义 */
export interface Capability {
  id: string
  name: string
  description: string
  category: string
  actions: string[]
  requiresAi: boolean
}

// 任务预设

/** 预设类型 */
export type PresetType = 'favorite' | 'history'

/** 任务预设 */
export interface TaskPreset {
  id: number
  input: string
  presetType: PresetType
  parsedSteps?: unknown
  intentSummary?: string
  /** 对话标题（用于继续对话时显示） */
  title?: string
  /** 完整对话记录 - 支持「继续对话」模式 */
  conversationData?: ConversationMessage[]
  /** 是否有可继续的对话（conversationData 非空） */
  hasConversation?: boolean
  lastUsedAt: string
  useCount: number
  createdAt: string
}

/** 任务预设列表响应 */
export interface TaskPresetListResponse {
  favorites: TaskPreset[]
  history: TaskPreset[]
}

/** 创建任务预设请求 */
export interface CreatePresetRequest {
  input: string
  presetType: PresetType
  parsedSteps?: unknown
  intentSummary?: string
  /** 对话标题 */
  title?: string
  /** 对话记录（用于保存完整对话以支持「继续对话」） */
  conversationData?: ConversationMessage[]
}

// 会话相关

/** 对话消息（存储在 TaskPreset 中） */
export interface ConversationMessage {
  role: 'user' | 'assistant' | 'system'
  content: string
  metadata?: Record<string, unknown>
  createdAt: string
}

/** 会话信息 */
export interface SessionInfo {
  id: string
  title: string | null
  messageCount: number
  archived: boolean
  createdAt: string
  lastActiveAt: string
}

/** 会话消息 */
export interface SessionMessage {
  id: number
  role: 'user' | 'assistant' | 'system'
  content: string
  taskId?: string
  metadata?: Record<string, unknown>
  createdAt: string
}

// 队列状态 (Phase 1A)

/** 队列状态 */
export interface QueueStatus {
  total_lanes: number
  max_concurrent: number
  available_permits: number
  /** 正在排队等待许可的请求数 */
  waiting?: number
}

// Heartbeat (Phase 4)

/** Heartbeat 定时任务 */
export interface HeartbeatTask {
  id: string
  name: string
  schedule: string
  action: string
  enabled: boolean
  lastRun?: string
  lastResult?: string
}

// 执行追踪

/** 步骤追踪详情 */
export interface StepTrace {
  stepId: string
  capabilityId: string
  tierUsed: string
  durationMs: number
  tokensIn?: number
  tokensOut?: number
  success: boolean
  error?: string
  action?: string
  params?: Record<string, unknown>
  outputPreview?: string
  isDynamic?: boolean
}

/** 执行追踪 */
export interface ExecutionTrace {
  traceId: string
  totalDurationMs: number
  tierUsage: Record<string, number>
  steps: StepTrace[]
  plannerDecision?: {
    status: string
    reasoning?: string
    confidence: number
    plannedSteps: Array<{
      id: string
      capabilityId: string
      action: string
      params?: Record<string, unknown>
    }>
  }
}

// 记忆 (Phase 3)

/** 记忆条目 */
export interface MemoryEntry {
  id: string
  memoryType:
    | 'preference'
    | 'fact'
    | 'interaction'
    | 'decision'
    | 'entity_knowledge'
    | 'execution_lesson'
    | 'effective_pattern'
    | 'session_insight'
    | 'session_summary'
  content: string
  source?: string
  createdAt: string
  tier?: string
  importance?: number
  entities?: string[]
  relatedCapabilities?: string[]
}

// 技能 (Phase 2B)

/** 技能信息 */
export interface SkillInfo {
  id: string
  name: string
  description: string
  category: string
  origin: 'manual' | 'agent_generated' | 'agent_improved'
  successCount?: number
  failureCount?: number
  tierHint?: 'pro' | 'standard'
}

// 中断/转向 (Phase 1A)

/** 中断会话请求 */
export interface InterruptRequest {
  input: string
}

/** 转向会话请求 */
export interface SteerRequest {
  instruction: string
}

// 多 Agent (Phase 6)

/** Agent 配置信息 */
export interface AgentProfile {
  id: string
  role: AgentRole
  description: string
  defaultTier: string
  maxConcurrency: number
  capabilityPrefixes: string[]
}
