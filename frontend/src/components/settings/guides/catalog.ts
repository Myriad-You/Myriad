import type { Locale } from '../../../i18n'
import type { SettingGuidesCatalog } from './types'
import { en } from './catalog.en'
import { ja } from './catalog.ja'
import { zh } from './catalog.zh'

export function getSettingGuidesCatalog(locale: Locale): SettingGuidesCatalog {
  if (locale === 'zh-CN') return zh
  if (locale === 'ja-JP') return ja
  return en
}

export type { SettingGuideEntry, SettingGuidesCatalog } from './types'
