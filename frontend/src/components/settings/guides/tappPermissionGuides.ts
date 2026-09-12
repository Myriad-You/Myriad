import type { Locale } from '../../../i18n'
import type { TappPermission } from '../../../tapp/types'
import type { SettingGuideEntry } from './types'
import { createLocaleLoader } from '../../../i18n/createLocaleLoader'
import en from './tappPermissionGuides.en-US.json' with { type: 'json' }

export type TappPermissionGuides = Record<TappPermission, SettingGuideEntry>

export function tappPermissionGuidePath(permission: TappPermission): string {
  return `tapp.perm.${permission}`
}

const loader = createLocaleLoader<TappPermissionGuides>({
  'zh-CN': async () =>
    (await import('./tappPermissionGuides.zh-CN.json')).default,
  'zh-TW': async () =>
    (await import('./tappPermissionGuides.zh-TW.json')).default,
  'en-US': async () => en,
  'ja-JP': async () =>
    (await import('./tappPermissionGuides.ja-JP.json')).default,
  'ko-KR': async () =>
    (await import('./tappPermissionGuides.ko-KR.json')).default,
  'fr-FR': async () =>
    (await import('./tappPermissionGuides.fr-FR.json')).default,
  'de-DE': async () =>
    (await import('./tappPermissionGuides.de-DE.json')).default,
})
loader.seed('en-US', en)

export function loadTappPermissionGuides(
  locale: Locale,
): Promise<TappPermissionGuides> {
  return loader.load(locale)
}

export function getTappPermissionGuides(locale: Locale): TappPermissionGuides {
  const cached = loader.getCached(locale)
  if (cached) return cached
  void loadTappPermissionGuides(locale)
  return loader.getCached(locale) ?? en
}

export function getTappPermissionGuide(
  locale: Locale,
  permission: TappPermission,
): SettingGuideEntry {
  return getTappPermissionGuides(locale)[permission]
}
