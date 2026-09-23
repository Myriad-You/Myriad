import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { createStore, patchStore } from './store'

describe('createStore', () => {
  it('publishes only real changes, so snapshots stay identical between them', () => {
    const store = createStore({ n: 1 })
    let calls = 0
    const stop = store.subscribe(() => { calls++ })
    const first = store.get()
    store.set(first)
    assert.equal(calls, 0)
    store.set(current => ({ n: current.n + 1 }))
    assert.equal(calls, 1)
    assert.notEqual(store.get(), first)
    stop()
    store.set({ n: 9 })
    assert.equal(calls, 1)
  })

  it('accepts a custom equality', () => {
    const store = createStore([1, 2], (a, b) => a.join() === b.join())
    let calls = 0
    store.subscribe(() => { calls++ })
    store.set([1, 2])
    assert.equal(calls, 0)
    store.set([2, 1])
    assert.equal(calls, 1)
  })
})

describe('patchStore', () => {
  it('keeps the snapshot when a patch changes nothing', () => {
    const store = createStore({ a: 1, b: 'x' })
    const before = store.get()
    let calls = 0
    store.subscribe(() => { calls++ })
    assert.equal(patchStore(store, { a: 1 }), before)
    assert.equal(calls, 0)
    assert.deepEqual(patchStore(store, { b: 'y' }), { a: 1, b: 'y' })
    assert.equal(calls, 1)
  })
})
