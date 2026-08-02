import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { checkRateLimit } from './rateLimiter.ts'

// store 是模块级的且没有重置入口，所以每个用例用互不相交的 key 前缀。
describe('rateLimiter store 容量回收', () => {
  it('回收不会放行仍在封禁期的 key', () => {
    const victim = 'blocked/endpoint'

    // login: 5 次/5 分钟，第 6 次触发封禁
    for (let i = 0; i < 5; i++) {
      assert.equal(checkRateLimit(victim, 'login'), true)
    }
    assert.equal(checkRateLimit(victim, 'login'), false)

    // 塞入远超上限的新 key，反复触发 prune
    for (let i = 0; i < 400; i++) {
      checkRateLimit(`evict-filler/${i}`, 'api')
    }

    // 封禁必须存活——回收掉它等于放行一个正被限流的调用方
    assert.equal(checkRateLimit(victim, 'login'), false)
  })

  it('大量一次性 key 不会让 store 无界增长', () => {
    // 无法直接读 store.size（未导出），改为断言可观察行为：
    // 被回收过的 key 重新出现时，配额是干净的而非继承旧状态。
    for (let i = 0; i < 400; i++) {
      checkRateLimit(`growth-filler/${i}`, 'api')
    }

    const revisited = 'growth-filler/0'
    // api: 100 次/分钟。若该 key 已被回收，这里应能重新取满配额。
    for (let i = 0; i < 100; i++) {
      assert.equal(checkRateLimit(revisited, 'api'), true)
    }
  })
})
