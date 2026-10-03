import type { ConfigField } from './types'

export const AGENT_AI_FIELD_KEYS = new Set([
  'qq_bot_enabled',
  'qq_bot_app_id',
  'qq_bot_app_secret',
  'telegram_bot_enabled',
  'telegram_bot_token',
  'discord_bot_enabled',
  'discord_bot_token',
  'feishu_bot_enabled',
  'feishu_bot_app_id',
  'feishu_bot_app_secret',
  'onebot_bot_enabled',
  'onebot_bot_groups_enabled',
  'onebot_bot_group_ids',
  'onebot_bot_ws_url',
  'onebot_bot_access_token',
])

// Bag keys from api/config/build.rs, with factory values from DynamicConfig::default.
// Credentials and optional source/model overrides reset to an empty value.
const AI_FIELD_DEFAULTS: Readonly<Record<string, string>> = {
  ai_model: 'minimax/minimax-m3',
  pro_enabled: 'false',
  pro_ai_model: 'anthropic/claude-opus-5.5',
  ai_image_model: 'openai/gpt-image-2.5-sunburst',
  ai_vendor_sources: '[]',
  qq_bot_enabled: 'false',
  telegram_bot_enabled: 'false',
  discord_bot_enabled: 'false',
  feishu_bot_enabled: 'false',
  onebot_bot_enabled: 'false',
  onebot_bot_groups_enabled: 'false',
}

export function defaultAiFieldValue(key: string): string {
  return Object.hasOwn(AI_FIELD_DEFAULTS, key) ? AI_FIELD_DEFAULTS[key] : ''
}

export function defaultTripoFieldValue(key: string): string {
  if (key === 'tripo_enabled') return 'false'
  if (key === 'tripo_base_url') return 'https://openapi.tripo3d.ai/v3'
  if (key === 'tripo_model') return 'P1-20260311'
  if (key === 'tripo_face_limit') return '5000'
  if (key === 'tripo_poll_interval_seconds') return '2'
  if (key === 'tripo_task_timeout_seconds') return '900'
  if (key === 'tripo_max_download_mb') return '64'
  return ''
}

export function defaultUiFieldValue(key: string): string {
  if (key === 'wallpaper_url') {
    return ''
  }
  if (key === 'wallpaper_blur') return '3'
  if (key === 'evocative_parallax') return 'true'
  if (key === 'evocative_dynamic_blur') return 'false'
  if (key === 'evocative_ripple') return 'false'
  if (key === 'evocative_fps') return '30'
  if (key === 'evocative_ripple_quality') return '0.85'
  if (key === 'music_enabled') return 'false'
  if (key === 'analytics_enabled') return 'true'
  if (key === 'pwa_enabled') return 'true'
  if (key === 'site_noindex') return 'false'
  if (key === 'site_visibility_policy') return 'ai_full'
  if (key === 'site_keywords') return ''
  if (key === 'site_og_image') return ''
  if (key === 'google_site_verification') return ''
  if (key === 'site_ai_intro') return ''
  if (key === 'site_seo_review_cadence') return 'off'
  if (key === 'ga_measurement_id') return ''
  if (key === 'umami_website_id') return ''
  if (key === 'umami_script_url') return ''
  if (key === 'site_footer_custom') return ''
  if (key === 'music_source') return 'netease'
  if (key === 'music_playlist_id') return ''
  if (key === 'music_proxy_enabled') return 'true'
  if (key === 'music_preload_enabled') return 'true'
  if (key === 'proxy_enabled') return 'false'
  if (key === 'memory_saver_enabled') return 'false'
  if (key === 'precise_location_enabled') return 'false'
  if (key === 'merope_enabled') return 'false'
  if (key === 'merope_speech_enabled') return 'false'
  if (key === 'merope_voice_mode') return 'tts'
  if (key === 'proxy_url') return ''
  if (key === 'proxy_bypass') return ''
  if (key === 'gemini_base_url') return ''
  if (key === 'github_api_base_url') return ''
  return ''
}

export function mapConfigFields(
  fields: ConfigField[],
  getDefault: (key: string) => string,
  onlyKeys?: Set<string>,
): ConfigField[] {
  return fields.map((field) => {
    if (onlyKeys && !onlyKeys.has(field.key)) return field
    return { ...field, value: getDefault(field.key) }
  })
}
