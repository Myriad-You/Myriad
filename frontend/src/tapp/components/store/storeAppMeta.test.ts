import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { inferPrimaryCatalogLocale } from './storeAppMeta.ts'

describe('inferPrimaryCatalogLocale', () => {
  it('prefers Traditional Chinese when distinctive characters appear', () => {
    assert.equal(inferPrimaryCatalogLocale('這個應用可預設連線'), 'zh-TW')
    assert.equal(inferPrimaryCatalogLocale('軟體設定與網路'), 'zh-TW')
  })

  it('maps other CJK text to Simplified Chinese', () => {
    assert.equal(inferPrimaryCatalogLocale('这个应用可以设置网络'), 'zh-CN')
  })

  it('maps kana to Japanese', () => {
    assert.equal(inferPrimaryCatalogLocale('アプリを設定します'), 'ja-JP')
  })
})
