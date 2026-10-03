import type React from 'react'

export interface ConfigField {
  key: string
  label: string
  field_type: string
  value: string
  placeholder: string
  required: boolean
}

export interface PlatformConfig {
  name: string
  enabled: boolean
  has_token: boolean
  config_fields: ConfigField[]
  description: string
  icon: string
}

export interface AiConfig {
  config_fields: ConfigField[]
}

export interface TripoConfig {
  config_fields: ConfigField[]
}

export interface UiConfig {
  config_fields: ConfigField[]
}

export interface Config {
  platforms: PlatformConfig[]
  auto_fetch: PlatformAutoFetchConfig
  ai_config: AiConfig
  tripo_config: TripoConfig
  ui_config: UiConfig
}

export interface QuickAccessItem {
  id: string
  label: string
  description: string
  icon: React.ReactNode
  section: string
  subsection?: string
  /** 离开 /config 的门，不是管理台房间。 */
  href?: string
}

export interface SaveLibrarySourcePreferencesResponse {
  success: boolean
  preferences?: import('../ModuleConfigSection').LibrarySourcePreferences
  message?: string
}

export type ShowMessage = (
  message: string,
  type?: import('../../Toast').ToastType,
  /** <= 0 declares a sticky toast; otherwise auto-dismisses. */
  duration?: number,
) => void

export interface PlatformAutoFetchConfig {
  enabled: boolean
  interval_hours: number
}

export interface PermissionConfigValues extends Record<
  string,
  boolean | number
> {
  user_perm_ai_generate: boolean
  user_perm_ai_analyze: boolean
  user_perm_ai_chat: boolean
  user_perm_network_fetch: boolean
  user_perm_component_theme: boolean
  user_perm_shortcut_register: boolean
  user_perm_event_publish: boolean
  user_perm_ai_image: boolean
  user_perm_ai_search: boolean
  user_perm_3d_generate: boolean
  user_perm_scheduler_register: boolean
  user_perm_speech_tts: boolean
  user_perm_speech_asr: boolean
  user_perm_storage_write: boolean
  user_perm_federation_post: boolean
  user_perm_federation_channel: boolean
  user_perm_federation_room: boolean
  user_perm_phantasi_comment_write: boolean
  guest_perm_ai_generate: boolean
  guest_perm_ai_analyze: boolean
  guest_perm_ai_chat: boolean
  guest_perm_network_fetch: boolean
  guest_perm_event_publish: boolean
  guest_perm_ai_image: boolean
  guest_perm_ai_search: boolean
  guest_perm_storage_write: boolean
  user_ai_daily_calls: number
  user_ai_daily_tokens: number
  user_ai_cooldown_seconds: number
  guest_ai_daily_calls: number
  guest_ai_daily_tokens: number
  guest_ai_cooldown_seconds: number
}
