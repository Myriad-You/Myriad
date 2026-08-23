/**
 * 访客统计图表共用：数值 / 时长格式化与坐标轴取整
 */

import { currentCopy } from '../../../i18n/localeCopy'

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

export function formatDuration(ms: number, _locale: string): string {
  if (!Number.isFinite(ms) || ms <= 0) return '—'
  const sec = Math.round(ms / 1000)
  const t = currentCopy().common
  if (sec < 60) {
    return t.durationSeconds.replace('{sec}', String(sec))
  }
  const min = Math.floor(sec / 60)
  const rem = sec % 60
  return rem
    ? t.durationMinutesSeconds
        .replace('{min}', String(min))
        .replace('{sec}', String(rem))
    : t.durationMinutes.replace('{min}', String(min))
}

/** `2026-07-30` → `07-30`（轴与行内标签用短日期） */
export function shortDay(day: string): string {
  return day.length > 5 ? day.slice(5) : day
}

/**
 * Calendar day label for analytics backup download filenames.
 *
 * BE day buckets use the process local calendar (`analytics_today` / TZ).
 * Order: (1) `bucket_today` from BE (authoritative), (2) `exported_at` in
 * `backup.timezone`, (3) max `day` key in payload tables only as last resort
 * when timestamp invalid. Never prefer max day over today — empty today
 * buckets would pin filenames to yesterday.
 */
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
    // `UTC+8` / `UTC-5` from analytics_tz_label when TZ env unset
    const offsetMatch = /^UTC([+-]\d+)$/i.exec(tz)
    if (offsetMatch) {
      const hours = Number(offsetMatch[1])
      if (Number.isFinite(hours)) {
        const shifted = new Date(instant.getTime() + hours * 3_600_000)
        return shifted.toISOString().slice(0, 10)
      }
    }

    // Bare UTC, or BE "local" when process offset hours==0 (UTC container, no TZ).
    // Must NOT use the browser's local calendar — that drifts from analytics_today.
    if (!tz || tz === 'local' || /^UTC$/i.test(tz)) {
      return instant.toISOString().slice(0, 10)
    }

    // IANA zone (e.g. Asia/Shanghai) when TZ env is set
    try {
      // en-CA → YYYY-MM-DD
      return new Intl.DateTimeFormat('en-CA', {
        timeZone: tz,
        year: 'numeric',
        month: '2-digit',
        day: '2-digit',
      }).format(instant)
    } catch {
      /* invalid IANA — fall through to max payload day / UTC */
    }
  }

  // Timestamp missing/invalid: fall back to latest day key in tables
  const payloadDay = maxAnalyticsPayloadDay(backup)
  if (payloadDay) return payloadDay

  // Absolute last resort: UTC calendar of "now" (still not browser-local)
  return new Date().toISOString().slice(0, 10)
}

/** Latest YYYY-MM-DD among analytics table rows in a backup payload. */
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
  days.sort()
  return days[days.length - 1]!
}

const NICE_STEPS = [1, 2, 5, 10]

/** 把一格的粗略高度吸附到 1/2/5/10×10ⁿ 的整数刻度 */
function niceStep(rough: number): number {
  if (rough <= 1) return 1
  const mag = 10 ** Math.floor(Math.log10(rough))
  for (const n of NICE_STEPS) {
    const step = n * mag
    if (step >= rough) return Math.max(1, Math.round(step))
  }
  return Math.max(1, Math.round(10 * mag))
}

/**
 * 计数轴刻度：2~4 格里挑「上限最贴近数据」的一组，
 * 刻度值保持整数（浏览量没有半次），留白不会浪费半张图。
 */
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
