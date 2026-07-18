/** Read-only report catalog APIs used by Tapp host surfaces. */

import { apiRequest } from './TappHttpClient'

export async function listReports(runtimeGrant?: string): Promise<{
  reports: {
    id: string
    platform: string
    type: 'platform' | 'comprehensive'
    createdAt: string
    summary?: string
  }[]
}> {
  return apiRequest('/api/tapp/report-catalog', { runtimeGrant })
}

export async function getReport(
  reportId: string,
  runtimeGrant?: string,
): Promise<{
  id: string
  platform?: string
  type: 'platform' | 'comprehensive'
  content: unknown
  createdAt: string
}> {
  return apiRequest(
    `/api/tapp/report-catalog/${encodeURIComponent(reportId)}`,
    { runtimeGrant },
  )
}

export async function getPlatformReport(
  platform: string,
  runtimeGrant?: string,
): Promise<{
  platform: string
  summary: string
  insights: string[]
  metadata: unknown
  cardVisuals: unknown
  createdAt: string
} | null> {
  try {
    return await apiRequest(
      `/api/tapp/report-catalog/platform/${encodeURIComponent(platform)}`,
      { runtimeGrant },
    )
  } catch {
    return null
  }
}
