/** Platform data APIs exposed to the Tapp host runtime. */

import type {
  NewPlatformItem,
  PlatformInfo,
  PlatformItemResult,
} from '../types'
import { apiRequest } from './TappHttpClient'

/** Raw `/api/platforms` row — host UI keeps numeric id + display name. */
interface ApiPlatformRow {
  id: number | string
  name: string
  enabled?: boolean
  icon?: string
  /** Stable slug (DB name), e.g. "steam" */
  slug?: string
  key?: string
  description?: string
  color?: string
  isTappPlatform?: boolean
  tappId?: string
}

/**
 * List enabled platforms for Tapp SDK.
 * Maps `id`/`key` to the stable slug so `platform.getData(id)` hits
 * `cache/platforms/{slug}_filtered.json`. Display name stays in `name`.
 */
export async function listEnabledPlatforms(
  runtimeGrant?: string,
): Promise<PlatformInfo[]> {
  const data = await apiRequest<{ platforms: ApiPlatformRow[] }>(
    '/api/platforms',
    { runtimeGrant },
  )
  return data.platforms
    .filter((platform) => platform.enabled)
    .map((platform) => {
      const slug =
        (platform.slug && String(platform.slug).trim()) ||
        (platform.key && String(platform.key).trim()) ||
        // Legacy fallback: some environments may already send slug as id
        (typeof platform.id === 'string' &&
        platform.id &&
        !/^\d+$/.test(platform.id)
          ? platform.id
          : '')
      const stableId = slug || String(platform.id)
      return {
        id: stableId,
        key: stableId,
        name: platform.name,
        icon: platform.icon || stableId,
        color: platform.color || '',
        enabled: true,
        isTappPlatform: !!platform.isTappPlatform,
        tappId: platform.tappId,
        description: platform.description,
      } satisfies PlatformInfo
    })
}

export async function getPlatformData(
  platform: string,
  options?: {
    limit?: number
    offset?: number
  },
  runtimeGrant?: string,
): Promise<{
  items: unknown[]
  total: number
  platform: string
}> {
  const params = new URLSearchParams()
  if (options?.limit !== undefined) params.set('limit', String(options.limit))
  if (options?.offset !== undefined)
    params.set('offset', String(options.offset))

  const query = params.size > 0 ? `?${params}` : ''
  return apiRequest(
    `/api/tapp/platform/${encodeURIComponent(platform)}/data${query}`,
    { runtimeGrant },
  )
}

export async function getPlatformStats(
  platform: string,
  runtimeGrant?: string,
): Promise<{
  platform: string
  total: number
  distribution: Record<string, number>
  recentActivity: { date: string; count: number }[]
}> {
  return apiRequest(`/api/tapp/platform/${platform}/stats`, { runtimeGrant })
}

export async function getPlatformDistribution(
  platform: string,
  dimension: string,
  runtimeGrant?: string,
): Promise<{ dimension: string; data: { label: string; value: number }[] }> {
  return apiRequest(
    `/api/tapp/platform/${platform}/distribution/${dimension}`,
    { runtimeGrant },
  )
}

export async function addPlatformItem(
  tappId: string,
  item: NewPlatformItem,
  runtimeGrant?: string,
): Promise<PlatformItemResult> {
  return apiRequest('/api/tapp/platform/items', {
    method: 'POST',
    body: JSON.stringify({
      tapp_id: tappId,
      item,
    }),
    runtimeGrant,
  })
}

export async function addPlatformItems(
  tappId: string,
  items: NewPlatformItem[],
  runtimeGrant?: string,
): Promise<{ success: boolean; results: PlatformItemResult[] }> {
  return apiRequest('/api/tapp/platform/items/batch', {
    method: 'POST',
    body: JSON.stringify({
      tapp_id: tappId,
      items,
    }),
    runtimeGrant,
  })
}
