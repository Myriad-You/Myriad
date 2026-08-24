import type { TranslationKeys } from './index'
import { enUS } from './en-US'
import { getDefaultLocale } from './index'
import { jaJP } from './ja-JP'
import { zhCN } from './zh-CN'

/** Copy for an explicitly selected UI language. */
export function copyForLocale(locale: string): TranslationKeys {
  switch (locale) {
    case 'zh-CN':
      return zhCN
    case 'ja-JP':
      return jaJP
    default:
      return enUS
  }
}

/** Current UI language copy for service-layer errors (outside React). */
export function currentCopy(): TranslationKeys {
  return copyForLocale(getDefaultLocale())
}
