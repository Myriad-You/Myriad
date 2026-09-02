import assert from 'node:assert/strict'
import test from 'node:test'
import { RigMotionCoordinator } from './coordinator'
import {
  captureRigStateSummary,
  sanitizeRigStateSummary,
  semanticRigCapabilities,
} from './rigStateSummary'
import { MotionRuntime } from './runtime'

test('summary keeps only semantic fields and drops drivers', () => {
  const summary = sanitizeRigStateSummary({
    expression: 'warm',
    posture: 'open',
    acting: {
      intent: 'listen',
      phase: 'delivery',
      function: 'attend',
      lifecycle: 'holding',
      remainingMs: 800,
    },
    activeBehaviors: [
      {
        function: 'attend',
        lifecycle: 'holding',
        source: 'performance',
        resources: ['face.gaze', 'angleX'],
        remainingMs: 900,
      },
      {
        function: 'not-real',
        lifecycle: 'holding',
        source: 'performance',
        resources: [],
        remainingMs: 1,
      },
    ],
    owners: {
      mouth: 'speech',
      expression: 'coSpeech',
      gaze: 'ambient',
      headBody: 'music',
    },
    speaking: true,
    singing: true,
    musicPlaying: true,
    music: { energy: 'present', beat: 'downbeat' },
    capabilities: ['dizzy-eye', 'head-body', 'psd-layer'],
    recentIntents: ['delight', 'angleX'],
    motionStyle: 'restrained',
    pageVisible: true,
    faceVisible: true,
    angleX: 0.4,
    driver: { mouthOpen: 1 },
  })
  assert.ok(summary)
  const encoded = JSON.stringify(summary)
  assert.equal(encoded.includes('angleX'), false)
  assert.equal(encoded.includes('driver'), false)
  assert.equal(encoded.includes('mouthOpen'), false)
  assert.equal(summary?.owners.mouth, 'speech')
  assert.equal(summary?.owners.headBody, 'music')
  assert.equal(summary?.acting.function, 'attend')
  assert.equal(summary?.acting.lifecycle, 'holding')
  assert.deepEqual(summary?.activeBehaviors, [
    {
      function: 'attend',
      lifecycle: 'holding',
      source: 'performance',
      resources: ['face.gaze'],
      remainingMs: 900,
    },
  ])
  assert.equal(
    sanitizeRigStateSummary({
      owners: {
        mouth: 'angleX',
        expression: {},
        gaze: 'ambient',
        headBody: 'music',
      },
    })?.owners.mouth,
    'idle',
  )
  assert.deepEqual(summary?.capabilities, ['dizzy-eye', 'head-body'])
  assert.deepEqual(summary?.recentIntents, ['delight'])
})

test('capture samples the runtime once and never includes per-frame driver keys', () => {
  const runtime = new MotionRuntime(new RigMotionCoordinator())
  const release = runtime.retain()
  runtime.setCapabilities(['blink', 'head-body'])
  runtime.speech.handleForTest({
    phase: 'start',
    messageId: 'message-1',
    utteranceId: 'stream-1',
    source: 'reply',
  })
  const summary = captureRigStateSummary(runtime, 0)
  assert.equal(summary.speaking, true)
  assert.equal(summary.owners.mouth, 'speech')
  assert.equal(summary.faceVisible, true)
  assert.ok(summary.capabilities.includes('head-body'))
  const keys = Object.keys(summary)
  assert.equal(keys.includes('angleX'), false)
  assert.equal(keys.includes('driver'), false)
  release()
})

test('missing special-expression layers are not advertised as capabilities', () => {
  assert.deepEqual(semanticRigCapabilities(null), [])
})

function cue(
  intent: 'think' | 'greet' | 'listen',
  atMs: number,
): {
  intent: 'think' | 'greet' | 'listen'
  atMs: number
  intensity: number
  tempo: number
  fadeInMs: number
  fadeOutMs: number
  interrupt: 'replace' | 'queue' | 'if-lower'
} {
  return {
    intent,
    atMs,
    intensity: 1,
    tempo: 1,
    fadeInMs: 150,
    fadeOutMs: 220,
    interrupt: atMs === 0 ? 'if-lower' : 'queue',
  }
}

test('acting names only the cue that is currently playing', () => {
  const runtime = new MotionRuntime(new RigMotionCoordinator())
  const release = runtime.retain()
  runtime.performance.handleForTest({
    phase: 'delivery',
    moodRevision: 1,
    motionStyle: 'even',
    plan: { cues: [cue('think', 0), cue('greet', 2_000)] },
  })
  const started = runtime.frame().performance?.startedAtMs ?? 0
  const notStarted = captureRigStateSummary(runtime, started - 20)
  assert.equal(notStarted.acting.intent, null)
  assert.ok(notStarted.acting.remainingMs > 0)
  const duringFace = captureRigStateSummary(runtime, started + 40)
  assert.equal(duringFace.acting.intent, 'think')
  assert.equal(duringFace.acting.function, 'prepareSpeech')
  assert.equal(duringFace.acting.lifecycle, 'preparing')
  assert.ok(duringFace.acting.remainingMs > 0)
  const between = captureRigStateSummary(runtime, started + 1_200)
  assert.equal(between.acting.intent, null)
  assert.ok(between.acting.remainingMs > 0)
  const duringBody = captureRigStateSummary(runtime, started + 2_040)
  assert.equal(duringBody.acting.intent, 'greet')
  const ended = captureRigStateSummary(runtime, started + 8_000)
  assert.equal(ended.acting.intent, null)
  assert.equal(ended.acting.remainingMs, 0)
  release()
})

test('speech accents share the agent-visible behavior lifecycle', () => {
  const runtime = new MotionRuntime(new RigMotionCoordinator())
  const release = runtime.retain()
  const startedAtMs = performance.now() + 100
  runtime.speech.handleForTest({
    phase: 'start',
    messageId: 'message-2',
    utteranceId: 'stream-2',
    source: 'reply',
  })
  runtime.speech.handleForTest({
    phase: 'prosody',
    messageId: 'message-2',
    utteranceId: 'stream-2',
    source: 'reply',
    prosody: {
      utteranceId: 'stream-2',
      startedAtMs,
      durationMs: 600,
      accents: [{ offsetMs: 100, intensity: 0.8 }],
    },
  })
  const preparing = captureRigStateSummary(runtime, startedAtMs + 50)
  assert.equal(preparing.acting.intent, null)
  assert.equal(preparing.acting.function, 'emphasize')
  assert.equal(preparing.acting.lifecycle, 'preparing')
  assert.equal(preparing.activeBehaviors[0]?.source, 'coSpeech')
  assert.deepEqual(preparing.activeBehaviors[0]?.resources, [
    'face.expression',
    'body.head',
    'body.torso',
  ])
  release()
})

test('empty capabilities stay empty after capture', () => {
  const runtime = new MotionRuntime(new RigMotionCoordinator())
  const release = runtime.retain()
  runtime.setCapabilities(['dizzy-eye', 'head-body'])
  runtime.setCapabilities([])
  const summary = captureRigStateSummary(runtime)
  assert.deepEqual(summary.capabilities, [])
  release()
})

test('a long utterance of accents cannot crowd the director out of the list', () => {
  const runtime = new MotionRuntime(new RigMotionCoordinator())
  const release = runtime.retain()
  const startedAtMs = performance.now() + 100
  runtime.speech.handleForTest({
    phase: 'start',
    messageId: 'message-3',
    utteranceId: 'stream-3',
    source: 'reply',
  })
  runtime.speech.handleForTest({
    phase: 'prosody',
    messageId: 'message-3',
    utteranceId: 'stream-3',
    source: 'reply',
    prosody: {
      utteranceId: 'stream-3',
      startedAtMs,
      durationMs: 6_000,
      accents: Array.from({ length: 12 }, (_, index) => ({
        offsetMs: 200 + index * 420,
        intensity: 0.8,
      })),
    },
  })
  runtime.performance.handleForTest({
    phase: 'delivery',
    moodRevision: 1,
    motionStyle: 'even',
    plan: { cues: [cue('greet', 0)] },
  })

  const summary = captureRigStateSummary(runtime, startedAtMs + 300)
  const sources = summary.activeBehaviors.map((behavior) => behavior.source)
  // Twelve prosody accents used to fill the eight slots by start time, so the
  // director saw seven identical `emphasize` rows and none of its own acting —
  // while being told not to repeat a function already in flight.
  assert.ok(
    sources.includes('performance'),
    `director acting missing from ${JSON.stringify(summary.activeBehaviors)}`,
  )
  const accents = summary.activeBehaviors.filter(
    (behavior) =>
      behavior.source === 'coSpeech' && behavior.function === 'emphasize',
  )
  assert.equal(accents.length, 1)
  assert.ok(summary.activeBehaviors.length <= 8)
  release()
})
