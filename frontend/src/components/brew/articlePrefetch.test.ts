import assert from 'node:assert/strict'
import { it } from 'node:test'
import {
  articlePrefetchGeneration,
  cancelArticlePrefetch,
  prefetchArticleDetails,
  selectPrefetchIds,
  setArticlePrefetchLoader,
} from './articlePrefetch'

it('caps prefetch ids to one unique positive article', () => {
  assert.deepEqual(selectPrefetchIds([2, 2, 3, 0, -1, 4.5, 5]), [2])
  assert.deepEqual(selectPrefetchIds([], 2), [])
  assert.deepEqual(selectPrefetchIds([8, 9], 2), [8, 9])
})

it('loads only the capped target and ignores later work after cancel', async () => {
  const loaded: number[] = []
  setArticlePrefetchLoader(async (id) => {
    loaded.push(id)
  })
  prefetchArticleDetails([11, 12, 13])
  const after = articlePrefetchGeneration()
  cancelArticlePrefetch()
  prefetchArticleDetails([14])
  await Promise.resolve()
  assert.deepEqual(loaded, [11, 14])
  assert.notEqual(articlePrefetchGeneration(), after)
  setArticlePrefetchLoader(null)
})

it('aborts the in-flight prefetch when cancelled or replaced', async () => {
  let seen: AbortSignal | undefined
  setArticlePrefetchLoader(async (_id, signal) => {
    seen = signal
  })
  prefetchArticleDetails([11])
  await Promise.resolve()
  assert.equal(seen?.aborted, false)
  cancelArticlePrefetch()
  assert.equal(seen?.aborted, true)
  prefetchArticleDetails([12])
  const next = seen
  await Promise.resolve()
  prefetchArticleDetails([13])
  assert.equal(next?.aborted, true)
  setArticlePrefetchLoader(null)
})
