import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { requestManager } from './concurrentRequestManager.ts'

describe('request promise settlement', () => {
  it('rejects a cancelled queued request without starting it and releases active slots', async () => {
    const gates = Array.from({ length: 6 }, () => Promise.withResolvers<number>())
    const active = gates.map((gate, index) =>
      requestManager.fetch(`active-${index}`, () => gate.promise),
    )
    let started = false
    try {
      const queued = requestManager.fetch('queued', async () => {
        started = true
        return 99
      })
      assert.deepEqual(requestManager.getStatus(), { active: 6, queued: 1, maxConcurrent: 6 })
      requestManager.cancelRequest('queued')
      await assert.rejects(queued, /Request was cancelled/)
      assert.equal(started, false)
    } finally {
      gates.forEach((gate, index) => gate.resolve(index))
      assert.deepEqual(await Promise.all(active), [0, 1, 2, 3, 4, 5])
    }
    assert.deepEqual(requestManager.getStatus(), { active: 0, queued: 0, maxConcurrent: 6 })
  })

  it('rejects synchronous and asynchronous fetch failures and allows subsequent requests', async () => {
    const failure = new Error('fetch failed')
    for (const fetcher of [
      () => { throw failure },
      () => Promise.reject(failure),
    ]) {
      await assert.rejects(requestManager.fetch('failure', fetcher), (error) => error === failure)
    }
    assert.equal(await requestManager.fetch('success', async () => 42), 42)
    assert.deepEqual(requestManager.getStatus(), { active: 0, queued: 0, maxConcurrent: 6 })
  })
})
