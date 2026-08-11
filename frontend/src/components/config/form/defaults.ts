import type { PermissionConfigValues } from '../PermissionsConfigSection'
import type { PlatformAutoFetchConfig } from '../PlatformAutoRefreshSettings'

export const DEFAULT_PERMISSION_CONFIG: PermissionConfigValues = {
  user_perm_ai_generate: false,
  user_perm_ai_analyze: false,
  user_perm_ai_chat: false,
  user_perm_report_write: false,
  user_perm_network_fetch: false,
  user_perm_component_theme: true,
  user_perm_shortcut_register: true,
  user_perm_event_publish: true,
  user_perm_ai_image: false,
  user_perm_scheduler_register: true,
  user_perm_speech_tts: true,
  user_perm_speech_asr: true,
  guest_perm_ai_generate: false,
  guest_perm_ai_analyze: false,
  guest_perm_ai_chat: false,
  guest_perm_report_write: false,
  guest_perm_network_fetch: false,
  guest_perm_component_theme: false,
  guest_perm_shortcut_register: false,
  guest_perm_event_publish: false,
  guest_perm_ai_image: false,
  guest_perm_scheduler_register: false,
  guest_perm_speech_tts: false,
  guest_perm_speech_asr: false,
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

/** Retired config nav ids remapped when restoring favorites / deep links / search. */
export const LEGACY_CONFIG_SECTION_MAP: Record<string, string> = {
  music: 'modules',
  /** Standalone data-management page removed; alias lands on platforms list. */
  data: 'platforms',
  network: 'advanced',
  /** Updater panel lives under About (no standalone nav section). */
  updater: 'about',
  /** MCP ops panel lives under Advanced (was briefly under About). */
  mcp: 'advanced',
  /** Basic settings nav id used to be `ui` (display copy was already “basic”). */
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
    return [
      ...new Set(
        (parsed as string[]).map((id) => LEGACY_CONFIG_SECTION_MAP[id] ?? id),
      ),
    ]
  } catch {
    return DEFAULT_CONFIG_FAVORITES
  }
}
