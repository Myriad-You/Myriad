import apiService from './api'

export interface AdminUserIdentity {
  id: number
  provider: string
  provider_username: string | null
  email: string | null
  avatar_url: string | null
  is_primary: boolean
  linked_at: string | null
  last_login_at: string | null
}

export interface AdminUserTapp {
  tapp_id: string
  name: string
  version: string
  status: string
  icon: string | null
  icon_svg?: string | null
  icon_shell?: boolean | null
  theme_color?: string | null
  installed_at: string | null
  last_run_at: string | null
}

export interface AdminUser {
  id: number
  username: string
  display_name: string | null
  email: string | null
  avatar_url: string | null
  is_admin: boolean
  is_owner: boolean
  auth_provider: string
  local_login_disabled: boolean
  /** Blocks new Tapp installs only. */
  tapp_install_disabled?: boolean
  has_password: boolean
  created_at: string | null
  last_login_at: string | null
  last_seen_at: string | null
  online: boolean
  online_seconds: number
  tapp_count: number
  identities: AdminUserIdentity[]
  tapps?: AdminUserTapp[]
}

export interface AdminUserUpdate {
  is_admin?: boolean
  local_login_disabled?: boolean
  tapp_install_disabled?: boolean
}

export interface AdminCreateUserInput {
  username: string
  password: string
  email?: string
  is_admin?: boolean
}

// API_BASE already includes /api.
const BASE = '/admin/users'

export const adminUsersApi = {
  async list(): Promise<AdminUser[]> {
    const response = await apiService.get<{ users: AdminUser[] }>(BASE)
    return response.users
  },

  async get(userId: number): Promise<AdminUser> {
    const response = await apiService.get<{ user: AdminUser }>(
      `${BASE}/${userId}`,
    )
    return response.user
  },

  async update(
    userId: number,
    update: AdminUserUpdate,
  ): Promise<{ user: AdminUser; notice?: string }> {
    const response = await apiService.patch<{
      user: AdminUser
      notice?: string
      message?: string
    }>(`${BASE}/${userId}`, update)
    return {
      user: response.user,
      notice: response.notice || response.message,
    }
  },

  async create(
    input: AdminCreateUserInput,
  ): Promise<{ notice?: string } | void> {
    const response = await apiService.post<{
      notice?: string
      message?: string
    }>(BASE, input)
    return {
      notice: response?.notice || response?.message,
    }
  },

  async unlinkIdentity(userId: number, identityId: number): Promise<AdminUser> {
    const response = await apiService.delete<{ user: AdminUser }>(
      `${BASE}/${userId}/identities/${identityId}`,
    )
    return response.user
  },

  async uninstallTapp(userId: number, tappId: string): Promise<AdminUser> {
    const response = await apiService.delete<{ user: AdminUser }>(
      `${BASE}/${userId}/tapps/${encodeURIComponent(tappId)}`,
    )
    return response.user
  },

  async delete(userId: number): Promise<{ success: boolean; deleted_user_id: number; username: string }> {
    return apiService.delete<{
      success: boolean
      deleted_user_id: number
      username: string
    }>(`${BASE}/${userId}`)
  },
}

export default adminUsersApi
