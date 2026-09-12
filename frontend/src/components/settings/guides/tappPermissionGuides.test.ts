import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'
import {
  getTappPermissionGuides,
  loadTappPermissionGuides,
} from './tappPermissionGuides.ts'

describe('tapp permission guides', () => {
  it('does not statically import ja or zh catalogs', () => {
    const src = readFileSync(
      new URL('./tappPermissionGuides.ts', import.meta.url),
      'utf8',
    )
    assert.equal(src.includes("from './tappPermissionGuides.ja-JP.json'"), false)
    assert.equal(src.includes("from './tappPermissionGuides.zh-CN.json'"), false)
  })

  it('serves English synchronously', () => {
    const en = getTappPermissionGuides('en-US')
    assert.ok(en['widget:register'].what.includes('home grid'))
  })

  it('loads Japanese on demand and then serves it synchronously', async () => {
    const ja = await loadTappPermissionGuides('ja-JP')
    assert.equal(getTappPermissionGuides('ja-JP'), ja)
    assert.ok(ja['widget:register'].what.includes('ホーム'))
    assert.notEqual(
      ja['widget:register'].what,
      getTappPermissionGuides('en-US')['widget:register'].what,
    )
  })
})
