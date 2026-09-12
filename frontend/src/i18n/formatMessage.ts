import type { Locale } from './locales'

const SIMPLE_TOKEN = /\{(\w+)\}/g
const PLURAL_TOKEN =
  /\{(\w+),\s*plural\s*,((?:[^{}]|\{[^{}]*\})+)\}/g

function pluralCategory(locale: Locale, count: number): string {
  try {
    return new Intl.PluralRules(locale).select(Math.abs(count))
  } catch {
    return count === 1 ? 'one' : 'other'
  }
}

function pickPluralBranch(body: string, locale: Locale, count: number): string {
  const branches = new Map<string, string>()
  const re = /(=\d+|zero|one|two|few|many|other)\s*\{([^{}]*)\}/g
  let match = re.exec(body)
  while (match) {
    branches.set(match[1], match[2])
    match = re.exec(body)
  }
  const exact = branches.get(`=${count}`)
  if (exact != null) return exact
  const category = pluralCategory(locale, count)
  return branches.get(category) ?? branches.get('other') ?? ''
}

/** `{name}` plus ICU `{count, plural, one {# item} other {# items}}`. */
export function formatMessage(
  locale: Locale,
  template: string,
  params: Record<string, string | number> = {},
): string {
  if (!template) return ''
  const withPlurals = template.replace(PLURAL_TOKEN, (_, key: string, body: string) => {
    const raw = params[key]
    const count = typeof raw === 'number' ? raw : Number(raw)
    const n = Number.isFinite(count) ? count : 0
    return pickPluralBranch(body, locale, n).replaceAll('#', String(n))
  })
  return withPlurals.replace(SIMPLE_TOKEN, (_, key: string) => {
    const value = params[key]
    return value == null ? `{${key}}` : String(value)
  })
}

export function formatNumber(locale: Locale, value: number): string {
  if (!Number.isFinite(value)) return '0'
  try {
    return new Intl.NumberFormat(locale).format(value)
  } catch {
    return String(value)
  }
}

export function formatDate(
  locale: Locale,
  value: Date | number | string,
  options?: Intl.DateTimeFormatOptions,
): string {
  const date = value instanceof Date ? value : new Date(value)
  if (Number.isNaN(date.getTime())) return ''
  try {
    return new Intl.DateTimeFormat(locale, options).format(date)
  } catch {
    return date.toISOString()
  }
}
