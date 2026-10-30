import assert from 'node:assert/strict'
import test from 'node:test'
import { singingPlaybackGap } from './singingHold'

test('keeps the singing pose while a track is switching', () => {
  assert.equal(singingPlaybackGap(true, false), 'active')
  assert.equal(singingPlaybackGap(true, true), 'active')
  assert.equal(singingPlaybackGap(false, true), 'hold')
  assert.equal(singingPlaybackGap(false, false), 'stop')
})
