import type { PermissionConfigValues } from '../PermissionsConfigSection'
import type { PlatformAutoFetchConfig } from '../PlatformAutoRefreshSettings'

export const DEFAULT_PERMISSION_CONFIG: PermissionConfigValues = {
  user_perm_ai_generate: false,
  user_perm_ai_analyze: false,
  user_perm_ai_chat: false,
  user_perm_report_write: false,
  user_perm_network_fetch: false,
  // component:theme 与 shortcut:register 对普通用户默认下放（游客仍关闭）
  user_perm_component_theme: true,
  user_perm_shortcut_register: true,
  user_perm_event_publish: false,
  user_perm_ai_image: false,
  user_perm_ai_search: false,
  user_perm_3d_generate: false,
  user_perm_scheduler_register: false,
  user_perm_speech_tts: false,
  user_perm_speech_asr: false,
  user_perm_storage_write: false,
  user_perm_federation_post: false,
  user_perm_federation_channel: false,
  user_perm_federation_room: false,
  user_perm_brew_comment_write: false,
  guest_perm_ai_generate: false,
  guest_perm_ai_analyze: false,
  guest_perm_ai_chat: false,
  guest_perm_report_write: false,
  guest_perm_network_fetch: false,
  guest_perm_component_theme: false,
  guest_perm_shortcut_register: false,
  guest_perm_event_publish: false,
  guest_perm_ai_image: false,
  guest_perm_ai_search: false,
  guest_perm_3d_generate: false,
  guest_perm_scheduler_register: false,
  guest_perm_speech_tts: false,
  guest_perm_speech_asr: false,
  guest_perm_storage_write: false,
  guest_perm_federation_post: false,
  guest_perm_federation_channel: false,
  guest_perm_federation_room: false,
  guest_perm_brew_comment_write: false,
  user_ai_daily_calls: 50,
  user_ai_daily_tokens: 20000,
  user_ai_cooldown_seconds: 5,
  guest_ai_daily_calls: 10,
  guest_ai_daily_tokens: 5000,
  guest_ai_cooldown_seconds: 10,
}

export const DEFAULT_CONFIG_FAVORITES = ['platforms', 'ai']

export const DEFAULT_AUTO_FETCH_CONFIG: PlatformAutoFetchConfig = {
  enabled: false,
  interval_hours: 24,
}

/** 收藏/深链/搜索里的旧导航 id → 现行 section。 */
export const LEGACY_CONFIG_SECTION_MAP: Record<string, string> = {
  music: 'modules',
  data: 'platforms',
  network: 'advanced',
  updater: 'about',
  mcp: 'advanced',
  ui: 'basic',
}

export function loadConfigFavorites(): string[] {
  if (typeof window === 'undefined') return DEFAULT_CONFIG_FAVORITES
  try {
    const saved = localStorage.getItem('config_favorites')
    if (!saved) return DEFAULT_CONFIG_FAVORITES
    const parsed: unknown = JSON.parse(saved)
    if (
      !Array.isArray(parsed) ||
      !parsed.every((item) => typeof item === 'string')
    ) {
      return DEFAULT_CONFIG_FAVORITES
    }
    return Iterator.from(
      new Set(
        (parsed as string[]).map((id) => LEGACY_CONFIG_SECTION_MAP[id] ?? id),
      ),
    ).toArray()
  } catch {
    return DEFAULT_CONFIG_FAVORITES
  }
}
