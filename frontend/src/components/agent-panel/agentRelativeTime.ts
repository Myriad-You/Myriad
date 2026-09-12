export type RelativeTimeBucket =
  | { kind: 'justNow' }
  | { kind: 'minutes'; value: number }
  | { kind: 'hours'; value: number }
  | { kind: 'days'; value: number }
  | { kind: 'date'; date: Date }

const MINUTE = 60_000
const HOUR = 60 * MINUTE
const DAY = 24 * HOUR

export const RELATIVE_TIME_MAX_DAYS = 7

export function relativeTimeBucket(
  iso: string | null | undefined,
  nowMs: number,
): RelativeTimeBucket | null {
  if (!iso) return null
  const then = new Date(iso)
  const at = then.getTime()
  if (!Number.isFinite(at)) return null

  // clock skew: future → justNow
  const elapsed = Math.max(0, nowMs - at)

  if (elapsed < MINUTE) return { kind: 'justNow' }
  if (elapsed < HOUR) {
    return { kind: 'minutes', value: Math.floor(elapsed / MINUTE) }
  }
  if (elapsed < DAY) return { kind: 'hours', value: Math.floor(elapsed / HOUR) }
  const days = Math.floor(elapsed / DAY)
  if (days <= RELATIVE_TIME_MAX_DAYS) return { kind: 'days', value: days }
  return { kind: 'date', date: then }
}
