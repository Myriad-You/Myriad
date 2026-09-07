/**
 * Tapp Bridge - 消息桥接层
 * 负责主应用与 Tapp 沙箱之间的安全通信
 *
 * 安全特性：
 * - 严格的消息来源验证（event.source）
 * - request/event 会话 token
 * - 显式 sandbox inbound event 白名单
 * - 细粒度权限检查（含用户角色验证）
 * - 输入、大小和时间戳验证
 * - 会话内请求 ID 防重放
 * - 每 Bridge 软限速 + 并发上限（防 iframe 洪水）
 */

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

/** Soft host-side abuse limits (defense-in-depth; backend still authoritative). */
const BRIDGE_LIMITS = {
  /** Max validated inbound messages (request+event) per sliding minute */
  messagesPerMinute: 240,
  /** Max concurrent in-flight request handlers */
  maxConcurrentRequests: 32,
  /** Invalid messages before short mute */
  invalidBeforeMute: 40,
  /** Mute duration after invalid burst */
  invalidMuteMs: 10_000,
  /** Acceptable clock skew for message timestamps */
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
  // Count without allocating a second full-size Uint8Array for large packages.
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
      // BMP code points and unpaired surrogates (UTF-8 replacement character).
      bytes += 3
    }
  }
  return bytes
}

/** Tiny sliding-window counter for bridge-local limits. */
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

  /** Ms until the oldest stamp leaves the window (0 if under cap). */
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
export interface TappBridgeInitOptions {
  /**
   * When set, destroy() releases a shared grant instead of destroying it.
   * Used by multi-widget same-Tapp sandboxes (refcount on TappRuntimeGrant).
   */
  releaseSharedGrant?: () => void
  /**
   * Re-acquire shared grant after subject reset (login/logout destroyAll).
   * Must return a fresh { grant, release } pair so refcounts stay correct.
   */
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

  /**
   * Sandbox → host event actions this bridge instance will dispatch.
   * Empty until {@link allowSandboxEvent}; host must opt in per action.
   */
  private allowedSandboxEvents = new Set<string>()

  /** Soft per-bridge rate limit for validated inbound traffic. */
  private readonly inboundRate = new BridgeWindowCounter(
    60_000,
    BRIDGE_LIMITS.messagesPerMinute,
  )

  private inFlightRequests = 0
  private invalidCount = 0
  private mutedUntil = 0

  /**
   * postMessage 目标 origin（发送消息用）
   * srcdoc iframe 未启用 allow-same-origin，origin 为 null
   * 浏览器不接受字符串 'null' 作为 postMessage 目标，使用 '*' 代替
   * 安全性由 event.source === iframe.contentWindow 检查保证
   */
  private postMessageTarget: string = '*'

  /** 会话 token（用于验证消息来源） */
  private sessionToken: string = ''

  /** True after {@link destroy}; host shortcuts treat the bridge as dead. */
  private destroyed = false

  /**
   * Host surface visibility (multi-window minimize / lifecycle pause).
   * When false, host keyboard shortcuts skip this bridge.
   * Set by sandboxes from the `paused` prop (and related lifecycle).
   */
  private surfaceActive = true

  /** Host-only backend identity for this concrete Page/Widget/headless runtime. */
  private runtimeGrant: TappRuntimeGrant | null = null

  /**
   * Params to mint a replacement grant after subject reset (`destroyAll`).
   * Destroyed grants cannot issue tokens; live sandboxes re-mint with these.
   */
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

  /**
   * Single window-level router for all bridges (N widgets → 1 listener).
   * Routing key remains event.source === iframe.contentWindow (isolation intact).
   */
  private static readonly bridgesBySource = new Map<
    MessageEventSource,
    TappBridge
  >()

  /** Live bridges for srcdoc attach race: ready may fire before attachSource. */
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

  /**
   * Resolve bridge for an inbound message.
   * Prefer O(1) source map; fall back to contentWindow scan so early
   * tapp.ready is not lost when srcdoc scripts race attachSource().
   */
  private static resolveBridgeForSource(
    source: MessageEventSource,
  ): TappBridge | null {
    const mapped = TappBridge.bridgesBySource.get(source)
    if (mapped) return mapped
    for (const bridge of TappBridge.activeBridges) {
      const win = bridge.iframe?.contentWindow
      if (win && win === source) {
        // Heal the map for subsequent messages
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
    // bound instance methods for handler registration
    this.handleMessage = this.handleMessage.bind(this)
    void refreshFederationLimits()
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

    // 集中式 message 路由（同 Tapp 多 Widget 时只挂一个 window listener）
    TappBridge.ensureSharedListener()
    TappBridge.activeBridges.add(this)
    this.attachSource()
  }

  /**
   * Register iframe.contentWindow as the routing key.
   * Call after the iframe is in the document (srcdoc parse may recreate the window).
   */
  attachSource(): void {
    const win = this.iframe?.contentWindow
    if (!win) return
    if (this.registeredSource && this.registeredSource !== win) {
      TappBridge.bridgesBySource.delete(this.registeredSource)
    }
    this.registeredSource = win
    TappBridge.bridgesBySource.set(win, this)
  }

  /**
   * 获取会话 token（供沙箱 HTML 生成时使用）。
   * Empty string after {@link destroy} so host shortcut bindings treat the bridge as dead.
   */
  getSessionToken(): string {
    return this.destroyed ? '' : this.sessionToken
  }

  /** Whether this bridge has been destroyed (session cleared, no longer live). */
  isDestroyed(): boolean {
    return this.destroyed
  }

  /**
   * Host surface active flag (multi-window minimize / lifecycle pause).
   * Sandboxes should call this from the `paused` prop effect.
   */
  setSurfaceActive(active: boolean): void {
    this.surfaceActive = active
  }

  /**
   * Whether host keyboard shortcuts should target this bridge.
   * False when destroyed, host-paused, iframe gone/disconnected, or CSS-hidden.
   */
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
      /* ignore getComputedStyle failures */
    }
    return true
  }

  /**
   * After AuthContext `destroyAll()` (login/logout), previous grants are dead.
   * Re-mint from seed so open Aro (and others) can call context.getUser /
   * federation again without requiring a full browser reload.
   */
  private ensureLiveRuntimeGrant(): TappRuntimeGrant {
    if (this.runtimeGrant && !this.runtimeGrant.isDestroyed()) {
      return this.runtimeGrant
    }
    // Shared widget grants: re-enter the refcounted pool after destroyAll.
    if (this.reacquireSharedGrant) {
      // Drop the previous shared-pool hold before (or as we) re-acquire so
      // refcounts do not leak across subject resets.
      const previousRelease = this.releaseSharedGrant
      this.releaseSharedGrant = null
      if (previousRelease) {
        try {
          previousRelease()
        } catch {
          /* ignore stale release */
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

  /**
   * 宿主 API 归因头：把 speech/brew 等宿主路径调用绑定到当前 Tapp 运行时，
   * 由服务端校验 Grant 并强制权限。预览模式没有 Grant，返回 undefined
   * （请求按宿主自身身份执行，与旧行为一致）。
   */
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

  /**
   * 销毁 Bridge
   */
  destroy(): void {
    this.destroyed = true
    this.surfaceActive = false
    // Clear session first so host shortcut checks see a dead bridge immediately.
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
   * Opt-in: allow a sandbox → host event action on this bridge instance.
   * Action format: same as postMessage action (`[\w.]+`, max 50).
   */
  allowSandboxEvent(action: string): void {
    if (!/^[\w.]+$/.test(action) || action.length > 50) {
      throw new Error(`Invalid sandbox event action: ${action}`)
    }
    this.allowedSandboxEvents.add(action)
  }

  /** Remove a previously allowed sandbox → host event action. */
  disallowSandboxEvent(action: string): void {
    this.allowedSandboxEvents.delete(action)
  }

  /** Whether this bridge will dispatch the given sandbox event action. */
  isSandboxEventAllowed(action: string): boolean {
    return this.allowedSandboxEvents.has(action)
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

    // payload 大小检查（防止内存攻击）
    // 默认 1 MiB；action-specific higher caps (media / packages / chat) —
    // tens of MiB, not unbounded. Align with backend where applicable.
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
        // Align with live note image/video caps (default 32/256; saver 8/32).
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
        // Unknown type: allow up to video cap (BE still enforces by actual MIME)
        const maxRaw = isImage
          ? MAX_IMAGE_RAW_BYTES
          : isVideo
            ? MAX_VIDEO_RAW_BYTES
            : MAX_VIDEO_RAW_BYTES
        // Bridge carries data URL / base64 (~4/3 raw) + small JSON envelope.
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
        // Image tasks need room for 10 MiB of base64 images, 256 KiB of text,
        // and the envelope (backend ai_task_image enforces decoded/text limits).
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

    // iframe → host: request 与 event 都必须携带 session token。
    // event.source 已校验具体 WindowProxy；token 防止同页其它脚本在
    // 误获 contentWindow 引用时伪造宿主监听的事件（如 tapp.ready）。
    // host → iframe 的 emit 不走此路径。
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

  /**
   * 处理来自 Tapp 的消息（增强安全版本）
   */
  private async handleMessage(event: MessageEvent): Promise<void> {
    // srcdoc 沙箱的 origin 为 null；真实安全边界是具体 iframe WindowProxy。
    if (event.source !== this.iframe?.contentWindow) {
      return
    }

    const now = Date.now()
    // While muted: cheap request-shape check only (answer BRIDGE_MUTED so the
    // iframe unblocks). Defer full payload validation (JSON size, token, …)
    // until unmuted — large invalid payloads must not burn CPU during mute.
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

    // 验证消息格式
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

    // Soft rate limit after validation (token + shape OK)
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

    // Valid traffic gradually cools the invalid counter
    if (this.invalidCount > 0) this.invalidCount -= 1

    const message = event.data as TappMessage

    if (message.type === 'request') {
      if (this.seenRequestIds.has(message.id)) {
        // Structured reply so the SDK does not hang on a silent drop.
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

    if (this.inFlightRequests >= BRIDGE_LIMITS.maxConcurrentRequests) {
      this.sendResponse(id, {
        success: false,
        error: 'Too many concurrent bridge requests',
        code: 'CONCURRENCY_LIMIT',
      })
      return
    }

    // 配额检查（含全局 bridge 软限速）
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

      // 记录配额使用：成功与失败都计入，避免刷失败绕过
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

  /**
   * 处理来自沙箱的事件（仅本实例 allowSandboxEvent 白名单）
   */
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
      if (!this.runtimeGrant && !this.grantSeed) {
        return {
          allowed: false,
          reason: 'Runtime grant is not initialized',
          requiredPermission,
        }
      }
      try {
        // Re-mint if subject reset destroyed the previous grant.
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
