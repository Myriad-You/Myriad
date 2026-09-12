import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'
import {
  getSettingGuidesCatalog,
  loadSettingGuidesCatalog,
} from './catalog.ts'

describe('setting guide catalog', () => {
  it('does not statically import ja or zh catalogs', () => {
    const src = readFileSync(new URL('./catalog.ts', import.meta.url), 'utf8')
    assert.equal(src.includes("from './catalog.ja-JP.json'"), false)
    assert.equal(src.includes("from './catalog.zh-CN.json'"), false)
  })

  it('serves English synchronously', () => {
    const en = getSettingGuidesCatalog('en-US')
    assert.ok(en.ui.siteUrl.what.includes('public address'))
  })

  it('loads Japanese on demand and then serves it synchronously', async () => {
    const ja = await loadSettingGuidesCatalog('ja-JP')
    assert.equal(getSettingGuidesCatalog('ja-JP'), ja)
    assert.ok(ja.ui.siteUrl.what.includes('住所'))
    assert.notEqual(
      ja.ui.siteUrl.what,
      getSettingGuidesCatalog('en-US').ui.siteUrl.what,
    )
  })
})
