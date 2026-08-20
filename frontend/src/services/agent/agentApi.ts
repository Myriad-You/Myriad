/**
 * Agent API 服务
 *
 * 处理与后端 Agent API 的通信
 */

import type {
  AgentResponse,
  Capability,
  ClarifyRequest,
  CreatePresetRequest,
  ExecutionTrace,
  HeartbeatTask,
  MemoryEntry,
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

import { ApiError, apiService } from '../api'
import { abortSseSubscriptions, executeSSERequest } from './sseTransport'

/** Keep in sync with DIGITAL_LIFE_PROXY_TIMEOUT_MS in frontend/astro.config.mjs */
const PERSONA_GENERATION_TIMEOUT_MS = 15 * 60 * 1000

const personaGenerationInflight = new Map<string, Promise<unknown>>()

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

/** On-disk MCP server entry (`mcp_servers.json`). */
export interface McpServerConfig {
  id: string
  command: string
  args: string[]
  env: Record<string, string>
  enabled: boolean
  auto_restart: boolean
  max_restart_attempts: number
  /**
   * Honour the server's own tool annotations when classifying risk.
   * Optional: the backend defaults it to false, so an older instance omits it.
   */
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
  hasCustomPersona: boolean
  personality?: string
  mood?: number
  activity?: string
  doNotDisturb?: boolean
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

/** Exported for `mcpServerConfig.test.ts`. */
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
    enabled: o.enabled !== false,
    auto_restart: o.auto_restart !== false,
    max_restart_attempts:
      typeof o.max_restart_attempts === 'number' &&
      Number.isFinite(o.max_restart_attempts)
        ? Math.max(0, Math.min(50, Math.floor(o.max_restart_attempts)))
        : 3,
    // Must be parsed, not defaulted: the settings panel rebuilds each server
    // field by field, so dropping it here would show the switch off after a
    // reload and write `false` back on the next save — silently revoking the
    // operator's opt-in to trusting this server's tool annotations.
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

/**
 * BE IntentAction serializes as snake_case unit strings (`"query"`).
 * Unknown/newtype variants may appear as objects; coerce to stable strings.
 */
export function normalizeCapabilityActions(raw: unknown): string[] {
  if (!Array.isArray(raw)) return []
  const out: string[] = []
  for (const item of raw) {
    if (typeof item === 'string' && item.trim()) {
      out.push(item.trim())
      continue
    }
    if (item && typeof item === 'object') {
      // serde newtype: { "unknown": "foo" } or tagged forms
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

/**
 * Agent 服务类
 *
 * 负责与后端 Agent API 通信
 */
class AgentService {
  private baseUrl = '/agent'

  /** 当前 SSE 请求；回答问题时主流与回答流会同时存在。 */
  private activeAbortControllers = new Set<AbortController>()

  /**
   * 中断当前正在进行的 SSE 请求（客户端侧，用户意图）。
   *
   * 调用后 executeSSERequest 的 Promise 将 reject，且**不会**自动 re-subscribe 同一 run。
   */
  abortCurrentRequest(): void {
    abortSseSubscriptions(this.activeAbortControllers, 'user')
  }

  /**
   * 重新订阅一个已存在的后端 run（页面刷新 / 通知打开后恢复进度）。
   * 不会创建新任务。
   */
  async subscribeRun(
    runId: string,
    onProgress: ProgressCallback,
  ): Promise<AgentResponse> {
    return this.executeSSERequest(
      `/api${this.baseUrl}/runs/${encodeURIComponent(runId)}/stream`,
      'GET',
      undefined,
      onProgress,
      false,
    )
  }

  /**
   * 处理自然语言请求
   */
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
        timeout: 120000,
      },
    )
    return response
  }

  /**
   * 带实时进度更新的处理请求（SSE）
   */
  async processWithProgress(
    input: string,
    onProgress: ProgressCallback,
    context?: Partial<ProcessContext>,
  ): Promise<AgentResponse> {
    console.log('[AgentService] processWithProgress called with input:', input)

    const request: ProcessRequest = {
      input,
      context: {
        currentRoute: window.location.pathname,
        ...context,
      },
    }

    return this.executeSSERequest(
      `/api${this.baseUrl}/process/stream`,
      'POST',
      request,
      onProgress,
      false,
    )
  }

  /**
   * 提供澄清回答
   */
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

  /**
   * 确认或拒绝敏感操作
   */
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

  /**
   * 获取任务状态
   */
  async getTask(taskId: string): Promise<TaskDetail> {
    const response = await apiService.get<{
      success: boolean
      task: TaskInfo & {
        pendingQuestion?: TaskInfo['pendingQuestion']
      }
      results: Record<string, unknown>
      startedAt: string
      completedAt?: string
    }>(`${this.baseUrl}/tasks/${taskId}`)

    return {
      taskId: response.task.taskId,
      recipeId: '',
      status: response.task.status,
      progress: response.task.progress,
      startedAt: response.startedAt,
      completedAt: response.completedAt,
      results: response.results,
      pendingQuestion: response.task.pendingQuestion,
    }
  }

  /**
   * 获取用户的所有任务
   */
  async listTasks(): Promise<TaskDetail[]> {
    const response = await apiService.get<{
      success: boolean
      tasks: TaskDetail[]
      total: number
    }>(`${this.baseUrl}/tasks`)
    return response.tasks
  }

  /**
   * 取消任务
   */
  async cancelTask(
    taskId: string,
  ): Promise<{ success: boolean; message: string }> {
    return apiService.post<{
      success: boolean
      message: string
      taskId: string
    }>(`${this.baseUrl}/tasks/${taskId}/cancel`)
  }

  /**
   * 回答任务中的问题
   */
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

  /**
   * 回答任务中的问题（SSE 流式，带进度回调）
   */
  async answerQuestionWithProgress(
    taskId: string,
    questionId: string,
    answer: string,
    onProgress: ProgressCallback,
  ): Promise<AgentResponse> {
    // abortPrevious=false：回答订阅与任务 run 订阅可以并存；显式中断由 UI 单独触发。
    return this.executeSSERequest(
      `/api${this.baseUrl}/tasks/${taskId}/answer/stream`,
      'POST',
      { questionId, answer },
      onProgress,
      false,
    )
  }

  /**
   * 获取系统能力列表
   */
  async getCapabilities(): Promise<Capability[]> {
    const response = await apiService.get<{
      success: boolean
      capabilities: {
        capabilities?: Array<{
          id: string
          name: string
          description?: string
          category?: string
          /** BE IntentAction[] serializes as snake_case strings; tolerate objects */
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
    if (Array.isArray(body?.capabilities)) {
      return body.capabilities.map((cap) => ({
        id: cap.id,
        name: cap.name,
        description: cap.description || '',
        category: cap.category || '',
        actions: normalizeCapabilityActions(cap.actions),
        requiresAi: Boolean(cap.requiresAi ?? cap.requires_ai),
      }))
    }
    // Legacy: flatten byCategory summary if flat list missing
    const byCat = body?.byCategory
    if (byCat && typeof byCat === 'object') {
      return Object.entries(byCat).flatMap(([category, items]) =>
        (items || []).map((item) => ({
          id: item.id,
          name: item.name,
          description: item.hint || '',
          category,
          actions: [],
          requiresAi: Boolean(item.ai),
        })),
      )
    }
    return []
  }

  /**
   * 健康检查
   */
  async health(): Promise<{
    status: string
    service: string
    version: string
  }> {
    return apiService.get<{ status: string; service: string; version: string }>(
      `${this.baseUrl}/health`,
    )
  }

  /**
   * 轮询任务状态直到完成
   */
  async pollTaskUntilComplete(
    taskId: string,
    options: {
      intervalMs?: number
      timeoutMs?: number
      onProgress?: (task: TaskDetail) => void
    } = {},
  ): Promise<TaskDetail> {
    const { intervalMs = 1000, timeoutMs = 300000, onProgress } = options
    const startTime = Date.now()

    while (Date.now() - startTime < timeoutMs) {
      const task = await this.getTask(taskId)

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

    throw new Error(`Task ${taskId} timed out after ${timeoutMs}ms`)
  }

  // 任务预设 API

  /**
   * 获取任务预设列表
   */
  async getPresets(): Promise<TaskPresetListResponse> {
    return apiService.get<TaskPresetListResponse>(`${this.baseUrl}/presets`)
  }

  /**
   * 创建或更新任务预设
   */
  async createPreset(preset: CreatePresetRequest): Promise<TaskPreset> {
    return apiService.post<TaskPreset>(`${this.baseUrl}/presets`, preset)
  }

  /**
   * 添加到收藏
   */
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
      // 收藏不保存对话数据，始终为「重新运行」模式
    })
  }

  /**
   * 删除任务预设
   */
  async deletePreset(presetId: number): Promise<{ success: boolean }> {
    return apiService.delete<{ success: boolean }>(
      `${this.baseUrl}/presets/${presetId}`,
    )
  }

  /**
   * 切换收藏状态
   */
  async toggleFavorite(presetId: number): Promise<TaskPreset> {
    return apiService.post<TaskPreset>(
      `${this.baseUrl}/presets/${presetId}/toggle-favorite`,
    )
  }

  /**
   * 更新预设使用时间
   */
  async usePreset(presetId: number): Promise<TaskPreset> {
    return apiService.post<TaskPreset>(
      `${this.baseUrl}/presets/${presetId}/use`,
    )
  }

  /**
   * 执行预设任务（直接执行已保存的 recipe）
   */
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

  // 队列管理 (Phase 1A)

  /**
   * 获取队列状态
   */
  async getQueueStatus(): Promise<QueueStatus> {
    return apiService.get<QueueStatus>(`${this.baseUrl}/queue/status`)
  }

  /**
   * 中断当前会话，替换为新请求
   */
  async interruptSession(input: string): Promise<{
    success: boolean
    cancelled_tasks: number
    response: AgentResponse
  }> {
    return apiService.post(`${this.baseUrl}/session/interrupt`, { input })
  }

  /**
   * 向当前会话注入转向指令
   */
  async steerSession(
    instruction: string,
    taskId?: string,
  ): Promise<{
    success: boolean
    message: string
    taskId: string
    queued: boolean
  }> {
    return apiService.post(`${this.baseUrl}/session/steer`, {
      instruction,
      ...(taskId ? { taskId } : {}),
    })
  }

  // Heartbeat (Phase 4)

  /**
   * 获取 Heartbeat 任务列表
   */
  async getHeartbeatTasks(): Promise<HeartbeatTask[]> {
    const response = await apiService.get<{ tasks: HeartbeatTask[] }>(
      `${this.baseUrl}/heartbeat`,
    )
    return response.tasks
  }

  /**
   * 切换 Heartbeat 任务启停
   */
  async toggleHeartbeat(
    taskId: string,
  ): Promise<{ task_id: string; enabled: boolean }> {
    return apiService.post(`${this.baseUrl}/heartbeat/${taskId}/toggle`)
  }

  /**
   * 更新 Heartbeat 任务字段
   */
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

  /**
   * 创建 Heartbeat 任务
   */
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

  /**
   * 删除 Heartbeat 任务
   */
  async deleteHeartbeat(
    taskId: string,
  ): Promise<{ deleted: boolean; task_id: string }> {
    return apiService.delete(
      `${this.baseUrl}/heartbeat/${encodeURIComponent(taskId)}`,
    )
  }

  // 执行追踪

  /**
   * 获取执行追踪列表
   */
  async getTraces(
    limit: number = 20,
  ): Promise<{ traces: ExecutionTrace[]; total: number }> {
    return apiService.get(`${this.baseUrl}/traces?limit=${limit}`)
  }

  // 记忆 (Phase 3)

  /**
   * 获取记忆条目（通过 recall）
   */
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

  /**
   * 删除记忆条目
   */
  async deleteMemory(memoryId: string): Promise<void> {
    await apiService.delete(
      `${this.baseUrl}/memory/${encodeURIComponent(memoryId)}`,
    )
  }

  /**
   * 更新记忆条目内容
   */
  async updateMemory(memoryId: string, content: string): Promise<void> {
    await apiService.put(
      `${this.baseUrl}/memory/${encodeURIComponent(memoryId)}`,
      { content },
    )
  }

  // MCP

  /** Admin: MCP server connection status (`id`, `healthy`, `tool_count`, `auto_restart`). */
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

  /** Admin: full on-disk config + runtime status for the editor UI. */
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

  /** Admin: replace mcp_servers.json and hot-reload children. */
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

  /** Admin: hot-reload mcp_servers.json and restart stdio children */
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

  // 技能 (Phase 2B)

  /**
   * 获取可用技能列表
   */
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

  /**
   * 删除技能
   */
  async deleteSkill(skillId: string): Promise<void> {
    await apiService.delete(
      `${this.baseUrl}/skills/${encodeURIComponent(skillId)}`,
    )
  }

  // 设定引导（词条 / 命名 / 人设稿）

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
    selectedTags: string[]
    gender?: string
    avoidName?: string
    language: string
  }): Promise<{ name: string }> {
    return sharePersonaGeneration(
      `name:${body.language || ''}:${body.gender || ''}:${body.avoidName || ''}:${body.selectedTags.join(',')}`,
      () =>
        apiService.post(`${this.baseUrl}/persona/name`, body, {
          timeout: PERSONA_GENERATION_TIMEOUT_MS,
        }),
    )
  }

  async getPersona(): Promise<AgentPersona | null> {
    try {
      return await apiService.get(`${this.baseUrl}/persona`)
    } catch (error) {
      if (error instanceof ApiError && error.code === 'agent_life_disabled') {
        return null
      }
      throw error
    }
  }

  async putPersona(body: {
    name: string
    personality: string
    portraitAssetId?: string | null
  }): Promise<AgentPersona> {
    return apiService.put(`${this.baseUrl}/persona`, body)
  }

  async deletePersona(): Promise<void> {
    await apiService.delete(`${this.baseUrl}/persona`)
  }

  async putAddressee(body: { doNotDisturb: boolean }): Promise<{
    mood: number
    activity: string
    doNotDisturb: boolean
  }> {
    return apiService.put(`${this.baseUrl}/addressee`, body)
  }

  // 会话管理

  async createSession(): Promise<SessionInfo> {
    return apiService.post<SessionInfo>(`${this.baseUrl}/sessions`)
  }

  /**
   * 列出最近会话
   */
  async listSessions(
    page: number = 1,
    limit: number = 20,
  ): Promise<SessionInfo[]> {
    const response = await apiService.get<{ sessions: SessionInfo[] }>(
      `${this.baseUrl}/sessions?page=${page}&limit=${limit}`,
    )
    return response.sessions
  }

  /**
   * 获取会话消息
   */
  async getSessionMessages(
    sessionId: string,
    page: number = 1,
    limit: number = 50,
  ): Promise<SessionMessage[]> {
    const response = await apiService.get<{ messages: SessionMessage[] }>(
      `${this.baseUrl}/sessions/${sessionId}/messages?page=${page}&limit=${limit}`,
    )
    return response.messages
  }

  /**
   * 归档会话
   */
  async archiveSession(sessionId: string): Promise<{ success: boolean }> {
    return apiService.delete<{ success: boolean }>(
      `${this.baseUrl}/sessions/${sessionId}`,
    )
  }

  /**
   * 更新会话标题
   */
  async updateSessionTitle(
    sessionId: string,
    title: string,
  ): Promise<SessionInfo> {
    return apiService.patch<SessionInfo>(
      `${this.baseUrl}/sessions/${sessionId}`,
      { title },
    )
  }

  /**
   * AI 生成会话标题
   */
  async generateSessionTitle(sessionId: string): Promise<{ title: string }> {
    return apiService.post<{ title: string }>(
      `${this.baseUrl}/sessions/${sessionId}/generate-title`,
      {},
    )
  }

  // 内部方法

  /**
   * 执行 SSE 请求的通用方法
   *
   * @param abortPrevious - 是否中断前一个活跃请求（默认 true）。
   *   answerQuestion 与后台 run 的订阅需要并存，因此该场景传 false。
   */
  private async executeSSERequest(
    url: string,
    method: 'GET' | 'POST',
    body?: unknown,
    onProgress?: ProgressCallback,
    abortPrevious = true,
  ): Promise<AgentResponse> {
    return executeSSERequest({
      url,
      method,
      body,
      onProgress,
      abortPrevious,
      activeControllers: this.activeAbortControllers,
      pollTaskUntilComplete: (taskId, options) =>
        this.pollTaskUntilComplete(taskId, options),
    })
  }
}

// 导出单例
export const agentService = new AgentService()
