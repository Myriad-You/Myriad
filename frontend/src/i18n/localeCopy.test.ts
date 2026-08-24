import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { currentCopy } from './localeCopy.ts'

describe('currentCopy', () => {
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
})
