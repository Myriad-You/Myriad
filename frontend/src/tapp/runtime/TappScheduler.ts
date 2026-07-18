/**
 * Tapp Scheduler - 定时任务调度器
 * 前端调度器，与后端调度引擎协同工作
 *
 * 混合架构：
 * - 后端负责调度逻辑和后端任务执行
 * - 前端通过 WebSocket 接收任务推送
 * - 前端执行 UI 相关任务（widget 更新、通知等）
 */

import type { TappAPIResponse } from '../types'
import { getCSRFToken } from '../../utils/csrf'
import { TappRuntimeGrant } from './TappRuntimeGrant'

// ============ 类型定义 ============

/** 调度类型 */
export type ScheduleType = 'cron' | 'interval' | 'once' | 'daily'

/** 执行目标 */
export type ExecutionTarget = 'backend' | 'frontend' | 'both'

/** 错过执行策略 */
export type MissedPolicy = 'skip' | 'run-once' | 'run-all'

/** 任务作用域 */
export type TaskScope = 'user' | 'tapp' | 'tapp-per-user' | 'global'

/** 任务执行状态 */
export type TaskExecutionStatus =
  'pending' | 'running' | 'success' | 'failed' | 'cancelled'

/** 调度配置 */
export interface ScheduleConfig {
  /** cron 表达式（type=cron 时） */
  cron?: string
  /** 间隔毫秒（type=interval 时） */
  interval?: number
  /** 执行时间戳（type=once 时） */
  at?: number
  /** 每日时间 HH:mm（type=daily 时） */
  time?: string
}

/** 重试配置 */
export interface RetryConfig {
  /** 最大重试次数 */
  maxRetries?: number
  /** 重试延迟（毫秒） */
  retryDelay?: number
}

/** 后端操作定义 */
export type BackendAction =
  | { type: 'platform.sync'; platform: string }
  | { type: 'storage.set'; key: string; value: unknown }
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

/** 任务注册选项 */
export interface TaskRegistrationOptions {
  /** 任务 ID（Tapp 内唯一） */
  taskId: string
  /** 任务名称 */
  name: string
  /** 调度类型 */
  scheduleType: ScheduleType
  /** 调度配置 */
  schedule: ScheduleConfig
  /** 任务负载（传递给回调的数据） */
  payload?: unknown
  /** 执行目标（默认：frontend） */
  executionTarget?: ExecutionTarget
  /** 后端操作列表（executionTarget=backend|both 时） */
  backendActions?: BackendAction[]
  /** 错过执行策略（默认：skip） */
  missedPolicy?: MissedPolicy
  /** 任务作用域（默认：user；跨用户作用域受后端权限约束） */
  scope?: TaskScope
  /** 重试配置 */
  retry?: RetryConfig
}

/** 已注册的任务信息 */
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

/** 任务执行事件 */
export interface TaskExecutionEvent {
  type: 'task:execute'
  task: {
    id: number
    taskId: string
    tappId: string
    userId: number
  }
  payload?: unknown
  scheduledAt: string
  executionId: number
}

/** 任务回调函数 */
export type TaskCallback = (
  payload: unknown,
  event: TaskExecutionEvent,
) => void | Promise<void>

/** WebSocket 消息类型 */
interface SchedulerWebSocketMessage {
  type: 'connected' | 'task:execute' | 'pong'
  user_id?: number
  message?: string
  task?: TaskExecutionEvent['task']
  payload?: unknown
  scheduledAt?: string
  executionId?: number
}

// ============ TappScheduler 类 ============

/**
 * Tapp 调度器
 * 管理定时任务的注册、接收和执行
 */
export class TappScheduler {
  private static instance: TappScheduler | null = null

  /** WebSocket 连接 */
  private ws: WebSocket | null = null

  /** 连接状态 */
  private connected: boolean = false

  /** 重连定时器 */
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null

  /** 重连间隔（毫秒） */
  private reconnectInterval: number = 3000

  /** 最大重连次数 */
  private maxReconnectAttempts: number = 10

  /** 当前重连次数 */
  private reconnectAttempts: number = 0

  /** 心跳定时器 */
  private heartbeatTimer: ReturnType<typeof setInterval> | null = null

  /** 心跳间隔（毫秒） */
  private heartbeatInterval: number = 30000

  /** 任务回调注册栈；同任务只执行最后挂载且仍存活的 runtime。 */
  private taskCallbacks: Map<string, Array<{ callback: TaskCallback }>> =
    new Map()

  /** 全局任务回调（接收所有任务） */
  private globalCallbacks: Set<TaskCallback> = new Set()

  /** 连接状态监听器 */
  private connectionListeners: Set<(connected: boolean) => void> = new Set()

  /** API 基础 URL */
  private apiBaseUrl: string = ''

  /** 认证 token */
  private authToken: string = ''

  private constructor() {}

  /**
   * 获取单例实例
   */
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

  /**
   * 初始化调度器
   * @param apiBaseUrl API 基础 URL
   * @param authToken 认证 token
   */
  initialize(apiBaseUrl: string, authToken: string): void {
    this.apiBaseUrl = apiBaseUrl.replace(/\/$/, '')
    this.authToken = authToken

    // 建立 WebSocket 连接
    this.connect()
  }

  /**
   * 销毁调度器
   */
  destroy(): void {
    this.disconnect()
    this.taskCallbacks.clear()
    this.globalCallbacks.clear()
    this.connectionListeners.clear()
  }

  /**
   * 建立 WebSocket 连接
   */
  private connect(): void {
    if (this.ws) {
      this.disconnect()
    }

    // 构建 WebSocket URL
    // apiBaseUrl 可能是绝对（https://host/api）或相对（/api，生产同源）。
    // 相对时用 window.location 补全为绝对 ws(s):// 地址，否则 new WebSocket 会抛错。
    let base = this.apiBaseUrl
    if (!/^https?:/i.test(base)) {
      const origin = typeof window !== 'undefined' ? window.location.origin : ''
      base = origin + (base.startsWith('/') ? base : `/${base}`)
    }
    const wsUrl = base
      .replace(/^http/i, 'ws')
      .replace(/\/api$/, '/api/tapp/scheduler/ws')

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

  /**
   * 断开 WebSocket 连接
   */
  private disconnect(): void {
    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer)
      this.reconnectTimer = null
    }

    this.stopHeartbeat()

    if (this.ws) {
      this.ws.onclose = null // 防止触发重连
      this.ws.close()
      this.ws = null
    }

    this.connected = false
  }

  /**
   * 安排重连
   */
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

  /**
   * 启动心跳
   */
  private startHeartbeat(): void {
    this.stopHeartbeat()
    this.heartbeatTimer = setInterval(() => {
      this.sendPing()
    }, this.heartbeatInterval)
  }

  /**
   * 停止心跳
   */
  private stopHeartbeat(): void {
    if (this.heartbeatTimer) {
      clearInterval(this.heartbeatTimer)
      this.heartbeatTimer = null
    }
  }

  /**
   * 发送心跳
   */
  private sendPing(): void {
    if (this.ws && this.connected) {
      try {
        this.ws.send(JSON.stringify({ type: 'ping' }))
      } catch {
        // 忽略发送错误
      }
    }
  }

  /**
   * 处理 WebSocket 消息
   */
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
          // 心跳响应，忽略
          break

        default:
          console.log('[TappScheduler] Unknown message type:', message)
      }
    } catch (error) {
      console.error('[TappScheduler] Failed to parse message:', error)
    }
  }

  /**
   * 处理任务执行事件
   */
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
    const callback = registrations?.[registrations.length - 1]?.callback

    // 执行特定任务回调
    if (callback) {
      try {
        await callback(message.payload, event)
        this.reportTaskComplete(event.executionId, true)
      } catch (error) {
        console.error('[TappScheduler] Task callback error:', error)
        this.reportTaskComplete(
          event.executionId,
          false,
          error instanceof Error ? error.message : 'Unknown error',
        )
      }
    } else {
      this.reportTaskComplete(
        event.executionId,
        false,
        'No active Tapp runtime callback',
      )
    }

    // 执行全局回调
    for (const globalCallback of this.globalCallbacks) {
      try {
        await globalCallback(message.payload, event)
      } catch (error) {
        console.error('[TappScheduler] Global callback error:', error)
      }
    }
  }

  /**
   * 报告任务完成状态
   */
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
        // 忽略发送错误
      }
    }
  }

  /**
   * 通知连接状态变化
   */
  private notifyConnectionChange(connected: boolean): void {
    for (const listener of this.connectionListeners) {
      try {
        listener(connected)
      } catch {
        // 忽略监听器错误
      }
    }
  }

  // ============ 公共 API ============

  /**
   * 检查是否已连接
   */
  isConnected(): boolean {
    return this.connected
  }

  /**
   * 监听连接状态变化
   */
  onConnectionChange(callback: (connected: boolean) => void): () => void {
    this.connectionListeners.add(callback)
    return () => {
      this.connectionListeners.delete(callback)
    }
  }

  /**
   * 注册定时任务
   */
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
      throw new Error('Failed to register task')
    }

    return response.task
  }

  /**
   * 注销定时任务
   */
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

  /**
   * 获取任务列表
   */
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

  /**
   * 获取单个任务
   */
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

  /**
   * 启用任务
   */
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

  /**
   * 禁用任务
   */
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

  /**
   * 手动触发任务
   */
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

  /**
   * 注册任务回调（特定任务）
   */
  onTask(tappId: string, taskId: string, callback: TaskCallback): () => void {
    const key = `${tappId}:${taskId}`
    const registration = { callback }
    const registrations = this.taskCallbacks.get(key) || []
    registrations.push(registration)
    this.taskCallbacks.set(key, registrations)
    return () => {
      const current = this.taskCallbacks.get(key)
      if (!current) return
      const index = current.indexOf(registration)
      if (index >= 0) current.splice(index, 1)
      if (current.length === 0) {
        this.taskCallbacks.delete(key)
      }
    }
  }

  /**
   * 注册全局任务回调（接收所有任务）
   */
  onAnyTask(callback: TaskCallback): () => void {
    this.globalCallbacks.add(callback)
    return () => {
      this.globalCallbacks.delete(callback)
    }
  }

  /**
   * API 请求辅助方法
   */
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
    }

    // 认证：与全站一致使用 cookie 会话 + CSRF（非 GET）。
    // authToken 保留为可选 Bearer（兼容 token 部署），但默认走 cookie。
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
        .catch(() => ({ error: 'Request failed' }))
      if (
        response.status === 401 &&
        retryOnRuntimeGrant &&
        runtimeGrant &&
        error.code === 'INVALID_RUNTIME_GRANT'
      ) {
        const replacement =
          await TappRuntimeGrant.recoverRejectedToken(runtimeGrant)
        if (replacement) {
          return this.apiRequest(method, endpoint, body, replacement, false)
        }
      }
      throw new Error(error.error || `HTTP ${response.status}`)
    }

    return response.json()
  }
}

/**
 * 获取调度器实例
 */
export function getTappScheduler(): TappScheduler {
  return TappScheduler.getInstance()
}

export default TappScheduler
