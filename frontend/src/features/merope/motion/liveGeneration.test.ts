import assert from 'node:assert/strict'
import test from 'node:test'
import { resetTurnTraceForTest, snapshotTurnTrace } from '../turnTrace'
import {
  acceptLiveMotionGeneration,
  isLiveMotionGeneration,
  liveMotionGeneration,
  newMotionIntentId,
  setLiveMotionGeneration,
} from './liveGeneration'

test('live generation is in-memory and missing values stay compatible', () => {
  setLiveMotionGeneration(4)
  assert.equal(liveMotionGeneration(), 4)
  assert.equal(isLiveMotionGeneration(4), true)
  assert.equal(isLiveMotionGeneration(3), false)
  assert.equal(isLiveMotionGeneration(undefined), true)
  const first = newMotionIntentId()
  const second = newMotionIntentId()
  assert.ok(first.startsWith('motion-'))
  assert.notEqual(first, second)
  setLiveMotionGeneration(0)
  assert.equal(liveMotionGeneration(), 0)
})

test('stale generations are counted on the local trace, not the run hub', () => {
  resetTurnTraceForTest()
  setLiveMotionGeneration(4)
  assert.equal(acceptLiveMotionGeneration(4), true)
  assert.equal(acceptLiveMotionGeneration(3), false)
  assert.equal(snapshotTurnTrace().counters.staleGenerationDrops, 1)
  setLiveMotionGeneration(0)
})
