/**
 * Updater pure helpers / channel model / mood derivation.
 */

import type { useI18n } from '../../../contexts/I18nContext'
import type {
  SnapshotMeta,
  UpdateMode,
  UpdaterStatus,
} from '../../../services/updaterApi'
import { UpdaterError } from '../../../services/updaterApi'
import { computeAgo } from '../updaterCheckFreshness'

export type U = ReturnType<typeof useI18n>['t']['config']

export type ChannelKey = 'stable' | 'preview' | 'dev'

export interface ChannelOption {
  key: ChannelKey
  mode: UpdateMode
  channel: string
  badge: 'recommended' | 'dev' | null
}

export type Mood =
  | 'healthy'
  | 'available'
  | 'downgrade'
  | 'updating'
  | 'maintenance'
  | 'needsManual'
  | 'offline'
  | 'firstRun'

export type Tone = 'ok' | 'info' | 'warn' | 'danger' | 'muted'

export type Toast = { kind: 'ok' | 'error'; text: string } | null

export const POLL_INTERVAL = 4_000
/** Trusted TCB handoff + bounded recovery: 2s × 2700 ≈ 90 minutes. */
export const INFRA_OUTCOME_POLL_MS = 2_000
export const INFRA_OUTCOME_MAX_TRIES = 2_700
const TEMPLATE_RE = /\{(\w+)\}/g
export const COMMIT_URL = 'https://github.com/Myriad-You/Myriad/commit/'

export const CHANNEL_OPTIONS: ChannelOption[] = [
  { key: 'stable', mode: 'release', channel: 'stable', badge: 'recommended' },
  { key: 'preview', mode: 'release', channel: 'preview', badge: null },
  { key: 'dev', mode: 'commit', channel: 'preview', badge: 'dev' },
]

/** Formal release tags look like v0.2.6 (`v`-prefixed semver, matching DeployTag). */
export function isReleaseTag(tag: string): boolean {
  return /^v\d+\.\d+\.\d+([.-][0-9A-Za-z.]+)?$/.test(tag.trim())
}

export function modeForTarget(target: string, fallback: UpdateMode): UpdateMode {
  // Non-semver targets must never be forced to release mode (BE 400).
  if (isReleaseTag(target)) return 'release'
  if (fallback === 'commit') return 'commit'
  // Default non-tag → commit (sha / branch / free text)
  return 'commit'
}

export function format(template: string, params: Record<string, string>): string {
  return template.replace(TEMPLATE_RE, (_, k) => params[k] ?? `{${k}}`)
}

/** Brief proxy/updater restart windows surface as 502/503 or fetch failures. */
/**
 * Whether a durable infra outcome is new enough to stop polling.
 * `targetTag` is only applied when the schedule RPC actually returned one —
 * the success path kills the updater before that response arrives, and the
 * app tip is a different version space from the updater tag.
 */
export function isFreshInfraOutcome(
  last:
    | {
        status?: string | null
        target_tag?: string | null
        at?: string | null
      }
    | null
    | undefined,
  beforeAt: string | null | undefined,
  targetTag: string,
): 'succeeded' | 'failed' | null {
  if (!last) return null
  if (beforeAt && last.at === beforeAt) return null
  if (targetTag && last.target_tag && last.target_tag !== targetTag) {
    return null
  }
  if (last.status === 'succeeded' || last.status === 'failed') {
    return last.status
  }
  return null
}

export function isTransientUpdaterError(e: unknown): boolean {
  if (e instanceof UpdaterError) {
    return (
      e.status === 0 ||
      e.status === 502 ||
      e.status === 503 ||
      e.status === 504
    )
  }
  const msg = e instanceof Error ? e.message : String(e)
  return /failed to fetch|networkerror|load failed|aborted|timeout|network/i.test(
    msg,
  )
}

export function sleep(ms: number): Promise<void> {
  return new Promise((r) => window.setTimeout(r, ms))
}

/** 从 upstream 转发的错误体里提取一句人能读的话（剥掉嵌套 JSON）。 */
export function upstreamDetail(message: string): string {
  const brace = message.indexOf('{')
  if (brace >= 0) {
    try {
      const parsed = JSON.parse(message.slice(brace)) as {
        error?: string
        message?: string
      }
      const inner = parsed.error ?? parsed.message
      if (inner) {
        const cut = inner.indexOf(' {')
        return (cut > 0 ? inner.slice(0, cut) : inner).trim()
      }
    } catch {
      /* fall through to raw message */
    }
  }
  return message.length > 160 ? `${message.slice(0, 160)}…` : message
}

export function channelLabel(key: ChannelKey, u: U): string {
  switch (key) {
    case 'stable':
      return u.updaterChannelStable
    case 'preview':
      return u.updaterChannelPreview
    case 'dev':
      return u.updaterChannelDev
  }
}

export function channelDesc(key: ChannelKey, u: U): string {
  switch (key) {
    case 'stable':
      return u.updaterChannelStableDesc
    case 'preview':
      return u.updaterChannelPreviewDesc
    case 'dev':
      return u.updaterChannelDevDesc
  }
}

/** 服务器保存的 (mode, channel) → 三选项之一。兼容旧命名。 */
export function deriveSelection(status: UpdaterStatus | null): ChannelKey {
  if (!status) return 'stable'
  if (status.update_mode === 'commit') return 'dev'
  return status.channel === 'preview' ? 'preview' : 'stable'
}

export function deriveMood(status: UpdaterStatus | null): Mood {
  if (!status) return 'offline'
  if (status.job_in_flight) return 'updating'
  if (status.maintenance_phase === 'needs_manual') return 'needsManual'
  if (status.maintenance_active) return 'maintenance'
  if (!status.current_version) return 'firstRun'
  if (status.update_available) return 'available'
  if (status.downgrade_available) return 'downgrade'
  return 'healthy'
}

export function moodText(
  mood: Mood,
  u: U,
): { title: string; hint: string | null; tone: Tone } {
  switch (mood) {
    case 'healthy':
      return {
        title: u.updaterStatusHealthy,
        hint: u.updaterHintHealthy,
        tone: 'ok',
      }
    case 'available':
      return { title: u.updaterStatusAvailable, hint: null, tone: 'info' }
    case 'downgrade':
      return { title: u.updaterStatusDowngrade, hint: null, tone: 'muted' }
    case 'updating':
      return {
        title: u.updaterStatusUpdating,
        hint: u.updaterHintUpdating,
        tone: 'warn',
      }
    case 'maintenance':
      return {
        title: u.updaterStatusMaintenance,
        hint: u.updaterHintMaintenance,
        tone: 'warn',
      }
    case 'needsManual':
      return {
        title: u.updaterStatusNeedsManual,
        hint: u.updaterHintNeedsManual,
        tone: 'danger',
      }
    case 'offline':
      return {
        title: u.updaterStatusOffline,
        hint: u.updaterHintOffline,
        tone: 'danger',
      }
    case 'firstRun':
      return {
        title: u.updaterStatusFirstRun,
        hint: u.updaterHintFirstRun,
        tone: 'muted',
      }
  }
}

/** Why delete is blocked (scheme-2 policy). Null when delete is allowed. */
export function snapshotDeleteBlockReason(
  snap: SnapshotMeta,
  opts: {
    rescueSnapshotId?: string | null
    totalSnapshots: number
    u: U
  },
): string | null {
  if (opts.rescueSnapshotId === snap.id) {
    return opts.u.updaterDeleteSnapshotInUse
  }
  if (snap.keep) {
    return opts.u.updaterDeleteSnapshotKept
  }
  if (opts.totalSnapshots <= 1) {
    return opts.u.updaterDeleteSnapshotLast
  }
  return null
}

export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`
  if (n < 1024 ** 2) return `${(n / 1024).toFixed(1)} KB`
  if (n < 1024 ** 3) return `${(n / 1024 ** 2).toFixed(1)} MB`
  return `${(n / 1024 ** 3).toFixed(2)} GB`
}

export function formatAgo(iso: string, u: U, nowMs: number = Date.now()): string {
  const parts = computeAgo(iso, nowMs)
  if (!parts) return u.updaterUnknown
  switch (parts.unit) {
    case 'justNow':
      return u.updaterAgoJustNow
    case 'min':
      return format(u.updaterAgoMin, { n: String(parts.n) })
    case 'hour':
      return format(u.updaterAgoHour, { n: String(parts.n) })
    case 'day':
      return format(u.updaterAgoDay, { n: String(parts.n) })
  }
}
