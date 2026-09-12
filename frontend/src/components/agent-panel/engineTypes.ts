import type { AgentAttachment } from './agentAttachments'

export interface ExecutionStep {
  id: string
  name: string
  status: 'pending' | 'running' | 'completed' | 'error'
  message?: string
  stepIndex?: number
  totalSteps?: number
  capabilityCategory?: string
  tierUsed?: 'pro' | 'standard'
  degraded?: boolean
  durationMs?: number
  retryAttempt?: number
  imageUrl?: string
}

export interface PendingQuestion {
  questionId: string
  confirmationId?: string
  questionType: string
  question: string
  context?: string
  options?: Array<{ value: string; label: string; description?: string }>
  required?: boolean
  defaultValue?: string
  riskLevel?: string
  expiresInSeconds?: number
  receivedAtMs?: number
  pendingSteps?: Array<{
    stepId: string
    capabilityName: string
    message: string
    impact: string[]
  }>
}

export interface ExecutionTrace {
  totalDurationMs: number
  tierUsage: Record<string, number>
  steps: Array<{
    stepId: string
    capabilityId: string
    tierUsed: string
    durationMs: number
    success: boolean
    error?: string
    action?: string
    params?: Record<string, unknown>
    outputPreview?: string
    isDynamic?: boolean
  }>
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

export interface MultiAgentAssignment {
  agents: Array<{
    role: string
    displayName: string
    icon: string
    tier: string
    capabilities: string[]
  }>
  totalAgents: number
  isMultiAgent: boolean
  tierMix: string
}

export interface DataDisplayHint {
  type: string
  [key: string]: unknown
}

export interface ChatMessage {
  id: string
  sessionId: string
  role: 'user' | 'assistant' | 'system'
  content: string
  createdAt: Date
  attachments?: AgentAttachment[]
  taskExecution?: TaskExecution
  suggestions?: string[]
  pendingQuestion?: PendingQuestion
  selectedAnswer?: string
  dataDisplay?: DataDisplayHint
  data?: unknown
  imageUrls?: string[]
}

export interface TaskExecution {
  taskId: string
  /** Re-subscribe after refresh; do not POST a new run. */
  runId?: string
  status: 'processing' | 'waiting' | 'cancelling' | 'completed' | 'error'
  progress: number
  steps: ExecutionStep[]
  executionTrace?: ExecutionTrace
  recalledMemories?: string[]
  skillId?: string
  skillName?: string
  assignment?: MultiAgentAssignment
  queuePosition?: number
  statusMessage?: string
  /** Same text as debugTrace; UI must not read the debug channel. */
  reasoning?: string
  planStepDescriptions?: string[]
  debugTrace?: DebugTrace
}

function historyString(value: unknown): string | undefined {
  return typeof value === 'string' && value.trim() ? value : undefined
}

/** Restore steps from terminal history if SSE dropped `step_started`. */
export function executionStepsFromHistory(
  history: ReadonlyArray<Record<string, unknown>> | undefined,
): ExecutionStep[] {
  if (!history?.length) return []
  return history.map((raw, index) => {
    const id =
      historyString(raw.stepId) ?? historyString(raw.step_id) ?? `hist-${index}`
    const name =
      historyString(raw.capabilityName) ??
      historyString(raw.capability_name) ??
      historyString(raw.description) ??
      id
    const statusRaw = historyString(raw.status)
    const status: ExecutionStep['status'] =
      statusRaw === 'failed' || statusRaw === 'error'
        ? 'error'
        : statusRaw === 'running'
          ? 'running'
          : statusRaw === 'pending'
            ? 'pending'
            : 'completed'
    const durationMs =
      typeof raw.durationMs === 'number'
        ? raw.durationMs
        : typeof raw.duration_ms === 'number'
          ? raw.duration_ms
          : undefined
    const note = historyString(raw.error) ?? historyString(raw.outputSummary)
    return {
      id,
      name,
      status,
      ...(typeof durationMs === 'number' ? { durationMs } : {}),
      ...(note ? { message: note } : {}),
    }
  })
}

export interface DebugTrace {
  plannerDecision?: {
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
  stepDebugEntries: StepDebugEntry[]
}

export interface StepDebugEntry {
  stepId: string
  capabilityId: string
  isDynamic: boolean
  directive?: string
  userRequest?: string
  params?: Record<string, unknown>
  outputPreview?: string
  durationMs?: number
  success?: boolean
  error?: string
}

export interface ChatSession {
  id: string
  mode?: 'work' | 'chat'
  title: string | null
  messageCount: number
  lastActiveAt: string
  createdAt?: string
}
