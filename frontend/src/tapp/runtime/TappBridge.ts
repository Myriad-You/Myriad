/**
 * Tapp Bridge - 消息桥接层
 * 负责主应用与 Tapp 沙箱之间的安全通信
 *
 * 安全特性：
 * - 严格的消息来源验证
 * - 细粒度权限检查（含用户角色验证）
 * - 输入、大小和时间戳验证
 * - 会话内请求 ID 防重放
 */

import type {
  TappAPIRequest,
  TappAPIResponse,
  TappInstance,
  TappMessage,
  TappPermission,
} from '../types'
import type { TappRuntimeGrant } from './TappRuntimeGrant'
import { getQuotaManager } from '../services/QuotaManager'
import { PERMISSION_MAP } from './permissionConfig'

type MessageHandler = (message: TappMessage) => Promise<TappAPIResponse>

/**
 * 生成唯一消息 ID（使用加密安全的随机数）
 */
function generateMessageId(): string {
  const array = new Uint8Array(16)
  crypto.getRandomValues(array)
  return `${Date.now()}-${Array.from(array)
    .map((b) => b.toString(16).padStart(2, '0'))
    .join('')}`
}

/**
 * 消息验证结果
 */
interface MessageValidationResult {
  valid: boolean
  error?: string
}

/**
 * 权限验证详情
 */
interface PermissionCheckResult {
  allowed: boolean
  reason?: string
  requiredPermission?: TappPermission | 'public'
}

// 仅浏览器侧执行、没有可强制权限的后端端点的能力才需要行前 authorize。
// speech/brew 走真实宿主端点并携带 Runtime Grant 头，由服务端就地强制。
const SERVER_AUTHORITATIVE_HOST_PERMISSIONS = new Set<TappPermission>([
  'media:control',
])

/**
 * Tapp Bridge 类
 * 处理主应用与沙箱之间的双向通信
 */
export class TappBridge {
  private iframe: HTMLIFrameElement | null = null
  private tappInstance: TappInstance | null = null
  private messageHandlers: Map<string, MessageHandler> = new Map()
  private eventListeners: Map<string, Set<(data: unknown) => void>> = new Map()
  private seenRequestIds = new Set<string>()

  /**
   * postMessage 目标 origin（发送消息用）
   * srcdoc iframe 未启用 allow-same-origin，origin 为 null
   * 浏览器不接受字符串 'null' 作为 postMessage 目标，使用 '*' 代替
   * 安全性由 event.source === iframe.contentWindow 检查保证
   */
  private postMessageTarget: string = '*'

  /** 会话 token（用于验证消息来源） */
  private sessionToken: string = ''

  /** Host-only backend identity for this concrete Page/Widget/headless runtime. */
  private runtimeGrant: TappRuntimeGrant | null = null

  constructor() {
    // 绑定消息处理器
    this.handleMessage = this.handleMessage.bind(this)
  }

  /**
   * 初始化 Bridge，连接到 iframe
   *
   * @param iframe - 沙箱 iframe 元素
   * @param tappInstance - Tapp 实例
   * @param sessionToken - 会话 token（用于消息验证）
   */
  initialize(
    iframe: HTMLIFrameElement,
    tappInstance: TappInstance,
    sessionToken?: string,
    runtimeGrant?: TappRuntimeGrant,
  ): void {
    this.iframe = iframe
    this.tappInstance = tappInstance
    this.runtimeGrant = runtimeGrant ?? null

    // 设置会话 token（如果未提供则生成一个）
    if (sessionToken) {
      this.sessionToken = sessionToken
    } else {
      // 生成安全的随机 token
      const array = new Uint8Array(32)
      crypto.getRandomValues(array)
      this.sessionToken = Array.from(array, (b) =>
        b.toString(16).padStart(2, '0'),
      ).join('')
    }

    // 监听消息
    window.addEventListener('message', this.handleMessage)
  }

  /**
   * 获取会话 token（供沙箱 HTML 生成时使用）
   */
  getSessionToken(): string {
    return this.sessionToken
  }

  async getRuntimeGrant(): Promise<string> {
    if (!this.runtimeGrant) {
      throw new Error('Tapp runtime grant is not initialized')
    }
    return this.runtimeGrant.getToken()
  }

  /**
   * 宿主 API 归因头：把 speech/brew 等宿主路径调用绑定到当前 Tapp 运行时，
   * 由服务端校验 Grant 并强制权限。预览模式没有 Grant，返回 undefined
   * （请求按宿主自身身份执行，与旧行为一致）。
   */
  async hostAttributionHeaders(): Promise<Record<string, string> | undefined> {
    if (!this.runtimeGrant) return undefined
    return { 'X-Tapp-Runtime-Grant': await this.runtimeGrant.getToken() }
  }

  async getRuntimeOwnerId(): Promise<number> {
    if (!this.runtimeGrant) {
      throw new Error('Tapp runtime grant is not initialized')
    }
    return this.runtimeGrant.getOwnerId()
  }

  async getRuntimeId(): Promise<string> {
    if (!this.runtimeGrant) {
      throw new Error('Tapp runtime grant is not initialized')
    }
    return this.runtimeGrant.getRuntimeId()
  }

  /**
   * 销毁 Bridge
   */
  destroy(): void {
    window.removeEventListener('message', this.handleMessage)

    this.messageHandlers.clear()
    this.eventListeners.clear()
    this.seenRequestIds.clear()

    this.iframe = null
    this.tappInstance = null
    this.runtimeGrant?.destroy()
    this.runtimeGrant = null
  }

  /**
   * 注册 API 处理器
   */
  registerHandler(action: string, handler: MessageHandler): void {
    this.messageHandlers.set(action, handler)
  }

  /**
   * 注销 API 处理器
   */
  unregisterHandler(action: string): void {
    this.messageHandlers.delete(action)
  }

  /**
   * 向 Tapp 发送事件
   */
  emit(event: string, data: unknown): void {
    if (!this.iframe?.contentWindow) {
      console.warn('[TappBridge] Cannot emit event: iframe not ready')
      return
    }

    const message: TappMessage = {
      type: 'event',
      id: generateMessageId(),
      action: event,
      payload: data,
      timestamp: Date.now(),
    }

    this.iframe.contentWindow.postMessage(message, this.postMessageTarget)
  }

  /**
   * 验证消息格式和内容
   */
  private validateMessage(message: unknown): MessageValidationResult {
    if (!message || typeof message !== 'object') {
      return { valid: false, error: 'Invalid message format' }
    }

    const msg = message as Record<string, unknown>

    // 必需字段检查
    if (!msg.type || typeof msg.type !== 'string') {
      return { valid: false, error: 'Missing or invalid type field' }
    }

    if (!msg.id || typeof msg.id !== 'string') {
      return { valid: false, error: 'Missing or invalid id field' }
    }

    // ID 格式验证（防止注入攻击）
    if (!/^[\w-]+$/.test(msg.id) || msg.id.length > 100) {
      return { valid: false, error: 'Invalid message ID format' }
    }

    // 类型验证
    // iframe -> host 方向只接受 API request 和轻量 event；response 仅由 host 发给 SDK。
    if (!['request', 'event'].includes(msg.type)) {
      return { valid: false, error: 'Unknown message type' }
    }

    // action 字段验证
    if (!msg.action || typeof msg.action !== 'string') {
      return { valid: false, error: 'Missing or invalid action field' }
    }

    // action 格式验证（防止注入攻击）
    if (msg.action && typeof msg.action === 'string') {
      if (!/^[\w.]+$/.test(msg.action) || msg.action.length > 50) {
        return { valid: false, error: 'Invalid action format' }
      }
    }

    if (
      typeof msg.timestamp !== 'number' ||
      !Number.isFinite(msg.timestamp) ||
      Math.abs(Date.now() - msg.timestamp) > 5 * 60 * 1000
    ) {
      return { valid: false, error: 'Invalid or stale timestamp' }
    }

    if (msg.type === 'request') {
      const payload = msg.payload as Record<string, unknown> | undefined
      if (
        !payload ||
        typeof payload.api !== 'string' ||
        typeof payload.method !== 'string' ||
        msg.action !== `${payload.api}.${payload.method}`
      ) {
        return { valid: false, error: 'Request action does not match payload' }
      }
    }

    // payload 大小检查（防止内存攻击）
    // 默认 1 MiB；action-specific higher caps (media / packages / chat) —
    // tens of MiB, not unbounded. Align with backend where applicable.
    if (msg.payload !== undefined) {
      if (msg.action === 'file.download') {
        // Align with multi-MB package / attachment downloads (32 MiB).
        const MAX_FILE_DOWNLOAD_BYTES = 32 * 1024 * 1024
        const args = (msg.payload as { args?: unknown[] }).args
        const options = args?.[0] as Record<string, unknown> | undefined
        if (
          !options ||
          typeof options.content !== 'string' ||
          new Blob([options.content]).size > MAX_FILE_DOWNLOAD_BYTES ||
          typeof options.filename !== 'string' ||
          options.filename.length > 1024 ||
          (options.mimeType !== undefined &&
            (typeof options.mimeType !== 'string' ||
              options.mimeType.length > 256))
        ) {
          return {
            valid: false,
            error: `Invalid or oversized file payload (max ${MAX_FILE_DOWNLOAD_BYTES} bytes)`,
          }
        }
      } else if (msg.action === 'federation.uploadMedia') {
        // Backend: image 10 MiB, video 50 MiB, route body 55 MiB.
        // Bridge carries data URL / base64 (~4/3 raw) + small JSON envelope.
        const MAX_MEDIA_RAW_BYTES = 50 * 1024 * 1024
        const MAX_MEDIA_DATA_CHARS =
          Math.ceil((MAX_MEDIA_RAW_BYTES * 4) / 3) + 256 // base64 + data-URL header
        const args = (msg.payload as { args?: unknown[] }).args
        const options = args?.[0] as Record<string, unknown> | undefined
        if (!options || typeof options !== 'object' || Array.isArray(options)) {
          return {
            valid: false,
            error: 'Invalid federation.uploadMedia payload',
          }
        }
        if (typeof options.data !== 'string') {
          return {
            valid: false,
            error:
              'Invalid federation.uploadMedia payload: data must be a string',
          }
        }
        if (options.data.length > MAX_MEDIA_DATA_CHARS) {
          return {
            valid: false,
            error: `Media data too large (max ${MAX_MEDIA_RAW_BYTES} bytes raw / ~${MAX_MEDIA_DATA_CHARS} chars base64)`,
          }
        }
        if (
          options.name !== undefined &&
          (typeof options.name !== 'string' || options.name.length > 1024)
        ) {
          return {
            valid: false,
            error: 'Invalid federation.uploadMedia payload: name',
          }
        }
        if (
          options.mime !== undefined &&
          (typeof options.mime !== 'string' || options.mime.length > 256)
        ) {
          return {
            valid: false,
            error: 'Invalid federation.uploadMedia payload: mime',
          }
        }
        if (
          options.media_type !== undefined &&
          (typeof options.media_type !== 'string' ||
            options.media_type.length > 256)
        ) {
          return {
            valid: false,
            error: 'Invalid federation.uploadMedia payload: media_type',
          }
        }
      } else if (
        msg.action === 'federation.sendMessage' ||
        msg.action === 'federation.sendRoomMessage' ||
        msg.action === 'tappList.install' ||
        msg.action === 'tappList.getInstallPackage'
      ) {
        // Channel/room MAX_MESSAGE_PAYLOAD = 32 MiB; direct install packages
        // and store-share snapshots may be multi-MB after JSON encoding.
        const MAX_PACKAGE_CHARS = 32 * 1024 * 1024 + 512 * 1024
        let payloadStr: string
        try {
          payloadStr = JSON.stringify(msg.payload)
        } catch {
          return { valid: false, error: 'Payload must be JSON-serializable' }
        }
        if (payloadStr.length > MAX_PACKAGE_CHARS) {
          return {
            valid: false,
            error: `Payload too large for ${msg.action} (max ~32 MiB; got ${payloadStr.length} chars)`,
          }
        }
      } else {
        let payloadStr: string
        try {
          payloadStr = JSON.stringify(msg.payload)
        } catch {
          return { valid: false, error: 'Payload must be JSON-serializable' }
        }
        // 1 MiB 业务值额外保留 JSON envelope 余量。
        if (payloadStr.length > 1024 * 1024 + 64 * 1024) {
          return {
            valid: false,
            error: `Payload too large (max ~1 MiB for ${msg.action || 'this action'}; use action-specific APIs for media/packages)`,
          }
        }
      }
    }

    // 会话 token 验证（增强安全性）
    // 对于 request 类型的消息，验证 session token
    if (msg.type === 'request' && this.sessionToken) {
      const sessionToken = msg._sessionToken as string | undefined
      if (sessionToken !== this.sessionToken) {
        console.warn(
          '[TappBridge] Session token mismatch - possible message spoofing',
        )
        return { valid: false, error: 'Invalid session token' }
      }
    }

    return { valid: true }
  }

  /**
   * 处理来自 Tapp 的消息（增强安全版本）
   */
  private async handleMessage(event: MessageEvent): Promise<void> {
    // srcdoc 沙箱的 origin 为 null；真实安全边界是具体 iframe WindowProxy。
    if (event.source !== this.iframe?.contentWindow) {
      return
    }

    // 验证消息格式
    const validation = this.validateMessage(event.data)
    if (!validation.valid) {
      console.warn(`[TappBridge] Invalid message: ${validation.error}`)
      const candidate = event.data as Record<string, unknown> | undefined
      if (
        candidate?.type === 'request' &&
        typeof candidate.id === 'string' &&
        /^[\w-]+$/.test(candidate.id) &&
        candidate.id.length <= 100
      ) {
        this.sendResponse(candidate.id, {
          success: false,
          error: validation.error || 'Invalid request',
          code: 'INVALID_REQUEST',
        })
      }
      return
    }

    const message = event.data as TappMessage

    if (message.type === 'request') {
      if (this.seenRequestIds.has(message.id)) return
      this.seenRequestIds.add(message.id)
      while (this.seenRequestIds.size > 2048) {
        const oldest = this.seenRequestIds.values().next().value
        if (oldest === undefined) break
        this.seenRequestIds.delete(oldest)
      }
    }

    switch (message.type) {
      case 'request':
        await this.handleRequest(message as TappMessage<TappAPIRequest>)
        break
      case 'event':
        this.handleEvent(message)
        break
    }
  }

  /**
   * 处理 API 请求（增强安全版本）
   */
  private async handleRequest(
    message: TappMessage<TappAPIRequest>,
  ): Promise<void> {
    const { id, payload } = message

    if (!payload || !payload.api || !payload.method) {
      this.sendResponse(id, {
        success: false,
        error: 'Invalid request format',
        code: 'INVALID_REQUEST',
      })
      return
    }

    // method 允许多级命名空间（例如 widget.instanceSettings.update）。
    const identifier = /^[a-z][a-z0-9]*$/i
    const namespacedMethod = /^[a-z][a-z0-9]*(?:\.[a-z][a-z0-9]*)*$/i
    if (
      !identifier.test(payload.api) ||
      !namespacedMethod.test(payload.method)
    ) {
      this.sendResponse(id, {
        success: false,
        error: 'Invalid API or method name format',
        code: 'INVALID_REQUEST',
      })
      return
    }

    const action = `${payload.api}.${payload.method}`

    // 配额检查
    if (this.tappInstance) {
      const quotaManager = getQuotaManager()
      const quotaCheck = quotaManager.checkQuota(this.tappInstance.id, action)
      if (!quotaCheck.allowed) {
        this.sendResponse(id, {
          success: false,
          error: quotaCheck.reason || 'Quota exceeded',
          code: 'QUOTA_EXCEEDED',
        })
        return
      }
    }

    // 权限检查（增强版）
    const permissionCheck = await this.checkPermissionDetailed(action)
    if (!permissionCheck.allowed) {
      this.sendResponse(id, {
        success: false,
        error: `Permission denied: ${permissionCheck.reason || action}`,
        code: 'PERMISSION_DENIED',
      })
      return
    }

    // 查找处理器
    const handler = this.messageHandlers.get(action)
    if (!handler) {
      this.sendResponse(id, {
        success: false,
        error: `Unknown action: ${action}`,
        code: 'UNKNOWN_ACTION',
      })
      return
    }

    try {
      const response = await handler(message)

      // 记录配额使用
      if (this.tappInstance && response.success) {
        const quotaManager = getQuotaManager()
        quotaManager.recordUsage(this.tappInstance.id, action)
      }

      this.sendResponse(id, response)
    } catch (error) {
      console.error(`[TappBridge] Handler error for ${action}:`, error)
      this.sendResponse(id, {
        success: false,
        error: error instanceof Error ? error.message : 'Internal error',
        code: 'HANDLER_ERROR',
      })
    }
  }

  /**
   * 处理事件
   */
  private handleEvent(message: TappMessage): void {
    const listeners = this.eventListeners.get(message.action)
    if (listeners) {
      for (const listener of listeners) {
        try {
          listener(message.payload)
        } catch (error) {
          console.error(`[TappBridge] Event listener error:`, error)
        }
      }
    }
  }

  /**
   * 发送响应到 Tapp
   */
  private sendResponse(requestId: string, response: TappAPIResponse): void {
    if (!this.iframe?.contentWindow) {
      console.warn('[TappBridge] Cannot send response: iframe not ready')
      return
    }

    const message: TappMessage<TappAPIResponse> = {
      type: 'response',
      id: requestId,
      action: 'response',
      payload: response,
      timestamp: Date.now(),
    }

    this.iframe.contentWindow.postMessage(message, this.postMessageTarget)
  }

  /**
   * 详细权限检查（返回检查结果和原因）
   *
   * 性能优化：使用模块级别的静态 Map 避免每次调用都创建对象
   */
  private async checkPermissionDetailed(
    action: string,
  ): Promise<PermissionCheckResult> {
    if (!this.tappInstance) {
      return { allowed: false, reason: 'Tapp instance not initialized' }
    }

    // 使用静态 Map 查找权限（O(1) 时间复杂度）
    const requiredPermission = PERMISSION_MAP.get(action)

    // 公开 API
    if (requiredPermission === 'public') {
      return { allowed: true, requiredPermission }
    }

    // 未知 action 默认拒绝
    if (!requiredPermission) {
      console.warn(
        `[TappBridge] Unknown action for permission check: ${action}`,
      )
      return { allowed: false, reason: `Unknown action: ${action}` }
    }

    // 检查是否已授权
    // 后端已经根据权限下放配置过滤了 grantedPermissions
    // 如果权限在列表中，说明后端已批准，前端无需再次验证角色
    const granted =
      this.tappInstance.grantedPermissions.includes(requiredPermission)
    if (!granted) {
      return {
        allowed: false,
        reason: `Missing permission: ${requiredPermission}`,
        requiredPermission,
      }
    }

    if (SERVER_AUTHORITATIVE_HOST_PERMISSIONS.has(requiredPermission)) {
      if (!this.runtimeGrant) {
        return {
          allowed: false,
          reason: 'Runtime grant is not initialized',
          requiredPermission,
        }
      }
      try {
        await this.runtimeGrant.authorize(requiredPermission)
      } catch {
        return {
          allowed: false,
          reason: `Permission was revoked: ${requiredPermission}`,
          requiredPermission,
        }
      }
    }

    return { allowed: true, requiredPermission }
  }

  /**
   * 监听来自 Tapp 的事件
   */
  on(event: string, callback: (data: unknown) => void): () => void {
    let listeners = this.eventListeners.get(event)
    if (!listeners) {
      listeners = new Set()
      this.eventListeners.set(event, listeners)
    }
    listeners.add(callback)

    // 返回取消监听的函数
    return () => {
      listeners?.delete(callback)
      if (listeners?.size === 0) this.eventListeners.delete(event)
    }
  }
}

/**
 * 创建 Bridge 实例
 */
export function createTappBridge(): TappBridge {
  return new TappBridge()
}
