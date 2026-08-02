/**
 * RankList progressive window helpers.
 * @vitest-environment node
 */

import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  clampRankInitialLoad,
  DEFAULT_RANK_LOAD_COUNT,
  nextRankVisibleCount,
} from './RankList.tsx'

describe('clampRankInitialLoad', () => {
  it('loads up to 30 by default, capped by total', () => {
    assert.equal(DEFAULT_RANK_LOAD_COUNT, 30)
    assert.equal(clampRankInitialLoad(undefined, 100), 30)
    assert.equal(clampRankInitialLoad(30, 12), 12)
    assert.equal(clampRankInitialLoad(30, 100), 30)
  })

  it('falls back on invalid initialCount', () => {
    assert.equal(clampRankInitialLoad(0, 50), 30)
    assert.equal(clampRankInitialLoad(-5, 50), 30)
    assert.equal(clampRankInitialLoad('nope', 50), 30)
  })
})

describe('nextRankVisibleCount', () => {
  it('on scroll end loads all remaining in one step', () => {
    assert.equal(nextRankVisibleCount(30, 100), 100)
    assert.equal(nextRankVisibleCount(30, 30), 30)
    assert.equal(nextRankVisibleCount(0, 0), 0)
    assert.equal(nextRankVisibleCount(12, 12), 12)
  })
})
