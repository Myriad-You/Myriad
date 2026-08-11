/**
 * 基础 API 处理器
 *
 * 包含 Lifecycle, UI, Storage 等基础处理器
 */

import type { PermissionLevel, TappInstance } from '../../../types'

import type { OpenUrlRequest } from '../../../utils/openUrlAllowlist'
import type { TappBridge } from '../../TappBridge'
import type { TappNotificationOptions } from '../types'
import { showToast } from '../../../../utils/toastManager'
import * as TappApiService from '../../../services/TappApiService'
import {
  listOpenUrlDeclarations,
  OpenUrlRateLimiter,

  resolveOpenUrl,
} from '../../../utils/openUrlAllowlist'
import { emitTappStorageChange } from '../../WidgetRuntimeSignals'
import { sanitizeStorageValue, validateStorageKey } from '../security'
import { prepareTappNotification } from './notificationPolicy'

/** Per-tapp openUrl rate limit (host-side; shared across sandboxes in this tab). */
const openUrlRateLimiter = new OpenUrlRateLimiter(8, 10_000)

/**
 * 注册生命周期处理器
 */
export function registerLifecycleHandlers(
  bridge: TappBridge,
  tappInstance: TappInstance,
  onReady?: () => void,
  onError?: (error: Error) => void,
): void {
  bridge.registerHandler('lifecycle.ready', async () => {
    onReady?.()
    return { success: true, data: null }
  })

  bridge.registerHandler('lifecycle.error', async (message) => {
    const payload = message.payload
    const payloadRecord =
      payload && typeof payload === 'object'
        ? (payload as Record<string, unknown>)
        : undefined
    const args = Array.isArray(payloadRecord?.args)
      ? (payloadRecord.args as unknown[])
      : []
    const errorMsg =
      typeof payload === 'string'
        ? payload
        : payloadRecord?.message || args[0] || 'Unknown error'
    console.error(`[Sandbox] Tapp ${tappInstance.id} error:`, payload)
    onError?.(new Error(String(errorMsg)))
    return { success: true, data: null }
  })
}

/**
 * 注册 UI 处理器
 */
export function registerUIHandlers(
  bridge: TappBridge,
  tappInstance: TappInstance,
  getLocale?: () => string,
  options: { headless?: boolean } = {},
): void {
  bridge.registerHandler('ui.getTheme', async () => {
    const isDark = document.documentElement.classList.contains('dark')
    return { success: true, data: isDark ? 'dark' : 'light' }
  })

  bridge.registerHandler('ui.getPrimaryColor', async () => {
    const color =
      getComputedStyle(document.documentElement)
        .getPropertyValue('--color-primary')
        .trim() || '#94a3b8'
    return { success: true, data: color }
  })

  bridge.registerHandler('ui.getLocale', async () => {
    // Prefer host locale; fall back to document/html lang, then en-US (not zh-CN).
    let locale = getLocale?.()
    if (!locale) {
      try {
        locale =
          document.documentElement.lang ||
          (typeof navigator !== 'undefined' ? navigator.language : '') ||
          ''
      } catch {
        locale = ''
      }
    }
    return { success: true, data: locale || 'en-US' }
  })

  if (!options.headless) {
    bridge.registerHandler('ui.setTitle', async () => {
      return { success: true, data: null }
    })
  }

  bridge.registerHandler('ui.showNotification', async (message) => {
    const [options] = (message.payload as { args: unknown[] }).args || []
    if (!options) {
      return { success: false, error: 'Notification options required' }
    }
    const prepared = prepareTappNotification(
      options as TappNotificationOptions,
      tappInstance.userRole,
    )
    if (prepared.delivery === 'session') {
      showToast(prepared)
      return {
        success: true,
        data: { notificationId: null, sessionOnly: true },
      }
    }
    try {
      const notificationId = await TappApiService.createTappNotification(
        {
          tappId: tappInstance.id,
          title: prepared.title,
          message: prepared.message,
          notificationType: prepared.type,
        },
        await bridge.getRuntimeGrant(),
      )
      return { success: true, data: { notificationId } }
    } catch (error) {
      return {
        success: false,
        error:
          error instanceof Error
            ? error.message
            : 'Failed to publish notification',
      }
    }
  })

  if (options.headless) return

  bridge.registerHandler('ui.confirm', async (message) => {
    const [msg] = (message.payload as { args: unknown[] }).args || []
    const result = window.confirm(String(msg) || 'Confirm?')
    return { success: true, data: result }
  })

  // Declared-link navigation: host opens a tab after allowlist resolution.
  // Never trust a free-form URL from the sandbox — only { id, path?, query? }.
  bridge.registerHandler('ui.listOpenUrls', async () => {
    return {
      success: true,
      data: listOpenUrlDeclarations(tappInstance.manifest.openUrls),
    }
  })

  bridge.registerHandler('ui.openUrl', async (message) => {
    const [raw] = (message.payload as { args: unknown[] }).args || []
    const request =
      typeof raw === 'string'
        ? ({ id: raw } satisfies OpenUrlRequest)
        : (raw as OpenUrlRequest | undefined)
    if (!request || typeof request !== 'object') {
      return { success: false, error: 'openUrl requires { id, path?, query? }' }
    }

    if (!openUrlRateLimiter.allow(tappInstance.id)) {
      return { success: false, error: 'openUrl rate limit exceeded' }
    }

    const resolved = resolveOpenUrl(tappInstance.manifest.openUrls, request)
    if (!resolved.ok) {
      return { success: false, error: resolved.error }
    }

    try {
      // Prefer anchor-click: with `noopener`, `window.open` often returns null
      // even on success, so we cannot treat a null handle as "blocked".
      const anchor = document.createElement('a')
      anchor.href = resolved.url
      anchor.target = '_blank'
      anchor.rel = 'noopener noreferrer'
      anchor.style.display = 'none'
      document.body.appendChild(anchor)
      anchor.click()
      document.body.removeChild(anchor)
      return {
        success: true,
        data: {
          id: resolved.id,
          url: resolved.url,
          match: resolved.match,
        },
      }
    } catch (error) {
      return {
        success: false,
        error:
          error instanceof Error ? error.message : 'Failed to open URL',
      }
    }
  })

  bridge.registerHandler('ui.requestFullscreen', async () => {
    try {
      // Safari/WebKit 兼容性：使用 webkitRequestFullscreen
      const docEl = document.documentElement as HTMLElement & {
        webkitRequestFullscreen?: () => Promise<void>
      }
      if (docEl.requestFullscreen) {
        await docEl.requestFullscreen()
      } else if (docEl.webkitRequestFullscreen) {
        await docEl.webkitRequestFullscreen()
      } else {
        return { success: false, error: 'Fullscreen not supported' }
      }
      return { success: true, data: null }
    } catch {
      return { success: false, error: 'Fullscreen request denied' }
    }
  })

  bridge.registerHandler('ui.exitFullscreen', async () => {
    try {
      // Safari/WebKit 兼容性
      const doc = document as Document & {
        webkitExitFullscreen?: () => Promise<void>
      }
      if (doc.exitFullscreen) {
        await doc.exitFullscreen()
      } else if (doc.webkitExitFullscreen) {
        await doc.webkitExitFullscreen()
      }
      return { success: true, data: null }
    } catch {
      return { success: false, error: 'Exit fullscreen failed' }
    }
  })

  bridge.registerHandler('ui.toggleFullscreen', async () => {
    try {
      // Safari/WebKit 兼容性
      const doc = document as Document & {
        webkitFullscreenElement?: Element
        webkitExitFullscreen?: () => Promise<void>
      }
      const docEl = document.documentElement as HTMLElement & {
        webkitRequestFullscreen?: () => Promise<void>
      }

      const fullscreenElement =
        doc.fullscreenElement || doc.webkitFullscreenElement

      if (fullscreenElement) {
        if (doc.exitFullscreen) {
          await doc.exitFullscreen()
        } else if (doc.webkitExitFullscreen) {
          await doc.webkitExitFullscreen()
        }
        return { success: true, data: { isFullscreen: false } }
      } else {
        if (docEl.requestFullscreen) {
          await docEl.requestFullscreen()
        } else if (docEl.webkitRequestFullscreen) {
          await docEl.webkitRequestFullscreen()
        }
        return { success: true, data: { isFullscreen: true } }
      }
    } catch {
      return { success: false, error: 'Fullscreen toggle failed' }
    }
  })

  bridge.registerHandler('ui.isFullscreen', async () => {
    // Safari/WebKit 兼容性
    const doc = document as Document & {
      webkitFullscreenElement?: Element
    }
    return {
      success: true,
      data: !!(doc.fullscreenElement || doc.webkitFullscreenElement),
    }
  })
}

/**
 * 注册 Storage 处理器
 *
 * 安全增强：
 * - 对所有 key 进行路径穿越检查
 * - 对 value 进行清理
 * - 限制存储大小
 */
export function registerStorageHandlers(
  bridge: TappBridge,
  tappId: string,
): void {
  // 存储值大小限制（单个值最大 1MB）
  const MAX_VALUE_SIZE = 1024 * 1024

  bridge.registerHandler('storage.get', async (message) => {
    const [key] = (message.payload as { args: unknown[] }).args || []
    if (!key) return { success: false, error: 'Key is required' }

    // 安全校验：验证 key 格式
    const keyValidation = validateStorageKey(key as string)
    if (!keyValidation.valid) {
      return { success: false, error: `Invalid key: ${keyValidation.reason}` }
    }

    try {
      const value = await TappApiService.getStorage(
        tappId,
        key as string,
        await bridge.getRuntimeGrant(),
      )
      return { success: true, data: value }
    } catch (error) {
      return {
        success: false,
        error: error instanceof Error ? error.message : 'Failed',
      }
    }
  })

  bridge.registerHandler('storage.set', async (message) => {
    const [key, value] = (message.payload as { args: unknown[] }).args || []
    if (!key) return { success: false, error: 'Key is required' }

    // 安全校验：验证 key 格式
    const keyValidation = validateStorageKey(key as string)
    if (!keyValidation.valid) {
      return { success: false, error: `Invalid key: ${keyValidation.reason}` }
    }

    // 安全校验：清理并检查 value 大小
    const sanitizedValue = sanitizeStorageValue(value)
    const valueSize = JSON.stringify(sanitizedValue).length
    if (valueSize > MAX_VALUE_SIZE) {
      return {
        success: false,
        error: `Value too large: ${valueSize} bytes (max ${MAX_VALUE_SIZE})`,
      }
    }

    try {
      await TappApiService.setStorage(
        tappId,
        key as string,
        sanitizedValue,
        await bridge.getRuntimeGrant(),
      )
      emitTappStorageChange({
        tappId,
        key: key as string,
        operation: 'set',
        source: bridge,
      })
      return { success: true, data: null }
    } catch (error) {
      return {
        success: false,
        error: error instanceof Error ? error.message : 'Failed',
      }
    }
  })

  bridge.registerHandler('storage.remove', async (message) => {
    const [key] = (message.payload as { args: unknown[] }).args || []
    if (!key) return { success: false, error: 'Key is required' }

    // 安全校验：验证 key 格式
    const keyValidation = validateStorageKey(key as string)
    if (!keyValidation.valid) {
      return { success: false, error: `Invalid key: ${keyValidation.reason}` }
    }

    try {
      await TappApiService.removeStorage(
        tappId,
        key as string,
        await bridge.getRuntimeGrant(),
      )
      emitTappStorageChange({
        tappId,
        key: key as string,
        operation: 'remove',
        source: bridge,
      })
      return { success: true, data: null }
    } catch (error) {
      return {
        success: false,
        error: error instanceof Error ? error.message : 'Failed',
      }
    }
  })

  bridge.registerHandler('storage.keys', async () => {
    try {
      const keys = await TappApiService.listStorageKeys(
        tappId,
        await bridge.getRuntimeGrant(),
      )
      return { success: true, data: keys }
    } catch (error) {
      return {
        success: false,
        error: error instanceof Error ? error.message : 'Failed',
      }
    }
  })

  bridge.registerHandler('storage.getAll', async () => {
    try {
      const entries = await TappApiService.listStorageEntries(
        tappId,
        await bridge.getRuntimeGrant(),
      )
      return { success: true, data: entries }
    } catch (error) {
      return {
        success: false,
        error: error instanceof Error ? error.message : 'Failed',
      }
    }
  })

  bridge.registerHandler('storage.clear', async () => {
    try {
      await TappApiService.clearStorage(tappId, await bridge.getRuntimeGrant())
      emitTappStorageChange({
        tappId,
        operation: 'clear',
        source: bridge,
      })
      return { success: true, data: null }
    } catch (error) {
      return {
        success: false,
        error: error instanceof Error ? error.message : 'Failed',
      }
    }
  })

  bridge.registerHandler('storage.usage', async () => {
    try {
      const usage = await TappApiService.getStorageUsage(
        tappId,
        await bridge.getRuntimeGrant(),
      )
      return { success: true, data: usage }
    } catch (error) {
      return {
        success: false,
        error: error instanceof Error ? error.message : 'Failed',
      }
    }
  })

  bridge.registerHandler('settings.get', async (message) => {
    const [key] = (message.payload as { args: unknown[] }).args || []
    if (!key) return { success: false, error: 'Key is required' }
    const keyValidation = validateStorageKey(key as string)
    if (!keyValidation.valid) {
      return { success: false, error: `Invalid key: ${keyValidation.reason}` }
    }
    try {
      const value = await TappApiService.getTappSetting(tappId, key as string)
      return { success: true, data: value }
    } catch (error) {
      return {
        success: false,
        error: error instanceof Error ? error.message : 'Failed',
      }
    }
  })

  bridge.registerHandler('settings.set', async (message) => {
    const [key, value] = (message.payload as { args: unknown[] }).args || []
    if (!key) return { success: false, error: 'Key is required' }
    const keyValidation = validateStorageKey(key as string)
    if (!keyValidation.valid) {
      return { success: false, error: `Invalid key: ${keyValidation.reason}` }
    }
    try {
      await TappApiService.setTappSetting(
        tappId,
        key as string,
        sanitizeStorageValue(value),
      )
      return { success: true, data: null }
    } catch (error) {
      return {
        success: false,
        error: error instanceof Error ? error.message : 'Failed',
      }
    }
  })

  bridge.registerHandler('settings.getAll', async () => {
    try {
      const values = await TappApiService.getTappSettings(tappId)
      return { success: true, data: values }
    } catch (error) {
      return {
        success: false,
        error: error instanceof Error ? error.message : 'Failed',
      }
    }
  })
}

/**
 * When the catalog left userRole as guest (stale list / race), re-probe
 * context/user so logged-in viewers are not permanently guest-locked in Aro.
 *
 * Order:
 *  1) Runtime-grant context user (normal path)
 *  2) Session cookie /api/auth/me (works after destroyAll / grant mint failure)
 */
async function resolveLiveUserRole(
  bridge: TappBridge,
  tappInstance: TappInstance,
): Promise<'guest' | 'user' | 'admin'> {
  let role = (tappInstance.userRole || 'guest') as 'guest' | 'user' | 'admin'
  if (role === 'user' || role === 'admin') return role

  const applyUser = (user: {
    role?: string
    isAdmin?: boolean
    id?: string | number
    username?: string
    authenticated?: boolean
  } | null): 'guest' | 'user' | 'admin' => {
    if (!user || typeof user !== 'object') return role
    const rawRole =
      user.role != null ? String(user.role).trim().toLowerCase() : ''
    if (rawRole === 'admin' || user.isAdmin === true) {
      role = 'admin'
    } else if (rawRole === 'user' || user.authenticated === true) {
      role = 'user'
    } else {
      const id = user.id != null ? String(user.id) : ''
      const username = user.username != null ? String(user.username).trim() : ''
      const m = /^user_(-?\d+)$/i.exec(id)
      const n = m ? Number.parseInt(m[1]!, 10) : Number.NaN
      if (Number.isFinite(n) && n > 0 && username) {
        role = 'user'
      }
    }
    if (role !== 'guest') {
      tappInstance.userRole = role
    }
    return role
  }

  try {
    const grant = await bridge.getRuntimeGrant()
    const user = (await TappApiService.getContextUser(grant)) as {
      role?: string
      isAdmin?: boolean
      id?: string | number
      username?: string
      authenticated?: boolean
    } | null
    applyUser(user)
    if (role !== 'guest') return role
  } catch (error) {
    if (
      error instanceof Error &&
      /runtime has already stopped|not initialized|grant/i.test(error.message)
    ) {
      console.warn(
        '[Tapp] runtime grant unavailable — probing session cookie for role',
        tappInstance.id,
        error.message,
      )
    }
  }

  // Session fallback: host cookie, no grant required
  try {
    const {
      fetchSessionUserSnapshot,
    } = await import('../../sessionUserFallback')
    const snap = await fetchSessionUserSnapshot()
    if (snap) {
      applyUser({
        role: snap.role,
        isAdmin: snap.isAdmin,
        id: snap.id,
        username: snap.username,
        authenticated: snap.authenticated,
      })
    }
  } catch {
    // remain catalog role
  }

  return role
}

/**
 * 注册用户角色处理器
 */
export function registerUserHandlers(
  bridge: TappBridge,
  tappInstance: TappInstance,
): void {
  bridge.registerHandler('user.getRole', async () => {
    const role = await resolveLiveUserRole(bridge, tappInstance)
    return { success: true, data: role }
  })

  bridge.registerHandler('user.isAdmin', async () => {
    const role = await resolveLiveUserRole(bridge, tappInstance)
    return { success: true, data: role === 'admin' }
  })

  bridge.registerHandler('user.isGuest', async () => {
    const role = await resolveLiveUserRole(bridge, tappInstance)
    return {
      success: true,
      data: role === 'guest',
    }
  })

  bridge.registerHandler('user.isLoggedIn', async () => {
    const role = await resolveLiveUserRole(bridge, tappInstance)
    return { success: true, data: role !== 'guest' }
  })

  bridge.registerHandler('user.getAllowedPermissionLevels', async () => {
    try {
      const levels = await TappApiService.getAllowedPermissionLevels()
      return { success: true, data: levels }
    } catch (error) {
      return {
        success: false,
        error:
          error instanceof Error
            ? error.message
            : 'Failed to load permission levels',
      }
    }
  })

  bridge.registerHandler('user.canUsePermissionLevel', async (message) => {
    const [level] = (message.payload as { args: unknown[] }).args || []
    if (!level) return { success: false, error: 'Level required' }
    try {
      const levels = await TappApiService.getAllowedPermissionLevels()
      return {
        success: true,
        data: levels.includes(level as PermissionLevel),
      }
    } catch (error) {
      return {
        success: false,
        error:
          error instanceof Error
            ? error.message
            : 'Failed to load permission levels',
      }
    }
  })
}

/**
 * 注册包内静态资源处理器
 *
 * 仅允许读取 Manifest `assets` 声明的路径；内容由宿主转 base64 交给沙箱
 * 在 iframe 内创建 blob URL（无 allow-same-origin 时不能跨上下文共享 blob）。
 */
export function registerAssetHandlers(
  bridge: TappBridge,
  tappInstance: TappInstance,
): void {
  bridge.registerHandler('assets.list', async () => {
    const assets = Array.isArray(tappInstance.manifest.assets)
      ? tappInstance.manifest.assets.slice()
      : []
    return { success: true, data: assets }
  })

  bridge.registerHandler('assets.get', async (message) => {
    const [pathArg] = (message.payload as { args?: unknown[] }).args || []
    if (typeof pathArg !== 'string' || pathArg.length === 0) {
      return { success: false, error: 'Asset path is required' }
    }
    if (
      pathArg.includes('..') ||
      pathArg.includes('\\') ||
      !pathArg.startsWith('assets/') ||
      pathArg.length > 512
    ) {
      return { success: false, error: 'Invalid asset path' }
    }
    const declared = tappInstance.manifest.assets
    if (!Array.isArray(declared) || !declared.includes(pathArg)) {
      return { success: false, error: `Asset not declared: ${pathArg}` }
    }
    try {
      const asset = await TappApiService.getTappAsset(tappInstance.id, pathArg)
      return { success: true, data: asset }
    } catch (error) {
      return {
        success: false,
        error: error instanceof Error ? error.message : 'Failed to load asset',
      }
    }
  })
}

/**
 * 注册文件处理器
 *
 * 提供文件下载功能，绕过 iframe 沙箱限制
 */
export function registerFileHandlers(bridge: TappBridge): void {
  bridge.registerHandler('file.download', async (message) => {
    const [options] = (message.payload as { args: unknown[] }).args || []
    if (!options) return { success: false, error: 'Options required' }

    const { content, filename, mimeType } = options as {
      content: string
      filename: string
      mimeType?: string
    }

    if (typeof content !== 'string' || content.length === 0) {
      return { success: false, error: 'Content must be a non-empty string' }
    }
    if (typeof filename !== 'string' || filename.length === 0) {
      return { success: false, error: 'Filename is required' }
    }

    // 验证文件名（防止路径遍历）
    if (
      filename.includes('..') ||
      filename.includes('/') ||
      filename.includes('\\')
    ) {
      return { success: false, error: 'Invalid filename' }
    }

    // 限制文件大小（最大 10MB）
    const MAX_SIZE = 10 * 1024 * 1024
    const blob = new Blob([content], {
      type: mimeType || 'text/plain;charset=utf-8',
    })
    if (blob.size > MAX_SIZE) {
      return {
        success: false,
        error: `Content too large: ${blob.size} bytes (max ${MAX_SIZE})`,
      }
    }

    try {
      // 在主应用上下文中创建下载（绕过 iframe 沙箱限制）
      const url = URL.createObjectURL(blob)

      const a = document.createElement('a')
      a.href = url
      a.download = filename
      a.style.display = 'none'
      document.body.appendChild(a)
      a.click()

      // 清理
      setTimeout(() => {
        document.body.removeChild(a)
        URL.revokeObjectURL(url)
      }, 100)

      return { success: true, data: { filename } }
    } catch (error) {
      return {
        success: false,
        error: error instanceof Error ? error.message : 'Download failed',
      }
    }
  })
}
