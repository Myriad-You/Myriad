import type { Locale } from '../../../i18n'
import type { SettingGuidesCatalog } from './types'
import { en } from './catalog.en'

const cache = new Map<Locale, SettingGuidesCatalog>([['en-US', en]])
const inflight = new Map<Locale, Promise<SettingGuidesCatalog>>()

async function importCatalog(locale: Locale): Promise<SettingGuidesCatalog> {
  switch (locale) {
    case 'zh-CN':
      return (await import('./catalog.zh')).zh
    case 'ja-JP':
      return (await import('./catalog.ja')).ja
    default:
      return en
  }
}

/** Load (and cache) the setting-guide catalog for one locale. */
export function loadSettingGuidesCatalog(
  locale: Locale,
): Promise<SettingGuidesCatalog> {
  const cached = cache.get(locale)
  if (cached) return Promise.resolve(cached)

  const pending = inflight.get(locale)
  if (pending) return pending

  const promise = importCatalog(locale)
    .then((catalog) => {
      cache.set(locale, catalog)
      inflight.delete(locale)
      return catalog
    })
    .catch((err) => {
      inflight.delete(locale)
      throw err
    })

  inflight.set(locale, promise)
  return promise
}

/**
 * Sync catalog. ja / zh are not in the static graph: the first call starts
 * the chunk load and returns English until it arrives.
 */
export function getSettingGuidesCatalog(locale: Locale): SettingGuidesCatalog {
  const cached = cache.get(locale)
  if (cached) return cached
  void loadSettingGuidesCatalog(locale)
  return cache.get(locale) ?? en
}

export type { SettingGuideEntry, SettingGuidesCatalog } from './types'
