import assert from 'node:assert/strict'
import test from 'node:test'
import { completeBehaviorQuality } from './behaviorMotion'
import { CoSpeechExpressionController } from './speechExpression'

function envelopeOnly(
  expression: CoSpeechExpressionController,
  phraseActivity: number,
  browAccent: number,
  headAccent: number,
) {
  return expression.sample(
    0,
    false,
    null,
    phraseActivity,
    browAccent,
    headAccent,
  )
}

test('keeps co-speech expression neutral without a speech envelope', () => {
  assert.deepEqual(
    { ...envelopeOnly(new CoSpeechExpressionController(), 0, 0, 0) },
    { brow: 0, eyeOpen: 0, angleY: 0, angleZ: 0, body: 0 },
  )
})

test('carries a speech beat through face, head, and torso', () => {
  const offset = {
    ...envelopeOnly(new CoSpeechExpressionController(), 1, 1, 1),
  }
  assert.equal(offset.brow, 0.095)
  assert.ok(offset.eyeOpen < 0)
  assert.ok(Math.abs(offset.eyeOpen) < 0.01)
  assert.equal(offset.angleY, 0.09)
  assert.ok(Math.abs(offset.angleZ) > 0.01)
  assert.ok(Math.abs(offset.body) > 0.07)
  assert.ok(Math.abs(offset.body) < 0.15)
})

test('behavior quality changes conversational timing and weight transfer', () => {
  const restrained = new CoSpeechExpressionController().sample(
    0,
    false,
    null,
    1,
    1,
    1,
    completeBehaviorQuality({
      tempo: 0.8,
      fluidity: 1.2,
      directness: 1.1,
      rebound: 0.1,
      asymmetry: 0.05,
      density: 0.4,
    }),
  )
  const open = new CoSpeechExpressionController().sample(
    0,
    false,
    null,
    1,
    1,
    1,
    completeBehaviorQuality({
      tempo: 1.2,
      fluidity: 0.7,
      directness: 0.55,
      rebound: 1.1,
      asymmetry: 1.2,
      density: 1.4,
    }),
  )
  assert.ok(Math.abs(open.angleZ) > Math.abs(restrained.angleZ))
  assert.ok(Math.abs(open.body) > Math.abs(restrained.body))
  assert.ok(open.brow > restrained.brow)
})

test('sanitizes unusable inputs and reuses its frame result', () => {
  const expression = new CoSpeechExpressionController()
  const first = envelopeOnly(expression, Number.NaN, -1, 2)
  assert.equal(first.brow, 0)
  assert.equal(first.eyeOpen, 0)
  assert.equal(first.angleY, 0.09)
  assert.ok(Number.isFinite(first.angleZ))
  assert.ok(Math.abs(first.body) > 0)
  assert.equal(first, envelopeOnly(expression, 0.5, 0.5, 0.5))
})

test('derives a delayed visual beat from authored energy without frame allocation', () => {
  const expression = new CoSpeechExpressionController()
  const neutral = expression.sample(0, true, 0, 0, 0, 0)
  const onset = expression.sample(0.1, true, 0.8, 0, 0, 0)
  const browLead = { ...expression.sample(0.18, true, 0.8, 0, 0, 0) }
  const headFollow = { ...expression.sample(0.26, true, 0.8, 0, 0, 0) }

  assert.equal(neutral, onset)
  assert.ok(browLead.brow > 0.04)
  assert.equal(browLead.angleY, 0)
  assert.ok(headFollow.angleY > 0)
  const releaseStart = { ...expression.sample(0.2, false, null, 0, 0, 0) }
  assert.ok(releaseStart.brow > 0)
  assert.ok(releaseStart.brow <= headFollow.brow + 1e-6)
  const mid = { ...expression.sample(0.32, false, null, 0, 0, 0) }
  assert.ok(mid.brow < releaseStart.brow)
  let rest = mid
  for (let frame = 1; frame <= 48; frame += 1) {
    rest = { ...expression.sample(0.32 + frame / 60, false, null, 0, 0, 0) }
  }
  assert.ok(Math.abs(rest.brow) < 1e-3)
  assert.ok(Math.abs(rest.eyeOpen) < 1e-3)
  assert.ok(Math.abs(rest.angleY) < 1e-3)
  assert.ok(Math.abs(rest.angleZ) < 1e-3)
  assert.ok(Math.abs(rest.body) < 0.01)
})

test('anticipates known TTS emphasis instead of waiting for the loudness edge', () => {
  const expression = new CoSpeechExpressionController()
  expression.setProsody(
    {
      utteranceId: 'utt-1',
      startedAtMs: 1_000,
      durationMs: 1_000,
      accents: [{ offsetMs: 400, intensity: 0.8 }],
    },
    0,
    1_000,
  )
  expression.sample(0.3, true, 0.2, 0, 0, 0)
  const preparation = {
    ...expression.sample(0.36, true, 0.2, 0, 0, 0),
  }
  const stroke = { ...expression.sample(0.4, true, 0.2, 0, 0, 0) }
  assert.ok(preparation.brow > 0)
  assert.ok(stroke.brow > preparation.brow)
})

test('a suppressed TTS accent cannot reappear as a generic head beat', () => {
  const expression = new CoSpeechExpressionController()
  expression.setProsody(
    {
      utteranceId: 'restrained',
      startedAtMs: 0,
      durationMs: 1_000,
      accents: [{ offsetMs: 400, intensity: 1, gesture: 'none' }],
    },
    0,
    0,
  )
  let largestPitch = 0
  for (let frame = 0; frame < 60; frame++) {
    const now = frame / 60
    const pose = expression.sample(now, true, now >= 0.35 ? 0.8 : 0.2, 0, 0, 0)
    largestPitch = Math.max(largestPitch, Math.abs(pose.angleY))
  }
  assert.equal(largestPitch, 0)
})

test('suppressing one accent preserves later emphasis and conversational activity', () => {
  const expression = new CoSpeechExpressionController()
  expression.setProsody(
    {
      utteranceId: 'mixed',
      startedAtMs: 0,
      durationMs: 1_500,
      accents: [
        { offsetMs: 400, intensity: 1, gesture: 'none' },
        { offsetMs: 1_000, intensity: 0.8 },
      ],
    },
    0,
    0,
  )
  let earlyPitch = 0
  let laterPitch = 0
  let activity = 0
  for (let frame = 0; frame < 90; frame++) {
    const now = frame / 60
    const pose = expression.sample(now, true, 0.7, 0, 0, 0)
    if (now < 0.8) earlyPitch = Math.max(earlyPitch, Math.abs(pose.angleY))
    else laterPitch = Math.max(laterPitch, Math.abs(pose.angleY))
    activity = Math.max(activity, Math.abs(pose.body))
  }
  assert.equal(earlyPitch, 0)
  assert.ok(laterPitch > 0.05)
  assert.ok(activity > 0.02)
})

test('sustained speech shifts weight smoothly instead of holding a frozen torso', () => {
  const expression = new CoSpeechExpressionController()
  let previous = { ...expression.sample(0, true, null, 1, 0, 0) }
  let minimumBody = previous.body
  let maximumBody = previous.body
  let minimumRoll = previous.angleZ
  let maximumRoll = previous.angleZ
  for (let frame = 1; frame <= 60 * 12; frame += 1) {
    const current = {
      ...expression.sample(frame / 60, true, null, 1, 0, 0),
    }
    minimumBody = Math.min(minimumBody, current.body)
    maximumBody = Math.max(maximumBody, current.body)
    minimumRoll = Math.min(minimumRoll, current.angleZ)
    maximumRoll = Math.max(maximumRoll, current.angleZ)
    assert.ok(Math.abs(current.body - previous.body) < 0.01)
    assert.ok(Math.abs(current.angleZ - previous.angleZ) < 0.01)
    previous = current
  }
  assert.ok(minimumBody < -0.03)
  assert.ok(maximumBody > 0.03)
  assert.ok(maximumBody - minimumBody > 0.04)
  assert.ok(minimumRoll < -0.02)
  assert.ok(maximumRoll > 0.02)
  assert.ok(maximumRoll - minimumRoll > 0.07)
})
