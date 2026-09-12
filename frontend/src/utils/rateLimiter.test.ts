import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { checkRateLimit } from './rateLimiter.ts'

describe('rateLimiter store 容量回收', () => {
  it('回收不会放行仍在封禁期的 key', () => {
    const victim = 'blocked/endpoint'

    for (let i = 0; i < 5; i++) {
      assert.equal(checkRateLimit(victim, 'login'), true)
    }
    assert.equal(checkRateLimit(victim, 'login'), false)

    for (let i = 0; i < 400; i++) {
      checkRateLimit(`evict-filler/${i}`, 'api')
    }

    // Do not GC an active ban.
    assert.equal(checkRateLimit(victim, 'login'), false)
  })

  it('大量一次性 key 不会让 store 无界增长', () => {
    for (let i = 0; i < 400; i++) {
      checkRateLimit(`growth-filler/${i}`, 'api')
    }

    const revisited = 'growth-filler/0'
    for (let i = 0; i < 100; i++) {
      assert.equal(checkRateLimit(revisited, 'api'), true)
    }
  })
})
