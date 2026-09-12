import { apiRequest } from './TappHttpClient'

export interface AnalyticsSummaryQuery {
  days?: number
  from?: string
  to?: string
}

export interface AnalyticsSummary {
  success?: boolean
  enabled?: boolean
  source?: string
  days?: number
  from?: string
  to?: string
  timezone?: string
  today?: {
    views: number
    unique_visitors: number
  }
  range?: {
    views: number
    unique_visitors: number
    engagement_ms?: number
    engaged_views?: number
    avg_engagement_ms?: number
    approx_bounce_permille?: number
  }
  all_time?: {
    views: number
    unique_visitors: number
  }
  daily?: Array<{
    day: string
    views: number
    unique_visitors: number
    engagement_ms?: number
  }>
  pages?: Array<{
    path: string
    views: number
    unique_visitors: number
    avg_engagement_ms?: number
  }>
  events?: Array<{
    name: string
    count: number
    unique_visitors: number
    targets?: Array<{
      target: string
      count: number
      unique_visitors: number
    }>
  }>
  referrers?: Array<{
    host: string
    count: number
  }>
  countries?: Array<{
    code: string
    name: string
    views: number
    unique_visitors: number
  }>
  compare?: unknown
  retention?: unknown
  definitions?: unknown
}

export interface AnalyticsVisitorCard {
  success?: boolean
  enabled?: boolean
  source?: string
  today?: {
    views: number
    unique_visitors: number
  }
  all_time?: {
    views: number
    unique_visitors: number
  }
  daily?: Array<{
    day: string
    views: number
    unique_visitors: number
  }>
  [key: string]: unknown
}

/** 需要授予 analytics:read。 */
export async function getAnalyticsSummary(
  options?: AnalyticsSummaryQuery,
  runtimeGrant?: string,
): Promise<AnalyticsSummary> {
  const params = new URLSearchParams()
  if (options?.days !== undefined) params.set('days', String(options.days))
  if (options?.from) params.set('from', options.from)
  if (options?.to) params.set('to', options.to)
  const query = params.size > 0 ? `?${params}` : ''
  return apiRequest(`/api/tapp/analytics/summary${query}`, { runtimeGrant })
}

/** 需要授予 analytics:read。 */
export async function getAnalyticsVisitorCard(
  runtimeGrant?: string,
): Promise<AnalyticsVisitorCard> {
  return apiRequest('/api/tapp/analytics/visitor', { runtimeGrant })
}
