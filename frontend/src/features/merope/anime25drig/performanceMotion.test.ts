import type { CueIntent } from './performanceCueDefinitions'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { intentExpressionPatch } from './performanceCueDefinitions'
import {
  baselineDriverPatch,
  cueDurationMs,
  idleSpeechDriverPatch,
  performanceRestDriverPatch,
  scheduleBodyCues,
  scheduledBodyCueRemainingDurationMs,
} from './performanceMotion'

test('keeps semantic face ownership out of the non-manual energy patch', () => {
  const patch = baselineDriverPatch({
    expression: 'warm',
    posture: 'open',
    motionEnergy: 1.4,
    attention: 0.8,
  })
  assert.equal(patch.mouthOpen, undefined)
  assert.equal(patch.talk, undefined)
  assert.equal(patch.mouthForm, undefined)
  assert.equal(patch.brow, undefined)
  assert.equal(patch.eyeOpenL, undefined)
})

// writeBaselineOffset composes posture onto the pose every frame; a second
// copy on the driver would apply it twice.
test('leaves posture to the per-frame pose offset', () => {
  for (const posture of ['closed', 'neutral', 'open'] as const) {
    const patch = baselineDriverPatch({
      expression: 'steady',
      posture,
      motionEnergy: 1,
      attention: 1,
    })
    assert.equal('body' in patch, false)
    assert.equal('armY' in patch, false)
    assert.equal('armPos' in patch, false)
  }
})

test('carries motionEnergy into secondary motion', () => {
  const still = baselineDriverPatch({
    expression: 'steady',
    posture: 'neutral',
    motionEnergy: 0.2,
    attention: 1,
  })
  const lively = baselineDriverPatch({
    expression: 'steady',
    posture: 'neutral',
    motionEnergy: 1.4,
    attention: 1,
  })
  assert.ok((lively.physAmp ?? 0) > (still.physAmp ?? 0))
  assert.ok((lively.fhAmp ?? 0) > (still.fhAmp ?? 0))
  assert.ok((lively.soft ?? 0) > (still.soft ?? 0))
})

test('centers semantic hair energy on the runtime sway defaults', () => {
  const patch = baselineDriverPatch({
    expression: 'steady',
    posture: 'neutral',
    motionEnergy: 1,
    attention: 1,
  })
  assert.equal(patch.fhAmp, 1)
  assert.equal(patch.physAmp, 0.5)
})

test('restores performance pose without taking speech, face, or gaze ownership', () => {
  const patch = performanceRestDriverPatch(
    {
      expression: 'warm',
      posture: 'open',
      motionEnergy: 1,
      attention: 1,
    },
    false,
  )
  for (const key of [
    'talk',
    'mouthOpen',
    'mouthForm',
    'eyeOpenL',
    'eyeOpenR',
    'eyeX',
    'eyeY',
    'angleX',
    'angleY',
    'angleZ',
    'brow',
  ]) {
    assert.equal(key in patch, false)
  }
  assert.equal(patch.body, 0)
  assert.equal(performanceRestDriverPatch(null, true).body, 0)
  assert.equal(performanceRestDriverPatch(null, true).rand, false)
  assert.equal(performanceRestDriverPatch(null, true).thinking, true)
  assert.equal(performanceRestDriverPatch(null, true).idle, true)
  assert.equal(performanceRestDriverPatch(null, true).blink, true)
  assert.equal(performanceRestDriverPatch(null, true).phys, true)
  assert.equal(
    performanceRestDriverPatch(
      {
        expression: 'warm',
        posture: 'open',
        motionEnergy: 1,
        attention: 1,
      },
      true,
    ).rand,
    false,
  )
})

test('keeps active speech channels out of a full base refresh', () => {
  assert.deepEqual(idleSpeechDriverPatch(70, true), {})
  assert.deepEqual(idleSpeechDriverPatch(70, false), {
    talk: false,
    mouthOpen: 0,
    mouthForm: 0.07,
  })
})

test('maps cue forms to bounded deterministic poses and durations', () => {
  const patch = (intent: CueIntent, intensity = 1.4) =>
    intentExpressionPatch(intent, intensity)
  assert.ok((patch('emphasize').body || 0) > 0.5)
  assert.ok((patch('emphasize').body || 0) < 0.6)
  assert.equal(
    cueDurationMs({
      intent: 'emphasize',
      atMs: 0,
      intensity: 1.4,
      tempo: 1,
      fadeInMs: 150,
      fadeOutMs: 220,
      interrupt: 'replace',
    }),
    1_090,
  )

  // Face-only forms write no body channel at all.
  for (const intent of ['dizzy', 'think', 'cry'] as const) {
    assert.equal(patch(intent).body, undefined, intent)
    assert.equal(patch(intent).armY, undefined, intent)
    assert.equal(patch(intent).armPos, undefined, intent)
  }
  assert.ok((patch('angry').body || 0) > 0)
})

test('a cue form never switches secondary physics off', () => {
  // Hair and cloth are bounded overlays, not a fifth channel a cue may own.
  // The sticker forms used to author `idle: false` here, which reached no
  // driver: the only reader takes it from the authored driver, not from a
  // realized behavior. Damping during a sticker is the ambient scale's job.
  const definitions = readFileSync(
    new URL('./performanceCueDefinitions.ts', import.meta.url),
    'utf8',
  )
  assert.doesNotMatch(definitions, /idle:/)
})

test('keeps directed body motion above idle scale at ordinary intensity', () => {
  const cue = (intent: CueIntent) => intentExpressionPatch(intent, 0.55)

  for (const intent of [
    'greet',
    'question',
    'delight',
    'emphasize',
    'notify',
    'angry',
    'speechless',
    'maniac',
    'silly',
    'lovestruck',
  ] as const) {
    const patch = cue(intent)
    const displacement = Math.max(
      Math.abs(patch.body ?? 0),
      Math.abs(patch.armY ?? 0),
      Math.abs(patch.armPos ?? 0),
      Math.abs(patch.bust ?? 0),
    )
    assert.ok(displacement >= 0.12, intent)
    assert.ok(displacement <= 0.5, intent)
  }
})

test('does not replay expired body cues after a throttled timer', () => {
  const cue = {
    intent: 'emphasize' as const,
    atMs: 2_000,
    intensity: 1,
    tempo: 1,
    fadeInMs: 120,
    fadeOutMs: 250,
    interrupt: 'replace' as const,
  }
  const duration = cueDurationMs(cue)
  const scheduled = { cue, startMs: 5_000, endMs: 5_000 + duration }
  assert.equal(scheduledBodyCueRemainingDurationMs(scheduled, 4_900), duration)
  assert.equal(
    scheduledBodyCueRemainingDurationMs(scheduled, 5_300),
    duration - 300,
  )
  assert.equal(
    scheduledBodyCueRemainingDurationMs(scheduled, 5_000 + duration),
    0,
  )
})

test('precomputes queued body timing independently of delayed callbacks', () => {
  const first = {
    intent: 'question' as const,
    atMs: 0,
    intensity: 1,
    tempo: 1,
    fadeInMs: 100,
    fadeOutMs: 200,
    interrupt: 'replace' as const,
  }
  const queued = {
    ...first,
    intent: 'notify' as const,
    interrupt: 'queue' as const,
  }
  const [scheduledFirst, scheduledQueued] = scheduleBodyCues(
    [first, queued],
    5_000,
  )
  assert.equal(scheduledFirst?.startMs, 5_000)
  assert.equal(scheduledQueued?.startMs, scheduledFirst?.endMs)
  assert.ok(
    scheduledBodyCueRemainingDurationMs(
      scheduledQueued!,
      (scheduledFirst?.endMs ?? 0) + 100,
    ) > 0,
  )
})

test('queues after the surviving replacement rather than a truncated cue', () => {
  const long = {
    intent: 'respond' as const,
    atMs: 0,
    intensity: 1,
    tempo: 0.5,
    fadeInMs: 100,
    fadeOutMs: 200,
    interrupt: 'replace' as const,
  }
  const replacement = {
    ...long,
    intent: 'notify' as const,
    atMs: 200,
    tempo: 1,
  }
  const queued = {
    ...long,
    intent: 'question' as const,
    atMs: 300,
    tempo: 1,
    interrupt: 'queue' as const,
  }
  const [truncated, active, after] = scheduleBodyCues(
    [long, replacement, queued],
    1_000,
  )
  assert.equal(truncated?.endMs, active?.startMs)
  assert.equal(after?.startMs, active?.endMs)
  assert.equal(
    truncated ? scheduledBodyCueRemainingDurationMs(truncated, 1_250) : -1,
    0,
  )
})
