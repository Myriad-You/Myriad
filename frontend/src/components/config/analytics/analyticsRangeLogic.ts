/**
 * Pure helpers for analytics range (no React / CSS) — unit-test friendly.
 */

export const ANALYTICS_RANGE_PRESETS = ['7', '14', '30'] as const
export type AnalyticsRangePreset =
  | (typeof ANALYTICS_RANGE_PRESETS)[number]
  | 'custom'

export interface AnalyticsRangeState {
  preset: AnalyticsRangePreset
  /** YYYY-MM-DD when preset === 'custom' */
  from: string
  to: string
}

function localIsoToday(): string {
  const d = new Date()
  const y = d.getFullYear()
  const m = String(d.getMonth() + 1).padStart(2, '0')
  const day = String(d.getDate()).padStart(2, '0')
  return `${y}-${m}-${day}`
}

function addDaysIso(iso: string, delta: number): string {
  const [y, m, d] = iso.split('-').map(Number)
  const dt = new Date(y!, m! - 1, d!)
  dt.setDate(dt.getDate() + delta)
  const yy = dt.getFullYear()
  const mm = String(dt.getMonth() + 1).padStart(2, '0')
  const dd = String(dt.getDate()).padStart(2, '0')
  return `${yy}-${mm}-${dd}`
}

/** Default custom window: last 7 local calendar days including today. */
export function defaultCustomRange(maxDate = localIsoToday()): Pick<
  AnalyticsRangeState,
  'from' | 'to'
> {
  return { from: addDaysIso(maxDate, -6), to: maxDate }
}

export function defaultAnalyticsRange(): AnalyticsRangeState {
  const { from, to } = defaultCustomRange()
  return { preset: '7', from, to }
}

/** Query string fragment for analytics APIs (no leading ?). */
export function analyticsRangeQuery(state: AnalyticsRangeState): string {
  if (state.preset === 'custom' && state.from && state.to) {
    return `from=${encodeURIComponent(state.from)}&to=${encodeURIComponent(state.to)}`
  }
  const days = state.preset === 'custom' ? '7' : state.preset
  return `days=${days}`
}

/** Inclusive day span for display (custom uses from/to). */
export function analyticsRangeDayCount(state: AnalyticsRangeState): number {
  if (state.preset !== 'custom') return Number(state.preset)
  if (!state.from || !state.to) return 0
  const a = new Date(`${state.from}T00:00:00`)
  const b = new Date(`${state.to}T00:00:00`)
  if (!Number.isFinite(a.getTime()) || !Number.isFinite(b.getTime())) return 0
  const diff = Math.round((b.getTime() - a.getTime()) / 86_400_000)
  return Math.max(1, Math.abs(diff) + 1)
}

export { addDaysIso, localIsoToday }
