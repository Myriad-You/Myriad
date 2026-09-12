export type AroUserRole = 'guest' | 'user' | 'admin'

export interface AroRoleState {
  userRole: AroUserRole
  isGuest: boolean
  isAdmin: boolean
}

export interface AroUserContextLike {
  id?: string | null
  username?: string | null
  role?: string | null
  isAdmin?: boolean | null
  authenticated?: boolean | null
}

export interface ResolveAroUserRoleInput {
  /** getRole 的 guest 单独不算定论；getUser 显示成员时继续下探。 */
  roleFromGetRole?: string | null
  /** isAdmin false 表示已登录非管理员，不是访客。 */
  isAdminFromApi?: boolean | null
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
  if (!id && !username) return false
  if (id === 'guest' || id === '0' || id === '-1') return false
  if (/^user_?-\d+$/i.test(id)) return false
  return !!(id || username)
}

export function resolveAroUserRole(
  input: ResolveAroUserRoleInput = {},
): AroRoleState {
  const roleRaw = input.roleFromGetRole
  if (roleRaw != null && String(roleRaw).trim() !== '') {
    const userRole = normalizeRole(String(roleRaw))
    if (userRole !== 'guest') {
      return {
        userRole,
        isGuest: false,
        isAdmin: userRole === 'admin',
      }
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
    const isAdmin = !!(
      user &&
      (user.isAdmin === true ||
        normalizeRole(String(user.role || '')) === 'admin')
    )
    return {
      userRole: isAdmin ? 'admin' : 'user',
      isGuest: false,
      isAdmin,
    }
  }

  return { ...GUEST }
}
