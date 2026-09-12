/** Types for the first-paint parser. Implementation is `parseLocale.shared.js`. */

export function isLocale(value: unknown): boolean
export function mapLanguageTag(tag: unknown): string | null
export function parseLanguageList(
  raw: unknown,
): Array<{ tag: string; q: number; index: number }>
export function parseLocale(raw: string | null | undefined): string | null
export function parseLocaleCookie(cookie: string): string | null
export function resolveHostLocale(
  stored: string | null | undefined,
  cookie: string | null | undefined,
  languageList: string | null | undefined,
): string
export function htmlLang(locale: string): string
