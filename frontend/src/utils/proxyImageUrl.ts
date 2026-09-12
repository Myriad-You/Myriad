import hosts from '../../../shared/image_proxy_hosts.json' with { type: 'json' }
import { API_URL } from '../config'

const HOTLINK_MARKERS: readonly string[] = hosts.markers
const AKAMAI_AND = (hosts.akamai_and_contains || 'steam').toLowerCase()

function normalizeHost(host: string): string {
  return host.replaceAll(/\.$/g, '').toLowerCase()
}

/** Exact host or DNS suffix; not substring. */
export function hostMatchesDomain(host: string, domain: string): boolean {
  const h = normalizeHost(host)
  const d = normalizeHost(domain)
  if (!h || !d) return false
  return h === d || h.endsWith(`.${d}`)
}

function parseUrlHost(url: string): string | null {
  try {
    let absolute = url
    if (absolute.startsWith('//')) absolute = `https:${absolute}`
    else if (absolute.startsWith('http://'))
      absolute = `https://${absolute.slice(7)}`
    const u = new URL(absolute)
    if (u.protocol !== 'http:' && u.protocol !== 'https:') return null
    const host = u.hostname?.trim()
    if (!host) return null
    return normalizeHost(host)
  } catch {
    return null
  }
}

export function needsImageProxy(url: string): boolean {
  const host = parseUrlHost(url)
  if (!host) return false
  if (HOTLINK_MARKERS.some((m) => hostMatchesDomain(host, m))) return true
  if (
    hostMatchesDomain(host, 'akamaihd.net') &&
    (host.includes(AKAMAI_AND) || url.toLowerCase().includes(AKAMAI_AND))
  ) {
    return true
  }
  return false
}

function isAlreadyProxied(url: string): boolean {
  if (url.startsWith('/api/proxy/image')) return true
  if (!url.includes('://')) return false
  try {
    const path = new URL(url).pathname
    return path === '/api/proxy/image' || path.startsWith('/api/proxy/image/')
  } catch {
    return url.includes('/api/proxy/image?')
  }
}

export function proxyImageUrl(
  url: string | null | undefined,
): string | undefined {
  if (url == null || typeof url !== 'string') return undefined
  let u = url.trim()
  if (!u) return undefined

  if (u.startsWith('data:') || u.startsWith('blob:')) return u

  if (u.startsWith('//')) u = `https:${u}`
  else if (u.startsWith('http://')) u = `https://${u.slice(7)}`

  if (isAlreadyProxied(u)) {
    if (u.startsWith('http://') || u.startsWith('https://')) return u
    return `${API_URL || ''}${u.startsWith('/') ? u : `/${u}`}`
  }

  if (needsImageProxy(u)) {
    return `${API_URL || ''}/api/proxy/image?url=${encodeURIComponent(u)}`
  }

  return u
}

export function proxyImageUrlOr(
  url: string | null | undefined,
  fallback = '',
): string {
  return proxyImageUrl(url) ?? fallback
}

export function normalizeJsonMediaUrls<T>(value: T): T {
  if (value == null) return value
  if (typeof value === 'string') {
    return (proxyImageUrl(value) ?? value) as T
  }
  if (Array.isArray(value)) {
    return value.map((item) => normalizeJsonMediaUrls(item)) as T
  }
  if (typeof value === 'object') {
    const out: Record<string, unknown> = {}
    for (const [k, v] of Object.entries(value as Record<string, unknown>)) {
      out[k] = normalizeJsonMediaUrls(v)
    }
    return out as T
  }
  return value
}
