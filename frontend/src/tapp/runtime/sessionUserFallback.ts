/**
 * Session-cookie fallback for tapp user/role when Runtime Grant is dead or
 * /api/tapp/context/user fails. Uses the same /api/auth/me session as the host.
 *
 * Guest contract: /api/auth/me returns HTTP 200 + authenticated:false (not 401).
 */

import { API_URL } from '../../config'
import {
  isAuthMeHttpOk,
  parseAuthMeResponse,
} from '../../utils/authMe'

export type HostUserRole = 'guest' | 'user' | 'admin'

export interface SessionUserSnapshot {
  id: string
  username: string
  display_name?: string | null
  avatar_url?: string | null
  avatar?: string | null
  isAdmin: boolean
  role: HostUserRole
  authenticated: boolean
}

/**
 * Probe the host session via cookie. Returns null when unauthenticated / error.
 * Does not use Runtime Grant — safe after AuthContext.destroyAll().
 */
export async function fetchSessionUserSnapshot(): Promise<SessionUserSnapshot | null> {
  try {
    const response = await fetch(`${API_URL}/api/auth/me`, {
      credentials: 'include',
      signal: AbortSignal.timeout(5000),
    })
    if (!isAuthMeHttpOk(response.status)) return null
    const parsed = parseAuthMeResponse(await response.json())
    if (!parsed.authenticated) return null
    const { user } = parsed
    const isAdmin = user.is_admin === true
    return {
      id: `user_${user.id}`,
      username: user.username,
      display_name: user.display_name ?? null,
      avatar_url: user.avatar_url ?? null,
      avatar: user.avatar_url ?? null,
      isAdmin,
      role: isAdmin ? 'admin' : 'user',
      authenticated: true,
    }
  } catch {
    return null
  }
}

export function roleFromSessionSnapshot(
  snap: SessionUserSnapshot | null,
): HostUserRole {
  if (!snap) return 'guest'
  return snap.role
}
