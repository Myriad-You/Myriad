export interface AuthMeUser {
  id: number
  username: string
  display_name?: string
  is_admin: boolean
  is_owner?: boolean
  auth_provider?: string
  linked_github_id?: string
  github_id?: number
  avatar_url?: string
  bio?: string
  has_password?: boolean
  last_login_at?: string | null
  locale?: import('../i18n').Locale | null
  identities?: Array<{
    id?: number
    provider?: string
    provider_username?: string | null
    is_primary?: boolean
    linked_at?: string | null
  }>
  authenticated: true
  [key: string]: unknown
}

export type AuthMeResult =
  | { authenticated: false }
  | { authenticated: true; user: AuthMeUser }

export function parseAuthMeResponse(data: unknown): AuthMeResult {
  if (!data || typeof data !== 'object') {
    return { authenticated: false }
  }
  const body = data as Record<string, unknown>
  if (body.authenticated === false) {
    return { authenticated: false }
  }

  const rawId = body.id
  const numericId =
    typeof rawId === 'number'
      ? rawId
      : typeof rawId === 'string'
        ? Number.parseInt(rawId, 10)
        : NaN
  if (!Number.isFinite(numericId) || numericId <= 0) {
    return { authenticated: false }
  }

  const username =
    typeof body.username === 'string' ? body.username.trim() : ''
  if (!username) {
    return { authenticated: false }
  }

  return {
    authenticated: true,
    user: {
      ...(body as AuthMeUser),
      id: numericId,
      username,
      is_admin: body.is_admin === true,
      is_owner: body.is_owner === true,
      authenticated: true,
    },
  }
}

export function isAuthMeHttpOk(status: number): boolean {
  return status >= 200 && status < 300
}
