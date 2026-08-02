/**
 * 强制清空前端侧缓存（内存 / 本地存储 / Cache Storage / Service Worker）并硬重载。
 *
 * 刻意保留（用户偏好与会话提示，不登出）：
 * - 主题、语言、动画偏好、设置收藏
 * - 阅读器 / TTS 等客户端配置
 * - 登录 Cookie / 会话提示（session hint）
 * - TApp Playground 会话数据
 * - 「用户已拒绝定位」标记（避免反复弹窗）
 *
 * 足够「强制」的要点：
 * 1) 清内存层 API / 去重 / 资源 / 媒体缓存
 * 2) 按已知键 + 缓存启发式清 localStorage（保留偏好白名单）
 * 3) 清空 sessionStorage 中的临时/缓存类数据（整表清，偏好在 localStorage）
 * 4) 通知 SW CLEAR_CACHE → 再 unregister 全部 SW → 再清一遍 Cache Storage
 * 5) 带 `_cache_bust` 的 location.replace，避开 SPA 状态与 bfcache
 */

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

/**
 * 清 localStorage 缓存层：已知键 + 前缀 + 启发式，跳过偏好白名单。
 */
export function clearLocalStorageCaches(): number {
  let removed = 0

  for (const key of KNOWN_LOCAL_CACHE_KEYS) {
    if (safeRemoveLocalKey(key)) removed++
  }

  try {
    // 快照 keys，删除过程中 length 会变
    const keys = Object.keys(localStorage)
    for (const key of keys) {
      if (shouldRemoveLocalCacheKey(key) && safeRemoveLocalKey(key)) {
        removed++
      }
    }
  } catch {
    // 私密模式等
  }

  return removed
}

/**
 * sessionStorage 几乎全是临时态 / CSRF / 歌单缓存；整表清空最彻底。
 * 用户偏好在 localStorage，登录在 Cookie。
 */
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
    // 二次确认：某些实现 delete 后 keys 仍短暂残留
    const remaining = await caches.keys()
    if (remaining.length > 0) {
      await Promise.all(remaining.map((key) => caches.delete(key)))
    }
    return keys.length
  } catch {
    return 0
  }
}

/**
 * 通过 MessageChannel 通知 SW 清空 Cache（与 public/sw.js 的 CLEAR_CACHE 约定一致）。
 */
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

/**
 * 注销全部 Service Worker，避免重载后旧 SW 继续劫持静态资源与 API。
 * 这是「强制」的核心：仅清 Cache 而不 unregister，下一请求仍可能被 SW 拦截。
 */
async function unregisterAllServiceWorkers(): Promise<number> {
  if (typeof navigator === 'undefined' || !('serviceWorker' in navigator)) {
    return 0
  }

  try {
    const registrations = await navigator.serviceWorker.getRegistrations()
    await Promise.all(
      registrations.map(async (registration) => {
        try {
          // 尽量先更新再注销，避免卡住 waiting worker
          try {
            await registration.update()
          } catch {
            // ignore
          }
          await registration.unregister()
        } catch {
          // 单个失败不阻断
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

/**
 * 清空前端所有缓存层。不会清除偏好设置，也不会登出。
 * 会注销 Service Worker（PROD 下次 load 会重新 register）。
 */
export async function purgeFrontendCaches(): Promise<FrontendCachePurgeResult> {
  const warnings: string[] = []

  clearInMemoryCaches(warnings)

  const localKeysRemoved = clearLocalStorageCaches()
  const sessionKeysRemoved = clearSessionStorageCaches()

  // 1) 先让 SW 自己清 Cache
  const serviceWorkerNotified = await notifyServiceWorkerClearCache()

  // 2) 页面侧清 Cache Storage
  let cacheStorageCleared = await clearCacheStorage()

  // 3) 注销全部 SW（关键强制：否则旧 SW 仍控制页面）
  const serviceWorkersUnregistered = await unregisterAllServiceWorkers()

  // 4) 注销后再清一次 Cache Storage（activate 竞态可能残留）
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

/**
 * 硬导航：去掉旧 bust、写入新时间戳，replace 避免 bfcache 回退到脏 SPA 状态。
 * 先用 cache:'reload' 预取文档（尽量穿透 HTTP 磁盘缓存），再 replace；预取超时仍强制跳转。
 */
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

  // 预取卡住时不能无限等
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

/**
 * 清空缓存并强制整页刷新。
 * @param delayMs 提示展示后再导航的等待时间
 */
export async function purgeFrontendCachesAndReload(
  delayMs: number = 600,
): Promise<FrontendCachePurgeResult> {
  const result = await purgeFrontendCaches()

  window.setTimeout(() => {
    hardNavigateWithCacheBust()
  }, delayMs)

  return result
}
