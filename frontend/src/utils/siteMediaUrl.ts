import { API_URL } from '../config'

/**
 * Paths the platform itself serves as media. Only these follow the current
 * API origin when stored with an older site origin; any other absolute URL is
 * another site's, even when its path happens to start with `/api/`.
 */
const SITE_MEDIA_PATH =
  /^\/(?:media\/assets\/|api\/media\/\d+\/content$|api\/phantasi\/image-cache\/|api\/proxy\/image(?:\/|$))/

/** Resolve backend-owned paths for display only; never rewrite stored asset identity. */
export function siteMediaUrl(src: string, apiUrl = API_URL): string {
  let path = src.trim()
  // Persisted local media can carry the previous public site origin. As in the
  // journal reader, read platform-owned media through the current API origin.
  try {
    const parsed = new URL(path, 'https://media.invalid')
    if (SITE_MEDIA_PATH.test(parsed.pathname)) {
      path = `${parsed.pathname}${parsed.search}${parsed.hash}`
    }
  } catch { /* Keep non-URL sources unchanged. */ }
  if (path.startsWith('/api/') || path.startsWith('/media/assets/')) {
    return `${apiUrl.replace(/\/$/, '')}${path}`
  }
  return path
}
