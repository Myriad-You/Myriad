/** Event keys are domain enums (`NotificationEventKey`), not flat TranslationKeys. */

import type {
  NotificationEventKey,
  NotificationSourceKey,
} from '../services/notificationPreferencesApi'
import type { Locale } from './index'
import { createLocaleLoader } from './createLocaleLoader'
import en from './notifications.en-US.json' with { type: 'json' }

export interface NotificationSourceCopy {
  title: string
  description: string
}

export interface NotificationUiCopy {
  master: string
  masterDesc: string
  delivery: string
  island: string
  islandDesc: string
  toast: string
  toastDesc: string
  browser: string
  browserDesc: string
  sourceEnabled: string
  sources: string
  locations: string
  locationsDesc: string
  events: string
  panelLocation: string
  toastLocation: string
  islandLocation: string
  browserLocation: string
}

export interface NotificationCopy {
  sources: Record<NotificationSourceKey, NotificationSourceCopy>
  events: Record<NotificationEventKey, string>
  ui: NotificationUiCopy
}

const loader = createLocaleLoader<NotificationCopy>({
  'zh-CN': async () => (await import('./notifications.zh-CN.json')).default,
  'zh-TW': async () => (await import('./notifications.zh-TW.json')).default,
  'en-US': async () => en,
  'ja-JP': async () => (await import('./notifications.ja-JP.json')).default,
  'ko-KR': async () => (await import('./notifications.ko-KR.json')).default,
  'fr-FR': async () => (await import('./notifications.fr-FR.json')).default,
  'de-DE': async () => (await import('./notifications.de-DE.json')).default,
})
loader.seed('en-US', en)

export function loadNotificationCopy(locale: Locale): Promise<NotificationCopy> {
  return loader.load(locale)
}

export function getNotificationCopy(locale: Locale): NotificationCopy {
  const cached = loader.getCached(locale)
  if (cached) return cached
  void loadNotificationCopy(locale)
  return loader.getCached(locale) ?? en
}
