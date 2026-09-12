import { apiRequest } from './TappHttpClient'

export async function listPlatform(runtimeGrant?: string): Promise<{
  reports: {
    id: string
    platform: string
    type: 'platform'
    createdAt: string
    summary?: string
  }[]
}> {
  return apiRequest('/api/tapp/report-catalog', { runtimeGrant })
}

export async function getPlatform(
  reportId: string,
  runtimeGrant?: string,
): Promise<{
  id: string
  platform?: string
  type: 'platform'
  summary?: string
  content: unknown
  createdAt: string
}> {
  return apiRequest(
    `/api/tapp/report-catalog/${encodeURIComponent(reportId)}`,
    { runtimeGrant },
  )
}

export async function byPlatform(
  platform: string,
  runtimeGrant?: string,
): Promise<{
  platform: string
  summary: string
  insights: string[]
  metadata: unknown
  content?: unknown
  card_visuals?: unknown
  cardVisuals?: unknown
  createdAt: string
} | null> {
  try {
    const raw = await apiRequest<Record<string, unknown> | null>(
      `/api/tapp/report-catalog/platform/${encodeURIComponent(platform)}`,
      { runtimeGrant },
    )
    if (!raw || typeof raw !== 'object') return null

    const content =
      raw.content && typeof raw.content === 'object'
        ? (raw.content as Record<string, unknown>)
        : undefined
    const cardVisuals =
      raw.cardVisuals ??
      raw.card_visuals ??
      content?.card_visuals ??
      content?.cardVisuals ??
      null

    return {
      platform: String(raw.platform ?? platform),
      summary: String(raw.summary ?? content?.summary ?? ''),
      insights: Array.isArray(raw.insights)
        ? (raw.insights as string[])
        : Array.isArray(content?.insights)
          ? (content!.insights as string[])
          : [],
      metadata: raw.metadata ?? content?.metadata,
      content: raw.content,
      card_visuals: cardVisuals,
      cardVisuals,
      createdAt: String(raw.createdAt ?? raw.created_at ?? ''),
    }
  } catch {
    return null
  }
}
