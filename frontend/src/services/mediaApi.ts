import { resolveMediaPointer } from '../components/phantasi/logic/mediaPointer'
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
}

export async function listMedia(signal?: AbortSignal): Promise<MediaAsset[]> {
  const data = await apiService.get<{ success: boolean; items: MediaAsset[] }>(
    '/media',
    { signal },
  )
  return data.items.flatMap((item) => {
    const pointer = resolveMediaPointer(item)
    if (!pointer) return []
    return [{ ...item, id: pointer.id, url: pointer.url }]
  })
}

export async function deleteMedia(id: number): Promise<void> {
  await apiService.delete(`/media/${id}`)
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
  const pointer = resolveMediaPointer(data.item)
  if (!pointer) {
    throw new Error(
      userFacingError(
        'Media upload returned no file pointer',
        currentCopy().errors.mediaUploadFailed,
      ),
    )
  }
  return { ...data.item, id: pointer.id, url: pointer.url }
}

export async function previewMediaEdit(id: number, prompt: string, width: number, height: number, signal?: AbortSignal): Promise<string> {
  const data = await apiService.post<{ image: string }>(`/media/${id}/edit-preview`, { prompt, width, height }, { signal, timeout: 240_000 })
  return data.image
}

export async function saveMediaEdit(id: number, image: string, generated: boolean): Promise<MediaAsset> {
  const data = await apiService.post<{ item: MediaAsset }>(`/media/${id}/edits`, { image, generated })
  return data.item
}
