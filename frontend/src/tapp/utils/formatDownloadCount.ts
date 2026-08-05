/** Format store install counts for UI (locale-aware compact form). */

/**
 * @param n non-negative install count
 * @param locale BCP-47 tag (e.g. zh-CN, en-US, ja-JP)
 */
export function formatDownloadCount(n: number, locale = 'en'): string {
  if (!Number.isFinite(n) || n < 0) return '0'
  const v = Math.floor(n)
  const lang = locale.toLowerCase()

  if (lang.startsWith('zh')) {
    if (v < 10_000) return String(v)
    if (v < 100_000_000) {
      const w = v / 10_000
      return `${trimOne(w)}万`
    }
    return `${trimOne(v / 100_000_000)}亿`
  }

  if (lang.startsWith('ja')) {
    if (v < 10_000) return String(v)
    if (v < 100_000_000) return `${trimOne(v / 10_000)}万`
    return `${trimOne(v / 100_000_000)}億`
  }

  if (v < 1000) return String(v)
  if (v < 1_000_000) return `${trimOne(v / 1000)}K`
  if (v < 1_000_000_000) return `${trimOne(v / 1_000_000)}M`
  return `${trimOne(v / 1_000_000_000)}B`
}

function trimOne(n: number): string {
  const s = n.toFixed(1)
  return s.endsWith('.0') ? s.slice(0, -2) : s
}
