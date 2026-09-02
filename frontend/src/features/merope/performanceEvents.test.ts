import assert from 'node:assert/strict'
import test from 'node:test'
import {
  meropePerformanceEventDetail,
  meropeStateEventDetail,
  sanitizePerformanceDirective,
} from './performanceEvents'

test('bounds production performance events and defaults their source', () => {
  assert.deepEqual(
    meropePerformanceEventDetail({
      text: '  太好了！  ',
      source: 'unknown',
      messageId: ' message-1 ',
    }),
    { text: '太好了！', source: 'reply', messageId: 'message-1' },
  )
  assert.equal(meropePerformanceEventDetail({ text: '   ' }), null)
  assert.equal(meropePerformanceEventDetail(null), null)
})

test('bounds strict-Lite semantic plans and rejects raw intents', () => {
  const performance = sanitizePerformanceDirective({
    phase: 'reaction',
    moodRevision: 123.9,
    motionStyle: 'open',
    plan: {
      baseline: {
        expression: 'warm',
        posture: 'open',
        motionEnergy: 99,
        attention: -1,
      },
      cues: [
        {
          intent: 'delight',
          atMs: 9000,
          intensity: 9,
          tempo: 0.1,
          fadeInMs: 1,
          fadeOutMs: 9,
          interrupt: 'replace',
        },
        {
          intent: 'think',
          atMs: 80,
          intensity: 0.9,
          tempo: 0.8,
          fadeInMs: 140,
          fadeOutMs: 300,
          interrupt: 'queue',
        },
        {
          intent: 'dizzy',
          atMs: 100,
          intensity: 1,
          tempo: 1,
          fadeInMs: 120,
          fadeOutMs: 260,
          interrupt: 'if-lower',
        },
        {
          intent: 'execute-code',
          atMs: 0,
          intensity: 1,
          tempo: 1,
          fadeInMs: 100,
          fadeOutMs: 100,
          interrupt: 'queue',
        },
      ],
    },
  })
  assert.equal(performance?.moodRevision, 123)
  assert.equal(performance?.motionStyle, 'open')
  assert.equal(performance?.plan.baseline?.motionEnergy, 1.4)
  assert.equal(performance?.plan.cues.length, 3)
  assert.equal(performance?.plan.cues[0]?.atMs, 5000)
  assert.equal(performance?.plan.cues[1]?.intent, 'think')
  assert.equal(performance?.plan.cues[2]?.intent, 'dizzy')
})

test('rejects directives without a current motion style', () => {
  const plan = {
    phase: 'reaction',
    moodRevision: 1,
    plan: {
      cues: [
        {
          intent: 'respond',
          atMs: 0,
          intensity: 1,
          tempo: 1,
          fadeInMs: 100,
          fadeOutMs: 200,
          interrupt: 'replace',
        },
      ],
    },
  }
  assert.equal(sanitizePerformanceDirective(plan), null)
  assert.equal(
    sanitizePerformanceDirective({ ...plan, motionStyle: 'legacy-vivid' }),
    null,
  )
})

test('accepts persisted Merope state transitions for immediate face sync', () => {
  const detail = meropeStateEventDetail({
    mood: {
      before: 70,
      after: 74,
      bandBefore: 'normal',
      bandAfter: 'normal',
      delta: 4,
      cause: 'user_praise',
      revision: 10,
    },
    activity: 'talking',
  })
  assert.equal(detail?.mood.after, 74)
  assert.equal(detail?.mood.bandBefore, 'calm')
  assert.equal(detail?.mood.bandAfter, 'calm')
  assert.equal(detail?.activity, 'talking')
})

test('accepts circumplex mood bands and optional arousal', () => {
  const detail = meropeStateEventDetail({
    mood: {
      before: 30,
      after: 30,
      arousalBefore: 40,
      arousalAfter: 70,
      bandBefore: 'sad',
      bandAfter: 'tense',
      delta: 0,
      cause: 'user_scold',
      revision: 11,
    },
    activity: 'idle',
  })
  assert.equal(detail?.mood.bandBefore, 'sad')
  assert.equal(detail?.mood.bandAfter, 'tense')
  assert.equal(detail?.mood.arousalBefore, 40)
  assert.equal(detail?.mood.arousalAfter, 70)
})

test('accepts crying only as a bounded semantic performance cue', () => {
  const performance = sanitizePerformanceDirective({
    phase: 'delivery',
    moodRevision: 2,
    motionStyle: 'even',
    plan: {
      cues: [
        {
          intent: 'cry',
          atMs: 120,
          intensity: 1.8,
          tempo: 0.7,
          fadeInMs: 260,
          fadeOutMs: 500,
          interrupt: 'replace',
        },
      ],
    },
  })
  assert.equal(performance?.plan.cues[0]?.intent, 'cry')
  assert.equal(performance?.plan.cues[0]?.intensity, 1.4)
})

test('accepts stylized semantic performance cues', () => {
  for (const intent of [
    'angry',
    'speechless',
    'maniac',
    'silly',
    'lovestruck',
  ] as const) {
    const performance = sanitizePerformanceDirective({
      phase: 'reaction',
      moodRevision: 3,
      motionStyle: 'restrained',
      plan: {
        cues: [
          {
            intent,
            atMs: 0,
            intensity: 1,
            tempo: 1,
            fadeInMs: 120,
            fadeOutMs: 260,
            interrupt: 'replace',
          },
        ],
      },
    })
    assert.equal(performance?.plan.cues[0]?.intent, intent)
  }
})
