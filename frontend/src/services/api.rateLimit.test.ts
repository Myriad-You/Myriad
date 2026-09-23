import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { RATE_LIMIT_RETRY_MAX_MS, rateLimitRetryDelayMs } from './api'

describe('rateLimitRetryDelayMs', () => {
  it('没有 Retry-After 就等一秒再试', () => {
    assert.equal(rateLimitRetryDelayMs(null), 1000)
  })

  it('短窗口按头等，太短也至少 250ms', () => {
    assert.equal(rateLimitRetryDelayMs(2), 2000)
    assert.equal(rateLimitRetryDelayMs(0), 250)
  })

  it('窗口超过上限就不等，直接报错', () => {
    assert.equal(rateLimitRetryDelayMs(60), null)
    assert.equal(rateLimitRetryDelayMs(RATE_LIMIT_RETRY_MAX_MS / 1000 + 1), null)
  })
})
