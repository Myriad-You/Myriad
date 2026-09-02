/**
 * Official Tapp store install stats.
 * - Reads: public edge GET /v1/stats
 * - Writes: only via Myriad backend (auth + CSRF + HMAC to edge)
 */

export const DEFAULT_STORE_STATS_URL = 'https://stats.store.myriad.you'

const STATS_TTL_MS = 60_000

export type StoreStatsEvent = 'install' | 'update'

export interface StoreAppStats {
  installs: number
  updates: number
  downloads: number
}

export interface StoreStatsResponse {
  updated_at: string
  apps: Record<string, StoreAppStats>
  ranked?: Array<{ id: string } & StoreAppStats>
}

let cachedAt = 0
let cachedMap: Record<string, number> = {}

function statsBaseUrl(): string | null {
  const fromEnv =
    typeof import.meta !== 'undefined' &&
    (import.meta as { env?: Record<string, string> }).env
      ?.VITE_TAPP_STORE_STATS_URL
  const raw = (fromEnv || DEFAULT_STORE_STATS_URL).trim()
  if (!raw || raw === '0' || raw === 'false' || raw === 'off') return null
  return raw.replace(/\/+$/, '')
}

/** Merge download counts for the given app ids (batch ≤ 100). */
export async function fetchStoreDownloadCounts(
  appIds: string[],
): Promise<Record<string, number>> {
  const base = statsBaseUrl()
  if (!base || appIds.length === 0) return {}

  const unique = [...new Set(appIds.filter(Boolean))]
  const now = Date.now()
  const out: Record<string, number> = {}
  const missing: string[] = []

  for (const id of unique) {
    if (now - cachedAt < STATS_TTL_MS && cachedMap[id] !== undefined) {
      if (cachedMap[id] > 0) out[id] = cachedMap[id]
    } else {
      missing.push(id)
    }
  }

  if (missing.length === 0) return out

  const batchSize = 100
  for (let i = 0; i < missing.length; i += batchSize) {
    const batch = missing.slice(i, i + batchSize)
    try {
      const url = `${base}/v1/stats?apps=${encodeURIComponent(batch.join(','))}`
      const res = await fetch(url, {
        method: 'GET',
        headers: { Accept: 'application/json' },
        signal: AbortSignal.timeout(5000),
      })
      if (!res.ok) continue
      const data = (await res.json()) as StoreStatsResponse
      for (const id of batch) {
        const entry = data.apps?.[id]
        const n = entry?.downloads ?? entry?.installs ?? 0
        cachedMap[id] = typeof n === 'number' && n > 0 ? n : 0
        if (cachedMap[id] > 0) out[id] = cachedMap[id]
      }
    } catch {
      // stats failure must never break store UI
    }
  }
  cachedAt = Date.now()
  return out
}

export function clearStoreStatsCache(): void {
  cachedAt = 0
  cachedMap = {}
}

export interface ReportStoreHitInput {
  appId: string
  version: string
  event: StoreStatsEvent
}

/**
 * Report install/update via Myriad backend (session cookie + CSRF via apiRequest).
 * Edge never sees browser credentials; backend signs HMAC.
 */
export function reportStoreInstallHit(input: ReportStoreHitInput): void {
  // Fire-and-forget; do not block install UI. Use dynamic import to avoid cycles.
  void (async () => {
    try {
      const { apiRequest } = await import('./TappHttpClient')
      await apiRequest('/api/tapps/store/stats-report', {
        method: 'POST',
        body: JSON.stringify({
          appId: input.appId,
          version: input.version,
          event: input.event,
        }),
      })
    } catch {
      // never surface stats failures
    }
  })()
}
