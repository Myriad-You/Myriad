import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

describe('requestDedup invalidation', () => {
  it('does not re-cache stale data after clearDedupCache', async () => {
    const { dedupedFetch, clearDedupCache } = await import('./requestDedup')

    const { promise: slow, resolve: resolveFetch } = Promise.withResolvers<{
      n: number
    }>()

    const key = '/api/config/ui-test-generation'
    const p1 = dedupedFetch(key, () => slow, { cacheTTL: 60_000, cacheKey: key })

    clearDedupCache(key)

    const { promise: fresh, resolve: resolveFresh } = Promise.withResolvers<{
      n: number
    }>()
    const p2 = dedupedFetch(key, () => fresh, { cacheTTL: 60_000, cacheKey: key })

    resolveFetch({ n: 1 })
    await p1.catch(() => {})

    resolveFresh({ n: 2 })
    const data = await p2
    assert.equal(data.n, 2)

    const cached = await dedupedFetch(
      key,
      async () => ({ n: 99 }),
      { cacheTTL: 60_000, cacheKey: key },
    )
    assert.equal(cached.n, 2)
  })

  it('keeps old responses invalid after more than 200 other invalidations', async () => {
    const { dedupedFetch, clearDedupCache } = await import('./requestDedup')
    const old = Promise.withResolvers<string>()
    const pending = dedupedFetch('invalidated', () => old.promise)
    clearDedupCache('invalidated')
    for (let index = 0; index < 201; index++) clearDedupCache(`other-${index}`)
    old.resolve('stale')
    await pending
    const value = await dedupedFetch('invalidated', async () => 'fresh')
    assert.equal(value, 'fresh')
    clearDedupCache()
  })

  it('force refresh bypasses a cached result but joins an existing request', async () => {
    const { dedupedFetch, clearDedupCache } = await import('./requestDedup')
    const key = 'forced-refresh'
    await dedupedFetch(key, async () => 'cached')
    const fresh = Promise.withResolvers<string>()
    const pending = dedupedFetch(key, () => fresh.promise, { forceRefresh: true })
    const joined = dedupedFetch(key, async () => assert.fail('must join pending request'), { forceRefresh: true })
    fresh.resolve('fresh')
    assert.deepEqual(await Promise.all([pending, joined]), ['fresh', 'fresh'])
    assert.equal(await dedupedFetch(key, async () => 'unexpected'), 'fresh')
    clearDedupCache()
  })

  it('caches a null response without treating it as a cache miss', async () => {
    const { dedupedFetch, clearDedupCache } = await import('./requestDedup')
    await dedupedFetch('null-result', async () => null)
    assert.equal(await dedupedFetch('null-result', async () => assert.fail('null was cached')), null)
    clearDedupCache()
  })
})
