import assert from 'node:assert/strict'
import test from 'node:test'
import { installFigureHost } from '../figureHost'
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

test('stale generations are reported to the host trace, not the run hub', () => {
  const drops: string[] = []
  installFigureHost({
    trace: { mark() {}, drop: reason => drops.push(reason), leaseExpiry() {}, frame() {} },
  })
  setLiveMotionGeneration(4)
  assert.equal(acceptLiveMotionGeneration(4), true)
  assert.equal(acceptLiveMotionGeneration(3), false)
  assert.deepEqual(drops, ['stale_generation'])
  setLiveMotionGeneration(0)
  installFigureHost({})
})
