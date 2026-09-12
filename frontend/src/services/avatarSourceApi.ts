/** Must broadcast notifyAvatarChanged() or other avatars stay stale. */

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
  | 'persona'

export interface AvatarSourceItem {
  kind: AvatarSourceKind
  /** Empty for account/auto. */
  ref: string
  label: string
  sublabel: string | null
  avatar_url: string | null
  /** True if either merged source is selected; do not compare only current.kind/ref. */
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

  /** Broadcast only for viewer or site owner. */
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
