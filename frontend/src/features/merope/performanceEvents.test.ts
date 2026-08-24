import assert from 'node:assert/strict'
import test from 'node:test'
import {
  meropeStateEventDetail,
  meropePerformanceEventDetail,
  sanitizePerformanceDirective,
} from './performanceEvents'

test('bounds production performance events and defaults their source', () => {
  assert.deepEqual(
    meropePerformanceEventDetail({
      text: '  太好了！  ',
      source: 'unknown',
    }),
    { text: '太好了！', source: 'reply' },
  )
  assert.equal(meropePerformanceEventDetail({ text: '   ' }), null)
  assert.equal(meropePerformanceEventDetail(null), null)
})

test('bounds strict-Lite semantic plans and rejects raw intents', () => {
  const performance = sanitizePerformanceDirective({
    phase: 'reaction',
    moodRevision: 123.9,
    plan: {
      baseline: { expression: 'warm', posture: 'open', motionEnergy: 99, attention: -1 },
      cues: [
        { intent: 'delight', atMs: 9000, intensity: 9, tempo: 0.1, fadeInMs: 1, fadeOutMs: 9, interrupt: 'replace' },
        { intent: 'execute-code', atMs: 0, intensity: 1, tempo: 1, fadeInMs: 100, fadeOutMs: 100, interrupt: 'queue' },
      ],
    },
  })
  assert.equal(performance?.moodRevision, 123)
  assert.equal(performance?.plan.baseline?.motionEnergy, 1.4)
  assert.equal(performance?.plan.cues.length, 1)
  assert.equal(performance?.plan.cues[0]?.atMs, 5000)
})

test('accepts persisted Merope state transitions for immediate face sync', () => {
  const detail = meropeStateEventDetail({
    mood: { before: 70, after: 74, bandBefore: 'normal', bandAfter: 'normal', delta: 4, cause: 'user_praise', revision: 10 },
    activity: 'talking',
  })
  assert.equal(detail?.mood.after, 74)
  assert.equal(detail?.activity, 'talking')
})
