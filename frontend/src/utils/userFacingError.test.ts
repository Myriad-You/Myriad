import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { ApiError } from '../services/api.ts'
import {
  httpStatusMessage,
  isUselessErrorText,
  userFacingError,
} from './userFacingError.ts'

describe('userFacingError', () => {
  it('treats API Error: 500 and JSON dumps as useless', () => {
    assert.equal(isUselessErrorText('API Error: 500'), true)
    assert.equal(isUselessErrorText('{"error":"boom"}'), true)
    assert.equal(isUselessErrorText('Failed to fetch'), true)
    assert.equal(isUselessErrorText('HTTP 500: Internal Server Error'), true)
    assert.equal(isUselessErrorText('CSRF token unavailable'), true)
    assert.equal(isUselessErrorText('Database error'), true)
    assert.equal(isUselessErrorText('Failed to save notification preferences'), true)
    assert.equal(isUselessErrorText('Steam 未返回游戏数据'), false)
  })

  it('maps HTTP status to a localized reason', () => {
    assert.match(httpStatusMessage(401), /登录|Sign in|ログイン/)
    assert.match(httpStatusMessage(502), /502/)
    assert.match(httpStatusMessage(0), /网络|network|ネットワーク/i)
  })

  it('prefers status copy over boilerplate and keeps useful detail', () => {
    const err = new ApiError('Steam 未返回游戏数据。请确认资料公开。', 502, 'platform_refresh_failed')
    const text = userFacingError(err, '操作失败')
    assert.match(text, /502/)
    assert.match(text, /Steam/)
  })

  it('does not leak API Error: status as the only message', () => {
    const err = new ApiError('API Error: 500', 500)
    const text = userFacingError(err)
    assert.equal(/API Error/i.test(text), false)
    assert.match(text, /500/)
  })

  it('extracts HTTP status from English fallbacks', () => {
    const text = userFacingError(
      new Error('Could not load site face (HTTP 502)'),
      '操作失败',
    )
    assert.match(text, /502/)
    assert.equal(/Could not load/i.test(text), false)
  })
})
