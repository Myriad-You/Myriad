/**
 * Soft URL policy for site config fields (favicon / OG / Umami / etc.).
 *
 * Unlike wallpaper policy, this intentionally allows private hosts and
 * favicon data:image/* so self-host and local-upload flows keep working.
 * Only rejects clearly dangerous schemes / formats.
 */

function isSchemeSmuggledPath(path: string): boolean {
  if (!(path.startsWith('/') && !path.startsWith('//'))) return false
  const rest = path.slice(1)
  const colon = rest.indexOf(':')
  if (colon <= 0) return false
  const scheme = rest.slice(0, colon)
  return /^[a-z][a-z0-9+.-]*$/i.test(scheme)
}

function sanitizeHttpAllowPrivate(raw: string): string | null {
  let s = raw.trim()
  if (!s) return null
  if (s.startsWith('//')) s = `https:${s}`
  try {
    const u = new URL(s)
    if (u.protocol !== 'http:' && u.protocol !== 'https:') return null
    if (!u.hostname) return null
    return u.toString()
  } catch {
    return null
  }
}

/** Favicon: empty, path, http(s), data:image/ */
export function sanitizeSiteFaviconUrl(
  raw: string | null | undefined,
): string | null {
  if (raw == null) return null
  const s = String(raw).trim()
  if (!s) return ''
  if (s.startsWith('/') && !s.startsWith('//')) {
    if (isSchemeSmuggledPath(s)) return null
    return s
  }
  if (s.toLowerCase().startsWith('data:')) {
    // Local upload favicons; reject non-image data URLs
    if (/^data:image\//i.test(s)) return s
    return null
  }
  return sanitizeHttpAllowPrivate(s)
}

/** OG image: empty, path, http(s) — no data: */
export function sanitizeSiteOgImageUrl(
  raw: string | null | undefined,
): string | null {
  if (raw == null) return null
  const s = String(raw).trim()
  if (!s) return ''
  if (s.startsWith('/') && !s.startsWith('//')) {
    if (isSchemeSmuggledPath(s)) return null
    return s
  }
  return sanitizeHttpAllowPrivate(s)
}

/** Umami script: empty or http(s); private hosts OK */
export function sanitizeUmamiScriptUrl(
  raw: string | null | undefined,
): string | null {
  if (raw == null) return null
  const s = String(raw).trim()
  if (!s) return ''
  return sanitizeHttpAllowPrivate(s)
}
