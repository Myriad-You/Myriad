import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  presenceEqual,
  presenceNextDrop,
  reconcilePresence,
} from './agentPresenceState'

const keyOf = (item: { id: string }) => item.id

describe('reconcilePresence', () => {
  it('mounts a first list as in when duration is zero', () => {
    const next = reconcilePresence([], [{ id: 'a' }, { id: 'b' }], keyOf, 0, 0)
    assert.deepEqual(
      next.map((entry) => [entry.key, entry.phase]),
      [
        ['a', 'in'],
        ['b', 'in'],
      ],
    )
  })

  it('mounts newly arrived items as in so CSS starting-style can play', () => {
    const prev = reconcilePresence([], [{ id: 'a' }], keyOf, 0, 0)
    const next = reconcilePresence(
      prev,
      [{ id: 'a' }, { id: 'b' }],
      keyOf,
      10,
      400,
    )
    assert.deepEqual(
      next.map((entry) => [entry.key, entry.phase]),
      [
        ['a', 'in'],
        ['b', 'in'],
      ],
    )
  })

  it('keeps a removed item as out until its window ends', () => {
    const prev = reconcilePresence([], [{ id: 'a' }, { id: 'b' }], keyOf, 0, 0)
    const next = reconcilePresence(prev, [{ id: 'b' }], keyOf, 100, 400)
    assert.deepEqual(
      next.map((entry) => [entry.key, entry.phase, entry.until]),
      [
        ['a', 'out', 500],
        ['b', 'in', 0],
      ],
    )
  })

  it('drops an out item after until', () => {
    const prev = reconcilePresence([], [{ id: 'a' }, { id: 'b' }], keyOf, 0, 0)
    const leaving = reconcilePresence(prev, [{ id: 'b' }], keyOf, 100, 400)
    const gone = reconcilePresence(leaving, [{ id: 'b' }], keyOf, 500, 400)
    assert.deepEqual(
      gone.map((entry) => entry.key),
      ['b'],
    )
  })

  it('inserts a new item at its incoming seat', () => {
    const prev = reconcilePresence([], [{ id: 'a' }, { id: 'c' }], keyOf, 0, 0)
    const next = reconcilePresence(
      prev,
      [{ id: 'a' }, { id: 'b' }, { id: 'c' }],
      keyOf,
      10,
      400,
    )
    assert.deepEqual(
      next.map((entry) => [entry.key, entry.phase]),
      [
        ['a', 'in'],
        ['b', 'in'],
        ['c', 'in'],
      ],
    )
  })

  it('updates the payload of a still-present key', () => {
    const first = { id: 'a', n: 1 }
    const second = { id: 'a', n: 2 }
    const prev = reconcilePresence([], [first], keyOf, 0, 0)
    const next = reconcilePresence(prev, [second], keyOf, 20, 400)
    assert.equal(next[0].item, second)
    assert.equal(next[0].phase, 'in')
  })

  it('does not keep outgoing items when duration is zero', () => {
    const prev = reconcilePresence([], [{ id: 'a' }], keyOf, 0, 0)
    const next = reconcilePresence(prev, [], keyOf, 10, 0)
    assert.deepEqual(next, [])
  })
})

describe('presenceEqual', () => {
  it('requires the same item reference', () => {
    const item = { id: 'a' }
    const a = reconcilePresence([], [item], keyOf, 0, 400)
    const b = reconcilePresence([], [item], keyOf, 0, 400)
    assert.equal(presenceEqual(a, b), true)
    assert.equal(
      presenceEqual(a, reconcilePresence([], [{ id: 'a' }], keyOf, 0, 400)),
      false,
    )
  })
})

describe('presenceNextDrop', () => {
  it('is null when nothing is leaving', () => {
    const entries = reconcilePresence([], [{ id: 'a' }], keyOf, 0, 400)
    assert.equal(presenceNextDrop(entries, 0), null)
  })

  it('returns the wait until the soonest out item drops', () => {
    const prev = reconcilePresence([], [{ id: 'a' }], keyOf, 0, 0)
    const leaving = reconcilePresence(prev, [], keyOf, 100, 400)
    assert.equal(presenceNextDrop(leaving, 100), 400)
    assert.equal(presenceNextDrop(leaving, 300), 200)
  })
})
