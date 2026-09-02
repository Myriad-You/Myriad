/**
 * Wallpaper URL policy (apply-time + resolve-time).
 *
 * - Schemes: http(s) only (absolute), or same-origin path (`/...`)
 * - Reject data:/javascript:/blob: and SVG data URLs
 * - Block loopback, private, link-local, metadata, and special hostnames so
 *   visitor browsers are not used as intranet probes via resolveImageUrl fetch
 */

/**
 * Whether a hostname must never be used for wallpaper fetch/apply.
 * Hostname only (no port); case-insensitive.
 */
export function isBlockedWallpaperHost(hostname: string): boolean {
  const host = hostname.trim().toLowerCase()
  if (!host) return true

  // Strip IPv6 brackets if present
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

  // IPv4 literal
  if (/^\d{1,3}(\.\d{1,3}){3}$/.test(h)) {
    return isBlockedIpv4(h)
  }

  // IPv6 literal (including IPv4-mapped)
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

  // 0.0.0.0/8, 127.0.0.0/8
  if (a === 0 || a === 127) return true
  // 10.0.0.0/8
  if (a === 10) return true
  // 172.16.0.0/12
  if (a === 172 && b >= 16 && b <= 31) return true
  // 192.168.0.0/16
  if (a === 192 && b === 168) return true
  // 169.254.0.0/16 link-local (incl. cloud metadata 169.254.169.254)
  if (a === 169 && b === 254) return true
  // 100.64.0.0/10 CGNAT
  if (a === 100 && b >= 64 && b <= 127) return true
  // 192.0.2.0/24, 198.51.100.0/24, 203.0.113.0/24 documentation
  if (a === 192 && b === 0 && parts[2] === 2) return true
  if (a === 198 && b === 51 && parts[2] === 100) return true
  if (a === 203 && b === 113) return true
  // 224.0.0.0/4 multicast + 240.0.0.0/4 reserved
  if (a >= 224) return true

  return false
}

function isBlockedIpv6(ip: string): boolean {
  const lower = ip.toLowerCase()

  // IPv4-mapped ::ffff:x.x.x.x
  const mapped = lower.match(/^::ffff:(\d{1,3}(?:\.\d{1,3}){3})$/)
  if (mapped) return isBlockedIpv4(mapped[1])

  // Compact common forms
  if (lower === '::1' || lower === '::') return true

  // Unique local fc00::/7, link-local fe80::/10
  // Parse first hextet when present
  const first = lower.split(':').find((s) => s.length > 0)
  if (first) {
    const n = Number.parseInt(first, 16)
    if (Number.isFinite(n)) {
      if ((n & 0xFE00) === 0xFC00) return true // fc00::/7
      if ((n & 0xFFC0) === 0xFE80) return true // fe80::/10
    }
  }

  // Loopback variants
  if (
    lower === '0:0:0:0:0:0:0:1' ||
    lower.endsWith('::1') ||
    /^0*:0*:0*:0*:0*:0*:0*:1$/.test(lower)
  ) {
    return true
  }

  return false
}

/**
 * Normalize + validate a wallpaper URL for fetch/apply.
 * Returns null when the URL must not be used.
 *
 * Allowed:
 * - `https://...` / `http://...` with non-blocked host
 * - protocol-relative `//host/path` → https
 * - same-origin path `/path` (no scheme tricks)
 *
 * Rejected:
 * - data:, blob:, javascript:, vbscript:, file:, etc.
 * - data:image/svg+xml (and any data:)
 * - private / loopback / link-local hosts
 */
export function sanitizeWallpaperUrl(
  raw: string | null | undefined,
): string | null {
  if (raw == null) return null
  let s = String(raw).trim()
  if (!s) return null

  // Same-origin relative path only (no //, no scheme)
  if (s.startsWith('/') && !s.startsWith('//')) {
    // Reject path that looks like a scheme smuggle: /javascript:alert(1)
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

  // No credentials in wallpaper URLs
  if (parsed.username || parsed.password) {
    return null
  }

  const host = parsed.hostname
  if (!host || isBlockedWallpaperHost(host)) {
    return null
  }

  // Reject obvious non-image script payloads in path (defense in depth)
  // (scheme already blocks javascript:)

  return parsed.toString()
}
