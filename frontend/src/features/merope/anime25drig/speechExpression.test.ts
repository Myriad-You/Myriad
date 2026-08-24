import assert from 'node:assert/strict'
import test from 'node:test'
import {
  CoSpeechExpressionController,
  coSpeechExpressionOffset,
} from './speechExpression'

test('keeps co-speech expression neutral without a speech envelope', () => {
  assert.deepEqual(
    { ...coSpeechExpressionOffset(0, 0, 0) },
    { brow: 0, eyeOpen: 0, angleY: 0 },
  )
})

test('adds a small bounded expression without owning the base pose', () => {
  const offset = { ...coSpeechExpressionOffset(1, 1, 1) }
  assert.equal(offset.brow, 0.095)
  assert.ok(offset.eyeOpen < 0)
  assert.ok(Math.abs(offset.eyeOpen) < 0.01)
  assert.equal(offset.angleY, 0.035)
})

test('sanitizes unusable inputs and reuses its frame result', () => {
  const first = coSpeechExpressionOffset(Number.NaN, -1, 2)
  assert.equal(first.brow, 0)
  assert.equal(first.eyeOpen, 0)
  assert.equal(first.angleY, 0.035)
  assert.equal(first, coSpeechExpressionOffset(0.5, 0.5, 0.5))
})

test('derives a delayed visual beat from authored energy without frame allocation', () => {
  const expression = new CoSpeechExpressionController()
  const neutral = expression.sample(0, true, 0, 0, 0, 0)
  const onset = expression.sample(0.1, true, 0.8, 0, 0, 0)
  const browLead = { ...expression.sample(0.14, true, 0.8, 0, 0, 0) }
  const headFollow = { ...expression.sample(0.18, true, 0.8, 0, 0, 0) }

  assert.equal(neutral, onset)
  assert.ok(browLead.brow > 0.04)
  assert.equal(browLead.angleY, 0)
  assert.ok(headFollow.angleY > 0)
  assert.deepEqual(
    { ...expression.sample(0.2, false, null, 0, 0, 0) },
    { brow: 0, eyeOpen: 0, angleY: 0 },
  )
})
