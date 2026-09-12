/** Preserve prefs/session; clear memory, Cache Storage, SW. */

import { clearColorCache as clearExtractorColorCache } from './colorExtractor'
import { clearCSRFToken } from './csrf'
import {
  KNOWN_LOCAL_CACHE_KEYS,
  shouldRemoveLocalCacheKey,
} from './frontendCacheKeys'
import { resetGeoCache } from './geoLocation'
import {
  clearLyricsCache,
  clearPlaylistCache,
} from './musicPlayer'
import { MemoryManager } from './performance'
import { requestCache } from './requestCache'
import { clearDedupCache } from './requestDedup'
import { globalResourceLoader } from './resourceLoader'
import {
  clearAllUserCache,
  invalidateCsrfCache,
} from './userInfoCache'
import { clearColorCache as clearWallpaperColorCache } from './wallpaperColorCache'

export interface FrontendCachePurgeResult {
  localKeysRemoved: number
  sessionKeysRemoved: number
  cacheStorageCleared: number
  serviceWorkersUnregistered: number
  serviceWorkerNotified: boolean
  warnings: string[]
}

function safeRemoveLocalKey(key: string): boolean {
  try {
    if (localStorage.getItem(key) === null) return false
    localStorage.removeItem(key)
    return true
  } catch {
    return false
  }
}

export function clearLocalStorageCaches(): number {
  let removed = 0

  for (const key of KNOWN_LOCAL_CACHE_KEYS) {
    if (safeRemoveLocalKey(key)) removed++
  }

  try {
    // Snapshot keys; length changes while deleting.
    const keys = Object.keys(localStorage)
    for (const key of keys) {
      if (shouldRemoveLocalCacheKey(key) && safeRemoveLocalKey(key)) {
        removed++
      }
    }
  } catch {
  }

  return removed
}

/** sessionStorage is ephemeral; prefs live in localStorage. */
function clearSessionStorageCaches(): number {
  try {
    const n = sessionStorage.length
    sessionStorage.clear()
    return n
  } catch {
    return 0
  }
}

async function clearCacheStorage(): Promise<number> {
  if (typeof caches === 'undefined') return 0
  try {
    const keys = await caches.keys()
    await Promise.all(keys.map((key) => caches.delete(key)))
    // Some implementations keep keys briefly after delete.
    const remaining = await caches.keys()
    if (remaining.length > 0) {
      await Promise.all(remaining.map((key) => caches.delete(key)))
    }
    return keys.length
  } catch {
    return 0
  }
}

/** MessageChannel CLEAR_CACHE, then unregister SW. */
function notifyServiceWorkerClearCache(): Promise<boolean> {
  if (
    typeof navigator === 'undefined' ||
    !('serviceWorker' in navigator) ||
    !navigator.serviceWorker.controller
  ) {
    return Promise.resolve(false)
  }

  return new Promise((resolve) => {
    try {
      const controller = navigator.serviceWorker.controller
      if (!controller) {
        resolve(false)
        return
      }

      const channel = new MessageChannel()
      const timer = window.setTimeout(() => {
        resolve(false)
      }, 3000)

      channel.port1.onmessage = (event) => {
        window.clearTimeout(timer)
        resolve(Boolean(event.data?.success))
      }

      controller.postMessage({ type: 'CLEAR_CACHE' }, [channel.port2])
    } catch {
      resolve(false)
    }
  })
}

/** Must unregister SW; clearing Cache alone is not enough. */
async function unregisterAllServiceWorkers(): Promise<number> {
  if (typeof navigator === 'undefined' || !('serviceWorker' in navigator)) {
    return 0
  }

  try {
    const registrations = await navigator.serviceWorker.getRegistrations()
    await Promise.all(
      registrations.map(async (registration) => {
        try {
          try {
            await registration.update()
          } catch {
            // ignore
          }
          await registration.unregister()
        } catch {
        }
      }),
    )
    return registrations.length
  } catch {
    return 0
  }
}

function clearInMemoryCaches(warnings: string[]): void {
  const steps: Array<[string, () => void]> = [
    ['requestCache', () => requestCache.clear()],
    ['requestDedup', () => clearDedupCache()],
    ['userInfo', () => clearAllUserCache()],
    ['csrfMemory', () => invalidateCsrfCache()],
    ['csrfSession', () => clearCSRFToken()],
    ['wallpaperColor', () => clearWallpaperColorCache()],
    ['colorExtractor', () => clearExtractorColorCache()],
    [
      'musicPlayer',
      () => {
        clearPlaylistCache()
        clearLyricsCache()
      },
    ],
    ['geo', () => resetGeoCache()],
    ['resourceLoader', () => globalResourceLoader.reset()],
    ['memoryManager', () => MemoryManager.clear()],
  ]

  for (const [name, run] of steps) {
    try {
      run()
    } catch (error) {
      warnings.push(
        `${name}: ${error instanceof Error ? error.message : String(error)}`,
      )
    }
  }
}

export async function purgeFrontendCaches(): Promise<FrontendCachePurgeResult> {
  const warnings: string[] = []

  clearInMemoryCaches(warnings)

  const localKeysRemoved = clearLocalStorageCaches()
  const sessionKeysRemoved = clearSessionStorageCaches()

  const serviceWorkerNotified = await notifyServiceWorkerClearCache()

  let cacheStorageCleared = await clearCacheStorage()

  const serviceWorkersUnregistered = await unregisterAllServiceWorkers()

  // Clear Cache Storage again after unregister (activate race).
  cacheStorageCleared += await clearCacheStorage()

  if (
    !serviceWorkerNotified &&
    typeof navigator !== 'undefined' &&
    'serviceWorker' in navigator &&
    navigator.serviceWorker.controller
  ) {
    warnings.push('serviceWorker: CLEAR_CACHE 未确认')
  }

  return {
    localKeysRemoved,
    sessionKeysRemoved,
    cacheStorageCleared,
    serviceWorkersUnregistered,
    serviceWorkerNotified,
    warnings,
  }
}

/** location.replace with cache bust; avoid bfcache. */
function hardNavigateWithCacheBust(): void {
  const url = new URL(window.location.href)
  url.searchParams.delete('_cache_bust')
  url.searchParams.set('_cache_bust', String(Date.now()))
  const target = url.toString()

  let navigated = false
  const go = () => {
    if (navigated) return
    navigated = true
    window.location.replace(target)
  }

  // Do not wait forever on prefetch.
  window.setTimeout(go, 1500)

  try {
    void fetch(target, {
      cache: 'reload',
      credentials: 'same-origin',
      headers: {
        'Cache-Control': 'no-cache',
        Pragma: 'no-cache',
      },
    }).finally(go)
  } catch {
    go()
  }
}

export async function purgeFrontendCachesAndReload(
  delayMs: number = 600,
): Promise<FrontendCachePurgeResult> {
  const result = await purgeFrontendCaches()

  window.setTimeout(() => {
    hardNavigateWithCacheBust()
  }, delayMs)

  return result
}
