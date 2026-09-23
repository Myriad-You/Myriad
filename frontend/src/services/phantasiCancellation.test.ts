import assert from 'node:assert/strict'
import { it } from 'node:test'
import { loadBoardNotes, loadFeedStories, loadNoteDocs } from '../components/phantasi/pageData'
import { requestCache } from '../utils/requestCache'
import { getCategories, getItem, getSources, getStats, listSubscriptionTopicCatalog } from './phantasiApi'

it('cached journal reads stop their transport when the last caller leaves', async () => {
  const original = globalThis.fetch
  const signals: AbortSignal[] = []
  globalThis.fetch = async (_input, init) => {
    const signal = init!.signal!
    signals.push(signal)
    return new Promise<Response>((_resolve, reject) => {
      signal.addEventListener('abort', () => reject(signal.reason), { once: true })
    })
  }
  try {
    const reads = [
      (signal: AbortSignal) => getItem(99181, undefined, { signal }),
      (signal: AbortSignal) => getSources(undefined, { signal }),
      (signal: AbortSignal) => getCategories(undefined, { signal }),
      (signal: AbortSignal) => getStats(undefined, { signal }),
      (signal: AbortSignal) => listSubscriptionTopicCatalog(undefined, { signal }),
      (signal: AbortSignal) => loadFeedStories(99182, 1, signal),
      (signal: AbortSignal) => loadNoteDocs(signal),
      (signal: AbortSignal) => loadBoardNotes([{ id: 99183, source_type: 'note' }], signal),
    ]
    for (const read of reads) {
      requestCache.clear()
      const owner = new AbortController()
      const first = read(owner.signal)
      // The shared client dispatches after its async pre-flight checks.
      await new Promise(resolve => setImmediate(resolve))
      const transport = signals.at(-1)!
      owner.abort()
      await assert.rejects(first, { name: 'AbortError' })
      assert.equal(transport.aborted, true)
    }
    assert.equal(signals.length, reads.length)
  } finally {
    globalThis.fetch = original
    requestCache.clear()
  }
})

it('reader and prefetch share one download without sharing cancellation ownership', async () => {
  const original = globalThis.fetch
  const response = Promise.withResolvers<Response>()
  let calls = 0
  let transport!: AbortSignal
  globalThis.fetch = async (_input, init) => {
    calls++
    transport = init!.signal!
    return response.promise
  }
  try {
    requestCache.clear()
    const prefetch = new AbortController()
    const reader = new AbortController()
    const warm = getItem(99184, undefined, { signal: prefetch.signal })
    const open = getItem(99184, undefined, { signal: reader.signal })
    prefetch.abort()
    await assert.rejects(warm, { name: 'AbortError' })
    assert.equal(transport.aborted, false)
    response.resolve(Response.json({ item: { id: 99184, content: '<p>Full body</p>' } }))
    assert.equal((await open).content, '<p>Full body</p>')
    await getItem(99184)
    assert.equal(calls, 1)
  } finally {
    globalThis.fetch = original
    requestCache.clear()
  }
})
