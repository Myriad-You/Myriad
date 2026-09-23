import { API_URL } from '../config'

/** Resolve backend-owned paths for display only; never rewrite stored asset identity. */
export function siteMediaUrl(src: string, apiUrl = API_URL): string {
  let path = src.trim()
  // Persisted local media can carry the previous public site origin. As in the
  // journal reader, read platform-owned media through the current API origin.
  try {
    const parsed = new URL(path, 'https://media.invalid')
    if (/^\/(?:(?:api\/)?media\/(?:assets|federation)\/|api\/media\/\d+\/content$)/.test(parsed.pathname)) {
      path = `${parsed.pathname}${parsed.search}${parsed.hash}`
    }
  } catch { /* Keep non-URL sources unchanged. */ }
  // The proxy updates only on explicit request, and older ones do not route
  // `/media/*` to the backend; every proxy forwards `/api/`, where the backend
  // mirrors these paths.
  if (path.startsWith('/media/assets/') || path.startsWith('/media/federation/')) {
    path = `/api${path}`
  }
  if (path.startsWith('/api/')) {
    return `${apiUrl.replace(/\/$/, '')}${path}`
  }
  return path
}
