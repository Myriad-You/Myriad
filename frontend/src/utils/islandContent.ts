/**
 * 智能岛收缩态轮播：站长可选择显示哪些内置/Tapp 内容。
 * 通知进岛走通知偏好，不走这一组。
 */

export const ISLAND_CONTENT_KEYS = [
  'greeting',
  'weather',
  'quote',
  'music',
  'tapp',
] as const

export type IslandContentKey = (typeof ISLAND_CONTENT_KEYS)[number]

export type IslandContentPreferences = Record<IslandContentKey, boolean>

export const DEFAULT_ISLAND_CONTENT: IslandContentPreferences = {
  greeting: true,
  weather: true,
  quote: true,
  music: true,
  tapp: true,
}

export const ISLAND_UI_BAG_KEYS = [
  'island_show_greeting',
  'island_show_weather',
  'island_show_quote',
  'island_show_music',
  'island_show_tapp',
] as const

export type IslandUiBagKey = (typeof ISLAND_UI_BAG_KEYS)[number]

export function islandBagKey(key: IslandContentKey): IslandUiBagKey {
  return `island_show_${key}`
}

/** 缺省 / 空 = 开。只有显式 false/0/"false"/"0" 才关。 */
export function parseIslandFlag(value: unknown): boolean {
  if (value === false || value === 0) return false
  if (typeof value === 'string') {
    const trimmed = value.trim().toLowerCase()
    if (trimmed === 'false' || trimmed === '0') return false
  }
  return true
}

export function islandContentFromPublicUi(
  cfg: Record<string, unknown> | null | undefined,
): IslandContentPreferences {
  return {
    greeting: parseIslandFlag(cfg?.island_show_greeting),
    weather: parseIslandFlag(cfg?.island_show_weather),
    quote: parseIslandFlag(cfg?.island_show_quote),
    music: parseIslandFlag(cfg?.island_show_music),
    tapp: parseIslandFlag(cfg?.island_show_tapp),
  }
}

export function islandContentFromBagFields(
  fields: Array<{ key: string; value: string }> | undefined,
): IslandContentPreferences {
  const map = Object.fromEntries(
    (fields ?? []).map((field) => [field.key, field.value]),
  )
  return islandContentFromPublicUi(map)
}

export function islandBagValues(
  prefs: IslandContentPreferences,
): Record<IslandUiBagKey, string> {
  return {
    island_show_greeting: String(prefs.greeting),
    island_show_weather: String(prefs.weather),
    island_show_quote: String(prefs.quote),
    island_show_music: String(prefs.music),
    island_show_tapp: String(prefs.tapp),
  }
}

export function allowsIslandType(
  prefs: IslandContentPreferences,
  type: string,
): boolean {
  if (type === 'notification') return true
  if (type.startsWith('tapp-')) return prefs.tapp
  if (
    type === 'greeting' ||
    type === 'weather' ||
    type === 'quote' ||
    type === 'music'
  ) {
    return prefs[type]
  }
  return true
}

export const ISLAND_CONTENT_CHANGED_EVENT = 'islandContentChanged'
