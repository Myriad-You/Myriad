/** backend: /api/admin/updater (CSRF). direct: /_updater (token, no CSRF). */

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
  current_commit_sha?: string | null
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
  proxy_version?: string | null
  current_version: string | null
  current_commit_sha?: string | null
  channel: string
  update_mode?: UpdateMode
  check_interval_secs?: number
  check_interval_secs_pref?: number | null
  auto_install?: boolean
  /** Pins and in-use backups are never auto-deleted. */
  snapshot_limit_enabled?: boolean
  /** 1–20 */
  snapshot_limit?: number
  maintenance_active: boolean
  maintenance_phase: string
  job_in_flight: string | null
  latest_available?: LatestAvailable | null
  update_available?: boolean
  /** Downgrade needs allow_downgrade. */
  downgrade_available?: boolean
  requires_self_update?: boolean
  last_checked_at?: string | null
  rescue_snapshot_id?: string | null
  rescue_source_version?: string | null
  rollback_version?: string | null
  available_channels?: string[]
  self_update_last?: InfraUpdateLastStatus | null
  proxy_update_last?: InfraUpdateLastStatus | null
  last_failed_update?: LastFailedUpdate | null
}

export interface LastFailedUpdate {
  from_version?: string | null
  to_version?: string | null
  at: string
  reason: string
  job_id: string
}

export const CHECK_INTERVAL_PRESETS = [0, 3600, 21600, 43200, 86400] as const

export const SNAPSHOT_LIMIT_PRESETS = [1, 2, 3, 5, 10, 15, 20] as const

export const SNAPSHOT_LIMIT_DEFAULT = 3
export const SNAPSHOT_LIMIT_MIN = 1
export const SNAPSHOT_LIMIT_MAX = 20

/** 1–20 */
export function clampSnapshotLimit(raw: number): number {
  if (!Number.isFinite(raw)) return SNAPSHOT_LIMIT_DEFAULT
  return Math.min(
    SNAPSHOT_LIMIT_MAX,
    Math.max(SNAPSHOT_LIMIT_MIN, Math.round(raw)),
  )
}

export interface InfraUpdateLastStatus {
  status: 'pending' | 'succeeded' | 'failed'
  target_tag: string
  previous_tag: string
  at: string
  error?: string | null
  rolled_back?: boolean
}

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
  mode?: UpdateMode
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
  snapshot_limit_enabled?: boolean
  snapshot_limit?: number
  eligible_count?: number
  protected_count?: number
  total_count?: number
  pruned_snapshot_ids?: string[]
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

  // CSRF: one force-refresh + retry.
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
        eligible_count?: number
        protected_count?: number
        total_count?: number
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
      // confirm_risk only when risk/downgrade flags are set.
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
    dismissLastFailed: () =>
      wrap<{ ok: boolean }>('POST', '/last-failed/dismiss'),
    dismissSelfUpdateLast: () =>
      wrap<{ ok: boolean }>('POST', '/self-update/last/dismiss'),
    dismissProxyUpdateLast: () =>
      wrap<{ ok: boolean }>('POST', '/proxy-update/last/dismiss'),
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
    rescueContinue: () =>
      wrap<{
        ok: boolean
        job_id: string
        snapshot_id: string
        source_version: string | null
      }>('POST', '/rescue/continue'),
    triggerSelfUpdate: () =>
      wrap<{
        ok: boolean
        helper_container_id: string
        new_updater_tag: string
        previous_updater_tag?: string
        scheduled?: boolean
      }>(
        'POST',
        mode === 'backend' ? '/self-update' : '/admin/self-update',
      ),
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

export const updaterApi = makeUpdaterApi()

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
