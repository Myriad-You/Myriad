/**
 * Pure role resolution for Aro guest-lock.
 *
 * Mirrors the loadUserRole() algorithm in
 * frontend/src/tapp/examples/tapps/aro.ts (PAGE_MOD_HELPERS):
 *   1. Explicit getRole → use it (including 'guest')
 *   2. Else isAdmin API → admin or user (never guest)
 *   3. Else context.getUser → authenticated user → user/admin
 *   4. Only remain guest when no auth user or explicit guest role
 *
 * Extracted so the fallback chain can be unit-tested without the sandbox.
 */

export type AroUserRole = 'guest' | 'user' | 'admin'

export interface AroRoleState {
  userRole: AroUserRole
  isGuest: boolean
  isAdmin: boolean
}

/** Minimal user context shape from Tapp.context.getUser(). */
export interface AroUserContextLike {
  id?: string | null
  username?: string | null
  role?: string | null
  isAdmin?: boolean | null
  authenticated?: boolean | null
}

export interface ResolveAroUserRoleInput {
  /**
   * Result of Tapp.user.getRole() when the call succeeded.
   * Omit / undefined / null / '' = unavailable (fall through).
   */
  roleFromGetRole?: string | null
  /**
   * Result of Tapp.user.isAdmin() when the call succeeded.
   * Omit / undefined / null = unavailable (fall through).
   * Note: false still means "logged-in non-admin", not guest.
   */
  isAdminFromApi?: boolean | null
  /**
   * Result of Tapp.context.getUser() when the call succeeded.
   * Omit / null = unavailable.
   */
  userFromContext?: AroUserContextLike | null
}

const GUEST: AroRoleState = {
  userRole: 'guest',
  isGuest: true,
  isAdmin: false,
}

function normalizeRole(raw: string): AroUserRole {
  const role = String(raw).trim().toLowerCase()
  if (role === 'admin') return 'admin'
  if (role === 'user') return 'user'
  return 'guest'
}

/**
 * True when context user looks like an authenticated (non-guest) account.
 * Explicit role 'guest' always loses; role user/admin or clear identity wins.
 */
export function isAuthenticatedAroUser(
  user: AroUserContextLike | null | undefined,
): boolean {
  if (!user || typeof user !== 'object') return false
  const role =
    user.role != null && String(user.role).trim() !== ''
      ? normalizeRole(String(user.role))
      : null
  if (role === 'guest') return false
  if (role === 'user' || role === 'admin') return true
  if (user.isAdmin === true) return true
  if (user.authenticated === true) return true

  const id = user.id != null ? String(user.id).trim() : ''
  const username = user.username != null ? String(user.username).trim() : ''
  // Guest / anonymous ids look like user_-N or "guest"
  if (!id && !username) return false
  if (id === 'guest' || id === '0' || id === '-1') return false
  if (/^user_?-\d+$/i.test(id)) return false
  return !!(id || username)
}

/**
 * Resolve Aro role flags from available host signals.
 * Callers supply only the signals that actually succeeded; missing
 * signals fall through so a broken getRole does not lock the UI as guest.
 */
export function resolveAroUserRole(
  input: ResolveAroUserRoleInput = {},
): AroRoleState {
  const roleRaw = input.roleFromGetRole
  if (roleRaw != null && String(roleRaw).trim() !== '') {
    const userRole = normalizeRole(String(roleRaw))
    return {
      userRole,
      isGuest: userRole === 'guest',
      isAdmin: userRole === 'admin',
    }
  }

  if (typeof input.isAdminFromApi === 'boolean') {
    return {
      userRole: input.isAdminFromApi ? 'admin' : 'user',
      isGuest: false,
      isAdmin: input.isAdminFromApi,
    }
  }

  const user = input.userFromContext
  if (isAuthenticatedAroUser(user)) {
    const isAdmin =
      !!(user && (user.isAdmin === true || normalizeRole(String(user.role || '')) === 'admin'))
    return {
      userRole: isAdmin ? 'admin' : 'user',
      isGuest: false,
      isAdmin,
    }
  }

  return { ...GUEST }
}
