/** http(s) or same-origin path; no data:/javascript:/blob:. */

export function isBlockedWallpaperHost(hostname: string): boolean {
  const host = hostname.trim().toLowerCase()
  if (!host) return true

  const h =
    host.startsWith('[') && host.endsWith(']') ? host.slice(1, -1) : host

  if (
    h === 'localhost' ||
    h === '0.0.0.0' ||
    h === '::' ||
    h === '::1' ||
    h.endsWith('.localhost') ||
    h.endsWith('.local') ||
    h.endsWith('.internal') ||
    h.endsWith('.arpa') ||
    h.endsWith('.lan') ||
    h.endsWith('.home') ||
    h.endsWith('.corp')
  ) {
    return true
  }

  if (/^\d{1,3}(\.\d{1,3}){3}$/.test(h)) {
    return isBlockedIpv4(h)
  }

  if (h.includes(':')) {
    return isBlockedIpv6(h)
  }

  return false
}

function isBlockedIpv4(ip: string): boolean {
  const parts = ip.split('.').map((p) => Number(p))
  if (parts.length !== 4 || parts.some((n) => !Number.isInteger(n) || n < 0 || n > 255)) {
    return true
  }
  const [a, b] = parts

  if (a === 0 || a === 127) return true
  if (a === 10) return true
  if (a === 172 && b >= 16 && b <= 31) return true
  if (a === 192 && b === 168) return true
  if (a === 169 && b === 254) return true
  if (a === 100 && b >= 64 && b <= 127) return true
  if (a === 192 && b === 0 && parts[2] === 2) return true
  if (a === 198 && b === 51 && parts[2] === 100) return true
  if (a === 203 && b === 113) return true
  if (a >= 224) return true

  return false
}

function isBlockedIpv6(ip: string): boolean {
  const lower = ip.toLowerCase()

  const mapped = lower.match(/^::ffff:(\d{1,3}(?:\.\d{1,3}){3})$/)
  if (mapped) return isBlockedIpv4(mapped[1])

  if (lower === '::1' || lower === '::') return true

  const first = lower.split(':').find((s) => s.length > 0)
  if (first) {
    const n = Number.parseInt(first, 16)
    if (Number.isFinite(n)) {
      if ((n & 0xFE00) === 0xFC00) return true
      if ((n & 0xFFC0) === 0xFE80) return true
    }
  }

  if (
    lower === '0:0:0:0:0:0:0:1' ||
    lower.endsWith('::1') ||
    /^0*:0*:0*:0*:0*:0*:0*:1$/.test(lower)
  ) {
    return true
  }

  return false
}

export function sanitizeWallpaperUrl(
  raw: string | null | undefined,
): string | null {
  if (raw == null) return null
  let s = String(raw).trim()
  if (!s) return null

  // Same-origin path only (no //, no scheme).
  if (s.startsWith('/') && !s.startsWith('//')) {
    // Reject /javascript: in paths.
    if (/^\/[a-z][a-z0-9+.-]*:/i.test(s)) return null
    return s
  }

  if (s.startsWith('//')) {
    s = `https:${s}`
  }

  let parsed: URL
  try {
    parsed = new URL(s)
  } catch {
    return null
  }

  const protocol = parsed.protocol.toLowerCase()
  if (protocol !== 'http:' && protocol !== 'https:') {
    return null
  }

  // No credentials in wallpaper URLs.
  if (parsed.username || parsed.password) {
    return null
  }

  const host = parsed.hostname
  if (!host || isBlockedWallpaperHost(host)) {
    return null
  }

  // Reject script-like wallpaper paths.

  return parsed.toString()
}
