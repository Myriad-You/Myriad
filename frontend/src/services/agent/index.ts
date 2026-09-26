import type { AgentResponse, ProcessContext } from './types'

import { agentService } from './agentApi'

export { agentService } from './agentApi'

export type { FrontendActionHandler } from './frontendActions'

export {
  clearAllHandlers,
  executeFrontendAction,
  frontendActionDedupeKey,
  hasActionHandler,
  registerActionHandler,
  unregisterActionHandler,
} from './frontendActions'

/** Quota rejections use error.code. */
export { AgentStreamError } from './sseTransport'
export type {
  AgentResponse,
  AgentResponseType,
  Capability,
  ClarificationPoint,
  ClarificationType,
  ClarifyRequest,
  ColumnDef,
  ConversationMessage,
  CreatePresetRequest,
  DataDisplayHint,
  ErrorEvent,
  ExecutionTrace,
  FrontendAction,
  FrontendActionType,
  HeartbeatTask,
  ManagedMemory,
  MeropeStateChangedEvent,
  MusicControlEvent,
  OutfitOverlayEvent,
  PageElementTarget,
  PerformanceDirective,
  PerformancePhase,
  PerformancePlanEvent,
  PlannerDecisionEvent,
  PresetType,
  ProcessContext,
  ProcessRequest,
  ProgressCallback,
  ProgressEvent,
  ProgressUpdateEvent,

  QueueStatus,
  ReadingListPayload,
  ScrollOptions,
  SessionInfo,
  SessionMessage,
  SkillInfo,
  StepCompletedEvent,
  StepDebugEvent,
  StepRetryingEvent,
  StepStartedEvent,
  StepTrace,
  SummaryTokenEvent,
  TaskCompletedEvent,
  TaskCreatedEvent,
  TaskDetail,
  TaskInfo,
  TaskPreset,
  TaskPresetListResponse,
  TaskStatus,
  TaskStepHistoryItem,
  ThinkingTokenEvent,
  WaitCondition,
  WaitingForInputEvent,
  WindowTarget,
} from './types'

export async function ask(
  input: string,
  context?: Partial<ProcessContext>,
): Promise<AgentResponse> {
  return agentService.process(input, context)
}

export async function chat(
  input: string,
  context?: Partial<ProcessContext>,
): Promise<AgentResponse> {
  return agentService.process(input, context)
}
