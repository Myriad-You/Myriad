import {
  htmlLang as htmlLangValue,
  isLocale as isLocaleValue,
  parseLocaleCookie as parseLocaleCookieValue,
  parseLocale as parseLocaleValue,
  resolveHostLocale,
} from './parseLocale.shared.js'

export const LOCALES = [
  'zh-CN',
  'zh-TW',
  'en-US',
  'ja-JP',
  'ko-KR',
  'fr-FR',
  'de-DE',
] as const

export type Locale = (typeof LOCALES)[number]

export interface HostLanguageLabels {
  languageZh: string
  languageZhTw: string
  languageEn: string
  languageJa: string
  languageKo: string
  languageFr: string
  languageDe: string
  languageZhShort: string
  languageZhTwShort: string
  languageEnShort: string
  languageJaShort: string
  languageKoShort: string
  languageFrShort: string
  languageDeShort: string
}

export function hostLanguageName(
  locale: Locale,
  labels: HostLanguageLabels,
): string {
  switch (locale) {
    case 'zh-CN':
      return labels.languageZh
    case 'zh-TW':
      return labels.languageZhTw
    case 'en-US':
      return labels.languageEn
    case 'ja-JP':
      return labels.languageJa
    case 'ko-KR':
      return labels.languageKo
    case 'fr-FR':
      return labels.languageFr
    case 'de-DE':
      return labels.languageDe
  }
}

export function hostLanguageShort(
  locale: Locale,
  labels: HostLanguageLabels,
): string {
  switch (locale) {
    case 'zh-CN':
      return labels.languageZhShort
    case 'zh-TW':
      return labels.languageZhTwShort
    case 'en-US':
      return labels.languageEnShort
    case 'ja-JP':
      return labels.languageJaShort
    case 'ko-KR':
      return labels.languageKoShort
    case 'fr-FR':
      return labels.languageFrShort
    case 'de-DE':
      return labels.languageDeShort
  }
}

export function isLocale(value: unknown): value is Locale {
  return isLocaleValue(value)
}

/** Map a BCP 47 tag or Accept-Language list onto a host UI locale. */
export function parseLocale(raw: string | null | undefined): Locale | null {
  const parsed = parseLocaleValue(raw)
  return parsed == null ? null : (parsed as Locale)
}

export function localeOrFallback(
  raw: string | null | undefined,
  fallback: Locale = 'en-US',
): Locale {
  return parseLocale(raw) ?? fallback
}

const LOCALE_COOKIE = 'locale'
const LOCALE_COOKIE_MAX_AGE = 60 * 60 * 24 * 365

export function parseLocaleCookie(cookie: string): string | null {
  return parseLocaleCookieValue(cookie)
}

function readLocaleCookie(): string | null {
  if (typeof document === 'undefined') return null
  return parseLocaleCookie(document.cookie)
}

function writeLocaleCookie(locale: Locale): void {
  if (typeof document === 'undefined') return
  document.cookie = `${LOCALE_COOKIE}=${encodeURIComponent(locale)}; Path=/; Max-Age=${LOCALE_COOKIE_MAX_AGE}; SameSite=Lax`
}

/** localStorage → cookie → navigator languages (q-aware) → en-US */
export function getDefaultLocale(): Locale {
  let stored: string | null = null
  let cookie: string | null = null
  if (typeof window !== 'undefined') {
    stored = localStorage.getItem('locale')
    cookie = readLocaleCookie()
  }

  let languageList = ''
  if (typeof window !== 'undefined' && typeof navigator !== 'undefined') {
    languageList = Array.isArray(navigator.languages)
      ? navigator.languages.join(',')
      : ''
    languageList =
      languageList ||
      navigator.language ||
      (navigator as { userLanguage?: string }).userLanguage ||
      ''
  }

  return resolveHostLocale(stored, cookie, languageList) as Locale
}

export function saveLocale(locale: Locale): void {
  if (typeof window !== 'undefined') {
    localStorage.setItem('locale', locale)
    writeLocaleCookie(locale)
  }
}

export function htmlLang(locale: Locale): string {
  return htmlLangValue(locale)
}
