/**
 * 常见 OAuth provider 预设
 *
 * 每个预设填好 discovery_url / scopes / icon / 注册文档链接，
 * 用户只需补 client_id / client_secret 即可上线。
 *
 * GitHub 用 kind="github" 走 GithubProvider（不需要 OIDC discovery）；
 * 其它都是标准 OIDC。
 *
 * 配置步骤文案见 oauthSetupGuides.ts + i18n（oauthSetup*）。
 */

import type { OAuthProviderEntry } from '../../utils/oauthSettings'
import { getOAuthIconAsset } from '../../utils/oauthIcons'

export interface OAuthPreset {
  /** 预设 ID，仅用于前端 UI 选择 */
  id: string
  /** 创建出的 entry slug 默认值（可被用户改） */
  defaultSlug: string
  /** "github" | "oidc" */
  kind: 'github' | 'oidc'
  /** UI 上的名字 */
  display_name: string
  /** 已知 discovery URL（OIDC） */
  discovery_url?: string
  /** 默认 scopes（OIDC 必须包含 openid） */
  scopes: string[]
  /** SVG icon URL；GitHub 留空，前端识别 slug="github" 后用内置 FaGithub */
  icon_url?: string
  /** 提供方申请 OAuth 应用的文档链接（SetupFlow「打开」用） */
  docs_url?: string
}

export const OAUTH_PRESETS: OAuthPreset[] = [
  {
    id: 'github',
    defaultSlug: 'github',
    kind: 'github',
    display_name: 'GitHub',
    scopes: ['read:user', 'user:email'],
    icon_url: undefined,
    docs_url: 'https://github.com/settings/developers',
  },
  {
    id: 'google',
    defaultSlug: 'google',
    kind: 'oidc',
    display_name: 'Google',
    discovery_url:
      'https://accounts.google.com/.well-known/openid-configuration',
    scopes: ['openid', 'email', 'profile'],
    icon_url: getOAuthIconAsset('google'),
    docs_url: 'https://console.cloud.google.com/apis/credentials',
  },
  {
    id: 'microsoft',
    defaultSlug: 'microsoft',
    kind: 'oidc',
    display_name: 'Microsoft',
    discovery_url:
      'https://login.microsoftonline.com/common/v2.0/.well-known/openid-configuration',
    scopes: ['openid', 'email', 'profile', 'User.Read'],
    icon_url: getOAuthIconAsset('microsoft'),
    docs_url:
      'https://portal.azure.com/#blade/Microsoft_AAD_RegisteredApps/ApplicationsListBlade',
  },
  {
    id: 'gitlab',
    defaultSlug: 'gitlab',
    kind: 'oidc',
    display_name: 'GitLab',
    discovery_url: 'https://gitlab.com/.well-known/openid-configuration',
    scopes: ['openid', 'email', 'profile'],
    icon_url: getOAuthIconAsset('gitlab'),
    docs_url: 'https://gitlab.com/-/user_settings/applications',
  },
  {
    id: 'discord',
    defaultSlug: 'discord',
    kind: 'oidc',
    display_name: 'Discord',
    discovery_url: 'https://discord.com/.well-known/openid-configuration',
    scopes: ['openid', 'email', 'identify'],
    icon_url: getOAuthIconAsset('discord'),
    docs_url: 'https://discord.com/developers/applications',
  },
  {
    id: 'authentik',
    defaultSlug: 'authentik',
    kind: 'oidc',
    display_name: 'Authentik',
    discovery_url: '',
    scopes: ['openid', 'email', 'profile'],
    icon_url: getOAuthIconAsset('authentik'),
    docs_url: 'https://goauthentik.io/docs/providers/oauth2',
  },
  {
    id: 'keycloak',
    defaultSlug: 'keycloak',
    kind: 'oidc',
    display_name: 'Keycloak',
    discovery_url: '',
    scopes: ['openid', 'email', 'profile'],
    icon_url: getOAuthIconAsset('keycloak'),
    docs_url: 'https://www.keycloak.org/docs/latest/server_admin/index.html',
  },
  {
    id: 'auth0',
    defaultSlug: 'auth0',
    kind: 'oidc',
    display_name: 'Auth0',
    discovery_url: '',
    scopes: ['openid', 'email', 'profile'],
    icon_url: getOAuthIconAsset('auth0'),
    docs_url: 'https://auth0.com/docs/get-started/applications',
  },
  {
    id: 'custom',
    defaultSlug: '',
    kind: 'oidc',
    display_name: 'Custom OIDC',
    discovery_url: '',
    scopes: ['openid', 'email', 'profile'],
    icon_url: undefined,
    docs_url: undefined,
  },
]

export function findPreset(id: string): OAuthPreset | undefined {
  return OAUTH_PRESETS.find((p) => p.id === id)
}

export function hasOAuthCredential(
  entry: Pick<OAuthProviderEntry, 'client_id' | 'client_secret'>,
): boolean {
  return Boolean(entry.client_id?.trim() && entry.client_secret?.trim())
}

function uniqueOAuthSlug(
  base: string,
  existing: Array<Pick<OAuthProviderEntry, 'slug'>>,
): string {
  const seed = base.trim() || 'oidc'
  if (!existing.some((item) => item.slug === seed)) return seed
  let index = 2
  while (existing.some((item) => item.slug === `${seed}-${index}`)) {
    index += 1
  }
  return `${seed}-${index}`
}

export function entryFromPreset(
  preset: OAuthPreset,
  existing: Array<Pick<OAuthProviderEntry, 'slug'>>,
): OAuthProviderEntry {
  const seed = preset.defaultSlug || preset.id
  const slug = uniqueOAuthSlug(seed, existing)
  const copy =
    slug === seed ? '' : slug.slice(seed.length).replace(/^-+/, '')
  return {
    slug,
    kind: preset.kind,
    display_name: copy
      ? `${preset.display_name} ${copy}`
      : preset.display_name,
    enabled: true,
    client_id: '',
    client_secret: '',
    scopes: [...preset.scopes],
    discovery_url: preset.discovery_url || '',
    icon_url: preset.icon_url || null,
  }
}
