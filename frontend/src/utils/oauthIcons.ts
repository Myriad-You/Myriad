const OAUTH_ICON_PATHS = {
  google: '/icons/oauth/google.png',
  microsoft: '/icons/oauth/microsoft.png',
  gitlab: '/icons/oauth/gitlab.png',
  discord: '/icons/oauth/discord.png',
  authentik: '/icons/oauth/authentik.png',
  keycloak: '/icons/oauth/keycloak.png',
  auth0: '/icons/oauth/auth0.png',
} as const

const OAUTH_ICON_DOMAINS: Record<string, keyof typeof OAUTH_ICON_PATHS> = {
  'google.com': 'google',
  'accounts.google.com': 'google',
  'microsoft.com': 'microsoft',
  'microsoftonline.com': 'microsoft',
  'login.microsoftonline.com': 'microsoft',
  'gitlab.com': 'gitlab',
  'discord.com': 'discord',
  'goauthentik.io': 'authentik',
  'authentik.io': 'authentik',
  'keycloak.org': 'keycloak',
  'auth0.com': 'auth0',
}

function findKnownIconByDomain(value: string): keyof typeof OAUTH_ICON_PATHS | null {
  const normalized = value.trim().toLowerCase().replaceAll(/^www\./g, '')
  if (!normalized) return null

  for (const [domain, iconId] of Object.entries(OAUTH_ICON_DOMAINS)) {
    if (normalized === domain || normalized.endsWith(`.${domain}`)) {
      return iconId
    }
  }

  return null
}

function findKnownIconId(iconUrl?: string | null): keyof typeof OAUTH_ICON_PATHS | null {
  const trimmed = iconUrl?.trim()
  if (!trimmed) return null

  const localMatch = Object.entries(OAUTH_ICON_PATHS).find(
    ([, path]) => path === trimmed,
  )
  if (localMatch) return localMatch[0] as keyof typeof OAUTH_ICON_PATHS

  const directDomainMatch = findKnownIconByDomain(trimmed)
  if (directDomainMatch) return directDomainMatch

  try {
    const parsed = new URL(trimmed)
    const domainParam = parsed.searchParams.get('domain')
    if (domainParam) {
      const domainMatch = findKnownIconByDomain(domainParam)
      if (domainMatch) return domainMatch
    }

    const hostMatch = findKnownIconByDomain(parsed.hostname)
    if (hostMatch) return hostMatch
  } catch {
    return null
  }

  return null
}

export function getOAuthIconAsset(iconId: keyof typeof OAUTH_ICON_PATHS) {
  return OAUTH_ICON_PATHS[iconId]
}

export function normalizeOAuthIconUrl(iconUrl?: string | null): string | null {
  const trimmed = iconUrl?.trim()
  if (!trimmed) return null

  const iconId = findKnownIconId(trimmed)
  return iconId ? OAUTH_ICON_PATHS[iconId] : trimmed
}

const preloadedOAuthIcons = new Set<string>()

export function preloadOAuthIcons(iconUrls: Array<string | null | undefined>) {
  if (typeof Image === 'undefined') return

  for (const iconUrl of iconUrls) {
    const src = normalizeOAuthIconUrl(iconUrl)
    if (!src || preloadedOAuthIcons.has(src)) continue

    preloadedOAuthIcons.add(src)

    const image = new Image()
    image.decoding = 'async'
    image.src = src

    void image.decode().catch(() => {
      preloadedOAuthIcons.delete(src)
    })
  }
}
