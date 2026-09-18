import { API_URL } from '../config'
import { ApiError } from '../services/api'
import { readJsonOk } from './apiHelper'
import { httpStatusMessage } from './httpStatus'
import { normalizeJsonMediaUrls } from './proxyImageUrl'
import { RequestCache } from './requestCache'
import { isUselessErrorText } from './uselessErrorText'

async function fetchDedupedJson(url: string): Promise<any> {
  const response = await fetch(url, {
    credentials: 'include',
    signal: AbortSignal.timeout(30000),
  })
  return readJsonOk(response)
}

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
    () => fetchDedupedJson(`${API_URL}/api/config/ui`),
    { cacheTTL: 30 * 1000 },
  )
}

export async function getLatestReportDeduped(
  options: { forceRefresh?: boolean } = {},
): Promise<any> {
  const cacheKey = `${API_URL}/api/reports/latest`
  const data = await dedupedFetch(
    cacheKey,
    () => fetchDedupedJson(`${API_URL}/api/reports/latest`),
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
    () => fetchDedupedJson(`${API_URL}/api/config/public`),
    { cacheTTL: 30 * 1000 },
  )
}

export function invalidatePublicConfigCache(): void {
  clearDedupCache(`${API_URL}/api/config/public`)
}

export interface LibraryTypeCounts {
  total: number
  game: number
  video: number
  music: number
  anime: number
  tv_series: number
  book: number
}

function asCount(value: unknown): number {
  return typeof value === 'number' && Number.isFinite(value) && value >= 0
    ? Math.floor(value)
    : 0
}

function countsFromTypeMap(
  total: unknown,
  counts: Record<string, unknown>,
): LibraryTypeCounts {
  return {
    total: asCount(total),
    game: asCount(counts.game),
    video: asCount(counts.video),
    music: asCount(counts.music),
    anime: asCount(counts.anime),
    tv_series: asCount(counts.tv_series),
    book: asCount(counts.book),
  }
}

function countsFromItems(items: unknown[]): LibraryTypeCounts {
  const counts: LibraryTypeCounts = {
    total: 0,
    game: 0,
    video: 0,
    music: 0,
    anime: 0,
    tv_series: 0,
    book: 0,
  }
  for (const item of items) {
    if (!item || typeof item !== 'object') continue
    const type = (item as { item_type?: unknown }).item_type
    if (typeof type !== 'string' || !Object.hasOwn(counts, type)) continue
    counts.total++
    counts[type as Exclude<keyof LibraryTypeCounts, 'total'>]++
  }
  return counts
}

export function libraryStatsFromResponse(data: unknown): LibraryTypeCounts | null {
  if (!data || typeof data !== 'object') return null
  const rec = data as Record<string, unknown>
  if (rec.success !== true) return null
  const counts = rec.type_counts
  if (counts && typeof counts === 'object' && !Array.isArray(counts)) {
    return countsFromTypeMap(rec.total, counts as Record<string, unknown>)
  }
  if (Array.isArray(rec.items)) {
    return countsFromItems(rec.items)
  }
  return null
}

export async function getLibraryStatsDeduped(): Promise<LibraryTypeCounts> {
  const url = `${API_URL}/api/library?counts_only=true`
  const data = await dedupedFetch(
    url,
    () => fetchDedupedJson(url),
    { cacheTTL: 2 * 60 * 1000 },
  )
  const stats = libraryStatsFromResponse(data)
  if (!stats) {
    const message =
      data && typeof data === 'object' && typeof data.message === 'string'
        ? data.message.trim()
        : ''
    throw new ApiError(
      message && !isUselessErrorText(message)
        ? message
        : httpStatusMessage(502),
      502,
    )
  }
  return stats
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
    async () => normalizeJsonMediaUrls(await fetchDedupedJson(url)),
    { cacheTTL: 2 * 60 * 1000 },
  )
}
