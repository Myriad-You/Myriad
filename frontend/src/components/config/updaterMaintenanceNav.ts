export const PROXY_STATUS_POLL_MS = 1_500
export const MAINT_NAV_STORAGE_KEY = 'myriad-updater-maint-nav'

export interface ProxyMaintenanceStatus {
  active: boolean
  phase?: string
}

export function parseProxyStatus(body: unknown): ProxyMaintenanceStatus | null {
  if (!body || typeof body !== 'object') return null
  const maintenance = (body as { maintenance?: unknown }).maintenance
  if (!maintenance || typeof maintenance !== 'object') return null
  const active = (maintenance as { active?: unknown }).active
  if (typeof active !== 'boolean') return null
  const phase = (maintenance as { phase?: unknown }).phase
  return {
    active,
    phase: typeof phase === 'string' ? phase : undefined,
  }
}

export function isLikelyMaintenanceHtml(detail: string): boolean {
  const s = detail.trimStart().toLowerCase()
  return (
    s.startsWith('<!doctype') ||
    s.startsWith('<html') ||
    (s.includes('maintenance') && s.includes('</html>'))
  )
}

export function hasNavigatedForJob(jobId: string): boolean {
  if (!jobId) return false
  try {
    return sessionStorage.getItem(MAINT_NAV_STORAGE_KEY) === jobId
  } catch {
    return false
  }
}

export function markNavigatedForJob(jobId: string): void {
  if (!jobId) return
  try {
    sessionStorage.setItem(MAINT_NAV_STORAGE_KEY, jobId)
  } catch {
    /* private mode / quota */
  }
}

export function shouldNavigateToMaintenance(opts: {
  jobId: string
  proxyActive?: boolean | null
  statusMaintenanceActive?: boolean
  nonJson503DuringJob?: boolean
  alreadyNavigated?: boolean
}): boolean {
  if (!opts.jobId) return false
  if (opts.alreadyNavigated ?? hasNavigatedForJob(opts.jobId)) return false
  if (opts.proxyActive === true) return true
  if (opts.statusMaintenanceActive === true) return true
  if (opts.nonJson503DuringJob === true) return true
  return false
}

export function navigateToMaintenancePage(jobId: string): void {
  markNavigatedForJob(jobId)
  window.location.assign('/')
}

export type MaintenancePollStop = () => void

export function startMaintenancePoll(opts: {
  jobId: string
  intervalMs?: number
  getStatusMaintenanceActive?: () => boolean
  onNavigate: () => void
  signal?: AbortSignal
  fetchImpl?: typeof fetch
}): MaintenancePollStop {
  const intervalMs = opts.intervalMs ?? PROXY_STATUS_POLL_MS
  const fetchFn = opts.fetchImpl ?? fetch
  let stopped = false
  let timer: ReturnType<typeof setTimeout> | null = null

  const stop: MaintenancePollStop = () => {
    stopped = true
    if (timer != null) {
      clearTimeout(timer)
      timer = null
    }
  }

  const schedule = () => {
    if (stopped || opts.signal?.aborted) {
      stop()
      return
    }
    timer = setTimeout(() => {
      void tick()
    }, intervalMs)
  }

  const tick = async () => {
    if (stopped || opts.signal?.aborted) {
      stop()
      return
    }
    if (hasNavigatedForJob(opts.jobId)) {
      stop()
      return
    }

    let proxyActive: boolean | null = null
    let nonJson503 = false

    try {
      const res = await fetchFn('/_proxy/status', {
        credentials: 'same-origin',
        cache: 'no-store',
        signal: opts.signal,
      })
      const ct = res.headers.get('content-type') ?? ''
      if (res.ok && ct.includes('application/json')) {
        const body: unknown = await res.json()
        const parsed = parseProxyStatus(body)
        proxyActive = parsed?.active ?? null
      } else if (res.status === 503) {
        const text = await res.text().catch(() => '')
        if (isLikelyMaintenanceHtml(text)) {
          nonJson503 = true
        } else {
          try {
            const body: unknown = JSON.parse(text)
            const parsed = parseProxyStatus(body)
            if (parsed?.active) proxyActive = true
          } catch {
            nonJson503 = true
          }
        }
      }
    } catch {
      // network blip — keep polling
    }

    const statusActive = opts.getStatusMaintenanceActive?.() ?? false

    if (
      shouldNavigateToMaintenance({
        jobId: opts.jobId,
        proxyActive,
        statusMaintenanceActive: statusActive,
        nonJson503DuringJob: nonJson503,
      })
    ) {
      stop()
      opts.onNavigate()
      return
    }

    schedule()
  }

  void tick()
  return stop
}
