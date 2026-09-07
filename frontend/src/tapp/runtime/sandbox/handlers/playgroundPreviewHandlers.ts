import type { TappInstance, TappMessage } from '../../../types'
import type { TappBridge } from '../../TappBridge'
import {
  emitTappSettingsChange,
  emitTappSharedChange,
  emitTappStorageChange,
} from '../../WidgetRuntimeSignals'
import { sanitizeStorageValue, validateStorageKey } from '../security'

export type PreviewStore = Map<string, unknown>

export interface PlaygroundPreviewStores {
  storage: PreviewStore
  settings: PreviewStore
  shared: PreviewStore
}

export interface PreviewAssetPayload {
  path: string
  mimeType: string
  size: number
  base64: string
}

function guessPreviewAssetMime(path: string): string {
  const ext = path.split('.').pop()?.toLowerCase() || ''
  switch (ext) {
    case 'png':
      return 'image/png'
    case 'jpg':
    case 'jpeg':
      return 'image/jpeg'
    case 'webp':
      return 'image/webp'
    case 'gif':
      return 'image/gif'
    case 'svg':
      return 'image/svg+xml'
    case 'glb':
      return 'model/gltf-binary'
    case 'gltf':
      return 'model/gltf+json'
    case 'json':
      return 'application/json'
    case 'wasm':
      return 'application/wasm'
    case 'mp3':
      return 'audio/mpeg'
    case 'wav':
      return 'audio/wav'
    case 'woff2':
      return 'font/woff2'
    default:
      return 'application/octet-stream'
  }
}

function byteLengthFromBase64(base64: string): number {
  const clean = base64.replace(/\s/g, '')
  if (!clean) return 0
  const padding = clean.endsWith('==') ? 2 : clean.endsWith('=') ? 1 : 0
  return Math.max(0, Math.floor((clean.length * 3) / 4) - padding)
}

/** Decode a Playground `code.assets` payload into the host asset envelope. */
export function previewAssetFromPackage(
  path: string,
  raw: string,
): PreviewAssetPayload | null {
  if (!path.startsWith('assets/') || path.includes('..') || path.includes('\\')) {
    return null
  }
  const dataUrl = raw.match(/^data:([^;,]+);base64,([\s\S]+)$/)
  let mimeType: string
  let base64: string
  if (dataUrl) {
    mimeType = dataUrl[1]
    base64 = dataUrl[2].replace(/\s/g, '')
  } else if (/^[A-Za-z0-9+/=\s]+$/.test(raw) && raw.replace(/\s/g, '').length % 4 === 0) {
    mimeType = guessPreviewAssetMime(path)
    base64 = raw.replace(/\s/g, '')
  } else {
    mimeType = guessPreviewAssetMime(path)
    try {
      base64 = btoa(unescape(encodeURIComponent(raw)))
    } catch {
      return null
    }
  }
  if (!base64) return null
  return {
    path,
    mimeType,
    size: byteLengthFromBase64(base64),
    base64,
  }
}

function argsOf(message: TappMessage): unknown[] {
  return (message.payload as { args?: unknown[] } | undefined)?.args || []
}

/** Preview `Tapp.context.getApp` — host fields plus no invented `mode: page`. */
export function previewContextApp(tapp: TappInstance) {
  return {
    version: tapp.manifest.version,
    locale: 'en-US',
    theme: 'system',
    features: { aiEnabled: false, platforms: [] as string[] },
  }
}

/** Preview `Tapp.context.getSystem` — production fields plus the preview marker. */
export function previewContextSystem() {
  return {
    online: true,
    serverConnected: true,
    version: 'preview',
    backgroundTasks: [] as unknown[],
    lastFetch: {},
    preview: true,
    runtime: 'tapp-playground',
  }
}

export function previewContextUser(tapp: TappInstance) {
  const role = tapp.userRole
  return {
    id: 'user_preview',
    username: 'preview',
    display_name: 'Preview',
    avatar: null as string | null,
    avatar_url: null as string | null,
    isAdmin: role === 'admin',
    role,
    authenticated: role !== 'guest',
    connectedPlatforms: [] as string[],
    preferences: { language: 'en-US', timezone: 'UTC' },
  }
}

export function previewContextNavigation() {
  return {
    currentPath: '/tapp/playground',
    previousPath: null,
    history: [] as unknown[],
    availableRoutes: [] as unknown[],
    tappPages: [] as unknown[],
    params: {},
  }
}

export function previewContextPlayer() {
  return {
    isPlaying: false,
    isPaused: false,
    currentTrack: null,
    progress: { current: 0, duration: 0, percentage: 0 },
    playlist: null,
    mode: 'sequence',
    volume: 80,
    muted: false,
  }
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
  packageAssets: Record<string, string> = {},
  sharedStore?: PreviewStore,
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
    emitTappStorageChange({
      tappId: tappInstance.id,
      key,
      operation: 'set',
      source: bridge,
    })
    return { success: true, data: null }
  })
  bridge.registerHandler('storage.remove', async (message) => {
    const key = validateKey(argsOf(message)[0])
    if (!key) return { success: false, error: 'Invalid storage key' }
    storage.delete(key)
    emitTappStorageChange({
      tappId: tappInstance.id,
      key,
      operation: 'remove',
      source: bridge,
    })
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
    emitTappStorageChange({
      tappId: tappInstance.id,
      operation: 'clear',
      source: bridge,
    })
    return { success: true, data: null }
  })
  bridge.registerHandler('storage.usage', async () => {
    const used = new Blob([JSON.stringify(Object.fromEntries(storage))]).size
    return {
      success: true,
      data: { used, quota: 8 * 1024 * 1024 },
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
    emitTappSettingsChange({
      tappId: tappInstance.id,
      key,
      operation: 'set',
      source: bridge,
    })
    return { success: true, data: null }
  })
  bridge.registerHandler('settings.getAll', async () => ({
    success: true,
    data: Object.fromEntries(settings),
  }))

  const shared: PreviewStore = sharedStore ?? new Map()
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
    emitTappSharedChange({
      tappId: tappInstance.id,
      key,
      operation: 'set',
      source: bridge,
    })
    return { success: true, data: null }
  })
  bridge.registerHandler('shared.remove', async (message) => {
    const key = validateKey(argsOf(message)[0])
    if (!key) return { success: false, error: 'Invalid shared key' }
    shared.delete(key)
    emitTappSharedChange({
      tappId: tappInstance.id,
      key,
      operation: 'remove',
      source: bridge,
    })
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
    emitTappSharedChange({
      tappId: tappInstance.id,
      operation: 'clear',
      source: bridge,
    })
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
  bridge.registerHandler('assets.list', async () => {
    const declared = Array.isArray(tappInstance.manifest.assets)
      ? tappInstance.manifest.assets.slice()
      : Object.keys(packageAssets)
    return { success: true, data: declared }
  })
  bridge.registerHandler('assets.get', async (message) => {
    const [pathArg] = argsOf(message)
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
    if (Array.isArray(declared) && !declared.includes(pathArg)) {
      return { success: false, error: `Asset not declared: ${pathArg}` }
    }
    const raw = packageAssets[pathArg]
    if (typeof raw !== 'string') {
      return {
        success: false,
        error: 'Package assets are unavailable in temporary preview',
      }
    }
    const payload = previewAssetFromPackage(pathArg, raw)
    if (!payload) {
      return { success: false, error: 'Invalid preview asset payload' }
    }
    return { success: true, data: payload }
  })
  bridge.registerHandler('api.list', async () => ({ success: true, data: [] }))
  bridge.registerHandler('api.execute', async () => ({
    success: false,
    error: 'Declared APIs are disabled in temporary preview',
  }))

  bridge.registerHandler('context.getApp', async () => ({
    success: true,
    data: previewContextApp(tappInstance),
  }))
  bridge.registerHandler('context.getUser', async () => ({
    success: true,
    data: previewContextUser(tappInstance),
  }))
  bridge.registerHandler('context.getNavigation', async () => ({
    success: true,
    data: previewContextNavigation(),
  }))
  bridge.registerHandler('context.getSystem', async () => ({
    success: true,
    data: previewContextSystem(),
  }))
  bridge.registerHandler('context.getPlayer', async () => ({
    success: true,
    data: previewContextPlayer(),
  }))
  bridge.registerHandler('context.getGeo', async () => ({
    success: true,
    data: null,
  }))
  bridge.registerHandler('persona.get', async () => ({
    success: true,
    data: {
      enabled: true,
      name: 'Arael',
      moodBand: 'calm',
      activity: 'idle',
      portraitUrl: null,
    },
  }))
}
