import assert from 'node:assert/strict'
import test from 'node:test'
import {
  animationCatchupSeconds,
  animationElapsedSeconds,
  animationSubstepCount,
  MAX_ANIMATION_CATCHUP_SECONDS,
  MAX_ANIMATION_STEP_SECONDS,
} from './frameClock'

test('keeps real elapsed time while bounding stable physics catch-up', () => {
  const elapsed = animationElapsedSeconds(0.8)
  const catchup = animationCatchupSeconds(elapsed)
  const steps = animationSubstepCount(catchup)
  assert.equal(elapsed, 0.8)
  assert.equal(catchup, MAX_ANIMATION_CATCHUP_SECONDS)
  assert.ok(catchup / steps <= MAX_ANIMATION_STEP_SECONDS)
})

test('common render rates advance without losing wall-clock time', () => {
  for (const elapsed of [1 / 30, 1 / 60, 1 / 120]) {
    const normalized = animationElapsedSeconds(elapsed)
    const catchup = animationCatchupSeconds(normalized)
    const steps = animationSubstepCount(catchup)
    assert.ok(Math.abs(normalized - elapsed) < 1e-12)
    assert.ok(catchup / steps <= MAX_ANIMATION_STEP_SECONDS)
  }
})
