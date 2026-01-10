/**
 * 基础 API 处理器
 *
 * 包含 Lifecycle, UI, Storage 等基础处理器
 */

import type { TappInstance } from '../../../types'
import type { TappBridge } from '../../TappBridge'
import type { TappNotificationOptions } from '../types'
import * as TappApiService from '../../../services/TappApiService'
import { sanitizeStorageValue, validateStorageKey } from '../security'

/**
 * 注册生命周期处理器
 */
export function registerLifecycleHandlers(
  bridge: TappBridge,
  tappInstance: TappInstance,
  onReady?: () => void,
): void {
  bridge.registerHandler('lifecycle.ready', async () => {
    onReady?.()
    return { success: true, data: null }
  })

  bridge.registerHandler('lifecycle.error', async (message) => {
    console.error(`[Sandbox] Tapp ${tappInstance.id} error:`, message.payload)
    return { success: true, data: null }
  })

  bridge.registerHandler('lifecycle.getInfo', async () => {
    return {
      success: true,
      data: {
        id: tappInstance.id,
        version: tappInstance.manifest.version,
        name: tappInstance.manifest.name,
        permissions: tappInstance.grantedPermissions,
        sandboxed: true,
      },
    }
  })
}

/**
 * 注册 UI 处理器
 */
export function registerUIHandlers(
  bridge: TappBridge,
  getLocale?: () => string,
  onNotification?: (options: TappNotificationOptions) => void,
): void {
  bridge.registerHandler('ui.getTheme', async () => {
    const isDark = document.documentElement.classList.contains('dark')
    return { success: true, data: isDark ? 'dark' : 'light' }
  })

  bridge.registerHandler('ui.getPrimaryColor', async () => {
    const color = getComputedStyle(document.documentElement)
      .getPropertyValue('--color-primary')
      .trim() || '#94a3b8'
    return { success: true, data: color }
  })

  bridge.registerHandler('ui.getLocale', async () => {
    return { success: true, data: getLocale?.() || 'zh-CN' }
  })

  bridge.registerHandler('ui.setTitle', async () => {
    return { success: true, data: null }
  })

  bridge.registerHandler('ui.showNotification', async (message) => {
    const [options] = (message.payload as { args: unknown[] }).args || []
    if (onNotification && options) {
      const opts = options as { title?: string, message?: string, type?: string, duration?: number }
      onNotification({
        title: opts.title || 'Tapp 通知',
        message: opts.message || '',
        type: (opts.type as 'success' | 'info' | 'warning' | 'error') || 'info',
        duration: opts.duration,
      })
    }
    return { success: true, data: null }
  })

  bridge.registerHandler('ui.confirm', async (message) => {
    const [msg] = (message.payload as { args: unknown[] }).args || []
    const result = window.confirm(String(msg) || 'Confirm?')
    return { success: true, data: result }
  })

  bridge.registerHandler('ui.requestFullscreen', async () => {
    try {
      // Safari/WebKit 兼容性：使用 webkitRequestFullscreen
      const docEl = document.documentElement as HTMLElement & {
        webkitRequestFullscreen?: () => Promise<void>
      }
      if (docEl.requestFullscreen) {
        await docEl.requestFullscreen()
      }
      else if (docEl.webkitRequestFullscreen) {
        await docEl.webkitRequestFullscreen()
      }
      else {
        return { success: false, error: 'Fullscreen not supported' }
      }
      return { success: true, data: null }
    }
    catch {
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
      }
      else if (doc.webkitExitFullscreen) {
        await doc.webkitExitFullscreen()
      }
      return { success: true, data: null }
    }
    catch {
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

      const fullscreenElement = doc.fullscreenElement || doc.webkitFullscreenElement

      if (fullscreenElement) {
        if (doc.exitFullscreen) {
          await doc.exitFullscreen()
        }
        else if (doc.webkitExitFullscreen) {
          await doc.webkitExitFullscreen()
        }
        return { success: true, data: { isFullscreen: false } }
      }
      else {
        if (docEl.requestFullscreen) {
          await docEl.requestFullscreen()
        }
        else if (docEl.webkitRequestFullscreen) {
          await docEl.webkitRequestFullscreen()
        }
        return { success: true, data: { isFullscreen: true } }
      }
    }
    catch {
      return { success: false, error: 'Fullscreen toggle failed' }
    }
  })

  bridge.registerHandler('ui.isFullscreen', async () => {
    // Safari/WebKit 兼容性
    const doc = document as Document & {
      webkitFullscreenElement?: Element
    }
    return { success: true, data: !!(doc.fullscreenElement || doc.webkitFullscreenElement) }
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
    if (!key)
      return { success: false, error: 'Key is required' }

    // 🔒 安全校验：验证 key 格式
    const keyValidation = validateStorageKey(key as string)
    if (!keyValidation.valid) {
      return { success: false, error: `Invalid key: ${keyValidation.reason}` }
    }

    try {
      const value = await TappApiService.getStorage(tappId, key as string)
      return { success: true, data: value }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Failed' }
    }
  })

  bridge.registerHandler('storage.set', async (message) => {
    const [key, value] = (message.payload as { args: unknown[] }).args || []
    if (!key)
      return { success: false, error: 'Key is required' }

    // 🔒 安全校验：验证 key 格式
    const keyValidation = validateStorageKey(key as string)
    if (!keyValidation.valid) {
      return { success: false, error: `Invalid key: ${keyValidation.reason}` }
    }

    // 🔒 安全校验：清理并检查 value 大小
    const sanitizedValue = sanitizeStorageValue(value)
    const valueSize = JSON.stringify(sanitizedValue).length
    if (valueSize > MAX_VALUE_SIZE) {
      return { success: false, error: `Value too large: ${valueSize} bytes (max ${MAX_VALUE_SIZE})` }
    }

    try {
      await TappApiService.setStorage(tappId, key as string, sanitizedValue)
      return { success: true, data: null }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Failed' }
    }
  })

  bridge.registerHandler('storage.remove', async (message) => {
    const [key] = (message.payload as { args: unknown[] }).args || []
    if (!key)
      return { success: false, error: 'Key is required' }

    // 🔒 安全校验：验证 key 格式
    const keyValidation = validateStorageKey(key as string)
    if (!keyValidation.valid) {
      return { success: false, error: `Invalid key: ${keyValidation.reason}` }
    }

    try {
      await TappApiService.removeStorage(tappId, key as string)
      return { success: true, data: null }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Failed' }
    }
  })

  bridge.registerHandler('storage.keys', async () => {
    try {
      const keys = await TappApiService.listStorageKeys(tappId)
      return { success: true, data: keys }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Failed' }
    }
  })

  bridge.registerHandler('storage.clear', async () => {
    try {
      await TappApiService.clearStorage(tappId)
      return { success: true, data: null }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Failed' }
    }
  })

  bridge.registerHandler('storage.usage', async () => {
    try {
      const keys = await TappApiService.listStorageKeys(tappId)
      let used = 0
      for (const key of keys) {
        const value = await TappApiService.getStorage(tappId, key)
        used += (key.length + JSON.stringify(value).length) * 2
      }
      return { success: true, data: { used, quota: 5 * 1024 * 1024 } }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Failed' }
    }
  })
}

/**
 * 注册用户角色处理器
 */
export function registerUserHandlers(
  bridge: TappBridge,
  tappInstance: TappInstance,
): void {
  bridge.registerHandler('user.getRole', async () => {
    return { success: true, data: tappInstance.userRole || 'guest' }
  })

  bridge.registerHandler('user.isAdmin', async () => {
    return { success: true, data: tappInstance.userRole === 'admin' }
  })

  bridge.registerHandler('user.isGuest', async () => {
    return { success: true, data: (tappInstance.userRole || 'guest') === 'guest' }
  })

  bridge.registerHandler('user.isLoggedIn', async () => {
    return { success: true, data: tappInstance.userRole !== 'guest' }
  })

  bridge.registerHandler('user.getAllowedPermissionLevels', async () => {
    const role = tappInstance.userRole || 'guest'
    const levels = role === 'admin'
      ? ['public', 'basic', 'elevated', 'privileged']
      : role === 'user'
        ? ['public', 'basic']
        : ['public']
    return { success: true, data: levels }
  })

  bridge.registerHandler('user.canUsePermissionLevel', async (message) => {
    const [level] = (message.payload as { args: unknown[] }).args || []
    if (!level)
      return { success: false, error: 'Level required' }
    const role = tappInstance.userRole || 'guest'
    const allowed = role === 'admin'
      ? ['public', 'basic', 'elevated', 'privileged'].includes(level as string)
      : role === 'user'
        ? ['public', 'basic'].includes(level as string)
        : level === 'public'
    return { success: true, data: allowed }
  })
}

/**
 * 注册文件处理器
 *
 * 提供文件下载功能，绕过 iframe 沙箱限制
 */
export function registerFileHandlers(
  bridge: TappBridge,
): void {
  bridge.registerHandler('file.download', async (message) => {
    const [options] = (message.payload as { args: unknown[] }).args || []
    if (!options)
      return { success: false, error: 'Options required' }

    const { content, filename, mimeType } = options as {
      content: string
      filename: string
      mimeType?: string
    }

    if (!content)
      return { success: false, error: 'Content is required' }
    if (!filename)
      return { success: false, error: 'Filename is required' }

    // 验证文件名（防止路径遍历）
    if (filename.includes('..') || filename.includes('/') || filename.includes('\\')) {
      return { success: false, error: 'Invalid filename' }
    }

    // 限制文件大小（最大 10MB）
    const MAX_SIZE = 10 * 1024 * 1024
    if (content.length > MAX_SIZE) {
      return { success: false, error: `Content too large (max ${MAX_SIZE} bytes)` }
    }

    try {
      // 在主应用上下文中创建下载（绕过 iframe 沙箱限制）
      const blob = new Blob([content], { type: mimeType || 'text/plain;charset=utf-8' })
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
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Download failed' }
    }
  })
}
