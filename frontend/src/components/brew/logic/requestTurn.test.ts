import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { RequestCache } from '../../../utils/requestCache'
import { RequestTurn, unlessAborted } from './requestTurn'

function deferred<T>() {
  return Promise.withResolvers<T>()
}

describe('RequestTurn', () => {
  it('begin aborts the previous signal so a rename of locals still works', () => {
    const turns = new RequestTurn()
    const first = turns.begin()
    const second = turns.begin()
    assert.equal(first.aborted, true)
    assert.equal(second.aborted, false)
    turns.cancel()
    assert.equal(second.aborted, true)
  })

  it('unlessAborted skips apply after unmount cancel', () => {
    const turns = new RequestTurn()
    const signal = turns.begin()
    let wrote = false
    turns.cancel()
    assert.equal(
      unlessAborted(signal, () => {
        wrote = true
      }),
      false,
    )
    assert.equal(wrote, false)
  })

  it('later request B wins the cache when A started first but finishes last', async () => {
    const cache = new RequestCache()
    const turns = new RequestTurn()
    const first = deferred<string[]>()
    const second = deferred<string[]>()

    const signalA = turns.begin()
    const pendingA = first.promise.then((sources) => {
      unlessAborted(signalA, () => cache.set('brew:sources', sources, 30_000))
      return sources
    })

    const signalB = turns.begin()
    const pendingB = second.promise.then((sources) => {
      unlessAborted(signalB, () => cache.set('brew:sources', sources, 30_000))
      return sources
    })

    second.resolve(['B'])
    assert.deepEqual(await pendingB, ['B'])
    assert.deepEqual(cache.get('brew:sources'), ['B'])

    first.resolve(['A'])
    assert.deepEqual(await pendingA, ['A'])
    assert.deepEqual(cache.get('brew:sources'), ['B'])
  })

  it('cancelled turn does not write the cache after the response arrives', async () => {
    const cache = new RequestCache()
    const turns = new RequestTurn()
    const first = deferred<string[]>()
    const signal = turns.begin()
    const pending = first.promise.then((sources) => {
      unlessAborted(signal, () => cache.set('brew:sources', sources, 30_000))
      return sources
    })
    turns.cancel()
    first.resolve(['late'])
    assert.deepEqual(await pending, ['late'])
    assert.equal(cache.get('brew:sources'), null)
  })
})
