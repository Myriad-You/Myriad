import { proxyImageUrl } from './proxyImageUrl'

const FALLBACK_COLORS = [
  '#6366f1',
  '#0ea5e9',
  '#14b8a6',
  '#f59e0b',
  '#ef4444',
  '#8b5cf6',
  '#ec4899',
  '#10b981',
] as const

/** Stable hue; no Math.random. */
function hashSeed(seed: string): number {
  let hash = 0
  for (let i = 0; i < seed.length; i += 1) {
    hash = (hash << 5) - hash + seed.charCodeAt(i)
    hash |= 0
  }
  return Math.abs(hash)
}

function initial(seed: string): string {
  const trimmed = seed.trim()
  if (!trimmed) return '?'
  const first = Iterator.from(trimmed).toArray()[0]
  return first.toUpperCase()
}

export function localFallbackAvatar(seed: string | null | undefined): string {
  const name = (seed ?? '').trim() || 'User'
  const color = FALLBACK_COLORS[hashSeed(name) % FALLBACK_COLORS.length]
  const letter = initial(name)
  // Escape & and < in SVG.
  const safeLetter = letter
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><rect width="64" height="64" fill="${color}"/><text x="50%" y="50%" dy=".35em" text-anchor="middle" font-family="system-ui,-apple-system,'PingFang SC','Microsoft YaHei',sans-serif" font-size="30" fill="#fff">${safeLetter}</text></svg>`
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`
}

export function resolveAvatar(
  src: string | null | undefined,
  seed?: string | null,
): string {
  return proxyImageUrl(src) ?? localFallbackAvatar(seed)
}
