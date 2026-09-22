import { API_URL } from '../config'
import { currentCopy } from '../i18n/localeCopy'
import { getCSRFToken } from '../utils/csrf'
import { userFacingError } from '../utils/userFacingError'
import { apiService } from './api'

export type MediaKind = 'upload' | 'generated'

export interface MediaAsset {
  id: number
  kind: MediaKind
  url: string
  mime: string
  name: string
  size: number
  created_at: number
  references: string[]
  public_id?: string
  content_path?: string | null
  public_path?: string | null
  state?: string
  exposure?: string
  source?: string
  derived_from_id?: number | null
}

// Validate the response without changing the catalog path or asset identity.
function validateMediaAsset(item: MediaAsset): void {
  if (!item || !Number.isInteger(item.id) || item.id <= 0 ||
      typeof item.url !== 'string' || !item.url.trim()) {
    throw new Error('Invalid media response')
  }
}

export interface MediaFilter {
  kind: 'all' | MediaKind
  format: string
  query: string
}

export interface MediaCursor {
  created_at: string
  id: number
}

export interface MediaPage {
  items: MediaAsset[]
  next_cursor: MediaCursor | null
  total?: number
}

export async function listMedia(
  options: { filter?: MediaFilter; cursor?: MediaCursor; limit?: number } = {},
  signal?: AbortSignal,
): Promise<MediaPage> {
  const params = new URLSearchParams()
  if (options.filter) {
    const { kind, format, query } = options.filter
    if (kind !== 'all') params.set('kind', kind)
    if (format !== 'all') params.set('format', format)
    if (query.trim()) params.set('query', query.trim())
  }
  if (options.cursor) {
    params.set('before_created_at', options.cursor.created_at)
    params.set('before_id', String(options.cursor.id))
  }
  if (options.limit) params.set('limit', String(options.limit))
  const suffix = params.size ? `?${params}` : ''
  const data = await apiService.get<MediaPage>(`/media${suffix}`, { signal })
  if (!Array.isArray(data.items)) throw new Error('Invalid media response')
  data.items.forEach(validateMediaAsset)
  return data
}

export async function deleteMedia(id: number): Promise<void> {
  await apiService.delete(`/media/${id}`)
}

/** Drafts keep the authenticated content path until an explicit publication. */
export function draftMediaSrc(item: MediaAsset): string {
  if (item.exposure === 'public' && item.public_path) return item.public_path
  return item.content_path || item.url
}

export function isPrivateMediaPath(src: string): boolean {
  try {
    const path = new URL(src.trim(), 'https://media.invalid').pathname
    return /^\/api\/media\/\d+\/content$/.test(path)
  } catch {
    return false
  }
}

export async function fetchMediaObjectUrl(
  path: string,
  signal?: AbortSignal,
): Promise<string> {
  path = path.trim()
  const url = path.startsWith('http') || path.startsWith('//') || path.startsWith('blob:') || path.startsWith('data:')
    ? path
    : `${API_URL.replace(/\/$/, '')}${path}`
  if (url.startsWith('blob:') || url.startsWith('data:')) return url
  const response = await fetch(url, { credentials: 'include', signal })
  if (!response.ok) {
    throw new Error(
      userFacingError(`Media read failed: ${response.status}`, currentCopy().errors.mediaUploadFailed),
    )
  }
  const blob = await response.blob()
  signal?.throwIfAborted()
  return URL.createObjectURL(blob)
}

export async function publishMedia(id: number): Promise<MediaAsset> {
  const data = await apiService.post<{ item: MediaAsset }>(`/media/${id}/publication`)
  validateMediaAsset(data.item)
  return data.item
}

export async function unpublishMedia(id: number): Promise<MediaAsset> {
  const data = await apiService.delete<{ item: MediaAsset }>(`/media/${id}/publication`)
  validateMediaAsset(data.item)
  return data.item
}

export async function uploadMedia(
  file: File,
  signal?: AbortSignal,
): Promise<MediaAsset> {
  const form = new FormData()
  form.append('file', file, file.name)
  const headers: Record<string, string> = {}
  const csrf = await getCSRFToken()
  if (csrf) headers['X-CSRF-Token'] = csrf
  const response = await fetch(`${API_URL}/api/media`, {
    method: 'POST',
    headers,
    body: form,
    credentials: 'include',
    signal,
  })
  if (!response.ok) {
    const errBody = await response.json().catch(() => ({}))
    throw new Error(
      userFacingError(
        (errBody as { error?: string; message?: string }).error ||
          (errBody as { message?: string }).message ||
          `Media upload failed: ${response.status}`,
        currentCopy().errors.mediaUploadFailed,
      ),
    )
  }
  const data = (await response.json()) as { success: boolean; item: MediaAsset }
  validateMediaAsset(data.item)
  return data.item
}

export async function previewMediaEdit(id: number, prompt: string, width: number, height: number, signal?: AbortSignal): Promise<string> {
  const data = await apiService.post<{ image: string }>(`/media/${id}/edit-preview`, { prompt, width, height }, { signal, timeout: 240_000 })
  return data.image
}

export async function saveMediaEdit(id: number, image: string, generated: boolean): Promise<MediaAsset> {
  const data = await apiService.post<{ item: MediaAsset }>(`/media/${id}/edits`, { image, generated })
  validateMediaAsset(data.item)
  return data.item
}
