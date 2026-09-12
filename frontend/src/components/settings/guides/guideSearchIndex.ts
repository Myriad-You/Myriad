import type { Locale } from '../../../i18n'
import type { SettingGuideEntry, SettingGuidesCatalog } from './types'
import { getSettingGuidesCatalog } from './catalog'

export const GUIDE_CATALOG_TO_SECTION: Record<
  keyof SettingGuidesCatalog,
  string
> = {
  ui: 'basic',
  modules: 'modules',
  platforms: 'platforms',
  notifications: 'notifications',
  ai: 'ai',
  tripo: 'tripo',
  oauth: 'oauth',
  permissions: 'permissions',
  users: 'users',
  advanced: 'advanced',
  federation: 'federation',
  updater: 'about',
  about: 'about',
  tapp: '',
}

export interface GuideSearchEntry {
  type: 'guide'
  section: string
  title: string
  description: string
  keywords: string[]
  haystack: string
  guidePath: string
}

function entryFields(entry: SettingGuideEntry): string[] {
  return [entry.what, entry.chain, entry.frontend, entry.notes].filter(
    (s): s is string => typeof s === 'string' && s.trim().length > 0,
  )
}

export function guideEntryTitle(what: string, max = 42): string {
  const cleaned = what
    .replaceAll(/^[①②③④⑤⑥⑦⑧⑨⑩\d]+[).、\s]*/ug, '')
    .trim()
  const first = cleaned.split(/[。！？\n]/u)[0]?.trim() || cleaned
  if (first.length <= max) return first
  return `${first.slice(0, max - 1)}…`
}

export function tokenizeForSearch(text: string): string[] {
  const lower = text.toLowerCase()
  const parts = lower
    .split(/[^\p{L}\p{N}+#./:_-]+/u)
    .map((s) => s.trim())
    .filter((s) => s.length >= 2)
  const extra: string[] = []
  for (const p of parts) {
    if (!/^[\u4E00-\u9FFF\u3040-\u30FF]+$/u.test(p)) continue
    if (p.length <= 4) continue
    extra.push(p.slice(0, 2), p.slice(0, 3))
    if (p.length >= 4) extra.push(p.slice(0, 4))
  }
  return Iterator.from(new Set(parts).union(new Set(extra))).toArray()
}

export function buildGuideSearchIndex(locale: Locale): GuideSearchEntry[] {
  const catalog = getSettingGuidesCatalog(locale)
  const out: GuideSearchEntry[] = []

  for (const [area, group] of Object.entries(catalog) as Array<
    [
      keyof SettingGuidesCatalog,
      SettingGuidesCatalog[keyof SettingGuidesCatalog],
    ]
  >) {
    const section = GUIDE_CATALOG_TO_SECTION[area]
    if (!section || !group || typeof group !== 'object') continue

    for (const [key, value] of Object.entries(group)) {
      if (
        value &&
        typeof value === 'object' &&
        Object.hasOwn(value, 'what') &&
        (value as SettingGuideEntry).what
      ) {
        const entry = value as SettingGuideEntry
        const fields = entryFields(entry)
        const blob = fields.join('\n')
        const haystack = blob.toLowerCase().replaceAll(/\s+/g, ' ').trim()
        const tokens = tokenizeForSearch(blob)
        tokens.push(key.toLowerCase(), area.toLowerCase())

        out.push({
          type: 'guide',
          section,
          title: guideEntryTitle(entry.what),
          description:
            entry.frontend?.split('\n')[0]?.trim() ||
            entry.notes?.split('\n')[0]?.trim() ||
            entry.what,
          keywords: Iterator.from(new Set(tokens)).toArray(),
          haystack,
          guidePath: `${area}.${key}`,
        })
        continue
      }

      if (value && typeof value === 'object') {
        for (const [subKey, subVal] of Object.entries(
          value as unknown as Record<string, SettingGuideEntry>,
        )) {
          if (!subVal?.what) continue
          const fields = entryFields(subVal)
          const blob = fields.join('\n')
          const haystack = blob.toLowerCase().replaceAll(/\s+/g, ' ').trim()
          const tokens = tokenizeForSearch(blob)
          tokens.push(
            key.toLowerCase(),
            subKey.toLowerCase(),
            area.toLowerCase(),
          )

          out.push({
            type: 'guide',
            section,
            title: guideEntryTitle(subVal.what),
            description:
              subVal.frontend?.split('\n')[0]?.trim() ||
              subVal.notes?.split('\n')[0]?.trim() ||
              subVal.what,
            keywords: Iterator.from(new Set(tokens)).toArray(),
            haystack,
            guidePath: `${area}.${key}.${subKey}`,
          })
        }
      }
    }
  }

  return out
}

export function guideKeywordsForSection(
  locale: Locale,
  sectionId: string,
): string[] {
  const entries = buildGuideSearchIndex(locale).filter(
    (e) => e.section === sectionId,
  )
  const set = new Set<string>()
  for (const e of entries) {
    // short dense tokens only; don't dump the haystack
    for (const k of e.keywords) {
      if (k.length >= 2 && k.length <= 16) set.add(k)
    }
  }
  return Iterator.from(set).toArray()
}
