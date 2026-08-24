import { API_URL } from '../config'
import api from '../lib/api'
import { getCSRFHeaderName, getCSRFToken } from '../utils/csrf'

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

function assertTripoHttpSuccess(
  status: number,
  data: unknown,
  fallback: string,
): void {
  if (status < 400) return
  const body =
    data && typeof data === 'object'
      ? (data as { error?: unknown; message?: unknown })
      : {}
  const message =
    typeof body.error === 'string'
      ? body.error
      : typeof body.message === 'string'
        ? body.message
        : fallback
  throw new Error(message)
}

export async function getTripoStatus(): Promise<TripoStatus> {
  const response = await api.get<TripoStatus>('/api/merope/3d/status')
  assertTripoHttpSuccess(
    response.status,
    response.data,
    'Could not load Tripo status',
  )
  return response.data
}

export async function uploadTripoFile(file: File): Promise<string> {
  const form = new FormData()
  form.append('file', file, file.name)
  const headers: Record<string, string> = {}
  const csrf = await getCSRFToken()
  if (csrf) headers[getCSRFHeaderName()] = csrf

  // Do not use the shared Axios instance here: its JSON default would prevent
  // the browser from generating the multipart boundary.
  const response = await fetch(`${API_URL}/api/merope/3d/files`, {
    method: 'POST',
    headers,
    body: form,
    credentials: 'include',
  })
  const data = (await response.json().catch(() => ({}))) as {
    file_token?: string
    error?: string
    message?: string
  }
  if (!response.ok || !data.file_token) {
    throw new Error(
      data.error || data.message || `Tripo upload failed: ${response.status}`,
    )
  }
  return data.file_token
}

export async function createTripoTask(
  operation: TripoOperation,
  payload: Record<string, unknown>,
): Promise<string> {
  const response = await api.post<{ task_id: string }>(
    '/api/merope/3d/tasks',
    { operation, payload },
  )
  assertTripoHttpSuccess(
    response.status,
    response.data,
    'Could not create Tripo task',
  )
  if (!response.data.task_id)
    throw new Error('Tripo task response did not include task_id')
  return response.data.task_id
}

/**
 * Query once. When Tripo reports success, the backend downloads and validates
 * the expiring provider URL and returns a stable content-addressed asset.
 */
export async function getTripoTask(
  taskId: string,
  signal?: AbortSignal,
): Promise<TripoTask> {
  const response = await api.get<TripoTask>(
    `/api/merope/3d/tasks/${encodeURIComponent(taskId)}`,
    // A successful query also downloads and validates provider model outputs.
    { timeout: 15 * 60_000, signal },
  )
  assertTripoHttpSuccess(
    response.status,
    response.data,
    'Could not query Tripo task',
  )
  return response.data
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
  // Backend configuration is capped at 60 minutes; leave one minute for the
  // final model download/validation response.
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
  throw new Error(`Tripo task ${taskId} timed out after ${timeoutMs}ms`)
}

/** Browser-safe polling; avoids one HTTP request being held for up to an hour. */
export async function awaitTripoTask(
  taskId: string,
  options: AwaitTripoTaskOptions = {},
): Promise<TripoTask> {
  return pollTripoTask(taskId, getTripoTask, options)
}

export function tripoAssetUrl(assetId: string): string {
  return `/api/merope/3d/assets/${encodeURIComponent(assetId)}`
}
