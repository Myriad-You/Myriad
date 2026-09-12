/** Runtime Grant 失效时用会话 cookie。访客：/api/auth/me 为 200 + authenticated:false，不是 401。 */

import { API_URL } from '../../config'
import { hostLocaleHeaders } from '../../i18n/hostLocaleHeaders'
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

/** 用 cookie 探宿主会话。不用 Runtime Grant；destroyAll 后仍安全。 */
export async function fetchSessionUserSnapshot(): Promise<SessionUserSnapshot | null> {
  try {
    const response = await fetch(`${API_URL}/api/auth/me`, {
      credentials: 'include',
      headers: hostLocaleHeaders(),
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
