/**
 * 设置页「用户管理」模块的 admin API 客户端。
 *
 * 对应后端 backend/src/api/admin_users.rs：
 * - GET    /api/admin/users
 * - POST   /api/admin/users （创建本地用户，auth_local.rs）
 * - GET    /api/admin/users/{id}
 * - PATCH  /api/admin/users/{id}
 * - DELETE /api/admin/users/{id}
 * - DELETE /api/admin/users/{id}/identities/{identity_id}
 */

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
  /** Durable site owner flag (was: heuristic id === 1). */
  is_owner: boolean
  auth_provider: string
  local_login_disabled: boolean
  has_password: boolean
  created_at: string | null
  last_login_at: string | null
  last_seen_at: string | null
  online: boolean
  online_seconds: number
  tapp_count: number
  identities: AdminUserIdentity[]
  /** 仅详情接口返回 */
  tapps?: AdminUserTapp[]
}

export interface AdminUserUpdate {
  is_admin?: boolean
  local_login_disabled?: boolean
}

export interface AdminCreateUserInput {
  username: string
  password: string
  email?: string
  is_admin?: boolean
}

// apiService 的 API_BASE 已含 /api 前缀，这里不能再写 /api
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

  async delete(userId: number): Promise<{ success: boolean; deleted_user_id: number; username: string }> {
    return apiService.delete<{
      success: boolean
      deleted_user_id: number
      username: string
    }>(`${BASE}/${userId}`)
  },
}

export default adminUsersApi
