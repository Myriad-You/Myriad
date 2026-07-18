/** Platform data APIs exposed to the Tapp host runtime. */

import type {
  NewPlatformItem,
  PlatformInfo,
  PlatformItemResult,
} from '../types'
import { apiRequest } from './TappHttpClient'

export async function listEnabledPlatforms(
  runtimeGrant?: string,
): Promise<PlatformInfo[]> {
  const data = await apiRequest<{ platforms: PlatformInfo[] }>(
    '/api/platforms',
    { runtimeGrant },
  )
  return data.platforms.filter((platform) => platform.enabled)
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
