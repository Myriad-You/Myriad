import type { TappManifest, TappManifestLocales } from '../types'
import { apiRequest } from './TappHttpClient'

export interface TappListItem {
  id: string
  name: string
  version: string
  description?: string
  icon?: string
  iconSvg?: string
  themeColor?: string
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
  /** Approved install permissions; returned to admins only. */
  approved_permissions?: string[]
  /** Hosts the sandbox may load remote media from (declared ∩ approved, needs media:remote). */
  granted_remote_media?: string[]
  /** Approved remoteMedia hosts; returned to admins only. */
  approved_remote_media?: string[]
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
  signal?: AbortSignal,
): Promise<TappDetail[]> {
  return apiRequest(`/api/tapps/details${catalogScopeQuery(scope)}`, { signal })
}

export async function getRecentTapps(
  limit: number = 10,
): Promise<RecentTappItem[]> {
  return apiRequest(`/api/tapps/recent?limit=${limit}`)
}

export async function getTapp(tappId: string, signal?: AbortSignal): Promise<TappDetail> {
  return apiRequest(`/api/tapps/${encodeURIComponent(tappId)}`, { signal })
}

export async function startTapp(tappId: string, signal?: AbortSignal): Promise<void> {
  return apiRequest(`/api/tapps/${encodeURIComponent(tappId)}/start`, {
    method: 'POST',
    signal,
  })
}

export async function stopTapp(tappId: string, signal?: AbortSignal): Promise<void> {
  return apiRequest(`/api/tapps/${encodeURIComponent(tappId)}/stop`, {
    method: 'POST',
    signal,
  })
}

/** 批准 remoteMedia 域名（与已装 manifest 声明求交集）。管理员批公开安装，用户批自己的私装。 */
export async function setTappRemoteMedia(
  tappId: string,
  hosts: string[],
): Promise<{ id: string; approvedRemoteMedia: string[] }> {
  return apiRequest(
    `/api/tapps/${encodeURIComponent(tappId)}/remote-media`,
    {
      method: 'PUT',
      body: JSON.stringify({ hosts }),
    },
  )
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
