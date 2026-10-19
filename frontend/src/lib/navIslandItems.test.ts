import assert from 'node:assert/strict'
import { readdirSync, readFileSync } from 'node:fs'
import { test } from 'node:test'
import { NAV_ISLAND_ITEMS } from './navIslandItems.tsx'

const I18N = new URL('../i18n/', import.meta.url)
const BACKEND_I18N = new URL('../../../backend/i18n/', import.meta.url)

const locales = readdirSync(I18N)
  .map((name) => /^([a-z]{2}-[A-Z]{2})\.json$/.exec(name)?.[1])
  .filter((locale): locale is string => locale != null)

function json(url: URL) {
  return JSON.parse(readFileSync(url, 'utf8')) as Record<string, unknown>
}

test('每个条目的文案键在每种语言里都有', () => {
  assert.ok(locales.length >= 7)
  for (const locale of locales) {
    const nav = json(new URL(`${locale}.json`, I18N)).nav as Record<string, string>
    for (const item of NAV_ISLAND_ITEMS) {
      for (const key of [item.label, item.tip]) {
        assert.ok(nav[key], `${locale} nav.${key}`)
      }
    }
  }
})

test('博客页上导航岛的文案和主站一字不差', () => {
  for (const locale of locales) {
    const nav = json(new URL(`${locale}.json`, I18N)).nav as Record<string, string>
    const backend = json(new URL(`nav.${locale}.json`, BACKEND_I18N))
    for (const [key, value] of Object.entries(backend)) {
      assert.equal(value, nav[key], `${locale} nav.${key}`)
    }
    const needed = new Set(['mainNavigation', ...NAV_ISLAND_ITEMS.flatMap((item) => [item.label, item.tip])])
    assert.deepEqual(new Set(Object.keys(backend)), needed, `nav.${locale}.json keys`)
  }
})
