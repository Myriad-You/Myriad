/**
 * Shared parsing for GET /api/auth/me session probe.
 *
 * Durable contract (backend): always HTTP 200 for guest/expired/invalid
 * session → `{ authenticated: false }`. Authenticated → `{ authenticated: true, id, ... }`.
 * Never rely on HTTP 401 for "is guest".
 */

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
  /** Linked OAuth/OIDC rows when server includes them */
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

/**
 * Parse /api/auth/me JSON after a successful (2xx) response.
 * Treats missing id / explicit authenticated:false as guest.
 */
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

  // authenticated:true preferred; legacy servers may omit it when id is present.
  return {
    authenticated: true,
    user: {
      ...(body as AuthMeUser),
      id: numericId,
      username,
      is_admin: body.is_admin === true,
      authenticated: true,
    },
  }
}

/** True when the HTTP status is a successful probe response (including guest 200). */
export function isAuthMeHttpOk(status: number): boolean {
  return status >= 200 && status < 300
}
