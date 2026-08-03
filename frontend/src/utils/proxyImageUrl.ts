/**
 * 站内图片代理（前端防御层）— dual-path。
 *
 * 热链域名名单唯一来源：仓库根 `shared/image_proxy_hosts.json`
 * （后端 `needs_image_proxy` + `/api/proxy/image` allowlist 同文件）。
 *
 * - 名单内 host → `/api/proxy/image?url=…`
 * - 其它 https → 原 URL 直链（RSS / 个人博客 / 健康 CDN 不经代理）
 *
 * Host 匹配：parsed host 精确或 DNS suffix（禁止 url.includes 子串）。
 */

import hosts from '../../../shared/image_proxy_hosts.json'
import { API_URL } from '../config'

const HOTLINK_MARKERS: readonly string[] = hosts.markers
const AKAMAI_AND = (hosts.akamai_and_contains || 'steam').toLowerCase()

function normalizeHost(host: string): string {
  return host.replace(/\.$/, '').toLowerCase()
}

/** Exact host or proper DNS suffix (i0.hdslb.com ↔ hdslb.com). */
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

/**
 * 规范为浏览器可显示地址；不需要代理时返回 https 规范化后的原 URL。
 */
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

/**
 * 递归规范化对象树中的字符串 URL（与后端 normalize_json_media_urls 对称）。
 * 用于报告 card_visuals / library 入口，组件内不必再散点 resolveMediaUrl。
 */
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
