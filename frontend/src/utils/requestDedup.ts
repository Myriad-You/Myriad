/**
 * 请求去重工具
 *
 * 解决多个组件同时请求相同 API 导致的重复网络请求问题。
 * 使用 Promise 共享机制，确保相同的请求在短时间内只发起一次。
 */

// 进行中的请求缓存
// 预定义的常用 API 去重函数

import { API_URL } from '../config'
import { ApiError } from '../services/api'
import { normalizeJsonMediaUrls } from './proxyImageUrl'
import { httpStatusMessage } from './userFacingError'

const pendingRequests = new Map<string, Promise<any>>()

// 已完成请求的结果缓存
const resultCache = new Map<string, { data: any; timestamp: number }>()

/**
 * 每 key 代际：clearDedupCache 后 +1。
 * 防止「清缓存时仍在飞的旧请求完成时把过期数据写回 resultCache」
 * （保存壁纸/Evocative 后软刷新若撞上启动期的 config/ui 飞行请求，会读回旧开关）
 */
const cacheGeneration = new Map<string, number>()

// LRU 缓存最大容量
const MAX_CACHE_SIZE = 50

// 默认缓存时间（毫秒）
const DEFAULT_CACHE_TTL = 30 * 1000 // 30秒

function generationOf(key: string): number {
  return cacheGeneration.get(key) ?? 0
}

function bumpGeneration(key: string): void {
  cacheGeneration.set(key, generationOf(key) + 1)
}

/**
 * 🔧 LRU 缓存清理 - 删除最早的条目直到缓存大小正常
 */
function ensureCacheSize() {
  if (resultCache.size <= MAX_CACHE_SIZE) return

  // Map 保持插入顺序，所以第一个就是最早的
  const keysToDelete: string[] = []
  const deleteCount = resultCache.size - MAX_CACHE_SIZE

  let count = 0
  for (const key of resultCache.keys()) {
    if (count >= deleteCount) break
    keysToDelete.push(key)
    count++
  }

  keysToDelete.forEach((key) => resultCache.delete(key))
}

/**
 * 🔧 读取缓存并更新 LRU 顺序
 */
function getCacheWithLRU(
  key: string,
): { data: any; timestamp: number } | undefined {
  const cached = resultCache.get(key)
  if (cached) {
    // 删除并重新插入，使其移到末尾（最新）
    resultCache.delete(key)
    resultCache.set(key, cached)
  }
  return cached
}

export interface DedupOptions {
  /** 缓存时间（毫秒），默认 30 秒 */
  cacheTTL?: number
  /** 是否跳过缓存，强制重新请求 */
  forceRefresh?: boolean
  /** 缓存键（默认使用 URL） */
  cacheKey?: string
}

/**
 * 去重请求包装器
 *
 * @param url 请求 URL
 * @param fetchFn 实际的 fetch 函数
 * @param options 配置选项
 * @returns Promise<T>
 *
 * @example
 * ```ts
 * // 基本用法
 * const data = await dedupedFetch('/api/config/ui', () =>
 *   fetch('/api/config/ui').then(r => r.json())
 * );
 *
 * // 自定义缓存时间
 * const weather = await dedupedFetch('/api/weather', fetchWeather, {
 *   cacheTTL: 5 * 60 * 1000 // 5分钟
 * });
 * ```
 */
export async function dedupedFetch<T>(
  url: string,
  fetchFn: () => Promise<T>,
  options: DedupOptions = {},
): Promise<T> {
  const {
    cacheTTL = DEFAULT_CACHE_TTL,
    forceRefresh = false,
    cacheKey = url,
  } = options

  // 1. 检查结果缓存（非强制刷新时）-  使用 LRU 读取
  if (!forceRefresh) {
    const cached = getCacheWithLRU(cacheKey)
    if (cached && Date.now() - cached.timestamp < cacheTTL) {
      return cached.data as T
    }
  }

  // 2. 检查是否有进行中的相同请求
  const pending = pendingRequests.get(cacheKey)
  if (pending) {
    return pending as Promise<T>
  }

  // 3. 发起新请求（记录代际，避免 clear 后旧响应污染缓存）
  const genAtStart = generationOf(cacheKey)
  const requestPromise = fetchFn()
    .then((data) => {
      if (generationOf(cacheKey) === genAtStart) {
        ensureCacheSize()
        resultCache.set(cacheKey, { data, timestamp: Date.now() })
      }
      return data
    })
    .finally(() => {
      // 仅清除「自己」登记的 pending，避免清缓存后新请求被误删
      if (pendingRequests.get(cacheKey) === requestPromise) {
        pendingRequests.delete(cacheKey)
      }
    })

  // 4. 记录进行中的请求
  pendingRequests.set(cacheKey, requestPromise)

  return requestPromise
}

/**
 * 清除指定 URL 的缓存
 */
export function clearDedupCache(url?: string): void {
  if (url) {
    bumpGeneration(url)
    resultCache.delete(url)
    pendingRequests.delete(url)
  } else {
    for (const key of resultCache.keys()) bumpGeneration(key)
    for (const key of pendingRequests.keys()) bumpGeneration(key)
    resultCache.clear()
    pendingRequests.clear()
  }
}

/** 清除同一端点的所有分页/筛选变体，并使仍在飞行的旧请求失效。 */
export function clearDedupCacheByPrefix(prefix: string): void {
  const keys = new Set<string>()
  for (const key of resultCache.keys()) {
    if (key.startsWith(prefix)) keys.add(key)
  }
  for (const key of pendingRequests.keys()) {
    if (key.startsWith(prefix)) keys.add(key)
  }
  for (const key of keys) {
    bumpGeneration(key)
    resultCache.delete(key)
    pendingRequests.delete(key)
  }
}

export function clearLibraryDataCache(): void {
  const endpoint = `${API_URL}/api/library`
  clearDedupCache(endpoint)
  clearDedupCacheByPrefix(`${endpoint}?`)
}

/**
 * 预热缓存（后台预加载）
 */
export function prefetchDedup<T>(
  url: string,
  fetchFn: () => Promise<T>,
  options?: DedupOptions,
): void {
  // 使用 requestIdleCallback 或 setTimeout 延迟执行
  const prefetch = () => {
    dedupedFetch(url, fetchFn, options).catch(() => {
      // 预热失败静默处理
    })
  }

  if ('requestIdleCallback' in window) {
    ;(window as any).requestIdleCallback(prefetch, { timeout: 5000 })
  } else {
    setTimeout(prefetch, 1000)
  }
}

/**
 * 获取 UI 配置（去重）
 * 缓存 30 秒
 */
export async function getUIConfigDeduped(): Promise<any> {
  return dedupedFetch(
    `${API_URL}/api/config/ui`,
    async () => {
      const response = await fetch(`${API_URL}/api/config/ui`)
      if (!response.ok) {
        throw new ApiError(httpStatusMessage(response.status), response.status)
      }
      return response.json()
    },
    { cacheTTL: 30 * 1000 },
  )
}

/**
 * 获取最新报告（去重）
 * 有有效 platform_reports 时缓存 30 秒；空结果不缓存，避免首页在生成报告后
 * 仍读到「无报告」缓存导致 ReportCard 空白。
 */
export async function getLatestReportDeduped(
  options: { forceRefresh?: boolean } = {},
): Promise<any> {
  const cacheKey = `${API_URL}/api/reports/latest`
  const data = await dedupedFetch(
    cacheKey,
    async () => {
      const response = await fetch(`${API_URL}/api/reports/latest`, {
        credentials: 'include',
      })
      if (!response.ok) {
        throw new ApiError(httpStatusMessage(response.status), response.status)
      }
      return response.json()
    },
    { cacheTTL: 30 * 1000, forceRefresh: options.forceRefresh },
  )

  // Drop empty / failed payloads from the result cache so the next home widget
  // mount re-fetches after the owner generates reports.
  const reports = data?.platform_reports
  const empty =
    !data ||
    data.success === false ||
    !Array.isArray(reports) ||
    reports.length === 0
  if (empty) {
    clearDedupCache(cacheKey)
  }
  return data
}

/** Invalidate cached latest-report payload (call after generate). */
export function invalidateLatestReportCache(): void {
  clearDedupCache(`${API_URL}/api/reports/latest`)
}

/**
 * 获取公开平台配置（去重）
 * 首页多个小组件（社交网络、报告卡片）都需要同一份数据
 * 缓存 30 秒
 */
export async function getPublicConfigDeduped(): Promise<any> {
  return dedupedFetch(
    `${API_URL}/api/config/public`,
    async () => {
      const response = await fetch(`${API_URL}/api/config/public`)
      if (!response.ok) {
        throw new ApiError(httpStatusMessage(response.status), response.status)
      }
      return response.json()
    },
    { cacheTTL: 30 * 1000 },
  )
}

export function invalidatePublicConfigCache(): void {
  clearDedupCache(`${API_URL}/api/config/public`)
}

/**
 * 获取设置状态（去重）
 * 缓存 1 分钟
 */
export async function getSetupStatusDeduped(): Promise<any> {
  return dedupedFetch(
    `${API_URL}/api/setup/status`,
    async () => {
      const response = await fetch(`${API_URL}/api/setup/status`)
      if (!response.ok) {
        throw new ApiError(httpStatusMessage(response.status), response.status)
      }
      return response.json()
    },
    { cacheTTL: 60 * 1000 },
  )
}

/**
 * 获取库数据（去重）
 * 缓存 2 分钟（数据量大，减少请求）
 * 入口统一 normalizeJsonMediaUrls，封面与报告卡同一套防盗链规则。
 */
export async function getLibraryDataDeduped(): Promise<any> {
  return dedupedFetch(
    `${API_URL}/api/library`,
    async () => {
      const response = await fetch(`${API_URL}/api/library`, {
        credentials: 'include',
        signal: AbortSignal.timeout(30000), // 30秒超时（数据量大）
      })
      if (!response.ok) {
        throw new ApiError(httpStatusMessage(response.status), response.status)
      }
      const data = await response.json()
      return normalizeJsonMediaUrls(data)
    },
    { cacheTTL: 2 * 60 * 1000 }, // 2分钟
  )
}

/**
 * 分批获取资料库数据。每个 offset 使用独立去重键，画布可以按需扩展，
 * 同时保留 getLibraryDataDeduped() 的全量兼容语义给统计组件。
 */
export async function getLibraryDataPageDeduped(
  offset: number,
  limit: number,
  itemType?: string,
): Promise<any> {
  const safeOffset = Math.max(0, Math.floor(offset))
  const safeLimit = Math.min(200, Math.max(1, Math.floor(limit)))
  const params = new URLSearchParams({
    offset: String(safeOffset),
    limit: String(safeLimit),
  })
  if (itemType && itemType !== 'all') params.set('type', itemType)
  const url = `${API_URL}/api/library?${params.toString()}`
  return dedupedFetch(
    url,
    async () => {
      const response = await fetch(url, {
        credentials: 'include',
        signal: AbortSignal.timeout(30000),
      })
      if (!response.ok) {
        throw new ApiError(httpStatusMessage(response.status), response.status)
      }
      const data = await response.json()
      return normalizeJsonMediaUrls(data)
    },
    { cacheTTL: 2 * 60 * 1000 },
  )
}
