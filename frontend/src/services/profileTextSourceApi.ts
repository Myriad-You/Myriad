import apiService from './api'
import { notifyProfileDisplayChanged } from './profileDisplayEvents'

export type ProfileTextSourceKind = 'auto' | 'account' | 'identity' | 'platform'

export interface ProfileTextSourceItem {
  kind: ProfileTextSourceKind
  ref: string
  label: string
  sublabel: string | null
  preview_name: string | null
  preview_bio: string | null
  is_current: boolean
}

export interface ProfileTextSourcesResponse {
  current: { kind: ProfileTextSourceKind; ref: string | null }
  sources: ProfileTextSourceItem[]
}

interface SetProfileTextSourceResponse {
  kind: ProfileTextSourceKind
  ref: string | null
  name: string | null
  bio: string | null
  platform: string | null
}

export const profileTextSourceApi = {
  async listMine(): Promise<ProfileTextSourcesResponse> {
    return apiService.get<ProfileTextSourcesResponse>(
      '/users/me/profile-text-sources',
    )
  },

  async setMine(
    kind: ProfileTextSourceKind,
    ref?: string | null,
  ): Promise<SetProfileTextSourceResponse> {
    const response = await apiService.put<SetProfileTextSourceResponse>(
      '/users/me/profile-text-source',
      { kind, ref: ref ?? null },
    )
    notifyProfileDisplayChanged()
    return response
  },

  async listForUser(userId: number): Promise<ProfileTextSourcesResponse> {
    return apiService.get<ProfileTextSourcesResponse>(
      `/admin/users/${userId}/profile-text-sources`,
    )
  },

  /** Broadcast only for viewer or site owner. */
  async setForUser(
    userId: number,
    kind: ProfileTextSourceKind,
    ref?: string | null,
    options?: { broadcast?: boolean },
  ): Promise<SetProfileTextSourceResponse> {
    const response = await apiService.put<SetProfileTextSourceResponse>(
      `/admin/users/${userId}/profile-text-source`,
      { kind, ref: ref ?? null },
    )
    if (options?.broadcast) {
      notifyProfileDisplayChanged()
    }
    return response
  },
}

export default profileTextSourceApi
