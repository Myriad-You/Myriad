import { currentCopy } from '../i18n/localeCopy'
import { userFacingError } from '../utils/userFacingError'
import { apiService } from './api'

/** Source uploads are unbounded by the default 30s request budget. */
const UPLOAD_TIMEOUT_MS = 10 * 60_000

export type TripoOperation =
  'image_to_model' | 'multiview_to_model' | 'rig_check' | 'rig' | 'retarget'

export interface TripoStatus {
  enabled: boolean
  configured: boolean
  base_url: string
  model: string
  face_limit: number
  poll_interval_seconds: number
  task_timeout_seconds: number
  max_download_mb: number
  capabilities: string[]
}

export interface TripoGlbMetrics {
  byte_length: number
  scene_count: number
  node_count: number
  mesh_count: number
  primitive_count: number
  material_count: number
  texture_count: number
  image_count: number
  skin_count: number
  joint_count: number
  animation_count: number
  accessor_count: number
  estimated_triangles: number | null
  web_budget: 'ready' | 'review' | string
  warnings: string[]
}

export interface PersistedTripoAsset {
  asset_id: string
  model_url: string
  metadata_url: string
  output_index: number
  metrics: TripoGlbMetrics
}

export interface TripoTask {
  task_id: string
  task_type: string
  status:
    | 'queued'
    | 'running'
    | 'success'
    | 'failed'
    | 'cancelled'
    | 'banned'
    | string
  progress: number
  output: Record<string, unknown>
  credits_consumed?: number | null
  error_code?: number | null
  error_message?: string | null
  created_at?: string | null
  completed_at?: string | null
  asset?: PersistedTripoAsset
  assets?: PersistedTripoAsset[]
}

export interface AwaitTripoTaskOptions {
  intervalMs?: number
  timeoutMs?: number
  signal?: AbortSignal
  onProgress?: (task: TripoTask) => void
}

export function getTripoStatus(): Promise<TripoStatus> {
  return apiService.get<TripoStatus>('/model3d/status')
}

export async function uploadTripoFile(file: File): Promise<string> {
  const form = new FormData()
  form.append('file', file, file.name)
  let data: { file_token?: string }
  try {
    data = await apiService.post('/model3d/files', form, { timeout: UPLOAD_TIMEOUT_MS })
  } catch (error) {
    throw new Error(
      userFacingError(
        error instanceof Error ? error.message : '',
        currentCopy().errors.model3dFailed,
      ),
    )
  }
  if (!data.file_token) throw new Error(currentCopy().errors.model3dFailed)
  return data.file_token
}

export async function createTripoTask(
  operation: TripoOperation,
  payload: Record<string, unknown>,
): Promise<string> {
  const data = await apiService.post<{ task_id: string }>(
    '/model3d/tasks',
    { operation, payload },
  )
  if (!data.task_id)
    throw new Error(currentCopy().errors.model3dFailed)
  return data.task_id
}

export function getTripoTask(
  taskId: string,
  signal?: AbortSignal,
): Promise<TripoTask> {
  return apiService.get<TripoTask>(
    `/model3d/tasks/${encodeURIComponent(taskId)}`,
    { timeout: 15 * 60_000, signal },
  )
}

function abortError(): Error {
  const error = new Error('Tripo task wait was aborted')
  error.name = 'AbortError'
  return error
}

function waitForNextPoll(ms: number, signal?: AbortSignal): Promise<void> {
  if (signal?.aborted) return Promise.reject(abortError())
  return new Promise((resolve, reject) => {
    const onAbort = () => {
      clearTimeout(timeout)
      reject(abortError())
    }
    const timeout = setTimeout(() => {
      signal?.removeEventListener('abort', onAbort)
      resolve()
    }, ms)
    signal?.addEventListener('abort', onAbort, { once: true })
  })
}

export async function pollTripoTask(
  taskId: string,
  query: (id: string, signal?: AbortSignal) => Promise<TripoTask>,
  options: AwaitTripoTaskOptions = {},
): Promise<TripoTask> {
  const intervalMs = Math.max(2_000, options.intervalMs ?? 2_000)
  // Cap 60 min; leave 1 min for download.
  const timeoutMs = Math.max(intervalMs, options.timeoutMs ?? 61 * 60_000)
  const deadline = Date.now() + timeoutMs

  while (Date.now() < deadline) {
    if (options.signal?.aborted) throw abortError()
    const task = await query(taskId, options.signal)
    options.onProgress?.(task)
    if (
      task.status === 'success' ||
      task.status === 'failed' ||
      task.status === 'cancelled' ||
      task.status === 'banned'
    ) {
      return task
    }
    await waitForNextPoll(
      Math.min(intervalMs, Math.max(1, deadline - Date.now())),
      options.signal,
    )
  }
  throw new Error(currentCopy().errors.timeout)
}

/** Poll; do not hold one HTTP request for an hour. */
export async function awaitTripoTask(
  taskId: string,
  options: AwaitTripoTaskOptions = {},
): Promise<TripoTask> {
  return pollTripoTask(taskId, getTripoTask, options)
}

export function tripoAssetUrl(assetId: string): string {
  return `/api/model3d/assets/${encodeURIComponent(assetId)}`
}
