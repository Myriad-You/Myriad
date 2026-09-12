/**
 * Host UI locale parsing. No TypeScript — first-paint inlines this file.
 *  Cases: `shared/host_locale_cases.json` (must match backend parse_host_locale).
 */

export function isLocale(value) {
  return (
    value === 'zh-CN' ||
    value === 'zh-TW' ||
    value === 'en-US' ||
    value === 'ja-JP' ||
    value === 'ko-KR' ||
    value === 'fr-FR' ||
    value === 'de-DE'
  )
}

export function mapLanguageTag(tag) {
  const raw = String(tag || '')
    .trim()
    .replaceAll('_', '-')
  if (!raw) return null
  if (isLocale(raw)) return raw
  const lower = raw.toLowerCase()
  if (
    lower.startsWith('zh-tw') ||
    lower.startsWith('zh-hk') ||
    lower.startsWith('zh-mo') ||
    lower.includes('hant')
  ) {
    return 'zh-TW'
  }
  if (lower.startsWith('zh')) return 'zh-CN'
  if (lower.startsWith('ja')) return 'ja-JP'
  if (lower.startsWith('ko')) return 'ko-KR'
  if (lower.startsWith('fr')) return 'fr-FR'
  if (lower.startsWith('de')) return 'de-DE'
  if (lower.startsWith('en')) return 'en-US'
  return null
}

export function parseLanguageList(raw) {
  return String(raw || '')
    .split(',')
    .map((part, index) => {
      const bits = part.trim().split(';')
      let q = 1
      for (const bit of bits.slice(1)) {
        const match = bit.trim().match(/^q=([0-9.]+)$/i)
        if (!match) continue
        const value = Number(match[1])
        q = Number.isFinite(value) ? Math.min(1, Math.max(0, value)) : 0
      }
      return { tag: (bits[0] || '').trim(), q, index }
    })
    .filter((item) => item.tag && item.q > 0)
    .toSorted((a, b) => b.q - a.q || a.index - b.index)
}

export function parseLocale(raw) {
  const tag = raw == null ? '' : String(raw).trim()
  if (!tag) return null
  if (isLocale(tag)) return tag
  const items = parseLanguageList(tag)
  for (const item of items) {
    const mapped = mapLanguageTag(item.tag)
    if (mapped) return mapped
  }
  return null
}

export function parseLocaleCookie(cookie) {
  const parts = String(cookie || '').split(';')
  for (const part of parts) {
    const trimmed = part.trim()
    if (trimmed.startsWith('locale=')) {
      try {
        return decodeURIComponent(trimmed.slice('locale='.length))
      } catch {
        return trimmed.slice('locale='.length)
      }
    }
  }
  return null
}

export function resolveHostLocale(stored, cookie, languageList) {
  return (
    parseLocale(stored) ||
    parseLocale(cookie) ||
    parseLocale(languageList) ||
    'en-US'
  )
}

export function htmlLang(locale) {
  switch (locale) {
    case 'zh-CN':
      return 'zh-CN'
    case 'zh-TW':
      return 'zh-TW'
    case 'ja-JP':
      return 'ja-JP'
    case 'ko-KR':
      return 'ko-KR'
    case 'fr-FR':
      return 'fr-FR'
    case 'de-DE':
      return 'de-DE'
    default:
      return 'en'
  }
}
