import type { ColorPalette } from './colorPalette'
import { isDefaultPalette } from './colorPalette'
import {
  areUrlsEquivalent,
  extractBackgroundUrl,
  normalizeWallpaperUrl,
  wallpaperState,
} from './wallpaperState'

interface WallpaperColorCacheItem {
  url: string
  palette: ColorPalette
  timestamp: number
  accessCount: number
}

interface WallpaperColorCacheStore {
  version: number
  items: WallpaperColorCacheItem[]
}

interface ColorExtractionCheckResult {
  shouldApply: boolean
  cacheKey?: string
  reason?: string
}

const CACHE_VERSION = 6
const CACHE_DURATION_MS = 6 * 60 * 60 * 1000 // 6h
const CACHE_KEY = 'myriad_wallpaper_color_cache_v6'
const MAX_CACHE_ITEMS = 10

function getCacheStore(): WallpaperColorCacheStore | null {
  try {
    const cached = localStorage.getItem(CACHE_KEY)
    if (!cached) return null

    const store: WallpaperColorCacheStore = JSON.parse(cached)
    if (store.version !== CACHE_VERSION) {
      // Drop cache on version mismatch.
      localStorage.removeItem(CACHE_KEY)
      return null
    }

    return store
  } catch {
    try {
      localStorage.removeItem(CACHE_KEY)
    } catch {
      /* 忽略 */
    }
    return null
  }
}

function saveCacheStore(store: WallpaperColorCacheStore): boolean {
  try {
    localStorage.setItem(CACHE_KEY, JSON.stringify(store))
    return true
  } catch (error) {
    console.warn('保存颜色缓存失败:', error)
    return false
  }
}

function cleanupCacheStore(store: WallpaperColorCacheStore): void {
  const now = Date.now()

  store.items = store.items.filter(
    (item) => now - item.timestamp < CACHE_DURATION_MS,
  )

  if (store.items.length > MAX_CACHE_ITEMS) {
    store.items = store.items
      .toSorted((a, b) => {
        if (b.accessCount !== a.accessCount) {
          return b.accessCount - a.accessCount
        }
        return b.timestamp - a.timestamp
      })
      .slice(0, MAX_CACHE_ITEMS)
  }
}

export async function shouldApplyColorExtraction(
  url: string,
): Promise<ColorExtractionCheckResult> {
  if (!url) {
    return { shouldApply: false, reason: 'URL为空' }
  }

  if (url.includes('/api/proxy/music/')) {
    return { shouldApply: false, reason: '音乐封面URL' }
  }
  if (url.startsWith('file://')) {
    return { shouldApply: false, reason: 'file://协议不支持' }
  }

  if (!wallpaperState.isUrlActive(url)) {
    const activeUrl = wallpaperState.getActiveUrl()
    console.debug('[ColorCache] URL mismatch:', {
      provided: url.slice(0, 60),
      active: activeUrl?.slice(0, 60),
    })
    return {
      shouldApply: false,
      reason: activeUrl ? `URL与当前壁纸不一致` : '没有活跃壁纸',
    }
  }

  const domUrl = extractBackgroundUrl()
  if (domUrl && !areUrlsEquivalent(domUrl, url)) {
    console.debug('[ColorCache] DOM URL mismatch (may be timing issue):', {
      provided: url.slice(0, 60),
      dom: domUrl.slice(0, 60),
    })
  }

  // Do not preload just to read size.
  return {
    shouldApply: true,
    cacheKey: normalizeWallpaperUrl(url),
  }
}

export function getColorFromCache(url: string): ColorPalette | null {
  try {
    const normalizedUrl = normalizeWallpaperUrl(url)
    const store = getCacheStore()
    if (!store) return null

    const item = store.items.find((i) => i.url === normalizedUrl)
    if (!item) return null

    const age = Date.now() - item.timestamp
    if (age > CACHE_DURATION_MS) {
      store.items = store.items.filter((i) => i.url !== normalizedUrl)
      saveCacheStore(store)
      return null
    }

    // Placeholder grey is a miss.
    if (isDefaultPalette(item.palette)) {
      store.items = store.items.filter((i) => i.url !== normalizedUrl)
      saveCacheStore(store)
      return null
    }

    item.accessCount++
    saveCacheStore(store)

    return item.palette
  } catch {
    return null
  }
}

export function saveColorToCache(url: string, palette: ColorPalette): void {
  // Do not cache placeholder grey.
  if (isDefaultPalette(palette)) return

  try {
    const normalizedUrl = normalizeWallpaperUrl(url)
    let store = getCacheStore()

    if (!store) {
      store = { version: CACHE_VERSION, items: [] }
    }

    store.items = store.items.filter((item) => item.url !== normalizedUrl)

    store.items.push({
      url: normalizedUrl,
      palette,
      timestamp: Date.now(),
      accessCount: 1,
    })

    cleanupCacheStore(store)
    saveCacheStore(store)
  } catch {
  }
}

export function clearColorCache(): void {
  try {
    localStorage.removeItem(CACHE_KEY)
  } catch {
  }
}

export function getCacheInfo(): {
  exists: boolean
  count?: number
  totalSize?: number
  items?: Array<{ url: string; age: number; accessCount: number }>
} {
  try {
    const cached = localStorage.getItem(CACHE_KEY)
    if (!cached) return { exists: false }

    const store: WallpaperColorCacheStore = JSON.parse(cached)
    const now = Date.now()

    return {
      exists: true,
      count: store.items.length,
      totalSize: cached.length,
      items: store.items.map((item) => ({
        url:
          item.url.length > 60 ? `${item.url.slice(0, 60)}...` : item.url,
        age: Math.round((now - item.timestamp) / 1000),
        accessCount: item.accessCount,
      })),
    }
  } catch {
    return { exists: false }
  }
}
