import assert from 'node:assert/strict'
import { readdirSync, readFileSync } from 'node:fs'
import { describe, it } from 'node:test'

const LOCALES = [
  'en-US',
  'zh-CN',
  'zh-TW',
  'ja-JP',
  'ko-KR',
  'fr-FR',
  'de-DE',
] as const
const NAMESPACES = [
  'config',
  'tapp',
  'brew',
  'merope',
  'errors',
  'agentCaps',
  'notifications',
] as const

function loadJson(rel: string): unknown {
  return JSON.parse(readFileSync(new URL(rel, import.meta.url), 'utf8'))
}

function collectKeys(value: unknown, prefix = ''): string[] {
  if (Array.isArray(value)) return prefix ? [prefix] : []
  if (value && typeof value === 'object') {
    const entries = Object.entries(value as Record<string, unknown>)
    if (entries.length === 0) return prefix ? [prefix] : []
    return entries.flatMap(([key, child]) =>
      collectKeys(child, prefix ? `${prefix}.${key}` : key),
    )
  }
  return prefix ? [prefix] : []
}

function assertSameKeys(label: string, canonical: unknown, other: unknown) {
  const expected = new Set(collectKeys(canonical))
  const actual = new Set(collectKeys(other))
  const missing = Iterator.from(expected.difference(actual)).toArray()
  const extra = Iterator.from(actual.difference(expected)).toArray()
  assert.deepEqual(
    { missing, extra },
    { missing: [], extra: [] },
    `${label} key set must match en-US`,
  )
}

describe('host catalog keys', () => {
  it('keeps core and namespace keys aligned', () => {
    for (const file of [`./en-US.json`, ...NAMESPACES.map((ns) => `./${ns}.en-US.json`)]) {
      const canonical = loadJson(file)
      for (const locale of LOCALES) {
        if (locale === 'en-US') continue
        assertSameKeys(
          `${file.replace('en-US', locale)}`,
          canonical,
          loadJson(file.replace('en-US', locale)),
        )
      }
    }
  })

  it('keeps setting-guide keys aligned', () => {
    const guides = new URL('../components/settings/guides/', import.meta.url)
    const pairs: Array<[string, string]> = [
      ['catalog.en-US.json', 'catalog.zh-CN.json'],
      ['catalog.en-US.json', 'catalog.zh-TW.json'],
      ['catalog.en-US.json', 'catalog.ja-JP.json'],
      ['catalog.en-US.json', 'catalog.ko-KR.json'],
      ['catalog.en-US.json', 'catalog.fr-FR.json'],
      ['catalog.en-US.json', 'catalog.de-DE.json'],
      ['tappPermissionGuides.en-US.json', 'tappPermissionGuides.zh-CN.json'],
      ['tappPermissionGuides.en-US.json', 'tappPermissionGuides.zh-TW.json'],
      ['tappPermissionGuides.en-US.json', 'tappPermissionGuides.ja-JP.json'],
      ['tappPermissionGuides.en-US.json', 'tappPermissionGuides.ko-KR.json'],
      ['tappPermissionGuides.en-US.json', 'tappPermissionGuides.fr-FR.json'],
      ['tappPermissionGuides.en-US.json', 'tappPermissionGuides.de-DE.json'],
    ]
    for (const [canonicalName, otherName] of pairs) {
      assertSameKeys(
        otherName,
        JSON.parse(readFileSync(new URL(canonicalName, guides), 'utf8')),
        JSON.parse(readFileSync(new URL(otherName, guides), 'utf8')),
      )
    }
  })

  it('does not leave OpenCC glossary leftovers in Traditional catalogs', () => {
    const banned = ['例項', '許可權', '引數', '全域性']
    const i18nDir = new URL('./', import.meta.url)
    const guidesDir = new URL('../components/settings/guides/', import.meta.url)
    const files = [
      ...readdirSync(i18nDir)
        .filter((name) => name.includes('zh-TW') && name.endsWith('.json'))
        .map((name) => new URL(name, i18nDir)),
      ...readdirSync(guidesDir)
        .filter((name) => name.includes('zh-TW') && name.endsWith('.json'))
        .map((name) => new URL(name, guidesDir)),
    ]
    for (const file of files) {
      const text = readFileSync(file, 'utf8')
      for (const term of banned) {
        assert.equal(
          text.includes(term),
          false,
          `${file.pathname} still contains ${term}`,
        )
      }
    }
  })
})
