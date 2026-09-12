import assert from 'node:assert/strict'
import { afterEach, describe, it } from 'node:test'

import {
  clearDedupCache,
  clearDedupCacheByPrefix,
  dedupedFetch,
} from './requestDedup'

describe('request dedup prefix invalidation', () => {
  afterEach(() => clearDedupCache())

  it('clears every pagination/filter variant without evicting other endpoints', async () => {
    const calls = new Map<string, number>()
    const fetchKey = (key: string) =>
      dedupedFetch(key, async () => {
        calls.set(key, (calls.get(key) ?? 0) + 1)
        return calls.get(key)
      })
    const pageOne = '/api/library?offset=0&limit=120'
    const musicPage = '/api/library?offset=120&limit=120&type=music'
    const unrelated = '/api/config/ui'

    await Promise.all([
      fetchKey(pageOne),
      fetchKey(musicPage),
      fetchKey(unrelated),
    ])
    clearDedupCacheByPrefix('/api/library')
    await Promise.all([
      fetchKey(pageOne),
      fetchKey(musicPage),
      fetchKey(unrelated),
    ])

    assert.equal(calls.get(pageOne), 2)
    assert.equal(calls.get(musicPage), 2)
    assert.equal(calls.get(unrelated), 1)
  })

  it('prevents an in-flight stale response from repopulating a cleared key', async () => {
    const { promise: firstPending, resolve: resolveFirst } =
      Promise.withResolvers<string>()
    const first = dedupedFetch('/api/library?offset=0', () => firstPending)
    clearDedupCacheByPrefix('/api/library')
    resolveFirst('stale')
    await first

    let refreshed = 0
    const value = await dedupedFetch('/api/library?offset=0', async () => {
      refreshed++
      return 'fresh'
    })
    assert.equal(value, 'fresh')
    assert.equal(refreshed, 1)
  })
})
