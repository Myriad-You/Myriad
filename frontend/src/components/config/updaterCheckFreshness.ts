import type { UpdateMode } from '../../services/updaterApi'

export const STALE_WHEN_OFF_SECS = 3600

export const AGO_TICK_MS = 30_000

export function checkAgeSecs(
  lastCheckedAt: string | null | undefined,
  nowMs: number = Date.now(),
): number | null {
  if (lastCheckedAt == null || lastCheckedAt === '') return null
  const then = new Date(lastCheckedAt).getTime()
  if (Number.isNaN(then)) return null
  return Math.max(0, (nowMs - then) / 1000)
}

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

export interface LatestTipFields {
  version?: string | null
  mode?: UpdateMode | null
  relation?: string | null
  is_upgrade?: boolean | null
  is_downgrade?: boolean | null
}

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
