import type { PermissionLevel, TappInstance } from '../../../types'

import type { OpenUrlRequest } from '../../../utils/openUrlAllowlist'
import type { TappBridge } from '../../TappBridge'
import type { TappNotificationOptions } from '../types'
import { currentCopy } from '../../../../i18n/localeCopy'
import { userFacingError } from '../../../../utils/userFacingError'
import * as TappApiService from '../../../services/TappApiService'
import {
  listOpenUrlDeclarations,
  OpenUrlRateLimiter,

  resolveOpenUrl,
} from '../../../utils/openUrlAllowlist'
import {
  emitTappPrivateChange,
  emitTappSettingsChange,
  emitTappSharedChange,
  emitTappStorageChange,
} from '../../WidgetRuntimeSignals'
import {
  decodeDownloadBase64,
  defaultDownloadFilename,
  FILE_DOWNLOAD_BLOB_MAX_BYTES,
  isSafeDownloadFilename,
  normalizeFileDownloadOptions,
  parseHostDownloadUrl,
  triggerBrowserDownload,
} from '../fileDownload'
import { sanitizeStorageValue, validateStorageKey } from '../security'
import { registerFullKvHandlers } from './kvHandlers'

const openUrlRateLimiter = new OpenUrlRateLimiter(8, 10_000)

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
    const opts = options as TappNotificationOptions
    try {
      const notificationId = await TappApiService.createTappNotification(
        {
          tappId: tappInstance.id,
          title: opts.title || currentCopy().tapp.defaultNotificationTitle,
          message: opts.message || '',
          notificationType: opts.type || 'info',
        },
        await bridge.getRuntimeGrant(),
      )
      return { success: true, data: { notificationId } }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  if (options.headless) return

  bridge.registerHandler('ui.confirm', async (message) => {
    const [msg] = (message.payload as { args: unknown[] }).args || []
    const result = window.confirm(String(msg) || 'Confirm?')
    return { success: true, data: result }
  })

  // 只开声明链接。不信任沙箱里的自由 URL，只用 { id, path?, query? }。
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
          userFacingError(error),
      }
    }
  })

  bridge.registerHandler('ui.fullscreen.request', async () => {
    try {
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

  bridge.registerHandler('ui.fullscreen.exit', async () => {
    try {
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

  bridge.registerHandler('ui.fullscreen.toggle', async () => {
    try {
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

  bridge.registerHandler('ui.fullscreen.isFullscreen', async () => {
    const doc = document as Document & {
      webkitFullscreenElement?: Element
    }
    return {
      success: true,
      data: !!(doc.fullscreenElement || doc.webkitFullscreenElement),
    }
  })
}

export function registerStorageHandlers(
  bridge: TappBridge,
  tappId: string,
): void {
  const MAX_VALUE_SIZE = 1024 * 1024

  registerFullKvHandlers(
    bridge,
    tappId,
    'storage',
    {
      get: TappApiService.getStorage,
      set: TappApiService.setStorage,
      remove: TappApiService.removeStorage,
      keys: TappApiService.listStorageKeys,
      getAll: TappApiService.listStorageEntries,
      clear: TappApiService.clearStorage,
      usage: TappApiService.getStorageUsage,
    },
    emitTappStorageChange,
    { maxValueSize: MAX_VALUE_SIZE, withGrant: true },
  )

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
        error: userFacingError(error),
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
      emitTappSettingsChange({
        tappId,
        key: key as string,
        operation: 'set',
        source: bridge,
      })
      return { success: true, data: null }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
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
        error: userFacingError(error),
      }
    }
  })

  registerFullKvHandlers(
    bridge,
    tappId,
    'shared',
    {
      get: TappApiService.getShared,
      set: TappApiService.setShared,
      remove: TappApiService.removeShared,
      keys: TappApiService.listSharedKeys,
      getAll: TappApiService.listSharedEntries,
      clear: TappApiService.clearShared,
      usage: TappApiService.getSharedUsage,
    },
    emitTappSharedChange,
    { maxValueSize: MAX_VALUE_SIZE, withGrant: false },
  )

  registerFullKvHandlers(
    bridge,
    tappId,
    'private',
    {
      get: TappApiService.getPrivate,
      set: TappApiService.setPrivate,
      remove: TappApiService.removePrivate,
      keys: TappApiService.listPrivateKeys,
      getAll: TappApiService.listPrivateEntries,
      clear: TappApiService.clearPrivate,
      usage: TappApiService.getPrivateUsage,
    },
    emitTappPrivateChange,
    { maxValueSize: MAX_VALUE_SIZE, withGrant: false },
  )
}

/** userRole 仍是 guest 时重探：先 Runtime Grant context，再会话 cookie /api/auth/me（destroyAll 后仍可用）。 */
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
  }

  return role
}

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
        error: userFacingError(error),
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
        error: userFacingError(error),
      }
    }
  })
}

/** 只读 Manifest assets 声明路径；宿主转 base64。无 allow-same-origin 时不能跨上下文共享 blob。 */
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
        error: userFacingError(error),
      }
    }
  })
}

export function registerFileHandlers(bridge: TappBridge): void {
  bridge.registerHandler('file.download', async (message) => {
    const [rawOptions] = (message.payload as { args: unknown[] }).args || []
    const options = normalizeFileDownloadOptions(rawOptions)
    if (!options) return { success: false, error: 'Options required' }

    const { content, url, base64, filename, mimeType } = options

    const hasContent = typeof content === 'string' && content.length > 0
    const hasUrl = typeof url === 'string' && url.length > 0
    const hasBase64 = typeof base64 === 'string' && base64.length > 0
    if (Number(hasContent) + Number(hasUrl) + Number(hasBase64) !== 1) {
      return {
        success: false,
        error: 'Provide exactly one of content, url, or base64',
      }
    }

    try {
      if (hasUrl) {
        const asset = parseHostDownloadUrl(url)
        if (!asset) {
          return {
            success: false,
            error:
              'Only local /api/brew/image-cache images or /api/model3d/assets models can be downloaded',
          }
        }
        const downloadName = filename || asset.defaultFilename
        if (!isSafeDownloadFilename(downloadName)) {
          return { success: false, error: 'Invalid filename' }
        }
        const response = await fetch(asset.path, {
          redirect: 'error',
          credentials: 'same-origin',
        })
        if (!response.ok) {
          return {
            success: false,
            error:
              response.status === 404
                ? 'Generated file not found'
                : 'Could not read generated file',
          }
        }
        const blob = await response.blob()
        if (blob.size === 0 || blob.size > FILE_DOWNLOAD_BLOB_MAX_BYTES) {
          return {
            success: false,
            error: `Content too large: ${blob.size} bytes (max ${FILE_DOWNLOAD_BLOB_MAX_BYTES})`,
          }
        }
        const type = mimeType || blob.type || asset.mimeType
        triggerBrowserDownload(
          type && type !== blob.type ? blob.slice(0, blob.size, type) : blob,
          downloadName,
        )
        return { success: true, data: { filename: downloadName } }
      }

      if (hasBase64) {
        const decoded = decodeDownloadBase64(base64 as string)
        if (!decoded) {
          return { success: false, error: 'Invalid or oversized base64 payload' }
        }
        const downloadName =
          filename && isSafeDownloadFilename(filename)
            ? filename
            : defaultDownloadFilename(mimeType || decoded.mimeType)
        triggerBrowserDownload(
          new Blob([decoded.bytes as BlobPart], {
            type: mimeType || decoded.mimeType || 'application/octet-stream',
          }),
          downloadName,
        )
        return { success: true, data: { filename: downloadName } }
      }

      if (!filename || !isSafeDownloadFilename(filename)) {
        return { success: false, error: 'Invalid filename' }
      }

      const blob = new Blob([content as string], {
        type: mimeType || 'text/plain;charset=utf-8',
      })
      if (blob.size > FILE_DOWNLOAD_BLOB_MAX_BYTES) {
        return {
          success: false,
          error: `Content too large: ${blob.size} bytes (max ${FILE_DOWNLOAD_BLOB_MAX_BYTES})`,
        }
      }
      triggerBrowserDownload(blob, filename)
      return { success: true, data: { filename } }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })
}
