import { API_URL } from '../../../config'
import { displayImageUrl } from '../notes/noteImageUrl'

/** Resolvable workbench media URL for the current API origin. */
export function mediaPointerUrl(
  raw: unknown,
  apiUrl: string = API_URL,
): string | null {
  if (typeof raw !== 'string') return null
  const trimmed = raw.trim()
  if (!trimmed) return null
  const url = displayImageUrl(trimmed, apiUrl)
  return url || null
}

export function resolveMediaPointer(
  item: { id?: unknown; url?: unknown } | null | undefined,
  apiUrl: string = API_URL,
): { id: number; url: string } | null {
  if (!item || typeof item !== 'object') return null
  const id = Number(item.id)
  if (!Number.isFinite(id) || id <= 0) return null
  const url = mediaPointerUrl(item.url, apiUrl)
  if (!url) return null
  return { id, url }
}
