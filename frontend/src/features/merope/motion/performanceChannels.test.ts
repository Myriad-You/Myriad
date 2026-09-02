import type { PerformanceDirective } from '../../../services/agent/types'
import assert from 'node:assert/strict'
import test from 'node:test'
import {
  cueOccupiesGaze,
  cueOccupiesHeadBody,
  performanceOccupiedChannels,
} from './performanceChannels'

function directive(
  overrides: Partial<PerformanceDirective['plan']> = {},
): PerformanceDirective {
  return {
    phase: 'delivery',
    moodRevision: 1,
    motionStyle: 'even',
    plan: {
      baseline: {
        expression: 'warm',
        posture: 'neutral',
        motionEnergy: 1,
        attention: 1,
      },
      cues: [],
      ...overrides,
    },
  }
}

test('a baseline-only plan does not exclusive-claim expression', () => {
  assert.deepEqual(performanceOccupiedChannels(directive()), [])
})

test('head motion claims head/body for both think and greet', () => {
  assert.equal(
    cueOccupiesHeadBody({
      intent: 'think',
      atMs: 0,
      intensity: 1,
      tempo: 1,
      fadeInMs: 80,
      fadeOutMs: 120,
      interrupt: 'replace',
    }),
    true,
  )
  assert.equal(
    cueOccupiesHeadBody({
      intent: 'greet',
      atMs: 0,
      intensity: 1,
      tempo: 1,
      fadeInMs: 80,
      fadeOutMs: 120,
      interrupt: 'replace',
    }),
    true,
  )
})

test('a thinking head tilt temporarily owns the coarse head/body channel', () => {
  const channels = performanceOccupiedChannels(
    directive({
      cues: [
        {
          intent: 'think',
          atMs: 0,
          intensity: 1,
          tempo: 1,
          fadeInMs: 80,
          fadeOutMs: 120,
          interrupt: 'replace',
        },
      ],
    }),
  )
  // The resource declaration is precise (`body.head`), while the current
  // compatibility channel still groups head and torso together.
  assert.deepEqual(channels.sort(), ['expression', 'gaze', 'headBody'])
})

test('every stylized cue that moves the eye axes claims gaze', () => {
  const cue = (
    intent: PerformanceDirective['plan']['cues'][number]['intent'],
  ) =>
    cueOccupiesGaze({
      intent,
      atMs: 0,
      intensity: 1,
      tempo: 1,
      fadeInMs: 80,
      fadeOutMs: 120,
      interrupt: 'replace',
    })

  for (const intent of [
    'think',
    'speechless',
    'maniac',
    'lovestruck',
  ] as const) {
    assert.equal(cue(intent), true, intent)
  }
  for (const intent of ['angry', 'silly'] as const) {
    assert.equal(cue(intent), false, intent)
  }
})

test('posture and body cues claim head/body without taking the mouth', () => {
  const channels = performanceOccupiedChannels(
    directive({
      baseline: {
        expression: 'warm',
        posture: 'open',
        motionEnergy: 1,
        attention: 1,
      },
      cues: [
        {
          intent: 'greet',
          atMs: 0,
          intensity: 1,
          tempo: 1,
          fadeInMs: 80,
          fadeOutMs: 120,
          interrupt: 'replace',
        },
      ],
    }),
  )
  assert.ok(channels.includes('expression'))
  assert.ok(channels.includes('headBody'))
  assert.equal(channels.includes('mouth'), false)
})
