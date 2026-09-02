import type { PerformanceDirective } from '../../../services/agent/types'
import type { BehaviorPlan, ScheduledBehavior } from './behavior'
import assert from 'node:assert/strict'
import test from 'node:test'
import {
  HumanPerformanceRuntime,
  mergeBehaviorPlans,
} from './humanPerformanceRuntime'
import { compilePerformanceBehaviorPlan } from './performanceBehaviorPlan'

function plan(
  id: string,
  behavior: Partial<ScheduledBehavior> = {},
): BehaviorPlan {
  const prefix = `${id}:behavior`
  return {
    id,
    originMs: 100,
    pegs: [
      { id: `${prefix}:start`, atMs: 100, revision: 0 },
      { id: `${prefix}:ready`, atMs: 140, revision: 0 },
      { id: `${prefix}:stroke-start`, atMs: 180, revision: 0 },
      { id: `${prefix}:stroke-peak`, atMs: 220, revision: 0 },
      { id: `${prefix}:stroke-end`, atMs: 280, revision: 0 },
      { id: `${prefix}:relax`, atMs: 620, revision: 0 },
      { id: `${prefix}:end`, atMs: 800, revision: 0 },
    ],
    behaviors: [
      {
        id: prefix,
        function: 'emphasize',
        kind: 'oneShot',
        source: 'performance',
        resources: ['body.head', 'body.torso'],
        channels: ['headBody'],
        timing: {
          start: `${prefix}:start`,
          ready: `${prefix}:ready`,
          strokeStart: `${prefix}:stroke-start`,
          strokePeak: `${prefix}:stroke-peak`,
          strokeEnd: `${prefix}:stroke-end`,
          relax: `${prefix}:relax`,
          end: `${prefix}:end`,
        },
        form: { family: 'performance-cue', id: 'emphasize' },
        intensity: 1,
        quality: { extent: 1, power: 1 },
        ...behavior,
      },
    ],
  }
}

test('merges speech, performance and music into one scheduler plan', () => {
  const runtime = new HumanPerformanceRuntime()
  const speech = plan('speech', {
    source: 'coSpeech',
    form: { family: 'co-speech', id: 'accent' },
  })
  const music = plan('music', {
    source: 'music',
    form: { family: 'music', id: 'groove' },
  })
  const first = runtime.frame([speech, music], 120)
  assert.equal(first.plan?.id, 'human-performance')
  assert.deepEqual(first.behaviors.map((behavior) => behavior.source).sort(), [
    'coSpeech',
    'music',
  ])

  const second = runtime.frame([music], 300)
  assert.deepEqual(
    second.plan?.behaviors.map((behavior) => behavior.source),
    ['music'],
  )
  assert.ok(
    second.behaviors.some(
      (behavior) =>
        behavior.source === 'coSpeech' && behavior.phase === 'recovering',
    ),
  )
})

test('anticipator revisions update lifecycle without replaying the body plan', () => {
  const runtime = new HumanPerformanceRuntime()
  const firstPlan = plan('music', {
    kind: 'rhythmic',
    source: 'music',
    anticipation: 'music:next-beat',
    form: { family: 'music', id: 'groove' },
    timing: {
      start: 'music:behavior:start',
      ready: 'music:behavior:ready',
      strokeStart: 'music:behavior:stroke-start',
      strokePeak: 'music:behavior:stroke-peak',
      strokeEnd: 'music:behavior:stroke-end',
      relax: null,
      end: null,
    },
  })
  firstPlan.pegs = [
    ...firstPlan.pegs.slice(0, 5),
    { id: 'music:next-beat', atMs: 600, revision: 0, confidence: 0.4 },
  ]
  const first = runtime.frame([firstPlan], 100)
  const revised = structuredClone(firstPlan)
  revised.pegs = revised.pegs.map((peg) =>
    peg.id === 'music:next-beat'
      ? { ...peg, atMs: 720, revision: 1, confidence: 0.9 }
      : peg,
  )
  const second = runtime.frame([revised], 140)
  assert.equal(second.revision, first.revision)
  assert.equal(second.behaviors[0]?.anticipatedAtMs, 720)
  assert.equal(second.behaviors[0]?.anticipationConfidence, 0.9)
})

test('motion style changes quality at the single merge boundary', () => {
  const restrained = mergeBehaviorPlans([plan('cue')], 100, 'restrained')
  const open = mergeBehaviorPlans([plan('cue')], 100, 'open')
  assert.ok(
    (open?.behaviors[0]?.quality?.extent ?? 0) >
      (restrained?.behaviors[0]?.quality?.extent ?? 0),
  )
  assert.ok(
    (open?.behaviors[0]?.quality?.power ?? 0) >
      (restrained?.behaviors[0]?.quality?.power ?? 0),
  )
})

test('a refinement restating a live beat retimes it instead of restarting it', () => {
  const directive = (atMs: number, intensity: number): PerformanceDirective => ({
    phase: 'delivery',
    moodRevision: 1,
    motionStyle: 'even',
    plan: {
      cues: [
        {
          intent: 'emphasize',
          atMs,
          intensity,
          tempo: 1,
          fadeInMs: 120,
          fadeOutMs: 200,
          interrupt: 'replace',
        },
      ],
    },
  })
  const runtime = new HumanPerformanceRuntime()
  const floor = compilePerformanceBehaviorPlan(directive(0, 0.9), 1_000, 'performance')
  runtime.frame([floor], 1_000)
  const live = runtime.frame([floor], 1_400).behaviors[0]
  assert.ok(live)
  assert.equal(live.phase, 'holding')

  // Lite lands late and picks the same beat. The floor is mid-hold, so its
  // committed pegs are locked and the behavior must carry on under the new
  // spec rather than recover and replay.
  const refinement = compilePerformanceBehaviorPlan(
    directive(0, 1.3),
    1_400,
    'performance',
  )
  const after = runtime.frame([refinement], 1_400).behaviors
  assert.equal(after.length, 1)
  assert.equal(after[0]?.id, live.id)
  assert.equal(after[0]?.phase, 'holding')
  assert.equal(after[0]?.startedAtMs, live.startedAtMs)
})

test('a refinement that chose a different beat recovers the old one', () => {
  const cue = (intent: 'emphasize' | 'delight'): PerformanceDirective => ({
    phase: 'delivery',
    moodRevision: 1,
    motionStyle: 'even',
    plan: {
      cues: [
        {
          intent,
          atMs: 0,
          intensity: 1,
          tempo: 1,
          fadeInMs: 120,
          fadeOutMs: 200,
          interrupt: 'replace',
        },
      ],
    },
  })
  const runtime = new HumanPerformanceRuntime()
  runtime.frame(
    [compilePerformanceBehaviorPlan(cue('emphasize'), 0, 'performance')],
    0,
  )
  const live = runtime.frame(
    [compilePerformanceBehaviorPlan(cue('emphasize'), 0, 'performance')],
    400,
  ).behaviors[0]
  assert.ok(live)
  const after = runtime.frame(
    [compilePerformanceBehaviorPlan(cue('delight'), 400, 'performance')],
    400,
  ).behaviors
  const previous = after.find((behavior) => behavior.id === live.id)
  assert.equal(previous?.phase, 'recovering')
  assert.ok(after.some((behavior) => behavior.form.id === 'delight'))
})
