/** A site asset's permanent address, or the signed-in content route. */
const SITE_MEDIA_PATH =
  /^\/media\/assets\/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\/[\w.-]+$|^\/api\/media\/[1-9]\d*\/content$/i

/** An attachment URL the backend will accept: this site's own media. */
export function isValidFederationMediaUrl(url: unknown): boolean {
  if (typeof url !== 'string') return false
  const trimmed = url.trim()
  if (!trimmed || trimmed.includes('..')) return false
  let parsed: URL
  try {
    parsed = new URL(trimmed)
  } catch {
    return false
  }
  if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') return false
  return SITE_MEDIA_PATH.test(parsed.pathname)
}

export function federationMediaUrlRejectionReason(url: unknown): string | null {
  return isValidFederationMediaUrl(url) ? null : 'Invalid attachment URL'
}
