import type { TappManifestLocales } from '../types'

export interface LocalizedManifestText {
  name: string
  description?: string
}

function chineseScript(tag: string): 'hant' | 'hans' | null {
  const lower = tag.toLowerCase().replaceAll('_', '-')
  if (!lower.startsWith('zh')) return null
  if (
    lower.startsWith('zh-tw') ||
    lower.startsWith('zh-hk') ||
    lower.startsWith('zh-mo') ||
    lower.includes('hant')
  ) {
    return 'hant'
  }
  return 'hans'
}

export function pickLocaleEntry<T>(
  locales: Record<string, T> | undefined,
  locale: string | undefined,
): T | undefined {
  if (!locales || !locale) return undefined
  const target = locale.toLowerCase().replaceAll('_', '-')
  const keys = Object.keys(locales)

  const exact = keys.find((key) => key.toLowerCase().replaceAll('_', '-') === target)
  if (exact) return locales[exact]

  const targetScript = chineseScript(target)
  if (targetScript) {
    const sameScript = keys.find((key) => chineseScript(key) === targetScript)
    return sameScript ? locales[sameScript] : undefined
  }

  const targetLang = target.split('-')[0]
  const prefix = keys.find(
    (key) => key.toLowerCase().replaceAll('_', '-').split('-')[0] === targetLang,
  )
  return prefix ? locales[prefix] : undefined
}

export function nonEmptyText(value: unknown): string | undefined {
  return typeof value === 'string' && value.trim() ? value : undefined
}

export function resolveManifestText(
  source: {
    name: string
    description?: string
    locales?: TappManifestLocales
  },
  locale: string | undefined,
): LocalizedManifestText {
  const entry = pickLocaleEntry(source.locales, locale)
  return {
    name: nonEmptyText(entry?.name) ?? source.name,
    description: nonEmptyText(entry?.description) ?? source.description,
  }
}
