import type { RuntimeGrantKind } from '../services/TappApiService'
import type {
  TappAPIRequest,
  TappAPIResponse,
  TappInstance,
  TappMessage,
  TappPermission,
} from '../types'
import { userFacingError } from '../../utils/userFacingError'
import { getQuotaManager } from '../services/QuotaManager'
import {
  PREVIEW_UNAVAILABLE_CODE,
  previewUnavailableMessage,
} from '../utils/previewGrants'
import { TAPP_PACKAGE_PAYLOAD_BYTES } from '../utils/tappPackageLimits'
import {
  federationLiveLimits,
  federationMessageEnvelopeBytes,
  refreshFederationLimits,
} from './federationLimits'
import { PERMISSION_MAP } from './permissionConfig'
import { validateFileDownloadOptions } from './sandbox/fileDownload'
import { TappRuntimeGrant } from './TappRuntimeGrant'

type MessageHandler = (message: TappMessage) => Promise<TappAPIResponse>

const BRIDGE_LIMITS = {
  messagesPerMinute: 240,
  maxConcurrentRequests: 32,
  invalidBeforeMute: 40,
  invalidMuteMs: 10_000,
  timestampSkewMs: 2 * 60 * 1000,
} as const

function serializedUtf8Bytes(value: unknown): number | null {
  let serialized: string | undefined
  try {
    serialized = JSON.stringify(value)
  } catch {
    return null
  }
  if (serialized === undefined) return null
  let bytes = 0
  for (let index = 0; index < serialized.length; index += 1) {
    const code = serialized.charCodeAt(index)
    if (code < 0x80) {
      bytes += 1
    } else if (code < 0x800) {
      bytes += 2
    } else if (
      code >= 0xD800 &&
      code <= 0xDBFF &&
      index + 1 < serialized.length &&
      serialized.charCodeAt(index + 1) >= 0xDC00 &&
      serialized.charCodeAt(index + 1) <= 0xDFFF
    ) {
      bytes += 4
      index += 1
    } else {
      bytes += 3
    }
  }
  return bytes
}

class BridgeWindowCounter {
  private stamps: number[] = []
  constructor(
    private readonly windowMs: number,
    private readonly max: number,
  ) {}

  tryTake(now = Date.now()): boolean {
    this.prune(now)
    if (this.stamps.length >= this.max) return false
    this.stamps.push(now)
    return true
  }

  retryAfterMs(now = Date.now()): number {
    this.prune(now)
    if (this.stamps.length < this.max) return 0
    const oldest = this.stamps[0]
    if (oldest === undefined) return 0
    return Math.max(0, oldest + this.windowMs - now)
  }

  reset(): void {
    this.stamps = []
  }

  private prune(now: number): void {
    const cutoff = now - this.windowMs
    while (this.stamps.length > 0 && this.stamps[0]! <= cutoff) {
      this.stamps.shift()
    }
  }
}

function generateMessageId(): string {
  const array = new Uint8Array(16)
  crypto.getRandomValues(array)
  return `${Date.now()}-${Iterator.from(array)
    .map((b) => b.toString(16).padStart(2, '0'))
    .toArray()
    .join('')}`
}

interface MessageValidationResult {
  valid: boolean
  error?: string
}

interface PermissionCheckResult {
  allowed: boolean
  reason?: string
  requiredPermission?: TappPermission | 'public'
}

// 仅 media:control 在前端 authorize；speech/brew 走 Runtime Grant 头，由服务端强制。
const SERVER_AUTHORITATIVE_HOST_PERMISSIONS = new Set<TappPermission>([
  'media:control',
])

export interface TappBridgeInitOptions {
  /** destroy() 释放共享 grant 引用，不销毁 grant 本身。 */
  releaseSharedGrant?: () => void
  /** 主体重置后须返回新的 { grant, release }，以免引用计数错乱。 */
  reacquireSharedGrant?: () => {
    grant: TappRuntimeGrant
    release: () => void
  }
}

export class TappBridge {
  private iframe: HTMLIFrameElement | null = null
  private tappInstance: TappInstance | null = null
  private messageHandlers: Map<string, MessageHandler> = new Map()
  private eventListeners: Map<string, Set<(data: unknown) => void>> = new Map()
  private seenRequestIds = new Set<string>()

  /** 空直到 allowSandboxEvent；宿主须按 action 显式放行。 */
  private allowedSandboxEvents = new Set<string>()

  private readonly inboundRate = new BridgeWindowCounter(
    60_000,
    BRIDGE_LIMITS.messagesPerMinute,
  )

  private inFlightRequests = 0
  private invalidCount = 0
  private mutedUntil = 0

  /**
   * srcdoc 无 allow-same-origin，origin 为 null；postMessage 目标用 *。边界是 event.source
   * === iframe.contentWindow。
   */
  private postMessageTarget: string = '*'

  private sessionToken: string = ''

  /** destroy 后为 true；宿主快捷键视桥为死。 */
  private destroyed = false

  /** 宿主隐藏此表面时为 false；快捷键跳过。由 paused / lifecycle 设置。 */
  private surfaceActive = true

  private runtimeGrant: TappRuntimeGrant | null = null

  /** 主体重置后用这些参数重铸 grant。已销毁的 grant 不能发 token。 */
  private grantSeed: {
    tappId: string
    instanceId: string
    kind: RuntimeGrantKind
  } | null = null

  private releaseSharedGrant: (() => void) | null = null
  private reacquireSharedGrant:
    | (() => {
        grant: TappRuntimeGrant
        release: () => void
      })
    | null = null

  private registeredSource: MessageEventSource | null = null

  /** 窗口级单路由。路由键仍是 event.source === iframe.contentWindow。 */
  private static readonly bridgesBySource = new Map<
    MessageEventSource,
    TappBridge
  >()

  private static readonly activeBridges = new Set<TappBridge>()
  private static sharedListenerAttached = false

  private static ensureSharedListener(): void {
    if (TappBridge.sharedListenerAttached) return
    if (typeof window === 'undefined') return
    window.addEventListener('message', TappBridge.onSharedWindowMessage)
    TappBridge.sharedListenerAttached = true
  }

  private static maybeDetachSharedListener(): void {
    if (TappBridge.activeBridges.size > 0) return
    if (!TappBridge.sharedListenerAttached) return
    if (typeof window === 'undefined') return
    window.removeEventListener('message', TappBridge.onSharedWindowMessage)
    TappBridge.sharedListenerAttached = false
  }

  /** 先走 source 映射；srcdoc 竞态 attachSource 时回退扫描 contentWindow，以免丢掉早期 tapp.ready。 */
  private static resolveBridgeForSource(
    source: MessageEventSource,
  ): TappBridge | null {
    const mapped = TappBridge.bridgesBySource.get(source)
    if (mapped) return mapped
    for (const bridge of TappBridge.activeBridges) {
      const win = bridge.iframe?.contentWindow
      if (win && win === source) {
        bridge.attachSource()
        return bridge
      }
    }
    return null
  }

  private static onSharedWindowMessage(event: MessageEvent): void {
    const source = event.source
    if (!source) return
    const bridge = TappBridge.resolveBridgeForSource(source)
    if (!bridge) return
    void bridge.handleMessage(event)
  }

  constructor() {
    this.handleMessage = this.handleMessage.bind(this)
    void refreshFederationLimits()
  }

  initialize(
    iframe: HTMLIFrameElement,
    tappInstance: TappInstance,
    sessionToken?: string,
    runtimeGrant?: TappRuntimeGrant,
    options?: TappBridgeInitOptions,
  ): void {
    this.destroyed = false
    this.surfaceActive = true
    this.iframe = iframe
    this.tappInstance = tappInstance
    this.runtimeGrant = runtimeGrant ?? null
    this.releaseSharedGrant = options?.releaseSharedGrant ?? null
    this.reacquireSharedGrant = options?.reacquireSharedGrant ?? null
    this.grantSeed = runtimeGrant
      ? {
          tappId: runtimeGrant.getTappId(),
          instanceId: runtimeGrant.getInstanceId(),
          kind: runtimeGrant.getKind(),
        }
      : null

    if (sessionToken) {
      this.sessionToken = sessionToken
    } else {
      const array = new Uint8Array(32)
      crypto.getRandomValues(array)
      this.sessionToken = Iterator.from(array)
        .map((b) => b.toString(16).padStart(2, '0'))
        .toArray()
        .join('')
    }

    TappBridge.ensureSharedListener()
    TappBridge.activeBridges.add(this)
    this.attachSource()
  }

  /** iframe 入 DOM 后再登记；srcdoc 解析可能重建 window。 */
  attachSource(): void {
    const win = this.iframe?.contentWindow
    if (!win) return
    if (this.registeredSource && this.registeredSource !== win) {
      TappBridge.bridgesBySource.delete(this.registeredSource)
    }
    this.registeredSource = win
    TappBridge.bridgesBySource.set(win, this)
  }

  /** destroy 后为空；宿主快捷键视桥为死。 */
  getSessionToken(): string {
    return this.destroyed ? '' : this.sessionToken
  }

  isDestroyed(): boolean {
    return this.destroyed
  }

  setSurfaceActive(active: boolean): void {
    this.surfaceActive = active
  }

  /** 已销毁、宿主暂停、iframe 断开或 CSS 隐藏时为 false。 */
  isSurfaceActive(): boolean {
    if (this.destroyed || !this.surfaceActive) return false
    const iframe = this.iframe
    if (!iframe || !iframe.isConnected) return false
    try {
      if (typeof window !== 'undefined') {
        const style = window.getComputedStyle(iframe)
        if (style.display === 'none' || style.visibility === 'hidden') {
          return false
        }
      }
    } catch {
    }
    return true
  }

  /** AuthContext destroyAll 后旧 grant 已死；按 seed 重铸。 */
  private ensureLiveRuntimeGrant(): TappRuntimeGrant {
    if (this.runtimeGrant && !this.runtimeGrant.isDestroyed()) {
      return this.runtimeGrant
    }
    if (this.reacquireSharedGrant) {
      // 共享 widget grant：destroyAll 后重新入池，引用计数不跨主体泄漏。
      const previousRelease = this.releaseSharedGrant
      this.releaseSharedGrant = null
      if (previousRelease) {
        try {
          previousRelease()
        } catch {
        }
      }
      const next = this.reacquireSharedGrant()
      this.runtimeGrant = next.grant
      this.releaseSharedGrant = next.release
      this.grantSeed = {
        tappId: this.runtimeGrant.getTappId(),
        instanceId: this.runtimeGrant.getInstanceId(),
        kind: this.runtimeGrant.getKind(),
      }
      return this.runtimeGrant
    }
    if (!this.grantSeed) {
      throw new Error('Tapp runtime grant is not initialized')
    }
    this.runtimeGrant = new TappRuntimeGrant(
      this.grantSeed.tappId,
      this.grantSeed.instanceId,
      this.grantSeed.kind,
    )
    return this.runtimeGrant
  }

  async getRuntimeGrant(): Promise<string> {
    return this.ensureLiveRuntimeGrant().getToken()
  }

  /** speech/brew 等宿主路径带 Runtime Grant 头。预览无 Grant，返回 undefined。 */
  async hostAttributionHeaders(): Promise<Record<string, string> | undefined> {
    if (!this.runtimeGrant && !this.grantSeed) return undefined
    return {
      'X-Tapp-Runtime-Grant': await this.getRuntimeGrant(),
    }
  }

  async getRuntimeOwnerId(): Promise<number> {
    return this.ensureLiveRuntimeGrant().getOwnerId()
  }

  async getRuntimeId(): Promise<string> {
    return this.ensureLiveRuntimeGrant().getRuntimeId()
  }

  destroy(): void {
    this.destroyed = true
    this.surfaceActive = false
    this.sessionToken = ''

    TappBridge.activeBridges.delete(this)
    if (this.registeredSource) {
      const mapped = TappBridge.bridgesBySource.get(this.registeredSource)
      if (mapped === this) {
        TappBridge.bridgesBySource.delete(this.registeredSource)
      }
      this.registeredSource = null
    }
    TappBridge.maybeDetachSharedListener()

    this.messageHandlers.clear()
    this.eventListeners.clear()
    this.seenRequestIds.clear()
    this.allowedSandboxEvents.clear()
    this.inboundRate.reset()
    this.inFlightRequests = 0
    this.invalidCount = 0
    this.mutedUntil = 0

    this.iframe = null
    this.tappInstance = null
    if (this.releaseSharedGrant) {
      this.releaseSharedGrant()
    } else {
      this.runtimeGrant?.destroy()
    }
    this.runtimeGrant = null
    this.grantSeed = null
    this.releaseSharedGrant = null
    this.reacquireSharedGrant = null
  }

  registerHandler(action: string, handler: MessageHandler): void {
    this.messageHandlers.set(action, handler)
  }

  unregisterHandler(action: string): void {
    this.messageHandlers.delete(action)
  }

  allowSandboxEvent(action: string): void {
    if (!/^[\w.]+$/.test(action) || action.length > 50) {
      throw new Error(`Invalid sandbox event action: ${action}`)
    }
    this.allowedSandboxEvents.add(action)
  }

  disallowSandboxEvent(action: string): void {
    this.allowedSandboxEvents.delete(action)
  }

  isSandboxEventAllowed(action: string): boolean {
    return this.allowedSandboxEvents.has(action)
  }

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

  private validateMessage(message: unknown): MessageValidationResult {
    if (!message || typeof message !== 'object') {
      return { valid: false, error: 'Invalid message format' }
    }

    const msg = message as Record<string, unknown>

    if (!msg.type || typeof msg.type !== 'string') {
      return { valid: false, error: 'Missing or invalid type field' }
    }

    if (!msg.id || typeof msg.id !== 'string') {
      return { valid: false, error: 'Missing or invalid id field' }
    }

    if (!/^[\w-]+$/.test(msg.id) || msg.id.length > 100) {
      return { valid: false, error: 'Invalid message ID format' }
    }

    // iframe→host 只接受 request 与轻量 event；response 仅 host→SDK。
    if (!['request', 'event'].includes(msg.type)) {
      return { valid: false, error: 'Unknown message type' }
    }

    if (!msg.action || typeof msg.action !== 'string') {
      return { valid: false, error: 'Missing or invalid action field' }
    }

    if (msg.action && typeof msg.action === 'string') {
      if (!/^[\w.]+$/.test(msg.action) || msg.action.length > 50) {
        return { valid: false, error: 'Invalid action format' }
      }
    }

    if (
      typeof msg.timestamp !== 'number' ||
      !Number.isFinite(msg.timestamp) ||
      Math.abs(Date.now() - msg.timestamp) > BRIDGE_LIMITS.timestampSkewMs
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

    if (msg.payload !== undefined) {
      if (msg.action === 'file.download') {
        const args = (msg.payload as { args?: unknown[] }).args
        const check = validateFileDownloadOptions(args?.[0])
        if (!check.valid) {
          return {
            valid: false,
            error: check.error || 'Invalid or oversized file payload',
          }
        }
      } else if (msg.action === 'federation.uploadMedia') {
        const limits = federationLiveLimits()
        const MAX_IMAGE_RAW_BYTES = limits.noteImageBytes
        const MAX_VIDEO_RAW_BYTES = limits.noteVideoBytes
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
        const mimeHint = String(
          options.mime || options.media_type || '',
        ).toLowerCase()
        const isVideo =
          mimeHint.startsWith('video/') ||
          mimeHint.includes('video') ||
          /\.(mp4|webm|mov|m4v)(\?|$)/i.test(String(options.name || ''))
        const isImage =
          mimeHint.startsWith('image/') ||
          mimeHint.includes('image') ||
          /\.(jpe?g|png|gif|webp|avif|svg)(\?|$)/i.test(
            String(options.name || ''),
          )
        const maxRaw = isImage
          ? MAX_IMAGE_RAW_BYTES
          : isVideo
            ? MAX_VIDEO_RAW_BYTES
            : MAX_VIDEO_RAW_BYTES
        const maxDataChars = Math.ceil((maxRaw * 4) / 3) + 256
        if (options.data.length > maxDataChars) {
          return {
            valid: false,
            error: `Media data too large (max ${maxRaw} bytes raw for ${
              isImage ? 'image' : isVideo ? 'video' : 'media'
            } / ~${maxDataChars} chars base64)`,
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
        msg.action === 'federation.sendRoomMessage'
      ) {
        const limits = federationLiveLimits()
        const envelopeBytes = serializedUtf8Bytes(msg.payload)
        if (envelopeBytes === null) {
          return { valid: false, error: 'Payload must be JSON-serializable' }
        }
        if (envelopeBytes > federationMessageEnvelopeBytes()) {
          return {
            valid: false,
            error: `Payload too large for ${msg.action} (message payload max ${limits.messagePayloadBytes} bytes; got ${envelopeBytes} UTF-8 bytes). Use federation chunked transfer for larger data.`,
          }
        }
        const args =
          msg.payload && typeof msg.payload === 'object'
            ? (msg.payload as { args?: unknown }).args
            : undefined
        const request = Array.isArray(args) ? args[1] : undefined
        const messagePayload =
          request && typeof request === 'object'
            ? (request as { payload?: unknown }).payload
            : undefined
        const messageBytes = serializedUtf8Bytes(messagePayload)
        if (
          messageBytes !== null &&
          messageBytes > limits.messagePayloadBytes
        ) {
          return {
            valid: false,
            error: `Message payload too large for ${msg.action} (max ${limits.messagePayloadBytes} bytes; got ${messageBytes} UTF-8 bytes). Use federation chunked transfer for larger data.`,
          }
        }
      } else if (
        msg.action === 'tappList.install' ||
        msg.action === 'tappList.getInstallPackage'
      ) {
        const payloadBytes = serializedUtf8Bytes(msg.payload)
        if (payloadBytes === null) {
          return { valid: false, error: 'Payload must be JSON-serializable' }
        }
        if (payloadBytes > TAPP_PACKAGE_PAYLOAD_BYTES) {
          return {
            valid: false,
            error: `Payload too large for ${msg.action} (max ~128 MiB; got ${payloadBytes} UTF-8 bytes)`,
          }
        }
      } else {
        const payloadBytes = serializedUtf8Bytes(msg.payload)
        if (payloadBytes === null) {
          return { valid: false, error: 'Payload must be JSON-serializable' }
        }
        const request = (msg.payload as {
          args?: Array<{ operation?: unknown; input?: { referenceImages?: unknown } }>
        })?.args?.[0]
        const hasImageReferences = msg.action === 'ai.tasks.create'
          && request?.operation === 'image'
          && Array.isArray(request.input?.referenceImages)
        const maxPayloadBytes = hasImageReferences
          ? 14 * 1024 * 1024
          : 1024 * 1024 + 64 * 1024
        if (payloadBytes > maxPayloadBytes) {
          return {
            valid: false,
            error: `Payload too large (max ${maxPayloadBytes} bytes for ${msg.action || 'this action'})`,
          }
        }
      }
    }

    // iframe→host 的 request/event 必须带 session token。
    if ((msg.type === 'request' || msg.type === 'event') && this.sessionToken) {
      const sessionToken = msg._sessionToken as string | undefined
      if (
        typeof sessionToken !== 'string' ||
        sessionToken.length === 0 ||
        sessionToken !== this.sessionToken
      ) {
        console.warn(
          '[TappBridge] Session token mismatch - possible message spoofing',
        )
        return { valid: false, error: 'Invalid session token' }
      }
    }

    return { valid: true }
  }

  private noteInvalidMessage(): void {
    this.invalidCount += 1
    if (this.invalidCount >= BRIDGE_LIMITS.invalidBeforeMute) {
      this.mutedUntil = Date.now() + BRIDGE_LIMITS.invalidMuteMs
      this.invalidCount = 0
      console.warn(
        '[TappBridge] Temporarily muting bridge after invalid message burst',
      )
    }
  }

  private async handleMessage(event: MessageEvent): Promise<void> {
    // srcdoc origin 为 null；边界是具体 iframe WindowProxy。
    if (event.source !== this.iframe?.contentWindow) {
      return
    }

    const now = Date.now()
    // 静音期间不校验完整载荷，以免无效大包烧 CPU。
    if (now < this.mutedUntil) {
      const candidate = event.data as Record<string, unknown> | undefined
      if (
        candidate &&
        typeof candidate === 'object' &&
        candidate.type === 'request' &&
        typeof candidate.id === 'string' &&
        /^[\w-]+$/.test(candidate.id) &&
        candidate.id.length <= 100
      ) {
        this.sendResponse(candidate.id, {
          success: false,
          error: 'Bridge temporarily muted after invalid message burst',
          code: 'BRIDGE_MUTED',
          retryAfter: Math.max(0, this.mutedUntil - now),
        })
      }
      return
    }

    const validation = this.validateMessage(event.data)
    if (!validation.valid) {
      this.noteInvalidMessage()
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

    if (!this.inboundRate.tryTake(now)) {
      const candidate = event.data as TappMessage
      if (candidate.type === 'request' && typeof candidate.id === 'string') {
        this.sendResponse(candidate.id, {
          success: false,
          error: 'Bridge rate limit exceeded; slow down',
          code: 'RATE_LIMITED',
          retryAfter: this.inboundRate.retryAfterMs(now),
        })
      }
      return
    }

    if (this.invalidCount > 0) this.invalidCount -= 1

    const message = event.data as TappMessage

    if (message.type === 'request') {
      if (this.seenRequestIds.has(message.id)) {
        this.sendResponse(message.id, {
          success: false,
          error: 'Duplicate request id',
          code: 'DUPLICATE_REQUEST',
        })
        return
      }
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

    if (this.inFlightRequests >= BRIDGE_LIMITS.maxConcurrentRequests) {
      this.sendResponse(id, {
        success: false,
        error: 'Too many concurrent bridge requests',
        code: 'CONCURRENCY_LIMIT',
      })
      return
    }

    if (this.tappInstance) {
      const quotaManager = getQuotaManager()
      const quotaCheck = quotaManager.checkQuota(this.tappInstance.id, action)
      if (!quotaCheck.allowed) {
        this.sendResponse(id, {
          success: false,
          error: quotaCheck.reason || 'Quota exceeded',
          code: 'QUOTA_EXCEEDED',
          retryAfter: quotaCheck.retryAfter,
        })
        return
      }
    }

    const permissionCheck = await this.checkPermissionDetailed(action)
    if (!permissionCheck.allowed) {
      this.sendResponse(id, {
        success: false,
        error: `Permission denied: ${permissionCheck.reason || action}`,
        code: 'PERMISSION_DENIED',
      })
      return
    }

    const handler = this.messageHandlers.get(action)
    if (!handler) {
      if (this.tappInstance?.previewMode) {
        this.sendResponse(id, {
          success: false,
          error: previewUnavailableMessage(action),
          code: PREVIEW_UNAVAILABLE_CODE,
        })
        return
      }
      this.sendResponse(id, {
        success: false,
        error: `Unknown action: ${action}`,
        code: 'UNKNOWN_ACTION',
      })
      return
    }

    this.inFlightRequests += 1
    try {
      const response = await handler(message)

      // 成功与失败都计入配额，避免刷失败绕过。
      if (this.tappInstance) {
        const quotaManager = getQuotaManager()
        quotaManager.recordUsage(this.tappInstance.id, action)
      }

      this.sendResponse(id, response)
    } catch (error) {
      if (this.tappInstance) {
        getQuotaManager().recordUsage(this.tappInstance.id, action)
      }
      console.error(`[TappBridge] Handler error for ${action}:`, error)
      this.sendResponse(id, {
        success: false,
        error: userFacingError(error),
        code: 'HANDLER_ERROR',
      })
    } finally {
      this.inFlightRequests = Math.max(0, this.inFlightRequests - 1)
    }
  }

  private handleEvent(message: TappMessage): void {
    if (!this.allowedSandboxEvents.has(message.action)) {
      console.warn(
        `[TappBridge] Dropping unsolicited sandbox event: ${message.action}`,
      )
      return
    }
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

  private async checkPermissionDetailed(
    action: string,
  ): Promise<PermissionCheckResult> {
    if (!this.tappInstance) {
      return { allowed: false, reason: 'Tapp instance not initialized' }
    }

    const requiredPermission = PERMISSION_MAP.get(action)

    if (requiredPermission === 'public') {
      return { allowed: true, requiredPermission }
    }

    if (!requiredPermission) {
      console.warn(
        `[TappBridge] Unknown action for permission check: ${action}`,
      )
      return { allowed: false, reason: `Unknown action: ${action}` }
    }

    // 授予权限已由后端按角色与下放过滤；在列即已授予，前端不再验角色。
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
      if (!this.runtimeGrant && !this.grantSeed) {
        return {
          allowed: false,
          reason: 'Runtime grant is not initialized',
          requiredPermission,
        }
      }
      try {
        // 主体重置销毁旧 grant 后重铸。
        await this.ensureLiveRuntimeGrant().authorize(requiredPermission)
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

  on(event: string, callback: (data: unknown) => void): () => void {
    let listeners = this.eventListeners.get(event)
    if (!listeners) {
      listeners = new Set()
      this.eventListeners.set(event, listeners)
    }
    listeners.add(callback)

    return () => {
      listeners?.delete(callback)
      if (listeners?.size === 0) this.eventListeners.delete(event)
    }
  }
}

export function createTappBridge(): TappBridge {
  return new TappBridge()
}
