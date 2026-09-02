/**
 * Agent 服务模块
 *
 * AI 驱动的自然语言任务编排系统前端接口
 */

// 导出类型
// 便捷函数

import type { AgentResponse, ProcessContext } from './types'

import { agentService } from './agentApi'

// 导出 API 服务
export { agentService } from './agentApi'

// 导出前端动作处理器
export type { FrontendActionHandler } from './frontendActions'

export {
  clearAllHandlers,
  executeFrontendAction,
  frontendActionDedupeKey,
  getRegisteredActionTypes,
  hasActionHandler,
  registerActionHandler,
  unregisterActionHandler,
} from './frontendActions'

/** Stream failures carry a `code`, so callers can single out quota rejections. */
export { AgentStreamError } from './sseTransport'
export type {
  AgentResponse,
  // 响应
  AgentResponseType,
  // 能力
  Capability,
  ClarificationPoint,
  // 澄清
  ClarificationType,
  ClarifyRequest,
  // 数据展示
  ColumnDef,
  ConfirmationInfo,
  ConfirmationStep,
  ConversationMessage,
  CreatePresetRequest,
  DataDisplayHint,
  ErrorEvent,
  // 执行追踪
  ExecutionTrace,
  FrontendAction,
  // 前端动作
  FrontendActionType,
  // Heartbeat
  HeartbeatTask,
  // 记忆
  MemoryEntry,
  MeropeStateChangedEvent,
  PageElementTarget,
  PerformanceDirective,
  PerformancePhase,
  PerformancePlanEvent,
  PlannerDecisionEvent,
  // 预设
  PresetType,
  // 上下文
  ProcessContext,
  ProcessRequest,
  ProgressCallback,
  ProgressEvent,
  ProgressUpdateEvent,

  // 队列
  QueueStatus,
  ReadingListPayload,
  ScrollOptions,
  // 会话
  SessionInfo,
  SessionMessage,
  // 技能
  SkillInfo,
  StepCompletedEvent,
  StepDebugEvent,
  StepRetryingEvent,
  StepStartedEvent,
  StepTrace,
  SummaryTokenEvent,
  TaskCompletedEvent,
  // SSE 事件
  TaskCreatedEvent,
  TaskDetail,
  TaskInfo,
  TaskPreset,
  TaskPresetListResponse,
  // 任务
  TaskStatus,
  ThinkingTokenEvent,
  WaitCondition,
  WaitingForInputEvent,
  WindowTarget,
} from './types'

/**
 * 快捷处理函数
 */
export async function ask(
  input: string,
  context?: Partial<ProcessContext>,
): Promise<AgentResponse> {
  return agentService.process(input, context)
}

/**
 * 对话处理函数（可以传入对话历史上下文）
 */
export async function chat(
  input: string,
  context?: Partial<ProcessContext>,
): Promise<AgentResponse> {
  return agentService.process(input, context)
}
