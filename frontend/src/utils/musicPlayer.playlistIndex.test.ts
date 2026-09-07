import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import {
  clampSeekTime,
  pickAdjacentIndex,
  pickShuffleIndex,
} from './musicPlayer.ts'

const songs = [
  { isVip: false },
  { isVip: true },
  { isVip: false },
  { isVip: true },
]

describe('pickAdjacentIndex', () => {
  it('wraps forward and backward without VIP skip', () => {
    assert.equal(pickAdjacentIndex(songs, 0, 1, false), 1)
    assert.equal(pickAdjacentIndex(songs, 3, 1, false), 0)
    assert.equal(pickAdjacentIndex(songs, 0, -1, false), 3)
  })

  it('skips VIP and returns null when every track is VIP', () => {
    assert.equal(pickAdjacentIndex(songs, 0, 1, true), 2)
    assert.equal(pickAdjacentIndex(songs, 2, 1, true), 0)
    assert.equal(pickAdjacentIndex(songs, 0, -1, true), 2)
    assert.equal(
      pickAdjacentIndex([{ isVip: true }, { isVip: true }], 0, 1, true),
      null,
    )
  })
})

describe('pickShuffleIndex', () => {
  it('returns -1 for empty or single-track lists', () => {
    assert.equal(pickShuffleIndex([], 0, false), -1)
    assert.equal(pickShuffleIndex([{ isVip: false }], 0, false), -1)
  })

  it('never returns the current index when another non-VIP exists', () => {
    for (let i = 0; i < 20; i++) {
      const next = pickShuffleIndex(songs, 0, true)
      assert.equal(next, 2)
    }
  })
})

describe('clampSeekTime', () => {
  it('clamps to duration-1 for tracks longer than 1s', () => {
    assert.equal(clampSeekTime(-3, 200), 0)
    assert.equal(clampSeekTime(199.4, 200), 199)
    assert.equal(clampSeekTime(12, 200), 12)
  })

  it('uses 95% for sub-second duration', () => {
    assert.equal(clampSeekTime(1, 0.8), 0.8 * 0.95)
  })
})
