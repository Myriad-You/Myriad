import type { Locale, TranslationKeys } from './index'
import { enUS } from './en-US'
import { getDefaultLocale } from './index'
import { getCachedLocale, loadLocale } from './loadLocale'

function asLocale(value: string): Locale {
  if (value === 'zh-CN' || value === 'ja-JP') return value
  return 'en-US'
}

/**
 * Copy for an explicitly selected UI language.
 *
 * ja / zh stay out of the static graph. The first call kicks off `loadLocale`;
 * until that chunk arrives, English is the sync fallback so service-layer
 * errors never throw. After I18nProvider (or a test) awaits the same locale,
 * later calls hit the cache.
 */
export function copyForLocale(locale: string): TranslationKeys {
  const key = asLocale(locale)
  const cached = getCachedLocale(key)
  if (cached) return cached
  void loadLocale(key)
  return getCachedLocale(key) ?? getCachedLocale('en-US') ?? enUS
}

/** Current UI language copy for service-layer errors (outside React). */
export function currentCopy(): TranslationKeys {
  return copyForLocale(getDefaultLocale())
}
