import type { TranslationKeys } from './index'
import { getDefaultLocale } from './index'
import { enUS } from './en-US'
import { jaJP } from './ja-JP'
import { zhCN } from './zh-CN'

/** Current UI language copy for service-layer errors (outside React). */
export function currentCopy(): TranslationKeys {
  switch (getDefaultLocale()) {
    case 'zh-CN':
      return zhCN
    case 'ja-JP':
      return jaJP
    default:
      return enUS
  }
}
