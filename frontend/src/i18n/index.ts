export type { TranslationKeys } from './assembleLocale'
export { formatDate, formatMessage, formatNumber } from './formatMessage'
export type { Locale } from './locales'

export {
  getDefaultLocale,
  hostLanguageName,
  hostLanguageShort,
  htmlLang,
  isLocale,
  localeOrFallback,
  LOCALES,
  parseLocale,
  parseLocaleCookie,
  saveLocale,
} from './locales'
export type { HostLanguageLabels } from './locales'

/** Event keys are domain enums, not TranslationKeys. */
export {
  getNotificationCopy,
  type NotificationCopy,
  type NotificationSourceCopy,
  type NotificationUiCopy,
} from './notificationCatalog'
