/**
 * 画像源（头像来源）API 客户端。
 *
 * 对应后端 backend/src/api/avatar_source.rs：
 * - GET /api/users/me/avatar-sources        本人可选来源 + 当前选择
 * - PUT /api/users/me/avatar-source         本人切换
 * - GET /api/admin/users/{id}/avatar-sources 管理员查看他人来源
 * - PUT /api/admin/users/{id}/avatar-source  管理员替他人切换
 *
 * 切换成功后必须广播 `notifyAvatarChanged()`（同时触发 profile-display-changed），
 * 否则同页其它头像位置（控制面板、首页信息条）会停在旧图直到刷新。
 *
 * 名称/简介来源见 `profileTextSourceApi` —— 与画像源独立存储，互不覆盖。
 */

import apiService from './api'
import {
  notifyAvatarChanged,
  onAvatarChanged,
} from './profileDisplayEvents'

export type AvatarSourceKind =
  | 'auto'
  | 'account'
  | 'identity'
  | 'platform'
  /** 站点人设的 Q 版贴纸头像。全站一份，谁选谁用同一张。 */
  | 'persona'

export interface AvatarSourceItem {
  kind: AvatarSourceKind
  /** identity id 或平台名；account/auto 为空串 */
  ref: string
  /**
   * account → 'account'；identity → provider slug；platform → 'Bilibili' 等；
   * persona → 'persona'（与 account 一样由前端本地化）
   */
  label: string
  /** provider 用户名 / 平台昵称（同站合并后去重）；persona 为人设对外名字 */
  sublabel: string | null
  avatar_url: string | null
  /**
   * 是否为当前选中源。同站合并（OAuth + 平台抓取）时，若底层 identity 或
   * platform 任一已选中则为 true；前端应用此字段标亮，勿只比 current.kind/ref。
   */
  is_current: boolean
}

export interface AvatarSourcesResponse {
  current: { kind: AvatarSourceKind; ref: string | null }
  sources: AvatarSourceItem[]
}

interface SetAvatarSourceResponse {
  kind: AvatarSourceKind
  ref: string | null
  avatar_url: string | null
}

// Re-export notify/on for existing importers (Home, UserSection, …)
export { notifyAvatarChanged, onAvatarChanged }
export {
  notifyProfileDisplayChanged,
  onProfileDisplayChanged,
} from './profileDisplayEvents'

export const avatarSourceApi = {
  async listMine(): Promise<AvatarSourcesResponse> {
    return apiService.get<AvatarSourcesResponse>('/users/me/avatar-sources')
  },

  async setMine(
    kind: AvatarSourceKind,
    ref?: string | null,
  ): Promise<SetAvatarSourceResponse> {
    const response = await apiService.put<SetAvatarSourceResponse>(
      '/users/me/avatar-source',
      { kind, ref: ref ?? null },
    )
    notifyAvatarChanged()
    return response
  },

  async listForUser(userId: number): Promise<AvatarSourcesResponse> {
    return apiService.get<AvatarSourcesResponse>(
      `/admin/users/${userId}/avatar-sources`,
    )
  },

  /**
   * Admin: set avatar source for another user.
   * Only broadcast global refresh when the target is the viewer or site owner —
   * editing a random user must not flash the homepage owner card.
   */
  async setForUser(
    userId: number,
    kind: AvatarSourceKind,
    ref?: string | null,
    options?: { broadcast?: boolean },
  ): Promise<SetAvatarSourceResponse> {
    const response = await apiService.put<SetAvatarSourceResponse>(
      `/admin/users/${userId}/avatar-source`,
      { kind, ref: ref ?? null },
    )
    if (options?.broadcast) {
      notifyAvatarChanged()
    }
    return response
  },
}

export default avatarSourceApi
