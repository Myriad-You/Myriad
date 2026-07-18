import { API_URL } from '../config'
import { fetchJson } from './apiHelper'
import { clearCSRFToken, getCSRFToken } from './csrf'
import { normalizeOAuthIconUrl } from './oauthIcons'

export interface OAuthProviderEntry {
  slug: string
  kind: 'github' | 'oidc'
  display_name: string
  enabled: boolean
  client_id: string
  client_secret: string
  scopes: string[]
  discovery_url?: string | null
  icon_url?: string | null
}

export interface OAuthSettings {
  providers: OAuthProviderEntry[]
  allowLocalRegistration: boolean
}

export const DEFAULT_OAUTH_SETTINGS: OAuthSettings = {
  providers: [],
  allowLocalRegistration: false,
}

function normalizeProviderEntry(
  provider: OAuthProviderEntry,
): OAuthProviderEntry {
  return {
    ...provider,
    discovery_url: provider.discovery_url || null,
    icon_url: normalizeOAuthIconUrl(provider.icon_url),
    scopes: Array.isArray(provider.scopes)
      ? provider.scopes.filter(Boolean)
      : [],
  }
}

function normalizeOAuthSettings(data: any): OAuthSettings {
  return {
    providers: Array.isArray(data?.providers)
      ? data.providers.map(normalizeProviderEntry)
      : [],
    allowLocalRegistration: Boolean(data?.allow_local_registration),
  }
}

export function cloneOAuthSettings(settings: OAuthSettings): OAuthSettings {
  return {
    ...settings,
    providers: settings.providers.map((provider) => ({
      ...provider,
      scopes: [...provider.scopes],
    })),
  }
}

export function areOAuthSettingsEqual(
  left: OAuthSettings,
  right: OAuthSettings,
): boolean {
  return JSON.stringify(left) === JSON.stringify(right)
}

export async function fetchOAuthSettings(): Promise<OAuthSettings> {
  const data = await fetchJson(`${API_URL}/api/config/oauth-providers`, {
    credentials: 'include',
  })
  return normalizeOAuthSettings(data)
}

export async function updateOAuthSettings(
  settings: OAuthSettings,
): Promise<OAuthSettings> {
  const body = JSON.stringify({
    providers: settings.providers.map(normalizeProviderEntry),
    allow_local_registration: settings.allowLocalRegistration,
  })

  const putOnce = async (forceCsrfRefresh: boolean) => {
    if (forceCsrfRefresh) clearCSRFToken()
    const csrfToken = await getCSRFToken(forceCsrfRefresh)
    const headers: Record<string, string> = {
      'Content-Type': 'application/json',
    }
    if (csrfToken) headers['X-CSRF-Token'] = csrfToken

    await fetchJson(`${API_URL}/api/config/oauth-providers`, {
      method: 'PUT',
      credentials: 'include',
      headers,
      body,
    })
  }

  try {
    await putOnce(false)
  } catch (error) {
    // Align with brewApi / lib/api: one CSRF refresh + retry on 403 CSRF failures.
    const message =
      error instanceof Error ? error.message.toLowerCase() : String(error)
    if (message.includes('csrf')) {
      console.warn(
        '[oauthSettings] CSRF rejection — refreshing token and retrying once',
      )
      await putOnce(true)
    } else {
      throw error
    }
  }

  // 后端会规范化 slug 并重新掩码 secret，保存后以服务端快照为准。
  return fetchOAuthSettings()
}
