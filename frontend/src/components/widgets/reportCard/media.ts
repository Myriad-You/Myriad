import { proxyImageUrl, proxyImageUrlOr } from '../../../utils/proxyImageUrl'

export function normalizeHttpsMediaUrl(url?: string | null): string | null {
  if (!url || typeof url !== 'string') return null
  let u = url.trim()
  if (!u) return null
  if (u.startsWith('//')) u = `https:${u}`
  if (u.startsWith('http://')) u = `https://${u.slice(7)}`
  return u
}

// Xbox/MS 图升 https，并把 images-eds → images-eds-ssl。
export function normalizeXboxMediaUrl(url?: string | null): string | null {
  const base = normalizeHttpsMediaUrl(url)
  if (!base) return null
  return base.replace(
    '://images-eds.xboxlive.com',
    '://images-eds-ssl.xboxlive.com',
  )
}

// 仅用于未走 extractCardVisuals 的路径；报告卡 body 走入口 normalizeJsonMediaUrls。
export function resolveMediaUrl(url?: string | null): string | null {
  const base = normalizeHttpsMediaUrl(url)
  if (!base) return null
  return proxyImageUrl(base) ?? base
}

// B 站封面无图占位，有图走 proxy。
export function getBilibiliProxyUrl(cover?: string, title?: string): string {
  if (!cover) {
    return `https://ui-avatars.com/api/?name=${encodeURIComponent(title || 'B')}&size=400&background=00A1D6&color=fff`
  }
  return proxyImageUrlOr(
    cover,
    `https://ui-avatars.com/api/?name=${encodeURIComponent(title || 'B')}&size=400&background=00A1D6&color=fff`,
  )
}
