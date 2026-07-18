/**
 * Pure helpers for updater panel check freshness / stale policy.
 *
 * Policy (open About → Updater):
 * - Missing / invalid last_checked_at → stale (never checked).
 * - check_interval_secs > 0 → stale when age >= interval (same cadence as worker).
 * - check_interval_secs === 0 (auto-check off) → still stale after STALE_WHEN_OFF_SECS
 *   so visiting the panel never trusts multi-day cache.
 */

/** When worker auto-check is off, recheck if last check is at least this old (1h). */
export const STALE_WHEN_OFF_SECS = 3600

/** How often the UI re-renders relative “ago” labels. */
export const AGO_TICK_MS = 30_000

/**
 * Age of last check in seconds, or `null` if never checked / unparsable
 * (treat as infinitely stale).
 */
export function checkAgeSecs(
  lastCheckedAt: string | null | undefined,
  nowMs: number = Date.now(),
): number | null {
  if (lastCheckedAt == null || lastCheckedAt === '') return null
  const then = new Date(lastCheckedAt).getTime()
  if (Number.isNaN(then)) return null
  return Math.max(0, (nowMs - then) / 1000)
}

/**
 * Whether cached updater status should be revalidated against GitHub.
 *
 * @param lastCheckedAt ISO timestamp from status, or null/undefined if never checked
 * @param checkIntervalSecs effective interval from status (0 = worker auto-check off)
 * @param nowMs injectable clock for tests
 */
export function isCheckStale(
  lastCheckedAt: string | null | undefined,
  checkIntervalSecs: number | null | undefined,
  nowMs: number = Date.now(),
): boolean {
  const age = checkAgeSecs(lastCheckedAt, nowMs)
  if (age === null) return true
  const interval =
    typeof checkIntervalSecs === 'number' && Number.isFinite(checkIntervalSecs)
      ? Math.max(0, checkIntervalSecs)
      : 0
  if (interval > 0) return age >= interval
  return age >= STALE_WHEN_OFF_SECS
}

export type AgoUnit = 'justNow' | 'min' | 'hour' | 'day'

export type AgoParts =
  | { unit: 'justNow' }
  | { unit: 'min'; n: number }
  | { unit: 'hour'; n: number }
  | { unit: 'day'; n: number }

/**
 * Relative-time breakdown for last_checked_at. Returns null if unparsable.
 */
export function computeAgo(
  iso: string,
  nowMs: number = Date.now(),
): AgoParts | null {
  const then = new Date(iso).getTime()
  if (Number.isNaN(then)) return null
  const diffSec = Math.max(0, Math.round((nowMs - then) / 1000))
  if (diffSec < 45) return { unit: 'justNow' }
  const min = Math.round(diffSec / 60)
  if (min < 60) return { unit: 'min', n: min }
  const hr = Math.round(min / 60)
  if (hr < 24) return { unit: 'hour', n: hr }
  const d = Math.round(hr / 24)
  return { unit: 'day', n: d }
}
