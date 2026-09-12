import type { TappAppCardSize } from '../components/TappAppCard'
import { apiRequest } from './TappHttpClient'

export type TappListCardSizesMap = Record<string, TappAppCardSize>

export interface TappListCardLayout {
  sizes: TappListCardSizesMap
  order: string[]
}

export interface TappListCardLayoutResponse extends TappListCardLayout {
  siteSizes: TappListCardSizesMap
  siteOrder: string[]
  source: 'viewer' | 'site_owner' | string
  writable: boolean
}

function normalizeSizes(raw: unknown): TappListCardSizesMap {
  if (!raw || typeof raw !== 'object') return {}
  const out: TappListCardSizesMap = {}
  for (const [id, size] of Object.entries(raw as Record<string, unknown>)) {
    if (size === '1x1' || size === '2x1') out[id] = size
  }
  return out
}

function normalizeOrder(raw: unknown): string[] {
  if (!Array.isArray(raw)) return []
  const seen = new Set<string>()
  const out: string[] = []
  for (const item of raw) {
    if (typeof item !== 'string') continue
    const id = item.trim()
    if (!id || seen.has(id)) continue
    seen.add(id)
    out.push(id)
  }
  return out
}

export async function fetchTappListCardSizes(): Promise<TappListCardLayoutResponse> {
  const data = await apiRequest<{
    sizes?: unknown
    order?: unknown
    site_sizes?: unknown
    site_order?: unknown
    source?: unknown
    writable?: unknown
  }>('/api/tapps/list-card-sizes')
  const sizes = normalizeSizes(data?.sizes)
  const order = normalizeOrder(data?.order)
  const siteSizes = normalizeSizes(data?.site_sizes)
  const siteOrder = normalizeOrder(data?.site_order)
  const hasSitePayload =
    data != null &&
    typeof data === 'object' &&
    (Object.hasOwn(data, 'site_sizes') || Object.hasOwn(data, 'site_order'))
  return {
    sizes,
    order,
    siteSizes: hasSitePayload ? siteSizes : sizes,
    siteOrder: hasSitePayload ? siteOrder : order,
    source:
      typeof data?.source === 'string' && data.source
        ? data.source
        : 'site_owner',
    writable: data?.writable === true,
  }
}

export async function saveTappListCardSizes(
  layout: TappListCardLayout | TappListCardSizesMap,
): Promise<TappListCardLayout> {
  const body: TappListCardLayout =
    layout && typeof layout === 'object' && Object.hasOwn(layout, 'sizes')
      ? {
          sizes: normalizeSizes((layout as TappListCardLayout).sizes),
          order: normalizeOrder((layout as TappListCardLayout).order),
        }
      : { sizes: normalizeSizes(layout), order: [] }

  const data = await apiRequest<{ sizes?: unknown; order?: unknown }>(
    '/api/tapps/list-card-sizes',
    {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    },
  )
  return {
    sizes: normalizeSizes(data?.sizes),
    order: normalizeOrder(data?.order),
  }
}
