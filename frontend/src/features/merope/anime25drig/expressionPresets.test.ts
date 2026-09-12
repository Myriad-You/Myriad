import assert from 'node:assert/strict'
import test from 'node:test'
import { IDENTITY_DRIVER } from './driver'
import {
  activityExpressionDriverPatch,
  CRY_EXPRESSION_PRESET,
  DIZZY_EXPRESSION_PRESET,
  LOVESTRUCK_EXPRESSION_PRESET,
  MANIAC_EXPRESSION_PRESET,
  releaseThinkingExpression,
  SQUEEZE_EXPRESSION_PRESET,
  THINKING_ACTIVITY_EXPRESSION,
  THINKING_EXPRESSION_PRESET,
} from './expressionPresets'

test('thinking activity owns face and gaze without taking speech channels', () => {
  const thinking = activityExpressionDriverPatch(true)
  assert.equal(thinking, THINKING_ACTIVITY_EXPRESSION)
  assert.ok(thinking.eyeOpenL >= 0.7 && thinking.eyeOpenL < 0.85)
  assert.ok(thinking.eyeOpenR > thinking.eyeOpenL && thinking.eyeOpenR < 1)
  assert.ok(thinking.irisScale >= 0.9 && thinking.irisScale < 1)
  assert.ok(thinking.browAngL - thinking.browAngR > 0.5)
  assert.ok(Math.abs(thinking.eyeX) > 0.5)
  assert.ok(thinking.eyeY < -0.35)
  assert.ok(Math.abs(thinking.angleZ) > 0.15)
  assert.ok(thinking.brow > 0.15)
  assert.equal(Object.hasOwn(thinking, 'mouthOpen'), false)
  assert.equal(Object.hasOwn(thinking, 'mouthForm'), false)
  assert.equal(Object.hasOwn(thinking, 'talk'), false)
})

test('leaving thinking resets every activity-owned expression channel', () => {
  const neutral = activityExpressionDriverPatch(false)
  assert.deepEqual(
    Object.keys(neutral).toSorted(),
    Object.keys(THINKING_ACTIVITY_EXPRESSION).toSorted(),
  )
  assert.equal(neutral.eyeOpenL, 1)
  assert.equal(neutral.eyeOpenR, 1)
  assert.equal(neutral.eyeDizzy, 0)
  assert.equal(neutral.eyeSqueeze, 0)
  assert.equal(neutral.eyeCry, 0)
  assert.equal(neutral.maniac, 0)
  assert.equal(neutral.lovestruck, 0)
  assert.equal(neutral.irisScale, 1)
  for (const key of [
    'angleX',
    'angleY',
    'angleZ',
    'eyeX',
    'eyeY',
    'brow',
    'browAngL',
    'browAngR',
    'browAngSym',
  ] as const) {
    assert.equal(neutral[key], 0)
  }
})

test('dizzy preview owns only the dedicated artwork replacement channel', () => {
  assert.deepEqual(DIZZY_EXPRESSION_PRESET, { eyeDizzy: 1 })
})

test('squeeze preview owns only the inward chevron artwork channel', () => {
  assert.deepEqual(SQUEEZE_EXPRESSION_PRESET, { eyeSqueeze: 1 })
})

test('cry preview combines its own artwork with sad symmetric brows', () => {
  assert.equal(CRY_EXPRESSION_PRESET.eyeCry, 1)
  assert.ok((CRY_EXPRESSION_PRESET.brow || 0) > 0.2)
  assert.ok((CRY_EXPRESSION_PRESET.browAngSym || 0) < -0.3)
  assert.equal(CRY_EXPRESSION_PRESET.eyeSqueeze, undefined)
})

test('maniac preview selects its dedicated mouth while retaining source eyes', () => {
  assert.equal(MANIAC_EXPRESSION_PRESET.maniac, 1)
  assert.equal(MANIAC_EXPRESSION_PRESET.eyeDizzy, undefined)
  assert.equal(MANIAC_EXPRESSION_PRESET.eyeCry, undefined)
})

test('lovestruck preview selects the additive face expression', () => {
  assert.equal(LOVESTRUCK_EXPRESSION_PRESET.lovestruck, 1)
  assert.equal(LOVESTRUCK_EXPRESSION_PRESET.silly, 0)
  assert.equal(LOVESTRUCK_EXPRESSION_PRESET.eyeOpenL, 1)
  assert.equal(LOVESTRUCK_EXPRESSION_PRESET.eyeOpenR, 1)
})

test('thinking preview enables the dedicated motion loop', () => {
  assert.equal(THINKING_EXPRESSION_PRESET.thinking, true)
})

test('speech releases authored thinking face without erasing newer emotion or articulation', () => {
  for (const preset of [THINKING_ACTIVITY_EXPRESSION, THINKING_EXPRESSION_PRESET]) {
    const target = { ...IDENTITY_DRIVER, ...preset, thinking: true,
      anger: 0.8, mouthOpen: 0.7, talk: true, brow: -0.4 }
    releaseThinkingExpression(target)
    assert.equal(target.thinking, false)
    assert.equal(target.eyeX, 0)
    assert.equal(target.eyeY, 0)
    assert.equal(target.eyeOpenL, 1)
    assert.equal(target.browAngL, 0)
    assert.equal(target.anger, 0.8)
    assert.equal(target.brow, -0.4)
    assert.equal(target.mouthOpen, 0.7)
    assert.equal(target.talk, true)
    const released = { ...target }
    releaseThinkingExpression(target)
    assert.deepEqual(target, released)
  }
})
