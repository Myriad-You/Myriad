import type { TouchObservation } from './touchGesture'
import assert from 'node:assert/strict'
import test from 'node:test'
import { touchExpressionPatch } from '../anime25drig/touchExpression'
import { selectTouchReaction } from './touchReaction'

const touch: TouchObservation = {
  id: 1, phase: 'update', gesture: 'stroke', region: 'hair',
  durationMs: 800, distance: 0.3, speed: 0.3, repeatCount: 0, x: 0.4, y: 0.2,
}
test('touch appraisal distinguishes body region and affect without forced happiness', () => {
  assert.equal(selectTouchReaction(touch, 70, 48), 'accept')
  assert.equal(selectTouchReaction(touch, 30, 30), 'hesitate')
  assert.equal(selectTouchReaction(touch, 30, 80), 'hesitate')
  assert.equal(selectTouchReaction({ ...touch, region: 'face' }, 70, 48), 'hesitate')
  assert.equal(selectTouchReaction({ ...touch, region: 'body' }, 70, 48), 'notice')
  assert.equal(selectTouchReaction({ ...touch, region: 'accessory' }, 70, 48), 'hesitate')
})
test('repeated face taps can withdraw, but speed alone is not treated as force', () => {
  assert.equal(selectTouchReaction({ ...touch, gesture: 'tap', region: 'face', repeatCount: 3 }, 70, 48), 'withdraw')
  assert.equal(selectTouchReaction({ ...touch, gesture: 'contact', speed: 100 }, 70, 48), 'notice')
})
test('a completed click has a regional expression without waiting for a model', () => {
  assert.equal(selectTouchReaction({ ...touch, gesture: 'tap', region: 'hair', repeatCount: 1 }, 70, 48), 'accept')
  assert.equal(selectTouchReaction({ ...touch, gesture: 'tap', region: 'face', repeatCount: 1 }, 70, 48), 'hesitate')
  assert.equal(selectTouchReaction({ ...touch, gesture: 'tap', region: 'face', repeatCount: 3 }, 70, 48), 'withdraw')
  assert.equal(selectTouchReaction({ ...touch, gesture: 'tap', region: 'hair', repeatCount: 1 }, 20, 70), 'hesitate')
  assert.equal(selectTouchReaction({ ...touch, gesture: 'contact', region: 'hair', repeatCount: 1 }, 70, 48), 'accept')
  assert.equal(selectTouchReaction({ ...touch, gesture: 'hold', region: 'face', repeatCount: 3 }, 70, 48), 'withdraw')
})
test('touch poses do not write mouth, special eyes, arms or bust', () => {
  for (const form of ['notice', 'accept', 'hesitate', 'withdraw'] as const) {
    const patch = touchExpressionPatch(form, 1)
    for (const forbidden of ['mouthForm', 'eyeCry', 'eyeDizzy', 'eyeSqueeze', 'armY', 'armPos', 'bust']) {
      assert.equal(Object.hasOwn(patch, forbidden), false)
    }
    assert.ok(Object.values(patch).every(Number.isFinite))
  }
})
