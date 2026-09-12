import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { computeShellScore } from './sanitizeTappPreview.ts'

describe('computeShellScore', () => {
  it('ignores decorated empty mounts when the document has text', () => {
    const score = computeShellScore({
      blockedNodeCount: 0,
      emptyMountCount: 61,
      bareEmptyMountCount: 0,
      hasText: true,
    })
    assert.ok(score < 120)
  })

  it('counts all empty mounts when the document has no text', () => {
    const score = computeShellScore({
      blockedNodeCount: 0,
      emptyMountCount: 61,
      bareEmptyMountCount: 0,
      hasText: false,
    })
    assert.ok(score >= 120)
  })

  it('counts bare empty mounts even when the document has text', () => {
    const score = computeShellScore({
      blockedNodeCount: 0,
      emptyMountCount: 60,
      bareEmptyMountCount: 60,
      hasText: true,
    })
    assert.equal(score, 120)
  })

  it('adds blocked node count to the score', () => {
    const score = computeShellScore({
      blockedNodeCount: 40,
      emptyMountCount: 40,
      bareEmptyMountCount: 40,
      hasText: true,
    })
    assert.equal(score, 120)
  })

  it('keeps text-rich previews with few bare mounts below the limit', () => {
    const score = computeShellScore({
      blockedNodeCount: 2,
      emptyMountCount: 30,
      bareEmptyMountCount: 3,
      hasText: true,
    })
    assert.ok(score < 120)
  })
})
