import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { __evaluateSampleForTest } from './animationAutoAdapt'

describe('animationAutoAdapt evaluateSample', () => {
  it('does not demote healthy 60fps-ish stream', () => {
    const intervals = Array.from({ length: 40 }, () => 16.5)
    const r = __evaluateSampleForTest(intervals)
    assert.equal(r.demoted, false)
  })

  it('does not demote mild occasional jank', () => {
    // 少数 20ms 尖刺，平均仍健康
    const intervals = Array.from({ length: 40 }, (_, i) =>
      i % 10 === 0 ? 22 : 16,
    )
    const r = __evaluateSampleForTest(intervals)
    assert.equal(r.demoted, false)
  })

  it('demotes only when clearly bad', () => {
    // 大量 40ms+ 与若干 50ms+，平均也差
    const intervals = Array.from({ length: 40 }, (_, i) =>
      i % 3 === 0 ? 55 : 40,
    )
    const r = __evaluateSampleForTest(intervals)
    assert.equal(r.demoted, true)
    assert.ok(r.severeCount >= 4)
    assert.ok(r.badRatio >= 0.45)
  })
})
