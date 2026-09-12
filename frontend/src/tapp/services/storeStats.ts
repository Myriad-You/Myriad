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
  return raw.replaceAll(/\/+$/g, '')
}

export async function fetchStoreDownloadCounts(
  appIds: string[],
): Promise<Record<string, number>> {
  const base = statsBaseUrl()
  if (!base || appIds.length === 0) return {}

  const unique = Iterator.from(new Set(appIds.filter(Boolean))).toArray()
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
      // stats 失败不得打断商店 UI。
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

/** 经 Myriad 后端上报。Edge 看不到浏览器凭据；后端签 HMAC。 */
export function reportStoreInstallHit(input: ReportStoreHitInput): void {
  // fire-and-forget；不阻塞安装 UI。
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
      // 不把 stats 失败抛给 UI。
    }
  })()
}
