import type { TappAPIResponse } from '../types'
import { hostLocaleHeaders } from '../../i18n/hostLocaleHeaders'
import { currentCopy } from '../../i18n/localeCopy'
import { getCSRFToken } from '../../utils/csrf'
import { httpStatusMessage, userFacingError } from '../../utils/userFacingError'
import { TappRuntimeGrant } from './TappRuntimeGrant'

export type ScheduleType = 'cron' | 'interval' | 'once' | 'daily'

export type ExecutionTarget = 'backend' | 'frontend' | 'both'

export type MissedPolicy = 'skip' | 'run-once' | 'run-all'

export type TaskScope = 'user' | 'tapp' | 'tapp-per-user' | 'global'

export type TaskExecutionStatus =
  'pending' | 'running' | 'success' | 'failed' | 'cancelled'

export interface ScheduleConfig {
  cron?: string
  interval?: number
  at?: number
  /** 每日 HH:mm 按 timezone 墙钟；默认 process local，不是 UTC。 */
  time?: string
  timezone?: string
}

export interface RetryConfig {
  maxRetries?: number
  retryDelay?: number
}

export type BackendAction =
  | { type: 'platform.sync'; platform: string }
  | { type: 'storage.set'; key: string; value: unknown }
  | { type: 'storage.get'; key: string }
  | { type: 'storage.delete'; key: string }
  | { type: 'ai.generate'; prompt: string }
  | {
      type: 'fetch'
      url: string
      method?: string
      headers?: Record<string, string>
      body?: unknown
    }
  | {
      type: 'notification.queue'
      title?: string
      message: string
      notificationType?: string
    }
  | {
      type: 'transform'
      input: string
      extract?: string
      template?: string
    }

export interface TaskRegistrationOptions {
  taskId: string
  name: string
  scheduleType: ScheduleType
  schedule: ScheduleConfig
  payload?: unknown
  executionTarget?: ExecutionTarget
  backendActions?: BackendAction[]
  missedPolicy?: MissedPolicy
  scope?: TaskScope
  retry?: RetryConfig
}

export interface RegisteredTask {
  id: number
  taskId: string
  tappId: string
  name: string
  scheduleType: ScheduleType
  schedule: ScheduleConfig
  payload?: unknown
  executionTarget: ExecutionTarget
  enabled: boolean
  missedPolicy: MissedPolicy
  scope: TaskScope
  nextRunAt?: string
  lastRunAt?: string
  lastRunResult?: unknown
  stats: {
    totalRuns: number
    successRuns: number
    failedRuns: number
    missedRuns: number
  }
  createdAt: string
}

export interface TaskExecutionEvent {
  type: 'task:execute'
  task: {
    id: number
    taskId: string
    tappId: string
    userId: number
    scope?: string
    scheduledAt?: string
    executedAt?: string
    isCompensation?: boolean
    payload?: unknown
  }
  payload?: unknown
  scheduledAt: string
  executionId: number
}

export type TaskCallback = (
  payload: unknown,
  event: TaskExecutionEvent,
) => void | Promise<void>

interface SchedulerWebSocketMessage {
  type: 'connected' | 'task:execute' | 'pong'
  user_id?: number
  message?: string
  task?: TaskExecutionEvent['task']
  payload?: unknown
  scheduledAt?: string
  executionId?: number
}

export class TappScheduler {
  private static instance: TappScheduler | null = null

  private ws: WebSocket | null = null

  private connected: boolean = false

  private reconnectTimer: ReturnType<typeof setTimeout> | null = null

  private reconnectInterval: number = 3000

  private maxReconnectAttempts: number = 10

  private reconnectAttempts: number = 0

  private heartbeatTimer: ReturnType<typeof setInterval> | null = null

  private heartbeatInterval: number = 30000

  private taskCallbacks: Map<string, Array<{ callback: TaskCallback }>> =
    new Map()

  private globalCallbacks: Set<TaskCallback> = new Set()

  private connectionListeners: Set<(connected: boolean) => void> = new Set()

  private apiBaseUrl: string = ''

  private authToken: string = ''

  private constructor() {}

  static getInstance(): TappScheduler {
    if (!TappScheduler.instance) {
      TappScheduler.instance = new TappScheduler()
    }
    return TappScheduler.instance
  }

  static reset(): void {
    TappScheduler.instance?.destroy()
    TappScheduler.instance = null
  }

  initialize(apiBaseUrl: string, authToken: string): void {
    this.apiBaseUrl = apiBaseUrl.replaceAll(/\/$/g, '')
    this.authToken = authToken

    this.connect()
  }

  destroy(): void {
    this.disconnect()
    this.taskCallbacks.clear()
    this.globalCallbacks.clear()
    this.connectionListeners.clear()
  }

  private connect(): void {
    if (this.ws) {
      this.disconnect()
    }

    let base = this.apiBaseUrl
    // 相对路径须补成绝对 ws(s) URL，否则 new WebSocket 会抛错。
    if (!/^https?:/i.test(base)) {
      const origin = typeof window !== 'undefined' ? window.location.origin : ''
      base = origin + (base.startsWith('/') ? base : `/${base}`)
    }
    const wsUrl = base
      .replaceAll(/^http/gi, 'ws')
      .replaceAll(/\/api$/g, '/api/tapp/scheduler/ws')

    try {
      this.ws = new WebSocket(wsUrl)

      this.ws.onopen = () => {
        console.log('[TappScheduler] WebSocket connected')
        this.connected = true
        this.reconnectAttempts = 0
        this.startHeartbeat()
        this.notifyConnectionChange(true)
      }

      this.ws.onmessage = (event) => {
        this.handleMessage(event.data)
      }

      this.ws.onclose = () => {
        console.log('[TappScheduler] WebSocket disconnected')
        this.connected = false
        this.stopHeartbeat()
        this.notifyConnectionChange(false)
        this.scheduleReconnect()
      }

      this.ws.onerror = (error) => {
        console.error('[TappScheduler] WebSocket error:', error)
      }
    } catch (error) {
      console.error('[TappScheduler] Failed to create WebSocket:', error)
      this.scheduleReconnect()
    }
  }

  private disconnect(): void {
    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer)
      this.reconnectTimer = null
    }

    this.stopHeartbeat()

    if (this.ws) {
      this.ws.onclose = null
      this.ws.close()
      this.ws = null
    }

    this.connected = false
  }

  private scheduleReconnect(): void {
    if (this.reconnectAttempts >= this.maxReconnectAttempts) {
      console.warn('[TappScheduler] Max reconnect attempts reached')
      return
    }

    if (this.reconnectTimer) {
      return
    }

    this.reconnectAttempts++
    const delay = this.reconnectInterval * Math.min(this.reconnectAttempts, 5)

    console.log(
      `[TappScheduler] Reconnecting in ${delay}ms (attempt ${this.reconnectAttempts})`,
    )

    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null
      this.connect()
    }, delay)
  }

  private startHeartbeat(): void {
    this.stopHeartbeat()
    this.heartbeatTimer = setInterval(() => {
      this.sendPing()
    }, this.heartbeatInterval)
  }

  private stopHeartbeat(): void {
    if (this.heartbeatTimer) {
      clearInterval(this.heartbeatTimer)
      this.heartbeatTimer = null
    }
  }

  private sendPing(): void {
    if (this.ws && this.connected) {
      try {
        this.ws.send(JSON.stringify({ type: 'ping' }))
      } catch {
      }
    }
  }

  private handleMessage(data: string): void {
    try {
      const message = JSON.parse(data) as SchedulerWebSocketMessage

      switch (message.type) {
        case 'connected':
          console.log('[TappScheduler] Server welcomed:', message.message)
          break

        case 'task:execute':
          this.handleTaskExecution(message)
          break

        case 'pong':
          break

        default:
          console.log('[TappScheduler] Unknown message type:', message)
      }
    } catch (error) {
      console.error('[TappScheduler] Failed to parse message:', error)
    }
  }

  private async handleTaskExecution(
    message: SchedulerWebSocketMessage,
  ): Promise<void> {
    if (!message.task) {
      return
    }

    const event: TaskExecutionEvent = {
      type: 'task:execute',
      task: message.task,
      payload: message.payload,
      scheduledAt: message.scheduledAt || new Date().toISOString(),
      executionId: message.executionId || 0,
    }

    const callbackKey = `${message.task.tappId}:${message.task.taskId}`
    const registrations = this.taskCallbacks.get(callbackKey)
    const callback = registrations?.at(-1)?.callback

    if (callback) {
      try {
        await callback(message.payload, event)
        this.reportTaskComplete(event.executionId, true)
      } catch (error) {
        console.error('[TappScheduler] Task callback error:', error)
        this.reportTaskComplete(
          event.executionId,
          false,
          userFacingError(error),
        )
      }
    } else {
      this.reportTaskComplete(
        event.executionId,
        false,
        currentCopy().errors.noticeScheduleFailed,
      )
    }

    for (const globalCallback of this.globalCallbacks) {
      try {
        await globalCallback(message.payload, event)
      } catch (error) {
        console.error('[TappScheduler] Global callback error:', error)
      }
    }
  }

  private reportTaskComplete(
    executionId: number,
    success: boolean,
    error?: string,
  ): void {
    if (this.ws && this.connected) {
      try {
        this.ws.send(
          JSON.stringify({
            type: 'task:complete',
            executionId,
            success,
            error,
          }),
        )
      } catch {
      }
    }
  }

  private notifyConnectionChange(connected: boolean): void {
    for (const listener of this.connectionListeners) {
      try {
        listener(connected)
      } catch {
      }
    }
  }

  isConnected(): boolean {
    return this.connected
  }

  onConnectionChange(callback: (connected: boolean) => void): () => void {
    this.connectionListeners.add(callback)
    return () => {
      this.connectionListeners.delete(callback)
    }
  }

  async registerTask(
    tappId: string,
    options: TaskRegistrationOptions,
    runtimeGrant?: string,
  ): Promise<RegisteredTask> {
    const response = await this.apiRequest<{
      success: boolean
      task: RegisteredTask
    }>(
      'POST',
      '/tasks',
      {
        tapp_id: tappId,
        task_id: options.taskId,
        name: options.name,
        schedule_type: options.scheduleType,
        schedule: options.schedule,
        payload: options.payload,
        execution_target: options.executionTarget || 'frontend',
        backend_actions: options.backendActions,
        missed_policy: options.missedPolicy || 'skip',
        scope: options.scope || 'user',
        retry: options.retry,
      },
      runtimeGrant,
    )

    if (!response.success) {
      throw new Error(currentCopy().errors.scheduleRegisterFailed)
    }

    return response.task
  }

  async unregisterTask(
    tappId: string,
    taskId: string,
    runtimeGrant?: string,
  ): Promise<void> {
    await this.apiRequest(
      'DELETE',
      `/${tappId}/tasks/${taskId}`,
      undefined,
      runtimeGrant,
    )
  }

  async listTasks(
    tappId?: string,
    runtimeGrant?: string,
  ): Promise<RegisteredTask[]> {
    const endpoint = tappId ? `/${tappId}/tasks` : '/tasks'
    const response = await this.apiRequest<{
      success: boolean
      tasks: RegisteredTask[]
    }>('GET', endpoint, undefined, runtimeGrant)
    return response.tasks || []
  }

  async getTask(
    tappId: string,
    taskId: string,
    runtimeGrant?: string,
  ): Promise<RegisteredTask | null> {
    try {
      const response = await this.apiRequest<{
        success: boolean
        task: RegisteredTask
      }>('GET', `/${tappId}/tasks/${taskId}`, undefined, runtimeGrant)
      return response.task || null
    } catch {
      return null
    }
  }

  async enableTask(
    tappId: string,
    taskId: string,
    runtimeGrant?: string,
  ): Promise<void> {
    await this.apiRequest(
      'POST',
      `/${tappId}/tasks/${taskId}/enable`,
      undefined,
      runtimeGrant,
    )
  }

  async disableTask(
    tappId: string,
    taskId: string,
    runtimeGrant?: string,
  ): Promise<void> {
    await this.apiRequest(
      'POST',
      `/${tappId}/tasks/${taskId}/disable`,
      undefined,
      runtimeGrant,
    )
  }

  async triggerTask(
    tappId: string,
    taskId: string,
    runtimeGrant?: string,
  ): Promise<void> {
    await this.apiRequest(
      'POST',
      `/${tappId}/tasks/${taskId}/trigger`,
      undefined,
      runtimeGrant,
    )
  }

  onTask(tappId: string, taskId: string, callback: TaskCallback): () => void {
    const key = `${tappId}:${taskId}`
    const registration = { callback }
    const registrations = this.taskCallbacks.get(key) ?? []
    registrations.push(registration)
    this.taskCallbacks.set(key, registrations)
    return () => {
      const current = this.taskCallbacks.get(key)
      if (!current) return
      const index = current.indexOf(registration)
      if (index < 0) return
      const next = current.toSpliced(index, 1)
      if (next.length === 0) this.taskCallbacks.delete(key)
      else this.taskCallbacks.set(key, next)
    }
  }

  onAnyTask(callback: TaskCallback): () => void {
    this.globalCallbacks.add(callback)
    return () => {
      this.globalCallbacks.delete(callback)
    }
  }

  private async apiRequest<T = TappAPIResponse>(
    method: string,
    endpoint: string,
    body?: unknown,
    runtimeGrant?: string,
    retryOnRuntimeGrant: boolean = true,
  ): Promise<T> {
    const url = `${this.apiBaseUrl}/tapp/scheduler${endpoint}`

    const headers: Record<string, string> = {
      'Content-Type': 'application/json',
      ...hostLocaleHeaders(),
    }

    if (this.authToken) {
      headers.Authorization = `Bearer ${this.authToken}`
    }
    if (runtimeGrant) {
      headers['X-Tapp-Runtime-Grant'] = runtimeGrant
    }
    const upper = method.toUpperCase()
    if (upper !== 'GET' && upper !== 'HEAD' && upper !== 'OPTIONS') {
      const csrf = (await getCSRFToken()) || ''
      if (csrf) headers['X-CSRF-Token'] = csrf
    }

    const response = await fetch(url, {
      method,
      headers,
      credentials: 'include',
      body: body ? JSON.stringify(body) : undefined,
    })

    if (!response.ok) {
      const error = await response
        .json()
        .catch(() => ({ error: currentCopy().errors.requestFailed }))
      if (
        response.status === 401 &&
        retryOnRuntimeGrant &&
        runtimeGrant &&
        (error.code === 'INVALID_RUNTIME_GRANT' ||
          error.code === 'RUNTIME_GRANT_SUBJECT_MISMATCH')
      ) {
        const replacement =
          await TappRuntimeGrant.recoverRejectedToken(runtimeGrant)
        if (replacement) {
          return this.apiRequest(method, endpoint, body, replacement, false)
        }
      }
      throw new Error(
        userFacingError(
          error.error || httpStatusMessage(response.status),
          currentCopy().errors.noticeScheduleFailed,
        ),
      )
    }

    return response.json()
  }
}

export function getTappScheduler(): TappScheduler {
  return TappScheduler.getInstance()
}

export default TappScheduler
