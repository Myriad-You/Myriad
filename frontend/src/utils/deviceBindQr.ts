export const DEVICE_BIND_URI_VERSION = 1

export interface DeviceBindParams {
  version: 1
  site: string
  token: string
}

/**
 * Normalizes a site URL to an `http:` or `https:` base without query, hash, or
 * trailing slashes. Returns `null` if the input is not a valid HTTP(S) URL.
 */
export function normalizeSiteUrl(raw: string): string | null {
  const trimmed = raw.trim()
  if (!trimmed) return null
  let parsed: URL
  try {
    parsed = new URL(trimmed)
  } catch {
    return null
  }
  if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') {
    return null
  }
  const pathname = parsed.pathname.replace(/\/+$/, '')
  return pathname ? `${parsed.origin}${pathname}` : parsed.origin
}

/**
 * Picks the durable configured public site URL when valid, otherwise falls back
 * to the browser's current origin.
 */
export function resolveBindSiteUrl(
  configuredSiteUrl: string | null | undefined,
  fallbackOrigin: string,
): string {
  if (configuredSiteUrl) {
    const normalized = normalizeSiteUrl(configuredSiteUrl)
    if (normalized) return normalized
  }
  const normalizedFallback = normalizeSiteUrl(fallbackOrigin)
  if (normalizedFallback) return normalizedFallback
  return fallbackOrigin.trim().replace(/\/+$/, '')
}

/**
 * Reports whether `site` points at a local loopback address that mobile or XR
 * devices on another host cannot reach.
 */
export function isLoopbackSiteUrl(site: string): boolean {
  let parsed: URL
  try {
    parsed = new URL(site)
  } catch {
    return false
  }
  const host = parsed.hostname.toLowerCase()
  return (
    host === 'localhost' ||
    host === '127.0.0.1' ||
    host === '::1' ||
    host === '[::1]' ||
    host === '0.0.0.0' ||
    host.endsWith('.localhost')
  )
}

/**
 * Builds the `myriad://device/bind` URI encoded into the device-binding QR
 * code. Parameter order (`v`, `site`, `token`) is deterministic.
 */
export function buildDeviceBindUri({
  site,
  token,
}: {
  site: string
  token: string
}): string {
  const normalizedSite = normalizeSiteUrl(site)
  if (!normalizedSite) {
    throw new Error('Device bind URI requires a valid http(s) site URL')
  }
  const trimmedToken = token.trim()
  if (!trimmedToken) {
    throw new Error('Device bind URI requires a non-empty token')
  }
  const params = new URLSearchParams()
  params.set('v', String(DEVICE_BIND_URI_VERSION))
  params.set('site', normalizedSite)
  params.set('token', trimmedToken)
  return `myriad://device/bind?${params.toString()}`
}

/**
 * Parses a `myriad://device/bind` URI back into normalized parameters, or
 * returns `null` if malformed or unsupported.
 */
export function parseDeviceBindUri(uri: string): DeviceBindParams | null {
  let parsed: URL
  try {
    parsed = new URL(uri.trim())
  } catch {
    return null
  }
  if (
    parsed.protocol !== 'myriad:' ||
    parsed.hostname !== 'device' ||
    parsed.pathname !== '/bind'
  ) {
    return null
  }
  if (parsed.searchParams.get('v') !== String(DEVICE_BIND_URI_VERSION)) {
    return null
  }
  const rawSite = parsed.searchParams.get('site')
  const rawToken = parsed.searchParams.get('token')?.trim()
  if (!rawSite || !rawToken) return null
  const site = normalizeSiteUrl(rawSite)
  if (!site) return null
  return {
    version: 1,
    site,
    token: rawToken,
  }
}
