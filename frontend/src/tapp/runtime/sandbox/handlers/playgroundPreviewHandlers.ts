import type { TappInstance, TappMessage } from '../../../types'
import type { TappBridge } from '../../TappBridge'
import { sanitizeStorageValue, validateStorageKey } from '../security'

type PreviewStore = Map<string, unknown>

function argsOf(message: TappMessage): unknown[] {
  return (message.payload as { args?: unknown[] } | undefined)?.args || []
}

/**
 * Host APIs available before a generated Tapp is installed.
 *
 * No handler in this set asks for a backend Runtime Grant. State is scoped to
 * the current Playground tab and disappears with the preview component.
 */
export function registerPlaygroundPreviewHandlers(
  bridge: TappBridge,
  tappInstance: TappInstance,
  storage: PreviewStore,
  settings: PreviewStore,
): void {
  const validateKey = (key: unknown): string | null => {
    if (typeof key !== 'string') return null
    return validateStorageKey(key).valid ? key : null
  }

  bridge.registerHandler('storage.get', async (message) => {
    const key = validateKey(argsOf(message)[0])
    if (!key) return { success: false, error: 'Invalid storage key' }
    return { success: true, data: storage.get(key) ?? null }
  })
  bridge.registerHandler('storage.set', async (message) => {
    const [rawKey, rawValue] = argsOf(message)
    const key = validateKey(rawKey)
    if (!key) return { success: false, error: 'Invalid storage key' }
    const value = sanitizeStorageValue(rawValue)
    if (JSON.stringify(value).length > 1024 * 1024) {
      return { success: false, error: 'Preview storage value is too large' }
    }
    storage.set(key, value)
    return { success: true, data: null }
  })
  bridge.registerHandler('storage.remove', async (message) => {
    const key = validateKey(argsOf(message)[0])
    if (!key) return { success: false, error: 'Invalid storage key' }
    storage.delete(key)
    return { success: true, data: null }
  })
  bridge.registerHandler('storage.keys', async () => ({
    success: true,
    data: Array.from(storage.keys()),
  }))
  bridge.registerHandler('storage.getAll', async () => ({
    success: true,
    data: Object.fromEntries(storage),
  }))
  bridge.registerHandler('storage.clear', async () => {
    storage.clear()
    return { success: true, data: null }
  })
  bridge.registerHandler('storage.usage', async () => {
    const used = new Blob([JSON.stringify(Object.fromEntries(storage))]).size
    return {
      success: true,
      data: { used, limit: 8 * 1024 * 1024, remaining: 8 * 1024 * 1024 - used },
    }
  })

  bridge.registerHandler('settings.get', async (message) => {
    const key = validateKey(argsOf(message)[0])
    if (!key) return { success: false, error: 'Invalid setting key' }
    return { success: true, data: settings.get(key) ?? null }
  })
  bridge.registerHandler('settings.set', async (message) => {
    const [rawKey, rawValue] = argsOf(message)
    const key = validateKey(rawKey)
    if (!key) return { success: false, error: 'Invalid setting key' }
    settings.set(key, sanitizeStorageValue(rawValue))
    return { success: true, data: null }
  })
  bridge.registerHandler('settings.getAll', async () => ({
    success: true,
    data: Object.fromEntries(settings),
  }))

  const shared: PreviewStore = new Map()
  bridge.registerHandler('shared.get', async (message) => {
    const key = validateKey(argsOf(message)[0])
    if (!key) return { success: false, error: 'Invalid shared key' }
    return { success: true, data: shared.get(key) ?? null }
  })
  bridge.registerHandler('shared.set', async (message) => {
    const [rawKey, rawValue] = argsOf(message)
    const key = validateKey(rawKey)
    if (!key) return { success: false, error: 'Invalid shared key' }
    const value = sanitizeStorageValue(rawValue)
    if (JSON.stringify(value).length > 1024 * 1024) {
      return { success: false, error: 'Preview shared value is too large' }
    }
    shared.set(key, value)
    return { success: true, data: null }
  })
  bridge.registerHandler('shared.remove', async (message) => {
    const key = validateKey(argsOf(message)[0])
    if (!key) return { success: false, error: 'Invalid shared key' }
    shared.delete(key)
    return { success: true, data: null }
  })
  bridge.registerHandler('shared.keys', async () => ({
    success: true,
    data: Array.from(shared.keys()),
  }))
  bridge.registerHandler('shared.getAll', async () => ({
    success: true,
    data: Object.fromEntries(shared),
  }))
  bridge.registerHandler('shared.clear', async () => {
    shared.clear()
    return { success: true, data: null }
  })
  bridge.registerHandler('shared.usage', async () => {
    const used = new Blob([JSON.stringify(Object.fromEntries(shared))]).size
    return {
      success: true,
      data: { used, quota: 8 * 1024 * 1024 },
    }
  })

  bridge.registerHandler('ui.showNotification', async () => ({
    success: false,
    error: 'Notifications are disabled in temporary preview',
  }))
  bridge.registerHandler('assets.list', async () => ({ success: true, data: [] }))
  bridge.registerHandler('assets.get', async () => ({
    success: false,
    error: 'Package assets are unavailable in temporary preview',
  }))
  bridge.registerHandler('api.list', async () => ({ success: true, data: [] }))
  bridge.registerHandler('api.execute', async () => ({
    success: false,
    error: 'Declared APIs are disabled in temporary preview',
  }))

  bridge.registerHandler('context.getApp', async () => ({
    success: true,
    data: {
      id: tappInstance.id,
      name: tappInstance.manifest.name,
      version: tappInstance.manifest.version,
      mode: 'page',
      preview: true,
    },
  }))
  bridge.registerHandler('context.getUser', async () => ({
    success: true,
    data: { role: tappInstance.userRole, authenticated: true },
  }))
  bridge.registerHandler('context.getNavigation', async () => ({
    success: true,
    data: { route: '/tapp/playground', params: {} },
  }))
  bridge.registerHandler('context.getSystem', async () => ({
    success: true,
    data: { preview: true, runtime: 'tapp-playground' },
  }))
  bridge.registerHandler('context.getPlayer', async () => ({
    success: true,
    data: null,
  }))
  bridge.registerHandler('context.getGeo', async () => ({
    success: true,
    data: null,
  }))
}
