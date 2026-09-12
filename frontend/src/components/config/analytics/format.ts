import { formatMessage, localeOrFallback } from '../../../i18n'
import { copyForLocale } from '../../../i18n/localeCopy'

export function formatCount(n: number, locale: string): string {
  if (!Number.isFinite(n)) return '—'
  try {
    return new Intl.NumberFormat(locale, {
      notation: Math.abs(n) >= 10000 ? 'compact' : 'standard',
      maximumFractionDigits: 1,
    }).format(n)
  } catch {
    return String(n)
  }
}

export function formatDuration(ms: number, locale: string): string {
  if (!Number.isFinite(ms) || ms <= 0) return '—'
  const sec = Math.round(ms / 1000)
  const t = copyForLocale(locale).common
  const loc = localeOrFallback(locale)
  if (sec < 60) {
    return formatMessage(loc, t.durationSeconds, { sec })
  }
  const min = Math.floor(sec / 60)
  const rem = sec % 60
  return rem
    ? formatMessage(loc, t.durationMinutesSeconds, { min, sec: rem })
    : formatMessage(loc, t.durationMinutes, { min })
}

export function shortDay(day: string): string {
  return day.length > 5 ? day.slice(5) : day
}

/** BE calendar: bucket_today, then exported_at in backup.timezone; never browser-local */
export function analyticsBackupFilenameDay(backup: {
  timezone?: unknown
  exported_at?: unknown
  bucket_today?: unknown
  page_daily?: unknown
  event_daily?: unknown
  visitor_seen?: unknown
}): string {
  const bucketToday =
    typeof backup.bucket_today === 'string'
      ? backup.bucket_today.trim().slice(0, 10)
      : ''
  if (/^\d{4}-\d{2}-\d{2}$/.test(bucketToday)) {
    return bucketToday
  }

  const exportedAt =
    typeof backup.exported_at === 'string' ? backup.exported_at : null
  const tz =
    typeof backup.timezone === 'string' ? backup.timezone.trim() : ''
  const instant = exportedAt ? new Date(exportedAt) : new Date()

  if (Number.isFinite(instant.getTime())) {
    const offsetMatch = /^UTC([+-]\d+)$/i.exec(tz)
    if (offsetMatch) {
      const hours = Number(offsetMatch[1])
      if (Number.isFinite(hours)) {
        const shifted = new Date(instant.getTime() + hours * 3_600_000)
        return shifted.toISOString().slice(0, 10)
      }
    }

    // not the browser calendar — drifts from analytics_today
    if (!tz || tz === 'local' || /^UTC$/i.test(tz)) {
      return instant.toISOString().slice(0, 10)
    }

    try {
      return new Intl.DateTimeFormat('en-CA', {
        timeZone: tz,
        year: 'numeric',
        month: '2-digit',
        day: '2-digit',
      }).format(instant)
    } catch {
    }
  }

  const payloadDay = maxAnalyticsPayloadDay(backup)
  if (payloadDay) return payloadDay

  // last resort: UTC calendar of now (not browser-local)
  return new Date().toISOString().slice(0, 10)
}

function maxAnalyticsPayloadDay(backup: {
  page_daily?: unknown
  event_daily?: unknown
  visitor_seen?: unknown
}): string | null {
  const days: string[] = []
  for (const key of ['page_daily', 'event_daily', 'visitor_seen'] as const) {
    const arr = backup[key]
    if (!Array.isArray(arr)) continue
    for (const row of arr) {
      if (
        row &&
        typeof row === 'object' &&
        typeof (row as { day?: unknown }).day === 'string'
      ) {
        const d = String((row as { day: string }).day).slice(0, 10)
        if (/^\d{4}-\d{2}-\d{2}$/.test(d)) days.push(d)
      }
    }
  }
  if (days.length === 0) return null
  return days.toSorted().at(-1)!
}

const NICE_STEPS = [1, 2, 5, 10]

function niceStep(rough: number): number {
  if (rough <= 1) return 1
  const mag = 10 ** Math.floor(Math.log10(rough))
  for (const n of NICE_STEPS) {
    const step = n * mag
    if (step >= rough) return Math.max(1, Math.round(step))
  }
  return Math.max(1, Math.round(10 * mag))
}

export function niceAxis(rawMax: number): { max: number; ticks: number[] } {
  const target = Math.max(1, Math.ceil(rawMax))
  let best = { max: Number.POSITIVE_INFINITY, step: 1, count: 2 }
  for (const count of [2, 3, 4]) {
    const step = niceStep(target / count)
    const max = step * count
    if (max >= target && max < best.max) best = { max, step, count }
  }
  const ticks: number[] = []
  for (let i = 0; i <= best.count; i += 1) ticks.push(best.step * i)
  return { max: best.max, ticks }
}
