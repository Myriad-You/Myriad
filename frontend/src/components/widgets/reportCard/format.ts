export function formatCompactNumber(n: number | undefined | null): string {
  const num = Number(n) || 0
  if (num >= 1_000_000_000) return `${(num / 1_000_000_000).toFixed(1)}B`
  if (num >= 1_000_000) return `${(num / 1_000_000).toFixed(1)}M`
  if (num >= 1_000) return `${(num / 1_000).toFixed(1)}K`
  return String(num)
}

/**
 * YouTube Data API `contentDetails.duration` (ISO-8601), e.g. PT1H2M3S → 1:02:03.
 */
export function formatYoutubeDuration(
  iso?: string | null,
): string | null {
  if (!iso || typeof iso !== 'string') return null
  const m = iso.trim().match(/^PT(?:(\d+)H)?(?:(\d+)M)?(?:(\d+)S)?$/i)
  if (!m) return null
  const h = Number(m[1] || 0)
  const min = Number(m[2] || 0)
  const s = Number(m[3] || 0)
  if (!Number.isFinite(h + min + s) || (h === 0 && min === 0 && s === 0)) {
    return null
  }
  if (h > 0) {
    return `${h}:${String(min).padStart(2, '0')}:${String(s).padStart(2, '0')}`
  }
  return `${min}:${String(s).padStart(2, '0')}`
}
