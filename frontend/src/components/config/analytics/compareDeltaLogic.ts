export type CompareKind = 'day' | 'week' | 'month' | 'period'

export interface MetricDelta {
  current?: number
  previous?: number
  pct?: number | null
}

export interface CompareLabels {
  day: string
  week: string
  month: string
  period: string
  new: string
  vsPrevious: string
}

export function compareKindLabel(
  kind: string | undefined,
  labels: CompareLabels,
): string {
  switch (kind) {
    case 'day':
      return labels.day
    case 'week':
      return labels.week
    case 'month':
      return labels.month
    default:
      return labels.period
  }
}

export type CompareTone = 'up' | 'down' | 'flat' | 'new' | 'none'

export type CompareColorPalette = 'green-up' | 'red-up'

export function compareTone(delta: MetricDelta | null | undefined): CompareTone {
  if (!delta) return 'none'
  const pct = delta.pct
  if (pct == null) {
    const cur = Number(delta.current ?? 0)
    return cur > 0 ? 'new' : 'flat'
  }
  if (!Number.isFinite(pct) || pct === 0) return 'flat'
  return pct > 0 ? 'up' : 'down'
}

export function compareColorPalette(locale: string): CompareColorPalette {
  const lang = (locale || 'en').toLowerCase().split(/[-_]/)[0] ?? 'en'
  if (lang === 'zh' || lang === 'ja' || lang === 'ko') return 'red-up'
  return 'green-up'
}

export function formatComparePct(
  pct: number | null | undefined,
  locale: string,
): string {
  if (pct == null || !Number.isFinite(pct)) return '—'
  const abs = Math.abs(pct)
  const digits = abs >= 100 ? 0 : 1
  let body: string
  try {
    body = new Intl.NumberFormat(locale, {
      maximumFractionDigits: digits,
      minimumFractionDigits: 0,
    }).format(abs)
  } catch {
    body = abs.toFixed(digits)
  }
  if (pct > 0) return `+${body}%`
  if (pct < 0) return `−${body}%`
  return `${body}%`
}

export function formatCompareValue(
  delta: MetricDelta | null | undefined,
  locale: string,
  labels: Pick<CompareLabels, 'new'>,
): string {
  if (!delta) return '—'
  if (delta.pct == null) {
    const cur = Number(delta.current ?? 0)
    return cur > 0 ? labels.new : '0%'
  }
  return formatComparePct(delta.pct, locale)
}
