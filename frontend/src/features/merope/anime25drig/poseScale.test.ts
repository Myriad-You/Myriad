import assert from 'node:assert/strict'
import test from 'node:test'
import { bodyLeanShare } from './poseScale'

test('the canvas cut takes none of the lean and the shoulders take all of it', () => {
  assert.equal(bodyLeanShare(100, 100, 60), 0)
  assert.equal(bodyLeanShare(130, 100, 60), 0)
  assert.equal(bodyLeanShare(40, 100, 60), 1)
  assert.equal(bodyLeanShare(0, 100, 60), 1)
  let previous = 0
  for (let y = 100; y >= 40; y -= 1) {
    const share = bodyLeanShare(y, 100, 60)
    assert.ok(share >= previous)
    previous = share
  }
})

test('no bend height is a rigid lean', () => {
  assert.equal(bodyLeanShare(100, 100, 0), 1)
})
