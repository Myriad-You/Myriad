import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'
import { loadLocale } from './loadLocale.ts'
import { copyForLocale, currentCopy } from './localeCopy.ts'

describe('currentCopy', () => {
  it('does not statically import ja or zh locale modules', () => {
    const src = readFileSync(new URL('./localeCopy.ts', import.meta.url), 'utf8')
    assert.equal(src.includes("from './ja-JP'"), false)
    assert.equal(src.includes("from './zh-CN'"), false)
  })

  it('returns localized wallpaper and store errors', () => {
    const copy = currentCopy()
    assert.equal(typeof copy.wallpaperStatus.unsafeUrl, 'string')
    assert.ok(copy.wallpaperStatus.unsafeUrl.length > 0)
    assert.equal(typeof copy.tapp.storeAdminRequired, 'string')
    assert.equal(typeof copy.brew.loadSourcesFailed, 'string')
    assert.equal(typeof copy.errors.setupCheckFailed, 'string')
    assert.ok(copy.tapp.storeAppNotFound.includes('{id}'))
    assert.ok(copy.merope.anime25dPartCount.includes('{max}'))
    assert.ok(copy.errors.rateLimitedRetry.includes('{sec}'))
    assert.ok(copy.brew.webSearch.length > 0)
    assert.ok(copy.errors.serverError.includes('{status}'))
    assert.ok(copy.errors.lyricsFailed.includes('{status}'))
    assert.ok(copy.tapp.storeDownloadFailed.includes('{name}'))
  })

  it('falls back to English until a non-default locale chunk is loaded', async () => {
    const before = copyForLocale('ja-JP')
    assert.equal(before.common.loading, currentCopy().common.loading)
    const ja = await loadLocale('ja-JP')
    assert.equal(copyForLocale('ja-JP'), ja)
    assert.equal(copyForLocale('ja-JP').common.loading, '読み込み中...')
    assert.equal(currentCopy().common.loading, 'Loading...')
  })
})
