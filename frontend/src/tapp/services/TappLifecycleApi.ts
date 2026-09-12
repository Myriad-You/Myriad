import type { TappManifest, TappManifestLocales } from '../types'
import { apiRequest } from './TappHttpClient'

export interface TappListItem {
  id: string
  name: string
  version: string
  description?: string
  icon?: string
  iconSvg?: string
  locales?: TappManifestLocales
  status: string
  errorMessage?: string
  installedAt: string
  lastRunAt?: string
  isTemporary?: boolean
  isAdminTapp?: boolean
  /** 公开安装可见性。私有安装忽略此项。 */
  visibility?: TappVisibility
  needsReauthorization?: boolean
}

export type TappVisibility = 'all' | 'admin'

export interface TappDetail {
  id: string
  name: string
  version: string
  description?: string
  author?: {
    name: string
    email?: string
    url?: string
  }
  icon?: string
  theme_color?: string
  manifest: TappManifest
  status: string
  error_message?: string
  granted_permissions: string[]
  needs_reauthorization?: boolean
  installed_at: string
  last_run_at?: string
  user_role?: string
  is_temporary?: boolean
  is_admin_tapp?: boolean
  /** 公开安装可见性。私有安装忽略此项。 */
  visibility?: TappVisibility
}

export interface RecentTappItem {
  id: string
  name: string
  icon?: string
  iconSvg?: string
  themeColor?: string
  locales?: TappManifestLocales
  lastRunAt: string
  runCount: number
}

export type TappCatalogScope = 'all' | 'mine' | 'site'

function catalogScopeQuery(scope?: TappCatalogScope): string {
  if (!scope || scope === 'all') return ''
  return `?scope=${encodeURIComponent(scope)}`
}

export async function listTapps(
  scope?: TappCatalogScope,
): Promise<TappListItem[]> {
  return apiRequest(`/api/tapps${catalogScopeQuery(scope)}`)
}

export async function listTappDetails(
  scope?: TappCatalogScope,
): Promise<TappDetail[]> {
  return apiRequest(`/api/tapps/details${catalogScopeQuery(scope)}`)
}

export async function getRecentTapps(
  limit: number = 10,
): Promise<RecentTappItem[]> {
  return apiRequest(`/api/tapps/recent?limit=${limit}`)
}

export async function getTapp(tappId: string): Promise<TappDetail> {
  return apiRequest(`/api/tapps/${encodeURIComponent(tappId)}`)
}

export async function startTapp(tappId: string): Promise<void> {
  return apiRequest(`/api/tapps/${encodeURIComponent(tappId)}/start`, {
    method: 'POST',
  })
}

export async function stopTapp(tappId: string): Promise<void> {
  return apiRequest(`/api/tapps/${encodeURIComponent(tappId)}/stop`, {
    method: 'POST',
  })
}

export async function setTappVisibility(
  tappId: string,
  visibility: TappVisibility,
): Promise<{ id: string; visibility: TappVisibility }> {
  return apiRequest(
    `/api/tapps/${encodeURIComponent(tappId)}/visibility`,
    {
      method: 'POST',
      body: JSON.stringify({ visibility }),
    },
  )
}
