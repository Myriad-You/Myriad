import type {
  AgentResponse,
  Capability,
  ClarifyRequest,
  CreatePresetRequest,
  ExecutionTrace,
  HeartbeatTask,
  MemoryEntry,
  MoodTransition,
  ProcessContext,
  ProcessRequest,
  ProgressCallback,
  QueueStatus,
  SessionInfo,
  SessionMessage,
  SkillInfo,
  TaskDetail,
  TaskInfo,
  TaskPreset,
  TaskPresetListResponse,
} from './types'

import { currentCopy } from '../../i18n/localeCopy'
import { authSubject } from '../../utils/authSubject'
import { userFacingError } from '../../utils/userFacingError'
import { ApiError, apiService } from '../api'
import { abortSseSubscriptions, executeSSERequest } from './sseTransport'

/** Keep in sync with MEROPE_PROXY_TIMEOUT_MS. */
const PERSONA_GENERATION_TIMEOUT_MS = 15 * 60 * 1000
/** Must exceed backend NAME_CALL_TIMEOUT. */
const NAME_SUGGEST_TIMEOUT_MS = 6 * 60 * 1000

const personaGenerationInflight = new Map<string, Promise<unknown>>()

export type AgentIntentionStatus =
  | 'proposed'
  | 'accepted'
  | 'running'
  | 'waiting'
  | 'completed'
  | 'failed'
  | 'abandoned'
  | 'expired'

export interface AgentWorkProposal {
  title: string
  instruction: string
  expected_outcome: string
  source_event_id: string
}

export interface AgentIntention {
  id: string
  summary: string
  reason_code: string
  status: AgentIntentionStatus
  proposal: AgentWorkProposal
  created_at: string
  updated_at: string
  expires_at?: string
}

export interface QqPairingStatus {
  paired: boolean
  identityId: number | null
  openidMasked: string | null
  linkedAt: string | null
  pendingCode: string | null
  pendingExpiresAt: string | null
}

export type QqBotPhase =
  | 'offline'
  | 'connecting'
  | 'online'
  | 'rejected'
  | 'reconnecting'

export interface QqBotStatus {
  phase: QqBotPhase
  enabled: boolean
  hasAppId: boolean
  hasSecret: boolean
  appId?: string | null
  lastInboundAt?: string | null
}

export type TelegramBotPhase = QqBotPhase

export interface TelegramBotStatus {
  phase: TelegramBotPhase
  enabled: boolean
  hasToken: boolean
  botUsername?: string | null
  botName?: string | null
  lastInboundAt?: string | null
}

export type TelegramPairingStatus = QqPairingStatus

export type DiscordBotPhase = QqBotPhase

export interface DiscordBotStatus {
  phase: DiscordBotPhase
  enabled: boolean
  hasToken: boolean
  botUsername?: string | null
  botName?: string | null
  botUserId?: string | null
  lastInboundAt?: string | null
}

export type DiscordPairingStatus = QqPairingStatus

export type FeishuBotPhase = QqBotPhase

export interface FeishuBotStatus {
  phase: FeishuBotPhase
  enabled: boolean
  hasAppId: boolean
  hasSecret: boolean
  appId?: string | null
  lastInboundAt?: string | null
}

export type FeishuPairingStatus = QqPairingStatus

function sharePersonaGeneration<T>(
  key: string,
  start: () => Promise<T>,
): Promise<T> {
  const existing = personaGenerationInflight.get(key)
  if (existing) return existing as Promise<T>
  const promise = start().finally(() => {
    if (personaGenerationInflight.get(key) === promise) {
      personaGenerationInflight.delete(key)
    }
  })
  personaGenerationInflight.set(key, promise)
  return promise
}

export interface McpServerConfig {
  id: string
  command: string
  args: string[]
  env: Record<string, string>
  enabled: boolean
  auto_restart: boolean
  max_restart_attempts: number
  trust_annotations?: boolean
}

export interface McpRuntimeServer {
  id: string
  healthy: boolean
  tool_count: number
  auto_restart: boolean
}

export interface McpConfigSnapshot {
  servers: McpServerConfig[]
  configPath: string
  runtimeServers: McpRuntimeServer[]
  toolCount: number
}

export interface AgentPersona {
  name: string
  portraitAssetId: string | null
  avatarAssetId: string | null
  hasCustomPersona: boolean
  personality?: string
  persona?: Record<string, unknown> | null
  visualProfile?: Record<string, unknown> | null
  portraitGeneration?: Record<string, unknown> | null
  mood?: number
  moodRevision?: number
  arousal?: number
  activity?: string
  doNotDisturb?: boolean
  doNotDisturbActive?: boolean
  dndStart?: string | null
  dndEnd?: string | null
  reportCount?: number
}

function parseMcpRuntimeServers(
  raw: Array<Record<string, unknown>> | undefined,
): McpRuntimeServer[] {
  return (Array.isArray(raw) ? raw : [])
    .map((row) => {
      const id = typeof row.id === 'string' ? row.id.trim() : ''
      if (!id) return null
      return {
        id,
        healthy: row.healthy === true,
        tool_count:
          typeof row.tool_count === 'number' && Number.isFinite(row.tool_count)
            ? Math.max(0, Math.floor(row.tool_count))
            : 0,
        auto_restart: row.auto_restart === true,
      }
    })
    .filter((s): s is McpRuntimeServer => s != null)
}

export function parseMcpServerConfig(raw: unknown): McpServerConfig | null {
  if (!raw || typeof raw !== 'object') return null
  const o = raw as Record<string, unknown>
  const id = typeof o.id === 'string' ? o.id.trim() : ''
  const command = typeof o.command === 'string' ? o.command.trim() : ''
  if (!id || !command) return null
  const args = Array.isArray(o.args)
    ? o.args.filter((a): a is string => typeof a === 'string')
    : []
  const env: Record<string, string> = {}
  if (o.env && typeof o.env === 'object' && !Array.isArray(o.env)) {
    for (const [k, v] of Object.entries(o.env as Record<string, unknown>)) {
      if (typeof k === 'string' && typeof v === 'string') env[k] = v
    }
  }
  return {
    id,
    command,
    args,
    env,
    enabled: o.enabled === true,
    auto_restart: o.auto_restart !== false,
    max_restart_attempts:
      typeof o.max_restart_attempts === 'number' &&
      Number.isFinite(o.max_restart_attempts)
        ? Math.max(0, Math.min(50, Math.floor(o.max_restart_attempts)))
        : 3,
    // Must parse; defaulting false revokes tool-annotation opt-in on save.
    trust_annotations: o.trust_annotations === true,
  }
}

function normalizeMcpConfigSnapshot(response: {
  config?: { servers?: unknown }
  config_path?: string
  runtime?: {
    servers?: Array<Record<string, unknown>>
    tool_count?: number
  }
}): McpConfigSnapshot {
  const rawServers = response.config?.servers
  const servers = (Array.isArray(rawServers) ? rawServers : [])
    .map(parseMcpServerConfig)
    .filter((s): s is McpServerConfig => s != null)
  return {
    servers,
    configPath:
      typeof response.config_path === 'string' ? response.config_path : '',
    runtimeServers: parseMcpRuntimeServers(response.runtime?.servers),
    toolCount:
      typeof response.runtime?.tool_count === 'number'
        ? response.runtime.tool_count
        : 0,
  }
}

/** IntentAction is a snake_case string; coerce objects. */
export function normalizeCapabilityActions(raw: unknown): string[] {
  if (!Array.isArray(raw)) return []
  const out: string[] = []
  for (const item of raw) {
    if (typeof item === 'string' && item.trim()) {
      out.push(item.trim())
      continue
    }
    if (item && typeof item === 'object') {
      const entries = Object.entries(item as Record<string, unknown>)
      if (entries.length === 1) {
        const [k, v] = entries[0]
        out.push(typeof v === 'string' && v ? `${k}:${v}` : k)
        continue
      }
    }
  }
  return out
}

class AgentService {
  private baseUrl = '/agent'

  /** Chat abort must not drop an in-flight Work run. */
  private activeAbortControllersByMode: Record<
    'work' | 'chat',
    Set<AbortController>
  > = {
    work: new Set<AbortController>(),
    chat: new Set<AbortController>(),
  }

  /** Rejects; does not re-subscribe the same run. */
  abortCurrentRequest(mode?: 'work' | 'chat'): void {
    const lanes: Array<'work' | 'chat'> = mode ? [mode] : ['work', 'chat']
    for (const lane of lanes) {
      abortSseSubscriptions(this.activeAbortControllersByMode[lane], 'user')
    }
  }

  async listIntentions(): Promise<AgentIntention[]> {
    const response = await apiService.get<{ intentions: AgentIntention[] }>(
      `${this.baseUrl}/intentions`,
    )
    return response.intentions
  }

  async acceptIntention(
    intentionId: string,
  ): Promise<{
    intention: AgentIntention
    work: { mode: 'work'; input: string }
  }> {
    return apiService.post(
      `${this.baseUrl}/intentions/${encodeURIComponent(intentionId)}/accept`,
    )
  }

  async dismissIntention(
    intentionId: string,
  ): Promise<{ intention: AgentIntention }> {
    return apiService.post(
      `${this.baseUrl}/intentions/${encodeURIComponent(intentionId)}/dismiss`,
    )
  }

  async getAutonomyGrant(): Promise<{
    grant: {
      userId: number
      allowedPermissions: string[]
      revoked: boolean
    } | null
  }> {
    return apiService.get(`${this.baseUrl}/autonomy`)
  }

  async putAutonomyGrant(allowedPermissions: string[] = []): Promise<{
    grant: { userId: number; allowedPermissions: string[]; revoked: boolean }
  }> {
    return apiService.put(`${this.baseUrl}/autonomy`, { allowedPermissions })
  }

  async revokeAutonomyGrant(): Promise<{
    grant: { userId: number; allowedPermissions: string[]; revoked: boolean }
  }> {
    return apiService.delete(`${this.baseUrl}/autonomy`)
  }

  async getQqPairing(): Promise<{ pairing: QqPairingStatus }> {
    return apiService.get(`${this.baseUrl}/qq/pairing`)
  }

  async issueQqPairingCode(): Promise<{
    code: string
    expiresAt: string
    pairing: QqPairingStatus
  }> {
    return apiService.post(`${this.baseUrl}/qq/pairing`)
  }

  async unpairQq(): Promise<{ success: boolean }> {
    return apiService.delete(`${this.baseUrl}/qq/pairing`)
  }

  async getQqBotStatus(): Promise<QqBotStatus> {
    return apiService.get(`${this.baseUrl}/qq/status`)
  }

  async testQqBot(): Promise<{ success: boolean; phase?: QqBotPhase }> {
    return apiService.post(`${this.baseUrl}/qq/test`)
  }

  async getTelegramPairing(): Promise<{ pairing: TelegramPairingStatus }> {
    return apiService.get(`${this.baseUrl}/telegram/pairing`)
  }

  async issueTelegramPairingCode(): Promise<{
    code: string
    expiresAt: string
    pairing: TelegramPairingStatus
  }> {
    return apiService.post(`${this.baseUrl}/telegram/pairing`)
  }

  async unpairTelegram(): Promise<{ success: boolean }> {
    return apiService.delete(`${this.baseUrl}/telegram/pairing`)
  }

  async getTelegramBotStatus(): Promise<TelegramBotStatus> {
    return apiService.get(`${this.baseUrl}/telegram/status`)
  }

  async testTelegramBot(): Promise<{
    success: boolean
    phase?: TelegramBotPhase
    botUsername?: string | null
    botName?: string | null
  }> {
    return apiService.post(`${this.baseUrl}/telegram/test`)
  }

  async getDiscordPairing(): Promise<{ pairing: DiscordPairingStatus }> {
    return apiService.get(`${this.baseUrl}/discord/pairing`)
  }

  async issueDiscordPairingCode(): Promise<{
    code: string
    expiresAt: string
    pairing: DiscordPairingStatus
  }> {
    return apiService.post(`${this.baseUrl}/discord/pairing`)
  }

  async unpairDiscord(): Promise<{ success: boolean }> {
    return apiService.delete(`${this.baseUrl}/discord/pairing`)
  }

  async getDiscordBotStatus(): Promise<DiscordBotStatus> {
    return apiService.get(`${this.baseUrl}/discord/status`)
  }

  async testDiscordBot(): Promise<{
    success: boolean
    phase?: DiscordBotPhase
    botUsername?: string | null
    botName?: string | null
    botUserId?: string | null
  }> {
    return apiService.post(`${this.baseUrl}/discord/test`)
  }

  async getFeishuPairing(): Promise<{ pairing: FeishuPairingStatus }> {
    return apiService.get(`${this.baseUrl}/feishu/pairing`)
  }

  async issueFeishuPairingCode(): Promise<{
    code: string
    expiresAt: string
    pairing: FeishuPairingStatus
  }> {
    return apiService.post(`${this.baseUrl}/feishu/pairing`)
  }

  async unpairFeishu(): Promise<{ success: boolean }> {
    return apiService.delete(`${this.baseUrl}/feishu/pairing`)
  }

  async getFeishuBotStatus(): Promise<FeishuBotStatus> {
    return apiService.get(`${this.baseUrl}/feishu/status`)
  }

  async testFeishuBot(): Promise<{ success: boolean; phase?: FeishuBotPhase }> {
    return apiService.post(`${this.baseUrl}/feishu/test`)
  }

  /**
   * 重新订阅一个已存在的后端 run（页面刷新 / 通知打开后恢复进度）。
   * 不会创建新任务。
   */
  async subscribeRun(
    runId: string,
    onProgress: ProgressCallback,
    mode: 'chat' | 'work' = 'work',
    signal = authSubject.signal,
  ): Promise<AgentResponse> {
    return this.executeSSERequest(
      `/api${this.baseUrl}/runs/${encodeURIComponent(runId)}/stream`,
      'GET',
      undefined,
      onProgress,
      false,
      mode,
      signal,
    )
  }

  async process(
    input: string,
    context?: Partial<ProcessContext>,
  ): Promise<AgentResponse> {
    const request: ProcessRequest = {
      input,
      context: {
        currentRoute: window.location.pathname,
        ...context,
      },
    }

    const response = await apiService.post<AgentResponse>(
      `${this.baseUrl}/process`,
      request,
      {
        timeout: 5 * 60 * 1000,
      },
    )
    return response
  }

  async processWithProgress(
    input: string,
    onProgress: ProgressCallback,
    context?: Partial<ProcessContext>,
    signal = authSubject.signal,
  ): Promise<AgentResponse> {
    console.log('[AgentService] processWithProgress called with input:', input)

    const request: ProcessRequest = {
      input,
      context: {
        currentRoute: window.location.pathname,
        ...context,
      },
    }

    const lane = context?.mode === 'chat' ? 'chat' : 'work'
    return this.executeSSERequest(
      `/api${this.baseUrl}/process/stream`,
      'POST',
      request,
      onProgress,
      lane === 'chat',
      lane,
      signal,
    )
  }

  async clarify(
    originalInput: string,
    clarificationId: string,
    answer: string,
    context?: Partial<ProcessContext>,
  ): Promise<AgentResponse> {
    const request: ClarifyRequest = {
      originalInput,
      clarificationId,
      answer,
      context,
    }

    return apiService.post<AgentResponse>(`${this.baseUrl}/clarify`, request)
  }

  async confirmOperation(
    confirmationId: string,
    confirmed: boolean,
    note?: string,
    onProgress?: ProgressCallback,
  ): Promise<AgentResponse> {
    return this.executeSSERequest(
      `/api${this.baseUrl}/confirm/stream`,
      'POST',
      {
        confirmationId,
        confirmed,
        ...(note ? { note } : {}),
      },
      onProgress,
      false,
    )
  }

  async getTask(taskId: string, signal = authSubject.signal): Promise<TaskDetail> {
    const response = await apiService.get<{
      success: boolean
      task: TaskInfo & {
        pendingQuestion?: TaskInfo['pendingQuestion']
      }
      results: Record<string, unknown>
      startedAt: string
      completedAt?: string
    }>(`${this.baseUrl}/tasks/${taskId}`, { signal })

    return {
      taskId: response.task.taskId,
      recipeId: '',
      status: response.task.status,
      progress: response.task.progress,
      startedAt: response.startedAt,
      completedAt: response.completedAt,
      results: response.results,
      pendingQuestion: response.task.pendingQuestion,
      stepHistory: response.task.stepHistory,
    }
  }

  async listTasks(): Promise<TaskDetail[]> {
    const response = await apiService.get<{
      success: boolean
      tasks: TaskDetail[]
      total: number
    }>(`${this.baseUrl}/tasks`)
    return response.tasks
  }

  async cancelTask(
    taskId: string,
  ): Promise<{ success: boolean; message: string }> {
    return apiService.post<{
      success: boolean
      message: string
      taskId: string
    }>(`${this.baseUrl}/tasks/${taskId}/cancel`)
  }

  async submitFrontendAck(
    taskId: string,
    stepId: string,
    payload: {
      musicStatus?: unknown
      windowState?: unknown
    },
    signal?: AbortSignal,
  ): Promise<void> {
    try {
      await apiService.post(`${this.baseUrl}/tasks/${taskId}/frontend-ack`, {
        stepId,
        musicStatus: payload.musicStatus ?? null,
        windowState: payload.windowState ?? null,
      }, { signal })
    } catch {
    }
  }

  async answerQuestion(
    taskId: string,
    questionId: string,
    answer: string,
  ): Promise<AgentResponse> {
    return apiService.post<AgentResponse>(
      `${this.baseUrl}/tasks/${taskId}/answer`,
      { questionId, answer },
    )
  }

  async answerQuestionWithProgress(
    taskId: string,
    questionId: string,
    answer: string,
    onProgress: ProgressCallback,
  ): Promise<AgentResponse> {
    // abortPrevious=false: answer + run subscriptions may coexist.
    return this.executeSSERequest(
      `/api${this.baseUrl}/tasks/${taskId}/answer/stream`,
      'POST',
      { questionId, answer },
      onProgress,
      false,
    )
  }

  async getCapabilities(): Promise<Capability[]> {
    const response = await apiService.get<{
      success: boolean
      capabilities: {
        capabilities?: Array<{
          id: string
          name: string
          description?: string
          category?: string
          /** IntentAction is a snake_case string; coerce objects. */
          actions?: unknown
          requiresAi?: boolean
          requires_ai?: boolean
        }>
        totalCount?: number
        total?: number
        byCategory?: Record<
          string,
          Array<{ id: string; name: string; hint?: string; ai?: boolean }>
        >
      }
    }>(`${this.baseUrl}/capabilities`)
    const body = response.capabilities
    const capNames = currentCopy().agentCaps as Record<string, string>
    const capName = (id: string, fallback: string) =>
      capNames[id] || (fallback ? userFacingError(fallback) : '')
    const capDesc = (id: string, fallback: string) =>
      capNames[`${id}.desc`] || fallback || ''
    if (Array.isArray(body?.capabilities)) {
      return body.capabilities.map((cap) => ({
        id: cap.id,
        name: capName(cap.id, cap.name),
        description: capDesc(cap.id, cap.description || ''),
        category: cap.category ? userFacingError(cap.category) : '',
        actions: normalizeCapabilityActions(cap.actions),
        requiresAi: Boolean(cap.requiresAi ?? cap.requires_ai),
      }))
    }
    const byCat = body?.byCategory
    if (byCat && typeof byCat === 'object') {
      return Object.entries(byCat).flatMap(([category, items]) =>
        (items || []).map((item) => ({
          id: item.id,
          name: capName(item.id, item.name),
          description: capDesc(item.id, item.hint || ''),
          category: category ? userFacingError(category) : '',
          actions: [],
          requiresAi: Boolean(item.ai),
        })),
      )
    }
    return []
  }

  async health(): Promise<{
    status: string
    service: string
    version: string
  }> {
    return apiService.get<{ status: string; service: string; version: string }>(
      `${this.baseUrl}/health`,
    )
  }

  async pollTaskUntilComplete(
    taskId: string,
    options: {
      intervalMs?: number
      timeoutMs?: number
      onProgress?: (task: TaskDetail) => void
      signal?: AbortSignal
    } = {},
  ): Promise<TaskDetail> {
    const { intervalMs = 1000, timeoutMs = 300000, onProgress, signal = authSubject.signal } = options
    const startTime = Date.now()

    while (Date.now() - startTime < timeoutMs) {
      signal.throwIfAborted()
      const task = await this.getTask(taskId, signal)
      signal.throwIfAborted()

      if (onProgress) {
        onProgress(task)
      }

      if (
        task.status === 'completed' ||
        task.status === 'failed' ||
        task.status === 'cancelled' ||
        task.status === 'waiting_for_input'
      ) {
        return task
      }

      await new Promise((resolve) => setTimeout(resolve, intervalMs))
    }

    throw new Error(currentCopy().errors.agentStepTimeout)
  }

  async getPresets(): Promise<TaskPresetListResponse> {
    return apiService.get<TaskPresetListResponse>(`${this.baseUrl}/presets`)
  }

  async createPreset(preset: CreatePresetRequest): Promise<TaskPreset> {
    return apiService.post<TaskPreset>(`${this.baseUrl}/presets`, preset)
  }

  async addToFavorites(
    input: string,
    parsedSteps?: unknown,
    intentSummary?: string,
  ): Promise<TaskPreset> {
    return this.createPreset({
      input,
      presetType: 'favorite',
      parsedSteps,
      intentSummary,
      // Favorites never store chat; always rerun.
    })
  }

  async deletePreset(presetId: number): Promise<{ success: boolean }> {
    return apiService.delete<{ success: boolean }>(
      `${this.baseUrl}/presets/${presetId}`,
    )
  }

  async toggleFavorite(presetId: number): Promise<TaskPreset> {
    return apiService.post<TaskPreset>(
      `${this.baseUrl}/presets/${presetId}/toggle-favorite`,
    )
  }

  async usePreset(presetId: number): Promise<TaskPreset> {
    return apiService.post<TaskPreset>(
      `${this.baseUrl}/presets/${presetId}/use`,
    )
  }

  async executePreset(
    presetId: number,
    onProgress?: ProgressCallback,
  ): Promise<AgentResponse> {
    return this.executeSSERequest(
      `/api${this.baseUrl}/presets/${presetId}/execute`,
      'POST',
      undefined,
      onProgress,
    )
  }

  async getQueueStatus(): Promise<QueueStatus> {
    return apiService.get<QueueStatus>(`${this.baseUrl}/queue/status`)
  }

  async interruptSession(input: string): Promise<{
    success: boolean
    cancelled_tasks: number
    response: AgentResponse
  }> {
    return apiService.post(`${this.baseUrl}/session/interrupt`, { input })
  }

  async cancelChatTurn(sessionId: string): Promise<{ success: boolean }> {
    return apiService.post(`${this.baseUrl}/session/cancel-chat`, { sessionId })
  }

  async steerSession(
    instruction: string,
    taskId?: string,
    signal = authSubject.signal,
  ): Promise<{
    success: boolean
    message: string
    taskId: string
    queued: boolean
  }> {
    return apiService.post(`${this.baseUrl}/session/steer`, {
      instruction,
      ...(taskId ? { taskId } : {}),
    }, { signal })
  }

  async getHeartbeatTasks(): Promise<HeartbeatTask[]> {
    const response = await apiService.get<{ tasks: HeartbeatTask[] }>(
      `${this.baseUrl}/heartbeat`,
    )
    return response.tasks
  }

  async toggleHeartbeat(
    taskId: string,
  ): Promise<{ task_id: string; enabled: boolean }> {
    return apiService.post(`${this.baseUrl}/heartbeat/${taskId}/toggle`)
  }

  async updateHeartbeat(
    taskId: string,
    patch: {
      name?: string
      schedule?: string
      action?: string
      enabled?: boolean
    },
  ): Promise<HeartbeatTask> {
    const response = await apiService.put<{ task: HeartbeatTask }>(
      `${this.baseUrl}/heartbeat/${encodeURIComponent(taskId)}`,
      patch,
    )
    return response.task
  }

  async createHeartbeat(body: {
    name: string
    schedule: string
    action: string
    enabled?: boolean
    id?: string
  }): Promise<HeartbeatTask> {
    const response = await apiService.post<{ task: HeartbeatTask }>(
      `${this.baseUrl}/heartbeat`,
      body,
    )
    return response.task
  }

  async deleteHeartbeat(
    taskId: string,
  ): Promise<{ deleted: boolean; task_id: string }> {
    return apiService.delete(
      `${this.baseUrl}/heartbeat/${encodeURIComponent(taskId)}`,
    )
  }

  async getTraces(
    limit: number = 20,
  ): Promise<{ traces: ExecutionTrace[]; total: number }> {
    return apiService.get(`${this.baseUrl}/traces?limit=${limit}`)
  }

  async getMemories(): Promise<MemoryEntry[]> {
    try {
      const response = await apiService.get<{ memories: MemoryEntry[] }>(
        `${this.baseUrl}/memory`,
      )
      return response.memories
    } catch {
      return []
    }
  }

  async deleteMemory(memoryId: string): Promise<void> {
    await apiService.delete(
      `${this.baseUrl}/memory/${encodeURIComponent(memoryId)}`,
    )
  }

  async updateMemory(memoryId: string, content: string): Promise<void> {
    await apiService.put(
      `${this.baseUrl}/memory/${encodeURIComponent(memoryId)}`,
      { content },
    )
  }

  async getMcpStatus(): Promise<{
    servers: Array<{
      id: string
      healthy: boolean
      tool_count: number
      auto_restart: boolean
    }>
    tool_count: number
  }> {
    const response = await apiService.get<{
      servers?: Array<Record<string, unknown>>
      tool_count?: number
    }>(`${this.baseUrl}/mcp/status`)
    return {
      servers: parseMcpRuntimeServers(response.servers),
      tool_count:
        typeof response.tool_count === 'number' ? response.tool_count : 0,
    }
  }

  async getMcpConfig(): Promise<McpConfigSnapshot> {
    const response = await apiService.get<{
      config?: { servers?: unknown }
      config_path?: string
      runtime?: {
        servers?: Array<Record<string, unknown>>
        tool_count?: number
      }
    }>(`${this.baseUrl}/mcp/config`)
    return normalizeMcpConfigSnapshot(response)
  }

  async putMcpConfig(servers: McpServerConfig[]): Promise<McpConfigSnapshot> {
    const response = await apiService.put<{
      config?: { servers?: unknown }
      config_path?: string
      runtime?: {
        servers?: Array<Record<string, unknown>>
        tool_count?: number
      }
      error?: string
    }>(`${this.baseUrl}/mcp/config`, { servers })
    return normalizeMcpConfigSnapshot(response)
  }

  async reloadMcp(): Promise<{ reloaded: boolean; tool_count: number }> {
    const response = await apiService.post<{
      reloaded?: boolean
      tool_count?: number
    }>(`${this.baseUrl}/mcp/reload`, {})
    return {
      reloaded: response.reloaded === true,
      tool_count:
        typeof response.tool_count === 'number' ? response.tool_count : 0,
    }
  }

  async getSkills(): Promise<SkillInfo[]> {
    try {
      const response = await apiService.get<{ skills: SkillInfo[] }>(
        `${this.baseUrl}/skills`,
      )
      return response.skills
    } catch {
      return []
    }
  }

  async deleteSkill(skillId: string): Promise<void> {
    await apiService.delete(
      `${this.baseUrl}/skills/${encodeURIComponent(skillId)}`,
    )
  }

  async getPersonaSignals(body: {
    language: string
    regenerate?: boolean
  }): Promise<{
    reportCount: number
    tags: string[]
    aiDistilled: boolean
  }> {
    return sharePersonaGeneration(
      `signals:${body.language}:${body.regenerate === true}`,
      () =>
        apiService.post(
          `${this.baseUrl}/persona/signals`,
          {
            consent: true,
            language: body.language,
            regenerate: body.regenerate === true,
          },
          { timeout: PERSONA_GENERATION_TIMEOUT_MS },
        ),
    )
  }

  async importPersona(body: {
    source: string
    name?: string
    gender?: string
    language: string
  }): Promise<{
    persona: Record<string, unknown>
  }> {
    return sharePersonaGeneration(
      `import:${body.language || ''}:${body.name || ''}:${body.gender || ''}:${body.source.length}:${body.source.slice(0, 80)}`,
      () =>
        apiService.post(`${this.baseUrl}/persona/import`, body, {
          timeout: PERSONA_GENERATION_TIMEOUT_MS,
        }),
    )
  }

  async draftPersona(body: {
    name: string
    tags: string[]
    gender?: string
    extraRequirements?: string
    language: string
  }): Promise<{
    persona: Record<string, unknown>
  }> {
    return sharePersonaGeneration(
      `draft:${body.language || ''}:${body.name}:${body.gender || ''}:${body.extraRequirements || ''}:${body.tags.join(',')}`,
      () =>
        apiService.post(`${this.baseUrl}/persona/draft`, body, {
          timeout: PERSONA_GENERATION_TIMEOUT_MS,
        }),
    )
  }

  async suggestPersonaName(body: {
    gender?: string
    avoidName?: string
    nameStyle?: string
    language: string
  }): Promise<{ name: string }> {
    return sharePersonaGeneration(
      `name:${body.language || ''}:${body.nameStyle || ''}:${body.gender || ''}:${body.avoidName || ''}`,
      () =>
        apiService.post(`${this.baseUrl}/persona/name`, body, {
          timeout: NAME_SUGGEST_TIMEOUT_MS,
        }),
    )
  }

  async suggestPersonaVisualDesign(body: {
    gender: string
    language: string
    visualRequirements?: string
    clothingStyle: string
    keepCharacter?: boolean
    regenerate?: boolean
    existingVisualIdentity?: object
  }): Promise<{ visualIdentity: Record<string, unknown> }> {
    return sharePersonaGeneration(
      `visual-design:${body.language}:${body.gender}:${body.regenerate === true}:${body.keepCharacter === true}:${body.clothingStyle}:${body.visualRequirements || ''}:${JSON.stringify(body.existingVisualIdentity ?? null)}`,
      () =>
        apiService.post(`${this.baseUrl}/persona/visual-design`, body, {
          timeout: PERSONA_GENERATION_TIMEOUT_MS,
        }),
    )
  }

  async observeVisualFromPortrait(body: {
    gender: string
    language: string
  }): Promise<{
    visualIdentity: Record<string, unknown>
    clothingStyle: string
  }> {
    // Do not reuse sharePersonaGeneration across main-image changes.
    return apiService.post(`${this.baseUrl}/persona/visual-from-portrait`, body, {
      timeout: PERSONA_GENERATION_TIMEOUT_MS,
    })
  }

  async getPersona(): Promise<AgentPersona | null> {
    try {
      return await apiService.get(`${this.baseUrl}/persona`)
    } catch (error) {
      if (error instanceof ApiError && error.code === 'merope_disabled') {
        return null
      }
      throw error
    }
  }

  async putPersona(body: {
    name: string
    personality: string
    portraitAssetId?: string | null
    persona?: Record<string, unknown> | null
    visualProfile?: Record<string, unknown> | null
  }): Promise<AgentPersona> {
    return apiService.put(`${this.baseUrl}/persona`, body)
  }

  async deletePersona(): Promise<void> {
    await apiService.delete(`${this.baseUrl}/persona`)
  }

  async putAddressee(body: {
    doNotDisturb?: boolean
    dndStart?: string | null
    dndEnd?: string | null
  }): Promise<{
    mood: number
    arousal?: number
    activity: string
    doNotDisturb: boolean
    doNotDisturbActive?: boolean
    dndStart?: string | null
    dndEnd?: string | null
  }> {
    return apiService.put(`${this.baseUrl}/addressee`, body)
  }

  async creditMusicListening(listenedSeconds: number): Promise<{
    credited: boolean
    nextCreditInSeconds: number
    mood: MoodTransition
    activity: string
  }> {
    return apiService.post(`${this.baseUrl}/addressee/music-listening`, {
      listenedSeconds: Math.max(0, Math.floor(listenedSeconds)),
    })
  }

  async createSession(): Promise<SessionInfo> {
    return apiService.post<SessionInfo>(`${this.baseUrl}/sessions`)
  }

  async listSessions(
    page: number = 1,
    limit: number = 20,
  ): Promise<SessionInfo[]> {
    const response = await apiService.get<{ sessions: SessionInfo[] }>(
      `${this.baseUrl}/sessions?page=${page}&limit=${limit}`,
    )
    return response.sessions
  }

  async getSessionMessages(
    sessionId: string,
    page: number = 1,
    limit: number = 50,
    signal = authSubject.signal,
  ): Promise<SessionMessage[]> {
    const response = await apiService.get<{ messages: SessionMessage[] }>(
      `${this.baseUrl}/sessions/${sessionId}/messages?page=${page}&limit=${limit}`,
      { signal },
    )
    return response.messages
  }

  async archiveSession(sessionId: string): Promise<{ success: boolean }> {
    return apiService.delete<{ success: boolean }>(
      `${this.baseUrl}/sessions/${sessionId}`,
    )
  }

  async updateSessionTitle(
    sessionId: string,
    title: string,
  ): Promise<SessionInfo> {
    return apiService.patch<SessionInfo>(
      `${this.baseUrl}/sessions/${sessionId}`,
      { title },
    )
  }

  async generateSessionTitle(sessionId: string): Promise<{ title: string }> {
    return apiService.post<{ title: string }>(
      `${this.baseUrl}/sessions/${sessionId}/generate-title`,
      {},
    )
  }

  private async executeSSERequest(
    url: string,
    method: 'GET' | 'POST',
    body?: unknown,
    onProgress?: ProgressCallback,
    abortPrevious = true,
    lane: 'work' | 'chat' = 'work',
    signal = authSubject.signal,
  ): Promise<AgentResponse> {
    return executeSSERequest({
      signal,
      url,
      method,
      body,
      onProgress,
      abortPrevious,
      activeControllers: this.activeAbortControllersByMode[lane],
      pollTaskUntilComplete: (taskId, options) =>
        this.pollTaskUntilComplete(taskId, options),
    })
  }
}

export const agentService = new AgentService()
