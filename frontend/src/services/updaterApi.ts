/**
 * Client for the Myriad updater.
 *
 * Two transports, selected by `mode` at call sites:
 *
 *  - 'backend' (default): goes through `/api/admin/updater/*`. The backend has admin-session
 *    auth and talks to updater-gateway with `UPDATER_GATEWAY_SECRET` (no `UPDATE_TOKEN` in
 *    the fat process). Mutating calls go through the backend's `csrf_middleware`, so we
 *    attach `X-CSRF-Token` for POSTs.
 *
 *  - 'direct': goes through `/_updater/*` on the proxy. Requires the caller to supply
 *    `UPDATE_TOKEN` manually. Kept as a fallback for when the backend is down or the user
 *    is running an operator's rescue flow.
 *
 * See docs/updater-spec.md §13.
 */

import { clearCSRFToken, getCSRFToken } from '../utils/csrf'

const BACKEND_BASE = '/api/admin/updater'
const DIRECT_BASE = '/_updater'

export type TransportMode = 'backend' | 'direct'

export type CommitRelation =
  'ahead' | 'behind' | 'identical' | 'diverged' | 'unknown'

export interface LatestAvailable {
  version: string
  channel: string
  seen_at: string
  mode?: UpdateMode
  source?: 'github' | 'dockerhub'
  commit_sha?: string | null
  /** Running deploy resolved to git sha (commit mode). */
  current_commit_sha?: string | null
  /** Ancestry of tip vs current: ahead = tip is newer. */
  relation?: CommitRelation | string | null
  ahead_by?: number | null
  behind_by?: number | null
  is_upgrade?: boolean | null
  is_downgrade?: boolean | null
  requires_self_update: boolean
  min_updater_version: string | null
  notes_url: string
}

export type UpdateMode = 'release' | 'commit'

export interface CommitListItem {
  sha: string
  short_sha: string
  message: string
  html_url: string
  committed_at: string | null
  tag: string
}

export interface DockerBuildListItem {
  tag: string
  short_sha: string
  /** `commit` (dev-<sha>) or `release` (vX.Y.Z). */
  kind?: 'commit' | 'release' | string
  pushed_at: string | null
  backend_digest: string | null
  frontend_digest: string | null
  backend_url: string
  frontend_url: string
}

export interface ReleaseListItem {
  tag_name: string
  name: string | null
  prerelease: boolean
  version: string
}

export interface CompareResult {
  schema_version: number
  relation: string
  ahead_by: number
  behind_by: number
  current_sha: string | null
  target_sha: string | null
  current_ref: string | null
  target_ref: string
  is_upgrade: boolean
  is_downgrade: boolean
}

export interface UpdaterStatus {
  schema_version: number
  updater_version: string
  /** Running PROXY_TAG from .env (edge reverse-proxy image tag). Optional on older updaters. */
  proxy_version?: string | null
  current_version: string | null
  /** Exact source commit backing current_version, when resolved. */
  current_commit_sha?: string | null
  channel: string
  /** release | commit — present on updater ≥ channel/mode support */
  update_mode?: UpdateMode
  /** Effective check interval seconds (prefs or env). 0 = off. */
  check_interval_secs?: number
  /** Raw prefs value when set; omitted when using env fallback. */
  check_interval_secs_pref?: number | null
  /** Auto-install clear upgrades on the current channel. Default false. */
  auto_install?: boolean
  /**
   * Auto-prune pgdata backups so only the latest N non-keep / non-in-use
   * entries (any age) are retained. Pins (`keep`) and in-use/rescue backups
   * are never auto-deleted. Default true.
   */
  snapshot_limit_enabled?: boolean
  /** Max non-keep / non-protected backups when limit is enabled (1–20). Default 3. */
  snapshot_limit?: number
  maintenance_active: boolean
  maintenance_phase: string
  job_in_flight: string | null
  // 字段从 updater 0.2 起出现，旧 updater 不返回；UI 必须按可选处理。
  latest_available?: LatestAvailable | null
  update_available?: boolean
  /** Target is older than current; confirm then send allow_downgrade. */
  downgrade_available?: boolean
  requires_self_update?: boolean
  last_checked_at?: string | null
  /** Snapshot for one-click continue when stuck in needs_manual. */
  rescue_snapshot_id?: string | null
  rescue_source_version?: string | null
  /** Version pinned locally for rollback as `*:myriad-rollback`. */
  rollback_version?: string | null
  available_channels?: string[]
  /** Last TCB self-update helper outcome (`state/self-update-last.json`), when present. */
  self_update_last?: SelfUpdateLastStatus | null
  /** Last manual proxy upgrade outcome (`state/proxy-update-last.json`), when present. */
  proxy_update_last?: InfraUpdateLastStatus | null
  /**
   * Last failed update attempt. Present even when auto-rollback restored the prior
   * stack and maintenance is idle (job status was `failed`, not `needs_manual`).
   */
  last_failed_update?: LastFailedUpdate | null
}

export interface LastFailedUpdate {
  from_version?: string | null
  to_version?: string | null
  /** RFC3339 */
  at: string
  reason: string
  job_id: string
}

/** UI presets for periodic update checks (seconds). */
export const CHECK_INTERVAL_PRESETS = [
  0, 3600, 21600, 43200, 86400,
] as const
export type CheckIntervalSecs = (typeof CHECK_INTERVAL_PRESETS)[number]

/**
 * Snapshot retention limit presets (count of older non-keep backups kept).
 * BE accepts 1–20; presets cover common values and the max.
 */
export const SNAPSHOT_LIMIT_PRESETS = [1, 2, 3, 5, 10, 15, 20] as const
export type SnapshotLimitPreset = (typeof SNAPSHOT_LIMIT_PRESETS)[number]
export const SNAPSHOT_LIMIT_DEFAULT = 3
export const SNAPSHOT_LIMIT_MIN = 1
export const SNAPSHOT_LIMIT_MAX = 20

/** Clamp a raw snapshot_limit into the documented 1–20 range. */
export function clampSnapshotLimit(raw: number): number {
  if (!Number.isFinite(raw)) return SNAPSHOT_LIMIT_DEFAULT
  return Math.min(
    SNAPSHOT_LIMIT_MAX,
    Math.max(SNAPSHOT_LIMIT_MIN, Math.round(raw)),
  )
}

/** True when n is an integer in SNAPSHOT_LIMIT_MIN..=SNAPSHOT_LIMIT_MAX. */
export function isValidSnapshotLimit(n: number): boolean {
  return (
    Number.isInteger(n) && n >= SNAPSHOT_LIMIT_MIN && n <= SNAPSHOT_LIMIT_MAX
  )
}

/** Shared shape for self-update / proxy-update durable last outcome. */
export interface InfraUpdateLastStatus {
  status: 'succeeded' | 'failed'
  target_tag: string
  previous_tag: string
  /** RFC3339 UTC */
  at: string
  error?: string | null
  /** Proxy only: true when PROXY_TAG was restored after failure. */
  rolled_back?: boolean
}

/** @deprecated Prefer InfraUpdateLastStatus — same JSON shape. */
export type SelfUpdateLastStatus = InfraUpdateLastStatus

export interface ImageRef {
  ref: string
  digest: string
}

export interface ReleaseManifest {
  schema_version: number
  version: string
  channel: string
  released_at: string
  min_from_version?: string
  /** Present when mode=commit (synthetic available payload). */
  mode?: UpdateMode
  /** Metadata provider used for commit-mode availability. */
  source?: 'github' | 'dockerhub'
  commit_sha?: string
  current_commit_sha?: string | null
  relation?: CommitRelation | string | null
  ahead_by?: number | null
  behind_by?: number | null
  is_upgrade?: boolean | null
  is_downgrade?: boolean | null
  message?: string
  images: Record<string, ImageRef>
  env: {
    required: string[]
    new: Array<{
      name: string
      required: boolean
      default?: string
      description?: string
    }>
    removed: string[]
  }
  migrations: {
    irreversible: boolean
    estimated_seconds: number
    requires_full_backup: boolean
  }
  updater: { min_updater_version: string; self_update_required: boolean }
  postgres: { min_pg_version: string; max_pg_version?: string }
  notes_url: string
  signature: string | null
}

export interface SnapshotMeta {
  id: string
  created_at: string
  source_version: string | null
  size_bytes: number
  file_count: number
  keep: boolean
  sample_sha256: string | null
}

export interface SnapshotsResponse {
  schema_version: number
  items: SnapshotMeta[]
}

export interface JobStep {
  phase: string
  started_at: string
  finished_at: string | null
  ok: boolean | null
  log_tail: string
  error: string | null
}

export interface Job {
  id: string
  kind: 'update' | 'rollback' | 'self_update'
  created_at: string
  finished_at: string | null
  from_version: string | null
  to_version: string | null
  snapshot_id: string | null
  status: 'pending' | 'running' | 'succeeded' | 'failed' | 'needs_manual'
  steps: JobStep[]
}

interface CallOptions {
  mode?: TransportMode
  token?: string
  idempotencyKey?: string
}

function isCsrfFailureMessage(detail: string): boolean {
  return /csrf/i.test(detail)
}

async function callOnce<T>(
  method: string,
  path: string,
  body: unknown | undefined,
  opts: CallOptions,
  forceCsrfRefresh: boolean,
): Promise<
  { ok: true; data: T } | { ok: false; status: number; detail: string }
> {
  const mode: TransportMode = opts.mode ?? 'backend'
  const base = mode === 'backend' ? BACKEND_BASE : DIRECT_BASE

  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
    Accept: 'application/json',
  }
  if (opts.token) headers['X-Update-Token'] = opts.token
  if (opts.idempotencyKey) headers['Idempotency-Key'] = opts.idempotencyKey

  // Backend mode + state-changing methods go through backend csrf_middleware.
  // Direct mode bypasses backend entirely so no CSRF token is required.
  const stateChanging =
    method === 'POST' ||
    method === 'PUT' ||
    method === 'PATCH' ||
    method === 'DELETE'
  if (mode === 'backend' && stateChanging) {
    if (forceCsrfRefresh) clearCSRFToken()
    const csrf = await getCSRFToken(forceCsrfRefresh).catch(() => null)
    if (!csrf) {
      return {
        ok: false,
        status: 403,
        detail: 'CSRF token missing; refresh the page and try again',
      }
    }
    headers['X-CSRF-Token'] = csrf
  }

  const resp = await fetch(`${base}${path}`, {
    method,
    headers,
    body: body !== undefined ? JSON.stringify(body) : undefined,
    // backend mode rides on admin session cookie; direct mode is token-only.
    credentials: mode === 'backend' ? 'same-origin' : 'omit',
  })

  if (!resp.ok) {
    const text = await resp.text().catch(() => '')
    let detail = text
    try {
      const parsed = JSON.parse(text) as { error?: string; message?: string }
      detail = parsed.error ?? parsed.message ?? text
    } catch {
      /* keep raw */
    }
    return {
      ok: false,
      status: resp.status,
      detail: detail || resp.statusText,
    }
  }
  if (resp.status === 204) return { ok: true, data: undefined as T }
  return { ok: true, data: (await resp.json()) as T }
}

async function call<T>(
  method: string,
  path: string,
  body?: unknown,
  opts: CallOptions = {},
): Promise<T> {
  const mode: TransportMode = opts.mode ?? 'backend'
  const stateChanging =
    method === 'POST' ||
    method === 'PUT' ||
    method === 'PATCH' ||
    method === 'DELETE'

  let result = await callOnce<T>(method, path, body, opts, false)

  // Backend restart wipes in-memory CSRF store while the browser keeps a stale
  // token in sessionStorage. One forced refresh + retry recovers automatically.
  if (
    !result.ok &&
    mode === 'backend' &&
    stateChanging &&
    result.status === 403 &&
    isCsrfFailureMessage(result.detail)
  ) {
    result = await callOnce<T>(method, path, body, opts, true)
  }

  if (!result.ok) {
    throw new UpdaterError(result.status, result.detail)
  }
  return result.data
}

export class UpdaterError extends Error {
  constructor(
    public status: number,
    message: string,
  ) {
    super(message)
    this.name = 'UpdaterError'
  }
}

/**
 * Builder so callers can pin a single transport mode + token once and reuse it.
 *
 * Usage:
 *   const api = makeUpdaterApi({ mode: 'backend' })       // default
 *   const api = makeUpdaterApi({ mode: 'direct', token }) // fallback
 */
export function makeUpdaterApi(
  opts: { mode?: TransportMode; token?: string } = {},
) {
  const mode = opts.mode ?? 'backend'
  const token = opts.token

  const wrap = <T>(
    method: string,
    path: string,
    body?: unknown,
    idempotencyKey?: string,
  ) => call<T>(method, path, body, { mode, token, idempotencyKey })

  return {
    mode,
    status: () => wrap<UpdaterStatus>('GET', '/status'),
    available: (opts?: { channel?: string; mode?: UpdateMode }) => {
      const q = new URLSearchParams()
      if (opts?.channel) q.set('channel', opts.channel)
      if (opts?.mode) q.set('mode', opts.mode)
      const qs = q.toString()
      return wrap<ReleaseManifest | null>(
        'GET',
        `/available${qs ? `?${qs}` : ''}`,
      )
    },
    jobs: () => wrap<string[]>('GET', '/jobs'),
    job: (id: string) => wrap<Job>('GET', `/jobs/${id}`),
    snapshots: () => wrap<SnapshotsResponse>('GET', '/snapshots'),
    setPrefs: (prefs: {
      channel?: string
      mode?: UpdateMode
      check_interval_secs?: number | null
      auto_install?: boolean
      snapshot_limit_enabled?: boolean
      snapshot_limit?: number
    }) =>
      wrap<{
        ok: boolean
        channel: string
        mode: UpdateMode
        check_interval_secs?: number
        check_interval_secs_pref?: number | null
        auto_install?: boolean
        snapshot_limit_enabled?: boolean
        snapshot_limit?: number
        pruned_snapshot_ids?: string[]
      }>('POST', '/prefs', prefs),
    commits: (opts?: { branch?: string; limit?: number }) => {
      const q = new URLSearchParams()
      if (opts?.branch) q.set('branch', opts.branch)
      if (opts?.limit) q.set('limit', String(opts.limit))
      const qs = q.toString()
      return wrap<{
        schema_version: number
        branch: string
        items: CommitListItem[]
      }>('GET', `/commits${qs ? `?${qs}` : ''}`)
    },
    builds: (opts?: { limit?: number }) => {
      const q = new URLSearchParams()
      if (opts?.limit) q.set('limit', String(opts.limit))
      const qs = q.toString()
      return wrap<{
        schema_version: number
        source: 'dockerhub'
        items: DockerBuildListItem[]
      }>('GET', `/builds${qs ? `?${qs}` : ''}`)
    },
    releases: (opts?: { channel?: string; limit?: number }) => {
      const q = new URLSearchParams()
      if (opts?.channel) q.set('channel', opts.channel)
      if (opts?.limit) q.set('limit', String(opts.limit))
      const qs = q.toString()
      return wrap<{
        schema_version: number
        channel: string
        items: ReleaseListItem[]
      }>('GET', `/releases${qs ? `?${qs}` : ''}`)
    },
    compare: (to: string, from?: string) => {
      const q = new URLSearchParams({ to })
      if (from) q.set('from', from)
      return wrap<CompareResult>('GET', `/compare?${q.toString()}`)
    },
    triggerUpdate: (
      target: string,
      opts?: {
        mode?: UpdateMode
        idemKey?: string
        commit?: boolean
        allowDowngrade?: boolean
        allowRisk?: boolean
        allowDiverged?: boolean
        allowUnknown?: boolean
        allowIrreversible?: boolean
      },
    ) => {
      const allowDowngrade = !!opts?.allowDowngrade
      const allowRisk = !!opts?.allowRisk
      const allowDiverged = opts?.allowDiverged
      const allowUnknown = opts?.allowUnknown
      const allowIrreversible = opts?.allowIrreversible
      // Soft gate: only when risk/downgrade flags are set; normal upgrades omit confirm_risk.
      const needsConfirm =
        allowDowngrade ||
        allowRisk ||
        allowDiverged === true ||
        allowUnknown === true ||
        allowIrreversible === true
      return wrap<{ job_id: string; mode?: string }>(
        'POST',
        '/update',
        {
          ...(opts?.commit || opts?.mode === 'commit'
            ? { target_commit: target, mode: 'commit' as const }
            : {
                target_version: target,
                mode: (opts?.mode ?? 'release') as UpdateMode,
              }),
          allow_downgrade: allowDowngrade,
          allow_risk: allowRisk,
          allow_diverged: allowDiverged,
          allow_unknown: allowUnknown,
          allow_irreversible: allowIrreversible,
          ...(needsConfirm ? { confirm_risk: true } : {}),
        },
        opts?.idemKey,
      )
    },
    rollback: (snapshotId: string) =>
      wrap<{ job_id: string }>('POST', '/rollback', {
        snapshot_id: snapshotId,
      }),
    /** Permanently remove a single backup snapshot. */
    deleteSnapshot: (snapshotId: string) =>
      wrap<{ ok: boolean; id: string }>(
        'DELETE',
        `/snapshots/${encodeURIComponent(snapshotId)}`,
      ),
    diagnostics: () => wrap<unknown>('GET', '/diagnostics'),
    exitMaintenance: () =>
      wrap<{ ok: boolean }>('POST', '/rescue/exit-maintenance'),
    forgetCurrent: () =>
      wrap<{ ok: boolean }>('POST', '/rescue/forget-current'),
    /** One-click rollback to the snapshot on the stuck needs_manual job. */
    rescueContinue: () =>
      wrap<{
        ok: boolean
        job_id: string
        snapshot_id: string
        source_version: string | null
      }>('POST', '/rescue/continue'),
    /** 触发 updater 自更新；旧 updater 几秒后会被 helper container 替换。 */
    triggerSelfUpdate: () =>
      wrap<{
        ok: boolean
        helper_container_id: string
        new_updater_tag: string
        previous_updater_tag?: string
        scheduled?: boolean
      }>(
        'POST',
        // backend mode: 走 backend 代理；direct mode: 直接命中 updater /admin/self-update
        mode === 'backend' ? '/self-update' : '/admin/self-update',
      ),
    /**
     * 手动升级 proxy 镜像（改 PROXY_TAG + compose up proxy）。
     * 不在业务自动更新路径内；短暂边缘 downtime（通常 <10s）。
     */
    triggerProxyUpdate: (targetVersion?: string) =>
      wrap<{
        ok: boolean
        previous_proxy_tag: string
        new_proxy_tag: string
        image_ref: string
        pulled_digest: string
      }>(
        'POST',
        mode === 'backend' ? '/proxy-update' : '/admin/proxy-update',
        targetVersion ? { target_version: targetVersion } : {},
      ),
  }
}

/** Default singleton — uses backend transport (admin session). */
export const updaterApi = makeUpdaterApi()

/**
 * Compare the browser-cached frontend version with what the backend currently reports.
 * The updater health flow ensures version matching at swap time, but a tab opened before
 * the swap will keep running the old bundle. Use this on app load to nudge a reload.
 */
export async function detectVersionDrift(): Promise<{
  current: string
  build: string
  drift: boolean
} | null> {
  try {
    const built = document
      .querySelector('meta[name="myriad-version"]')
      ?.getAttribute('content')
    if (!built) return null
    const resp = await fetch('/health', { credentials: 'omit' })
    if (!resp.ok) return null
    const health = await resp.json()
    const current = String(health?.version ?? '')
    if (!current) return null
    return {
      current,
      build: built,
      drift: current !== built && built !== 'dev',
    }
  } catch {
    return null
  }
}
