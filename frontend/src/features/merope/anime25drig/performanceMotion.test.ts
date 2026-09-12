import type { CueIntent } from './performanceCueDefinitions'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { completeBehaviorQuality } from './behaviorMotion'
import { intentExpressionPatch } from './performanceCueDefinitions'
import { PerformanceExpressionController } from './performanceExpression'
import {
  authoredCueEnvelope,
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

test('directed thinking has a readable face without borrowing angry or speechless artwork', () => {
  const expression = new PerformanceExpressionController()
  expression.playBehaviorUnits(
    [
      {
        behaviorId: 'think-probe',
        family: 'performance',
        form: 'think',
        kind: 'state',
        intensity: 1,
        quality: completeBehaviorQuality(undefined),
        timing: {
          startMs: 0,
          readyMs: 100,
          strokeStartMs: 200,
          strokePeakMs: 400,
          strokeEndMs: 500,
          relaxMs: null,
          endMs: null,
        },
      },
    ],
    0,
    0,
  )
  const shown = expression.sample(1)
  assert.ok(expression.getThinkingLevel() > 0.5)
  assert.ok(shown.eyeOpen < -0.1)
  assert.ok(shown.brow > 0.2)
  assert.ok(shown.browAngSym < -0.1)
  assert.equal(shown.anger ?? 0, 0)
  assert.equal(shown.speechless ?? 0, 0)
  const beforeSpeech = { ...shown }
  assert.deepEqual(expression.sample(1, {}, true), beforeSpeech)
  const released = expression.sample(3, {}, true)
  assert.equal(expression.getThinkingLevel(), 0)
  assert.ok(Math.abs(released.eyeOpen) < 1e-6)
  assert.ok(Math.abs(released.browAngSym) < 1e-6)
})

test('leaves posture to the per-frame pose offset', () => {
  for (const posture of ['closed', 'neutral', 'open'] as const) {
    const patch = baselineDriverPatch({
      expression: 'steady',
      posture,
      motionEnergy: 1,
      attention: 1,
    })
    assert.equal(Object.hasOwn(patch, 'body'), false)
    assert.equal(Object.hasOwn(patch, 'armY'), false)
    assert.equal(Object.hasOwn(patch, 'armPos'), false)
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
  assert.ok((patch('emphasize').body || 0) > 0.8)
  assert.ok((patch('emphasize').body || 0) < 1)
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
    1_577,
  )

  // Face-only forms write no body channel at all.
  for (const intent of ['dizzy', 'think', 'cry'] as const) {
    assert.equal(patch(intent).body, undefined, intent)
    assert.equal(patch(intent).armY, undefined, intent)
    assert.equal(patch(intent).armPos, undefined, intent)
  }
  assert.ok((patch('angry').body || 0) > 0)
})

test('strong performances recruit body and arms without amplifying the head', () => {
  for (const intent of ['greet', 'delight', 'emphasize'] as const) {
    const gentle = intentExpressionPatch(intent, 0.5)
    const strong = intentExpressionPatch(intent, 1.4)
    const poseRatio = 1.4 / 0.89
    assert.ok((strong.body ?? 0) / (gentle.body ?? 1) > poseRatio * 1.45)
    for (const key of ['angleY', 'angleZ'] as const) {
      if (gentle[key]) {
        assert.ok(
          Math.abs((strong[key] ?? 0) / gentle[key]! - poseRatio) < 1e-8,
        )
      }
    }
    for (const key of ['body', 'armY', 'armPos'] as const) {
      assert.ok(Math.abs(strong[key] ?? 0) <= 1)
    }
  }
})

test('larger coordinated cues get travel time without delaying their scheduled start', () => {
  const base = {
    intent: 'greet' as const,
    atMs: 0,
    intensity: 0.5,
    tempo: 1,
    fadeInMs: 120,
    fadeOutMs: 200,
    interrupt: 'replace' as const,
  }
  const gentle = authoredCueEnvelope(base)
  const strong = authoredCueEnvelope({ ...base, intensity: 1.4 })
  assert.ok(strong.fadeIn > gentle.fadeIn)
  assert.ok(strong.fadeOut > gentle.fadeOut)
  assert.equal(strong.hold, gentle.hold)
  assert.equal(
    scheduleBodyCues([{ ...base, intensity: 1.4 }], 1000)[0].startMs,
    1000,
  )
  const listen = authoredCueEnvelope({ ...base, intent: 'listen' })
  assert.equal(listen.fadeIn, 0.12)
  assert.equal(listen.fadeOut, 0.2)
})

test('a cue form never switches secondary physics off', () => {
  // Hair and cloth are bounded overlays, not a fifth channel a cue may own.
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
