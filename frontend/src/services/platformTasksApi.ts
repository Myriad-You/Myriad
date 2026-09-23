import { currentCopy } from '../i18n/localeCopy'
import { apiService } from './api'

export interface Task {
  id: string
  platform: string
  status: 'Pending' | 'Processing' | 'Completed' | 'Failed'
  progress: number
  error?: string
  created_at: string
  updated_at: string
  completed_at?: string
}

/** Scraping a platform can outlast the shared client's default 30s budget. */
const PLATFORM_FETCH_TIMEOUT_MS = 10 * 60_000

export interface CacheInfo {
  platform: string
  exists: boolean
  size_bytes?: number
  modified_at?: string
  path: string
}

export interface PlatformMetadataStatus {
  success: boolean
  has_raw_data: boolean
  raw_data_size: number
  raw_fetched_at: string | null
}

/** A 2xx reply; `success: false` with `partial` still means some data was stored. */
export interface PlatformFetchResult {
  success: boolean
  partial?: boolean
  issues?: unknown
  message?: string
}

export interface TaskEnvelope {
  success: boolean
  task?: Task
  error?: string
}

interface Envelope {
  success?: boolean
  error?: string
}

/** These endpoints also report failure as `success: false` inside a 2xx body. */
function requireSuccess<T extends Envelope>(data: T): T {
  if (!data.success) throw new Error(data.error || currentCopy().errors.requestFailed)
  return data
}

export function getPlatformMetadataStatus(platformId: string): Promise<PlatformMetadataStatus> {
  return apiService.get(`/profile/metadata/status/${encodeURIComponent(platformId)}`)
}

/** Resolves null when the platform has no cache entry yet. */
export async function getPlatformCacheStatus(platformId: string): Promise<CacheInfo | null> {
  const data = requireSuccess(
    await apiService.get<Envelope & { cache?: CacheInfo }>(`/cache/status/${platformId}`),
  )
  return data.cache ?? null
}

export async function clearPlatformCache(platformId: string): Promise<void> {
  requireSuccess(await apiService.delete<Envelope>(`/cache/${platformId}`))
}

/** Queues background processing; resolves the task id to poll. */
export async function submitPlatformTask(platformId: string): Promise<string> {
  const data = requireSuccess(
    await apiService.post<Envelope & { task_id?: string }>('/tasks', { platform: platformId }),
  )
  if (!data.task_id) throw new Error(currentCopy().errors.requestFailed)
  return data.task_id
}

export function fetchPlatformData(platformId: string): Promise<PlatformFetchResult> {
  return apiService.post('/profile/fetch-platform', { platform: platformId }, {
    timeout: PLATFORM_FETCH_TIMEOUT_MS,
  })
}

export function getTask(taskId: string, signal?: AbortSignal): Promise<TaskEnvelope> {
  return apiService.get(`/tasks/${taskId}`, { signal })
}

export interface PlatformReportsResult<Report> {
  success?: boolean
  message?: string
  skipped?: Array<{ platform?: string, reason?: string }>
  reports?: Report[]
}

/** AI generation; the shared client already gives this route the long AI time budget. */
export function generatePlatformReports<Report>(platformIds: string[]): Promise<PlatformReportsResult<Report>> {
  return apiService.post('/reports/platform', { platforms: platformIds })
}
