/**
 * Pure helpers for updater panel check freshness and pre-update revalidation.
 *
 * Open About → Updater policy:
 * - On each panel mount (status OK, admin, not blocked): always silent
 *   checkAvailable once. Remounting About = a new check is desired.
 * - isCheckStale is for UI only (relative “ago”, unconfirmed badge, stale
 *   hints) — not a gate for the auto recheck path.
 * - Before applying “update to latest”: re-fetch available+status; abort if
 *   no longer necessary (no target / identical / same version). On recheck
 *   error, do not apply from stale cache.
 *
 * Stale age policy (display / messaging):
 * - Missing / invalid last_checked_at → stale (never checked).
 * - check_interval_secs > 0 → stale when age >= interval (same cadence as worker).
 * - check_interval_secs === 0 (auto-check off) → still stale after STALE_WHEN_OFF_SECS
 *   so the UI never presents multi-day cache as “fresh”.
 */

import type { UpdateMode } from '../../services/updaterApi'

/** When worker auto-check is off, treat last check as stale after this age (1h). */
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
 * Whether cached updater status is old enough that the UI should show
 * stale / unconfirmed messaging (not a gate for mount auto-recheck).
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

// Pre-update revalidation (update to latest)

/** Why applying “latest” is no longer necessary after a fresh check. */
export type LatestUpdateAbortReason =
  | 'no_target'
  | 'identical'
  | 'same_version'

export type LatestUpdatePlan =
  | { proceed: false; reason: LatestUpdateAbortReason }
  | {
      proceed: true
      target: string
      mode: UpdateMode
      isDowngrade: boolean
      needsRisk: boolean
    }

/** Minimal tip / latest_available shape for planning. */
export interface LatestTipFields {
  version?: string | null
  mode?: UpdateMode | null
  relation?: string | null
  is_upgrade?: boolean | null
  is_downgrade?: boolean | null
}

/**
 * Formal release tags look like v0.4.0 (`v`-prefixed semver, matching DeployTag).
 * Kept local so planLatestUpdate stays free of UI imports.
 */
function isReleaseTag(tag: string): boolean {
  return /^v\d+\.\d+\.\d+([.-][0-9A-Za-z.]+)?$/.test(tag.trim())
}

function modeForTarget(target: string, fallback: UpdateMode): UpdateMode {
  if (isReleaseTag(target)) return 'release'
  if (fallback === 'commit') return 'commit'
  return 'commit'
}

function normalizeVersion(v: string): string {
  return v.trim().toLowerCase()
}

/**
 * Decide whether “update to latest” should still run after a fresh
 * available + status recheck. Pure — no I/O.
 *
 * Aborts when there is no target, relation is identical, or the tip version
 * equals the running version. Otherwise returns mode / risk flags from the
 * fresh tip (prefer `available` over status.latest_available).
 */
export function planLatestUpdate(input: {
  available: LatestTipFields | null | undefined
  latestAvailable: LatestTipFields | null | undefined
  currentVersion: string | null | undefined
  downgradeAvailable?: boolean
  channelMode: UpdateMode
}): LatestUpdatePlan {
  const tip = input.available ?? null
  const la = input.latestAvailable ?? null
  const target = (tip?.version || la?.version || '').trim()
  if (!target) {
    return { proceed: false, reason: 'no_target' }
  }

  const relation = tip?.relation ?? la?.relation
  if (relation === 'identical') {
    return { proceed: false, reason: 'identical' }
  }

  const current = (input.currentVersion ?? '').trim()
  if (current && normalizeVersion(target) === normalizeVersion(current)) {
    return { proceed: false, reason: 'same_version' }
  }

  const fallback: UpdateMode =
    (tip?.mode ?? la?.mode ?? input.channelMode) === 'commit'
      ? 'commit'
      : 'release'
  const mode = modeForTarget(target, fallback)
  const isUpgrade = tip?.is_upgrade === true || la?.is_upgrade === true
  const isDowngrade =
    tip?.is_downgrade === true ||
    la?.is_downgrade === true ||
    input.downgradeAvailable === true
  // Dev/commit: build-time upgrades may report relation=unknown without ancestry;
  // only force risk confirm for diverged, or unknown when not a clear upgrade.
  const needsRisk =
    relation === 'diverged' || (relation === 'unknown' && !isUpgrade)

  return { proceed: true, target, mode, isDowngrade, needsRisk }
}
