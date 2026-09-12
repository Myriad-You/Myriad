import { API_URL } from '../config'
import { ApiError } from '../services/api'
import { normalizeJsonMediaUrls } from './proxyImageUrl'
import { RequestCache } from './requestCache'
import { httpStatusMessage } from './userFacingError'

const requestCache = new RequestCache(50)
const DEFAULT_CACHE_TTL = 30 * 1000

export interface DedupOptions {
  /** TTL ms; default 30s */
  cacheTTL?: number
  forceRefresh?: boolean
  cacheKey?: string
}

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

  return requestCache.fetch(cacheKey, fetchFn, cacheTTL, forceRefresh)
}

export function clearDedupCache(url?: string): void {
  if (url) requestCache.delete(url)
  else requestCache.clear()
}

export function clearDedupCacheByPrefix(prefix: string): void {
  requestCache.deleteByPrefix(prefix)
}

export function clearLibraryDataCache(): void {
  const endpoint = `${API_URL}/api/library`
  clearDedupCache(endpoint)
  clearDedupCacheByPrefix(`${endpoint}?`)
}

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

  // Do not cache empty/failed payloads.
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

export function invalidateLatestReportCache(): void {
  clearDedupCache(`${API_URL}/api/reports/latest`)
}

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

export async function getLibraryDataDeduped(): Promise<any> {
  return dedupedFetch(
    `${API_URL}/api/library`,
    async () => {
      const response = await fetch(`${API_URL}/api/library`, {
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
